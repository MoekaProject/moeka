mod app_settings;
mod assets;
mod attachments;
mod auto_save;
mod branch_picker;
mod caret;
mod chat_panel;
mod code_commands;
mod command_palette;
pub(crate) mod config_paths;
mod config_store;
mod element;
mod file_list;
pub(crate) mod file_ops;
mod fold;
mod git_ops;
mod git_panel;
mod handlers;
mod helpers;
mod history_store;
mod image_view;
mod info_panel;
mod inline_diff;
mod input;
mod input_entity;
mod input_ime;
mod input_table;
mod input_wrap;
mod language_detection;
mod language_picker;
mod list_viewport;
mod menus;
mod notifications;
mod outline;
mod pane;
mod profiler;
mod quick_open;
mod render;
mod render_pane;
mod scaling;
mod scrollbar;
mod search;
mod search_index;
mod search_menu;
mod session_store;
pub(crate) mod settings_panel;
mod shortcut;
mod sidebar_files;
mod sidebar_nav;
mod sidebar_ops;
mod sidebar_render;
mod sidebar_state;
mod sidebar_tree;
mod sidebar_ui;
mod status_bar;
mod tabs;
mod temp_store;
mod theme_debugger;
mod tooltip;
mod types;
mod updater;
mod whiteboard;
mod whiteboard_element;
mod whiteboard_export;
mod whiteboard_geometry;
mod whiteboard_input;
mod whiteboard_panel;
mod workspace_search;

pub(crate) use app_settings::{AppSettings, MermaidTheme, theme_kind_to_string};
pub(crate) use assets::*;
pub(crate) use attachments::AttachmentPolicy;
pub(crate) use branch_picker::BranchPickerItem;
pub(crate) use file_list::FileListItem;
pub(crate) use git_ops::*;
use language_detection::classify_pasted_text_kind;
pub(crate) use outline::MarkdownOutlineIndex;
pub(crate) use pane::{
    Pane, PaneId, PaneLayout, PaneResizeDrag, PaneState, SplitAxis, SplitDirection,
};
pub(crate) use scrollbar::{PanelScrollbarKind, PanelScrollbarState};
pub(crate) use search_menu::{SearchMenuConfig, SearchMenuRow};
pub(crate) use shortcut::{register_keybinding_overrides, set_application_menus};
pub(crate) use status_bar::IndentSettings;
pub(crate) use types::*;
pub(crate) use updater::{APP_VERSION, PreparedUpdate, UpdateState};
pub(crate) use whiteboard::*;

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::Cursor as IoCursor;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use chrono::Local;
use gpui::{
    Bounds, ClipboardEntry, ClipboardItem, Context, FocusHandle, Focusable, Pixels, Point, Render,
    RenderImage, ScrollHandle, SharedString, Task, WrappedLine,
};

use crate::actions::*;
use crate::buffer::Buffer;
use crate::cursor::Cursor;
use crate::i18n::{TextKey, UiLanguage};
use crate::markdown::{self, BlockType, LineDecoration};
use crate::theme::{EditorThemeMode, Theme, ThemeColorField, ThemeKind};

const CODE_BLOCK_COPY_FEEDBACK_MS: u64 = 1000;
const PANEL_ANIMATION_FRAME_MS: u64 = 8;
/// Maximum gap between consecutive single-character edits for them to be merged
/// into one undo step (time-interval undo grouping).
const UNDO_GROUP_INTERVAL: Duration = Duration::from_millis(300);
const SIDEBAR_GIT_STATUS_REFRESH_MIN_INTERVAL_MS: u64 = 350;
const GIT_HISTORY_REFRESH_MIN_INTERVAL_MS: u64 = 350;
const GENERATED_PREVIEW_RENDER_BUDGET: usize = 24;
const GENERATED_PREVIEW_CACHE_RETAIN: usize = 96;
/// Entry count alone says nothing about cost: one full-width retina diagram can
/// outweigh dozens of small ones, so cap the decoded bytes as well.
const GENERATED_PREVIEW_CACHE_BYTE_BUDGET: usize = 64 * 1024 * 1024;

#[derive(Default)]
pub(crate) struct GeneratedPreviewTabCache {
    keys: HashSet<String>,
    images: HashMap<String, Option<Arc<RenderImage>>>,
    svgs: HashMap<String, Arc<str>>,
    overlays: HashMap<String, (u16, Arc<RenderImage>)>,
    views: HashMap<String, ImageViewState>,
    code_mode_preview_key: Option<String>,
    rerender_pending: bool,
}
const LINE_DISPLAY_CACHE_TRIM_MIN_LINES: usize = 20_000;
const LINE_DISPLAY_CACHE_MAX_CACHED_LINES: usize = 2_000;
const LINE_DISPLAY_CACHE_VISIBLE_MARGIN_LINES: usize = 600;
const LINE_DISPLAY_CACHE_CURSOR_MARGIN_LINES: usize = 200;
const LINE_DISPLAY_CACHE_TRIM_UPDATE_INTERVAL: usize = 64;
const SPARSE_LINE_GEOMETRY_BLOCK_LINES: usize = 256;
pub(super) const WELCOME_TAB_ID: &str = "welcome-1";

fn generated_preview_intersects_range(
    preview_start: usize,
    preview_end: usize,
    visible_start: usize,
    visible_end: usize,
) -> bool {
    preview_start <= visible_end && preview_end >= visible_start
}

#[derive(Clone, Copy, Default)]
struct GitMetadataImpact {
    affects_sidebar_status: bool,
    affects_history: bool,
    affects_working_tree_diff: bool,
}

impl GitMetadataImpact {
    fn any(self) -> bool {
        self.affects_sidebar_status || self.affects_history || self.affects_working_tree_diff
    }

    fn merge(&mut self, other: Self) {
        self.affects_sidebar_status |= other.affects_sidebar_status;
        self.affects_history |= other.affects_history;
        self.affects_working_tree_diff |= other.affects_working_tree_diff;
    }
}

fn ease_in_out_cubic(t: f32) -> f32 {
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

fn text_tab_is_dirty(saved_text: Option<&str>, current_text: &str) -> bool {
    match saved_text {
        Some(saved_text) => saved_text != current_text,
        None => !current_text.is_empty(),
    }
}

/// Finds the nesting-aware matching bracket for the ASCII bracket located at
/// `bracket_offset` (a byte offset). Brackets are ASCII so byte scanning is safe
/// even for multi-byte UTF-8 text. Returns the partner's byte offset.
fn matching_bracket_offset(text: &str, bracket_offset: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let ch = *bytes.get(bracket_offset)?;
    let (open, close, forward) = match ch {
        b'(' => (b'(', b')', true),
        b'[' => (b'[', b']', true),
        b'{' => (b'{', b'}', true),
        b')' => (b'(', b')', false),
        b']' => (b'[', b']', false),
        b'}' => (b'{', b'}', false),
        _ => return None,
    };

    let mut depth: i32 = 0;
    if forward {
        let mut i = bracket_offset;
        while i < bytes.len() {
            let b = bytes[i];
            if b == open {
                depth += 1;
            } else if b == close {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            i += 1;
        }
    } else {
        let mut i = bracket_offset as isize;
        while i >= 0 {
            let b = bytes[i as usize];
            if b == close {
                depth += 1;
            } else if b == open {
                depth -= 1;
                if depth == 0 {
                    return Some(i as usize);
                }
            }
            i -= 1;
        }
    }
    None
}

fn remaining_refresh_delay(scanned_at: Option<SystemTime>, minimum_interval_ms: u64) -> Duration {
    let Some(scanned_at) = scanned_at else {
        return Duration::ZERO;
    };
    let minimum_interval = Duration::from_millis(minimum_interval_ms);
    let Ok(elapsed) = scanned_at.elapsed() else {
        return Duration::ZERO;
    };
    minimum_interval.saturating_sub(elapsed)
}

fn timestamp_file_stem() -> String {
    Local::now().format("%Y%m%d%H%M%S").to_string()
}

fn image_extension(format: gpui::ImageFormat) -> &'static str {
    match format {
        gpui::ImageFormat::Png => "png",
        gpui::ImageFormat::Jpeg => "jpg",
        gpui::ImageFormat::Webp => "webp",
        gpui::ImageFormat::Gif => "gif",
        gpui::ImageFormat::Svg => "svg",
        gpui::ImageFormat::Bmp => "bmp",
        gpui::ImageFormat::Tiff => "tiff",
    }
}

fn normalize_clipboard_image(image: &gpui::Image) -> gpui::Image {
    if image.format == gpui::ImageFormat::Png {
        return image.clone();
    }

    let Ok(decoded) = image::load_from_memory(&image.bytes) else {
        return image.clone();
    };

    let mut cursor = IoCursor::new(Vec::new());
    if decoded
        .write_to(&mut cursor, image::ImageFormat::Png)
        .is_err()
    {
        return image.clone();
    }

    gpui::Image::from_bytes(gpui::ImageFormat::Png, cursor.into_inner())
}

fn is_image_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "gif" | "svg" | "bmp" | "tif" | "tiff"
            )
        })
}

fn unique_import_path(directory: &Path, stem: &str, extension: Option<&str>) -> PathBuf {
    let normalized_extension = extension
        .map(str::trim)
        .filter(|extension| !extension.is_empty())
        .map(|extension| extension.trim_start_matches('.'));
    let mut suffix = 0usize;

    loop {
        let file_name = match (suffix, normalized_extension) {
            (0, Some(extension)) => format!("{stem}.{extension}"),
            (0, None) => stem.to_string(),
            (_, Some(extension)) => format!("{stem}-{suffix}.{extension}"),
            (_, None) => format!("{stem}-{suffix}"),
        };
        let candidate = directory.join(&file_name);
        if !candidate.exists() {
            return candidate;
        }
        suffix += 1;
    }
}

fn parse_file_urls_from_text(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(crate::platform::paths::path_from_file_url)
        .collect()
}

// ── Layout constants ────────────────────────────────────────────────
// NOTE: Numeric layout/style constants are now in style.rs (Styles struct).
// Only non-style constants remain here.

// ── Cursor blink constants ──────────────────────────────────────────
// NOTE: Cursor timing is now in style.rs (CursorStyles).

// ── Editor element constants ────────────────────────────────────────
// NOTE: Element layout constants are now in style.rs.

// ── UI constants ────────────────────────────────────────────────────
// NOTE: UI dimension constants are now in style.rs.

#[derive(Clone)]
pub(crate) struct ContextMenu {
    pub position: Point<Pixels>,
    pub kind: ContextMenuKind,
}

#[derive(Clone, Copy)]
pub(crate) struct ContextSubmenuAim {
    pub origin: Point<Pixels>,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum ContextMenuKind {
    Sidebar {
        path: PathBuf,
        is_dir: bool,
    },
    SidebarStarred {
        path: PathBuf,
    },
    FileList {
        path: PathBuf,
    },
    SidebarSort,
    SidebarCreate,
    Format,
    FormatHeading,
    FormatMore,
    Export {
        path: PathBuf,
    },
    WikiLink,
    Tab {
        tab_id: String,
    },
    Workspace,
    Image,
    Whiteboard,
    Table {
        table_start_line: usize,
        row: usize,
        col: usize,
    },
    Terminal {
        tab_id: String,
    },
    TerminalTabCreate,
}

#[derive(Clone)]
pub(crate) enum TableDragState {
    DraggingColumn {
        table_start_line: usize,
        source_col: usize,
        current_x: f32,
    },
    DraggingRow {
        table_start_line: usize,
        source_row: usize,
        current_y: f32,
    },
}

#[derive(Clone, Copy)]
pub(crate) struct EditorScrollbarDragState {
    pub start_position_y: f32,
    pub start_thumb_top: f32,
    pub track_travel: f32,
    pub max_scroll: f32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SidebarSortMode {
    CreatedDesc,
    CreatedAsc,
    NameAsc,
    NameDesc,
    Type,
}

impl SidebarSortMode {
    pub(crate) fn label_key(self) -> crate::i18n::TextKey {
        match self {
            Self::CreatedDesc => crate::i18n::TextKey::SortCreatedDesc,
            Self::CreatedAsc => crate::i18n::TextKey::SortCreatedAsc,
            Self::NameAsc => crate::i18n::TextKey::SortNameAsc,
            Self::NameDesc => crate::i18n::TextKey::SortNameDesc,
            Self::Type => crate::i18n::TextKey::SortType,
        }
    }
}

/// Whether FileList items show the extracted document title or the file name.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum FileListDisplayMode {
    Title,
    FileName,
}

impl FileListDisplayMode {
    pub(crate) fn label_key(self) -> crate::i18n::TextKey {
        match self {
            Self::Title => crate::i18n::TextKey::FileListShowTitle,
            Self::FileName => crate::i18n::TextKey::FileListShowFileName,
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum FormatMenuAction {
    Cut,
    Copy,
    Paste,
    Heading1,
    Heading2,
    Heading3,
    Heading4,
    Heading5,
    Heading6,
    Paragraph,
    EditImageDescription,
    EditImageUrl,
    DeleteImage,
    SelectImageFile,
    Bold,
    Italic,
    Strikethrough,
    Underline,
    InlineCode,
    Highlight,
    CodeBlock,
    HorizontalRule,
    Quote,
    UnorderedList,
    OrderedList,
    Todo,
    Link,
    WikiLink,
    Image,
    Table,
    DateTime,
    Superscript,
    Subscript,
    Footnote,
    CalloutTip,
}

#[derive(Clone)]
pub(crate) enum SidebarEditMode {
    Rename {
        path: PathBuf,
        editor: gpui::Entity<crate::text_input::TextInput>,
    },
    RenameStarred {
        path: PathBuf,
        editor: gpui::Entity<crate::text_input::TextInput>,
    },
    CreateFile {
        parent: PathBuf,
        editor: gpui::Entity<crate::text_input::TextInput>,
    },
    CreateDir {
        parent: PathBuf,
        editor: gpui::Entity<crate::text_input::TextInput>,
    },
}

impl SidebarEditMode {
    pub fn editor(&self) -> &gpui::Entity<crate::text_input::TextInput> {
        match self {
            Self::Rename { editor, .. }
            | Self::RenameStarred { editor, .. }
            | Self::CreateFile { editor, .. }
            | Self::CreateDir { editor, .. } => editor,
        }
    }

    pub fn text(&self, cx: &gpui::App) -> String {
        self.editor().read(cx).text().to_string()
    }
}

#[derive(Clone)]
pub(crate) struct FileClipboard {
    pub paths: Vec<PathBuf>,
    pub is_cut: bool,
}

#[derive(Clone)]
pub(crate) struct MultiCursorClipboard {
    pub serialized_text: String,
    pub segments: Vec<String>,
}

// ── Sidebar drag-and-drop types ─────────────────────────────────────

#[derive(Clone)]
pub(crate) struct DraggedSidebarItems {
    pub paths: Vec<PathBuf>,
}

pub(crate) struct DraggedSidebarItemsView {
    pub label: String,
    pub count: usize,
    pub font_size_px: f32,
    pub background_color: gpui::Hsla,
    pub text_color: gpui::Hsla,
    pub cursor_offset: gpui::Point<gpui::Pixels>,
}

impl Render for DraggedSidebarItemsView {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        _cx: &mut Context<Self>,
    ) -> impl gpui::IntoElement {
        use gpui::{div, prelude::*, px};
        let text = if self.count > 1 {
            format!("{} (+{})", self.label, self.count - 1)
        } else {
            self.label.clone()
        };
        div()
            .pl(self.cursor_offset.x + px(16.0))
            .pt(self.cursor_offset.y + px(16.0))
            .child(
                div()
                    .px(px(8.0))
                    .py(px(4.0))
                    .bg(self.background_color)
                    .text_color(self.text_color)
                    .text_size(px(self.font_size_px))
                    .rounded(px(4.0))
                    .shadow_md()
                    .child(text),
            )
    }
}

// ── Tab drag-and-drop types ─────────────────────────────────────────

#[derive(Clone)]
pub(crate) struct WorkspaceSearchMatch {
    pub line_number: usize,
    pub line_text: String,
    pub match_start: usize,
    pub match_end: usize,
    pub byte_offset: usize,
    pub byte_end: usize,
}

#[derive(Clone, Copy)]
pub(crate) struct WorkspaceSearchResume {
    pub next_line: usize,
    pub next_byte_offset: usize,
}

#[derive(Clone)]
pub(crate) struct WorkspaceSearchResult {
    pub path: PathBuf,
    pub matches: Vec<WorkspaceSearchMatch>,
    /// Set when the file was longer than the per-file scan budget.
    pub resume: Option<WorkspaceSearchResume>,
}

#[derive(Clone)]
pub(crate) struct QuickOpenResult {
    pub path: PathBuf,
    pub score: i64,
    pub file_name_match_indices: Vec<usize>,
    pub path_match_indices: Vec<usize>,
}

#[derive(Clone)]
pub(crate) struct WorkspaceFileCandidate {
    pub path: PathBuf,
    pub relative_path: String,
    pub file_name: String,
}

#[derive(Clone)]
pub(crate) struct DraggedTab {
    pub tab_id: String,
    pub title: String,
    pub source_index: usize,
    pub source_pane_id: PaneId,
}

pub(crate) struct DraggedTabView {
    pub title: String,
    pub font_size_px: f32,
    pub background_color: gpui::Hsla,
    pub text_color: gpui::Hsla,
    pub cursor_offset: gpui::Point<gpui::Pixels>,
}

impl Render for DraggedTabView {
    fn render(
        &mut self,
        _window: &mut gpui::Window,
        _cx: &mut Context<Self>,
    ) -> impl gpui::IntoElement {
        use gpui::{div, prelude::*, px};
        div()
            .pl(self.cursor_offset.x + px(16.0))
            .pt(self.cursor_offset.y + px(16.0))
            .child(
                div()
                    .px(px(10.0))
                    .py(px(4.0))
                    .bg(self.background_color)
                    .text_color(self.text_color)
                    .text_size(px(self.font_size_px))
                    .rounded(px(4.0))
                    .shadow_md()
                    .opacity(0.85)
                    .child(self.title.clone()),
            )
    }
}

#[derive(Clone)]
#[allow(dead_code)]
pub(crate) enum DialogAction {
    CloseTabUnsaved {
        tab_id: String,
    },
    CloseRunningTerminal {
        tab_id: String,
    },
    SaveAndCloseTab {
        tab_id: String,
    },
    DeletePath {
        path: PathBuf,
    },
    DeletePaths {
        paths: Vec<PathBuf>,
    },
    InstallUpdate {
        update: PreparedUpdate,
    },
    GitDiscardEntries {
        tracked: Vec<String>,
        untracked: Vec<String>,
    },
    FormatWholeDocument,
    DismissOnly,
}

#[derive(Clone)]
pub(crate) struct Dialog {
    pub title: String,
    pub message: String,
    pub save_label: Option<String>,
    pub confirm_label: String,
    pub cancel_label: String,
    #[allow(dead_code)]
    pub is_danger: bool,
    pub action: DialogAction,
}

enum ClipboardPasteOutcome {
    NotHandled,
    Insert(String),
    Consumed,
}

/// Display info for a single buffer line.
#[derive(Clone)]
pub(crate) struct LineDisplayInfo {
    pub display_text: String,
    pub display_to_buffer: Vec<usize>,
    pub buffer_to_display: Vec<usize>,
    pub base_runs: Vec<gpui::TextRun>,
    pub font_size: f32,
    pub line_height: f32,
    pub visual_line_count: usize,
    pub wrapped_line: WrappedLine,
    pub wrap_width_px: Option<f32>,
    pub unwrapped_width_px: f32,
    pub wrap_boundary_upstream_xs: HashMap<usize, f32>,
    pub extra_height: f32,
    pub indent_px: f32,
    pub content_x: f32,
    pub selection_padding_left_px: f32,
}

pub(crate) type LineDisplayCache = Vec<Option<Box<LineDisplayInfo>>>;

const LARGE_MARKDOWN_MIN_LINES: usize = 100_000;
const LARGE_MARKDOWN_CHUNK_LINES: usize = 128;
const LARGE_MARKDOWN_BOUNDARY_SCAN_LINES: usize = 512;
const LARGE_DOCUMENT_FRAME_BENCH_WARMUP: usize = 60;
const LARGE_DOCUMENT_FRAME_BENCH_SCROLL_FRAMES: usize = 240;
const LARGE_DOCUMENT_FRAME_BENCH_EDIT_FRAMES: usize = 120;

pub(crate) struct LargeDocumentFrameBench {
    output_path: PathBuf,
    frame: usize,
    last_frame: Option<Instant>,
    scroll_frame_intervals: Vec<Duration>,
    edit_frame_intervals: Vec<Duration>,
    frame_work_times: Vec<Duration>,
    edit_times: Vec<Duration>,
    character_edit_times: Vec<Duration>,
    newline_edit_times: Vec<Duration>,
    newline_buffer_times: Vec<Duration>,
    newline_reparse_times: Vec<Duration>,
    newline_history_times: Vec<Duration>,
    selection_times: Vec<Duration>,
    scrollbar_jump_times: Vec<Duration>,
    history_times: Vec<Duration>,
    frame_work_started: Option<Instant>,
    pending_edit_offset: Option<usize>,
    pending_newline_offset: Option<usize>,
    syntax_view_samples: usize,
    syntax_view_misses: usize,
}

impl LargeDocumentFrameBench {
    fn from_env() -> Option<Self> {
        std::env::var_os("MOEKA_LARGE_DOCUMENT_BENCH").map(|path| Self {
            output_path: PathBuf::from(path),
            frame: 0,
            last_frame: None,
            scroll_frame_intervals: Vec::new(),
            edit_frame_intervals: Vec::new(),
            frame_work_times: Vec::new(),
            edit_times: Vec::new(),
            character_edit_times: Vec::new(),
            newline_edit_times: Vec::new(),
            newline_buffer_times: Vec::new(),
            newline_reparse_times: Vec::new(),
            newline_history_times: Vec::new(),
            selection_times: Vec::new(),
            scrollbar_jump_times: Vec::new(),
            history_times: Vec::new(),
            frame_work_started: None,
            pending_edit_offset: None,
            pending_newline_offset: None,
            syntax_view_samples: 0,
            syntax_view_misses: 0,
        })
    }
}

#[derive(Clone, Default)]
pub(crate) struct LazyMarkdownDecorations {
    parsed_chunks: Vec<bool>,
}

impl LazyMarkdownDecorations {
    fn new(line_count: usize) -> Self {
        Self {
            parsed_chunks: vec![false; line_count.div_ceil(LARGE_MARKDOWN_CHUNK_LINES)],
        }
    }

    fn is_parsed(&self, chunk: usize) -> bool {
        self.parsed_chunks.get(chunk).copied().unwrap_or(false)
    }

    fn mark_parsed(&mut self, chunk: usize) {
        if let Some(parsed) = self.parsed_chunks.get_mut(chunk) {
            *parsed = true;
        }
    }

    fn invalidate_range(&mut self, range: Range<usize>) {
        if range.is_empty() || self.parsed_chunks.is_empty() {
            return;
        }
        let first = range.start / LARGE_MARKDOWN_CHUNK_LINES;
        let last = range.end.saturating_sub(1) / LARGE_MARKDOWN_CHUNK_LINES;
        for chunk in first..=last.min(self.parsed_chunks.len().saturating_sub(1)) {
            self.parsed_chunks[chunk] = false;
        }
    }

    fn resize_after_structural_edit(&mut self, line_count: usize, start_line: usize) {
        let chunk_count = line_count.div_ceil(LARGE_MARKDOWN_CHUNK_LINES);
        self.parsed_chunks.resize(chunk_count, false);
        let first_invalid = start_line / LARGE_MARKDOWN_CHUNK_LINES;
        for parsed in self.parsed_chunks.iter_mut().skip(first_invalid) {
            *parsed = false;
        }
    }
}

pub(crate) fn new_lazy_markdown_decorations(
    line_count: usize,
) -> (Vec<LineDecoration>, LazyMarkdownDecorations) {
    let line_count = line_count.max(1);
    (
        (0..line_count).map(|_| LineDecoration::default()).collect(),
        LazyMarkdownDecorations::new(line_count),
    )
}

pub(crate) fn parse_lazy_markdown_range(
    buffer: &Buffer,
    decorations: &mut [LineDecoration],
    lazy: &mut LazyMarkdownDecorations,
    range: Range<usize>,
) -> bool {
    if buffer.line_count() == 0 || lazy.parsed_chunks.is_empty() {
        return false;
    }

    let line_count = buffer.line_count();
    let start = range.start.min(line_count.saturating_sub(1));
    let end = range.end.max(start + 1).min(line_count);
    let first_chunk = start / LARGE_MARKDOWN_CHUNK_LINES;
    let last_chunk = (end - 1) / LARGE_MARKDOWN_CHUNK_LINES;
    let mut changed = false;

    for chunk in first_chunk..=last_chunk {
        if lazy.is_parsed(chunk) {
            continue;
        }

        let core_start = chunk * LARGE_MARKDOWN_CHUNK_LINES;
        let core_end = ((chunk + 1) * LARGE_MARKDOWN_CHUNK_LINES).min(line_count);
        let mut parse_start = core_start;
        let mut scanned = 0;
        while parse_start > 0
            && scanned < LARGE_MARKDOWN_BOUNDARY_SCAN_LINES
            && !buffer.line_content(parse_start - 1).trim().is_empty()
        {
            parse_start -= 1;
            scanned += 1;
        }
        let mut parse_end = core_end;
        scanned = 0;
        while parse_end < line_count
            && scanned < LARGE_MARKDOWN_BOUNDARY_SCAN_LINES
            && !buffer.line_content(parse_end).trim().is_empty()
        {
            parse_end += 1;
            scanned += 1;
        }

        let byte_start = buffer.line_start(parse_start);
        let byte_end = if parse_end < line_count {
            buffer.line_start(parse_end)
        } else {
            buffer.len()
        };
        let mut parsed = if parse_start == 0 {
            markdown::parse_decorations(&buffer.text()[byte_start..byte_end])
        } else {
            markdown::parse_decorations_fragment(&buffer.text()[byte_start..byte_end])
        };
        let expected_len = parse_end - parse_start;
        parsed.truncate(expected_len);
        parsed.resize_with(expected_len, LineDecoration::default);
        for decoration in &mut parsed {
            offset_decoration_positions(decoration, parse_start as isize, byte_start as isize);
        }
        decorations[parse_start..parse_end].clone_from_slice(&parsed);
        lazy.mark_parsed(chunk);
        changed = true;
    }

    changed
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum WordCharClass {
    AsciiWord,
    Cjk,
    Whitespace,
    Other,
}

#[derive(Clone, Copy)]
struct MultiCursorResolvedEdit {
    start: usize,
    cursor_after: usize,
    delta: isize,
    is_primary: bool,
    ordered_list_update: Option<bool>,
}

fn apply_offset_delta(offset: usize, delta: isize) -> usize {
    if delta >= 0 {
        offset.saturating_add(delta as usize)
    } else {
        offset.saturating_sub((-delta) as usize)
    }
}

fn shift_line_after_transfer(
    old_line: usize,
    start_line: usize,
    old_end_line_exclusive: usize,
    new_end_line_exclusive: usize,
) -> usize {
    if old_line <= start_line {
        old_line
    } else if old_line <= old_end_line_exclusive {
        new_end_line_exclusive
    } else {
        let old_suffix_distance = old_line - old_end_line_exclusive;
        new_end_line_exclusive + old_suffix_distance
    }
}

/// Shifts the absolute line- and byte-offset fields stored in a
/// `LineDecoration` by the given deltas. Every other range a decoration carries
/// (inline spans, syntax ranges, display replacements, table cell ranges, …) is
/// stored relative to its own line, so only `block_preview` (absolute line
/// numbers and an absolute source byte range) and `table_info` (absolute line
/// numbers) need adjusting when decorations are spliced incrementally.
fn offset_decoration_positions(
    decoration: &mut LineDecoration,
    line_delta: isize,
    byte_delta: isize,
) {
    let shift_usize = |value: &mut usize, delta: isize| {
        if delta != 0 {
            *value = (*value as isize + delta).max(0) as usize;
        }
    };
    let Some(payload) = decoration.payload_mut() else {
        return;
    };
    if let Some(preview) = payload.block_preview.as_mut() {
        shift_usize(&mut preview.start_line, line_delta);
        shift_usize(&mut preview.end_line, line_delta);
        shift_usize(&mut preview.source_range.start, byte_delta);
        shift_usize(&mut preview.source_range.end, byte_delta);
    }
    if let Some(table_info) = payload.table_info.as_mut() {
        shift_usize(&mut table_info.table_start_line, line_delta);
        shift_usize(&mut table_info.table_end_line, line_delta);
    }
}

/// Pure core of incremental markdown decoration parsing. Given the pre-edit
/// decorations, the already-edited buffer, and the line/byte deltas of the edit,
/// returns a full set of decorations equivalent to a full `parse_decorations` of
/// the edited buffer — but built by re-parsing only the affected block window
/// and splicing it into the moved (not cloned) prefix/suffix. Returns `None`
/// when incremental parsing is unsafe and the caller must fall back to a full
/// parse. See `Editor::try_incremental_markdown_decorations` for the rationale
/// behind each guard.
fn compute_incremental_markdown_decorations(
    old_decorations: Vec<LineDecoration>,
    buffer: &Buffer,
    new_end_line_exclusive: usize,
    old_end_line_exclusive: usize,
    dirty_start_line: usize,
    dirty_end_line_exclusive: usize,
    byte_delta: isize,
) -> Result<Vec<LineDecoration>, &'static str> {
    let new_line_count = buffer.line_count();
    if new_line_count == 0 {
        return Err("empty_buffer");
    }

    let line_delta = new_end_line_exclusive as isize - old_end_line_exclusive as isize;
    let old_line_count = new_line_count as isize - line_delta;
    if old_line_count < 0 || old_decorations.len() != old_line_count as usize {
        return Err("decoration_len_mismatch");
    }

    let mut win_start = dirty_start_line.min(new_line_count);
    let mut win_end = dirty_end_line_exclusive.min(new_line_count);
    if win_start >= win_end {
        return Err("empty_window");
    }

    // Grow the window up/down to blank-line boundaries so a multi-line paragraph
    // (and any inline span that spans a soft newline) is parsed whole.
    let max_window_growth = MAX_INCREMENTAL_WINDOW_LINES;
    while win_start > 0
        && win_end - win_start < max_window_growth
        && !buffer.line_content(win_start - 1).trim().is_empty()
    {
        win_start -= 1;
    }
    while win_end < new_line_count
        && win_end - win_start < max_window_growth
        && !buffer.line_content(win_end).trim().is_empty()
    {
        win_end += 1;
    }
    // If the surrounding block has no blank-line boundary within the growth
    // budget, we cannot safely parse it in isolation.
    if win_end - win_start >= max_window_growth {
        return Err("no_block_boundary");
    }

    // Not worth incremental work (and front-matter/global concerns apply) if the
    // window now spans essentially the whole document.
    if win_start == 0 && win_end == new_line_count {
        return Err("window_spans_document");
    }

    let window_len = win_end - win_start;

    // The line just above the window must be a true block boundary. A blank line
    // can still live *inside* a fenced code block, so consult the old decoration
    // (identity-mapped because `win_start <= start_line`) and bail if we'd cut a
    // code block.
    if win_start > 0 {
        if let Some(above) = old_decorations.get(win_start - 1) {
            if matches!(
                above.block_type,
                BlockType::CodeBlock | BlockType::CodeBlockFence
            ) {
                return Err("window_starts_in_code_block");
            }
        }
    }

    // Map the new-coords window end back to old-coords to know which slice of the
    // existing decorations is replaced.
    let old_win_end = win_end as isize - line_delta;
    if old_win_end < win_start as isize || old_win_end > old_decorations.len() as isize {
        return Err("old_window_out_of_range");
    }
    let old_win_end = old_win_end as usize;

    // Byte range of the window in the already-edited buffer.
    let byte_start = buffer.line_start(win_start);
    let byte_end = if win_end < new_line_count {
        buffer.line_start(win_end)
    } else {
        buffer.len()
    };
    let buffer_text = buffer.text();
    if byte_start > byte_end || byte_end > buffer_text.len() {
        return Err("byte_range_invalid");
    }
    let window_text = &buffer_text[byte_start..byte_end];

    // Multi-line block math (`$$ … $$`) can straddle the window edge and its
    // delimiters pair across arbitrary distance, so re-parsing a window that
    // contains one in isolation is unsafe. A bare `$` (inline math or currency)
    // is *not* a hazard: inline math produces no decoration. Reference links and
    // footnotes are handled below by prepending their definitions, so they no
    // longer force a full parse.
    if window_text.contains("$$") {
        return Err("window_has_block_math");
    }
    // A non-leading window must not begin with a front-matter delimiter, or the
    // sub-parse would misread it as front matter.
    if win_start > 0 {
        let first_line = buffer.line_content(win_start);
        let first_trimmed = first_line.trim();
        if first_trimmed == "---" || first_trimmed == "+++" {
            return Err("window_starts_front_matter");
        }
    }

    // Reference-style link/image definitions (`[label]: …`) and footnote
    // definitions (`[^label]: …`) may live anywhere in the document, yet a
    // *usage* inside the window needs its definition present for pulldown to
    // resolve it as a link/footnote rather than literal text. Collect every
    // definition outside the window and prepend it to the window text so the
    // isolated parse resolves identically to a full-document parse. A definition
    // merely *present* in the window is harmless (it renders as block metadata),
    // and the edit itself never touches reference syntax here — the dirty-range
    // pre-guard returns no transfer for edits containing `]:`, `[^`, or `][` —
    // so no out-of-window usage's rendering can change.
    //
    // A reference *usage* always contains `[`, so when the window has none we
    // can skip the whole-document definition scan and the combined re-parse.
    // This is the dominant cost on large files (every Enter/delete in plain
    // prose would otherwise rebuild and re-parse a prefix of all definitions),
    // so the `[`-free fast path keeps ordinary editing cheap.
    let definition_prefix = if window_text.contains('[') {
        collect_reference_definition_prefix(buffer, win_start, win_end, window_text)
    } else {
        String::new()
    };
    let prefix_byte_len = definition_prefix.len();
    let prefix_line_count = definition_prefix
        .bytes()
        .filter(|byte| *byte == b'\n')
        .count();

    let mut window_decorations = if definition_prefix.is_empty() {
        markdown::parse_decorations(window_text)
    } else {
        let combined = format!("{definition_prefix}{window_text}");
        let mut combined_decorations = markdown::parse_decorations(&combined);
        // Drop the prepended definition lines; the remainder lines up with the
        // real window content.
        if combined_decorations.len() < prefix_line_count {
            return Err("prefix_parse_short");
        }
        combined_decorations.drain(0..prefix_line_count);
        combined_decorations
    };

    // `parse_decorations` can emit a trailing phantom line when the slice ends in
    // a newline; normalize to exactly `window_len` entries.
    if window_decorations.len() > window_len {
        window_decorations.truncate(window_len);
    } else {
        while window_decorations.len() < window_len {
            window_decorations.push(LineDecoration::default());
        }
    }
    // Make the (combined-relative) absolute fields global. Combined line
    // `prefix_line_count + i` and combined byte `prefix_byte_len + b` map to the
    // real window line `win_start + i` / byte `byte_start + b`, so the deltas
    // subtract the prepended prefix.
    let line_offset = win_start as isize - prefix_line_count as isize;
    let byte_offset = byte_start as isize - prefix_byte_len as isize;
    for decoration in &mut window_decorations {
        offset_decoration_positions(decoration, line_offset, byte_offset);
    }

    // Splice in place; `Vec::splice` moves the unchanged prefix/suffix, never
    // cloning the ~100k lines a full parse would rebuild.
    let mut result = old_decorations;
    result.splice(win_start..old_win_end, window_decorations);

    // Shift absolute positions in the untouched suffix by the edit deltas.
    if line_delta != 0 || byte_delta != 0 {
        for decoration in result.iter_mut().skip(win_start + window_len) {
            offset_decoration_positions(decoration, line_delta, byte_delta);
        }
    }

    if result.len() != new_line_count {
        return Err("result_len_mismatch");
    }
    Ok(result)
}

/// Upper bound on how far the incremental window may grow when searching for a
/// blank-line block boundary. Edits inside a contiguous non-blank block larger
/// than this fall back to a full parse rather than re-parsing an unbounded
/// window (which would defeat the point of going incremental).
const MAX_INCREMENTAL_WINDOW_LINES: usize = 512;

/// Returns `true` if `trimmed` (a leading-whitespace-trimmed line) is a
/// reference-style link/image definition (`[label]: …`) or a footnote
/// definition (`[^label]: …`). Such lines must be visible to pulldown for any
/// usage referencing the same label to resolve, even when the usage is in a
/// re-parsed window and the definition is not.
fn is_reference_definition_line(trimmed: &str) -> bool {
    if !trimmed.starts_with('[') {
        return false;
    }
    match trimmed.find(']') {
        // Need a non-empty label and a `:` immediately after the closing `]`.
        Some(close) => close > 1 && trimmed[close + 1..].starts_with(':'),
        None => false,
    }
}

/// Collects every reference/footnote definition line that lies *outside* the
/// `[win_start, win_end)` line range **and whose label is actually referenced
/// inside the window**, each terminated with a newline and the whole block
/// followed by a blank line so it parses as standalone block metadata when
/// prepended to the window text. Definitions inside the window are skipped
/// because they are already part of `window_text`.
///
/// Filtering by referenced label is essential on large documents: a file with
/// thousands of definitions would otherwise prepend and re-parse all of them on
/// every keystroke (a full-document-sized parse), even though the window
/// usually references at most a handful. Over-inclusion would only be a minor
/// slowdown (extra definitions render as harmless metadata that is dropped),
/// and every reference syntax (`[label]`, `[text][label]`, `[label][]`,
/// `[^label]`) embeds the label literally in the usage, so a substring match
/// against the window never misses a needed definition.
fn collect_reference_definition_prefix(
    buffer: &Buffer,
    win_start: usize,
    win_end: usize,
    window_text: &str,
) -> String {
    let window_lower = window_text.to_lowercase();
    let mut prefix = String::new();
    let line_count = buffer.line_count();
    for line in 0..line_count {
        if line >= win_start && line < win_end {
            continue;
        }
        let content = buffer.line_content(line);
        let trimmed = content.trim_start();
        if !is_reference_definition_line(trimmed) {
            continue;
        }
        let Some(label) = reference_definition_label(trimmed) else {
            continue;
        };
        // Match the label only when it appears as an actual bracketed usage in
        // the window (`[label]`, `[text][label]`, `[label][]`, or `[^label]`),
        // not as a bare substring. A bare-substring test over-includes on
        // footnote-heavy documents — a window containing the digit "1" would
        // pull in `[^1]`, `[^10]`, … turning the prepend+re-parse into a
        // full-document-sized parse (the source of the multi-hundred-ms spikes).
        // Over-inclusion is otherwise harmless, so a conservative empty-label
        // definition is always kept.
        let referenced = label.is_empty() || {
            let bracketed = format!("[{}]", label.to_lowercase());
            let footnote = format!("[^{}]", label.to_lowercase());
            window_lower.contains(&bracketed) || window_lower.contains(&footnote)
        };
        if referenced {
            prefix.push_str(content);
            prefix.push('\n');
        }
    }
    if !prefix.is_empty() {
        // Trailing blank line separates the definition block from the window so
        // pulldown does not fold the first window line into the last definition.
        prefix.push('\n');
    }
    prefix
}

/// Extracts the label of a reference/footnote definition line (the text between
/// the leading `[` and the matching `]`, with any footnote `^` prefix removed).
/// Assumes `trimmed` already satisfies `is_reference_definition_line`.
fn reference_definition_label(trimmed: &str) -> Option<&str> {
    let close = trimmed.find(']')?;
    let inner = &trimmed[1..close];
    Some(inner.strip_prefix('^').unwrap_or(inner))
}

fn markdown_decoration_is_plain_text(decoration: &LineDecoration) -> bool {
    matches!(decoration.block_type, BlockType::Normal)
        && decoration.block_contexts.is_empty()
        && decoration.callout_kind.is_none()
        && decoration.inline_spans.is_empty()
        && decoration.diff_added_spans.is_empty()
        && decoration.diff_removed_spans.is_empty()
        && decoration.syntax_ranges.is_empty()
        && decoration.display_replacements.is_empty()
        && decoration.block_preview.is_none()
        && decoration.highlight_spans.is_empty()
        && decoration.superscript_spans.is_empty()
        && decoration.subscript_spans.is_empty()
        && decoration.wiki_links.is_empty()
        && decoration.footnote_references.is_empty()
        && decoration.footnote_definition_label.is_none()
        && decoration.table_info.is_none()
        && decoration.code_language.is_none()
        && decoration.image_path.is_none()
}

fn markdown_plain_edit_can_change_inline_delimiter_flanking(
    _inserted_text: &str,
    line_before_edit: &str,
) -> bool {
    line_before_edit
        .chars()
        .any(|ch| matches!(ch, '*' | '_' | '~' | '+' | '=' | '[' | ']' | '`' | '^'))
}

fn finalize_multi_cursor_edits(
    mut edits: Vec<MultiCursorResolvedEdit>,
) -> (Vec<(usize, bool)>, Vec<(usize, bool)>) {
    edits.sort_by(|a, b| a.start.cmp(&b.start));

    let mut cumulative_delta = 0isize;
    let mut cursor_positions = Vec::with_capacity(edits.len());
    let mut ordered_list_updates = Vec::new();

    for edit in edits {
        let final_pos = apply_offset_delta(edit.cursor_after, cumulative_delta);
        cursor_positions.push((final_pos, edit.is_primary));
        if let Some(preserve_current_number) = edit.ordered_list_update {
            ordered_list_updates.push((final_pos, preserve_current_number));
        }
        cumulative_delta += edit.delta;
    }

    (cursor_positions, ordered_list_updates)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PendingPreviewReveal {
    pub(crate) scroll_offset: f32,
}

pub(crate) struct PendingHistoryTransaction {
    before_view: history_store::SnapshotViewState,
    merge_with_previous: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct PendingTypingScrollGuard {
    pub(crate) line: usize,
    pub(crate) visual_row: usize,
    pub(crate) scroll_offset: f32,
}

/// Anchors a clicked line to its current on-screen position so that a focus
/// change which re-folds/expands content above it does not make the view jump.
/// Captured at click time (old layout) and reconciled after the next layout.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ClickScrollAnchor {
    pub(crate) line: usize,
    pub(crate) rel_y_before: f32,
    pub(crate) scroll_before: f32,
}

pub(crate) struct PendingLineDisplayTransfer {
    cached_lines: LineDisplayCache,
    start_line: usize,
    old_end_line_exclusive: usize,
    new_end_line_exclusive: usize,
    dirty_start_line: usize,
    dirty_end_line_exclusive: usize,
    markdown_plain_decoration_transfer: bool,
    /// Net change in buffer byte length caused by the edit
    /// (`inserted_text.len() - replaced_range.len()`). Used to shift the
    /// absolute byte offsets stored in suffix `block_preview` ranges when
    /// splicing decorations incrementally.
    byte_delta: isize,
}

impl PendingLineDisplayTransfer {
    fn apply(mut self, new_line_count: usize) -> LineDisplayCache {
        let replace_start = self.start_line.min(self.cached_lines.len());
        let replace_end = self
            .old_end_line_exclusive
            .min(self.cached_lines.len())
            .max(replace_start);
        let inserted_line_count = self.new_end_line_exclusive.saturating_sub(self.start_line);
        if replace_end - replace_start == inserted_line_count {
            for line in &mut self.cached_lines[replace_start..replace_end] {
                *line = None;
            }
        } else {
            self.cached_lines.splice(
                replace_start..replace_end,
                std::iter::repeat_with(|| None).take(inserted_line_count),
            );
        }

        if self.cached_lines.len() > new_line_count {
            self.cached_lines.truncate(new_line_count);
        } else if self.cached_lines.len() < new_line_count {
            self.cached_lines.resize_with(new_line_count, || None);
        }

        let dirty_start = self.dirty_start_line.min(new_line_count);
        let dirty_end = self.dirty_end_line_exclusive.min(new_line_count);
        for line_idx in dirty_start..dirty_end {
            self.cached_lines[line_idx] = None;
        }

        self.cached_lines
    }

    /// Splice default (plain) decorations over the edited line range. Shared
    /// by code/text tabs and by markdown edits that provably stay plain text;
    /// `code_language` tags the inserted lines for syntax highlighting.
    fn apply_plain_decoration_transfer(
        &self,
        decorations: &mut Vec<LineDecoration>,
        code_language: Option<&str>,
    ) {
        let replace_start = self.start_line.min(decorations.len());
        let replace_end = self.old_end_line_exclusive.min(decorations.len());
        let inserted_line_count = self.new_end_line_exclusive.saturating_sub(self.start_line);
        let mut default_decoration = LineDecoration::default();
        if let Some(language) = code_language {
            default_decoration.code_language = Some(language.to_string());
        }

        if replace_end - replace_start == inserted_line_count {
            for decoration in &mut decorations[replace_start..replace_end] {
                *decoration = default_decoration.clone();
            }
        } else {
            decorations.splice(
                replace_start..replace_end,
                (0..inserted_line_count).map(move |_| default_decoration.clone()),
            );
        }
        if decorations.is_empty() {
            decorations.push(LineDecoration::default());
        }
    }
}

fn line_display_transfer_line_bounds(
    buffer: &Buffer,
    replaced_range: &Range<usize>,
    inserted_text: &str,
) -> (usize, usize, usize) {
    let start = replaced_range.start.min(buffer.len());
    let end = replaced_range.end.min(buffer.len());
    let start_line = buffer.offset_to_line_col(start).0;
    let end_probe = end
        .saturating_sub(usize::from(end > start))
        .min(buffer.len());
    let old_end_line = buffer.offset_to_line_col(end_probe).0;
    let mut old_end_line_exclusive = old_end_line + 1;

    if end > start && end < buffer.len() {
        let end_line = buffer.offset_to_line_col(end).0;
        if end_line > old_end_line && end == buffer.line_start(end_line) {
            old_end_line_exclusive = (end_line + 1).min(buffer.line_count());
        }
    }

    let new_line_count = inserted_text.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let new_end_line_exclusive = start_line + new_line_count;

    (start_line, old_end_line_exclusive, new_end_line_exclusive)
}

#[derive(Default)]
struct SparseLineGeometry {
    line_count: usize,
    default_height: f64,
    measured_deltas: BTreeMap<usize, f64>,
    block_delta_tree: Vec<f64>,
}

impl SparseLineGeometry {
    fn reset(&mut self, line_count: usize, default_height: f64) {
        self.line_count = line_count;
        self.default_height = default_height;
        self.measured_deltas.clear();
        let block_count = line_count.div_ceil(SPARSE_LINE_GEOMETRY_BLOCK_LINES);
        self.block_delta_tree.clear();
        self.block_delta_tree.resize(block_count + 1, 0.0);
    }

    fn ensure_dimensions(&mut self, line_count: usize, default_height: f64) {
        if self.line_count != line_count || (self.default_height - default_height).abs() > 0.01 {
            self.reset(line_count, default_height);
        }
    }

    fn add_block_delta(&mut self, block: usize, delta: f64) {
        let mut index = block + 1;
        while index < self.block_delta_tree.len() {
            self.block_delta_tree[index] += delta;
            index += index & index.wrapping_neg();
        }
    }

    fn set_measured_height(&mut self, line: usize, height: f64) {
        if line >= self.line_count {
            return;
        }
        let next_delta = height - self.default_height;
        let previous_delta = self.measured_deltas.remove(&line).unwrap_or(0.0);
        let stored_delta = if next_delta.abs() > 0.01 {
            self.measured_deltas.insert(line, next_delta);
            next_delta
        } else {
            0.0
        };
        self.add_block_delta(
            line / SPARSE_LINE_GEOMETRY_BLOCK_LINES,
            stored_delta - previous_delta,
        );
    }

    fn clear_range(&mut self, range: Range<usize>) {
        let removed: Vec<(usize, f64)> = self
            .measured_deltas
            .range(range)
            .map(|(&line, &delta)| (line, delta))
            .collect();
        for (line, delta) in removed {
            self.measured_deltas.remove(&line);
            self.add_block_delta(line / SPARSE_LINE_GEOMETRY_BLOCK_LINES, -delta);
        }
    }

    fn splice_lines(
        &mut self,
        start_line: usize,
        old_end_line_exclusive: usize,
        new_end_line_exclusive: usize,
        dirty_range: Range<usize>,
        new_line_count: usize,
        default_height: f64,
    ) {
        let line_delta = new_end_line_exclusive as isize - old_end_line_exclusive as isize;
        let expected_old_line_count = (new_line_count as isize - line_delta).max(0) as usize;
        if self.line_count != expected_old_line_count
            || (self.default_height - default_height).abs() > 0.01
        {
            self.reset(new_line_count, default_height);
            return;
        }
        let previous = std::mem::take(&mut self.measured_deltas);
        self.line_count = new_line_count;
        self.block_delta_tree.clear();
        self.block_delta_tree.resize(
            new_line_count.div_ceil(SPARSE_LINE_GEOMETRY_BLOCK_LINES) + 1,
            0.0,
        );

        for (line, delta) in previous {
            let mapped = if line < start_line {
                Some(line)
            } else if line >= old_end_line_exclusive {
                Some((line as isize + line_delta).max(0) as usize)
            } else {
                None
            };
            let Some(mapped) = mapped else {
                continue;
            };
            if mapped >= new_line_count || dirty_range.contains(&mapped) {
                continue;
            }
            self.measured_deltas.insert(mapped, delta);
            self.add_block_delta(mapped / SPARSE_LINE_GEOMETRY_BLOCK_LINES, delta);
        }
    }

    fn block_delta_before(&self, block: usize) -> f64 {
        let mut index = block.min(self.block_delta_tree.len().saturating_sub(1));
        let mut total = 0.0;
        while index > 0 {
            total += self.block_delta_tree[index];
            index &= index - 1;
        }
        total
    }

    fn delta_before_line(&self, line: usize) -> f64 {
        let line = line.min(self.line_count);
        let block = line / SPARSE_LINE_GEOMETRY_BLOCK_LINES;
        let block_start = block * SPARSE_LINE_GEOMETRY_BLOCK_LINES;
        self.block_delta_before(block)
            + self
                .measured_deltas
                .range(block_start..line)
                .map(|(_, delta)| *delta)
                .sum::<f64>()
    }

    fn y_offset(&self, line: usize, padding_top: f64) -> f64 {
        let line = line.min(self.line_count);
        padding_top + line as f64 * self.default_height + self.delta_before_line(line)
    }

    fn line_at_y(&self, y: f64, padding_top: f64) -> usize {
        if self.line_count == 0 {
            return 0;
        }
        let mut low = 0usize;
        let mut high = self.line_count;
        while low < high {
            let mid = (low + high) / 2;
            if self.y_offset(mid + 1, padding_top) <= y {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        low.min(self.line_count.saturating_sub(1))
    }
}

#[derive(Default)]
pub(crate) struct LineGeometryCacheState {
    prefix_y_offsets: Vec<f32>,
    sparse: SparseLineGeometry,
}

// ── EditorView ──────────────────────────────────────────────────────

pub struct EditorView {
    pub buffer: Buffer,
    pub cursor: Cursor,
    pub(crate) extra_cursors: Vec<Cursor>,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) embedded_editor: bool,
    /// Layout-accurate content height recorded during element prepaint, used
    /// by embedded containers so their measured height never
    /// oscillates between estimated and shaped layouts while typing.
    pub(crate) embedded_content_height: Cell<f32>,
    /// Read-only embedded editors follow the newest content
    /// while the view rests at the bottom; scrolling up detaches, scrolling
    /// back to the bottom re-attaches.
    pub(crate) embedded_stick_to_bottom: bool,
    pub(crate) embedded_read_only: bool,
    pub(crate) theme_kind: ThemeKind,
    pub(crate) editor_theme_mode: EditorThemeMode,
    pub(crate) theme: Theme,
    pub(crate) styles: crate::style::Styles,
    pub(crate) decorations: Vec<LineDecoration>,
    pub(crate) lazy_markdown_decorations: Option<LazyMarkdownDecorations>,
    pub(crate) markdown_outline_index: RefCell<Option<MarkdownOutlineIndex>>,
    pub(crate) markdown_outline_build_task: Option<Task<()>>,
    pub(crate) markdown_outline_epoch: usize,
    pub(crate) large_document_frame_bench: Option<LargeDocumentFrameBench>,
    pub(crate) scroll_offset: f32,
    pub(crate) is_selecting: bool,
    pub(crate) vertical_move_goal_x: Option<f32>,
    pub(crate) last_scroll_cursor_visual_x: Option<f32>,
    pub(crate) wrap_boundary_affinity: Option<WrapBoundaryAffinity>,
    pub(crate) line_display_info: LineDisplayCache,
    pub(crate) visible_line_geometry: Vec<(usize, f32, f32)>,
    pending_line_display_transfer: Option<PendingLineDisplayTransfer>,
    line_display_trim_update_counter: Cell<usize>,
    line_geometry_cache_dirty: Cell<bool>,
    line_geometry_cache: RefCell<LineGeometryCacheState>,
    indent_guide_step_cache_dirty: Cell<bool>,
    indent_guide_step_cache: Cell<usize>,
    pub(crate) layout_dirty: bool,
    pub(crate) pending_scroll_to_cursor: bool,
    pub(crate) pending_center_cursor_in_view: bool,
    pub(crate) pending_center_cursor_in_view_immediately: bool,
    /// Frames left to re-affirm a centered cursor while geometry settles.
    pub(crate) center_cursor_confirm_frames: u8,
    /// Last text removed by KillToLineEnd (ctrl-k); pasted back by Yank.
    pub(crate) kill_ring: Option<String>,
    pub(crate) pending_preview_reveal: Option<PendingPreviewReveal>,
    pub(crate) pending_typing_scroll_guard: Option<PendingTypingScrollGuard>,
    pub(crate) pending_click_anchor: Option<ClickScrollAnchor>,
    pub(crate) pending_history_transaction: Option<PendingHistoryTransaction>,
    /// Cache key of the generated preview currently shown in code (edit) mode,
    /// tracked so the rendered view (zoom/pan) can be reset whenever the cursor
    /// enters or leaves a preview's source, i.e. on every edit↔render switch.
    pub(crate) code_mode_preview_key: Option<String>,
    /// Debounced live re-raster of the chart/formula being edited in code view.
    /// The in-edit raster is frozen on each keystroke; this rebuilds it once
    /// typing settles so the render updates live without jittering per keystroke.
    pub(crate) generated_preview_rerender_task: Option<Task<()>>,
    pub(crate) generated_preview_rerender_pending: bool,
    pub(crate) last_viewport_height: f32,
    pub(crate) last_viewport_width: f32,
    /// Screen bounds of the whiteboard canvas element from the last paint, used
    /// to map pointer positions into canvas coordinates.
    pub(crate) whiteboard_canvas_bounds: Option<Bounds<Pixels>>,
    /// Active pointer interaction on the whiteboard canvas, if any.
    pub(crate) whiteboard_drag: Option<WhiteboardDrag>,
    /// Last known pointer position over the whiteboard canvas (screen space),
    /// used to compute the hover cursor during paint.
    pub(crate) whiteboard_pointer: Option<Point<Pixels>>,
    /// Whether Space is held (enables drag-to-pan on the whiteboard).
    pub(crate) whiteboard_space_down: bool,
    /// Elements currently highlighted as magnetic bind targets while dragging
    /// linear endpoints over them.
    pub(crate) whiteboard_bind_targets: Vec<String>,
    /// Active inline text editor for a whiteboard element, if any.
    pub(crate) whiteboard_text_editor: Option<whiteboard_input::WhiteboardTextEditor>,
    // Cursor blink state
    pub(crate) cursor_opacity: f32,
    pub(crate) blink_epoch: usize,
    /// Mirrors the conditions under which the caret is actually painted; the
    /// blink loop repaints the whole view tree, so it must idle otherwise.
    pub(crate) cursor_blink_allowed: bool,
    // IME preedit state
    pub(crate) marked_range: Option<Range<usize>>,
    pub(crate) extra_marked_ranges: Vec<Range<usize>>,
    pub(crate) ime_composition_snapshot: Option<EditorSnapshot>,
    pub(crate) checkbox_areas: Vec<(gpui::Bounds<Pixels>, usize)>,
    pub(crate) hovering_checkbox: bool,
    pub(crate) hovering_checkbox_line: Option<usize>,
    pub(crate) image_click_areas: Vec<(gpui::Bounds<Pixels>, usize)>,
    pub(crate) image_preview_zoom_areas: Vec<(gpui::Bounds<Pixels>, usize)>,
    pub(crate) generated_preview_click_areas: Vec<(gpui::Bounds<Pixels>, usize)>,
    pub(crate) generated_preview_image_areas: Vec<(gpui::Bounds<Pixels>, usize)>,
    pub(crate) generated_preview_zoom_areas: Vec<(gpui::Bounds<Pixels>, usize)>,
    pub(crate) generated_preview_action_areas:
        Vec<(gpui::Bounds<Pixels>, usize, GeneratedPreviewAction)>,
    pub(crate) preview_overlay_action_areas: Vec<(gpui::Bounds<Pixels>, GeneratedPreviewAction)>,
    pub(crate) hovering_image_preview_line: Option<usize>,
    pub(crate) hovering_generated_preview_line: Option<usize>,
    pub(crate) hovering_generated_preview_zoom_line: Option<usize>,
    pub(crate) hovering_generated_preview_action: Option<(usize, GeneratedPreviewAction)>,
    pub(crate) hovering_preview_overlay_action: Option<GeneratedPreviewAction>,
    pub(crate) generated_preview_actions_visible_line: Option<usize>,
    pub(crate) generated_preview_actions_opacity: f32,
    pub(crate) generated_preview_actions_opacity_target: f32,
    pub(crate) generated_preview_actions_opacity_epoch: usize,
    pub(crate) generated_preview_actions_opacity_task: Option<Task<()>>,
    pub(crate) code_block_hover_areas: Vec<(gpui::Bounds<Pixels>, usize)>,
    pub(crate) code_block_copy_areas: Vec<(gpui::Bounds<Pixels>, usize)>,
    pub(crate) hovering_code_block_start: Option<usize>,
    pub(crate) hovering_code_block_copy_start: Option<usize>,
    pub(crate) copied_code_block_start: Option<usize>,
    pub(crate) code_block_copy_feedback_epoch: usize,
    /// Table cell click areas: (bounds, table_start_line, visual_row, col)
    pub(crate) table_click_areas: Vec<(gpui::Bounds<Pixels>, usize, usize, usize)>,
    /// Table column handle areas: (bounds, table_start_line, col_index)
    pub(crate) table_col_handle_areas: Vec<(gpui::Bounds<Pixels>, usize, usize)>,
    /// Table row handle areas: (bounds, table_start_line, visual_row)
    pub(crate) table_row_handle_areas: Vec<(gpui::Bounds<Pixels>, usize, usize)>,
    /// Active table drag operation
    pub(crate) table_drag: Option<TableDragState>,
    /// Which table (by table_start_line) the mouse is hovering near (for handle visibility)
    pub(crate) hovering_table_start: Option<usize>,
    /// Which specific handle the mouse is over: (table_start_line, is_column, index)
    pub(crate) hovering_handle: Option<(usize, bool, usize)>,
    /// Which specific handle is selected: (table_start_line, is_column, index)
    pub(crate) selected_handle: Option<(usize, bool, usize)>,
    pub(crate) image_cache: HashMap<String, Option<Arc<RenderImage>>>,
    pub(crate) generated_preview_svg_cache: HashMap<String, Arc<str>>,
    pub(crate) generated_preview_overlay_cache: HashMap<String, (u16, Arc<RenderImage>)>,
    pub(crate) generated_preview_views: HashMap<String, ImageViewState>,
    pub(crate) generated_preview_drag_state: Option<GeneratedPreviewDragState>,
    pub(crate) sidebar_root: PathBuf,
    pub(crate) workspace_bar_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub(crate) expanded_dirs: HashSet<PathBuf>,
    /// 多选：所有选中的 sidebar 路径
    pub(crate) selected_sidebar_paths: HashSet<PathBuf>,
    pub(crate) current_file_path: Option<PathBuf>,
    pub(crate) sidebar_error: Option<String>,
    pub(crate) tabs: Vec<OpenTab>,
    pub(crate) tab_layout_cache: HashMap<String, (LineDisplayCache, f32)>,
    pub(crate) tab_generated_preview_cache: HashMap<String, GeneratedPreviewTabCache>,
    pub(crate) active_tab_id: Option<String>,
    pub(crate) active_tab_index_cache: Cell<Option<usize>>,
    pub(crate) active_file_kind_cache: Option<FileKind>,
    pub(crate) next_tab_id: usize,
    // ── Split panes ─────────────────────────────────────────────────
    // NOTE: every per-document field of this struct must be listed in
    // `pane_state!` (src/editor/pane.rs) or its value will leak between panes.
    /// All panes, including the focused one (whose state slot is detached; its
    /// real state lives in this struct's root fields).
    pub(crate) panes: Vec<Pane>,
    /// Recursive split layout over `panes` (leaves are pane ids).
    pub(crate) pane_layout: PaneLayout,
    /// The pane that owns keyboard focus and the root document fields.
    pub(crate) focused_pane_id: PaneId,
    /// Set while `with_pane` has temporarily swapped a non-focused pane's
    /// state into the root fields.
    pub(crate) swapped_pane_id: Option<PaneId>,
    pub(crate) next_pane_id: PaneId,
    /// Screen bounds of each pane's editor content area from the last paint.
    pub(crate) pane_bounds: HashMap<PaneId, Bounds<Pixels>>,
    pub(crate) pane_editor_content_views: HashMap<PaneId, gpui::Entity<caret::EditorContentView>>,
    pub(crate) pane_caret_overlay_views: HashMap<PaneId, gpui::Entity<caret::CaretOverlayView>>,
    /// While a tab drag hovers a pane's content area: the hovered pane and the
    /// split direction (None = merge into that pane).
    pub(crate) tab_drag_split_target: Option<(PaneId, Option<SplitDirection>)>,
    /// Pane previewed by a split/move item in a tab or terminal context menu.
    pub(crate) context_menu_pane_target: Option<(PaneId, Option<SplitDirection>)>,
    /// An in-progress drag on a divider between two split children.
    pub(crate) pane_resize_drag: Option<PaneResizeDrag>,
    pub(crate) last_editor_left: f32,
    pub(crate) last_editor_width: f32,
    pub(crate) last_content_left: f32,
    pub(crate) last_text_width: f32,
    pub(crate) last_window_scale_factor: f32,
    pub(crate) content_origin: Point<Pixels>,
    pub(crate) last_stable_input_bounds: Option<Bounds<Pixels>>,
    pub(crate) pending_input_bounds: Option<Bounds<Pixels>>,
    pub(crate) preview_overlay: Option<PreviewOverlayState>,
    pub(crate) preview_overlay_bounds: Option<Bounds<Pixels>>,
    pub(crate) preview_overlay_image_bounds: Option<Bounds<Pixels>>,
    pub(crate) last_mouse_down_target: Option<(Point<Pixels>, usize)>,
    pub(crate) selection_drag_has_moved: bool,
    pub(crate) context_menu: Option<ContextMenu>,
    pub(crate) suppress_next_editor_mouse_up: bool,
    pub(crate) context_submenu: Option<ContextMenuKind>,
    pub(crate) context_submenu_aim: Option<ContextSubmenuAim>,
    pub(crate) context_submenu_hovered: bool,
    pub(crate) format_heading_trigger_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub(crate) format_more_trigger_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// Anchors the heading popup to the format toolbar button; kept separate
    /// from the menu trigger bounds, which `open_context_menu` clears.
    pub(crate) format_toolbar_heading_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub(crate) export_trigger_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub(crate) context_menu_opacity: f32,
    pub(crate) context_menu_opacity_target: f32,
    pub(crate) context_menu_opacity_epoch: usize,
    pub(crate) context_menu_opacity_task: Option<Task<()>>,
    pub(crate) sidebar_edit: Option<SidebarEditMode>,
    pub(crate) sidebar_edit_blur_sub: Option<gpui::Subscription>,
    pub(crate) file_clipboard: Option<FileClipboard>,
    pub(crate) multi_cursor_clipboard: Option<MultiCursorClipboard>,
    pub(crate) ui_scale: f32,
    #[cfg(target_os = "macos")]
    pub(crate) traffic_light_ui_scale: Option<f32>,
    #[cfg(not(target_os = "macos"))]
    pub(crate) window_is_maximized: bool,
    #[cfg(target_os = "linux")]
    pub(crate) client_side_window_decorations: bool,
    pub(crate) folded_sections: HashSet<usize>,
    pub(crate) folded_image_previews: HashSet<usize>,
    pub(crate) generated_preview_keys: HashSet<String>,
    pub(crate) gutter_fold_areas: Vec<(gpui::Bounds<Pixels>, usize, bool)>,
    pub(crate) image_edit_areas: Vec<(gpui::Bounds<Pixels>, usize)>,
    pub(crate) hovering_image_edit_line: Option<usize>,
    pub(crate) hovering_fold_line: Option<usize>,
    pub(crate) hovering_gutter_fold_area: bool,
    pub(crate) gutter_fold_reveal: f32,
    pub(crate) gutter_fold_reveal_target: f32,
    pub(crate) gutter_fold_reveal_epoch: usize,
    pub(crate) gutter_fold_reveal_task: Option<Task<()>>,
    pub(crate) image_drag_state: Option<ImageDragState>,
    pub(crate) command_held: bool,
    pub(crate) hovered_editor_position: Option<Point<Pixels>>,
    pub(crate) editor_scrollbar_hovered: bool,
    pub(crate) editor_scrollbar_revealed: bool,
    pub(crate) editor_scrollbar_opacity: f32,
    pub(crate) editor_scrollbar_opacity_target: f32,
    pub(crate) editor_scrollbar_opacity_epoch: usize,
    pub(crate) editor_scrollbar_opacity_task: Option<Task<()>>,
    pub(crate) editor_scrollbar_width: f32,
    pub(crate) editor_scrollbar_width_target: f32,
    pub(crate) editor_scrollbar_width_epoch: usize,
    pub(crate) editor_scrollbar_width_task: Option<Task<()>>,
    pub(crate) editor_scrollbar_idle_epoch: usize,
    pub(crate) editor_scrollbar_idle_task: Option<Task<()>>,
    pub(crate) editor_scrollbar_drag: Option<EditorScrollbarDragState>,
    pub(crate) pending_editor_scrollbar_drag_position: Option<Point<Pixels>>,
    pub(crate) editor_scrollbar_drag_task: Option<Task<()>>,
    pub(crate) panel_scrollbars: HashMap<PanelScrollbarKind, PanelScrollbarState>,
    pub(crate) sidebar_custom_width: Option<f32>,
    /// Time of the last grouped typing checkpoint, used to coalesce rapid
    /// single-character edits into a single undo step (time-interval grouping).
    pub(crate) last_typing_checkpoint_time: Option<Instant>,
    /// Predicted cursor offset after the last grouped typing checkpoint. A new
    /// edit is only coalesced if it continues contiguously from this offset.
    pub(crate) last_typing_checkpoint_offset: Option<usize>,
    pub(crate) sidebar_resizing: bool,
    pub(crate) sidebar_display_width: f32,
    pub(crate) sidebar_layout_width: f32,
    pub(crate) sidebar_width_target: f32,
    pub(crate) sidebar_width_epoch: usize,
    pub(crate) sidebar_width_task: Option<Task<()>>,
    pub(crate) heading_submenu_open: bool,
    pub(crate) slash_menu_start: Option<usize>,
    pub(crate) slash_filter: String,
    pub(crate) wiki_link_start: Option<usize>,
    pub(crate) wiki_link_query: String,
    pub(crate) wiki_link_recent_results: Vec<WorkspaceFileCandidate>,
    pub(crate) wiki_link_search_results: Vec<QuickOpenResult>,
    pub(crate) wiki_link_recent_task: Option<Task<()>>,
    pub(crate) wiki_link_search_task: Option<Task<()>>,
    pub(crate) menu_selected_index: usize,
    pub(crate) sidebar_visible: bool,
    pub(crate) editor_left_aligned: bool,
    pub(crate) indent_settings: IndentSettings,
    pub(crate) tab_drag_candidate_id: Option<String>,
    pub(crate) tab_drag_press_position: Option<gpui::Point<gpui::Pixels>>,
    pub(crate) tab_dragging_id: Option<String>,
    pub(crate) recently_closed_tabs: Vec<OpenTab>,
    pub(crate) tab_navigation_history: Vec<TabNavigationTarget>,
    pub(crate) tab_navigation_index: usize,
    pub(crate) tab_navigation_suspended: bool,
    pub(crate) dialog: Option<Dialog>,
    pub(crate) notifications: Vec<notifications::Notification>,
    pub(crate) next_notification_id: usize,
    pub(crate) pending_close_tab_ids: Vec<String>,
    /// 多选：最后一个选中的 focus 路径
    pub(crate) focused_sidebar_path: Option<PathBuf>,
    pub(crate) suppress_next_sidebar_sync_path: Option<PathBuf>,
    /// 多选锚点（shift 连续选择的起点）
    pub(crate) sidebar_selection_anchor: Option<PathBuf>,
    pub(crate) sidebar_sort_mode: SidebarSortMode,
    pub(crate) file_list_display_mode: FileListDisplayMode,
    pub(crate) sidebar_starred_collapsed: bool,
    /// 拖拽悬停的目标目录（仅用于高亮）
    pub(crate) sidebar_drag_hover_target: Option<PathBuf>,
    /// 拖拽悬停的目录（用于延迟自动展开）
    pub(crate) sidebar_drag_expand_hover_dir: Option<PathBuf>,
    /// 当前 drag move 是否已被子 item 命中；tree 容器据此避免同帧清空 child hover。
    pub(crate) sidebar_drag_child_hover_position: Option<(f32, f32)>,
    /// 拖拽接近 sidebar 可见区域边缘时的自动滚动任务。
    pub(crate) sidebar_drag_scroll_task: Option<Task<()>>,
    pub(crate) selection_drag_scroll_task: Option<Task<()>>,
    pub(crate) selection_drag_position: Option<Point<Pixels>>,
    pub(crate) suppress_fold_for_empty_selection_drag: bool,
    /// 拖拽悬停自动展开的延迟 task
    pub(crate) sidebar_drag_expand_task: Option<Task<()>>,
    /// 拖拽悬停时自动展开、离开后可回折的目录集合
    pub(crate) sidebar_drag_auto_expanded_dirs: HashSet<PathBuf>,
    /// Shared so the render path can take a handle per frame instead of deep
    /// copying every entry (and its `PathBuf`) on each repaint.
    pub(crate) sidebar_items_cache: Rc<Vec<SidebarItem>>,
    pub(crate) sidebar_items_dirty: bool,
    pub(crate) sidebar_directory_children:
        HashMap<PathBuf, Vec<sidebar_tree::SidebarDirectoryChild>>,
    pub(crate) sidebar_directory_loading: HashSet<PathBuf>,
    pub(crate) sidebar_directory_epoch: usize,
    pub(crate) sidebar_directory_refresh_pending: bool,
    /// Markdown file count directly inside each directory, shown as a badge
    /// next to FileTree directory rows while the FileList pane is open.
    pub(crate) sidebar_dir_file_counts: HashMap<PathBuf, usize>,
    pub(crate) sidebar_dir_file_counts_loading: HashSet<PathBuf>,
    pub(crate) sidebar_dir_file_counts_dirty: HashSet<PathBuf>,
    pub(crate) sidebar_scroll_handle: ScrollHandle,
    pub(crate) pending_sidebar_scroll_top: Rc<Cell<Option<f32>>>,
    pub(crate) pending_sidebar_scroll_anchor: Option<PendingSidebarScrollAnchor>,
    pub(crate) pending_sidebar_reveal_path: Option<PathBuf>,
    pub(crate) tab_bar_scroll_handle: ScrollHandle,
    /// FileList pane (right of the file tree): current directory and cached items.
    pub(crate) file_list_dir: PathBuf,
    pub(crate) file_list_items: Vec<FileListItem>,
    pub(crate) file_list_dirty: bool,
    pub(crate) file_list_loaded_dir: Option<PathBuf>,
    pub(crate) file_list_epoch: usize,
    pub(crate) file_list_task: Option<Task<()>>,
    pub(crate) file_list_detail_task: Option<Task<()>>,
    pub(crate) file_list_scroll_handle: ScrollHandle,
    pub(crate) pending_file_list_scroll_top: Rc<Cell<Option<f32>>>,
    pub(crate) file_list_custom_width: Option<f32>,
    pub(crate) file_list_visible: bool,
    pub(crate) file_list_display_width: f32,
    pub(crate) file_list_layout_width: f32,
    pub(crate) file_list_width_target: f32,
    pub(crate) file_list_width_epoch: usize,
    pub(crate) file_list_width_task: Option<Task<()>>,
    pub(crate) file_list_resizing: bool,
    pub(crate) file_list_resize_grab_offset_x: f32,
    /// A markdown file whose FileList item should be scrolled into view once
    /// the current directory's items finish loading (tab/FileTree linkage).
    pub(crate) file_list_pending_reveal_path: Option<PathBuf>,
    /// When set, the pending FileList reveal centers the row instead of only
    /// bringing it minimally into view ("reveal in tree" navigation).
    pub(crate) file_list_pending_reveal_centered: bool,
    /// True while the active `sidebar_edit` (create/rename input) should render
    /// inside the FileList pane instead of the file tree.
    pub(crate) sidebar_edit_in_file_list: bool,
    /// True when the currently open sidebar context menu was opened from a
    /// FileList item (used to route the follow-up rename/create input row).
    pub(crate) file_list_menu_origin: bool,
    pub(crate) sidebar_git_statuses: HashMap<PathBuf, SidebarGitStatus>,
    pub(crate) sidebar_git_statuses_dirty: bool,
    pub(crate) sidebar_git_statuses_scanned_at: Option<SystemTime>,
    pub(crate) sidebar_git_statuses_refresh_epoch: usize,
    pub(crate) sidebar_git_statuses_task: Option<Task<()>>,
    pub(crate) workspaces: Vec<PathBuf>,
    pub(crate) _watcher_task: Option<Task<()>>,
    pub(crate) temp_save_tasks: HashMap<String, Task<()>>,
    pub(crate) temp_save_epochs: HashMap<String, usize>,
    pub(crate) auto_save_tasks: HashMap<String, Task<()>>,
    pub(crate) auto_save_epochs: HashMap<String, usize>,
    pub(crate) outline_rail_visible: bool,
    pub(crate) outline_rail_hovered: bool,
    pub(crate) outline_rail_clicked_active_line: Option<usize>,
    pub(crate) outline_rail_collapsed_heading_lines: HashSet<usize>,
    pub(crate) outline_headings_cache: RefCell<Vec<outline::OutlineHeading>>,
    pub(crate) outline_headings_cache_dirty: Cell<bool>,
    pub(crate) outline_panel_layout_epoch: usize,
    pub(crate) outline_panel_layout_cache: RefCell<outline::OutlinePanelLayoutCache>,
    pub(crate) outline_rail_previous_active_line: Cell<Option<usize>>,
    pub(crate) outline_rail_last_active_line: Cell<Option<usize>>,
    pub(crate) outline_rail_active_since: Cell<Option<Instant>>,
    pub(crate) outline_rail_hover_from: Cell<f32>,
    pub(crate) outline_rail_hover_target: Cell<f32>,
    pub(crate) outline_rail_hover_since: Cell<Option<Instant>>,
    /// Width (px, from the right viewport edge) occupied by the outline rail's
    /// interactive zone this frame, so toolbar-hover logic can treat hovering it
    /// as keep-visible. 0 when the rail is not shown.
    pub(crate) outline_rail_zone_from_right: Cell<f32>,
    pub(crate) info_panel_scroll_handle: ScrollHandle,
    pub(crate) git_history_scroll_handle: ScrollHandle,
    pub(crate) outline_panel_scroll_handle: ScrollHandle,
    pub(crate) info_panel_cache_tab_id: Option<String>,
    pub(crate) info_panel_cache_result: Option<Option<Arc<info_panel::InfoPanelData>>>,
    pub(crate) info_panel_refresh_epoch: usize,
    pub(crate) info_panel_refresh_task: Option<Task<()>>,
    pub(crate) git_history_cache_path: Option<PathBuf>,
    pub(crate) git_history_cache_result: Option<Option<Vec<GitHistoryItem>>>,
    pub(crate) git_history_cache_dirty: bool,
    pub(crate) git_history_has_more: bool,
    pub(crate) git_history_cache_scanned_at: Option<SystemTime>,
    pub(crate) git_history_refresh_epoch: usize,
    pub(crate) git_history_refresh_task: Option<Task<()>>,
    pub(crate) git_panel_tab: GitPanelTab,
    pub(crate) git_panel_snapshot: Option<GitPanelSnapshot>,
    pub(crate) git_panel_status_dirty: bool,
    pub(crate) git_panel_status_scanned_at: Option<SystemTime>,
    pub(crate) git_panel_status_epoch: usize,
    pub(crate) git_panel_status_task: Option<Task<()>>,
    pub(crate) git_operation_in_flight: Option<GitOperation>,
    pub(crate) git_commit_message: String,
    pub(crate) git_commit_message_editor: Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) git_changes_scroll_handle: ScrollHandle,
    pub(crate) git_history_scope: GitHistoryScope,
    pub(crate) git_history_expanded_commit: Option<String>,
    pub(crate) git_commit_files: Option<(String, Vec<GitCommitFileEntry>)>,
    pub(crate) git_commit_files_task: Option<Task<()>>,
    pub(crate) git_remote_input_visible: bool,
    pub(crate) git_remote_input_editor: Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) branch_picker_visible: bool,
    pub(crate) branch_picker_anchor: Option<Bounds<Pixels>>,
    pub(crate) status_bar_branch_button_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub(crate) git_panel_branch_button_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub(crate) branch_picker_query: String,
    pub(crate) branch_picker_all: Vec<String>,
    pub(crate) branch_picker_current: Option<String>,
    pub(crate) branch_picker_items: Vec<BranchPickerItem>,
    pub(crate) branch_picker_selected_index: usize,
    pub(crate) branch_picker_editor: Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) branch_picker_scroll_handle: ScrollHandle,
    pub(crate) utility_panel: Option<RightUtilityPanel>,
    pub(crate) displayed_utility_panel: Option<RightUtilityPanel>,
    pub(crate) info_panel_custom_width: Option<f32>,
    pub(crate) git_history_panel_custom_width: Option<f32>,
    pub(crate) outline_panel_custom_width: Option<f32>,
    pub(crate) utility_panel_resizing: bool,
    pub(crate) utility_panel_display_width: f32,
    pub(crate) utility_panel_layout_width: f32,
    pub(crate) utility_panel_width_target: f32,
    pub(crate) utility_panel_width_epoch: usize,
    pub(crate) utility_panel_width_task: Option<Task<()>>,
    pub(crate) theme_debugger_visible: bool,
    pub(crate) displayed_theme_debugger: bool,
    pub(crate) theme_debugger_display_width: f32,
    pub(crate) theme_debugger_width_target: f32,
    pub(crate) theme_debugger_width_epoch: usize,
    pub(crate) theme_debugger_width_task: Option<Task<()>>,
    pub(crate) theme_debugger_scroll_handle: ScrollHandle,
    pub(crate) theme_debugger_section: theme_debugger::ThemeDebuggerSection,
    pub(crate) theme_debugger_selected_field: ThemeColorField,
    pub(crate) theme_debugger_hex_editor: Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) theme_debugger_alpha_editor: Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) theme_debugger_drag_target: Option<theme_debugger::ThemeDebuggerDragTarget>,
    pub(crate) theme_debugger_sv_bounds:
        std::rc::Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
    pub(crate) theme_debugger_hue_bounds:
        std::rc::Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
    pub(crate) theme_debugger_alpha_bounds:
        std::rc::Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
    pub(crate) scroll_anim_epoch: usize,
    pub(crate) toolbar_hidden_by_scroll: bool,
    pub(crate) toolbar_corner_hovered: bool,
    pub(crate) hovered_toolbar_button: Option<&'static str>,
    pub(crate) toolbar_opacity: f32,
    pub(crate) toolbar_opacity_target: f32,
    pub(crate) toolbar_opacity_epoch: usize,
    pub(crate) toolbar_opacity_task: Option<Task<()>>,
    pub(crate) toolbar_hide_task: Option<Task<()>>,
    pub(crate) toolbar_corner_hide_task: Option<Task<()>>,
    pub(crate) last_window_width: f32,
    pub(crate) last_window_height: f32,

    // ── Inline (working-tree vs HEAD) diff overlay ──────────────────
    /// Set when the active editor buffer changed and its inline diff needs to
    /// be recomputed (consumed on the next render, which has a `Context`).
    pub(crate) inline_diff_dirty: bool,
    /// Set when the HEAD baseline must be re-read from git (file open, tab
    /// switch, or a git working-tree change) rather than reusing the cached one.
    pub(crate) inline_diff_base_dirty: bool,
    /// Supersede token so only the latest debounced recompute applies.
    pub(crate) inline_diff_epoch: u64,
    /// Hit areas for the left-edge diff indicators: `(bounds, hunk_index)`.
    pub(crate) inline_diff_gutter_areas: Vec<(gpui::Bounds<gpui::Pixels>, usize)>,
    pub(crate) hovering_inline_diff_gutter_hunk: Option<usize>,
    /// Full-width per-hunk regions used to reveal the hover "revert" button:
    /// `(bounds, hunk_index)`.
    pub(crate) inline_diff_hunk_areas: Vec<(gpui::Bounds<gpui::Pixels>, usize)>,
    /// Hit areas for the hover "revert" buttons: `(bounds, hunk_index)`.
    pub(crate) inline_diff_revert_areas: Vec<(gpui::Bounds<gpui::Pixels>, usize)>,
    /// Hunk currently hovered (controls revert-button visibility).
    pub(crate) hovering_inline_diff_hunk: Option<usize>,
    /// Revert button currently hovered (controls its highlight + cursor).
    pub(crate) hovering_inline_diff_revert_hunk: Option<usize>,
    /// Measured total height of the expanded removed rows anchored above / below
    /// a buffer line, keyed by that line. Filled during prepaint after the
    /// removed lines are shaped (they soft-wrap), and read by
    /// `block_padding_for_line` so the reserved band matches what is painted.
    pub(crate) inline_diff_phantom_height_above: HashMap<usize, f32>,
    pub(crate) inline_diff_phantom_height_below: HashMap<usize, f32>,
    /// Per-removed-row geometry for hit-testing read-only phantom-row selection.
    pub(crate) phantom_row_hits: Vec<PhantomRowHit>,
    /// Read-only character-level selection over an expanded hunk's removed rows.
    pub(crate) phantom_selection: Option<PhantomSelection>,
    /// True while a phantom-row selection is being dragged.
    pub(crate) phantom_selecting: bool,

    // ── Buffer search state ─────────────────────────────────────────
    pub(crate) search_visible: bool,
    pub(crate) search_query: String,
    pub(crate) search_replace: String,
    pub(crate) search_replace_visible: bool,
    pub(crate) search_case_sensitive: bool,
    pub(crate) search_whole_word: bool,
    pub(crate) search_use_regex: bool,
    pub(crate) search_matches: Vec<std::ops::Range<usize>>,
    pub(crate) search_current_index: Option<usize>,
    pub(crate) search_query_editor: Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) search_replace_editor: Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) pending_editor_focus: bool,

    // ── Global workspace search state ───────────────────────────────
    pub(crate) workspace_search_visible: bool,
    pub(crate) workspace_search_query: String,
    pub(crate) workspace_search_case_sensitive: bool,
    pub(crate) workspace_search_whole_word: bool,
    pub(crate) workspace_search_use_regex: bool,
    pub(crate) workspace_search_results: Vec<WorkspaceSearchResult>,
    pub(crate) workspace_search_staging_results: Vec<WorkspaceSearchResult>,
    pub(crate) workspace_search_selected_match: Option<usize>,
    pub(crate) workspace_search_highlight_path: Option<PathBuf>,
    pub(crate) workspace_search_highlight_ranges: Vec<std::ops::Range<usize>>,
    pub(crate) workspace_search_highlight_current_index: Option<usize>,
    pub(crate) outline_flash_highlight_path: Option<PathBuf>,
    pub(crate) outline_flash_highlight_ranges: Vec<std::ops::Range<usize>>,
    pub(crate) outline_flash_highlight_current_index: Option<usize>,
    pub(crate) outline_flash_epoch: usize,
    pub(crate) workspace_search_editor: Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) workspace_search_focus_sub: Option<gpui::Subscription>,
    pub(crate) workspace_search_blur_sub: Option<gpui::Subscription>,
    pub(crate) workspace_search_input_focused: bool,
    pub(crate) workspace_search_task: Option<Task<()>>,
    pub(crate) workspace_search_epoch: usize,
    pub(crate) workspace_search_pending_paths: Vec<PathBuf>,
    pub(crate) workspace_search_has_more: bool,
    pub(crate) workspace_search_scroll_handle: ScrollHandle,
    pub(crate) workspace_search_render_limit: usize,
    pub(crate) workspace_search_resizing: bool,
    /// tantivy candidate index for workspace search; built lazily in the
    /// background on first search and refreshed when the workspace changes.
    pub(crate) workspace_search_index: Option<std::sync::Arc<search_index::WorkspaceSearchIndex>>,
    pub(crate) workspace_search_index_task: Option<Task<()>>,
    pub(crate) workspace_search_index_epoch: usize,
    /// Workspace files changed since the index snapshot was built; always
    /// appended to index candidates so results never go stale.
    pub(crate) workspace_search_index_dirty_paths: HashSet<PathBuf>,

    // ── Quick open state ───────────────────────────────────────────
    pub(crate) quick_open_visible: bool,
    pub(crate) quick_open_query: String,
    pub(crate) quick_open_results: Vec<QuickOpenResult>,
    pub(crate) quick_open_selected_index: usize,
    pub(crate) quick_open_editor: Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) quick_open_task: Option<Task<()>>,
    pub(crate) quick_open_epoch: usize,
    pub(crate) quick_open_index_root: Option<PathBuf>,
    pub(crate) quick_open_file_index: Vec<PathBuf>,
    pub(crate) quick_open_scroll_handle: ScrollHandle,
    pub(crate) quick_open_render_limit: usize,
    pub(crate) command_palette_visible: bool,
    pub(crate) command_palette_query: String,
    pub(crate) command_palette_selected_index: usize,
    pub(crate) command_palette_editor: Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) command_palette_scroll_handle: ScrollHandle,
    pub(crate) command_palette_pending: Option<shortcut::CommandId>,
    pub(crate) language_picker_visible: bool,
    pub(crate) language_picker_mode: language_picker::LanguagePickerMode,
    pub(crate) language_picker_anchor: Option<Bounds<Pixels>>,
    pub(crate) status_bar_indent_button_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub(crate) status_bar_language_button_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    pub(crate) language_picker_query: String,
    pub(crate) language_picker_items: Vec<language_picker::LanguagePickerItem>,
    pub(crate) language_picker_selected_index: usize,
    pub(crate) language_picker_scroll_handle: ScrollHandle,
    pub(crate) language_picker_editor: Option<gpui::Entity<crate::text_input::TextInput>>,

    // ── Terminal tab state ─────────────────────────────────────────
    /// Per-terminal-tab runtime state keyed by `OpenTab::id`. Root-level (not
    /// part of the per-pane swap state) so terminal tabs can move between
    /// panes while their PTY sessions stay alive.
    pub(crate) terminal_tab_states: HashMap<String, chat_panel::TerminalTabState>,
    /// Terminal tab ids ordered from least to most recently focused.
    pub(crate) terminal_focus_history: Vec<String>,
    /// Terminal tab that should grab keyboard focus on the next render.
    pub(crate) pending_terminal_focus: Option<String>,
    pub(crate) pending_terminal_inputs: HashMap<String, Vec<u8>>,
    pub(crate) chat_terminal_cell_width_px: f32,
    pub(crate) chat_terminal_cell_height_px: f32,

    // Settings panel state
    pub(crate) settings_scroll_handle: ScrollHandle,
    pub(crate) settings_theme_dark_mode: bool,
    pub(crate) settings_font_picker: Option<settings_panel::FontPickerState>,
    pub(crate) settings_font_picker_editor: Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) settings_font_size_editor: Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) settings_ui_font_size_editor: Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) settings_line_height_editor: Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) settings_letter_spacing_editor: Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) settings_editor_content_width_editor:
        Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) settings_stepper_blur_subscriptions: Vec<gpui::Subscription>,
    pub(crate) settings_attachment_folder_editor:
        Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) settings_terminal_command_args_editor:
        Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) settings_terminal_font_size_editor:
        Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) font_picker_trigger_position: Option<gpui::Point<gpui::Pixels>>,
    pub(crate) font_picker_button_bounds: std::collections::HashMap<
        (crate::editor::settings_panel::FontRole, usize),
        std::rc::Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
    >,
    pub(crate) cached_font_names: Option<Vec<String>>,
    pub(crate) update_state: UpdateState,
    pub(crate) update_task: Option<Task<()>>,
    pub(crate) update_poll_task: Option<Task<()>>,
    /// Set while a user-initiated check runs, so its outcome is reported.
    pub(crate) update_check_is_manual: bool,
    /// Last observed OS appearance, used to drive `ThemeMode::Auto`.
    pub(crate) system_appearance: Option<gpui::WindowAppearance>,
    pub(crate) window_is_fullscreen: bool,
    pub(crate) ui_language: UiLanguage,
    pub(crate) app_settings: AppSettings,
    pub(crate) runtime_profiler: Option<profiler::RuntimeProfiler>,
}

impl Drop for EditorView {
    fn drop(&mut self) {
        if !self.embedded_editor {
            self.flush_recovery_files();
        }
        self.stop_all_terminal_sessions();
        let diagnostics = self.runtime_profile_diagnostics();
        if let Some(profiler) = self.runtime_profiler.take()
            && let Err(error) = profiler.stop_with_diagnostics("drop", Some(diagnostics))
        {
            log_error!("Failed to write runtime profile on shutdown: {error:#}");
        }
    }
}

impl EditorView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self::new_with_mode(cx, false, false, None)
    }

    fn new_with_mode(
        cx: &mut Context<Self>,
        embedded_editor: bool,
        embedded_read_only: bool,
        initial_text: Option<&str>,
    ) -> Self {
        let sidebar_root = PathBuf::new();
        let expanded_dirs = HashSet::new();
        let sidebar_error = None;
        let app_settings = AppSettings::load();
        let runtime_profiler = if embedded_editor {
            None
        } else {
            match profiler::RuntimeProfiler::start_from_env() {
                Ok(profiler) => profiler,
                Err(error) => {
                    log_error!("Failed to start runtime profiler from environment: {error:#}");
                    None
                }
            }
        };
        if !embedded_editor {
            settings_panel::apply_app_icon_to_dock();
            // Resolve the login-shell PATH early so terminal CLI detection
            // works even when the app was launched from Finder.
            chat_panel::prefetch_login_shell_path();
        }
        let ui_language = app_settings.ui_language();
        let mut welcome_tab = builtin_welcome_tab(ui_language);
        if let Some(text) = initial_text {
            welcome_tab.snapshot.text = text.to_string();
            welcome_tab.snapshot.cursor_offset = text.len();
            welcome_tab.snapshot.cursor_anchor = text.len();
        }
        let welcome_file_kind = welcome_tab.file_kind.clone();
        let buffer = Buffer::from_text(&welcome_tab.snapshot.text);
        // A non-embedded editor immediately restores its session (or opens the
        // launch file) and reparses, so parsing the placeholder document here
        // would be thrown away before the first frame.
        let decorations = if embedded_editor {
            markdown::parse_decorations(buffer.text())
        } else {
            Vec::new()
        };
        let theme_kind = app_settings.theme_kind();
        let editor_theme_mode = app_settings.editor_theme_mode_for_appearance(theme_kind.is_dark());
        let theme = crate::theme::apply_editor_theme_mode(
            &theme_kind.build(),
            editor_theme_mode,
            theme_kind.is_dark(),
        );
        let ui_theme = app_settings.ui_theme_for(theme_kind);
        let mut styles = crate::style::Styles::from_theme_with_options(
            &theme,
            &ui_theme,
            app_settings.sidebar_always_dark,
        );
        if theme_kind.is_moeka_retro() {
            styles.apply_moeka_retro_chrome(&theme, &ui_theme);
        }
        // Keep Markdown headings at their original sizes while rendering body
        // text one point smaller than the configured legacy size.
        styles.editor.editor_font_size_px = app_settings.effective_editor_font_size();
        styles.editor.editor_paragraph_line_height_factor = app_settings.line_height;
        styles.editor.editor_heading_line_height_factor = app_settings.line_height;
        styles.editor.editor_letter_spacing_px = app_settings.letter_spacing;
        styles.editor.editor_max_width_px = app_settings.effective_editor_content_width();
        styles.ui_font_family = Box::leak(app_settings.ui_font.clone().into_boxed_str());
        styles.editor.editor_body_latin_font_family =
            Box::leak(app_settings.body_latin_font.clone().into_boxed_str());
        styles.editor.editor_body_cjk_font_family =
            Box::leak(app_settings.body_cjk_font.clone().into_boxed_str());
        styles.editor.editor_heading_latin_font_family =
            styles.editor.editor_body_latin_font_family;
        styles.editor.editor_heading_cjk_font_family = styles.editor.editor_body_cjk_font_family;
        styles.editor.editor_monospace_font_family =
            Box::leak(app_settings.code_font.clone().into_boxed_str());
        if embedded_editor {
            // Panel-embedded editors (agent transcript/composer) use compact
            // chrome: tight padding, full-width wrapping, and a smaller body
            // font tuned for the utility panel.
            styles.editor.editor_font_size_px = 12.0;
            styles.editor.editor_h1_font_size_px = 15.0;
            styles.editor.editor_h2_font_size_px = 14.0;
            styles.editor.editor_h3_font_size_px = 13.0;
            styles.editor.editor_h4_font_size_px = 12.0;
            styles.editor.editor_h5_font_size_px = 12.0;
            styles.editor.editor_h6_font_size_px = 12.0;
            if embedded_read_only {
                styles.editor.editor_padding_left_px = 12.0;
                styles.editor.editor_padding_right_px = 12.0;
                styles.editor.editor_padding_top_px = 10.0;
                styles.editor.editor_padding_bottom_px = 12.0;
            } else {
                // Composer: compact, equal padding on all four sides.
                styles.editor.editor_padding_left_px = 8.0;
                styles.editor.editor_padding_right_px = 8.0;
                styles.editor.editor_padding_top_px = 8.0;
                styles.editor.editor_padding_bottom_px = 8.0;
            }
            styles.editor.editor_max_width_px = 4096.0;
            styles.editor.editor_paragraph_line_height_factor = 1.5;
            styles.editor.editor_heading_line_height_factor = 1.4;
        }
        let init_max_width = styles.editor.editor_max_width_px;
        let init_padding_left = styles.editor.editor_padding_left_px;
        let init_padding_right = styles.editor.editor_padding_right_px;
        let init_cursor_opacity = styles.cursor.cursor_opacity_max;
        let init_scrollbar_opacity = styles.editor.editor_scrollbar_idle_opacity;
        let init_scrollbar_width = styles.editor.editor_scrollbar_width_px;
        let mut this = Self {
            buffer,
            cursor: Cursor::new(),
            extra_cursors: Vec::new(),
            focus_handle: cx.focus_handle(),
            embedded_editor,
            embedded_content_height: Cell::new(0.0),
            embedded_stick_to_bottom: true,
            embedded_read_only,
            theme_kind,
            editor_theme_mode,
            theme,
            styles,
            decorations,
            lazy_markdown_decorations: None,
            markdown_outline_index: RefCell::new(None),
            markdown_outline_build_task: None,
            markdown_outline_epoch: 0,
            large_document_frame_bench: None,
            scroll_offset: 0.0,
            is_selecting: false,
            vertical_move_goal_x: None,
            last_scroll_cursor_visual_x: None,
            wrap_boundary_affinity: None,
            line_display_info: Vec::new(),
            visible_line_geometry: Vec::new(),
            pending_line_display_transfer: None,
            line_display_trim_update_counter: Cell::new(0),
            line_geometry_cache_dirty: Cell::new(true),
            line_geometry_cache: RefCell::new(LineGeometryCacheState::default()),
            indent_guide_step_cache_dirty: Cell::new(true),
            indent_guide_step_cache: Cell::new(4),
            layout_dirty: false,
            pending_scroll_to_cursor: false,
            pending_center_cursor_in_view: false,
            pending_center_cursor_in_view_immediately: false,
            center_cursor_confirm_frames: 0,
            kill_ring: None,
            pending_preview_reveal: None,
            pending_typing_scroll_guard: None,
            pending_click_anchor: None,
            pending_history_transaction: None,
            code_mode_preview_key: None,
            generated_preview_rerender_task: None,
            generated_preview_rerender_pending: false,
            last_viewport_height: 600.0,
            last_viewport_width: init_max_width,
            whiteboard_canvas_bounds: None,
            whiteboard_drag: None,
            whiteboard_pointer: None,
            whiteboard_space_down: false,
            whiteboard_bind_targets: Vec::new(),
            whiteboard_text_editor: None,
            cursor_opacity: init_cursor_opacity,
            blink_epoch: 0,
            cursor_blink_allowed: false,
            marked_range: None,
            extra_marked_ranges: Vec::new(),
            ime_composition_snapshot: None,
            checkbox_areas: Vec::new(),
            hovering_checkbox: false,
            hovering_checkbox_line: None,
            image_click_areas: Vec::new(),
            image_preview_zoom_areas: Vec::new(),
            generated_preview_click_areas: Vec::new(),
            generated_preview_image_areas: Vec::new(),
            generated_preview_zoom_areas: Vec::new(),
            generated_preview_action_areas: Vec::new(),
            preview_overlay_action_areas: Vec::new(),
            hovering_image_preview_line: None,
            hovering_generated_preview_line: None,
            hovering_generated_preview_zoom_line: None,
            hovering_generated_preview_action: None,
            hovering_preview_overlay_action: None,
            generated_preview_actions_visible_line: None,
            generated_preview_actions_opacity: 0.0,
            generated_preview_actions_opacity_target: 0.0,
            generated_preview_actions_opacity_epoch: 0,
            generated_preview_actions_opacity_task: None,
            code_block_hover_areas: Vec::new(),
            code_block_copy_areas: Vec::new(),
            hovering_code_block_start: None,
            hovering_code_block_copy_start: None,
            copied_code_block_start: None,
            code_block_copy_feedback_epoch: 0,
            table_click_areas: Vec::new(),
            table_col_handle_areas: Vec::new(),
            table_row_handle_areas: Vec::new(),
            table_drag: None,
            hovering_table_start: None,
            hovering_handle: None,
            selected_handle: None,
            image_cache: HashMap::new(),
            generated_preview_svg_cache: HashMap::new(),
            generated_preview_overlay_cache: HashMap::new(),
            generated_preview_views: HashMap::new(),
            generated_preview_drag_state: None,
            sidebar_root: sidebar_root.clone(),
            workspace_bar_bounds: Rc::new(Cell::new(None)),
            expanded_dirs,
            selected_sidebar_paths: HashSet::new(),
            current_file_path: None,
            sidebar_error,
            tabs: vec![welcome_tab],
            tab_layout_cache: HashMap::new(),
            tab_generated_preview_cache: HashMap::new(),
            active_tab_id: Some(WELCOME_TAB_ID.to_string()),
            active_tab_index_cache: Cell::new(Some(0)),
            active_file_kind_cache: welcome_file_kind,
            next_tab_id: 1,
            panes: vec![Pane::new(0)],
            pane_layout: PaneLayout::Leaf(0),
            focused_pane_id: 0,
            swapped_pane_id: None,
            next_pane_id: 1,
            pane_bounds: HashMap::new(),
            pane_editor_content_views: HashMap::new(),
            pane_caret_overlay_views: HashMap::new(),
            tab_drag_split_target: None,
            context_menu_pane_target: None,
            pane_resize_drag: None,
            last_editor_left: 0.0,
            last_editor_width: init_max_width,
            last_content_left: init_padding_left,
            last_text_width: init_max_width - init_padding_left - init_padding_right,
            last_window_scale_factor: 1.0,
            content_origin: Point::default(),
            last_stable_input_bounds: None,
            pending_input_bounds: None,
            preview_overlay: None,
            preview_overlay_bounds: None,
            preview_overlay_image_bounds: None,
            last_mouse_down_target: None,
            selection_drag_has_moved: false,
            context_menu: None,
            suppress_next_editor_mouse_up: false,
            context_submenu: None,
            context_submenu_aim: None,
            context_submenu_hovered: false,
            format_heading_trigger_bounds: Rc::new(Cell::new(None)),
            format_more_trigger_bounds: Rc::new(Cell::new(None)),
            format_toolbar_heading_bounds: Rc::new(Cell::new(None)),
            export_trigger_bounds: Rc::new(Cell::new(None)),
            context_menu_opacity: 0.0,
            context_menu_opacity_target: 0.0,
            context_menu_opacity_epoch: 0,
            context_menu_opacity_task: None,
            sidebar_edit: None,
            sidebar_edit_blur_sub: None,
            file_clipboard: None,
            multi_cursor_clipboard: None,
            ui_scale: app_settings.ui_scale,
            #[cfg(target_os = "macos")]
            traffic_light_ui_scale: None,
            #[cfg(not(target_os = "macos"))]
            window_is_maximized: false,
            #[cfg(target_os = "linux")]
            client_side_window_decorations: false,
            folded_sections: HashSet::new(),
            folded_image_previews: HashSet::new(),
            generated_preview_keys: HashSet::new(),
            gutter_fold_areas: Vec::new(),
            image_edit_areas: Vec::new(),
            hovering_image_edit_line: None,
            hovering_fold_line: None,
            hovering_gutter_fold_area: false,
            gutter_fold_reveal: 0.0,
            gutter_fold_reveal_target: 0.0,
            gutter_fold_reveal_epoch: 0,
            gutter_fold_reveal_task: None,
            image_drag_state: None,
            command_held: false,
            hovered_editor_position: None,
            editor_scrollbar_hovered: false,
            editor_scrollbar_revealed: false,
            editor_scrollbar_opacity: init_scrollbar_opacity,
            editor_scrollbar_opacity_target: init_scrollbar_opacity,
            editor_scrollbar_opacity_epoch: 0,
            editor_scrollbar_opacity_task: None,
            editor_scrollbar_width: init_scrollbar_width,
            editor_scrollbar_width_target: init_scrollbar_width,
            editor_scrollbar_width_epoch: 0,
            editor_scrollbar_width_task: None,
            editor_scrollbar_idle_epoch: 0,
            editor_scrollbar_idle_task: None,
            editor_scrollbar_drag: None,
            pending_editor_scrollbar_drag_position: None,
            editor_scrollbar_drag_task: None,
            panel_scrollbars: PanelScrollbarKind::ALL
                .into_iter()
                .map(|kind| {
                    (
                        kind,
                        PanelScrollbarState::new(init_scrollbar_opacity, init_scrollbar_width),
                    )
                })
                .collect(),
            sidebar_custom_width: None,
            last_typing_checkpoint_time: None,
            last_typing_checkpoint_offset: None,
            sidebar_resizing: false,
            sidebar_display_width: 0.0,
            sidebar_layout_width: 0.0,
            sidebar_width_target: 0.0,
            sidebar_width_epoch: 0,
            sidebar_width_task: None,
            heading_submenu_open: false,
            menu_selected_index: 0,
            slash_menu_start: None,
            slash_filter: String::new(),
            wiki_link_start: None,
            wiki_link_query: String::new(),
            wiki_link_recent_results: Vec::new(),
            wiki_link_search_results: Vec::new(),
            wiki_link_recent_task: None,
            wiki_link_search_task: None,
            sidebar_visible: false,
            editor_left_aligned: false,
            indent_settings: IndentSettings::default(),
            tab_drag_candidate_id: None,
            tab_drag_press_position: None,
            tab_dragging_id: None,
            recently_closed_tabs: Vec::new(),
            tab_navigation_history: vec![TabNavigationTarget::LiveTab(WELCOME_TAB_ID.to_string())],
            tab_navigation_index: 0,
            tab_navigation_suspended: false,
            dialog: None,
            notifications: Vec::new(),
            next_notification_id: 0,
            pending_close_tab_ids: Vec::new(),
            focused_sidebar_path: None,
            suppress_next_sidebar_sync_path: None,
            sidebar_selection_anchor: None,
            sidebar_sort_mode: app_settings.sidebar_sort_mode(),
            file_list_display_mode: app_settings.file_list_display_mode(),
            sidebar_starred_collapsed: false,
            sidebar_drag_hover_target: None,
            sidebar_drag_expand_hover_dir: None,
            sidebar_drag_child_hover_position: None,
            sidebar_drag_scroll_task: None,
            selection_drag_scroll_task: None,
            selection_drag_position: None,
            suppress_fold_for_empty_selection_drag: false,
            sidebar_drag_expand_task: None,
            sidebar_drag_auto_expanded_dirs: HashSet::new(),
            sidebar_items_cache: Rc::new(Vec::new()),
            sidebar_items_dirty: true,
            sidebar_directory_children: HashMap::new(),
            sidebar_directory_loading: HashSet::new(),
            sidebar_dir_file_counts: HashMap::new(),
            sidebar_dir_file_counts_loading: HashSet::new(),
            sidebar_dir_file_counts_dirty: HashSet::new(),
            sidebar_directory_epoch: 0,
            sidebar_directory_refresh_pending: false,
            sidebar_scroll_handle: ScrollHandle::new(),
            pending_sidebar_scroll_top: Rc::new(Cell::new(None)),
            pending_sidebar_scroll_anchor: None,
            pending_sidebar_reveal_path: None,
            tab_bar_scroll_handle: ScrollHandle::new(),
            file_list_dir: PathBuf::new(),
            file_list_items: Vec::new(),
            file_list_dirty: true,
            file_list_loaded_dir: None,
            file_list_epoch: 0,
            file_list_task: None,
            file_list_detail_task: None,
            file_list_scroll_handle: ScrollHandle::new(),
            pending_file_list_scroll_top: Rc::new(Cell::new(None)),
            file_list_custom_width: app_settings.file_list_width,
            file_list_visible: app_settings.file_list_visible,
            file_list_display_width: 0.0,
            file_list_layout_width: 0.0,
            file_list_width_target: 0.0,
            file_list_width_epoch: 0,
            file_list_width_task: None,
            file_list_resizing: false,
            file_list_resize_grab_offset_x: 0.0,
            file_list_pending_reveal_path: None,
            file_list_pending_reveal_centered: false,
            sidebar_edit_in_file_list: false,
            file_list_menu_origin: false,
            sidebar_git_statuses: HashMap::new(),
            sidebar_git_statuses_dirty: true,
            sidebar_git_statuses_scanned_at: None,
            sidebar_git_statuses_refresh_epoch: 0,
            sidebar_git_statuses_task: None,
            workspaces: Vec::new(),
            _watcher_task: None,
            temp_save_tasks: HashMap::new(),
            temp_save_epochs: HashMap::new(),
            auto_save_tasks: HashMap::new(),
            auto_save_epochs: HashMap::new(),
            outline_rail_visible: app_settings.outline_rail_visible,
            outline_rail_hovered: false,
            outline_rail_clicked_active_line: None,
            outline_rail_collapsed_heading_lines: HashSet::new(),
            outline_headings_cache: RefCell::new(Vec::new()),
            outline_headings_cache_dirty: Cell::new(true),
            outline_panel_layout_epoch: 0,
            outline_panel_layout_cache: RefCell::new(outline::OutlinePanelLayoutCache::default()),
            outline_rail_previous_active_line: Cell::new(None),
            outline_rail_last_active_line: Cell::new(None),
            outline_rail_active_since: Cell::new(None),
            outline_rail_hover_from: Cell::new(0.0),
            outline_rail_hover_target: Cell::new(0.0),
            outline_rail_hover_since: Cell::new(None),
            outline_rail_zone_from_right: Cell::new(0.0),
            info_panel_scroll_handle: ScrollHandle::new(),
            git_history_scroll_handle: ScrollHandle::new(),
            outline_panel_scroll_handle: ScrollHandle::new(),
            info_panel_cache_tab_id: None,
            info_panel_cache_result: None,
            info_panel_refresh_epoch: 0,
            info_panel_refresh_task: None,
            git_history_cache_path: None,
            git_history_cache_result: None,
            git_history_cache_dirty: true,
            git_history_has_more: false,
            git_history_cache_scanned_at: None,
            git_history_refresh_epoch: 0,
            git_history_refresh_task: None,
            git_panel_tab: GitPanelTab::Changes,
            git_panel_snapshot: None,
            git_panel_status_dirty: true,
            git_panel_status_scanned_at: None,
            git_panel_status_epoch: 0,
            git_panel_status_task: None,
            git_operation_in_flight: None,
            git_commit_message: String::new(),
            git_commit_message_editor: None,
            git_changes_scroll_handle: ScrollHandle::new(),
            git_history_scope: GitHistoryScope::Repository,
            git_history_expanded_commit: None,
            git_commit_files: None,
            git_commit_files_task: None,
            git_remote_input_visible: false,
            git_remote_input_editor: None,
            branch_picker_visible: false,
            branch_picker_anchor: None,
            status_bar_branch_button_bounds: Rc::new(Cell::new(None)),
            git_panel_branch_button_bounds: Rc::new(Cell::new(None)),
            branch_picker_query: String::new(),
            branch_picker_all: Vec::new(),
            branch_picker_current: None,
            branch_picker_items: Vec::new(),
            branch_picker_selected_index: 0,
            branch_picker_editor: None,
            branch_picker_scroll_handle: ScrollHandle::new(),
            utility_panel: None,
            displayed_utility_panel: None,
            info_panel_custom_width: None,
            git_history_panel_custom_width: None,
            outline_panel_custom_width: None,
            utility_panel_resizing: false,
            utility_panel_display_width: 0.0,
            utility_panel_layout_width: 0.0,
            utility_panel_width_target: 0.0,
            utility_panel_width_epoch: 0,
            utility_panel_width_task: None,
            theme_debugger_visible: false,
            displayed_theme_debugger: false,
            theme_debugger_display_width: 0.0,
            theme_debugger_width_target: 0.0,
            theme_debugger_width_epoch: 0,
            theme_debugger_width_task: None,
            theme_debugger_scroll_handle: ScrollHandle::new(),
            theme_debugger_section: theme_debugger::ThemeDebuggerSection::Interface,
            theme_debugger_selected_field: ThemeColorField::TextPrimary,
            theme_debugger_hex_editor: None,
            theme_debugger_alpha_editor: None,
            theme_debugger_drag_target: None,
            theme_debugger_sv_bounds: std::rc::Rc::new(std::cell::Cell::new(None)),
            theme_debugger_hue_bounds: std::rc::Rc::new(std::cell::Cell::new(None)),
            theme_debugger_alpha_bounds: std::rc::Rc::new(std::cell::Cell::new(None)),
            scroll_anim_epoch: 0,
            toolbar_hidden_by_scroll: false,
            toolbar_corner_hovered: false,
            hovered_toolbar_button: None,
            toolbar_opacity: 1.0,
            toolbar_opacity_target: 1.0,
            toolbar_opacity_epoch: 0,
            toolbar_opacity_task: None,
            toolbar_hide_task: None,
            toolbar_corner_hide_task: None,
            last_window_width: 1360.0,
            last_window_height: 820.0,

            inline_diff_dirty: false,
            inline_diff_base_dirty: false,
            inline_diff_epoch: 0,
            inline_diff_gutter_areas: Vec::new(),
            hovering_inline_diff_gutter_hunk: None,
            inline_diff_hunk_areas: Vec::new(),
            inline_diff_revert_areas: Vec::new(),
            hovering_inline_diff_hunk: None,
            hovering_inline_diff_revert_hunk: None,
            inline_diff_phantom_height_above: HashMap::new(),
            inline_diff_phantom_height_below: HashMap::new(),
            phantom_row_hits: Vec::new(),
            phantom_selection: None,
            phantom_selecting: false,

            search_visible: false,
            search_query: String::new(),
            search_replace: String::new(),
            search_replace_visible: false,
            search_case_sensitive: false,
            search_whole_word: false,
            search_use_regex: false,
            search_matches: Vec::new(),
            search_current_index: None,
            search_query_editor: None,
            search_replace_editor: None,
            pending_editor_focus: false,

            workspace_search_visible: false,
            workspace_search_query: String::new(),
            workspace_search_case_sensitive: false,
            workspace_search_whole_word: false,
            workspace_search_use_regex: false,
            workspace_search_results: Vec::new(),
            workspace_search_staging_results: Vec::new(),
            workspace_search_selected_match: None,
            workspace_search_highlight_path: None,
            workspace_search_highlight_ranges: Vec::new(),
            workspace_search_highlight_current_index: None,
            outline_flash_highlight_path: None,
            outline_flash_highlight_ranges: Vec::new(),
            outline_flash_highlight_current_index: None,
            outline_flash_epoch: 0,
            workspace_search_editor: None,
            workspace_search_focus_sub: None,
            workspace_search_blur_sub: None,
            workspace_search_input_focused: false,
            workspace_search_task: None,
            workspace_search_epoch: 0,
            workspace_search_pending_paths: Vec::new(),
            workspace_search_has_more: false,
            workspace_search_scroll_handle: ScrollHandle::new(),
            workspace_search_render_limit: list_viewport::SEARCH_LIST_POLICY.initial_count,
            workspace_search_resizing: false,
            workspace_search_index: None,
            workspace_search_index_task: None,
            workspace_search_index_epoch: 0,
            workspace_search_index_dirty_paths: HashSet::new(),

            quick_open_visible: false,
            quick_open_query: String::new(),
            quick_open_results: Vec::new(),
            quick_open_selected_index: 0,
            quick_open_editor: None,
            quick_open_task: None,
            quick_open_epoch: 0,
            quick_open_index_root: None,
            quick_open_file_index: Vec::new(),
            quick_open_scroll_handle: ScrollHandle::new(),
            quick_open_render_limit: list_viewport::SEARCH_LIST_POLICY.initial_count,
            command_palette_visible: false,
            command_palette_query: String::new(),
            command_palette_selected_index: 0,
            command_palette_editor: None,
            command_palette_scroll_handle: ScrollHandle::new(),
            command_palette_pending: None,
            language_picker_visible: false,
            language_picker_mode: language_picker::LanguagePickerMode::Language,
            language_picker_anchor: None,
            status_bar_indent_button_bounds: Rc::new(Cell::new(None)),
            status_bar_language_button_bounds: Rc::new(Cell::new(None)),
            language_picker_query: String::new(),
            language_picker_items: Vec::new(),
            language_picker_selected_index: 0,
            language_picker_scroll_handle: ScrollHandle::new(),
            language_picker_editor: None,
            terminal_tab_states: HashMap::new(),
            terminal_focus_history: Vec::new(),
            pending_terminal_focus: None,
            pending_terminal_inputs: HashMap::new(),
            chat_terminal_cell_width_px: 0.0,
            chat_terminal_cell_height_px: 0.0,

            // Settings panel state
            settings_scroll_handle: ScrollHandle::new(),
            settings_theme_dark_mode: theme_kind.is_dark(),
            settings_font_picker: None,
            settings_font_picker_editor: None,
            settings_font_size_editor: None,
            settings_ui_font_size_editor: None,
            settings_line_height_editor: None,
            settings_letter_spacing_editor: None,
            settings_editor_content_width_editor: None,
            settings_stepper_blur_subscriptions: Vec::new(),
            settings_attachment_folder_editor: None,
            settings_terminal_command_args_editor: None,
            settings_terminal_font_size_editor: None,
            font_picker_trigger_position: None,
            font_picker_button_bounds: std::collections::HashMap::new(),
            cached_font_names: None,
            update_state: UpdateState::Idle,
            update_task: None,
            update_poll_task: None,
            update_check_is_manual: false,
            system_appearance: None,
            window_is_fullscreen: false,
            ui_language,
            app_settings,
            runtime_profiler,
        };
        this.last_content_left = this.padding_left();
        this.last_text_width =
            (init_max_width - this.padding_left() - this.padding_right()).max(this.scaled(120.0));
        if !embedded_editor {
            session_store::restore_session(&mut this);
            if this.decorations.is_empty() {
                this.reparse();
            }
            this.refresh_builtin_tab_titles();
            this.sync_panel_display_state();
            this.start_update_polling(cx);
        } else {
            this.editor_left_aligned = true;
            this.cursor.offset = this.buffer.len();
            this.cursor.anchor = this.buffer.len();
        }
        this.start_blinking(cx);
        if !embedded_editor && !this.sidebar_root.as_os_str().is_empty() {
            this.start_watching(cx);
        }
        if !embedded_editor {
            this.ensure_font_size_editor(cx);
            this.ensure_ui_font_size_editor(cx);
        }
        this.toolbar_opacity = this.desired_toolbar_opacity();
        this.toolbar_opacity_target = this.toolbar_opacity;
        // Warm up the syntect syntax set off the UI thread so the first code
        // block render doesn't pay the grammar-loading cost.
        if !embedded_editor {
            cx.background_executor()
                .spawn(async {
                    let _ = helpers::syntax_set();
                })
                .detach();
            let (_, keybinding_warnings) =
                shortcut::keybinding_overrides(&this.app_settings.keybindings);
            for warning in keybinding_warnings {
                let message = format!("{}: {warning}", this.t(TextKey::OperationFailed));
                this.notify_error(message, cx);
            }
        }

        this
    }

    pub(crate) fn t(&self, key: TextKey) -> &'static str {
        crate::i18n::translate(self.ui_language, key)
    }

    /// Global icon tooltips are hidden while any menu-like overlay is open.
    pub(crate) fn global_tooltips_suppressed(&self) -> bool {
        self.context_menu.is_some()
    }

    pub(crate) fn refresh_builtin_tab_titles(&mut self) {
        let welcome_title = self.t(TextKey::WelcomeTitle).to_string();
        let settings_title = self.t(TextKey::SettingsTitle).to_string();
        for tab in self.all_tabs_mut() {
            if tab.kind == TabKind::Settings {
                tab.title = settings_title.clone();
            } else if tab.id == WELCOME_TAB_ID && tab.path.is_none() {
                tab.title = welcome_title.clone();
            }
        }
    }

    pub(crate) fn refresh_welcome_tab_content(&mut self, cx: &mut Context<Self>) {
        let text = default_document_text(self.ui_language);
        let first_line_end = text.find('\n').unwrap_or(text.len());
        let active_welcome = self
            .active_tab_index()
            .and_then(|index| self.tabs.get(index))
            .is_some_and(|tab| tab.id == WELCOME_TAB_ID && tab.path.is_none());

        for tab in self.all_tabs_mut() {
            if tab.id == WELCOME_TAB_ID && tab.path.is_none() {
                tab.snapshot.text = text.clone();
                tab.snapshot.cursor_offset = first_line_end;
                tab.snapshot.cursor_anchor = first_line_end;
                tab.snapshot.scroll_offset = 0.0;
                tab.snapshot.extra_cursors.clear();
                tab.is_dirty = false;
            }
        }

        if active_welcome {
            self.buffer = Buffer::from_text(&text);
            self.cursor.move_to(first_line_end, &self.buffer);
            self.extra_cursors.clear();
            self.scroll_offset = 0.0;
            self.reparse();
        }
        session_store::persist_session(self);
        cx.notify();
    }

    pub(crate) fn apply_ui_language(&mut self, language: UiLanguage, cx: &mut Context<Self>) {
        if self.ui_language == language {
            self.close_font_picker(cx);
            return;
        }
        self.ui_language = language;
        self.app_settings.language = language.code().to_string();
        self.refresh_builtin_tab_titles();
        self.refresh_welcome_tab_content(cx);
        cx.set_menus(shortcut::app_menus(language));
        cx.set_dock_menu(shortcut::dock_menu(language));
        self.finalize_settings_change(cx);
    }

    pub(crate) fn should_show_editor_toolbar(&self, base_visible: bool) -> bool {
        base_visible
            && (self.scroll_offset <= 0.5
                || !self.toolbar_hidden_by_scroll
                || self.toolbar_corner_hovered
                || self.outline_rail_hovered)
    }

    fn toolbar_base_visible(&self) -> bool {
        !self.app_settings.simple_mode
            && !self.tabs.is_empty()
            && !self.active_tab_is_settings()
            && !self.active_tab_is_binary()
            && !self.active_tab_is_terminal()
    }

    fn desired_toolbar_opacity(&self) -> f32 {
        if self.should_show_editor_toolbar(self.toolbar_base_visible()) {
            1.0
        } else {
            self.styles.toolbar.toolbar_hidden_opacity
        }
    }

    pub(crate) fn sync_toolbar_opacity(&mut self, cx: &mut Context<Self>) {
        let target = self.desired_toolbar_opacity();
        if (self.toolbar_opacity_target - target).abs() <= 0.001 {
            return;
        }

        self.toolbar_opacity_target = target;
        self.toolbar_opacity_epoch += 1;
        let epoch = self.toolbar_opacity_epoch;
        let duration_ms = self.styles.toolbar.toolbar_opacity_animation_ms.max(1);
        let step = (16.0 / duration_ms as f32).clamp(0.08, 1.0);

        self.toolbar_opacity_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let done = this
                    .update(cx, |this, cx| {
                        if this.toolbar_opacity_epoch != epoch {
                            return true;
                        }

                        let diff = this.toolbar_opacity_target - this.toolbar_opacity;
                        if diff.abs() <= 0.01 {
                            this.toolbar_opacity = this.toolbar_opacity_target;
                            this.toolbar_opacity_task = None;
                            cx.notify();
                            return true;
                        }

                        this.toolbar_opacity += diff * step;
                        cx.notify();
                        false
                    })
                    .unwrap_or(true);

                if done {
                    return;
                }

                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
            }
        }));
    }

    /// Updates toolbar visibility in response to a scroll change. Crossing away
    /// from the top immediately starts the opacity animation; returning to the
    /// top immediately starts the reveal animation.
    pub(crate) fn update_toolbar_visibility_for_scroll(&mut self, cx: &mut Context<Self>) {
        if self.scroll_offset <= 0.5 {
            self.toolbar_hide_task = None;
            if self.toolbar_hidden_by_scroll {
                self.toolbar_hidden_by_scroll = false;
                self.sync_toolbar_opacity(cx);
            }
            return;
        }

        self.toolbar_hide_task = None;
        let visibility_changed = !self.toolbar_hidden_by_scroll || self.toolbar_corner_hovered;
        self.toolbar_hidden_by_scroll = true;
        self.toolbar_corner_hovered = false;
        if visibility_changed {
            self.sync_toolbar_opacity(cx);
            cx.notify();
        }
    }

    fn editor_scrollbar_can_show(&self) -> bool {
        !self.active_tab_is_image()
            && !self.active_tab_is_binary()
            && !self.active_tab_is_terminal()
            && self.editor_scrollbar_max_scroll() > 1.0
    }

    fn editor_scrollbar_track_top(&self) -> f32 {
        self.scaled(self.styles.editor.editor_scrollbar_top_margin_px)
    }

    fn editor_scrollbar_track_bottom_margin(&self) -> f32 {
        self.scaled(self.styles.editor.editor_scrollbar_bottom_margin_px)
    }

    fn editor_scrollbar_track_height(&self) -> f32 {
        (self.last_viewport_height.max(1.0)
            - self.editor_scrollbar_track_top()
            - self.editor_scrollbar_track_bottom_margin())
        .max(1.0)
    }

    fn editor_scrollbar_hit_width(&self) -> f32 {
        self.scaled(
            self.styles
                .editor
                .editor_scrollbar_hit_width_px
                .max(self.styles.editor.editor_scrollbar_width_px)
                .max(self.styles.editor.editor_scrollbar_hover_width_px),
        )
    }

    fn desired_editor_scrollbar_width(&self) -> f32 {
        if self.editor_scrollbar_hovered || self.editor_scrollbar_drag.is_some() {
            self.styles.editor.editor_scrollbar_hover_width_px
        } else {
            self.styles.editor.editor_scrollbar_width_px
        }
    }

    pub(crate) fn editor_scrollbar_region_contains(&self, position: Point<Pixels>) -> bool {
        if !self.editor_scrollbar_can_show() {
            return false;
        }

        let left = f32::from(self.content_origin.x);
        let top = f32::from(self.content_origin.y);
        let right = left + self.last_viewport_width.max(1.0);
        let bottom =
            top + self.last_viewport_height.max(1.0) - self.editor_scrollbar_track_bottom_margin();
        let x = f32::from(position.x);
        let y = f32::from(position.y);
        let hit_width = self.editor_scrollbar_hit_width();
        let right_margin = self.scaled(self.styles.editor.editor_scrollbar_right_margin_px);
        let track_top = top + self.editor_scrollbar_track_top();
        x >= right - right_margin - hit_width && x <= right && y >= track_top && y <= bottom
    }

    pub(crate) fn editor_scrollbar_thumb_geometry(&self) -> Option<(f32, f32)> {
        if !self.editor_scrollbar_can_show() {
            return None;
        }

        let track_top = self.editor_scrollbar_track_top();
        let viewport_height = self.editor_scrollbar_track_height();
        let total_content_height = if self.embedded_editor {
            self.total_content_height()
                .max(self.embedded_content_height.get())
        } else {
            self.total_content_height()
        }
        .max(1.0);
        let min_thumb_height = self.scaled(self.styles.editor.editor_scrollbar_min_thumb_height_px);
        let thumb_height = (viewport_height * (viewport_height / total_content_height))
            .clamp(min_thumb_height, viewport_height.max(min_thumb_height));

        let max_scroll = self.editor_scrollbar_max_scroll();
        if max_scroll <= 0.0 {
            return None;
        }

        let track_travel = (viewport_height - thumb_height).max(0.0);
        let thumb_top =
            track_top + (self.scroll_offset / max_scroll).clamp(0.0, 1.0) * track_travel;
        Some((thumb_top, thumb_height))
    }

    fn editor_scrollbar_max_scroll(&self) -> f32 {
        // Single source of truth shared with `clamp_scroll`, so the thumb
        // range always matches the actual scrollable range.
        self.max_scroll_offset()
    }

    fn desired_editor_scrollbar_opacity(&self) -> f32 {
        if !self.editor_scrollbar_can_show() {
            0.0
        } else if self.editor_scrollbar_hovered || self.editor_scrollbar_drag.is_some() {
            self.styles.editor.editor_scrollbar_hover_opacity
        } else if self.editor_scrollbar_revealed {
            self.styles.editor.editor_scrollbar_opacity
        } else {
            self.styles.editor.editor_scrollbar_idle_opacity
        }
    }

    fn sync_editor_scrollbar_opacity(&mut self, cx: &mut Context<Self>) {
        let target = self.desired_editor_scrollbar_opacity();

        if (self.editor_scrollbar_opacity_target - target).abs() > 0.001 {
            self.editor_scrollbar_opacity_target = target;
        } else if self.editor_scrollbar_opacity_task.is_some()
            || (self.editor_scrollbar_opacity - target).abs() <= 0.01
        {
            return;
        }

        self.editor_scrollbar_opacity_epoch += 1;
        let epoch = self.editor_scrollbar_opacity_epoch;
        let duration_ms = self.styles.editor.editor_scrollbar_fade_animation_ms.max(1);
        let frame_ms = self
            .styles
            .editor
            .editor_scrollbar_animation_frame_ms
            .max(1);
        let step = (frame_ms as f32 / duration_ms as f32).clamp(0.08, 1.0);

        let pane_id = self.current_pane_id();
        self.editor_scrollbar_opacity_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let done = this
                    .update(cx, |this, cx| {
                        this.with_pane_if_alive(pane_id, |this| {
                            if this.editor_scrollbar_opacity_epoch != epoch {
                                return true;
                            }

                            let diff = this.editor_scrollbar_opacity_target
                                - this.editor_scrollbar_opacity;
                            if diff.abs() <= 0.01 {
                                this.editor_scrollbar_opacity =
                                    this.editor_scrollbar_opacity_target;
                                this.editor_scrollbar_opacity_task = None;
                                cx.notify();
                                return true;
                            }

                            this.editor_scrollbar_opacity += diff * step;
                            cx.notify();
                            false
                        })
                        .unwrap_or(true)
                    })
                    .unwrap_or(true);

                if done {
                    return;
                }

                cx.background_executor()
                    .timer(Duration::from_millis(frame_ms))
                    .await;
            }
        }));
    }

    fn sync_editor_scrollbar_width(&mut self, cx: &mut Context<Self>) {
        let target = self.desired_editor_scrollbar_width();

        if (self.editor_scrollbar_width_target - target).abs() > 0.001 {
            self.editor_scrollbar_width_target = target;
        } else if self.editor_scrollbar_width_task.is_some()
            || (self.editor_scrollbar_width - target).abs() <= 0.01
        {
            return;
        }

        self.editor_scrollbar_width_epoch += 1;
        let epoch = self.editor_scrollbar_width_epoch;
        let duration_ms = self
            .styles
            .editor
            .editor_scrollbar_width_animation_ms
            .max(1);
        let frame_ms = self
            .styles
            .editor
            .editor_scrollbar_animation_frame_ms
            .max(1);
        let step = (frame_ms as f32 / duration_ms as f32).clamp(0.08, 1.0);

        let pane_id = self.current_pane_id();
        self.editor_scrollbar_width_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let done = this
                    .update(cx, |this, cx| {
                        this.with_pane_if_alive(pane_id, |this| {
                            if this.editor_scrollbar_width_epoch != epoch {
                                return true;
                            }

                            let diff =
                                this.editor_scrollbar_width_target - this.editor_scrollbar_width;
                            if diff.abs() <= 0.01 {
                                this.editor_scrollbar_width = this.editor_scrollbar_width_target;
                                this.editor_scrollbar_width_task = None;
                                cx.notify();
                                return true;
                            }

                            this.editor_scrollbar_width += diff * step;
                            cx.notify();
                            false
                        })
                        .unwrap_or(true)
                    })
                    .unwrap_or(true);

                if done {
                    return;
                }

                cx.background_executor()
                    .timer(Duration::from_millis(frame_ms))
                    .await;
            }
        }));
    }

    fn cancel_editor_scrollbar_idle_fade(&mut self) {
        self.editor_scrollbar_idle_epoch += 1;
        self.editor_scrollbar_idle_task = None;
    }

    fn schedule_editor_scrollbar_idle_fade(&mut self, cx: &mut Context<Self>) {
        if !self.editor_scrollbar_can_show() {
            self.editor_scrollbar_revealed = false;
            self.cancel_editor_scrollbar_idle_fade();
            self.sync_editor_scrollbar_opacity(cx);
            return;
        }

        self.editor_scrollbar_idle_epoch += 1;
        let epoch = self.editor_scrollbar_idle_epoch;
        let delay_ms = self.styles.editor.editor_scrollbar_idle_delay_ms;
        let pane_id = self.current_pane_id();
        self.editor_scrollbar_idle_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(delay_ms))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.with_pane_if_alive(pane_id, |this| {
                    if this.editor_scrollbar_idle_epoch != epoch {
                        return;
                    }

                    this.editor_scrollbar_idle_task = None;
                    if this.editor_scrollbar_hovered || this.editor_scrollbar_drag.is_some() {
                        return;
                    }

                    this.editor_scrollbar_revealed = false;
                    this.sync_editor_scrollbar_opacity(cx);
                });
            });
        }));
    }

    pub(crate) fn reveal_editor_scrollbar(&mut self, cx: &mut Context<Self>) {
        if !self.editor_scrollbar_can_show() {
            return;
        }

        self.editor_scrollbar_revealed = true;
        self.cancel_editor_scrollbar_idle_fade();
        let target = self.desired_editor_scrollbar_opacity();
        self.editor_scrollbar_opacity_epoch = self.editor_scrollbar_opacity_epoch.wrapping_add(1);
        self.editor_scrollbar_opacity_task = None;
        self.editor_scrollbar_opacity = target;
        self.editor_scrollbar_opacity_target = target;
        if !self.editor_scrollbar_hovered && self.editor_scrollbar_drag.is_none() {
            self.schedule_editor_scrollbar_idle_fade(cx);
        }
        cx.notify();
    }

    pub(crate) fn set_editor_scrollbar_hovered(&mut self, hovered: bool, cx: &mut Context<Self>) {
        if self.editor_scrollbar_hovered == hovered {
            return;
        }

        self.editor_scrollbar_hovered = hovered;
        if hovered {
            self.cancel_editor_scrollbar_idle_fade();
        } else if self.editor_scrollbar_revealed && self.editor_scrollbar_drag.is_none() {
            self.schedule_editor_scrollbar_idle_fade(cx);
        }
        self.sync_editor_scrollbar_opacity(cx);
        self.sync_editor_scrollbar_width(cx);
    }

    pub(crate) fn start_editor_scrollbar_drag(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some((thumb_top, thumb_height)) = self.editor_scrollbar_thumb_geometry() else {
            return;
        };
        let track_top = self.editor_scrollbar_track_top();
        let track_travel = (self.editor_scrollbar_track_height() - thumb_height).max(0.0);
        let max_scroll = self.editor_scrollbar_max_scroll();
        if max_scroll <= 0.0 {
            return;
        }

        self.editor_scrollbar_revealed = true;
        self.cancel_editor_scrollbar_idle_fade();
        self.editor_scrollbar_drag = Some(EditorScrollbarDragState {
            start_position_y: f32::from(position.y),
            start_thumb_top: (thumb_top - track_top).max(0.0),
            track_travel,
            max_scroll,
        });
        self.editor_scrollbar_hovered = true;
        self.sync_editor_scrollbar_opacity(cx);
        self.sync_editor_scrollbar_width(cx);
        cx.notify();
    }

    pub(crate) fn update_editor_scrollbar_drag(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.editor_scrollbar_drag.is_none() {
            return false;
        }

        self.pending_editor_scrollbar_drag_position = Some(position);
        if self.editor_scrollbar_drag_task.is_none() {
            if let Some(position) = self.pending_editor_scrollbar_drag_position.take() {
                self.apply_editor_scrollbar_drag_position(position, cx);
            }
            let pane_id = self.current_pane_id();
            self.editor_scrollbar_drag_task = Some(cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(16))
                        .await;
                    let keep_running = this
                        .update(cx, |this, cx| {
                            this.with_pane_if_alive(pane_id, |this| {
                                if this.editor_scrollbar_drag.is_none() {
                                    this.pending_editor_scrollbar_drag_position = None;
                                    this.editor_scrollbar_drag_task = None;
                                    return false;
                                }
                                if let Some(position) =
                                    this.pending_editor_scrollbar_drag_position.take()
                                {
                                    this.apply_editor_scrollbar_drag_position(position, cx);
                                }
                                true
                            })
                            .unwrap_or(false)
                        })
                        .unwrap_or(false);
                    if !keep_running {
                        return;
                    }
                }
            }));
        }
        true
    }

    fn apply_editor_scrollbar_drag_position(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.editor_scrollbar_drag else {
            return;
        };

        let delta_y = f32::from(position.y) - drag.start_position_y;
        let thumb_top = (drag.start_thumb_top + delta_y).clamp(0.0, drag.track_travel);
        self.scroll_offset = if drag.track_travel <= 0.0 {
            0.0
        } else {
            thumb_top / drag.track_travel * drag.max_scroll
        }
        .clamp(0.0, drag.max_scroll);
        self.update_toolbar_visibility_for_scroll(cx);
        cx.notify();
    }

    pub(crate) fn stop_editor_scrollbar_drag(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.editor_scrollbar_drag.is_some() {
            self.apply_editor_scrollbar_drag_position(position, cx);
        }
        self.pending_editor_scrollbar_drag_position = None;
        self.editor_scrollbar_drag_task = None;
        if self.editor_scrollbar_drag.take().is_none() {
            return false;
        }

        self.editor_scrollbar_hovered = self.editor_scrollbar_region_contains(position);
        if self.editor_scrollbar_hovered {
            self.cancel_editor_scrollbar_idle_fade();
        } else if self.editor_scrollbar_revealed {
            self.schedule_editor_scrollbar_idle_fade(cx);
        }
        self.sync_editor_scrollbar_opacity(cx);
        self.sync_editor_scrollbar_width(cx);
        cx.notify();
        true
    }

    pub(crate) fn sidebar_target_width(&self) -> f32 {
        self.active_sidebar_custom_width()
            .unwrap_or_else(|| self.scaled(self.styles.sidebar.sidebar_width_px))
    }

    /// The custom width shared by all left sidebar modes.
    pub(crate) fn active_sidebar_custom_width(&self) -> Option<f32> {
        self.sidebar_custom_width
    }

    pub(crate) fn set_active_sidebar_custom_width(&mut self, width: f32) {
        self.sidebar_custom_width = Some(width);
    }

    pub(crate) fn sidebar_width(&self) -> f32 {
        self.sidebar_display_width.max(0.0)
    }

    pub(crate) fn sidebar_layout_width(&self) -> f32 {
        if self.sidebar_resizing {
            self.sidebar_display_width.max(0.0)
        } else if self.sidebar_width_task.is_some() {
            self.sidebar_layout_width.max(0.0)
        } else {
            self.sidebar_display_width.max(0.0)
        }
    }

    pub(crate) fn should_render_sidebar_panel(&self) -> bool {
        self.sidebar_visible || self.sidebar_width_task.is_some() || self.sidebar_width() > 0.5
    }

    pub(crate) fn utility_panel_target_width(&self) -> f32 {
        match self.utility_panel {
            Some(RightUtilityPanel::Outline) => self
                .outline_panel_custom_width
                .unwrap_or_else(|| self.outline_panel_preferred_width()),
            Some(panel) => self
                .utility_panel_custom_width_for(panel)
                .unwrap_or_else(|| self.scaled(self.styles.utility_panel.utility_panel_width_px)),
            None => self.scaled(self.styles.utility_panel.utility_panel_width_px),
        }
    }

    pub(crate) fn utility_panel_resize_bounds_for(&self, panel: RightUtilityPanel) -> (f32, f32) {
        if panel == RightUtilityPanel::Outline {
            self.outline_panel_resize_bounds()
        } else {
            (
                self.scaled(self.styles.utility_panel.utility_panel_resize_min_width_px),
                self.scaled(self.styles.utility_panel.utility_panel_resize_max_width_px),
            )
        }
    }

    pub(crate) fn utility_panel_custom_width_for(&self, panel: RightUtilityPanel) -> Option<f32> {
        match panel {
            RightUtilityPanel::Info => self.info_panel_custom_width,
            RightUtilityPanel::GitHistory => self.git_history_panel_custom_width,
            RightUtilityPanel::Outline => self.outline_panel_custom_width,
        }
    }

    pub(crate) fn set_utility_panel_custom_width_for(
        &mut self,
        panel: RightUtilityPanel,
        width: Option<f32>,
    ) {
        match panel {
            RightUtilityPanel::Info => self.info_panel_custom_width = width,
            RightUtilityPanel::GitHistory => self.git_history_panel_custom_width = width,
            RightUtilityPanel::Outline => self.outline_panel_custom_width = width,
        }
    }

    pub(crate) fn active_resizable_utility_panel(&self) -> Option<RightUtilityPanel> {
        self.displayed_utility_panel.or(self.utility_panel)
    }

    pub(crate) fn utility_panel_width(&self) -> f32 {
        self.utility_panel_display_width.max(0.0)
    }

    pub(crate) fn utility_panel_layout_width(&self) -> f32 {
        if self.utility_panel_resizing {
            self.utility_panel_display_width.max(0.0)
        } else if self.utility_panel_width_task.is_some() {
            self.utility_panel_layout_width.max(0.0)
        } else {
            self.utility_panel_display_width.max(0.0)
        }
    }

    pub(crate) fn should_render_utility_panel(&self) -> bool {
        self.displayed_utility_panel.is_some()
            && (self.utility_panel_width_task.is_some() || self.utility_panel_width() > 0.5)
    }

    pub(crate) fn is_utility_panel_active(&self, panel: RightUtilityPanel) -> bool {
        self.utility_panel == Some(panel)
    }

    pub(crate) fn should_render_utility_panel_on_right(&self) -> bool {
        self.should_render_utility_panel()
    }

    pub(crate) fn theme_debugger_target_width(&self) -> f32 {
        self.scaled(theme_debugger::THEME_DEBUGGER_WIDTH_PX)
    }

    pub(crate) fn theme_debugger_width(&self) -> f32 {
        self.theme_debugger_display_width.max(0.0)
    }

    pub(crate) fn theme_debugger_layout_width(&self) -> f32 {
        if self.theme_debugger_width_task.is_some() {
            self.theme_debugger_width_target.max(0.0)
        } else {
            self.theme_debugger_display_width.max(0.0)
        }
    }

    pub(crate) fn should_render_theme_debugger_panel(&self) -> bool {
        !self.focused_pane_active_tab_is_terminal()
            && self.displayed_theme_debugger
            && (self.theme_debugger_width_task.is_some() || self.theme_debugger_width() > 0.5)
    }

    pub(crate) fn should_render_right_panels(&self) -> bool {
        self.should_render_theme_debugger_panel() || self.should_render_utility_panel_on_right()
    }

    pub(crate) fn set_sidebar_width_immediately(&mut self, width: f32) {
        self.sidebar_width_epoch += 1;
        self.sidebar_display_width = width.max(0.0);
        self.sidebar_layout_width = self.sidebar_display_width;
        self.sidebar_width_target = self.sidebar_display_width;
        self.sidebar_width_task = None;
    }

    pub(crate) fn set_utility_panel_width_immediately(&mut self, width: f32) {
        self.utility_panel_width_epoch += 1;
        self.utility_panel_display_width = width.max(0.0);
        self.utility_panel_layout_width = self.utility_panel_display_width;
        self.utility_panel_width_target = self.utility_panel_display_width;
        self.utility_panel_width_task = None;
    }

    pub(crate) fn sync_panel_display_state(&mut self) {
        let sidebar_width = if self.sidebar_visible {
            self.sidebar_target_width()
        } else {
            0.0
        };
        self.sidebar_display_width = sidebar_width;
        self.sidebar_layout_width = sidebar_width;
        self.sidebar_width_target = sidebar_width;
        self.sidebar_width_task = None;

        let file_list_width = if self.file_list_visible {
            self.file_list_target_width()
        } else {
            0.0
        };
        self.file_list_display_width = file_list_width;
        self.file_list_layout_width = file_list_width;
        self.file_list_width_target = file_list_width;
        self.file_list_width_task = None;

        self.displayed_utility_panel = self.utility_panel;
        let utility_width = if self.utility_panel.is_some() {
            self.utility_panel_target_width()
        } else {
            0.0
        };
        self.utility_panel_display_width = utility_width;
        self.utility_panel_layout_width = utility_width;
        self.utility_panel_width_target = utility_width;
        self.utility_panel_width_task = None;

        self.displayed_theme_debugger = self.theme_debugger_visible;
        let theme_debugger_width = if self.theme_debugger_visible {
            self.theme_debugger_target_width()
        } else {
            0.0
        };
        self.theme_debugger_display_width = theme_debugger_width;
        self.theme_debugger_width_target = theme_debugger_width;
        self.theme_debugger_width_task = None;
    }

    pub(crate) fn sync_sidebar_width_animation(&mut self, cx: &mut Context<Self>) {
        let target = if self.sidebar_visible {
            self.sidebar_target_width()
        } else {
            0.0
        };
        if (self.sidebar_display_width - target).abs() <= 0.01
            && (self.sidebar_width_target - target).abs() <= 0.01
        {
            self.sidebar_display_width = target;
            self.sidebar_width_target = target;
            return;
        }

        let start = self.sidebar_display_width;
        self.sidebar_width_target = target;
        self.sidebar_layout_width = if target > start { target } else { start };
        self.sidebar_width_epoch += 1;
        let epoch = self.sidebar_width_epoch;
        let duration_ms = self.styles.sidebar.sidebar_visibility_animation_ms.max(1);
        self.sidebar_width_task = Some(cx.spawn(async move |this, cx| {
            let started_at = Instant::now();
            loop {
                let t = (started_at.elapsed().as_secs_f32() * 1000.0 / duration_ms as f32)
                    .clamp(0.0, 1.0);
                let ease = ease_in_out_cubic(t);
                let done = this
                    .update(cx, |this, cx| {
                        if this.sidebar_width_epoch != epoch {
                            return true;
                        }

                        this.sidebar_display_width = start + (target - start) * ease;
                        if t >= 1.0 {
                            this.sidebar_display_width = target;
                            this.sidebar_width_task = None;
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
                    .timer(Duration::from_millis(PANEL_ANIMATION_FRAME_MS))
                    .await;
            }
        }));
    }

    pub(crate) fn sync_utility_panel_width_animation(&mut self, cx: &mut Context<Self>) {
        if self.utility_panel.is_some() {
            self.displayed_utility_panel = self.utility_panel;
        }

        let target = if self.utility_panel.is_some() {
            self.utility_panel_target_width()
        } else {
            0.0
        };

        if (self.utility_panel_display_width - target).abs() <= 0.01
            && (self.utility_panel_width_target - target).abs() <= 0.01
        {
            self.utility_panel_display_width = target;
            self.utility_panel_width_target = target;
            if target <= 0.01 {
                self.displayed_utility_panel = None;
            } else {
                self.displayed_utility_panel = self.utility_panel;
            }
            return;
        }

        let start = self.utility_panel_display_width;
        self.utility_panel_width_target = target;
        self.utility_panel_layout_width = if target > start { target } else { start };
        self.utility_panel_width_epoch += 1;
        let epoch = self.utility_panel_width_epoch;
        let duration_ms = self
            .styles
            .utility_panel
            .utility_panel_visibility_animation_ms
            .max(1);
        self.utility_panel_width_task = Some(cx.spawn(async move |this, cx| {
            let started_at = Instant::now();
            loop {
                let t = (started_at.elapsed().as_secs_f32() * 1000.0 / duration_ms as f32)
                    .clamp(0.0, 1.0);
                let ease = ease_in_out_cubic(t);
                let done = this
                    .update(cx, |this, cx| {
                        if this.utility_panel_width_epoch != epoch {
                            return true;
                        }

                        this.utility_panel_display_width = start + (target - start) * ease;
                        if t >= 1.0 {
                            this.utility_panel_display_width = target;
                            this.utility_panel_width_task = None;
                            if target <= 0.01 {
                                this.displayed_utility_panel = None;
                            } else {
                                this.displayed_utility_panel = this.utility_panel;
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
                    .timer(Duration::from_millis(PANEL_ANIMATION_FRAME_MS))
                    .await;
            }
        }));
    }

    pub(crate) fn sync_theme_debugger_width_animation(&mut self, cx: &mut Context<Self>) {
        if self.theme_debugger_visible {
            self.displayed_theme_debugger = true;
        }

        let target = if self.theme_debugger_visible {
            self.theme_debugger_target_width()
        } else {
            0.0
        };

        if (self.theme_debugger_display_width - target).abs() <= 0.01
            && (self.theme_debugger_width_target - target).abs() <= 0.01
        {
            self.theme_debugger_display_width = target;
            self.theme_debugger_width_target = target;
            if target <= 0.01 {
                self.displayed_theme_debugger = false;
            }
            return;
        }

        let start = self.theme_debugger_display_width;
        self.theme_debugger_width_target = target;
        self.theme_debugger_width_epoch += 1;
        let epoch = self.theme_debugger_width_epoch;
        let duration_ms = self
            .styles
            .utility_panel
            .utility_panel_visibility_animation_ms
            .max(1);
        self.theme_debugger_width_task = Some(cx.spawn(async move |this, cx| {
            let started_at = Instant::now();
            loop {
                let t = (started_at.elapsed().as_secs_f32() * 1000.0 / duration_ms as f32)
                    .clamp(0.0, 1.0);
                let ease = ease_in_out_cubic(t);
                let done = this
                    .update(cx, |this, cx| {
                        if this.theme_debugger_width_epoch != epoch {
                            return true;
                        }

                        this.theme_debugger_display_width = start + (target - start) * ease;
                        if t >= 1.0 {
                            this.theme_debugger_display_width = target;
                            this.theme_debugger_width_task = None;
                            if target <= 0.01 {
                                this.displayed_theme_debugger = false;
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
                    .timer(Duration::from_millis(PANEL_ANIMATION_FRAME_MS))
                    .await;
            }
        }));
    }

    pub(crate) fn update_toolbar_corner_hover(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) -> bool {
        let left = f32::from(self.content_origin.x);
        let top = f32::from(self.content_origin.y);
        let right = (left + self.last_viewport_width.max(1.0)).max(left + 1.0);
        let bottom = (top + self.last_viewport_height.max(1.0)).max(top + 1.0);
        let content_left = self.last_content_left.max(left);
        let content_right = (self.last_content_left + self.last_text_width)
            .clamp(content_left, right.max(content_left));
        let x = f32::from(position.x);
        let y = f32::from(position.y);
        let toolbar_left = left + self.scaled(self.styles.toolbar.toolbar_offset_left_px);
        let toolbar_gap = self.scaled(self.styles.toolbar.toolbar_gap_px);
        let primary_btn_size = self.scaled(self.styles.toolbar.toolbar_primary_btn_size_px);
        let secondary_btn_size = self.scaled(self.styles.toolbar.toolbar_secondary_btn_size_px);
        let left_toolbar_right = toolbar_left + primary_btn_size + toolbar_gap + secondary_btn_size;
        let right_toolbar_left = right
            - self.scaled(self.styles.toolbar.toolbar_offset_left_px)
            - secondary_btn_size * 2.0
            - toolbar_gap;
        let toolbar_overlaps_content =
            left_toolbar_right > content_left || right_toolbar_left < content_right;
        let hovering_side_gutters =
            y >= top && y <= bottom && (x < content_left || x > content_right);
        let hovering_top_reveal = toolbar_overlaps_content
            && y >= top
            && y <= (top + self.scaled(self.styles.toolbar.toolbar_reveal_hover_height_px))
                .min(bottom)
            && x >= left
            && x <= right;
        // The outline rail and the editor scrollbar live in a strip at the right
        // edge. Hovering anywhere in that strip must keep the toolbar visible
        // (even when the document is full-width and the rail overlays content, so
        // a plain content-bounds test would otherwise treat it as "inside"). The
        // rail records its exact interactive width each frame.
        let right_edge_zone = self.outline_rail_zone_from_right.get().max(self.scaled(
            self.styles.editor.editor_scrollbar_right_margin_px
                + self.styles.editor.editor_scrollbar_hover_width_px,
        ));
        let hovering_right_edge = y >= top && y <= bottom && x >= right - right_edge_zone;
        let hovering = hovering_side_gutters
            || hovering_top_reveal
            || hovering_right_edge
            || self.outline_rail_hovered;
        if hovering {
            // Entering the reveal zone: show immediately and cancel any pending
            // delayed hide.
            self.toolbar_corner_hide_task = None;
            if self.toolbar_corner_hovered {
                return false;
            }
            self.toolbar_corner_hovered = true;
            self.sync_toolbar_opacity(cx);
            return true;
        }
        // Left the reveal zone (e.g. moved into the content): keep the revealed
        // toolbar for 2s instead of hiding immediately. Throttled so an already
        // pending hide is not restarted.
        if self.toolbar_corner_hovered && self.toolbar_corner_hide_task.is_none() {
            self.toolbar_corner_hide_task = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(Duration::from_millis(2000))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    this.toolbar_corner_hide_task = None;
                    this.toolbar_corner_hovered = false;
                    this.sync_toolbar_opacity(cx);
                    cx.notify();
                });
            }));
        }
        false
    }

    pub(crate) fn queue_image_load(&mut self, path: String, cx: &mut Context<Self>) {
        if self.image_cache.contains_key(&path) {
            return;
        }

        self.image_cache.insert(path.clone(), None);
        let current_file = self.current_file_path.clone();
        let workspaces = self.workspaces.clone();
        let workspace_root = self.sidebar_root.clone();
        cx.spawn(async move |this, cx| {
            let image_path = path.clone();
            let file_dir = current_file
                .as_ref()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()));
            let maybe_image = cx
                .background_executor()
                .spawn(async move {
                    let resolved_path = if helpers::is_remote_image_url(&image_path) {
                        None
                    } else {
                        attachments::WorkspaceLinkIndex::build(&workspace_root)
                            .resolve(current_file.as_deref(), &image_path)
                    };
                    let load_path = resolved_path
                        .as_ref()
                        .map(|path| path.to_string_lossy().into_owned())
                        .unwrap_or(image_path);
                    helpers::load_image(&load_path, file_dir.as_ref(), &workspaces)
                        .ok()
                        .map(Arc::new)
                })
                .await;

            this.update(cx, |editor, cx| {
                editor.image_cache.insert(path, maybe_image);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    // ── Theme toggle ─────────────────────────────────────────────

    pub(crate) fn toggle_theme(&mut self, cx: &mut Context<Self>) {
        let next = if self.theme_kind.is_dark() {
            self.app_settings.light_theme_kind()
        } else {
            self.app_settings.dark_theme_kind()
        };
        self.set_theme(next, cx);
    }

    pub(crate) fn sync_overlay_text_input_styles(&mut self, cx: &mut Context<Self>) {
        if let Some(editor) = &self.search_query_editor {
            let styles = self.search_query_text_input_styles();
            editor.update(cx, |input, cx| input.set_styles(styles, cx));
        }
        if let Some(editor) = &self.search_replace_editor {
            let styles = self.search_replace_text_input_styles();
            editor.update(cx, |input, cx| input.set_styles(styles, cx));
        }
        if let Some(editor) = &self.workspace_search_editor {
            let styles = self.workspace_search_text_input_styles();
            editor.update(cx, |input, cx| input.set_styles(styles, cx));
        }
        if let Some(editor) = &self.quick_open_editor {
            let styles = self.quick_open_text_input_styles();
            editor.update(cx, |input, cx| input.set_styles(styles, cx));
        }
        if let Some(editor) = &self.command_palette_editor {
            let styles = self.quick_open_text_input_styles();
            editor.update(cx, |input, cx| input.set_styles(styles, cx));
        }
        if let Some(editor) = &self.language_picker_editor {
            let styles = self.search_menu_text_input_styles();
            editor.update(cx, |input, cx| input.set_styles(styles, cx));
        }
        if let Some(editor) = &self.branch_picker_editor {
            let styles = self.search_menu_text_input_styles();
            editor.update(cx, |input, cx| input.set_styles(styles, cx));
        }
        if let Some(sidebar_edit) = &self.sidebar_edit {
            let styles = self.sidebar_edit_text_input_styles();
            sidebar_edit
                .editor()
                .update(cx, |input, cx| input.set_styles(styles, cx));
        }
        self.sync_settings_text_input_styles(cx);
        self.update_theme_debugger_input_styles(cx);
    }

    pub(crate) fn set_theme(&mut self, kind: ThemeKind, cx: &mut Context<Self>) {
        let target_editor_theme_mode = if kind.is_dark() == self.theme_kind.is_dark() {
            self.editor_theme_mode
        } else {
            self.app_settings
                .editor_theme_mode_for_appearance(kind.is_dark())
        };
        self.theme_kind = kind;
        self.editor_theme_mode = target_editor_theme_mode;
        self.settings_theme_dark_mode = kind.is_dark();
        self.theme = crate::theme::apply_editor_theme_mode(
            &self.theme_kind.build(),
            self.editor_theme_mode,
            self.theme_kind.is_dark(),
        );
        // Persist the selection into settings BEFORE rebuilding styles:
        // refresh_theme_styles derives chrome (sidebar / utility panel /
        // settings) styles from app_settings.dark_theme_kind(), so a stale
        // value would leave those components rendered with the previous
        // theme until the next refresh.
        self.app_settings.theme = theme_kind_to_string(kind);
        if kind.is_dark() {
            self.app_settings.dark_theme = theme_kind_to_string(kind);
        } else {
            self.app_settings.light_theme = theme_kind_to_string(kind);
        }
        self.app_settings
            .set_editor_theme_mode_for_appearance(kind.is_dark(), self.editor_theme_mode);
        self.refresh_theme_styles(true, cx);
        self.app_settings.save();
        cx.notify();
    }

    /// Picks the theme to use for a light/dark appearance, keeping Moeka retro
    /// palettes paired with their own companion instead of the global default.
    pub(crate) fn theme_kind_for_appearance(&self, dark: bool) -> ThemeKind {
        self.theme_kind
            .moeka_companion()
            .filter(|kind| kind.is_dark() == dark)
            .unwrap_or_else(|| {
                if dark {
                    self.app_settings.dark_theme_kind()
                } else {
                    self.app_settings.light_theme_kind()
                }
            })
    }

    pub(crate) fn set_theme_mode(
        &mut self,
        mode: app_settings::ThemeMode,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.app_settings.theme_mode = mode.as_str().to_string();
        self.app_settings.save();
        if mode == app_settings::ThemeMode::Auto {
            self.system_appearance = Some(window.appearance());
            self.apply_system_appearance_theme(cx);
        }
        cx.notify();
    }

    /// Called every frame; only reacts when the OS appearance actually flips.
    pub(crate) fn sync_system_appearance(&mut self, window: &gpui::Window, cx: &mut Context<Self>) {
        let appearance = window.appearance();
        if self.system_appearance == Some(appearance) {
            return;
        }
        self.system_appearance = Some(appearance);
        if self.app_settings.theme_mode() == app_settings::ThemeMode::Auto {
            self.apply_system_appearance_theme(cx);
        }
    }

    fn apply_system_appearance_theme(&mut self, cx: &mut Context<Self>) {
        let dark = matches!(
            self.system_appearance,
            Some(gpui::WindowAppearance::Dark | gpui::WindowAppearance::VibrantDark)
        );
        if dark == self.theme_kind.is_dark() {
            return;
        }
        let kind = self.theme_kind_for_appearance(dark);
        self.set_theme(kind, cx);
    }

    pub(crate) fn set_editor_theme_mode(&mut self, mode: EditorThemeMode, cx: &mut Context<Self>) {
        if self.editor_theme_mode == mode {
            return;
        }
        self.editor_theme_mode = mode;
        self.theme = crate::theme::apply_editor_theme_mode(
            &self.theme_kind.build(),
            self.editor_theme_mode,
            self.theme_kind.is_dark(),
        );
        self.refresh_theme_styles(true, cx);
        self.app_settings
            .set_editor_theme_mode_for_appearance(self.theme_kind.is_dark(), mode);
        self.finalize_settings_change(cx);
    }

    // ── File system watcher ─────────────────────────────────────

    pub(crate) fn start_watching(&mut self, cx: &mut Context<Self>) {
        use notify::{RecursiveMode, Watcher};

        let root = self.sidebar_root.clone();
        if !root.exists() {
            self._watcher_task = None;
            return;
        }

        let pending = Arc::new(std::sync::Mutex::new(Vec::<notify::Event>::new()));
        let pending_writer = pending.clone();

        let mut watcher = match notify::recommended_watcher(
            move |result: Result<notify::Event, notify::Error>| {
                if let Ok(event) = result {
                    if !event.paths.is_empty()
                        && let Ok(mut guard) = pending_writer.lock()
                    {
                        guard.push(event);
                    }
                }
            },
        ) {
            Ok(w) => w,
            Err(_) => return,
        };

        if watcher.watch(&root, RecursiveMode::Recursive).is_err() {
            return;
        }

        self._watcher_task = Some(cx.spawn(async move |this, cx| {
            // Keep the watcher alive for the lifetime of the task
            let _watcher = watcher;
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(500))
                    .await;

                let events: Vec<notify::Event> = {
                    let mut guard = match pending.lock() {
                        Ok(g) => g,
                        Err(_) => break,
                    };
                    guard.drain(..).collect()
                };

                if events.is_empty() {
                    continue;
                }

                let result = this.update(cx, |editor, cx| {
                    editor.handle_fs_notify_events(&events, cx);
                });
                if result.is_err() {
                    break;
                }
            }
        }));
    }

    fn is_internal_temp_change_path(&self, path: &Path) -> bool {
        if file_ops::is_atomic_write_temp_path(path) {
            return true;
        }

        if path
            .components()
            .any(|component| component.as_os_str().to_str() == Some(".moeka-cache"))
        {
            return true;
        }

        if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| {
                name.starts_with('.') && (name.ends_with(".temp") || name.ends_with(".temp.tmp"))
            })
        {
            return true;
        }

        self.tabs
            .iter()
            .filter_map(|tab| tab.recovery.temp_path.as_deref())
            .any(|temp_path| {
                if path == temp_path {
                    return true;
                }

                let Some(file_name) = temp_path.file_name().and_then(|name| name.to_str()) else {
                    return false;
                };
                let writer_temp_path = temp_path.with_file_name(format!(".{file_name}.tmp"));
                path == writer_temp_path.as_path()
            })
    }

    fn path_contains_ignored_workspace_component(&self, path: &Path) -> bool {
        let Ok(relative_path) = path.strip_prefix(&self.sidebar_root) else {
            return false;
        };

        relative_path.components().any(|component| {
            let Some(name) = component.as_os_str().to_str() else {
                return false;
            };
            !name.is_empty()
                && name != "."
                && (name.starts_with('.')
                    || matches!(
                        name,
                        "node_modules" | "target" | "dist" | "build" | "coverage"
                    ))
        })
    }

    fn is_git_metadata_change_path(&self, path: &Path) -> bool {
        path.components()
            .any(|component| component.as_os_str().to_str() == Some(".git"))
    }

    fn git_metadata_change_impact(&self, path: &Path) -> GitMetadataImpact {
        let mut components = path.components();
        while let Some(component) = components.next() {
            if component.as_os_str().to_str() != Some(".git") {
                continue;
            }

            let category = components
                .next()
                .and_then(|component| component.as_os_str().to_str());

            return match category {
                None => GitMetadataImpact {
                    affects_sidebar_status: true,
                    affects_history: true,
                    affects_working_tree_diff: true,
                },
                Some(
                    "HEAD" | "refs" | "packed-refs" | "rebase-apply" | "rebase-merge" | "sequencer"
                    | "MERGE_HEAD" | "CHERRY_PICK_HEAD" | "REVERT_HEAD" | "ORIG_HEAD",
                ) => GitMetadataImpact {
                    affects_sidebar_status: true,
                    affects_history: true,
                    affects_working_tree_diff: true,
                },
                Some("index") => GitMetadataImpact {
                    affects_sidebar_status: true,
                    affects_history: true,
                    affects_working_tree_diff: false,
                },
                // `index.lock` only marks a git command in flight; the real
                // change arrives with the `index` write that follows.
                Some("index.lock") => GitMetadataImpact::default(),
                _ => GitMetadataImpact::default(),
            };
        }

        GitMetadataImpact::default()
    }

    fn fs_change_affects_git_status(&self, path: &Path) -> bool {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) => !metadata.is_dir(),
            Err(_) => true,
        }
    }

    fn fs_rename_mappings(events: &[notify::Event]) -> Vec<(PathBuf, PathBuf)> {
        use notify::EventKind;
        use notify::event::{ModifyKind, RenameMode};

        let mut mappings = Vec::new();
        let mut pending_from = std::collections::HashMap::<usize, PathBuf>::new();
        for event in events {
            match &event.kind {
                EventKind::Modify(ModifyKind::Name(RenameMode::Both | RenameMode::Any))
                    if event.paths.len() >= 2 =>
                {
                    mappings.push((event.paths[0].clone(), event.paths[1].clone()));
                }
                EventKind::Modify(ModifyKind::Name(RenameMode::From)) => {
                    if let (Some(tracker), Some(path)) =
                        (event.attrs.tracker(), event.paths.first())
                    {
                        pending_from.insert(tracker, path.clone());
                    }
                }
                EventKind::Modify(ModifyKind::Name(RenameMode::To)) => {
                    if let (Some(tracker), Some(path)) =
                        (event.attrs.tracker(), event.paths.first())
                        && let Some(from) = pending_from.remove(&tracker)
                    {
                        mappings.push((from, path.clone()));
                    }
                }
                _ => {}
            }
        }
        mappings
    }

    fn handle_fs_notify_events(&mut self, events: &[notify::Event], cx: &mut Context<Self>) {
        for (from, to) in Self::fs_rename_mappings(events) {
            if from.starts_with(&self.sidebar_root) {
                self.rewrite_open_tab_paths_after_move(&from, &to);
            }
        }
        let changed_paths = events
            .iter()
            .flat_map(|event| event.paths.iter().cloned())
            .collect::<Vec<_>>();
        self.handle_fs_events(&changed_paths, cx);
    }

    /// A rename outside the app arrives as "old path gone, new path appeared".
    /// Re-point saved tabs at the new path (matching by content) so the tab
    /// keeps tracking the same document instead of showing up as deleted.
    /// Returns the changed paths with the followed old paths dropped.
    fn follow_external_tab_renames(&mut self, changed_paths: Vec<PathBuf>) -> Vec<PathBuf> {
        let vanished = changed_paths
            .iter()
            .filter(|path| !path.exists())
            .cloned()
            .collect::<Vec<_>>();
        if vanished.is_empty() {
            return changed_paths;
        }
        let appeared = changed_paths
            .iter()
            .filter(|path| {
                path.is_file()
                    && !self
                        .all_tabs()
                        .any(|tab| tab.path.as_deref() == Some(path.as_path()))
            })
            .cloned()
            .collect::<Vec<_>>();
        if appeared.is_empty() {
            return changed_paths;
        }

        let mut followed = Vec::new();
        for old_path in vanished {
            // A dirty buffer has unsaved work whose content no longer matches
            // anything on disk; leave it flagged as deleted.
            let Some(expected) = self
                .all_tabs()
                .find(|tab| tab.path.as_deref() == Some(old_path.as_path()) && !tab.is_dirty)
                .map(|tab| tab.snapshot.text.clone())
            else {
                continue;
            };
            let Some(new_path) = appeared.iter().find(|candidate| {
                !followed.iter().any(|(_, taken)| taken == *candidate)
                    && std::fs::read(candidate)
                        .is_ok_and(|bytes| String::from_utf8_lossy(&bytes) == expected)
            }) else {
                continue;
            };
            followed.push((old_path, new_path.clone()));
        }

        if followed.is_empty() {
            return changed_paths;
        }
        for (old_path, new_path) in &followed {
            self.rewrite_open_tab_paths_after_move(old_path, new_path);
        }
        session_store::persist_session(self);
        changed_paths
            .into_iter()
            .filter(|path| !followed.iter().any(|(old_path, _)| old_path == path))
            .collect()
    }

    pub(crate) fn handle_fs_events(&mut self, changed_paths: &[PathBuf], cx: &mut Context<Self>) {
        let mut changed_paths_in_workspace = Vec::new();
        let mut seen_paths_in_workspace = HashSet::new();
        let mut git_metadata_impact = GitMetadataImpact::default();
        for path in changed_paths.iter().map(PathBuf::as_path) {
            if !path.starts_with(&self.sidebar_root) {
                continue;
            }
            if self.is_internal_temp_change_path(path) {
                continue;
            }
            if self.is_git_metadata_change_path(path) {
                git_metadata_impact.merge(self.git_metadata_change_impact(path));
                continue;
            }
            if self.path_contains_ignored_workspace_component(path) {
                continue;
            }
            let owned_path = path.to_path_buf();
            if seen_paths_in_workspace.insert(owned_path.clone()) {
                changed_paths_in_workspace.push(owned_path);
            }
        }

        // Open documents can live outside the watched workspace (the config
        // file, files opened by path), so reload those on their own paths.
        let mut changed_paths_for_tabs = changed_paths_in_workspace.clone();
        for path in changed_paths.iter().map(PathBuf::as_path) {
            if path.starts_with(&self.sidebar_root) || self.is_internal_temp_change_path(path) {
                continue;
            }
            let is_open = self.all_tabs().any(|tab| tab.path.as_deref() == Some(path));
            if is_open && seen_paths_in_workspace.insert(path.to_path_buf()) {
                changed_paths_for_tabs.push(path.to_path_buf());
            }
        }

        if changed_paths_for_tabs.is_empty() && !git_metadata_impact.any() {
            return;
        }

        changed_paths_for_tabs = self.follow_external_tab_renames(changed_paths_for_tabs);

        let current_file_hit = self.current_file_path.as_ref().is_some_and(|current_path| {
            changed_paths_in_workspace
                .iter()
                .any(|path| path == current_path || current_path.starts_with(path))
        });
        let mut needs_notify = false;
        if !changed_paths_in_workspace.is_empty() {
            self.mark_sidebar_dir_file_counts_dirty(
                changed_paths_in_workspace
                    .iter()
                    .filter_map(|path| path.parent().map(Path::to_path_buf)),
            );
            if self.sidebar_paths_require_structure_refresh(&changed_paths_in_workspace) {
                self.invalidate_sidebar_items_cache();
            } else {
                self.refresh_file_list_content_paths(&changed_paths_in_workspace, cx);
            }
            self.mark_workspace_search_index_dirty(&changed_paths_in_workspace);
            needs_notify = true;
        }
        let git_status_changed = git_metadata_impact.affects_sidebar_status
            || changed_paths_in_workspace
                .iter()
                .any(|path| self.fs_change_affects_git_status(path));
        if git_status_changed {
            self.invalidate_sidebar_git_statuses();
            self.invalidate_git_panel_status();
            needs_notify = true;
        }
        if git_metadata_impact.affects_history {
            self.invalidate_git_history_cache();
            self.invalidate_git_panel_status();
            needs_notify = true;
        }
        if current_file_hit {
            self.invalidate_info_panel_cache();
            self.invalidate_git_history_cache();
            needs_notify = true;
        }

        // Apply external file changes to every pane's tabs (marking deleted
        // files, reloading clean buffers, refreshing diff tabs).
        for pane_id in self.pane_ids() {
            let pane_notified = self.with_pane(pane_id, |this| {
                let mut needs_notify = false;
                let mut active_tab_needs_reload = false;
                let mut diff_tabs_to_refresh = Vec::new();
                let active_tab_id = this.active_tab_id.clone();

                for (tab_index, tab) in this.tabs.iter_mut().enumerate() {
                    if let Some(ref tab_path) = tab.path {
                        let path_changed = changed_paths_for_tabs
                            .iter()
                            .any(|path| path == tab_path || tab_path.starts_with(path));
                        let should_refresh_diff = tab.git_diff.as_ref().is_some_and(|diff| {
                            diff.source.tracks_local_state()
                                && (path_changed || git_metadata_impact.affects_working_tree_diff)
                        });
                        if path_changed || should_refresh_diff {
                            if tab.kind == TabKind::Diff {
                                if !should_refresh_diff {
                                    continue;
                                }
                                let exists = tab_path.exists();
                                if tab.is_deleted == exists {
                                    tab.is_deleted = !exists;
                                    needs_notify = true;
                                }
                                diff_tabs_to_refresh.push(tab_index);
                                continue;
                            }

                            if !tab_path.exists() {
                                if !tab.is_deleted {
                                    tab.is_deleted = true;
                                    needs_notify = true;
                                    active_tab_needs_reload |=
                                        active_tab_id.as_deref() == Some(tab.id.as_str());
                                }
                            } else {
                                if tab.is_deleted {
                                    tab.is_deleted = false;
                                    needs_notify = true;
                                    active_tab_needs_reload |=
                                        active_tab_id.as_deref() == Some(tab.id.as_str());
                                }
                                if !tab.is_dirty {
                                    if let Ok(bytes) = std::fs::read(tab_path) {
                                        let new_text = String::from_utf8_lossy(&bytes).into_owned();
                                        if new_text != tab.snapshot.text {
                                            tab.snapshot.text = new_text.clone();
                                            tab.snapshot.cursor_offset =
                                                tab.snapshot.cursor_offset.min(new_text.len());
                                            tab.snapshot.cursor_anchor =
                                                tab.snapshot.cursor_anchor.min(new_text.len());
                                            needs_notify = true;
                                            active_tab_needs_reload |=
                                                active_tab_id.as_deref() == Some(tab.id.as_str());
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                for tab_index in diff_tabs_to_refresh {
                    if this.refresh_git_diff_tab(tab_index) {
                        needs_notify = true;
                        if let Some(tab) = this.tabs.get(tab_index) {
                            active_tab_needs_reload |=
                                active_tab_id.as_deref() == Some(tab.id.as_str());
                        }
                    }
                }

                // A HEAD move (commit, checkout, reset) changes the inline-diff
                // baseline for the active editable tab, so re-read it from git
                // on the next render.
                if git_metadata_impact.affects_working_tree_diff {
                    this.mark_inline_diff_dirty(true);
                    needs_notify = true;
                }

                // Reload the active tab's buffer if it was externally modified
                if active_tab_needs_reload {
                    if let Some(ref active_id) = this.active_tab_id {
                        if let Some(tab) = this.tabs.iter().find(|t| &t.id == active_id) {
                            if !tab.is_dirty && !tab.is_deleted {
                                let snapshot = tab.snapshot.clone();
                                this.discard_history_transaction();
                                if let Some(index) = this.active_tab_index() {
                                    this.tabs[index].history.clear_entries();
                                }
                                this.buffer = crate::buffer::Buffer::from_text(&snapshot.text);
                                this.cursor.offset = snapshot.cursor_offset.min(this.buffer.len());
                                this.cursor.anchor = snapshot.cursor_anchor.min(this.buffer.len());
                                this.reset_cursor_visual_targeting();
                                this.reparse();
                            }
                        }
                    }
                }

                needs_notify
            });
            needs_notify |= pane_notified;
        }

        if needs_notify {
            cx.notify();
        }
    }

    // ── Dialog ──────────────────────────────────────────────────────

    pub(crate) fn show_dialog(&mut self, dialog: Dialog, cx: &mut Context<Self>) {
        self.close_preview_overlay();
        self.dialog = Some(dialog);
        cx.notify();
    }

    pub(crate) fn dismiss_dialog(&mut self, cx: &mut Context<Self>) {
        self.dialog = None;
        self.pending_close_tab_ids.clear();
        cx.notify();
    }

    pub(crate) fn confirm_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        match dialog.action {
            DialogAction::CloseTabUnsaved { tab_id } => {
                let found = self
                    .all_tabs_mut()
                    .find(|t| t.id == tab_id)
                    .map(|tab| tab.is_dirty = false)
                    .is_some();
                if found {
                    self.close_tab(&tab_id, cx);
                }
            }
            DialogAction::CloseRunningTerminal { tab_id } => {
                self.close_tab(&tab_id, cx);
            }
            DialogAction::SaveAndCloseTab { .. } => {
                // Save action is handled separately via save_dialog
            }
            DialogAction::DeletePath { path } => {
                self.delete_path(path, cx);
            }
            DialogAction::DeletePaths { paths } => {
                self.delete_paths_to_trash(&paths, cx);
            }
            DialogAction::InstallUpdate { update } => {
                self.install_update(update);
            }
            DialogAction::GitDiscardEntries { tracked, untracked } => {
                self.run_git_operation(GitOperation::DiscardPaths { tracked, untracked }, cx);
            }
            DialogAction::FormatWholeDocument => {
                self.format_whole_markdown_document(cx);
            }
            DialogAction::DismissOnly => {}
        }
        cx.notify();
    }

    pub(crate) fn save_and_close_dialog(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        match dialog.action {
            DialogAction::CloseTabUnsaved { ref tab_id }
            | DialogAction::SaveAndCloseTab { ref tab_id } => {
                let tab_id = tab_id.clone();
                // Phase 1 (bounded borrow): write the file / collect what the
                // save dialog needs. Phase 2 acts with `self` free again.
                enum SaveAndClose {
                    Missing,
                    Saved,
                    NeedsDialog { text: String, default_name: String },
                }
                let outcome = match self.all_tabs_mut().find(|t| t.id == tab_id) {
                    None => SaveAndClose::Missing,
                    Some(tab) => {
                        if let Some(ref path) = tab.path {
                            if std::fs::write(path, &tab.snapshot.text).is_ok() {
                                tab.is_dirty = false;
                            }
                            let snapshot = tab.snapshot.clone();
                            let _ = history_store::persist_tab_history(
                                &tab_id,
                                tab.path.as_deref(),
                                &mut tab.history,
                                &snapshot,
                            );
                            SaveAndClose::Saved
                        } else {
                            SaveAndClose::NeedsDialog {
                                text: tab.snapshot.text.clone(),
                                default_name: format!("{}.md", tab.title),
                            }
                        }
                    }
                };
                match outcome {
                    SaveAndClose::Saved => {
                        self.clear_temp_file_for_tab(&tab_id);
                        session_store::persist_session(self);
                        self.close_tab(&tab_id, cx);
                    }
                    SaveAndClose::NeedsDialog { text, default_name } => {
                        let tab_id_clone = tab_id.clone();
                        let title = self.t(crate::i18n::TextKey::SaveFileDialog).to_string();
                        cx.spawn(async move |this, cx| {
                            let maybe_path =
                                cx.background_executor()
                                    .spawn(async move {
                                        helpers::save_file_blocking(&default_name, &title)
                                    })
                                    .await;
                            if let Some(path) = maybe_path {
                                if std::fs::write(&path, &text).is_ok() {
                                    this.update(cx, |editor, cx| {
                                        if let Some(tab) =
                                            editor.all_tabs_mut().find(|t| t.id == tab_id_clone)
                                        {
                                            tab.path = Some(path.clone());
                                            tab.title = Self::title_for_path(&path);
                                            tab.is_dirty = false;
                                            let snapshot = tab.snapshot.clone();
                                            let _ = history_store::persist_tab_history(
                                                &tab_id_clone,
                                                tab.path.as_deref(),
                                                &mut tab.history,
                                                &snapshot,
                                            );
                                        }
                                        editor.clear_temp_file_for_tab(&tab_id_clone);
                                        session_store::persist_session(editor);
                                        editor.close_tab(&tab_id_clone, cx);
                                    })
                                    .ok();
                                }
                            }
                        })
                        .detach();
                    }
                    SaveAndClose::Missing => {
                        self.close_tab(&tab_id, cx);
                    }
                }
            }
            DialogAction::DeletePath { .. }
            | DialogAction::DeletePaths { .. }
            | DialogAction::InstallUpdate { .. }
            | DialogAction::GitDiscardEntries { .. }
            | DialogAction::FormatWholeDocument
            | DialogAction::CloseRunningTerminal { .. } => {
                // No save action for delete
            }
            DialogAction::DismissOnly => {
                // Informational dialog; save action is not applicable.
            }
        }
        cx.notify();
    }

    fn show_save_before_import_dialog(&mut self, cx: &mut Context<Self>) {
        self.show_dialog(
            Dialog {
                title: self
                    .t(crate::i18n::TextKey::SaveCurrentFileFirstTitle)
                    .to_string(),
                message: self
                    .t(crate::i18n::TextKey::SaveCurrentFileFirstMessage)
                    .to_string(),
                save_label: None,
                confirm_label: self.t(crate::i18n::TextKey::Acknowledge).to_string(),
                cancel_label: self.t(crate::i18n::TextKey::Cancel).to_string(),
                is_danger: false,
                action: DialogAction::DismissOnly,
            },
            cx,
        );
    }

    fn insert_pasted_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.checkpoint();
        if self.has_extra_cursors() {
            self.multi_cursor_insert(text);
        } else {
            self.cursor.insert(&mut self.buffer, text);
        }
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn active_tab_is_blank_untitled_text_editor(&self) -> bool {
        let Some(index) = self.active_tab_index() else {
            return false;
        };
        let Some(tab) = self.tabs.get(index) else {
            return false;
        };
        tab.kind == TabKind::Editor
            && tab.path.is_none()
            && tab
                .file_kind
                .as_ref()
                .is_some_and(FileKind::supports_text_buffer)
            && self.buffer.text().is_empty()
            && !self.cursor.has_selection()
            && !self.has_extra_cursors()
    }

    fn apply_pasted_text_file_kind_to_active_tab(&mut self, file_kind: FileKind) {
        let Some(index) = self.active_tab_index() else {
            return;
        };
        let is_code_or_text = matches!(file_kind, FileKind::Code { .. } | FileKind::Text);
        self.active_file_kind_cache = Some(file_kind.clone());
        if let Some(tab) = self.tabs.get_mut(index) {
            tab.file_kind = Some(file_kind);
            if is_code_or_text {
                tab.editor_left_aligned = true;
            }
        }
        if is_code_or_text {
            self.editor_left_aligned = true;
        }
        self.layout_dirty = true;
        session_store::persist_session(self);
    }

    fn import_clipboard_resources(
        &mut self,
        item: &ClipboardItem,
        cx: &mut Context<Self>,
    ) -> anyhow::Result<ClipboardPasteOutcome> {
        let mut images: Vec<_> = item
            .entries()
            .iter()
            .filter_map(|entry| match entry {
                ClipboardEntry::Image(image) => Some(image.clone()),
                _ => None,
            })
            .collect();

        if images.len() > 1 {
            images.sort_by_key(|image| match image.format {
                gpui::ImageFormat::Png => 0,
                gpui::ImageFormat::Jpeg => 1,
                gpui::ImageFormat::Webp => 2,
                gpui::ImageFormat::Gif => 3,
                gpui::ImageFormat::Bmp => 4,
                gpui::ImageFormat::Tiff => 5,
                gpui::ImageFormat::Svg => 6,
            });
            images.truncate(1);
        }
        images = images
            .into_iter()
            .map(|image| normalize_clipboard_image(&image))
            .collect();

        let mut file_paths = item
            .text()
            .map(|text| parse_file_urls_from_text(&text))
            .unwrap_or_default();
        file_paths.extend(crate::platform::clipboard::file_paths());
        file_paths.retain(|path| path.is_file());
        if !images.is_empty() {
            file_paths.retain(|path| !is_image_path(path));
        }
        file_paths.dedup();

        if images.is_empty() && file_paths.is_empty() {
            return Ok(ClipboardPasteOutcome::NotHandled);
        }

        let Some(target_dir) = self
            .current_file_path
            .as_deref()
            .map(|current_file| {
                self.app_settings
                    .attachment_policy()
                    .target_directory(&self.sidebar_root, current_file)
            })
            .transpose()?
        else {
            self.show_save_before_import_dialog(cx);
            return Ok(ClipboardPasteOutcome::Consumed);
        };
        std::fs::create_dir_all(&target_dir)?;

        let mut imported = Vec::new();
        for image in images {
            let target_path = unique_import_path(
                &target_dir,
                &timestamp_file_stem(),
                Some(image_extension(image.format)),
            );
            std::fs::write(&target_path, &image.bytes)?;
            imported.push((target_path, true));
        }

        for source_path in file_paths {
            let extension = source_path
                .extension()
                .and_then(|extension| extension.to_str());
            let stem = source_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .filter(|stem| !stem.is_empty())
                .unwrap_or("attachment");
            let target_path = unique_import_path(&target_dir, stem, extension);
            std::fs::copy(&source_path, &target_path)?;
            imported.push((target_path, is_image_path(&source_path)));
        }

        if imported.is_empty() {
            Ok(ClipboardPasteOutcome::Consumed)
        } else {
            Ok(ClipboardPasteOutcome::Insert(
                self.references_for_imported_paths(&imported)?.join("\n"),
            ))
        }
    }

    fn references_for_imported_paths(
        &self,
        imported: &[(PathBuf, bool)],
    ) -> anyhow::Result<Vec<String>> {
        let current_file = self.current_file_path.as_deref().ok_or_else(|| {
            anyhow::anyhow!("save the current document before importing attachments")
        })?;
        let policy = self.app_settings.attachment_policy();
        let index = attachments::WorkspaceLinkIndex::build(&self.sidebar_root);
        imported
            .iter()
            .map(|(path, is_image)| {
                policy.reference_for(
                    &self.sidebar_root,
                    current_file,
                    path,
                    *is_image,
                    Some(&index),
                )
            })
            .collect()
    }

    pub(crate) fn import_external_paths_as_attachments(
        &mut self,
        paths: &[PathBuf],
        cx: &mut Context<Self>,
    ) {
        if self.active_tab_is_read_only() || !self.active_tab_is_markdown() {
            self.open_external_paths_in_tabs(paths, cx);
            return;
        }
        let (markdown_paths, attachment_paths) = split_external_paths(paths);
        if attachment_paths.is_empty() {
            self.open_external_paths_in_tabs(&markdown_paths, cx);
            return;
        }
        let Some(current_file) = self.current_file_path.as_deref() else {
            self.show_save_before_import_dialog(cx);
            return;
        };
        let result = (|| -> anyhow::Result<String> {
            let policy = self.app_settings.attachment_policy();
            let target_dir = policy.target_directory(&self.sidebar_root, current_file)?;
            std::fs::create_dir_all(&target_dir)?;
            let mut imported = Vec::new();
            for source_path in attachment_paths.iter().filter(|path| path.is_file()) {
                let extension = source_path
                    .extension()
                    .and_then(|extension| extension.to_str());
                let stem = source_path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .filter(|stem| !stem.is_empty())
                    .unwrap_or("attachment");
                let target_path = unique_import_path(&target_dir, stem, extension);
                std::fs::copy(source_path, &target_path)?;
                imported.push((target_path, is_image_path(source_path)));
            }
            Ok(self.references_for_imported_paths(&imported)?.join("\n"))
        })();
        match result {
            Ok(insertion) if !insertion.is_empty() => self.insert_pasted_text(&insertion, cx),
            Ok(_) => {}
            Err(error) => {
                let message = format!("{}: {error}", self.t(TextKey::OperationFailed));
                self.notify_error(message, cx);
            }
        }
        self.open_external_paths_in_tabs(&markdown_paths, cx);
    }

    pub(crate) fn capture_workspace_rewrite_context(
        &self,
    ) -> Option<attachments::WorkspaceRewriteContext> {
        None
    }

    pub(crate) fn apply_workspace_link_rewrites(
        &mut self,
        context: Option<attachments::WorkspaceRewriteContext>,
        mappings: &[attachments::PathMapping],
        cx: &mut Context<Self>,
    ) {
        let Some(context) = context else {
            return;
        };
        let rewrites = context.plan(mappings, &self.app_settings.attachment_policy());
        let mut active_changed = false;
        let mut changed = false;
        for rewrite in rewrites {
            let open_tab_index = self.tabs.iter().position(|tab| {
                tab.kind == TabKind::Editor
                    && tab.path.as_deref() == Some(rewrite.new_path.as_path())
            });
            let is_dirty = open_tab_index.is_some_and(|index| self.tabs[index].is_dirty);
            if !is_dirty
                && let Err(error) =
                    attachments::write_text_atomically(&rewrite.new_path, &rewrite.text)
            {
                let message = format!("{}: {error}", self.t(TextKey::FileOperationFailed));
                self.notify_error(message, cx);
                continue;
            }

            if let Some(index) = open_tab_index {
                self.tabs[index].snapshot.text = rewrite.text.clone();
                self.tabs[index].snapshot.cursor_offset = self.tabs[index]
                    .snapshot
                    .cursor_offset
                    .min(rewrite.text.len());
                self.tabs[index].snapshot.cursor_anchor = self.tabs[index]
                    .snapshot
                    .cursor_anchor
                    .min(rewrite.text.len());
                if self.active_tab_id.as_deref() == Some(self.tabs[index].id.as_str()) {
                    self.commit_history_transaction();
                    self.checkpoint();
                    let old_len = self.buffer.len();
                    self.buffer.replace(0..old_len, &rewrite.text);
                    self.cursor.offset = self.cursor.offset.min(self.buffer.len());
                    self.cursor.anchor = self.cursor.anchor.min(self.buffer.len());
                    for cursor in &mut self.extra_cursors {
                        cursor.offset = cursor.offset.min(self.buffer.len());
                        cursor.anchor = cursor.anchor.min(self.buffer.len());
                    }
                    self.commit_history_transaction();
                    active_changed = true;
                }
            }
            changed = true;
        }

        if active_changed {
            self.reparse();
        }
        if changed {
            self.invalidate_info_panel_cache();
            session_store::persist_session(self);
            cx.notify();
        }
    }

    fn cursor_states_with_primary_flag(&self) -> Vec<(Cursor, bool)> {
        let mut states = Vec::with_capacity(1 + self.extra_cursors.len());
        states.push((self.cursor, true));
        states.extend(
            self.extra_cursors
                .iter()
                .copied()
                .map(|cursor| (cursor, false)),
        );
        states
    }

    fn restore_cursor_states(&mut self, states: Vec<(Cursor, bool)>) {
        self.extra_cursors.clear();
        for (cursor, is_primary) in states {
            if is_primary {
                self.cursor = cursor;
            } else {
                self.extra_cursors.push(cursor);
            }
        }
        self.dedup_cursors();
    }

    fn adjust_offset_for_line_prefix_change(
        offset: usize,
        line_start: usize,
        removed: usize,
        inserted: usize,
    ) -> usize {
        if offset < line_start {
            return offset;
        }

        let removed_end = line_start.saturating_add(removed);
        if offset <= line_start {
            return line_start.saturating_add(inserted);
        }
        if removed > 0 && offset <= removed_end {
            return line_start.saturating_add(inserted);
        }

        if inserted >= removed {
            offset.saturating_add(inserted - removed)
        } else {
            offset.saturating_sub(removed - inserted)
        }
    }

    fn line_span_for_range(&self, range: Range<usize>) -> (usize, usize) {
        let (start_line, _) = self.buffer.offset_to_line_col(range.start);
        let end_anchor = range
            .end
            .saturating_sub(usize::from(range.end > range.start));
        let (end_line, _) = self.buffer.offset_to_line_col(end_anchor);
        (start_line, end_line)
    }

    fn unordered_list_display_marker_for_line(&self, line_content: &str) -> &'static str {
        let leading_ws = line_content.len() - line_content.trim_start_matches([' ', '\t']).len();
        let marker_cycle = &self.styles.editor.editor_unordered_list_marker_cycle;
        let level = leading_ws / 2;
        marker_cycle[level % marker_cycle.len()]
    }

    pub(crate) fn apply_list_display_styles(
        &self,
        _line_content: &str,
        decoration: &LineDecoration,
        _display_text: &mut String,
        prefix_collapsed: bool,
    ) {
        if !prefix_collapsed || decoration.block_type != BlockType::UnorderedList {
            return;
        }
    }

    fn resolved_list_indent_widths(&self, line_indices: &[usize]) -> Vec<(usize, usize)> {
        let mut current_width = 2;
        let mut resolved = Vec::with_capacity(line_indices.len());

        for &line in line_indices {
            if let Some(width) = self.list_indent_width_for_line(line) {
                current_width = width;
            }
            resolved.push((line, current_width));
        }

        resolved
    }

    pub(crate) fn collect_line_indices_for_block_indent(&self) -> (Vec<usize>, bool, bool) {
        let mut lines = Vec::new();
        let mut has_selection = false;
        let mut has_list_line = false;

        for (offset, anchor) in self.all_cursor_ranges() {
            let range = offset.min(anchor)..offset.max(anchor);
            if range.is_empty() {
                let line = self.buffer.offset_to_line_col(offset).0;
                if self.list_indent_width_for_line(line).is_some() {
                    lines.push(line);
                    has_list_line = true;
                }
                continue;
            }

            has_selection = true;
            let (start_line, end_line) = self.line_span_for_range(range);
            for line in start_line..=end_line {
                has_list_line |= self.list_indent_width_for_line(line).is_some();
                lines.push(line);
            }
        }

        lines.sort_unstable();
        lines.dedup();
        (lines, has_selection, has_list_line)
    }

    /// Offset a line's indentation starts at: after any `>` prefix, so indenting
    /// a quoted list moves the list rather than the quote.
    fn line_indent_start(&self, line: usize) -> usize {
        let quote_len = markdown::blockquote_prefix_len(self.buffer.line_content(line));
        self.buffer.line_start(line) + quote_len
    }

    /// Indent spaces available to strip from `line`, capped at `max_spaces`.
    fn leading_indent_spaces(&self, line: usize, max_spaces: usize) -> usize {
        let content = self.buffer.line_content(line);
        content[markdown::blockquote_prefix_len(content)..]
            .bytes()
            .take(max_spaces)
            .take_while(|&byte| byte == b' ')
            .count()
    }

    pub(crate) fn indent_line_indices(&mut self, line_indices: &[usize], spaces: &str) {
        if line_indices.is_empty() || spaces.is_empty() {
            return;
        }

        let inserted = spaces.len();
        let mut cursor_states = self.cursor_states_with_primary_flag();
        for &line in line_indices {
            let indent_start = self.line_indent_start(line);
            self.buffer.insert(indent_start, spaces);
            for (cursor, _) in &mut cursor_states {
                cursor.offset = Self::adjust_offset_for_line_prefix_change(
                    cursor.offset,
                    indent_start,
                    0,
                    inserted,
                )
                .min(self.buffer.len());
                cursor.anchor = Self::adjust_offset_for_line_prefix_change(
                    cursor.anchor,
                    indent_start,
                    0,
                    inserted,
                )
                .min(self.buffer.len());
            }
        }

        self.restore_cursor_states(cursor_states);
        self.reparse();
        self.scroll_to_cursor();
    }

    pub(crate) fn indent_line_indices_with_widths(&mut self, line_widths: &[(usize, usize)]) {
        if line_widths.is_empty() {
            return;
        }

        let mut cursor_states = self.cursor_states_with_primary_flag();
        for &(line, width) in line_widths {
            if width == 0 {
                continue;
            }

            let indent_start = self.line_indent_start(line);
            let spaces = " ".repeat(width);
            self.buffer.insert(indent_start, &spaces);
            for (cursor, _) in &mut cursor_states {
                cursor.offset = Self::adjust_offset_for_line_prefix_change(
                    cursor.offset,
                    indent_start,
                    0,
                    width,
                )
                .min(self.buffer.len());
                cursor.anchor = Self::adjust_offset_for_line_prefix_change(
                    cursor.anchor,
                    indent_start,
                    0,
                    width,
                )
                .min(self.buffer.len());
            }
        }

        self.restore_cursor_states(cursor_states);
        self.reparse();
        self.scroll_to_cursor();
    }

    pub(crate) fn unindent_line_indices(&mut self, line_indices: &[usize], max_spaces: usize) {
        if line_indices.is_empty() || max_spaces == 0 {
            return;
        }

        let mut cursor_states = self.cursor_states_with_primary_flag();
        for &line in line_indices {
            let strip = self.leading_indent_spaces(line, max_spaces);
            if strip == 0 {
                continue;
            }

            let indent_start = self.line_indent_start(line);
            self.buffer.replace(indent_start..indent_start + strip, "");
            for (cursor, _) in &mut cursor_states {
                cursor.offset = Self::adjust_offset_for_line_prefix_change(
                    cursor.offset,
                    indent_start,
                    strip,
                    0,
                )
                .min(self.buffer.len());
                cursor.anchor = Self::adjust_offset_for_line_prefix_change(
                    cursor.anchor,
                    indent_start,
                    strip,
                    0,
                )
                .min(self.buffer.len());
            }
        }

        self.restore_cursor_states(cursor_states);
        self.reparse();
        self.scroll_to_cursor();
    }

    pub(crate) fn unindent_line_indices_with_widths(&mut self, line_widths: &[(usize, usize)]) {
        if line_widths.is_empty() {
            return;
        }

        let mut cursor_states = self.cursor_states_with_primary_flag();
        for &(line, width) in line_widths {
            if width == 0 {
                continue;
            }

            let strip = self.leading_indent_spaces(line, width);
            if strip == 0 {
                continue;
            }

            let indent_start = self.line_indent_start(line);
            self.buffer.replace(indent_start..indent_start + strip, "");
            for (cursor, _) in &mut cursor_states {
                cursor.offset = Self::adjust_offset_for_line_prefix_change(
                    cursor.offset,
                    indent_start,
                    strip,
                    0,
                )
                .min(self.buffer.len());
                cursor.anchor = Self::adjust_offset_for_line_prefix_change(
                    cursor.anchor,
                    indent_start,
                    strip,
                    0,
                )
                .min(self.buffer.len());
            }
        }

        self.restore_cursor_states(cursor_states);
        self.reparse();
        self.scroll_to_cursor();
    }

    fn multi_cursor_selected_texts(&self) -> Vec<String> {
        self.cursor_states_with_primary_flag()
            .into_iter()
            .filter_map(|(cursor, _)| {
                let range = cursor.selection_range();
                (!range.is_empty()).then(|| self.buffer.text()[range].to_string())
            })
            .collect()
    }

    fn apply_multi_cursor_paste_segments(&mut self, segments: &[String]) {
        let cursor_states = self.cursor_states_with_primary_flag();
        if cursor_states.len() != segments.len() {
            return;
        }

        let mut edits = Vec::with_capacity(cursor_states.len());
        let mut resolved: Vec<_> = cursor_states
            .into_iter()
            .zip(segments.iter().cloned())
            .map(|((cursor, is_primary), text)| (cursor, is_primary, text))
            .collect();
        resolved.sort_by(|left, right| {
            let left_start = left.0.selection_range().start;
            let right_start = right.0.selection_range().start;
            right_start.cmp(&left_start)
        });

        for (cursor, is_primary, text) in resolved {
            let range = cursor.selection_range();
            let start = range.start;
            let replaced_len = range.end.saturating_sub(range.start);
            self.buffer.replace(range, &text);
            edits.push(MultiCursorResolvedEdit {
                start,
                cursor_after: start + text.len(),
                delta: text.len() as isize - replaced_len as isize,
                is_primary,
                ordered_list_update: None,
            });
        }

        self.apply_multi_cursor_resolved_edits(edits);
    }

    /// Any overlay that owns the pointer while it is open, so window-level
    /// capture handlers must not act on what is painted underneath it.
    pub(crate) fn modal_overlay_is_open(&self) -> bool {
        self.context_menu.is_some()
            || self.dialog.is_some()
            || self.settings_font_picker.is_some()
            || self.quick_open_visible
            || self.command_palette_visible
            || self.language_picker_visible
            || self.branch_picker_visible
            || self.git_remote_input_visible
    }

    pub(crate) fn render_dialog(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        use gpui::{CursorStyle, FontWeight, MouseButton, SharedString, div, prelude::*, px};

        let ds = &self.styles.dialog;
        let backdrop_color = ds.dialog_backdrop_color;
        let btn_hover_opacity = ds.dialog_btn_hover_opacity;

        let Some(ref dialog) = self.dialog else {
            return div().into_any_element();
        };

        let confirm_label = SharedString::from(dialog.confirm_label.clone());
        let cancel_label = SharedString::from(dialog.cancel_label.clone());
        let save_label = dialog
            .save_label
            .as_ref()
            .map(|s| SharedString::from(s.clone()));
        let has_save = save_label.is_some();
        let save_btn_bg = ds.dialog_btn_primary_bg;
        let save_btn_text = ds.dialog_btn_primary_text_color;
        let secondary_bg = ds.dialog_btn_secondary_bg;
        let secondary_text = ds.dialog_btn_secondary_text_color;

        div()
            .absolute()
            .left(px(0.0))
            .top(px(0.0))
            .w_full()
            .h_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(backdrop_color)
            .cursor(CursorStyle::Arrow)
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.stop_propagation()),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|_, _, _, cx| cx.stop_propagation()),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|_, _, _, cx| cx.stop_propagation()),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|_, _, _, cx| cx.stop_propagation()),
            )
            .on_mouse_move(cx.listener(|_, _, _, cx| cx.stop_propagation()))
            .on_scroll_wheel(cx.listener(|_, _, _, cx| cx.stop_propagation()))
            .child(
                div()
                    .w(px(self.scaled(ds.dialog_width_px)))
                    .bg(ds.dialog_bg)
                    .border(px(ds.dialog_border_width_px))
                    .border_color(ds.dialog_border_color)
                    .rounded(px(ds.dialog_radius_px))
                    .px(px(self.scaled(ds.dialog_padding_x_px)))
                    .py(px(self.scaled(ds.dialog_padding_y_px)))
                    .flex()
                    .flex_col()
                    .gap(px(self.scaled(ds.dialog_gap_px)))
                    .child(
                        div()
                            .text_size(px(self.scaled(ds.dialog_title_font_size_px)))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(ds.dialog_title_text_color)
                            .child(SharedString::from(dialog.title.clone())),
                    )
                    .child(
                        div()
                            .id("dialog-message")
                            .max_h(px(self.scaled(320.0)))
                            .overflow_y_scroll()
                            .text_size(px(self.scaled(ds.dialog_message_font_size_px)))
                            .text_color(ds.dialog_message_text_color)
                            .child(SharedString::from(dialog.message.clone())),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(self.scaled(ds.dialog_btn_gap_px)))
                            .child(
                                div()
                                    .id("dialog-cancel")
                                    .h(px(self.scaled(ds.dialog_btn_height_px)))
                                    .px(px(self.scaled(ds.dialog_btn_padding_x_px)))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(secondary_bg)
                                    .text_color(secondary_text)
                                    .text_size(px(self.scaled(ds.dialog_btn_font_size_px)))
                                    .rounded(px(ds.dialog_btn_radius_px))
                                    .cursor(CursorStyle::Arrow)
                                    .hover(move |s| s.opacity(btn_hover_opacity))
                                    .child(cancel_label)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.dismiss_dialog(cx);
                                    })),
                            )
                            .child(
                                div()
                                    .id("dialog-confirm")
                                    .h(px(self.scaled(ds.dialog_btn_height_px)))
                                    .px(px(self.scaled(ds.dialog_btn_padding_x_px)))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(secondary_bg)
                                    .text_color(secondary_text)
                                    .text_size(px(self.scaled(ds.dialog_btn_font_size_px)))
                                    .rounded(px(ds.dialog_btn_radius_px))
                                    .cursor(CursorStyle::Arrow)
                                    .hover(move |s| s.opacity(btn_hover_opacity))
                                    .child(confirm_label)
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.confirm_dialog(cx);
                                    })),
                            )
                            .when(has_save, |this| {
                                this.child(
                                    div()
                                        .id("dialog-save")
                                        .h(px(self.scaled(ds.dialog_btn_height_px)))
                                        .px(px(self.scaled(ds.dialog_btn_padding_x_px)))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .bg(save_btn_bg)
                                        .text_color(save_btn_text)
                                        .text_size(px(self.scaled(ds.dialog_btn_font_size_px)))
                                        .rounded(px(ds.dialog_btn_radius_px))
                                        .cursor(CursorStyle::Arrow)
                                        .hover(move |s| s.opacity(btn_hover_opacity))
                                        .child(save_label.unwrap_or_default())
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.save_and_close_dialog(cx);
                                        })),
                                )
                            }),
                    ),
            )
            .into_any_element()
    }

    // ── Cursor blink ────────────────────────────────────────────────

    pub(crate) fn next_blink_epoch(&mut self) -> usize {
        self.blink_epoch += 1;
        self.blink_epoch
    }

    pub(crate) fn start_blinking(&mut self, cx: &mut Context<Self>) {
        self.cursor_opacity = self.styles.cursor.cursor_opacity_max;
        let epoch = self.next_blink_epoch();
        self.schedule_blink(epoch, self.styles.cursor.blink_visible_duration(), cx);
    }

    fn next_cursor_blink_opacity(current: f32, min: f32, max: f32) -> f32 {
        if current > min { min } else { max }
    }

    pub(crate) fn schedule_blink(&self, epoch: usize, delay: Duration, cx: &mut Context<Self>) {
        if !self.cursor_blink_allowed {
            return;
        }
        let min_opacity = self.styles.cursor.cursor_opacity_min;
        let max_opacity = self.styles.cursor.cursor_opacity_max;
        let transparent_dur = self.styles.cursor.blink_transparent_duration();
        let visible_dur = self.styles.cursor.blink_visible_duration();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            this.update(cx, |this, cx| {
                if this.blink_epoch != epoch || !this.cursor_blink_allowed {
                    return;
                }
                this.cursor_opacity =
                    Self::next_cursor_blink_opacity(this.cursor_opacity, min_opacity, max_opacity);
                let next_delay = if this.cursor_opacity <= min_opacity {
                    transparent_dur
                } else {
                    visible_dur
                };
                cx.notify();
                this.schedule_blink(epoch, next_delay, cx);
            })
            .ok();
        })
        .detach();
    }

    /// Starts or stops the caret animation as the caret becomes (in)visible,
    /// so an unfocused window costs nothing.
    pub(crate) fn sync_cursor_blink_allowed(&mut self, allowed: bool, cx: &mut Context<Self>) {
        if self.cursor_blink_allowed == allowed {
            return;
        }
        self.cursor_blink_allowed = allowed;
        if allowed {
            self.start_blinking(cx);
        } else {
            self.next_blink_epoch();
            self.cursor_opacity = self.styles.cursor.cursor_opacity_max;
        }
    }

    pub(crate) fn pause_cursor_blinking(&mut self, cx: &mut Context<Self>) {
        self.cursor_opacity = self.styles.cursor.cursor_opacity_max;
        let epoch = self.next_blink_epoch();
        cx.notify();
        self.schedule_blink(epoch, self.styles.cursor.blink_pause_delay(), cx);
    }

    pub(crate) fn pause_blinking(&mut self, cx: &mut Context<Self>) {
        self.commit_history_transaction();
        self.schedule_temp_save_for_active_tab(cx);
        self.schedule_auto_save_for_active_tab(cx);
        self.pause_cursor_blinking(cx);
    }

    // ── Shared utilities ────────────────────────────────────────────

    pub(crate) fn reparse(&mut self) {
        let file_kind = self
            .active_file_kind()
            .cloned()
            .unwrap_or(FileKind::Markdown);
        let pending_line_display_transfer = self.pending_line_display_transfer.take();
        if self.uses_sparse_line_geometry()
            && let Some(transfer) = pending_line_display_transfer.as_ref()
        {
            let old_line_count = transfer
                .old_end_line_exclusive
                .saturating_sub(transfer.start_line);
            let new_line_count = transfer
                .new_end_line_exclusive
                .saturating_sub(transfer.start_line);
            if old_line_count == new_line_count {
                self.clear_sparse_line_geometry_range(
                    transfer.dirty_start_line..transfer.dirty_end_line_exclusive,
                );
            } else {
                self.line_geometry_cache.borrow_mut().sparse.splice_lines(
                    transfer.start_line,
                    transfer.old_end_line_exclusive,
                    transfer.new_end_line_exclusive,
                    transfer.dirty_start_line..transfer.dirty_end_line_exclusive,
                    self.buffer.line_count(),
                    self.line_height_for_block(&BlockType::Normal) as f64,
                );
            }
        }
        // Take (don't just clear) the current shaping cache. When a transfer is
        // applied below it is restored from the transfer's own snapshot; when no
        // transfer exists we restore this snapshot as-is if the line count is
        // unchanged, so reparses triggered without a queued transfer (e.g. the
        // second reparse after ordered-list renumbering) do not force the next
        // prepaint to re-shape the entire document.
        let preserved_line_display_info = self.take_line_display_info();
        self.search_matches.clear();
        self.search_current_index = None;
        self.workspace_search_highlight_path = None;
        self.workspace_search_highlight_ranges.clear();
        self.workspace_search_highlight_current_index = None;
        let git_diff_rows = self.active_git_diff_state().map(|diff| diff.rows.clone());
        if let Some(rows) = git_diff_rows {
            self.set_decorations(rows.into_iter().map(|row| row.decoration).collect());
            if self.decorations.is_empty() {
                self.decorations.push(LineDecoration::default());
            }
        } else {
            // Unified decoration pipeline for markdown / text / code tabs:
            // apply the cheapest sound incremental strategy for the queued
            // edit, falling back to a full parse via
            // `parse_decorations_for_file_kind` (the single full-parse entry
            // point for every file kind).
            let incremental_applied = match (&file_kind, pending_line_display_transfer.as_ref()) {
                (FileKind::Code { .. } | FileKind::Text, Some(transfer)) => {
                    let code_language = match &file_kind {
                        FileKind::Code { language } => Some(language.as_str()),
                        _ => None,
                    };
                    transfer.apply_plain_decoration_transfer(&mut self.decorations, code_language);
                    self.invalidate_line_geometry_cache();
                    true
                }
                (FileKind::Markdown, Some(transfer))
                    if transfer.markdown_plain_decoration_transfer =>
                {
                    transfer.apply_plain_decoration_transfer(&mut self.decorations, None);
                    self.invalidate_line_geometry_cache();
                    true
                }
                (FileKind::Markdown, Some(transfer)) => {
                    !self.uses_lazy_markdown_decorations()
                        && self.try_incremental_markdown_decorations(transfer).is_ok()
                }
                _ => false,
            };
            if !incremental_applied {
                if file_kind.is_markdown() && self.buffer.line_count() >= LARGE_MARKDOWN_MIN_LINES {
                    let cursor_line = self.cursor_line();
                    let parse_range = pending_line_display_transfer
                        .as_ref()
                        .map(|transfer| {
                            transfer.dirty_start_line
                                ..transfer
                                    .dirty_end_line_exclusive
                                    .max(transfer.dirty_start_line + 1)
                        })
                        .unwrap_or_else(|| {
                            cursor_line.saturating_sub(LARGE_MARKDOWN_CHUNK_LINES)
                                ..cursor_line.saturating_add(LARGE_MARKDOWN_CHUNK_LINES + 1)
                        });
                    if self.lazy_markdown_decorations.is_none() {
                        self.reset_lazy_markdown_decorations();
                    } else if self.decorations.len() != self.buffer.line_count()
                        && let Some(transfer) = pending_line_display_transfer.as_ref()
                    {
                        transfer.apply_plain_decoration_transfer(&mut self.decorations, None);
                        if let Some(lazy) = self.lazy_markdown_decorations.as_mut() {
                            lazy.resize_after_structural_edit(
                                self.buffer.line_count(),
                                transfer.dirty_start_line,
                            );
                        }
                    }
                    if let Some(lazy) = self.lazy_markdown_decorations.as_mut() {
                        lazy.invalidate_range(parse_range.clone());
                    }
                    self.ensure_lazy_markdown_decorations(parse_range);
                } else {
                    self.set_decorations(parse_decorations_for_file_kind(
                        self.buffer.text(),
                        &file_kind,
                    ));
                }
            }
        }
        if file_kind.is_markdown()
            && self.uses_lazy_markdown_decorations()
            && let Some(transfer) = pending_line_display_transfer.as_ref()
        {
            self.markdown_outline_epoch = self.markdown_outline_epoch.wrapping_add(1);
            if let Some(index) = self.markdown_outline_index.borrow_mut().as_mut() {
                index.update_after_edit(
                    &self.buffer,
                    transfer.dirty_start_line,
                    transfer.old_end_line_exclusive,
                    transfer.new_end_line_exclusive,
                );
            }
            self.invalidate_outline_layout_cache();
        }
        // Restore the per-line shaping cache from the queued transfer whenever one
        // exists. For markdown this now also applies on the full-parse path: the
        // decorations are re-parsed for correctness, but unchanged lines outside
        // the computed dirty range keep their cached shaping, so the next prepaint
        // stays windowed instead of re-shaping the entire document on every
        // keystroke. Edits with non-local effects (reference/footnote definitions,
        // fence toggles) do not queue a transfer and still fall back to a full
        // re-layout.
        if let Some(transfer) = pending_line_display_transfer {
            self.set_line_display_info(transfer.apply(self.buffer.line_count()));
        } else if preserved_line_display_info.len() == self.buffer.line_count()
            && !preserved_line_display_info.is_empty()
        {
            // No transfer was queued, but the edit that triggered this reparse
            // (typically in-place ordered-list renumbering) did not change the
            // line count. Reuse the prior shaping cache so the next prepaint
            // stays windowed; the visible lines are re-shaped each frame anyway,
            // and any offscreen line whose decoration changed converges when it
            // next scrolls into view.
            self.set_line_display_info(preserved_line_display_info);
        }
        self.refresh_generated_block_previews();
        self.layout_dirty = true;
        self.invalidate_info_panel_cache();
        self.mark_inline_diff_dirty(false);
    }

    fn refresh_generated_block_previews(&mut self) {
        self.refresh_generated_block_previews_with_mode(false);
    }

    pub(crate) fn rerender_generated_block_previews(&mut self) {
        self.pending_preview_reveal = Some(PendingPreviewReveal {
            scroll_offset: self.scroll_offset,
        });
        self.refresh_generated_block_previews_with_mode(true);
    }

    pub(crate) fn invalidate_generated_block_preview_render_caches(&mut self) {
        for cache_key in self.generated_preview_keys.clone() {
            self.image_cache.remove(&cache_key);
            self.generated_preview_svg_cache.remove(&cache_key);
            self.generated_preview_overlay_cache.remove(&cache_key);
        }
        self.tab_generated_preview_cache.clear();
        for pane in &mut self.panes {
            pane.state.tab_generated_preview_cache.clear();
            for cache_key in pane.state.generated_preview_keys.clone() {
                self.image_cache.remove(&cache_key);
                self.generated_preview_svg_cache.remove(&cache_key);
                self.generated_preview_overlay_cache.remove(&cache_key);
            }
        }
    }

    pub(crate) fn ensure_visible_generated_block_previews(&mut self, cx: &mut Context<Self>) {
        if self.decorations.is_empty() {
            return;
        }
        let (visible_start, visible_end) = self.generated_preview_priority_line_range();
        let visible_start = visible_start.min(self.decorations.len().saturating_sub(1));
        let visible_end = visible_end.min(self.decorations.len().saturating_sub(1));
        let needs_visible_preview =
            self.decorations[visible_start..=visible_end]
                .iter()
                .any(|decoration| {
                    let Some(preview) = decoration.block_preview.as_ref() else {
                        return false;
                    };
                    preview.is_anchor && !self.image_cache.contains_key(&preview.cache_key)
                });
        if needs_visible_preview {
            self.refresh_generated_block_previews();
            cx.notify();
        }
    }

    /// Debounced live rebuild of the chart/formula being edited in code view: the
    /// in-edit raster is frozen on each keystroke (see the freeze in
    /// `refresh_generated_block_previews_with_mode`); this rebuilds it once
    /// typing settles so the render updates live yet stays steady while typing.
    /// Each call resets the timer.
    pub(crate) fn schedule_generated_preview_rerender(&mut self, cx: &mut Context<Self>) {
        let pane_id = self.current_pane_id();
        self.generated_preview_rerender_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(350))
                .await;
            let _ = this.update(cx, |this, cx| {
                this.with_pane_if_alive(pane_id, |this| {
                    this.generated_preview_rerender_task = None;
                    this.rerender_generated_block_previews();
                    cx.notify();
                });
            });
        }));
    }

    fn refresh_generated_block_previews_with_mode(&mut self, force: bool) {
        if self.active_tab_is_diff() {
            self.prune_generated_preview_caches(HashSet::new());
            return;
        }

        let mut preview_entries = Vec::new();
        for decoration in &self.decorations {
            let Some(preview) = decoration.block_preview.as_ref() else {
                continue;
            };
            if !preview.is_anchor || preview.source_text.is_empty() {
                continue;
            }
            preview_entries.push(preview.clone());
        }
        if let Some(inline_diff) = self.active_inline_diff() {
            for hunk in &inline_diff.hunks {
                for removed in &hunk.removed_lines {
                    let Some(preview) = removed.decoration.block_preview.as_ref() else {
                        continue;
                    };
                    if preview.is_anchor && !preview.source_text.is_empty() {
                        preview_entries.push(preview.clone());
                    }
                }
            }
        }

        let next_generated_keys = preview_entries
            .iter()
            .map(|preview| preview.cache_key.clone())
            .collect::<HashSet<_>>();

        // Freeze the chart/formula currently being edited in code view. Its cache
        // key is content-derived, so each keystroke yields a new key with no
        // cached image and forces an immediate re-raster — which made the render
        // jitter on every keystroke. Carry the previous raster (and its SVG) onto
        // the new key so the rebuild loop below skips it; a debounced rebuild
        // (scheduled from prepaint) refreshes it live once typing settles, and an
        // immediate rebuild runs when the cursor leaves code view.
        if !force
            && let Some(old_key) = self.code_mode_preview_key.clone()
            && let Some(new_key) = self.focused_code_mode_preview_key()
            && old_key != new_key
            && !self.image_cache.contains_key(&new_key)
        {
            let frozen_image = match self.image_cache.get(&old_key) {
                Some(Some(image)) => Some(image.clone()),
                _ => None,
            };
            let frozen_svg = self.generated_preview_svg_cache.get(&old_key).cloned();
            if let (Some(frozen_image), Some(frozen_svg)) = (frozen_image, frozen_svg) {
                self.image_cache.insert(new_key.clone(), Some(frozen_image));
                self.generated_preview_svg_cache.insert(new_key, frozen_svg);
                self.generated_preview_rerender_pending = true;
            }
        }

        let (priority_start, priority_end) = self.generated_preview_priority_line_range();
        preview_entries.sort_by_key(|preview| {
            let in_priority_range = generated_preview_intersects_range(
                preview.start_line,
                preview.end_line,
                priority_start,
                priority_end,
            );
            let cursor_distance = preview.start_line.abs_diff(
                self.cursor_line()
                    .min(self.buffer.line_count().saturating_sub(1)),
            );
            (!in_priority_range, cursor_distance, preview.start_line)
        });

        let mut rendered_count = 0usize;
        for preview in preview_entries {
            if !generated_preview_intersects_range(
                preview.start_line,
                preview.end_line,
                priority_start,
                priority_end,
            ) {
                continue;
            }
            let has_cached_image = self.image_cache.contains_key(&preview.cache_key);
            if !force
                && has_cached_image
                && self
                    .generated_preview_svg_cache
                    .contains_key(&preview.cache_key)
            {
                continue;
            }

            if rendered_count >= GENERATED_PREVIEW_RENDER_BUDGET {
                continue;
            }

            let svg = helpers::build_generated_block_preview_svg(
                preview.kind,
                &preview.source_text,
                &self.theme,
                helpers::GeneratedPreviewFontFamilies {
                    monospace: self.styles.editor.editor_monospace_font_family,
                    body_cjk: self.styles.editor.editor_body_cjk_font_family,
                    mermaid_theme: self.app_settings.mermaid_theme(),
                },
            );
            // The SVG fully determines the rasterized image. On a forced refresh
            // (theme/font/settings change) only re-rasterize when the SVG actually
            // changed; settings that do not affect previews (line height, letter
            // spacing, UI font) leave it identical, so the cached image is reused
            // instead of rebuilding every chart/formula.
            let svg_unchanged = self
                .generated_preview_svg_cache
                .get(&preview.cache_key)
                .is_some_and(|cached| cached.as_ref() == svg.as_str());
            if force && has_cached_image && svg_unchanged {
                continue;
            }
            self.generated_preview_svg_cache
                .insert(preview.cache_key.clone(), Arc::<str>::from(svg.clone()));
            let image = helpers::load_generated_preview_image_from_svg(&svg, preview.kind)
                .ok()
                .map(Arc::new);
            self.image_cache.insert(preview.cache_key.clone(), image);
            self.generated_preview_overlay_cache
                .remove(&preview.cache_key);
            rendered_count += 1;
        }

        // Preserve the zoom/pan of the preview currently being edited in code
        // view across content changes: the cache key is content-derived, so each
        // keystroke produces a new key. Carry the old key's view state onto the
        // new key (before the prune below) so the render keeps the user's zoom
        // and pan instead of snapping back to the default fit on every edit.
        if let Some(old_key) = self.code_mode_preview_key.clone()
            && !next_generated_keys.contains(&old_key)
            && let Some(new_key) = self.focused_code_mode_preview_key()
            && next_generated_keys.contains(&new_key)
        {
            let view = self
                .generated_preview_views
                .get(&old_key)
                .copied()
                .unwrap_or_default();
            self.generated_preview_views.insert(new_key.clone(), view);
            self.code_mode_preview_key = Some(new_key);
        }

        self.prune_generated_preview_caches(next_generated_keys);
    }

    fn generated_preview_priority_line_range(&self) -> (usize, usize) {
        let line_count = self.buffer.line_count();
        if line_count == 0 {
            return (0, 0);
        }

        let viewport_start = self
            .line_at_y((self.scroll_offset - self.padding_top()).max(0.0))
            .min(line_count.saturating_sub(1));
        let viewport_end = self
            .line_at_y(self.scroll_offset + self.editor_visible_height() + self.padding_bottom())
            .min(line_count.saturating_sub(1));
        (viewport_start, viewport_end)
    }

    fn prune_generated_preview_caches(&mut self, next_generated_keys: HashSet<String>) {
        let previous_generated_keys =
            std::mem::replace(&mut self.generated_preview_keys, next_generated_keys);
        for stale_key in previous_generated_keys {
            if self.generated_preview_keys.contains(&stale_key) {
                continue;
            }
            self.image_cache.remove(&stale_key);
            self.generated_preview_svg_cache.remove(&stale_key);
            self.generated_preview_overlay_cache.remove(&stale_key);
            self.generated_preview_views.remove(&stale_key);
        }
        self.generated_preview_views
            .retain(|cache_key, _| self.generated_preview_keys.contains(cache_key));
        self.trim_generated_preview_render_caches();
    }

    fn generated_preview_cache_bytes(&self, cache_key: &str) -> usize {
        fn decoded_bytes(image: &RenderImage) -> usize {
            let size = image.size(0);
            let width = i32::from(size.width).max(0) as usize;
            let height = i32::from(size.height).max(0) as usize;
            width * height * 4
        }

        let mut bytes = 0;
        if let Some(Some(image)) = self.image_cache.get(cache_key) {
            bytes += decoded_bytes(image);
        }
        if let Some(svg) = self.generated_preview_svg_cache.get(cache_key) {
            bytes += svg.len();
        }
        if let Some((_, image)) = self.generated_preview_overlay_cache.get(cache_key) {
            bytes += decoded_bytes(image);
        }
        bytes
    }

    fn trim_generated_preview_render_caches(&mut self) {
        let total_bytes = self
            .generated_preview_keys
            .iter()
            .map(|cache_key| self.generated_preview_cache_bytes(cache_key))
            .sum::<usize>();
        if self.generated_preview_keys.len() <= GENERATED_PREVIEW_CACHE_RETAIN
            && total_bytes <= GENERATED_PREVIEW_CACHE_BYTE_BUDGET
        {
            return;
        }

        let (priority_start, priority_end) = self.generated_preview_priority_line_range();
        let cursor_line = self
            .cursor_line()
            .min(self.buffer.line_count().saturating_sub(1));
        let mut ranked_keys = self
            .decorations
            .iter()
            .filter_map(|decoration| decoration.block_preview.as_ref())
            .filter(|preview| preview.is_anchor)
            .filter(|preview| {
                self.image_cache.contains_key(&preview.cache_key)
                    || self
                        .generated_preview_svg_cache
                        .contains_key(&preview.cache_key)
                    || self
                        .generated_preview_overlay_cache
                        .contains_key(&preview.cache_key)
                    || self
                        .generated_preview_views
                        .contains_key(&preview.cache_key)
            })
            .map(|preview| {
                let in_priority_range =
                    preview.start_line <= priority_end && preview.end_line >= priority_start;
                let cursor_distance = preview.start_line.abs_diff(cursor_line);
                (
                    !in_priority_range,
                    cursor_distance,
                    preview.cache_key.clone(),
                )
            })
            .collect::<Vec<_>>();

        ranked_keys.sort();
        let mut keep_keys = HashSet::new();
        let mut kept_bytes = 0usize;
        for (_, _, cache_key) in ranked_keys.into_iter().take(GENERATED_PREVIEW_CACHE_RETAIN) {
            let bytes = self.generated_preview_cache_bytes(&cache_key);
            // Keep the closest preview even when it alone exceeds the budget,
            // otherwise the one on screen would be dropped every frame.
            if !keep_keys.is_empty() && kept_bytes + bytes > GENERATED_PREVIEW_CACHE_BYTE_BUDGET {
                break;
            }
            kept_bytes += bytes;
            keep_keys.insert(cache_key);
        }
        if let Some(cache_key) = self.code_mode_preview_key.as_ref() {
            keep_keys.insert(cache_key.clone());
        }
        if let Some(overlay) = self.preview_overlay.as_ref() {
            keep_keys.insert(overlay.cache_key.clone());
        }

        for cache_key in self.generated_preview_keys.iter() {
            if keep_keys.contains(cache_key) {
                continue;
            }
            self.image_cache.remove(cache_key);
            self.generated_preview_svg_cache.remove(cache_key);
            self.generated_preview_overlay_cache.remove(cache_key);
            self.generated_preview_views.remove(cache_key);
        }
    }

    pub(crate) fn active_file_kind(&self) -> Option<&FileKind> {
        self.active_file_kind_cache.as_ref()
    }

    pub(crate) fn active_tab_supports_text_buffer(&self) -> bool {
        self.active_file_kind()
            .is_some_and(FileKind::supports_text_buffer)
    }

    pub(crate) fn active_tab_is_non_markdown_text(&self) -> bool {
        self.active_file_kind()
            .is_some_and(|file_kind| file_kind.supports_text_buffer() && !file_kind.is_markdown())
    }

    /// Buffer ranges of all occurrences of the currently selected text, excluding
    /// the active selection itself. Returns empty for trivial/whitespace/multi-line
    /// selections to avoid visual noise.
    pub(crate) fn selection_match_ranges(&self) -> Vec<std::ops::Range<usize>> {
        let sel = self.cursor.selection_range();
        if sel.start >= sel.end {
            return Vec::new();
        }
        let text = self.buffer.text();
        if sel.end > text.len() {
            return Vec::new();
        }
        let sel = self.buffer.clamp_offset(sel.start)..self.buffer.clamp_offset(sel.end);
        if sel.start >= sel.end {
            return Vec::new();
        }
        let needle = &text[sel.start..sel.end];
        if needle.is_empty()
            || needle.len() > 200
            || needle.contains('\n')
            || needle.trim().is_empty()
        {
            return Vec::new();
        }

        let mut ranges = Vec::new();
        let mut search_from = 0usize;
        while let Some(rel) = text[search_from..].find(needle) {
            let start = search_from + rel;
            let end = start + needle.len();
            if !(start == sel.start && end == sel.end) {
                ranges.push(start..end);
                if ranges.len() >= 500 {
                    break;
                }
            }
            search_from = end;
            if search_from >= text.len() {
                break;
            }
            while !text.is_char_boundary(search_from) {
                search_from += 1;
            }
        }
        ranges
    }

    fn bracket_matching_enabled_at(&self, offset: usize) -> bool {
        if self.active_tab_is_non_markdown_text() {
            return true;
        }
        if !self.active_tab_is_markdown() {
            return false;
        }
        let (line, _) = self.buffer.offset_to_line_col(offset);
        self.decorations.get(line).is_some_and(|decoration| {
            matches!(
                decoration.block_type,
                crate::markdown::BlockType::CodeBlock | crate::markdown::BlockType::CodeBlockFence
            )
        })
    }

    /// When the cursor sits next to a bracket (and bracket matching is enabled for
    /// the current context), returns the two single-char buffer ranges of the
    /// bracket and its nesting-aware partner.
    pub(crate) fn bracket_match_ranges(&self) -> Vec<std::ops::Range<usize>> {
        if self.cursor.has_selection() {
            return Vec::new();
        }
        let cursor = self.cursor.offset;
        if !self.bracket_matching_enabled_at(cursor) {
            return Vec::new();
        }
        let text = self.buffer.text();
        let bytes = text.as_bytes();
        let is_bracket = |b: u8| matches!(b, b'(' | b')' | b'[' | b']' | b'{' | b'}');

        let bracket_pos = if cursor > 0 && cursor <= bytes.len() && is_bracket(bytes[cursor - 1]) {
            Some(cursor - 1)
        } else if cursor < bytes.len() && is_bracket(bytes[cursor]) {
            Some(cursor)
        } else {
            None
        };
        let Some(pos) = bracket_pos else {
            return Vec::new();
        };
        let Some(partner) = matching_bracket_offset(&text, pos) else {
            return Vec::new();
        };
        vec![pos..pos + 1, partner..partner + 1]
    }

    pub(crate) fn active_tab_supports_text_search(&self) -> bool {
        self.active_file_kind()
            .is_some_and(FileKind::supports_text_search)
    }

    pub(crate) fn active_tab_supports_persistent_save(&self) -> bool {
        !self.active_tab_is_read_only()
            && self
                .active_file_kind()
                .is_some_and(FileKind::supports_persistent_save)
    }

    pub(crate) fn sync_active_tab_dirty_state(&mut self) {
        let Some(index) = self.active_tab_index() else {
            return;
        };
        let Some(tab) = self.tabs.get_mut(index) else {
            return;
        };
        if tab.kind != TabKind::Editor {
            tab.is_dirty = false;
            return;
        }
        if tab.file_kind.as_ref().is_some_and(FileKind::is_whiteboard) {
            // Whiteboard dirty state is driven by canvas mutations, not text diff.
            return;
        }
        if !tab
            .file_kind
            .as_ref()
            .is_some_and(FileKind::supports_text_buffer)
        {
            tab.is_dirty = false;
            return;
        }

        let current_text = self.buffer.text();
        tab.is_dirty = match tab.path.as_ref() {
            Some(path) => std::fs::read_to_string(path)
                .map(|saved_text| text_tab_is_dirty(Some(saved_text.as_str()), current_text))
                .unwrap_or(true),
            None => text_tab_is_dirty(None, current_text),
        };
    }

    pub(crate) fn active_tab_is_markdown(&self) -> bool {
        self.active_file_kind().is_some_and(FileKind::is_markdown)
    }

    pub(crate) fn active_tab_is_image(&self) -> bool {
        self.active_file_kind().is_some_and(FileKind::is_image)
    }

    pub(crate) fn active_tab_is_binary(&self) -> bool {
        self.active_file_kind().is_some_and(FileKind::is_binary)
    }

    pub(crate) fn active_tab_is_whiteboard(&self) -> bool {
        self.active_file_kind().is_some_and(FileKind::is_whiteboard)
    }

    pub(crate) fn active_whiteboard(&self) -> Option<&WhiteboardTabState> {
        self.active_tab_index()
            .and_then(|index| self.tabs.get(index))
            .and_then(|tab| tab.whiteboard.as_ref())
    }

    pub(crate) fn active_whiteboard_mut(&mut self) -> Option<&mut WhiteboardTabState> {
        let index = self.active_tab_index()?;
        self.tabs.get_mut(index)?.whiteboard.as_mut()
    }

    pub(crate) fn active_tab_is_diff(&self) -> bool {
        self.active_tab_id
            .as_ref()
            .and_then(|id| self.tabs.iter().find(|tab| &tab.id == id))
            .is_some_and(|tab| tab.kind == TabKind::Diff)
    }

    pub(crate) fn active_tab_is_read_only(&self) -> bool {
        self.embedded_read_only || self.active_tab_is_diff()
    }

    pub(crate) fn active_image_view_state(&self) -> Option<ImageViewState> {
        self.active_tab_index()
            .and_then(|index| self.tabs.get(index))
            .and_then(|tab| {
                tab.file_kind
                    .as_ref()
                    .filter(|kind| kind.is_image())
                    .map(|_| tab.image_view)
            })
    }

    pub(crate) fn active_image_view_state_mut(&mut self) -> Option<&mut ImageViewState> {
        let index = self.active_tab_index()?;
        let tab = self.tabs.get_mut(index)?;
        tab.file_kind.as_ref().filter(|kind| kind.is_image())?;
        Some(&mut tab.image_view)
    }

    pub(crate) fn reset_active_image_view(&mut self) {
        if let Some(image_view) = self.active_image_view_state_mut() {
            *image_view = ImageViewState::default();
        }
    }

    pub(crate) fn clamp_active_image_view(&mut self) {
        let image_size = self.active_image_dimensions();
        let viewport_width = self.last_viewport_width.max(1.0);
        let viewport_height = self.last_viewport_height.max(1.0);
        if let Some(image_view) = self.active_image_view_state_mut() {
            image_view.zoom_factor = image_view.zoom_factor.clamp(0.2, 8.0);
            if let Some((image_width, image_height)) = image_size {
                let fit_scale = viewport_width / image_width;
                let display_width = image_width * fit_scale * image_view.zoom_factor;
                let display_height = image_height * fit_scale * image_view.zoom_factor;
                let limit_x = ((display_width - viewport_width) / 2.0).max(0.0);
                let limit_y = if display_height > viewport_height {
                    (display_height - viewport_height).max(0.0)
                } else {
                    ((viewport_height - display_height) / 2.0).max(0.0)
                };
                image_view.pan_x = image_view.pan_x.clamp(-limit_x, limit_x);
                image_view.pan_y = if display_height > viewport_height {
                    image_view.pan_y.clamp(-limit_y, 0.0)
                } else {
                    image_view.pan_y.clamp(-limit_y, limit_y)
                };
            } else {
                image_view.pan_x = 0.0;
                image_view.pan_y = 0.0;
            }
        }
    }

    pub(crate) fn preview_overlay_state(&self) -> Option<&PreviewOverlayState> {
        self.preview_overlay.as_ref()
    }

    pub(crate) fn preview_overlay_state_mut(&mut self) -> Option<&mut PreviewOverlayState> {
        self.preview_overlay.as_mut()
    }

    pub(crate) fn close_preview_overlay(&mut self) {
        // Exiting fullscreen resets the inline rendered view (zoom/pan) so it
        // returns to its default fitted state rather than keeping the overlay's.
        if let Some(overlay) = self.preview_overlay.take() {
            self.reset_generated_preview_view(&overlay.cache_key);
        }
        self.preview_overlay_bounds = None;
        self.preview_overlay_image_bounds = None;
        self.preview_overlay_action_areas.clear();
        self.hovering_preview_overlay_action = None;
        self.image_drag_state = None;
    }

    pub(crate) fn generated_preview_view(&self, cache_key: &str) -> ImageViewState {
        self.generated_preview_views
            .get(cache_key)
            .copied()
            .unwrap_or_default()
    }

    pub(crate) fn generated_preview_view_mut(&mut self, cache_key: &str) -> &mut ImageViewState {
        self.generated_preview_views
            .entry(cache_key.to_string())
            .or_default()
    }

    pub(crate) fn reset_generated_preview_view(&mut self, cache_key: &str) {
        self.generated_preview_views
            .insert(cache_key.to_string(), ImageViewState::default());
    }

    /// The cache key of the preview whose source currently contains the cursor
    /// (i.e. the one shown in code/edit mode), or None when the cursor is not
    /// editing any preview.
    pub(crate) fn focused_code_mode_preview_key(&self) -> Option<String> {
        let cursor_line = self.buffer.offset_to_line_col(self.cursor.offset).0;
        let preview = self.decorations.get(cursor_line)?.block_preview.as_ref()?;
        if (preview.start_line..=preview.end_line).contains(&cursor_line)
            && preview.source_range.start <= self.cursor.offset
            && self.cursor.offset <= preview.source_range.end
        {
            Some(preview.cache_key.clone())
        } else {
            None
        }
    }

    pub(crate) fn zoom_generated_preview_view(&mut self, cache_key: &str, factor: f32) {
        let view = self.generated_preview_view_mut(cache_key);
        view.zoom_factor = (view.zoom_factor * factor).clamp(0.25, 6.0);
        view.pan_x = view.pan_x.clamp(-5000.0, 5000.0);
        view.pan_y = view.pan_y.clamp(-5000.0, 5000.0);
    }

    pub(crate) fn sync_generated_preview_action_opacity(&mut self, cx: &mut Context<Self>) {
        let visible_line = self
            .hovering_generated_preview_action
            .map(|(line, _)| line)
            .or(self.hovering_generated_preview_line)
            .or(self.hovering_code_block_start);
        if visible_line.is_some() {
            self.generated_preview_actions_visible_line = visible_line;
        }
        let target = if visible_line.is_some() { 1.0 } else { 0.0 };
        if (self.generated_preview_actions_opacity_target - target).abs() <= 0.001 {
            return;
        }

        self.generated_preview_actions_opacity_target = target;
        self.generated_preview_actions_opacity_epoch += 1;
        let epoch = self.generated_preview_actions_opacity_epoch;
        let pane_id = self.current_pane_id();
        self.generated_preview_actions_opacity_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let done = this
                    .update(cx, |this, cx| {
                        this.with_pane_if_alive(pane_id, |this| {
                            if this.generated_preview_actions_opacity_epoch != epoch {
                                return true;
                            }
                            let diff = this.generated_preview_actions_opacity_target
                                - this.generated_preview_actions_opacity;
                            if diff.abs() <= 0.01 {
                                this.generated_preview_actions_opacity =
                                    this.generated_preview_actions_opacity_target;
                                if this.generated_preview_actions_opacity <= 0.01 {
                                    this.generated_preview_actions_visible_line = None;
                                }
                                this.generated_preview_actions_opacity_task = None;
                                cx.notify();
                                return true;
                            }
                            this.generated_preview_actions_opacity += diff * 0.22;
                            cx.notify();
                            false
                        })
                        .unwrap_or(true)
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

    pub(crate) fn reset_preview_overlay_view(&mut self) {
        if let Some(overlay) = self.preview_overlay_state_mut() {
            overlay.view = ImageViewState::default();
        }
    }

    pub(crate) fn zoom_preview_overlay_view(&mut self, factor: f32) {
        if let Some(overlay) = self.preview_overlay_state_mut() {
            overlay.view.zoom_factor = (overlay.view.zoom_factor * factor).clamp(0.25, 6.0);
        }
        self.clamp_preview_overlay_view();
    }

    pub(crate) fn preview_overlay_dimensions(&self) -> Option<(f32, f32)> {
        let overlay = self.preview_overlay.as_ref()?;
        let image = self.image_cache.get(&overlay.cache_key)?.as_ref()?;
        let size = image.size(0);
        Some((size.width.0 as f32, size.height.0 as f32))
    }

    pub(crate) fn clamp_preview_overlay_view(&mut self) {
        let Some(bounds) = self.preview_overlay_bounds else {
            return;
        };
        let Some((image_width, image_height)) = self.preview_overlay_dimensions() else {
            return;
        };
        let Some(overlay) = self.preview_overlay.as_mut() else {
            return;
        };

        let view = &mut overlay.view;
        view.zoom_factor = view.zoom_factor.clamp(0.1, 6.0);

        let viewport_width = f32::from(bounds.size.width).max(1.0);
        let viewport_height = f32::from(bounds.size.height).max(1.0);
        let preview_kind = self
            .decorations
            .get(overlay.line_idx)
            .and_then(|decoration| decoration.block_preview.as_ref())
            .map(|preview| preview.kind);
        let (intrinsic_width, intrinsic_height) = if let Some(preview_kind) = preview_kind {
            helpers::intrinsic_generated_preview_size_for_kind(
                image_width,
                image_height,
                preview_kind,
            )
        } else {
            (image_width, image_height)
        };
        let fit_scale = if preview_kind.is_some() {
            viewport_width / intrinsic_width.max(1.0)
        } else {
            (viewport_width / intrinsic_width.max(1.0))
                .min(viewport_height / intrinsic_height.max(1.0))
                .min(1.0)
        };
        let display_width = intrinsic_width * fit_scale * view.zoom_factor;
        let display_height = intrinsic_height * fit_scale * view.zoom_factor;
        let limit_x = ((display_width - viewport_width) / 2.0).max(0.0);
        let limit_y = if preview_kind.is_some() {
            if display_height > viewport_height {
                (display_height - viewport_height).max(0.0)
            } else {
                ((viewport_height - display_height) / 2.0).max(0.0)
            }
        } else if display_height > viewport_height {
            ((display_height - viewport_height) / 2.0).max(0.0)
        } else {
            ((viewport_height - display_height) / 2.0).max(0.0)
        };

        view.pan_x = view.pan_x.clamp(-limit_x, limit_x);
        view.pan_y = if preview_kind.is_some() {
            if display_height > viewport_height {
                view.pan_y.clamp(-limit_y, 0.0)
            } else {
                view.pan_y.clamp(-limit_y, limit_y)
            }
        } else {
            view.pan_y.clamp(-limit_y, limit_y)
        };
    }

    pub(crate) fn active_image_dimensions(&self) -> Option<(f32, f32)> {
        let tab = self
            .active_tab_index()
            .and_then(|index| self.tabs.get(index))?;
        let path = tab.path.as_ref()?;
        let key = path.to_string_lossy();
        let image = self.image_cache.get(key.as_ref())?.as_ref()?;
        let size = image.size(0);
        Some((size.width.0 as f32, size.height.0 as f32))
    }

    pub(crate) fn cursor_line(&self) -> usize {
        self.buffer.offset_to_line_col(self.cursor.offset).0
    }

    /// Compute the byte column where content starts on a line (after block prefix).
    /// Returns 0 if the line has no block prefix.
    pub(crate) fn line_content_start_col(&self, line: usize) -> usize {
        let block_type = self.decorations.get(line).map(|d| &d.block_type);
        let content = self.buffer.line_content(line);
        let leading_ws = content.len() - content.trim_start_matches([' ', '\t']).len();
        let trimmed = &content[leading_ws..];
        let prefix_len = match block_type {
            Some(BlockType::OrderedList) => markdown::ordered_list_prefix_len(trimmed),
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
            Some(BlockType::TaskList(_)) => task_prefix_len(trimmed),
            Some(BlockType::Blockquote) => {
                if trimmed.starts_with("> ") {
                    2
                } else if trimmed.starts_with(">") {
                    1
                } else {
                    0
                }
            }
            Some(BlockType::Heading(n)) => (*n as usize) + 1, // "# " = level + space
            _ => 0,
        };
        if prefix_len > 0 {
            leading_ws + prefix_len
        } else {
            0
        }
    }

    /// After vertical cursor movement, if cursor lands within the block prefix
    /// of the destination line, snap it to the content start.
    pub(crate) fn snap_cursor_to_content(&mut self) {
        let line = self.cursor_line();
        let line_start = self.buffer.line_start(line);
        let previous_offset = self.cursor.offset;
        let cursor_col = self.cursor.offset - line_start;
        if let Some(col) = self.table_cell_col_for_line_col(line, cursor_col) {
            if col != cursor_col {
                self.cursor.move_to(line_start + col, &self.buffer);
                self.clear_wrap_boundary_affinity();
            }
            return;
        }
        let content_start = self.line_content_start_col(line);
        if content_start > 0 && cursor_col < content_start {
            self.cursor
                .move_to(line_start + content_start, &self.buffer);
            if self.cursor.offset != previous_offset {
                self.clear_wrap_boundary_affinity();
            }
        }
    }

    /// The `|---|` row is never painted, so a caret must not rest on it: move
    /// one more row in the direction the caret was travelling.
    pub(crate) fn skip_table_separator_line(&mut self, downward: bool) {
        let line = self.cursor_line();
        let is_separator = self
            .decorations
            .get(line)
            .and_then(|decoration| decoration.table_info.as_ref())
            .is_some_and(|table| table.is_separator);
        if !is_separator {
            return;
        }
        let target = if downward {
            if line + 1 >= self.buffer.line_count() {
                return;
            }
            line + 1
        } else {
            if line == 0 {
                return;
            }
            line - 1
        };
        let col = self.cursor.offset - self.buffer.line_start(line);
        let target_start = self.buffer.line_start(target);
        let target_len = self.buffer.line_content(target).len();
        self.cursor
            .move_to(target_start + col.min(target_len), &self.buffer);
        self.snap_cursor_to_content();
    }

    /// Table rows only paint a caret inside a cell's own range; a column that
    /// falls on a `|` (or the separator row) would leave the caret invisible.
    /// Returns the nearest column inside a cell, or `None` for non-table lines.
    fn table_cell_col_for_line_col(&self, line: usize, col: usize) -> Option<usize> {
        let table_info = self.decorations.get(line)?.table_info.as_ref()?;
        if table_info.is_separator || table_info.cell_ranges.is_empty() {
            return None;
        }
        if table_info
            .cell_ranges
            .iter()
            .any(|range| (range.start..=range.end).contains(&col))
        {
            return Some(col);
        }
        table_info
            .cell_ranges
            .iter()
            .map(|range| {
                let clamped = col.clamp(range.start, range.end);
                (clamped.abs_diff(col), clamped)
            })
            .min_by_key(|(distance, _)| *distance)
            .map(|(_, clamped)| clamped)
    }

    pub(crate) fn has_extra_cursors(&self) -> bool {
        !self.extra_cursors.is_empty()
    }

    pub(crate) fn marked_ranges(&self) -> impl Iterator<Item = &Range<usize>> {
        self.marked_range
            .iter()
            .chain(self.extra_marked_ranges.iter())
    }

    pub(crate) fn clear_marked_ranges(&mut self) {
        self.marked_range = None;
        self.extra_marked_ranges.clear();
    }

    pub(crate) fn clear_vertical_move_goal_x(&mut self) {
        self.vertical_move_goal_x = None;
        self.last_scroll_cursor_visual_x = None;
    }

    pub(crate) fn set_wrap_boundary_affinity(
        &mut self,
        offset: usize,
        kind: WrapBoundaryAffinityKind,
    ) {
        self.wrap_boundary_affinity = Some(WrapBoundaryAffinity { offset, kind });
    }

    pub(crate) fn clear_wrap_boundary_affinity(&mut self) {
        self.wrap_boundary_affinity = None;
    }

    pub(crate) fn reset_cursor_visual_targeting(&mut self) {
        self.clear_vertical_move_goal_x();
        self.clear_wrap_boundary_affinity();
    }

    pub(crate) fn clear_extra_cursors(&mut self) {
        self.extra_cursors.clear();
        self.extra_marked_ranges.clear();
    }

    fn collect_multi_cursor_targets_desc(&self) -> Vec<(usize, usize, bool)> {
        let mut all: Vec<(usize, usize, bool)> = Vec::with_capacity(1 + self.extra_cursors.len());
        all.push((self.cursor.offset, self.cursor.anchor, true));
        for cursor in &self.extra_cursors {
            all.push((cursor.offset, cursor.anchor, false));
        }
        all.sort_by(|a, b| {
            let a_start = a.0.min(a.1);
            let b_start = b.0.min(b.1);
            b_start.cmp(&a_start)
        });
        all
    }

    fn apply_multi_cursor_resolved_edits(
        &mut self,
        edits: Vec<MultiCursorResolvedEdit>,
    ) -> Vec<(usize, bool)> {
        let (cursor_positions, ordered_list_updates) = finalize_multi_cursor_edits(edits);

        self.extra_cursors.clear();
        for (pos, is_primary) in cursor_positions {
            let clamped = pos.min(self.buffer.len());
            if is_primary {
                self.cursor.offset = clamped;
                self.cursor.anchor = clamped;
            } else {
                self.extra_cursors.push(Cursor {
                    offset: clamped,
                    anchor: clamped,
                });
            }
        }

        if !self.extra_cursors.is_empty() {
            self.dedup_cursors();
        }

        ordered_list_updates
    }

    /// Collect all cursor offsets+anchors sorted by offset (ascending).
    pub(crate) fn all_cursor_ranges(&self) -> Vec<(usize, usize)> {
        let mut ranges: Vec<(usize, usize)> = Vec::with_capacity(1 + self.extra_cursors.len());
        ranges.push((self.cursor.offset, self.cursor.anchor));
        for c in &self.extra_cursors {
            ranges.push((c.offset, c.anchor));
        }
        ranges.sort_by_key(|&(offset, _)| offset);
        ranges
    }

    pub(crate) fn all_cursors_snapshot(&self) -> Vec<Cursor> {
        let mut cursors = Vec::with_capacity(1 + self.extra_cursors.len());
        cursors.push(self.cursor);
        cursors.extend(self.extra_cursors.iter().copied());
        cursors
    }

    /// Add a new cursor at the given offset, deduplicating if one already exists there.
    /// The new cursor becomes the primary (for IME positioning); the old primary
    /// is moved to extra_cursors.
    pub(crate) fn add_cursor_at(&mut self, offset: usize, anchor: usize) {
        let offset = offset.min(self.buffer.len());
        let anchor = anchor.min(self.buffer.len());
        // Check no existing cursor is at this exact location
        if self.cursor.offset == offset && self.cursor.anchor == anchor {
            return;
        }
        for c in &self.extra_cursors {
            if c.offset == offset && c.anchor == anchor {
                return;
            }
        }
        // Move current primary to extra, make new one primary
        let old_primary = Cursor {
            offset: self.cursor.offset,
            anchor: self.cursor.anchor,
        };
        self.extra_cursors.push(old_primary);
        self.cursor.offset = offset;
        self.cursor.anchor = anchor;
    }

    /// Apply a text insertion across all cursors. Cursors are processed from end
    /// to start so that earlier offsets remain valid.
    pub(crate) fn multi_cursor_insert(&mut self, text: &str) {
        if self.extra_cursors.is_empty() {
            self.cursor.insert(&mut self.buffer, text);
            return;
        }
        let all = self.collect_multi_cursor_targets_desc();

        let text_len = text.len();
        let mut edits = Vec::with_capacity(all.len());

        for (offset, anchor, is_primary) in &all {
            let range_start = (*offset).min(*anchor);
            let range_end = (*offset).max(*anchor);
            self.buffer.replace(range_start..range_end, text);
            edits.push(MultiCursorResolvedEdit {
                start: range_start,
                cursor_after: range_start + text_len,
                delta: text_len as isize - (range_end - range_start) as isize,
                is_primary: *is_primary,
                ordered_list_update: None,
            });
        }

        self.apply_multi_cursor_resolved_edits(edits);
    }

    /// Apply backspace across all cursors.
    pub(crate) fn multi_cursor_backspace(&mut self) {
        if self.extra_cursors.is_empty() {
            self.cursor.backspace(&mut self.buffer);
            return;
        }
        let all = self.collect_multi_cursor_targets_desc();
        let mut edits = Vec::with_capacity(all.len());

        for (offset, anchor, is_primary) in &all {
            let before_len = self.buffer.len();
            let mut cursor = Cursor {
                offset: *offset,
                anchor: *anchor,
            };
            cursor.backspace(&mut self.buffer);
            let after_len = self.buffer.len();
            edits.push(MultiCursorResolvedEdit {
                start: cursor.offset,
                cursor_after: cursor.offset,
                delta: after_len as isize - before_len as isize,
                is_primary: *is_primary,
                ordered_list_update: None,
            });
        }

        self.apply_multi_cursor_resolved_edits(edits);
    }

    /// Apply delete (forward) across all cursors.
    pub(crate) fn multi_cursor_delete(&mut self) {
        if self.extra_cursors.is_empty() {
            self.cursor.delete(&mut self.buffer);
            return;
        }
        let all = self.collect_multi_cursor_targets_desc();
        let mut edits = Vec::with_capacity(all.len());

        for (offset, anchor, is_primary) in &all {
            let before_len = self.buffer.len();
            let mut cursor = Cursor {
                offset: *offset,
                anchor: *anchor,
            };
            cursor.delete(&mut self.buffer);
            let after_len = self.buffer.len();
            edits.push(MultiCursorResolvedEdit {
                start: cursor.offset,
                cursor_after: cursor.offset,
                delta: after_len as isize - before_len as isize,
                is_primary: *is_primary,
                ordered_list_update: None,
            });
        }

        self.apply_multi_cursor_resolved_edits(edits);
    }

    /// Move all cursors left.
    pub(crate) fn multi_cursor_move_left(&mut self) {
        self.cursor.move_left(&self.buffer);
        for c in &mut self.extra_cursors {
            c.move_left(&self.buffer);
        }
        self.dedup_cursors();
    }

    /// Move all cursors right.
    pub(crate) fn multi_cursor_move_right(&mut self) {
        self.cursor.move_right(&self.buffer);
        for c in &mut self.extra_cursors {
            c.move_right(&self.buffer);
        }
        self.dedup_cursors();
    }

    /// Move all cursors up.
    pub(crate) fn multi_cursor_move_up(&mut self) {
        self.cursor.move_up(&self.buffer);
        for c in &mut self.extra_cursors {
            c.move_up(&self.buffer);
        }
        self.dedup_cursors();
    }

    /// Move all cursors down.
    pub(crate) fn multi_cursor_move_down(&mut self) {
        self.cursor.move_down(&self.buffer);
        for c in &mut self.extra_cursors {
            c.move_down(&self.buffer);
        }
        self.dedup_cursors();
    }

    /// Select left for all cursors.
    pub(crate) fn multi_cursor_select_left(&mut self) {
        self.cursor.select_left(&self.buffer);
        for c in &mut self.extra_cursors {
            c.select_left(&self.buffer);
        }
        self.dedup_cursors();
    }

    /// Select right for all cursors.
    pub(crate) fn multi_cursor_select_right(&mut self) {
        self.cursor.select_right(&self.buffer);
        for c in &mut self.extra_cursors {
            c.select_right(&self.buffer);
        }
        self.dedup_cursors();
    }

    /// Select up for all cursors.
    pub(crate) fn multi_cursor_select_up(&mut self) {
        self.cursor.select_up(&self.buffer);
        for c in &mut self.extra_cursors {
            c.select_up(&self.buffer);
        }
        self.dedup_cursors();
    }

    /// Select down for all cursors.
    pub(crate) fn multi_cursor_select_down(&mut self) {
        self.cursor.select_down(&self.buffer);
        for c in &mut self.extra_cursors {
            c.select_down(&self.buffer);
        }
        self.dedup_cursors();
    }

    /// Move all cursors to line start.
    pub(crate) fn multi_cursor_move_to_line_start(&mut self) {
        self.cursor.move_to_line_start(&self.buffer);
        for c in &mut self.extra_cursors {
            c.move_to_line_start(&self.buffer);
        }
        self.dedup_cursors();
    }

    /// Move all cursors to line end.
    pub(crate) fn multi_cursor_move_to_line_end(&mut self) {
        self.cursor.move_to_line_end(&self.buffer);
        for c in &mut self.extra_cursors {
            c.move_to_line_end(&self.buffer);
        }
        self.dedup_cursors();
    }

    pub(crate) fn multi_cursor_move_word_left(&mut self) {
        self.cursor.move_word_left(&self.buffer);
        for c in &mut self.extra_cursors {
            c.move_word_left(&self.buffer);
        }
        self.dedup_cursors();
    }

    pub(crate) fn multi_cursor_move_word_right(&mut self) {
        self.cursor.move_word_right(&self.buffer);
        for c in &mut self.extra_cursors {
            c.move_word_right(&self.buffer);
        }
        self.dedup_cursors();
    }

    pub(crate) fn multi_cursor_delete_word_left(&mut self) {
        if self.extra_cursors.is_empty() {
            self.cursor.delete_word_left(&mut self.buffer);
            return;
        }

        let all = self.collect_multi_cursor_targets_desc();
        let mut edits = Vec::with_capacity(all.len());
        for (offset, anchor, is_primary) in &all {
            let before_len = self.buffer.len();
            let mut cursor = Cursor {
                offset: *offset,
                anchor: *anchor,
            };
            cursor.delete_word_left(&mut self.buffer);
            let after_len = self.buffer.len();
            edits.push(MultiCursorResolvedEdit {
                start: cursor.offset,
                cursor_after: cursor.offset,
                delta: after_len as isize - before_len as isize,
                is_primary: *is_primary,
                ordered_list_update: None,
            });
        }

        self.apply_multi_cursor_resolved_edits(edits);
    }

    pub(crate) fn multi_cursor_delete_word_right(&mut self) {
        if self.extra_cursors.is_empty() {
            self.cursor.delete_word_right(&mut self.buffer);
            return;
        }

        let all = self.collect_multi_cursor_targets_desc();
        let mut edits = Vec::with_capacity(all.len());
        for (offset, anchor, is_primary) in &all {
            let before_len = self.buffer.len();
            let mut cursor = Cursor {
                offset: *offset,
                anchor: *anchor,
            };
            cursor.delete_word_right(&mut self.buffer);
            let after_len = self.buffer.len();
            edits.push(MultiCursorResolvedEdit {
                start: cursor.offset,
                cursor_after: cursor.offset,
                delta: after_len as isize - before_len as isize,
                is_primary: *is_primary,
                ordered_list_update: None,
            });
        }

        self.apply_multi_cursor_resolved_edits(edits);
    }

    pub(crate) fn multi_cursor_enter(&mut self) {
        if self.extra_cursors.is_empty() {
            self.cursor.insert(&mut self.buffer, "\n");
            return;
        }

        let all = self.collect_multi_cursor_targets_desc();
        let mut edits = Vec::with_capacity(all.len());

        for (offset, anchor, is_primary) in &all {
            let mut cursor = Cursor {
                offset: *offset,
                anchor: *anchor,
            };
            let before_len = self.buffer.len();
            let mut edit_start = (*offset).min(*anchor);
            let mut ordered_list_update = None;
            let cursor_line = self.buffer.offset_to_line_col(cursor.offset).0;
            let line_start = self.buffer.line_start(cursor_line);
            let line_end = self.buffer.line_end(cursor_line);
            let line_content = self.buffer.line_content(cursor_line).to_string();
            let block_type = self
                .decorations
                .get(cursor_line)
                .map(|d| d.block_type.clone())
                .unwrap_or(BlockType::Normal);
            let leading_ws =
                line_content.len() - line_content.trim_start_matches([' ', '\t']).len();
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
                    if marker_len > 0 && cursor.offset >= line_start + leading_ws + marker_len {
                        let text_after_marker = &line_content[leading_ws + marker_len..];
                        if text_after_marker.trim().is_empty() {
                            edit_start = line_start;
                            self.buffer.replace(line_start..line_end, "");
                            cursor.move_to(line_start, &self.buffer);
                            cursor.insert(&mut self.buffer, "\n");
                        } else {
                            let marker = &trimmed[..marker_len];
                            let continuation = format!("\n{}{}", ws_str, marker);
                            cursor.insert(&mut self.buffer, &continuation);
                        }
                        let after_len = self.buffer.len();
                        edits.push(MultiCursorResolvedEdit {
                            start: edit_start,
                            cursor_after: cursor.offset,
                            delta: after_len as isize - before_len as isize,
                            is_primary: *is_primary,
                            ordered_list_update,
                        });
                        continue;
                    }
                }
                BlockType::TaskList(_) => {
                    let prefix_len = task_prefix_len(trimmed);
                    if prefix_len > 0 && cursor.offset >= line_start + leading_ws + prefix_len {
                        let text_after_marker = &line_content[leading_ws + prefix_len..];
                        if text_after_marker.trim().is_empty() {
                            edit_start = line_start;
                            self.buffer.replace(line_start..line_end, "");
                            cursor.move_to(line_start, &self.buffer);
                            cursor.insert(&mut self.buffer, "\n");
                        } else {
                            let bullet = if trimmed.starts_with("- ") {
                                "- "
                            } else if trimmed.starts_with("* ") {
                                "* "
                            } else {
                                "+ "
                            };
                            let continuation = format!("\n{}{}[ ] ", ws_str, bullet);
                            cursor.insert(&mut self.buffer, &continuation);
                        }
                        let after_len = self.buffer.len();
                        edits.push(MultiCursorResolvedEdit {
                            start: edit_start,
                            cursor_after: cursor.offset,
                            delta: after_len as isize - before_len as isize,
                            is_primary: *is_primary,
                            ordered_list_update,
                        });
                        continue;
                    }
                }
                BlockType::OrderedList => {
                    let prefix_len = markdown::ordered_list_prefix_len(trimmed);
                    if prefix_len > 0 && cursor.offset >= line_start + leading_ws + prefix_len {
                        let number: u32 =
                            trimmed[..prefix_len.saturating_sub(2)].parse().unwrap_or(1);
                        let text_after_marker = &line_content[leading_ws + prefix_len..];
                        if text_after_marker.trim().is_empty() {
                            edit_start = line_start;
                            self.buffer.replace(line_start..line_end, "");
                            cursor.move_to(line_start, &self.buffer);
                            cursor.insert(&mut self.buffer, "\n");
                            ordered_list_update = Some(false);
                        } else {
                            let continuation = format!("\n{}{}. ", ws_str, number + 1);
                            cursor.insert(&mut self.buffer, &continuation);
                            ordered_list_update = Some(true);
                        }
                        let after_len = self.buffer.len();
                        edits.push(MultiCursorResolvedEdit {
                            start: edit_start,
                            cursor_after: cursor.offset,
                            delta: after_len as isize - before_len as isize,
                            is_primary: *is_primary,
                            ordered_list_update,
                        });
                        continue;
                    }
                }
                _ => {}
            }

            cursor.insert(&mut self.buffer, "\n");
            let after_len = self.buffer.len();
            edits.push(MultiCursorResolvedEdit {
                start: edit_start,
                cursor_after: cursor.offset,
                delta: after_len as isize - before_len as isize,
                is_primary: *is_primary,
                ordered_list_update,
            });
        }

        let ordered_list_updates = self.apply_multi_cursor_resolved_edits(edits);

        self.reparse();
        for (offset, preserve_current_number) in ordered_list_updates {
            let (line, _) = self
                .buffer
                .offset_to_line_col(offset.min(self.buffer.len()));
            self.normalize_ordered_list_near_line(line, preserve_current_number);
        }
        if !self.extra_cursors.is_empty() {
            self.reparse();
        }
    }

    /// Select to line start for all cursors.
    pub(crate) fn multi_cursor_select_to_line_start(&mut self) {
        self.cursor.select_to_line_start(&self.buffer);
        for c in &mut self.extra_cursors {
            c.select_to_line_start(&self.buffer);
        }
        self.dedup_cursors();
    }

    /// Select to line end for all cursors.
    pub(crate) fn multi_cursor_select_to_line_end(&mut self) {
        self.cursor.select_to_line_end(&self.buffer);
        for c in &mut self.extra_cursors {
            c.select_to_line_end(&self.buffer);
        }
        self.dedup_cursors();
    }

    /// Remove duplicate extra cursors (same offset+anchor as primary or each other).
    pub(crate) fn dedup_cursors(&mut self) {
        self.extra_cursors
            .retain(|c| c.offset != self.cursor.offset || c.anchor != self.cursor.anchor);
        let mut seen = vec![(self.cursor.offset, self.cursor.anchor)];
        self.extra_cursors.retain(|c| {
            let key = (c.offset, c.anchor);
            if seen.contains(&key) {
                false
            } else {
                seen.push(key);
                true
            }
        });
    }

    fn desired_gutter_fold_reveal(&self) -> f32 {
        if self.hovering_gutter_fold_area || self.hovering_fold_line.is_some() {
            1.0
        } else {
            0.0
        }
    }

    pub(crate) fn sync_gutter_fold_reveal(&mut self, cx: &mut Context<Self>) {
        let target = self.desired_gutter_fold_reveal();
        if (self.gutter_fold_reveal_target - target).abs() <= 0.001 {
            return;
        }

        self.gutter_fold_reveal_target = target;
        self.gutter_fold_reveal_epoch += 1;
        let epoch = self.gutter_fold_reveal_epoch;
        let duration_ms = self.styles.gutter.gutter_fold_animation_ms.max(1);
        let step = (16.0 / duration_ms as f32).clamp(0.08, 1.0);

        let pane_id = self.current_pane_id();
        self.gutter_fold_reveal_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let done = this
                    .update(cx, |this, cx| {
                        this.with_pane_if_alive(pane_id, |this| {
                            if this.gutter_fold_reveal_epoch != epoch {
                                return true;
                            }

                            let diff = this.gutter_fold_reveal_target - this.gutter_fold_reveal;
                            if diff.abs() <= 0.01 {
                                this.gutter_fold_reveal = this.gutter_fold_reveal_target;
                                this.gutter_fold_reveal_task = None;
                                cx.notify();
                                return true;
                            }

                            this.gutter_fold_reveal += diff * step;
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
                    .timer(Duration::from_millis(16))
                    .await;
            }
        }));
    }

    pub(crate) fn clear_gutter_fold_hover(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        if self.hovering_fold_line.take().is_some() {
            changed = true;
        }
        if self.hovering_gutter_fold_area {
            self.hovering_gutter_fold_area = false;
            changed = true;
        }
        if changed {
            self.sync_gutter_fold_reveal(cx);
            cx.notify();
        }
    }

    pub(crate) fn current_snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            text: self.buffer.text().to_string(),
            cursor_offset: self.cursor.offset,
            cursor_anchor: self.cursor.anchor,
            scroll_offset: self.scroll_offset,
            extra_cursors: self
                .extra_cursors
                .iter()
                .map(|cursor| (cursor.offset, cursor.anchor))
                .collect(),
        }
    }

    pub(crate) fn restore_snapshot(&mut self, snapshot: EditorSnapshot) {
        self.buffer = Buffer::from_text(&snapshot.text);
        self.cursor.offset = snapshot.cursor_offset.min(self.buffer.len());
        self.cursor.anchor = snapshot.cursor_anchor.min(self.buffer.len());
        self.extra_cursors = snapshot
            .extra_cursors
            .into_iter()
            .map(|(offset, anchor)| Cursor {
                offset: offset.min(self.buffer.len()),
                anchor: anchor.min(self.buffer.len()),
            })
            .collect();
        self.scroll_offset = snapshot.scroll_offset;
        self.clear_marked_ranges();
        self.reset_cursor_visual_targeting();
        self.ime_composition_snapshot = None;
        self.dedup_cursors();
        self.reparse();
        self.scroll_to_cursor();
    }

    pub(crate) fn restore_tab_snapshot_state(&mut self, snapshot: &EditorSnapshot) {
        self.buffer = Buffer::from_text(&snapshot.text);
        self.cursor.offset = snapshot.cursor_offset.min(self.buffer.len());
        self.cursor.anchor = snapshot.cursor_anchor.min(self.buffer.len());
        self.extra_cursors = snapshot
            .extra_cursors
            .iter()
            .map(|(offset, anchor)| Cursor {
                offset: (*offset).min(self.buffer.len()),
                anchor: (*anchor).min(self.buffer.len()),
            })
            .collect();
        self.scroll_offset = snapshot.scroll_offset;
        self.clear_marked_ranges();
        self.reset_cursor_visual_targeting();
        self.ime_composition_snapshot = None;
        self.dedup_cursors();
        self.reparse();
    }

    pub(crate) fn push_undo_snapshot(&mut self, snapshot: EditorSnapshot) {
        self.commit_history_transaction();
        if let Some(index) = self.active_tab_index() {
            self.tabs[index].history.push_undo_snapshot(snapshot);
        }
        self.mark_active_tab_dirty();
    }

    pub(crate) fn checkpoint(&mut self) {
        self.ime_composition_snapshot = None;
        self.reset_typing_checkpoint_grouping();
        self.begin_history_transaction(false);
    }

    /// Records an undo checkpoint for ordinary typing, coalescing consecutive
    /// single-character insertions that happen within `UNDO_GROUP_INTERVAL` and
    /// continue contiguously from the previous edit into a single undo step.
    /// This mirrors editors like Zed, so undo reverts a "word"/burst of typing
    /// at once instead of one Latin character at a time.
    pub(crate) fn checkpoint_for_typing(&mut self, new_text: &str) {
        self.ime_composition_snapshot = None;

        let now = Instant::now();
        let start = self.cursor.offset;
        // Only plain single-character insertions (no newline, no selection or
        // multi-cursor/marked state) are eligible for grouping. Everything else
        // forces a fresh undo boundary.
        let is_groupable = new_text.chars().count() == 1
            && new_text != "\n"
            && !self.cursor.has_selection()
            && !self.has_extra_cursors()
            && self.marked_range.is_none();

        let within_interval = self
            .last_typing_checkpoint_time
            .is_some_and(|previous| now.duration_since(previous) < UNDO_GROUP_INTERVAL);
        let contiguous = self.last_typing_checkpoint_offset == Some(start);
        let coalesce = is_groupable && within_interval && contiguous;

        self.begin_history_transaction(coalesce);

        if is_groupable {
            self.last_typing_checkpoint_time = Some(now);
            self.last_typing_checkpoint_offset = Some(start + new_text.len());
        } else {
            self.reset_typing_checkpoint_grouping();
        }
    }

    /// Forces the next typing checkpoint to start a new undo group (e.g. after
    /// undo/redo, cursor movement, or any non-typing edit).
    pub(crate) fn reset_typing_checkpoint_grouping(&mut self) {
        self.last_typing_checkpoint_time = None;
        self.last_typing_checkpoint_offset = None;
    }

    fn current_history_view(&self) -> history_store::SnapshotViewState {
        history_store::SnapshotViewState::new(
            self.cursor.offset,
            self.cursor.anchor,
            self.scroll_offset,
            self.extra_cursors
                .iter()
                .map(|cursor| (cursor.offset, cursor.anchor))
                .collect(),
        )
    }

    fn begin_history_transaction(&mut self, merge_with_previous: bool) {
        self.commit_history_transaction();
        self.buffer.begin_edit_recording();
        self.pending_history_transaction = Some(PendingHistoryTransaction {
            before_view: self.current_history_view(),
            merge_with_previous,
        });
    }

    pub(crate) fn commit_history_transaction(&mut self) {
        let Some(transaction) = self.pending_history_transaction.take() else {
            return;
        };
        let edits = self.buffer.finish_edit_recording();
        if edits.is_empty() {
            return;
        }
        let after_view = self.current_history_view();
        if let Some(index) = self.active_tab_index() {
            self.tabs[index].history.push_undo_transaction(
                edits,
                transaction.before_view,
                after_view,
                transaction.merge_with_previous,
            );
        }
        self.mark_active_tab_dirty();
    }

    pub(crate) fn discard_history_transaction(&mut self) {
        self.pending_history_transaction = None;
        self.buffer.finish_edit_recording();
    }

    pub(crate) fn copy_selection_to_clipboard(&mut self, cx: &mut Context<Self>) {
        // A read-only phantom-row selection takes precedence so the deleted
        // lines under an expanded hunk can be copied.
        if let Some(text) = self.phantom_selection_text() {
            self.multi_cursor_clipboard = None;
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            return;
        }

        if self
            .cursor_states_with_primary_flag()
            .iter()
            .all(|(cursor, _)| !cursor.has_selection())
        {
            let text = self.cursor_lines_clipboard_text();
            if !text.is_empty() {
                self.multi_cursor_clipboard = None;
                cx.write_to_clipboard(ClipboardItem::new_string(text));
            }
            return;
        }

        let texts = self.multi_cursor_selected_texts();
        if texts.is_empty() {
            return;
        }

        if texts.len() > 1 {
            let serialized_text = texts.join("\n");
            self.multi_cursor_clipboard = Some(MultiCursorClipboard {
                serialized_text: serialized_text.clone(),
                segments: texts,
            });
            cx.write_to_clipboard(ClipboardItem::new_string(serialized_text));
            return;
        }

        self.multi_cursor_clipboard = None;
        cx.write_to_clipboard(ClipboardItem::new_string(texts[0].clone()));
    }

    /// Copy the document text with markdown syntax markers stripped out. When a
    /// selection is present only that range is copied (still excluding any
    /// syntax characters that fall inside it); otherwise the whole document is
    /// copied as plain text.
    /// Formats the selection, or asks before rewriting the whole document.
    pub(crate) fn format_markdown_selection_or_prompt(&mut self, cx: &mut Context<Self>) {
        if self.active_tab_is_read_only() {
            return;
        }
        let selection = self.cursor.selection_range();
        if selection.start != selection.end {
            self.format_markdown_range(selection, cx);
            return;
        }
        self.show_dialog(
            Dialog {
                title: crate::i18n::format_markdown_label(self.ui_language).to_string(),
                message: crate::i18n::format_whole_document_prompt(self.ui_language).to_string(),
                save_label: None,
                confirm_label: self.t(TextKey::Acknowledge).to_string(),
                cancel_label: self.t(TextKey::Cancel).to_string(),
                is_danger: false,
                action: DialogAction::FormatWholeDocument,
            },
            cx,
        );
    }

    pub(crate) fn format_whole_markdown_document(&mut self, cx: &mut Context<Self>) {
        self.format_markdown_range(0..self.buffer.len(), cx);
    }

    fn format_markdown_range(&mut self, range: std::ops::Range<usize>, cx: &mut Context<Self>) {
        let Some(source) = self.buffer.text().get(range.clone()) else {
            return;
        };
        let Some(formatted) = crate::markdown_format::format_markdown(source) else {
            return;
        };
        self.checkpoint();
        self.clear_extra_cursors();
        self.queue_non_markdown_line_display_transfer(&range, &formatted);
        self.buffer.replace(range.clone(), &formatted);
        let caret = range.start + formatted.len();
        self.cursor
            .move_to(caret.min(self.buffer.len()), &self.buffer);
        self.reparse();
        self.scroll_to_cursor();
        cx.notify();
    }

    pub(crate) fn copy_plain_text_to_clipboard(&mut self, cx: &mut Context<Self>) {
        let selection = self.cursor.selection_range();
        let (start, end) = if selection.start != selection.end {
            (selection.start, selection.end)
        } else {
            (0, self.buffer.len())
        };
        let plain = self.plain_text_in_buffer_range(start, end);
        self.context_menu = None;
        self.multi_cursor_clipboard = None;
        cx.write_to_clipboard(ClipboardItem::new_string(plain));
        cx.notify();
    }

    /// Build the plain text for the buffer byte range `[start, end)`, dropping
    /// every per-line syntax marker span (e.g. `#`, `**`, `` ` ``, `~~`, `==`,
    /// list markers) so only the rendered content remains.
    fn plain_text_in_buffer_range(&self, start: usize, end: usize) -> String {
        let mut out = String::new();
        let mut first_line = true;
        for line_idx in 0..self.buffer.line_count() {
            let line_start = self.buffer.line_start(line_idx);
            let line_content = self.buffer.line_content(line_idx);
            let line_end = line_start + line_content.len();
            if line_end < start || line_start > end {
                continue;
            }
            if !first_line {
                out.push('\n');
            }
            first_line = false;
            let syntax_spans: Vec<std::ops::Range<usize>> = self
                .decorations
                .get(line_idx)
                .map(|decoration| {
                    decoration
                        .syntax_ranges
                        .iter()
                        .map(|sr| sr.range.clone())
                        .collect()
                })
                .unwrap_or_default();
            for (offset, ch) in line_content.char_indices() {
                let abs = line_start + offset;
                if abs < start || abs >= end {
                    continue;
                }
                let in_syntax = syntax_spans
                    .iter()
                    .any(|span| offset >= span.start && offset < span.end);
                if !in_syntax {
                    out.push(ch);
                }
            }
        }
        out
    }

    fn next_code_block_copy_feedback_epoch(&mut self) -> usize {
        self.code_block_copy_feedback_epoch += 1;
        self.code_block_copy_feedback_epoch
    }

    fn code_block_group_end(&self, start_line: usize) -> Option<usize> {
        if !matches!(
            self.decorations
                .get(start_line)
                .map(|decoration| &decoration.block_type),
            Some(BlockType::CodeBlock | BlockType::CodeBlockFence)
        ) {
            return None;
        }

        let mut end_line = start_line;
        while end_line + 1 < self.decorations.len()
            && matches!(
                self.decorations[end_line + 1].block_type,
                BlockType::CodeBlock | BlockType::CodeBlockFence
            )
        {
            end_line += 1;
        }

        Some(end_line)
    }

    fn code_block_text_for_group(&self, start_line: usize) -> Option<String> {
        let end_line = self.code_block_group_end(start_line)?;
        let mut lines = Vec::new();
        for line_idx in start_line..=end_line {
            if matches!(
                self.decorations
                    .get(line_idx)
                    .map(|decoration| &decoration.block_type),
                Some(BlockType::CodeBlock)
            ) {
                lines.push(self.buffer.line_content(line_idx).to_string());
            }
        }

        Some(lines.join("\n"))
    }

    pub(crate) fn copy_code_block_group_to_clipboard(
        &mut self,
        start_line: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(text) = self.code_block_text_for_group(start_line) else {
            return;
        };

        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.copied_code_block_start = Some(start_line);
        let epoch = self.next_code_block_copy_feedback_epoch();
        cx.notify();

        cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(CODE_BLOCK_COPY_FEEDBACK_MS))
                .await;
            this.update(cx, |this, cx| {
                if this.code_block_copy_feedback_epoch == epoch
                    && this.copied_code_block_start == Some(start_line)
                {
                    this.copied_code_block_start = None;
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn cut_selection_to_clipboard(&mut self, cx: &mut Context<Self>) {
        if self.active_tab_is_read_only() {
            return;
        }
        if self
            .cursor_states_with_primary_flag()
            .iter()
            .all(|(cursor, _)| !cursor.has_selection())
        {
            let text = self.cursor_lines_clipboard_text();
            let ranges = self.cursor_line_delete_ranges();
            if text.is_empty() || ranges.is_empty() {
                return;
            }
            self.checkpoint();
            cx.write_to_clipboard(ClipboardItem::new_string(text));
            let cursor_offset = ranges.first().map(|range| range.start).unwrap_or(0);
            for range in ranges.into_iter().rev() {
                self.buffer.delete(range);
            }
            self.extra_cursors.clear();
            self.cursor
                .move_to(cursor_offset.min(self.buffer.len()), &self.buffer);
            self.multi_cursor_clipboard = None;
            self.reparse();
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }
        let texts = self.multi_cursor_selected_texts();
        if texts.is_empty() {
            return;
        }

        self.checkpoint();
        if texts.len() > 1 {
            let serialized_text = texts.join("\n");
            self.multi_cursor_clipboard = Some(MultiCursorClipboard {
                serialized_text: serialized_text.clone(),
                segments: texts,
            });
            cx.write_to_clipboard(ClipboardItem::new_string(serialized_text));

            let all = self.collect_multi_cursor_targets_desc();
            let mut edits = Vec::with_capacity(all.len());
            for (offset, anchor, is_primary) in &all {
                let range_start = (*offset).min(*anchor);
                let range_end = (*offset).max(*anchor);
                if range_start != range_end {
                    self.buffer.delete(range_start..range_end);
                }
                edits.push(MultiCursorResolvedEdit {
                    start: range_start,
                    cursor_after: range_start,
                    delta: -(range_end as isize - range_start as isize),
                    is_primary: *is_primary,
                    ordered_list_update: None,
                });
            }
            self.apply_multi_cursor_resolved_edits(edits);
        } else {
            self.multi_cursor_clipboard = None;
            let range = self.cursor.selection_range();
            cx.write_to_clipboard(ClipboardItem::new_string(texts[0].clone()));
            self.buffer.delete(range.clone());
            self.cursor.move_to(range.start, &self.buffer);
        }

        self.reparse();
        self.pause_blinking(cx);
    }

    fn cursor_line_indices(&self) -> Vec<usize> {
        let mut lines = self
            .cursor_states_with_primary_flag()
            .into_iter()
            .map(|(cursor, _)| self.buffer.offset_to_line_col(cursor.offset).0)
            .collect::<Vec<_>>();
        lines.sort_unstable();
        lines.dedup();
        lines
    }

    fn cursor_lines_clipboard_text(&self) -> String {
        let mut text = String::new();
        for line in self.cursor_line_indices() {
            text.push_str(self.buffer.line_content(line));
            text.push('\n');
        }
        text
    }

    fn cursor_line_delete_ranges(&self) -> Vec<std::ops::Range<usize>> {
        let lines = self.cursor_line_indices();
        let mut ranges = Vec::new();
        let mut index = 0;
        while index < lines.len() {
            let first_line = lines[index];
            let mut last_line = first_line;
            while index + 1 < lines.len() && lines[index + 1] == last_line + 1 {
                index += 1;
                last_line = lines[index];
            }

            let mut start = self.buffer.line_start(first_line);
            let end = self.buffer.line_end(last_line);
            let end = if end < self.buffer.len() {
                end + 1
            } else {
                end
            };
            if end == self.buffer.len() && start == end && start > 0 {
                start -= 1;
            } else if end == self.buffer.len() && start > 0 {
                start -= 1;
            }
            ranges.push(start..end);
            index += 1;
        }
        ranges
    }

    pub(crate) fn paste_from_clipboard(&mut self, cx: &mut Context<Self>) {
        if self.active_tab_is_read_only() {
            return;
        }
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };

        match self.import_clipboard_resources(&item, cx) {
            Ok(ClipboardPasteOutcome::Insert(text)) => {
                self.insert_pasted_text(&text, cx);
                return;
            }
            Ok(ClipboardPasteOutcome::Consumed) => {
                return;
            }
            Ok(ClipboardPasteOutcome::NotHandled) => {}
            Err(error) => {
                let message = format!("{}: {error}", self.t(TextKey::OperationFailed));
                self.notify_error(message, cx);
                return;
            }
        }

        if let Some(text) = item.text() {
            if let Some(multi_cursor_clipboard) = self.multi_cursor_clipboard.as_ref()
                && multi_cursor_clipboard.serialized_text == text
                && multi_cursor_clipboard.segments.len() == 1 + self.extra_cursors.len()
            {
                let segments = multi_cursor_clipboard.segments.clone();
                self.checkpoint();
                self.apply_multi_cursor_paste_segments(&segments);
                self.reparse();
                self.scroll_to_cursor();
                self.pause_blinking(cx);
                return;
            }
            let detected_kind = self
                .active_tab_is_blank_untitled_text_editor()
                .then(|| classify_pasted_text_kind(&text))
                .flatten();
            if let Some(file_kind) = detected_kind {
                self.apply_pasted_text_file_kind_to_active_tab(file_kind);
            }
            self.insert_pasted_text(&text, cx);
        }
    }

    fn adjust_offset_for_replaced_line(
        offset: usize,
        line_start: usize,
        line_end: usize,
        leading_ws: usize,
        old_prefix_len: usize,
        new_prefix_len: usize,
        new_line_len: usize,
    ) -> usize {
        if offset <= line_start {
            return offset;
        }

        let old_line_len = line_end.saturating_sub(line_start);
        let diff = new_line_len as isize - old_line_len as isize;
        if offset >= line_end {
            return if diff >= 0 {
                offset + diff as usize
            } else {
                offset.saturating_sub((-diff) as usize)
            };
        }

        let col = offset.saturating_sub(line_start);
        let old_prefix_end = leading_ws + old_prefix_len;
        let new_prefix_end = leading_ws + new_prefix_len;
        if col <= leading_ws {
            line_start + col.min(new_line_len)
        } else if col <= old_prefix_end {
            line_start + new_prefix_end.min(new_line_len)
        } else {
            line_start + (new_prefix_end + (col - old_prefix_end)).min(new_line_len)
        }
    }

    pub(crate) fn toggle_current_line_prefix(&mut self, prefix: &str) {
        if self.extra_cursors.is_empty() {
            self.toggle_current_line_prefix_single(prefix);
            return;
        }

        self.apply_single_cursor_edit_to_all_cursors(
            |this, offset, anchor| {
                let (start_line, _) = this.buffer.offset_to_line_col(offset.min(anchor));
                this.buffer.line_start(start_line)
            },
            |this| this.toggle_current_line_prefix_single(prefix),
        );
    }

    fn toggle_current_line_prefix_single(&mut self, prefix: &str) {
        let selection = self.cursor.selection_range();
        let selection_start = selection.start.min(selection.end);
        let selection_end = selection.start.max(selection.end);
        let (start_line, _) = self.buffer.offset_to_line_col(selection_start);
        let (mut end_line, _) = self.buffer.offset_to_line_col(selection_end);
        if selection_end > selection_start && end_line > start_line {
            let end_line_start = self.buffer.line_start(end_line);
            if selection_end == end_line_start {
                end_line = end_line.saturating_sub(1);
            }
        }

        let original_cursor_offset = self.cursor.offset;
        let original_anchor_offset = self.cursor.anchor;
        let should_snap_single_cursor_to_line_end = selection.is_empty() && start_line == end_line;
        let mut cursor_offset = self.cursor.offset;
        let mut anchor_offset = self.cursor.anchor;
        let mut forced_cursor_line_end = None;
        for line in (start_line..=end_line).rev() {
            let line_start = self.buffer.line_start(line);
            let line_end = self.buffer.line_end(line);
            let line_text = self.buffer.line_content(line).to_string();
            let trimmed = line_text.trim_start();
            let leading_ws = line_text.len() - trimmed.len();
            let existing_prefix_len = Self::detect_block_prefix_len(trimmed);
            let existing_prefix = &trimmed[..existing_prefix_len];
            let old_content_start = line_start + leading_ws + existing_prefix_len;
            let is_adding_prefix = existing_prefix != prefix;

            let new_line = if existing_prefix == prefix {
                let body = &trimmed[existing_prefix_len..];
                let mut new_line = String::with_capacity(leading_ws + body.len());
                new_line.push_str(&line_text[..leading_ws]);
                new_line.push_str(body);
                new_line
            } else {
                let body = &trimmed[existing_prefix_len..];
                let mut new_line = String::with_capacity(leading_ws + prefix.len() + body.len());
                new_line.push_str(&line_text[..leading_ws]);
                new_line.push_str(prefix);
                new_line.push_str(body);
                new_line
            };

            cursor_offset = Self::adjust_offset_for_replaced_line(
                cursor_offset,
                line_start,
                line_end,
                leading_ws,
                existing_prefix_len,
                if existing_prefix == prefix {
                    0
                } else {
                    prefix.len()
                },
                new_line.len(),
            );
            anchor_offset = Self::adjust_offset_for_replaced_line(
                anchor_offset,
                line_start,
                line_end,
                leading_ws,
                existing_prefix_len,
                if existing_prefix == prefix {
                    0
                } else {
                    prefix.len()
                },
                new_line.len(),
            );

            if should_snap_single_cursor_to_line_end
                && line == start_line
                && is_adding_prefix
                && original_cursor_offset <= old_content_start
                && original_anchor_offset <= old_content_start
            {
                forced_cursor_line_end = Some(line_start + new_line.len());
            }

            self.buffer.replace(line_start..line_end, &new_line);
        }

        if let Some(line_end) = forced_cursor_line_end {
            cursor_offset = line_end;
            anchor_offset = line_end;
        }

        self.cursor.offset = cursor_offset.min(self.buffer.len());
        self.cursor.anchor = anchor_offset.min(self.buffer.len());
        self.reparse();
        self.scroll_to_cursor();
    }

    fn detect_block_prefix_len(trimmed: &str) -> usize {
        // Check todo variants first (longest match)
        if trimmed.starts_with("- [x] ") || trimmed.starts_with("- [X] ") {
            return 6;
        }
        if trimmed.starts_with("- [ ] ") {
            return 6;
        }
        // Unordered list
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
            return 2;
        }
        // Blockquote
        if trimmed.starts_with("> ") {
            return 2;
        }
        // Heading (# ... ###### )
        let hash_count = trimmed.bytes().take_while(|&b| b == b'#').count();
        if hash_count > 0 && hash_count <= 6 && trimmed.as_bytes().get(hash_count) == Some(&b' ') {
            return hash_count + 1;
        }
        // Ordered list: digits followed by ". "
        let digit_count = trimmed.bytes().take_while(|b| b.is_ascii_digit()).count();
        if digit_count > 0 && trimmed[digit_count..].starts_with(". ") {
            return digit_count + 2;
        }
        0
    }

    pub(crate) fn toggle_horizontal_rule_at_cursor(&mut self) {
        if self.extra_cursors.is_empty() {
            self.toggle_horizontal_rule_at_cursor_single();
            return;
        }

        self.apply_single_cursor_edit_to_all_cursors(
            |this, offset, _anchor| {
                let (line, _) = this.buffer.offset_to_line_col(offset);
                this.buffer.line_start(line)
            },
            |this| this.toggle_horizontal_rule_at_cursor_single(),
        );
    }

    fn toggle_horizontal_rule_at_cursor_single(&mut self) {
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

    fn apply_single_cursor_edit_to_all_cursors(
        &mut self,
        mut edit_start_for_cursor: impl FnMut(&Self, usize, usize) -> usize,
        mut apply_single: impl FnMut(&mut Self),
    ) {
        let targets = self.collect_multi_cursor_targets_desc();
        let mut resolved = Vec::with_capacity(targets.len());

        for (offset, anchor, is_primary) in targets {
            let edit_start = edit_start_for_cursor(self, offset, anchor).min(self.buffer.len());
            self.extra_cursors.clear();
            self.cursor.offset = offset.min(self.buffer.len());
            self.cursor.anchor = anchor.min(self.buffer.len());

            let before_len = self.buffer.len();
            apply_single(self);
            let delta = self.buffer.len() as isize - before_len as isize;

            for (resolved_offset, resolved_anchor, _) in &mut resolved {
                if *resolved_offset >= edit_start {
                    *resolved_offset = apply_offset_delta(*resolved_offset, delta);
                }
                if *resolved_anchor >= edit_start {
                    *resolved_anchor = apply_offset_delta(*resolved_anchor, delta);
                }
            }

            resolved.push((
                self.cursor.offset.min(self.buffer.len()),
                self.cursor.anchor.min(self.buffer.len()),
                is_primary,
            ));
        }

        self.extra_cursors.clear();
        let mut primary_cursor = None;
        for (offset, anchor, is_primary) in resolved {
            let cursor = Cursor {
                offset: offset.min(self.buffer.len()),
                anchor: anchor.min(self.buffer.len()),
            };
            if is_primary {
                primary_cursor = Some(cursor);
            } else {
                self.extra_cursors.push(cursor);
            }
        }

        if let Some(primary_cursor) = primary_cursor {
            self.cursor = primary_cursor;
        }

        if !self.extra_cursors.is_empty() {
            self.dedup_cursors();
        }

        self.scroll_to_cursor();
    }

    pub(crate) fn set_ui_scale(&mut self, new_scale: f32, cx: &mut Context<Self>) {
        let clamped = new_scale.clamp(
            self.styles.editor.editor_scale_min,
            self.styles.editor.editor_scale_max,
        );
        if (clamped - self.ui_scale).abs() < f32::EPSILON {
            return;
        }
        if let Some(ref mut w) = self.sidebar_custom_width {
            *w = *w * (clamped / self.ui_scale);
        }
        if let Some(ref mut w) = self.file_list_custom_width {
            *w = *w * (clamped / self.ui_scale);
        }
        self.ui_scale = clamped;
        self.app_settings.ui_scale = clamped;
        self.sync_panel_display_state();
        self.sync_overlay_text_input_styles(cx);
        // Zooming changes the scaled font sizes, so the cached per-line layout
        // (heights, wrapping, geometry) must be rebuilt to avoid geometry drift.
        // This affects every tab, so invalidate the inactive tab caches too;
        // otherwise switching back to an editor restores a stale pre-zoom layout
        // until a line is touched.
        self.clear_all_tab_layout_caches();
        self.clamp_scroll();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
        // Sync the scale editor display
        if let Some(editor) = &self.settings_ui_font_size_editor {
            let current = self.ui_scale;
            editor.update(cx, |ti, cx| {
                let display = format!("{:.1}", current);
                if ti.text() != display {
                    ti.set_text(&display, cx);
                }
            });
        }
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

    pub(crate) fn selected_line_span(&self) -> (usize, usize) {
        self.line_span_for_range(self.cursor.selection_range())
    }

    pub(crate) fn list_indent_width_for_line(&self, line: usize) -> Option<usize> {
        match self
            .decorations
            .get(line)
            .map(|decoration| &decoration.block_type)
        {
            Some(BlockType::OrderedList) => Some(3),
            Some(BlockType::UnorderedList | BlockType::TaskList(_)) => Some(2),
            _ => None,
        }
    }

    pub(crate) fn normalize_ordered_lists_in_line_span(
        &mut self,
        start_line: usize,
        end_line: usize,
    ) {
        if self.buffer.line_count() == 0 {
            return;
        }

        let first_line = start_line.saturating_sub(1);
        let last_line = (end_line + 1).min(self.buffer.line_count().saturating_sub(1));
        for line in first_line..=last_line {
            self.normalize_ordered_list_near_line(line, false);
        }
    }

    pub(crate) fn normalize_ordered_lists_near_all_cursors(
        &mut self,
        preserve_current_number: bool,
    ) {
        let mut lines: Vec<usize> = self
            .all_cursors_snapshot()
            .into_iter()
            .map(|cursor| {
                self.buffer
                    .offset_to_line_col(cursor.offset.min(cursor.anchor))
                    .0
            })
            .collect();
        lines.sort_unstable();
        lines.dedup();

        for line in lines {
            self.normalize_ordered_list_near_line(line, preserve_current_number);
        }
    }

    pub(crate) fn indent_selected_list_lines(&mut self) {
        let (lines, _, has_list_line) = self.collect_line_indices_for_block_indent();
        if !has_list_line {
            return;
        }
        let line_widths = self.resolved_list_indent_widths(&lines);
        self.indent_line_indices_with_widths(&line_widths);
        if let (Some(first_line), Some(last_line)) = (lines.first().copied(), lines.last().copied())
        {
            self.normalize_ordered_lists_in_line_span(first_line, last_line);
            self.reparse();
        }
    }

    pub(crate) fn unindent_selected_list_lines(&mut self) {
        let (lines, _, has_list_line) = self.collect_line_indices_for_block_indent();
        if !has_list_line {
            return;
        }
        let line_widths = self.resolved_list_indent_widths(&lines);
        self.unindent_line_indices_with_widths(&line_widths);
        if let (Some(first_line), Some(last_line)) = (lines.first().copied(), lines.last().copied())
        {
            self.normalize_ordered_lists_in_line_span(first_line, last_line);
            self.reparse();
        }
    }

    pub(crate) fn unindent_current_lines(&mut self, max_spaces: usize) {
        let selection = self.cursor.selection_range();
        let (start_line, _) = self.buffer.offset_to_line_col(selection.start);
        let end_anchor = selection
            .end
            .saturating_sub(usize::from(selection.end > selection.start));
        let (end_line, _) = self.buffer.offset_to_line_col(end_anchor);

        // Collect how many leading spaces to strip from each line (before mutation)
        let mut strips: Vec<usize> = Vec::new();
        for line in start_line..=end_line {
            let content = self.buffer.line_content(line);
            let leading: usize = content
                .bytes()
                .take(max_spaces)
                .take_while(|&b| b == b' ')
                .count();
            strips.push(leading);
        }

        let mut total_removed = 0usize;
        let mut first_removed = 0usize;
        for (i, &strip) in strips.iter().enumerate() {
            if strip > 0 {
                let line = start_line + i;
                // After previous removals, line_start already reflects the new state
                let line_start = self.buffer.line_start(line);
                self.buffer.replace(line_start..line_start + strip, "");
                if i == 0 {
                    first_removed = strip;
                }
                total_removed += strip;
            }
        }

        if total_removed > 0 {
            let new_start = selection.start.saturating_sub(first_removed);
            let new_end = selection.end.saturating_sub(total_removed);
            self.cursor.anchor = new_start.min(self.buffer.len());
            self.cursor.offset = new_end.min(self.buffer.len());
            self.reparse();
            self.scroll_to_cursor();
        }
    }

    pub(crate) fn insert_link_or_image(&mut self, prefix: &str, suffix: &str, placeholder: &str) {
        if self.extra_cursors.is_empty() {
            self.insert_link_or_image_single(prefix, suffix, placeholder);
            return;
        }

        self.apply_single_cursor_edit_to_all_cursors(
            |_this, offset, anchor| offset.min(anchor),
            |this| this.insert_link_or_image_single(prefix, suffix, placeholder),
        );
    }

    fn insert_link_or_image_single(&mut self, prefix: &str, suffix: &str, placeholder: &str) {
        let range = self.cursor.selection_range();
        let selected = self.buffer.text_for_range(range.clone()).to_string();
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

    pub(crate) fn wrap_current_lines_as_code_block(&mut self) {
        if self.extra_cursors.is_empty() {
            self.wrap_current_lines_as_code_block_single();
            return;
        }

        self.apply_single_cursor_edit_to_all_cursors(
            |this, offset, anchor| {
                let (start_line, _) =
                    this.line_span_for_range(offset.min(anchor)..offset.max(anchor));
                this.buffer.line_start(start_line)
            },
            |this| this.wrap_current_lines_as_code_block_single(),
        );
    }

    fn wrap_current_lines_as_code_block_single(&mut self) {
        let selection = self.cursor.selection_range();
        let (start_line, end_line) = self.selected_line_span();
        let block_start = self.buffer.line_start(start_line);
        let block_end = if end_line + 1 < self.buffer.line_count() {
            self.buffer.line_start(end_line + 1)
        } else {
            self.buffer.len()
        };
        let block_text = self.buffer.text()[block_start..block_end].to_string();
        let has_blank_line_before = start_line == 0
            || self
                .buffer
                .line_content(start_line.saturating_sub(1))
                .trim()
                .is_empty();
        let has_blank_line_after = end_line + 1 < self.buffer.line_count()
            && self.buffer.line_content(end_line + 1).trim().is_empty();
        let leading_gap = if has_blank_line_before { "" } else { "\n" };
        let trailing_gap = if has_blank_line_after { "" } else { "\n\n" };
        let replacement = if block_text.ends_with('\n') {
            format!("{leading_gap}```bash\n{block_text}```{trailing_gap}")
        } else {
            format!("{leading_gap}```bash\n{block_text}\n```{trailing_gap}")
        };
        let prefix_len = "```bash\n".len();
        let body_start = block_start + leading_gap.len() + prefix_len;
        let replaced_len = block_end.saturating_sub(block_start);
        let replacement_len = replacement.len();
        let adjust_offset = |offset: usize| {
            if offset < block_start {
                offset
            } else if offset <= block_end {
                body_start + (offset - block_start).min(block_text.len())
            } else if replacement_len >= replaced_len {
                offset + (replacement_len - replaced_len)
            } else {
                offset.saturating_sub(replaced_len - replacement_len)
            }
        };

        self.buffer.replace(block_start..block_end, &replacement);
        if selection.is_empty() {
            let target = adjust_offset(self.cursor.offset)
                .max(body_start)
                .min(self.buffer.len());
            self.cursor.offset = target;
            self.cursor.anchor = target;
        } else {
            self.cursor.anchor = adjust_offset(self.cursor.anchor).min(self.buffer.len());
            self.cursor.offset = adjust_offset(self.cursor.offset).min(self.buffer.len());
        }
        self.reparse();
        self.scroll_to_cursor();
    }

    /// Strips a `marker … marker` pair whose content span the caret sits in and
    /// selects the freed text. Used by syntaxes the inline-span parser reports
    /// separately (highlight, superscript, subscript), which `apply_wrap` does
    /// not know how to toggle off.
    pub(crate) fn remove_marker_wrap_at_cursor(&mut self, marker: &str) -> bool {
        let line = self.cursor_line();
        let line_start = self.buffer.line_start(line);
        let col = self.cursor.offset.saturating_sub(line_start);
        let line_content = self.buffer.line_content(line).to_string();
        let Some(decoration) = self.decorations.get(line) else {
            return false;
        };
        let spans: &Vec<Range<usize>> = match marker {
            "==" => &decoration.highlight_spans,
            "^" => &decoration.superscript_spans,
            "~" => &decoration.subscript_spans,
            _ => return false,
        };
        let marker_len = marker.len();
        let Some(span) = spans
            .iter()
            .filter(|span| {
                let start = span.start.saturating_sub(marker_len);
                let end = (span.end + marker_len).min(line_content.len());
                col >= start && col <= end
            })
            .min_by_key(|span| span.end.saturating_sub(span.start))
            .cloned()
        else {
            return false;
        };
        let marker_start = span.start.saturating_sub(marker_len);
        let marker_end = (span.end + marker_len).min(line_content.len());
        if !line_content[marker_start..].starts_with(marker)
            || !line_content[..marker_end].ends_with(marker)
        {
            return false;
        }
        let range = line_start + marker_start..line_start + marker_end;
        let wrapped = self.buffer.text_for_range(range.clone()).to_string();
        let inner = wrapped[marker_len..wrapped.len() - marker_len].to_string();
        self.buffer.replace(range.clone(), &inner);
        self.cursor.anchor = range.start;
        self.cursor.offset = range.start + inner.len();
        self.reparse();
        self.scroll_to_cursor();
        true
    }

    /// Replaces the enclosing `range` with `keep` and selects what is left, so
    /// re-invoking Link / Image / WikiLink / Footnote removes the syntax
    /// instead of nesting another one.
    pub(crate) fn unwrap_syntax_range(&mut self, range: Range<usize>, keep: Range<usize>) {
        let kept = self.buffer.text_for_range(keep).to_string();
        self.buffer.replace(range.clone(), &kept);
        self.cursor.anchor = range.start;
        self.cursor.offset = range.start + kept.len();
        self.reparse();
        self.scroll_to_cursor();
    }

    /// Removes the fences around the code block the caret is in.
    pub(crate) fn unwrap_code_block_at_cursor(&mut self) -> bool {
        let line = self.cursor_line();
        let Some((fence_start, fence_end)) = self.code_fence_group_range(line) else {
            return false;
        };
        if fence_end <= fence_start {
            return false;
        }
        let body_start = self.buffer.line_start(fence_start + 1);
        let body_end = self.buffer.line_start(fence_end).min(self.buffer.len());
        if body_start > body_end {
            return false;
        }
        let body = self.buffer.text()[body_start..body_end].to_string();
        let block_start = self.buffer.line_start(fence_start);
        let block_end = if fence_end + 1 < self.buffer.line_count() {
            self.buffer.line_start(fence_end + 1)
        } else {
            self.buffer.len()
        };
        self.buffer.replace(block_start..block_end, &body);
        self.cursor.anchor = block_start;
        self.cursor.offset =
            (block_start + body.trim_end_matches('\n').len()).min(self.buffer.len());
        self.reparse();
        self.scroll_to_cursor();
        true
    }

    /// First and last physical line of the fenced code block containing `line`.
    fn code_fence_group_range(&self, line: usize) -> Option<(usize, usize)> {
        let is_fence = |index: usize| {
            self.decorations
                .get(index)
                .is_some_and(|decoration| decoration.block_type == BlockType::CodeBlockFence)
        };
        let in_block = |index: usize| {
            self.decorations.get(index).is_some_and(|decoration| {
                matches!(
                    decoration.block_type,
                    BlockType::CodeBlock | BlockType::CodeBlockFence
                )
            })
        };
        if !in_block(line) {
            return None;
        }
        let mut start = line;
        while start > 0 && in_block(start - 1) {
            start -= 1;
        }
        let mut end = line;
        while end + 1 < self.buffer.line_count() && in_block(end + 1) {
            end += 1;
        }
        (is_fence(start) && is_fence(end) && end > start).then_some((start, end))
    }

    pub(crate) fn apply_wrap(&mut self, prefix: &str, suffix: &str) {
        if self.extra_cursors.is_empty() {
            self.apply_wrap_single(prefix, suffix);
            return;
        }

        self.apply_single_cursor_edit_to_all_cursors(
            |_this, offset, anchor| offset.min(anchor),
            |this| this.apply_wrap_single(prefix, suffix),
        );
    }

    fn apply_wrap_single(&mut self, prefix: &str, suffix: &str) {
        let offset = self.cursor.offset;
        let (line, col) = self.buffer.offset_to_line_col(offset);
        let line_content = self.buffer.line_content(line).to_string();

        // Try to find an enclosing inline span the cursor/selection is in
        if let Some(dec) = self.decorations.get(line).cloned() {
            let enclosing = dec
                .inline_spans
                .iter()
                .filter_map(|span| {
                    let kind_prefix_len = match span.kind {
                        crate::markdown::InlineKind::Bold => 2,
                        crate::markdown::InlineKind::Italic => 1,
                        crate::markdown::InlineKind::Strikethrough => 2,
                        crate::markdown::InlineKind::Underline => 2,
                        crate::markdown::InlineKind::InlineCode => 1,
                        _ => return None,
                    };
                    let marker_start = span.range.start.saturating_sub(kind_prefix_len);
                    let marker_end = (span.range.end + kind_prefix_len).min(line_content.len());
                    let contains_cursor = if self.cursor.has_selection() {
                        let sel = self.cursor.selection_range();
                        let line_start = self.buffer.line_start(line);
                        let sel_start_col = sel.start.saturating_sub(line_start);
                        let sel_end_col = sel.end.saturating_sub(line_start);
                        sel_start_col >= marker_start && sel_end_col <= marker_end
                    } else {
                        col >= marker_start && col <= marker_end
                    };

                    contains_cursor.then_some((marker_end - marker_start, span))
                })
                .min_by_key(|(span_len, _)| *span_len)
                .map(|(_, span)| span);

            if let Some(span) = enclosing.cloned() {
                let (existing_prefix, existing_suffix) = match span.kind {
                    crate::markdown::InlineKind::Bold => ("**", "**"),
                    crate::markdown::InlineKind::Italic => ("*", "*"),
                    crate::markdown::InlineKind::Strikethrough => ("~~", "~~"),
                    crate::markdown::InlineKind::Underline => ("++", "++"),
                    crate::markdown::InlineKind::InlineCode => ("`", "`"),
                    _ => unreachable!(),
                };
                let marker_start = span.range.start.saturating_sub(existing_prefix.len());
                let marker_end = (span.range.end + existing_suffix.len()).min(line_content.len());
                if marker_start + existing_prefix.len() <= line_content.len()
                    && line_content[marker_start..].starts_with(existing_prefix)
                    && marker_end >= existing_suffix.len()
                    && line_content[marker_end - existing_suffix.len()..marker_end]
                        .ends_with(existing_suffix)
                {
                    let line_start = self.buffer.line_start(line);
                    let sel_start = line_start + marker_start;
                    let sel_end = line_start + marker_end;
                    let inner_start = sel_start + existing_prefix.len();
                    let inner_end = sel_end - existing_suffix.len();
                    let inner = self
                        .buffer
                        .text_for_range(inner_start..inner_end)
                        .to_string();
                    self.buffer.replace(sel_start..sel_end, &inner);
                    let new_start = sel_start;
                    let new_end = sel_start + inner.len();
                    if existing_prefix == prefix && existing_suffix == suffix {
                        // Same format: select the inner text
                        self.cursor.anchor = new_start;
                        self.cursor.offset = new_end;
                    } else {
                        // Different format: apply the new wrap around inner text
                        let replacement = format!("{prefix}{inner}{suffix}");
                        self.buffer.replace(new_start..new_end, &replacement);
                        let new_start2 = new_start + prefix.len();
                        let new_end2 = new_start2 + inner.len();
                        self.cursor.anchor = new_start2;
                        self.cursor.offset = new_end2;
                    }
                    self.reparse();
                    self.scroll_to_cursor();
                    return;
                }
            }
        }

        let range = self.cursor.selection_range();
        let selected = self.buffer.text_for_range(range.clone()).to_string();
        let (replacement, new_start, new_end) = if range.is_empty() {
            (
                format!("{prefix}{suffix}"),
                range.start + prefix.len(),
                range.start + prefix.len(),
            )
        } else if selected.starts_with(prefix)
            && selected.ends_with(suffix)
            && selected.len() >= prefix.len() + suffix.len()
        {
            let inner = selected[prefix.len()..selected.len() - suffix.len()].to_string();
            let end = range.start + inner.len();
            (inner, range.start, end)
        } else {
            (
                format!("{prefix}{selected}{suffix}"),
                range.start + prefix.len(),
                range.end + prefix.len(),
            )
        };
        self.buffer.replace(range, &replacement);
        self.cursor.anchor = new_start;
        self.cursor.offset = new_end;
        self.reparse();
        self.scroll_to_cursor();
    }

    pub(crate) fn apply_heading(&mut self, level: u8) {
        if self.extra_cursors.is_empty() {
            self.apply_heading_single(level);
            return;
        }

        self.apply_single_cursor_edit_to_all_cursors(
            |this, offset, _anchor| {
                let (line, _) = this.buffer.offset_to_line_col(offset);
                this.buffer.line_start(line)
            },
            |this| this.apply_heading_single(level),
        );
    }

    fn apply_heading_single(&mut self, level: u8) {
        let (line, _) = self.buffer.offset_to_line_col(self.cursor.offset);
        let line_start = self.buffer.line_start(line);
        let line_end = self.buffer.line_end(line);
        let line_text = self.buffer.line_content(line).to_string();
        let target_prefix = format!("{} ", "#".repeat(level as usize));
        let (replacement, new_cursor) = if line_text.starts_with(&target_prefix) {
            (line_text[target_prefix.len()..].to_string(), line_start)
        } else {
            let stripped = line_text.trim_start_matches('#');
            let existing_hashes = line_text.len() - stripped.len();
            let content = if existing_hashes > 0 {
                stripped.strip_prefix(' ').unwrap_or(stripped)
            } else {
                &line_text
            };
            (
                format!("{target_prefix}{content}"),
                line_start + target_prefix.len(),
            )
        };
        self.buffer.replace(line_start..line_end, &replacement);
        self.cursor.move_to(new_cursor, &self.buffer);
        self.reparse();
        self.scroll_to_cursor();
    }

    pub(crate) fn move_cursor_to_first_line_end(&mut self) {
        let mut target_line = 0;
        while target_line + 1 < self.buffer.line_count()
            && self
                .decorations
                .get(target_line)
                .and_then(|decoration| decoration.table_info.as_ref())
                .is_some_and(|table_info| table_info.is_front_matter)
        {
            target_line += 1;
        }
        let target = self.buffer.line_end(target_line).min(self.buffer.len());
        self.cursor.move_to(target, &self.buffer);
        self.scroll_to_cursor();
    }

    pub(crate) fn insert_sidebar_items_as_wiki_links(
        &mut self,
        paths: &[PathBuf],
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab_is_markdown() || paths.is_empty() {
            return;
        }

        let policy = self.app_settings.attachment_policy();
        let index = attachments::WorkspaceLinkIndex::build(&self.sidebar_root);
        let unsaved_source = self.sidebar_root.join(".untitled.md");
        let current_file = self.current_file_path.as_deref().unwrap_or(&unsaved_source);
        let links: Vec<String> = paths
            .iter()
            .filter_map(|path| {
                policy
                    .reference_for(
                        &self.sidebar_root,
                        current_file,
                        path,
                        is_image_path(path),
                        Some(&index),
                    )
                    .ok()
            })
            .collect();
        if links.is_empty() {
            return;
        }

        let insertion = links.join("\n");
        self.checkpoint();
        let range = self.cursor.selection_range();
        let new_offset = range.start + insertion.len();
        self.buffer.replace(range, &insertion);
        self.cursor.move_to(new_offset, &self.buffer);
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
        self.mark_active_tab_dirty();
        self.request_editor_focus(cx);
    }

    // ── Table context menu operations ──────────────────────────────

    fn table_lines_for_block(
        &self,
        table_start_line: usize,
    ) -> Option<(usize, usize, Vec<String>)> {
        let dec = self.decorations.get(table_start_line)?;
        let ti = dec.table_info.as_ref()?;
        let start = ti.table_start_line;
        let end = ti.table_end_line;
        let lines: Vec<String> = (start..end)
            .map(|l| self.buffer.line_content(l).to_string())
            .collect();
        Some((start, end, lines))
    }

    pub(crate) fn block_is_front_matter(&self, table_start_line: usize) -> bool {
        self.decorations
            .get(table_start_line)
            .and_then(|dec| dec.table_info.as_ref())
            .is_some_and(|ti| ti.is_front_matter)
    }

    fn replace_table_block(
        &mut self,
        table_start: usize,
        table_end: usize,
        new_lines: &[String],
        cx: &mut Context<Self>,
    ) {
        let text_start = self.buffer.line_start(table_start);
        let text_end = self.buffer.line_end(table_end.saturating_sub(1));
        let new_text = new_lines.join("\n");
        self.ime_composition_snapshot = None;
        self.checkpoint();
        let replaced_range = text_start..text_end;
        self.queue_non_markdown_line_display_transfer(&replaced_range, &new_text);
        self.buffer.replace(replaced_range, &new_text);
        self.invalidate_indent_guide_step_cache();
        self.reparse();
        self.commit_history_transaction();
        self.schedule_auto_save_for_active_tab(cx);
    }

    pub(crate) fn table_insert_row(
        &mut self,
        table_start_line: usize,
        visual_row: usize,
        above: bool,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = None;
        let Some((start, end, lines)) = self.table_lines_for_block(table_start_line) else {
            cx.notify();
            return;
        };
        let insert_at = if above { visual_row } else { visual_row + 1 };
        let new_lines = if self.block_is_front_matter(table_start_line) {
            crate::table_ops::insert_front_matter_row(&lines, insert_at)
        } else {
            crate::table_ops::insert_row(&lines, insert_at)
        };
        self.replace_table_block(start, end, &new_lines, cx);
        cx.notify();
    }

    pub(crate) fn table_delete_row(
        &mut self,
        table_start_line: usize,
        visual_row: usize,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = None;
        let Some((start, end, lines)) = self.table_lines_for_block(table_start_line) else {
            cx.notify();
            return;
        };
        let new_lines = if self.block_is_front_matter(table_start_line) {
            crate::table_ops::delete_front_matter_row(&lines, visual_row)
        } else {
            crate::table_ops::delete_row(&lines, visual_row)
        };
        self.replace_table_block(start, end, &new_lines, cx);
        cx.notify();
    }

    pub(crate) fn table_insert_column(
        &mut self,
        table_start_line: usize,
        col: usize,
        left: bool,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = None;
        if self.block_is_front_matter(table_start_line) {
            cx.notify();
            return;
        }
        let Some((start, end, lines)) = self.table_lines_for_block(table_start_line) else {
            cx.notify();
            return;
        };
        let insert_at = if left { col } else { col + 1 };
        let new_lines = crate::table_ops::insert_column(&lines, insert_at);
        self.replace_table_block(start, end, &new_lines, cx);
        cx.notify();
    }

    pub(crate) fn table_delete_column(
        &mut self,
        table_start_line: usize,
        col: usize,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = None;
        if self.block_is_front_matter(table_start_line) {
            cx.notify();
            return;
        }
        let Some((start, end, lines)) = self.table_lines_for_block(table_start_line) else {
            cx.notify();
            return;
        };
        let new_lines = crate::table_ops::delete_column(&lines, col);
        self.replace_table_block(start, end, &new_lines, cx);
        cx.notify();
    }

    pub(crate) fn table_set_column_alignment(
        &mut self,
        table_start_line: usize,
        col: usize,
        alignment: crate::markdown::TableAlign,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = None;
        if self.block_is_front_matter(table_start_line) {
            cx.notify();
            return;
        }
        let Some((start, end, lines)) = self.table_lines_for_block(table_start_line) else {
            cx.notify();
            return;
        };
        let new_lines = crate::table_ops::set_column_alignment(&lines, col, alignment);
        self.replace_table_block(start, end, &new_lines, cx);
        cx.notify();
    }

    pub(crate) fn table_delete_table(&mut self, table_start_line: usize, cx: &mut Context<Self>) {
        self.context_menu = None;
        let Some((start, end, _)) = self.table_lines_for_block(table_start_line) else {
            cx.notify();
            return;
        };
        let text_start = self.buffer.line_start(start);
        let text_end = if end < self.buffer.line_count() {
            self.buffer.line_start(end)
        } else {
            self.buffer.len()
        };
        self.ime_composition_snapshot = None;
        self.checkpoint();
        let replaced_range = text_start..text_end;
        self.queue_non_markdown_line_display_transfer(&replaced_range, "");
        self.buffer.replace(replaced_range, "");
        self.cursor.offset = text_start.min(self.buffer.len());
        self.cursor.anchor = self.cursor.offset;
        self.invalidate_indent_guide_step_cache();
        self.reparse();
        self.commit_history_transaction();
        self.schedule_auto_save_for_active_tab(cx);
        cx.notify();
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
            if lws < indent {
                break;
            }
            if lws > indent {
                continue;
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

    pub(crate) fn ordered_list_prefix_info(
        &self,
        line: usize,
    ) -> Option<(Range<usize>, usize, u32)> {
        if line >= self.buffer.line_count() {
            return None;
        }

        let line_content = self.buffer.line_content(line).to_string();
        let leading_ws = line_content.len() - line_content.trim_start_matches([' ', '\t']).len();
        let trimmed = &line_content[leading_ws..];
        let prefix_len = markdown::ordered_list_prefix_len(trimmed);
        if prefix_len < 2 {
            return None;
        }

        let number_text = &trimmed[..prefix_len - 2];
        let number = number_text.parse::<u32>().ok()?;
        let line_start = self.buffer.line_start(line);
        Some((
            line_start + leading_ws..line_start + leading_ws + prefix_len,
            leading_ws,
            number,
        ))
    }

    pub(crate) fn ordered_list_prefix_touched_by_range(
        &self,
        range: &Range<usize>,
    ) -> Option<usize> {
        let (start_line, _) = self
            .buffer
            .offset_to_line_col(range.start.min(self.buffer.len()));
        let end_probe = range
            .end
            .saturating_sub(usize::from(range.end > range.start));
        let (end_line, _) = self
            .buffer
            .offset_to_line_col(end_probe.min(self.buffer.len()));

        for line in start_line..=end_line {
            let Some((prefix_range, _, _)) = self.ordered_list_prefix_info(line) else {
                continue;
            };
            let touches = if range.is_empty() {
                range.start >= prefix_range.start && range.start <= prefix_range.end
            } else {
                range.start < prefix_range.end && range.end > prefix_range.start
            };
            if touches {
                return Some(line);
            }
        }

        None
    }

    pub(crate) fn normalize_all_ordered_lists(&mut self) -> bool {
        match normalize_ordered_list_numbers_in_buffer(&mut self.buffer) {
            Some((first_line, last_line)) => {
                // Renumbering rewrites the digits on these lines in place. Drop
                // their shaping cache entries so the next reparse (which reuses
                // the rest of the cache to stay windowed) re-shapes them with
                // the corrected numbers instead of showing stale glyphs.
                for line in first_line..=last_line {
                    if line < self.line_display_info.len() {
                        self.line_display_info[line] = None;
                    }
                }
                self.invalidate_line_geometry_cache();
                true
            }
            None => false,
        }
    }

    pub(crate) fn renumber_ordered_list_block_from(
        &mut self,
        start_line: usize,
        starting_number: u32,
        indent: usize,
    ) {
        let mut items: Vec<(usize, usize, u32)> = Vec::new();
        let mut number = starting_number;
        for line in start_line.. {
            if line >= self.buffer.line_count() {
                break;
            }
            let lc = self.buffer.line_content(line).to_string();
            let lws = lc.len() - lc.trim_start_matches([' ', '\t']).len();
            if lws < indent {
                break;
            }
            if lws > indent {
                continue;
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

    fn previous_ordered_list_number_before(&self, line: usize, indent: usize) -> Option<u32> {
        if line == 0 {
            return None;
        }

        for prev_line in (0..line).rev() {
            let line_content = self.buffer.line_content(prev_line).to_string();
            let trimmed = line_content.trim();
            if trimmed.is_empty() {
                break;
            }

            let leading_ws =
                line_content.len() - line_content.trim_start_matches([' ', '\t']).len();
            if leading_ws > indent {
                continue;
            }
            if leading_ws < indent {
                break;
            }

            let Some((_, prev_indent, prev_number)) = self.ordered_list_prefix_info(prev_line)
            else {
                break;
            };
            if prev_indent == indent {
                return Some(prev_number);
            }
        }

        None
    }

    pub(crate) fn normalize_ordered_list_near_line(
        &mut self,
        line: usize,
        preserve_current_number: bool,
    ) {
        let candidate_line = if self.ordered_list_prefix_info(line).is_some() {
            Some(line)
        } else if line + 1 < self.buffer.line_count()
            && self.ordered_list_prefix_info(line + 1).is_some()
        {
            Some(line + 1)
        } else {
            None
        };

        let Some(target_line) = candidate_line else {
            return;
        };
        let Some((_, indent, current_number)) = self.ordered_list_prefix_info(target_line) else {
            return;
        };

        if preserve_current_number {
            self.renumber_ordered_list_after(target_line + 1, current_number + 1, indent);
            return;
        }

        let starting_number = self
            .previous_ordered_list_number_before(target_line, indent)
            .map(|prev_number| prev_number + 1)
            .unwrap_or(1);

        self.renumber_ordered_list_block_from(target_line, starting_number, indent);
    }
}

fn normalize_ordered_list_numbers_in_buffer(buffer: &mut Buffer) -> Option<(usize, usize)> {
    if buffer.line_count() == 0 {
        return None;
    }

    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut number_stack: Vec<(usize, u32)> = Vec::new();
    let mut changed_line_range: Option<(usize, usize)> = None;

    for line in 0..buffer.line_count() {
        let line_content = buffer.line_content(line);
        if line_content.trim().is_empty() {
            number_stack.clear();
            continue;
        }

        let leading_ws = line_content.len() - line_content.trim_start_matches([' ', '\t']).len();
        while number_stack
            .last()
            .is_some_and(|(indent, _)| *indent > leading_ws)
        {
            number_stack.pop();
        }

        let trimmed = &line_content[leading_ws..];
        let prefix_len = markdown::ordered_list_prefix_len(trimmed);
        let current_number = if prefix_len >= 2 {
            trimmed[..prefix_len - 2].parse::<u32>().ok()
        } else {
            None
        };

        let Some(current_number) = current_number else {
            if number_stack
                .last()
                .is_some_and(|(indent, _)| *indent == leading_ws)
            {
                number_stack.pop();
            }
            continue;
        };

        let expected_number = match number_stack.last_mut() {
            Some((indent, next_number)) if *indent == leading_ws => {
                let expected = *next_number;
                *next_number += 1;
                expected
            }
            Some((indent, _)) if *indent < leading_ws => {
                number_stack.push((leading_ws, 2));
                1
            }
            _ => {
                number_stack.push((leading_ws, 2));
                1
            }
        };

        if current_number == expected_number {
            continue;
        }

        let number_text = &trimmed[..prefix_len - 2];
        let number_start = buffer.line_start(line) + leading_ws;
        let number_end = number_start + number_text.len();
        edits.push((number_start, number_end, expected_number.to_string()));
        changed_line_range = Some(match changed_line_range {
            Some((first, last)) => (first.min(line), last.max(line)),
            None => (line, line),
        });
    }

    for (number_start, number_end, new_number) in edits.into_iter().rev() {
        buffer.replace(number_start..number_end, &new_number);
    }
    changed_line_range
}

// ── Free functions ──────────────────────────────────────────────────

pub(crate) fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "md" | "markdown" | "mdx"))
}

fn split_external_paths(paths: &[PathBuf]) -> (Vec<PathBuf>, Vec<PathBuf>) {
    paths
        .iter()
        .cloned()
        .partition(|path| is_markdown_file(path))
}

pub(crate) fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "png"
                    | "jpg"
                    | "jpeg"
                    | "gif"
                    | "webp"
                    | "bmp"
                    | "ico"
                    | "tif"
                    | "tiff"
                    | "avif"
                    | "svg"
            )
        })
}

pub(crate) fn should_skip_path(path: &Path, is_dir: bool) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };

    if name.starts_with('.') {
        return true;
    }

    if !is_dir {
        return false;
    }

    matches!(
        name,
        "node_modules" | "target" | "dist" | "build" | "coverage" | ".git"
    )
}

fn looks_like_text(bytes: &[u8]) -> bool {
    if bytes.contains(&0) {
        return false;
    }

    if bytes.is_empty() {
        return true;
    }

    let sample = &bytes[..bytes.len().min(4096)];
    let control_count = sample
        .iter()
        .filter(|&&byte| byte < 0x20 && !matches!(byte, b'\n' | b'\r' | b'\t' | 0x08 | 0x0c))
        .count();
    if control_count * 10 > sample.len().max(1) {
        return false;
    }

    if std::str::from_utf8(sample).is_ok() {
        return true;
    }

    let lossy = String::from_utf8_lossy(sample);
    let replacement_count = lossy.chars().filter(|&ch| ch == '\u{fffd}').count();
    replacement_count * 20 <= lossy.chars().count().max(1)
}

pub(crate) fn classify_file_kind_for_bytes(path: &Path, bytes: &[u8]) -> FileKind {
    if is_markdown_file(path) {
        return FileKind::Markdown;
    }

    if is_image_file(path) {
        return FileKind::Image;
    }

    if is_whiteboard_file(path) {
        return FileKind::Whiteboard;
    }

    if let Some(syntax) = helpers::find_syntax_for_path(path)
        && syntax.name != "Plain Text"
        && syntax.name != "Markdown"
    {
        return FileKind::Code {
            language: syntax.name.to_string(),
        };
    }

    if looks_like_text(bytes) {
        FileKind::Text
    } else {
        FileKind::Binary
    }
}

pub(crate) fn file_kind_badge_text(path: &Path) -> Option<String> {
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(file_kind_badge_text_for_name)
}

pub(crate) fn file_kind_has_indicator(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(file_kind_has_indicator_for_name)
}

pub(crate) fn file_kind_badge_text_for_name(name: &str) -> Option<String> {
    let ext = Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .filter(|ext| !ext.is_empty())?;
    if matches!(ext.to_ascii_lowercase().as_str(), "md" | "markdown" | "mdx") {
        None
    } else {
        Some(ext.to_ascii_uppercase())
    }
}

pub(crate) fn file_kind_has_indicator_for_name(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| !ext.is_empty())
}

pub(crate) fn split_file_name_for_display(name: &str) -> (&str, Option<&str>) {
    match name.rfind('.') {
        Some(index) if index > 0 && index + 1 < name.len() => {
            (&name[..index], Some(&name[index..]))
        }
        _ => (name, None),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MarkdownNavigationTarget {
    ExternalUrl(String),
    CurrentAnchor(String),
    File {
        path: PathBuf,
        anchor: Option<String>,
    },
}

fn split_markdown_target(target: &str) -> (&str, Option<&str>) {
    match target.split_once('#') {
        Some((path, anchor)) => (path, Some(anchor)),
        None => (target, None),
    }
}

fn strip_heading_attribute_block(text: &str) -> (&str, Option<&str>) {
    let trimmed = text.trim_end();
    if !trimmed.ends_with('}') {
        return (trimmed, None);
    }

    let Some(open_brace) = trimmed.rfind('{') else {
        return (trimmed, None);
    };
    let attrs = &trimmed[open_brace + 1..trimmed.len() - 1];
    if !attrs.split_whitespace().any(|token| token.starts_with('#')) {
        return (trimmed, None);
    }

    (trimmed[..open_brace].trim_end(), Some(attrs))
}

fn slugify_heading_text(text: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;

    for ch in text.trim().chars() {
        let mut appended_alnum = false;
        for lower in ch.to_lowercase() {
            if lower.is_alphanumeric() {
                slug.push(lower);
                last_was_dash = false;
                appended_alnum = true;
            }
        }

        if appended_alnum {
            continue;
        }

        if (ch.is_whitespace() || ch == '-') && !slug.is_empty() && !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }

    while slug.ends_with('-') {
        slug.pop();
    }
    slug
}

fn heading_matches_anchor(raw_line: &str, anchor: &str) -> bool {
    let trimmed = raw_line.trim_start();
    if !trimmed.starts_with('#') {
        return false;
    }

    let heading_text = trimmed.trim_start_matches('#').trim_start();
    let (text_without_attrs, attrs) = strip_heading_attribute_block(heading_text);
    if let Some(attrs) = attrs
        && attrs
            .split_whitespace()
            .filter_map(|token| token.strip_prefix('#'))
            .any(|id| id == anchor)
    {
        return true;
    }

    slugify_heading_text(text_without_attrs) == anchor
}

fn resolve_markdown_target_path(
    sidebar_root: &Path,
    current_file_path: Option<&Path>,
    target: &str,
) -> Option<PathBuf> {
    let (path_part, _) = split_markdown_target(target);
    let path_part = path_part.trim();
    if path_part.is_empty() {
        return current_file_path.map(Path::to_path_buf);
    }

    attachments::WorkspaceLinkIndex::build(sidebar_root).resolve(current_file_path, path_part)
}

pub(crate) fn resolve_wiki_link_target_path(sidebar_root: &Path, target: &str) -> Option<PathBuf> {
    resolve_markdown_target_path(sidebar_root, None, target)
}

pub(crate) fn parse_decorations_for_file_kind(
    text: &str,
    file_kind: &FileKind,
) -> Vec<LineDecoration> {
    match file_kind {
        FileKind::Markdown => markdown::parse_decorations(text),
        FileKind::Code { language } => plain_text_decorations(text, Some(language.clone())),
        FileKind::Text | FileKind::Image | FileKind::Binary | FileKind::Whiteboard => {
            plain_text_decorations(text, None)
        }
    }
}

fn plain_text_decorations(text: &str, _code_language: Option<String>) -> Vec<LineDecoration> {
    let line_count = text.lines().count().max(1);
    let line_count = if text.ends_with('\n') {
        line_count + 1
    } else {
        line_count
    };

    let decorations: Vec<LineDecoration> =
        (0..line_count).map(|_| LineDecoration::default()).collect();
    decorations
}

impl EditorView {
    fn invalidate_line_geometry_cache(&self) {
        self.line_geometry_cache_dirty.set(true);
    }

    pub(crate) fn reset_sparse_line_geometry(&self) {
        self.line_geometry_cache.borrow_mut().sparse = SparseLineGeometry::default();
    }

    fn clear_sparse_line_geometry_range(&self, range: Range<usize>) {
        self.line_geometry_cache
            .borrow_mut()
            .sparse
            .clear_range(range);
    }

    fn invalidate_indent_guide_step_cache(&self) {
        self.indent_guide_step_cache_dirty.set(true);
    }

    fn cached_non_markdown_indent_guide_step(&self) -> usize {
        if !self.indent_guide_step_cache_dirty.get() {
            return self.indent_guide_step_cache.get();
        }

        let step = element::detect_indent_guide_step(&self.buffer, self.buffer.line_count());
        self.indent_guide_step_cache.set(step);
        self.indent_guide_step_cache_dirty.set(false);
        step
    }

    fn clear_line_display_info(&mut self) {
        self.line_display_info.clear();
        self.reset_sparse_line_geometry();
        self.invalidate_line_geometry_cache();
    }

    fn invalidate_line_display_info_for_line(&mut self, line: usize) {
        if let Some(info) = self.line_display_info.get_mut(line) {
            *info = None;
        }
        self.clear_sparse_line_geometry_range(line..line.saturating_add(1));
        self.layout_dirty = true;
        self.invalidate_line_geometry_cache();
    }

    /// Invalidate the cached per-line layout for the active editor AND every
    /// inactive tab. Use this when a global rendering parameter (UI scale,
    /// fonts, theme, line height, letter spacing) changes, because those affect
    /// the layout of all tabs, not just the one currently shown. Inactive tabs
    /// recompute lazily on activation, so this stays cheap.
    pub(crate) fn clear_all_tab_layout_caches(&mut self) {
        self.clear_line_display_info();
        self.tab_layout_cache.clear();
    }

    pub(crate) fn clear_active_tab_layout_cache(&mut self) {
        self.clear_line_display_info();
        if let Some(tab_id) = self.active_tab_id.as_ref() {
            self.tab_layout_cache.remove(tab_id);
        }
        self.invalidate_indent_guide_step_cache();
        self.layout_dirty = true;
    }

    fn set_line_display_info(&mut self, line_display_info: LineDisplayCache) {
        self.line_display_info = line_display_info;
        self.invalidate_line_geometry_cache();
        self.maybe_trim_line_display_info_cache_after_update();
    }

    fn apply_line_display_info_updates(
        &mut self,
        line_count: usize,
        updates: Vec<(usize, Option<LineDisplayInfo>)>,
    ) {
        if self.line_display_info.len() > line_count {
            self.line_display_info.truncate(line_count);
        } else if self.line_display_info.len() < line_count {
            self.line_display_info.resize(line_count, None);
        }

        // Only invalidate the cumulative line-geometry cache when an update
        // actually changes a line's visual height. During steady scrolling the
        // window keeps re-emitting identical cached heights, and a blind
        // invalidation would force an O(n) prefix-sum rebuild every frame.
        let info_height = |info: Option<&LineDisplayInfo>| {
            info.map(|info| info.line_height * info.visual_line_count as f32 + info.extra_height)
        };
        let mut geometry_changed = false;
        let sparse_geometry = self.uses_sparse_line_geometry();
        if sparse_geometry {
            self.line_geometry_cache
                .borrow_mut()
                .sparse
                .ensure_dimensions(
                    line_count,
                    self.line_height_for_block(&BlockType::Normal) as f64,
                );
        }
        for (line_idx, info) in updates {
            if line_idx < line_count {
                if !geometry_changed {
                    let old_height = info_height(self.line_display_info[line_idx].as_deref());
                    let new_height = info_height(info.as_ref());
                    geometry_changed = match (old_height, new_height) {
                        (Some(a), Some(b)) => (a - b).abs() > 0.01,
                        (None, None) => false,
                        _ => true,
                    };
                }
                if sparse_geometry && let Some(height) = info_height(info.as_ref()) {
                    self.line_geometry_cache
                        .borrow_mut()
                        .sparse
                        .set_measured_height(line_idx, height as f64);
                }
                self.line_display_info[line_idx] = info.map(Box::new);
            }
        }

        if geometry_changed {
            self.invalidate_line_geometry_cache();
        }
        self.maybe_trim_line_display_info_cache_after_update();
    }

    fn take_line_display_info(&mut self) -> LineDisplayCache {
        self.invalidate_line_geometry_cache();
        std::mem::take(&mut self.line_display_info)
    }

    fn set_decorations(&mut self, decorations: Vec<LineDecoration>) {
        self.decorations = decorations;
        self.lazy_markdown_decorations = None;
        self.markdown_outline_index.borrow_mut().take();
        self.markdown_outline_build_task = None;
        self.markdown_outline_epoch = self.markdown_outline_epoch.wrapping_add(1);
        self.outline_headings_cache_dirty.set(true);
        self.invalidate_outline_layout_cache();
        self.reset_sparse_line_geometry();
        self.invalidate_line_geometry_cache();
    }

    pub(crate) fn uses_lazy_markdown_decorations(&self) -> bool {
        self.lazy_markdown_decorations.is_some()
    }

    pub(crate) fn uses_sparse_line_geometry(&self) -> bool {
        self.buffer.line_count() >= LARGE_MARKDOWN_MIN_LINES
            && self
                .active_file_kind()
                .is_some_and(FileKind::supports_text_buffer)
    }

    fn reset_lazy_markdown_decorations(&mut self) {
        let line_count = self.buffer.line_count().max(1);
        let (decorations, lazy) = new_lazy_markdown_decorations(line_count);
        self.decorations = decorations;
        self.lazy_markdown_decorations = Some(lazy);
        self.markdown_outline_index.borrow_mut().take();
        self.markdown_outline_build_task = None;
        self.markdown_outline_epoch = self.markdown_outline_epoch.wrapping_add(1);
        self.invalidate_outline_layout_cache();
        self.reset_sparse_line_geometry();
        self.invalidate_line_geometry_cache();
    }

    pub(crate) fn ensure_lazy_markdown_decorations(&mut self, range: Range<usize>) -> bool {
        let Some(lazy) = self.lazy_markdown_decorations.as_mut() else {
            return false;
        };
        let changed = parse_lazy_markdown_range(&self.buffer, &mut self.decorations, lazy, range);

        changed
    }

    pub(crate) fn begin_large_document_frame_bench_prepaint(&mut self) {
        let Some(bench) = self.large_document_frame_bench.as_mut() else {
            return;
        };
        let now = Instant::now();
        if bench.frame >= LARGE_DOCUMENT_FRAME_BENCH_WARMUP
            && let Some(last_frame) = bench.last_frame
        {
            let interval = now - last_frame;
            if bench.frame
                < LARGE_DOCUMENT_FRAME_BENCH_WARMUP + LARGE_DOCUMENT_FRAME_BENCH_SCROLL_FRAMES
            {
                bench.scroll_frame_intervals.push(interval);
            } else {
                bench.edit_frame_intervals.push(interval);
            }
        }
        bench.last_frame = Some(now);
    }

    pub(crate) fn begin_large_document_frame_work(&mut self) {
        if let Some(bench) = self.large_document_frame_bench.as_mut() {
            bench.frame_work_started = Some(Instant::now());
        }
    }

    pub(crate) fn record_large_document_syntax_viewport(&mut self, range: Range<usize>) {
        if !self.uses_lazy_markdown_decorations() {
            return;
        }
        let Some(bench) = self.large_document_frame_bench.as_mut() else {
            return;
        };
        let start = range.start.min(self.decorations.len());
        let end = range.end.min(self.decorations.len()).max(start);
        if start < 100_000 || start == end {
            return;
        }
        bench.syntax_view_samples += 1;
        let has_syntax = self.decorations[start..end].iter().any(|decoration| {
            decoration.block_type != BlockType::Normal
                || !decoration.inline_spans.is_empty()
                || !decoration.syntax_ranges.is_empty()
        });
        if !has_syntax {
            bench.syntax_view_misses += 1;
        }
    }

    pub(crate) fn end_large_document_frame_work(&mut self) {
        let Some(bench) = self.large_document_frame_bench.as_mut() else {
            return;
        };
        if bench.frame >= LARGE_DOCUMENT_FRAME_BENCH_WARMUP
            && let Some(started) = bench.frame_work_started.take()
        {
            bench.frame_work_times.push(started.elapsed());
        }
    }

    pub(crate) fn initialize_large_document_frame_bench_if_needed(&mut self) {
        if self.large_document_frame_bench.is_none()
            && self.buffer.line_count() >= LARGE_MARKDOWN_MIN_LINES
        {
            self.large_document_frame_bench = LargeDocumentFrameBench::from_env();
        }
    }

    pub(crate) fn advance_large_document_frame_bench(&mut self) -> Option<bool> {
        let Some(mut bench) = self.large_document_frame_bench.take() else {
            return None;
        };
        if bench.frame
            < LARGE_DOCUMENT_FRAME_BENCH_WARMUP + LARGE_DOCUMENT_FRAME_BENCH_SCROLL_FRAMES
        {
            self.scroll_offset += self.line_height_for_block(&BlockType::Normal) * 3.0;
            self.clamp_scroll();
        } else {
            let started = Instant::now();
            let operation = (bench.frame
                - LARGE_DOCUMENT_FRAME_BENCH_WARMUP
                - LARGE_DOCUMENT_FRAME_BENCH_SCROLL_FRAMES)
                % 10;
            match operation {
                0 => {
                    let edit_line = self
                        .line_at_y(self.scroll_offset + self.last_viewport_height * 0.5)
                        .min(self.buffer.line_count().saturating_sub(1));
                    let offset = self.buffer.line_start(edit_line);
                    self.checkpoint();
                    self.queue_non_markdown_line_display_transfer(&(offset..offset), "x");
                    self.buffer.insert(offset, "x");
                    self.cursor.move_to(offset + 1, &self.buffer);
                    self.reparse();
                    self.commit_history_transaction();
                    bench.pending_edit_offset = Some(offset);
                    bench.character_edit_times.push(started.elapsed());
                }
                1 => {
                    let offset = bench
                        .pending_edit_offset
                        .take()
                        .unwrap_or(self.cursor.offset);
                    self.checkpoint();
                    self.queue_non_markdown_line_display_transfer(&(offset..offset + 1), "");
                    self.buffer.delete(offset..offset + 1);
                    self.cursor.move_to(offset, &self.buffer);
                    self.reparse();
                    self.commit_history_transaction();
                    bench.character_edit_times.push(started.elapsed());
                }
                2 => {
                    let offset = self.cursor.offset;
                    self.checkpoint();
                    self.queue_non_markdown_line_display_transfer(&(offset..offset), "\n");
                    let buffer_started = Instant::now();
                    self.buffer.insert(offset, "\n");
                    bench.newline_buffer_times.push(buffer_started.elapsed());
                    self.cursor.move_to(offset + 1, &self.buffer);
                    let reparse_started = Instant::now();
                    self.reparse();
                    bench.newline_reparse_times.push(reparse_started.elapsed());
                    let history_started = Instant::now();
                    self.commit_history_transaction();
                    bench.newline_history_times.push(history_started.elapsed());
                    bench.pending_newline_offset = Some(offset);
                    bench.newline_edit_times.push(started.elapsed());
                }
                3 => {
                    let offset = bench
                        .pending_newline_offset
                        .take()
                        .unwrap_or(self.cursor.offset);
                    self.checkpoint();
                    self.queue_non_markdown_line_display_transfer(&(offset..offset + 1), "");
                    let buffer_started = Instant::now();
                    self.buffer.delete(offset..offset + 1);
                    bench.newline_buffer_times.push(buffer_started.elapsed());
                    self.cursor.move_to(offset, &self.buffer);
                    let reparse_started = Instant::now();
                    self.reparse();
                    bench.newline_reparse_times.push(reparse_started.elapsed());
                    let history_started = Instant::now();
                    self.commit_history_transaction();
                    bench.newline_history_times.push(history_started.elapsed());
                    bench.newline_edit_times.push(started.elapsed());
                }
                4 => {
                    self.cursor.anchor = 0;
                    self.cursor.offset = self.buffer.len();
                    bench.selection_times.push(started.elapsed());
                }
                5 => {
                    self.cursor.anchor = self.cursor.offset;
                    bench.selection_times.push(started.elapsed());
                }
                6 => {
                    self.commit_history_transaction();
                    if let Some(index) = self.active_tab_index()
                        && let Some((edits, view)) =
                            self.tabs[index].history.take_undo_transaction()
                    {
                        self.buffer.apply_edits_inverse(&edits);
                        self.cursor.offset = view.cursor_offset.min(self.buffer.len());
                        self.cursor.anchor = view.cursor_anchor.min(self.buffer.len());
                        self.reparse();
                    }
                    bench.history_times.push(started.elapsed());
                }
                7 => {
                    if let Some(index) = self.active_tab_index()
                        && let Some((edits, view)) =
                            self.tabs[index].history.take_redo_transaction()
                    {
                        self.buffer.apply_edits_forward(&edits);
                        self.cursor.offset = view.cursor_offset.min(self.buffer.len());
                        self.cursor.anchor = view.cursor_anchor.min(self.buffer.len());
                        self.reparse();
                    }
                    bench.history_times.push(started.elapsed());
                }
                8 => {
                    self.scroll_offset = self.max_scroll_offset() * 0.85;
                    bench.scrollbar_jump_times.push(started.elapsed());
                }
                _ => {
                    self.scroll_offset = self.max_scroll_offset() * 0.15;
                    bench.scrollbar_jump_times.push(started.elapsed());
                }
            }
            bench.edit_times.push(started.elapsed());
        }

        bench.frame += 1;
        let total_frames = LARGE_DOCUMENT_FRAME_BENCH_WARMUP
            + LARGE_DOCUMENT_FRAME_BENCH_SCROLL_FRAMES
            + LARGE_DOCUMENT_FRAME_BENCH_EDIT_FRAMES;
        if bench.frame < total_frames {
            self.large_document_frame_bench = Some(bench);
            return Some(true);
        }

        let percentile = |samples: &[Duration], percentile: usize| {
            let mut sorted = samples.to_vec();
            sorted.sort_unstable();
            sorted
                .get(sorted.len().saturating_sub(1) * percentile / 100)
                .copied()
                .unwrap_or_default()
                .as_secs_f64()
                * 1_000.0
        };
        let output = serde_json::json!({
            "scroll_frames": bench.scroll_frame_intervals.len(),
            "edit_frames": bench.edit_frame_intervals.len(),
            "buffer_lines": self.buffer.line_count(),
            "decorations": self.decorations.len(),
            "lazy_markdown": self.uses_lazy_markdown_decorations(),
            "line_display_slots": self.line_display_info.len(),
            "line_display_cached": self.line_display_info.iter().filter(|line| line.is_some()).count(),
            "scroll_frame_p50_ms": percentile(&bench.scroll_frame_intervals, 50),
            "scroll_frame_p95_ms": percentile(&bench.scroll_frame_intervals, 95),
            "scroll_frame_p99_ms": percentile(&bench.scroll_frame_intervals, 99),
            "edit_frame_p50_ms": percentile(&bench.edit_frame_intervals, 50),
            "edit_frame_p95_ms": percentile(&bench.edit_frame_intervals, 95),
            "edit_frame_p99_ms": percentile(&bench.edit_frame_intervals, 99),
            "frame_work_p50_ms": percentile(&bench.frame_work_times, 50),
            "frame_work_p95_ms": percentile(&bench.frame_work_times, 95),
            "frame_work_p99_ms": percentile(&bench.frame_work_times, 99),
            "edit_operation_p50_ms": percentile(&bench.edit_times, 50),
            "edit_operation_p95_ms": percentile(&bench.edit_times, 95),
            "edit_operation_p99_ms": percentile(&bench.edit_times, 99),
            "character_edit_p95_ms": percentile(&bench.character_edit_times, 95),
            "newline_edit_p95_ms": percentile(&bench.newline_edit_times, 95),
            "newline_buffer_p95_ms": percentile(&bench.newline_buffer_times, 95),
            "newline_reparse_p95_ms": percentile(&bench.newline_reparse_times, 95),
            "newline_history_p95_ms": percentile(&bench.newline_history_times, 95),
            "selection_update_p95_ms": percentile(&bench.selection_times, 95),
            "scrollbar_jump_p95_ms": percentile(&bench.scrollbar_jump_times, 95),
            "history_operation_p95_ms": percentile(&bench.history_times, 95),
            "syntax_view_samples": bench.syntax_view_samples,
            "syntax_view_misses": bench.syntax_view_misses,
        });
        if let Some(parent) = bench.output_path.parent() {
            std::fs::create_dir_all(parent).ok();
        }
        if let Ok(file) = std::fs::File::create(&bench.output_path) {
            serde_json::to_writer_pretty(file, &output).ok();
        }
        Some(false)
    }

    fn trim_line_display_info_cache_for_large_document(&mut self) {
        let line_count = self.line_display_info.len();
        if line_count < LINE_DISPLAY_CACHE_TRIM_MIN_LINES {
            return;
        }

        let cached_count = self
            .line_display_info
            .iter()
            .filter(|info| info.is_some())
            .count();
        if cached_count <= LINE_DISPLAY_CACHE_MAX_CACHED_LINES {
            return;
        }

        let viewport_start = self
            .line_at_y((self.scroll_offset - self.padding_top()).max(0.0))
            .min(line_count.saturating_sub(1));
        let viewport_end = self
            .line_at_y(self.scroll_offset + self.editor_visible_height() + self.padding_bottom())
            .min(line_count.saturating_sub(1));
        let visible_start = viewport_start.saturating_sub(LINE_DISPLAY_CACHE_VISIBLE_MARGIN_LINES);
        let visible_end = viewport_end
            .saturating_add(LINE_DISPLAY_CACHE_VISIBLE_MARGIN_LINES)
            .min(line_count.saturating_sub(1));
        let cursor_line = self.cursor_line().min(line_count.saturating_sub(1));
        let cursor_start = cursor_line.saturating_sub(LINE_DISPLAY_CACHE_CURSOR_MARGIN_LINES);
        let cursor_end = cursor_line
            .saturating_add(LINE_DISPLAY_CACHE_CURSOR_MARGIN_LINES)
            .min(line_count.saturating_sub(1));

        let mut pruned = false;
        for (line_idx, info) in self.line_display_info.iter_mut().enumerate() {
            let keep_visible = visible_start <= line_idx && line_idx <= visible_end;
            let keep_cursor = cursor_start <= line_idx && line_idx <= cursor_end;
            if !keep_visible && !keep_cursor && info.take().is_some() {
                pruned = true;
            }
        }

        if pruned {
            self.invalidate_line_geometry_cache();
        }
    }

    fn maybe_trim_line_display_info_cache_after_update(&mut self) {
        if self.line_display_info.len() < LINE_DISPLAY_CACHE_TRIM_MIN_LINES {
            return;
        }

        let next = self.line_display_trim_update_counter.get().wrapping_add(1);
        self.line_display_trim_update_counter.set(next);
        if next % LINE_DISPLAY_CACHE_TRIM_UPDATE_INTERVAL == 0 {
            self.trim_line_display_info_cache_for_large_document();
        }
    }

    fn queue_non_markdown_line_display_transfer(
        &mut self,
        replaced_range: &Range<usize>,
        inserted_text: &str,
    ) {
        self.pending_line_display_transfer = None;
        let Some(file_kind) = self.active_file_kind().cloned() else {
            return;
        };
        if !file_kind.supports_text_buffer() || self.line_display_info.is_empty() {
            return;
        }

        if !file_kind.is_markdown()
            && edit_may_change_leading_indent(&self.buffer, replaced_range, inserted_text)
        {
            self.invalidate_indent_guide_step_cache();
        }

        let (start_line, old_end_line_exclusive, new_end_line_exclusive) =
            line_display_transfer_line_bounds(&self.buffer, replaced_range, inserted_text);
        let markdown_plain_decoration_transfer = file_kind.is_markdown()
            && self.can_transfer_markdown_plain_decorations(
                inserted_text,
                start_line,
                old_end_line_exclusive,
            );
        let dirty_range = if file_kind.is_markdown() {
            let Some(dirty_range) = self.markdown_line_display_transfer_dirty_range(
                replaced_range,
                inserted_text,
                start_line,
                old_end_line_exclusive,
                new_end_line_exclusive,
            ) else {
                return;
            };
            dirty_range
        } else {
            start_line..new_end_line_exclusive
        };

        self.pending_line_display_transfer = Some(PendingLineDisplayTransfer {
            cached_lines: self.take_line_display_info(),
            start_line,
            old_end_line_exclusive,
            new_end_line_exclusive,
            dirty_start_line: dirty_range.start,
            dirty_end_line_exclusive: dirty_range.end,
            markdown_plain_decoration_transfer,
            byte_delta: inserted_text.len() as isize
                - (replaced_range.end.saturating_sub(replaced_range.start)) as isize,
        });
    }

    /// Attempts to update `self.decorations` in place for a markdown edit by
    /// re-parsing only the affected block window, instead of re-parsing the
    /// whole document. Returns `true` on success (decorations updated); `false`
    /// means the caller must fall back to a full `parse_decorations`.
    ///
    /// Correctness relies on the spliced window producing decorations identical
    /// to a full parse. To guarantee that we (1) grow the window to blank-line
    /// block boundaries so inline spans can't be cut mid-block, (2) bail when
    /// the window would start inside a multi-line code block, and (3) bail on
    /// constructs whose parse depends on out-of-window content (reference
    /// links/footnotes, block math, front matter). Per-line ranges are stored
    /// line-relative, so only absolute line numbers and the suffix
    /// `block_preview` byte ranges need shifting (see
    /// `offset_decoration_positions`).
    fn try_incremental_markdown_decorations(
        &mut self,
        transfer: &PendingLineDisplayTransfer,
    ) -> Result<(), &'static str> {
        if transfer.markdown_plain_decoration_transfer {
            return Err("plain_transfer");
        }
        // Move the old decorations out so the splice reuses their allocation
        // instead of cloning ~100k lines. On failure the old decorations are
        // discarded, but the caller immediately runs a full parse that rebuilds
        // them, so nothing is lost.
        let old_decorations = std::mem::take(&mut self.decorations);
        match compute_incremental_markdown_decorations(
            old_decorations,
            &self.buffer,
            transfer.new_end_line_exclusive,
            transfer.old_end_line_exclusive,
            transfer.dirty_start_line,
            transfer.dirty_end_line_exclusive,
            transfer.byte_delta,
        ) {
            Ok(new_decorations) => {
                self.decorations = new_decorations;
                if self.decorations.is_empty() {
                    self.decorations.push(LineDecoration::default());
                }
                self.invalidate_line_geometry_cache();
                Ok(())
            }
            Err(reason) => Err(reason),
        }
    }

    fn can_transfer_markdown_plain_decorations(
        &self,
        inserted_text: &str,
        start_line: usize,
        old_end_line_exclusive: usize,
    ) -> bool {
        if inserted_text.contains('\n') || old_end_line_exclusive != start_line.saturating_add(1) {
            return false;
        }

        if inserted_text.chars().any(|ch| {
            matches!(
                ch,
                '#' | '*'
                    | '_'
                    | '`'
                    | '~'
                    | '['
                    | ']'
                    | '('
                    | ')'
                    | '!'
                    | '<'
                    | '>'
                    | '|'
                    | ':'
                    | '-'
                    | '+'
                    | '='
                    | '\\'
                    | '$'
            )
        }) {
            return false;
        }

        // An ordered-list marker (`N. ` / `N) `) is built only from digits, a
        // `.`/`)` and spaces, none of which appear in the blocklist above (unlike
        // unordered markers, which trip the `-`/`*`/`+` entries). Without this
        // check, typing an ordered marker on a plain line would be treated as a
        // plain-text edit and never re-detected as a list. Transfers are queued
        // before the buffer mutation, and `ordered_list_prefix_len("1.")` is
        // enough to catch the common "type a space after the marker" case.
        let line_before_edit = self.buffer.line_content(start_line);
        let trimmed = line_before_edit.trim_start_matches([' ', '\t']);
        if crate::markdown::ordered_list_prefix_len(trimmed) > 0 {
            return false;
        }

        // A bare `-`/`*`/`+` stays plain text until its separating space arrives,
        // and a space trips none of the blocklist above, so the marker line has to
        // be re-parsed explicitly for the list to appear as soon as it is typed.
        if matches!(trimmed, "-" | "*" | "+") {
            return false;
        }

        if markdown_plain_edit_can_change_inline_delimiter_flanking(inserted_text, line_before_edit)
        {
            return false;
        }

        self.decorations
            .get(start_line)
            .is_some_and(markdown_decoration_is_plain_text)
    }

    fn markdown_line_display_transfer_dirty_range(
        &self,
        replaced_range: &Range<usize>,
        inserted_text: &str,
        start_line: usize,
        old_end_line_exclusive: usize,
        new_end_line_exclusive: usize,
    ) -> Option<Range<usize>> {
        const MARKDOWN_DIRTY_CONTEXT_LINES: usize = 8;

        let line_count = self.buffer.line_count();
        let touched_text = self.buffer.text().get(replaced_range.clone()).unwrap_or("");
        let changed_text_can_toggle_fence = inserted_text.contains("```")
            || inserted_text.contains("~~~")
            || touched_text.contains("```")
            || touched_text.contains("~~~")
            || (inserted_text.contains('`')
                && self.markdown_context_contains(start_line, old_end_line_exclusive, "``"))
            || (inserted_text.contains('~')
                && self.markdown_context_contains(start_line, old_end_line_exclusive, "~~"));
        if changed_text_can_toggle_fence {
            return None;
        }

        // Constructs whose effect can reach lines far outside the edited region
        // (reference-style link/image definitions and footnote definitions) must
        // force a full re-layout, because preserving cached shaping for distant
        // usage lines would leave them stale.
        let has_non_local_effect =
            |text: &str| text.contains("]:") || text.contains("[^") || text.contains("][");
        if has_non_local_effect(inserted_text) || has_non_local_effect(touched_text) {
            return None;
        }

        let mut dirty_start_old = start_line.saturating_sub(MARKDOWN_DIRTY_CONTEXT_LINES);
        let mut dirty_end_old = old_end_line_exclusive
            .saturating_add(MARKDOWN_DIRTY_CONTEXT_LINES)
            .min(line_count);
        let mut expanded = true;
        while expanded {
            expanded = false;
            let scan_start = dirty_start_old.min(line_count);
            let scan_end = dirty_end_old.min(line_count);
            for line_idx in scan_start..scan_end {
                let Some(decoration) = self.decorations.get(line_idx) else {
                    continue;
                };

                if matches!(decoration.block_type, BlockType::CodeBlockFence) {
                    let mut group_start = line_idx;
                    while group_start > 0
                        && matches!(
                            self.decorations
                                .get(group_start - 1)
                                .map(|decoration| &decoration.block_type),
                            Some(BlockType::CodeBlock | BlockType::CodeBlockFence)
                        )
                    {
                        group_start -= 1;
                    }

                    let mut group_end = line_idx.saturating_add(1);
                    while group_end < line_count
                        && matches!(
                            self.decorations
                                .get(group_end)
                                .map(|decoration| &decoration.block_type),
                            Some(BlockType::CodeBlock | BlockType::CodeBlockFence)
                        )
                    {
                        group_end += 1;
                    }

                    if group_start < dirty_start_old {
                        dirty_start_old = group_start;
                        expanded = true;
                    }
                    if group_end > dirty_end_old {
                        dirty_end_old = group_end;
                        expanded = true;
                    }
                }

                if let Some(table_info) = decoration.table_info.as_ref()
                    && !table_info.is_front_matter
                {
                    if table_info.table_start_line < dirty_start_old {
                        dirty_start_old = table_info.table_start_line;
                        expanded = true;
                    }
                    if table_info.table_end_line > dirty_end_old {
                        dirty_end_old = table_info.table_end_line;
                        expanded = true;
                    }
                }

                if let Some(preview) = decoration.block_preview.as_ref() {
                    if preview.start_line < dirty_start_old {
                        dirty_start_old = preview.start_line;
                        expanded = true;
                    }
                    let preview_end = preview.end_line.saturating_add(1);
                    if preview_end > dirty_end_old {
                        dirty_end_old = preview_end;
                        expanded = true;
                    }
                }
            }
        }

        let dirty_start = shift_line_after_transfer(
            dirty_start_old,
            start_line,
            old_end_line_exclusive,
            new_end_line_exclusive,
        );
        let dirty_end = shift_line_after_transfer(
            dirty_end_old,
            start_line,
            old_end_line_exclusive,
            new_end_line_exclusive,
        )
        .max(new_end_line_exclusive);

        Some(dirty_start..dirty_end)
    }

    fn markdown_context_contains(
        &self,
        start_line: usize,
        old_end_line_exclusive: usize,
        needle: &str,
    ) -> bool {
        let line_count = self.buffer.line_count();
        let scan_start = start_line.saturating_sub(1);
        let scan_end = old_end_line_exclusive.saturating_add(1).min(line_count);
        (scan_start..scan_end).any(|line_idx| self.buffer.line_content(line_idx).contains(needle))
    }

    fn find_anchor_line_in_current_buffer(&self, anchor: &str) -> Option<usize> {
        let anchor = anchor.trim();
        if anchor.is_empty() {
            return None;
        }

        self.decorations
            .iter()
            .enumerate()
            .find_map(|(line_idx, decoration)| {
                matches!(decoration.block_type, BlockType::Heading(_))
                    .then(|| self.buffer.line_content(line_idx))
                    .filter(|line| heading_matches_anchor(line, anchor))
                    .map(|_| line_idx)
            })
    }

    pub(crate) fn find_footnote_definition_line(&self, label: &str) -> Option<usize> {
        self.decorations
            .iter()
            .position(|decoration| decoration.footnote_definition_label.as_deref() == Some(label))
    }

    fn reveal_line_in_current_buffer(&mut self, target_line: usize, cx: &mut Context<Self>) {
        self.scroll_to_line(target_line, cx);
        self.flash_outline_heading_line(target_line, cx);
        cx.notify();
    }

    fn resolve_markdown_navigation_target(&self, target: &str) -> Option<MarkdownNavigationTarget> {
        let trimmed = target.trim();
        if trimmed.is_empty() {
            return None;
        }

        if trimmed.starts_with("http://")
            || trimmed.starts_with("https://")
            || trimmed.starts_with("mailto:")
        {
            return Some(MarkdownNavigationTarget::ExternalUrl(trimmed.to_string()));
        }

        let (path_part, anchor) = split_markdown_target(trimmed);
        if path_part.trim().is_empty() {
            return anchor
                .filter(|anchor| !anchor.is_empty())
                .map(|anchor| MarkdownNavigationTarget::CurrentAnchor(anchor.to_string()));
        }

        resolve_markdown_target_path(
            &self.sidebar_root,
            self.current_file_path.as_deref(),
            trimmed,
        )
        .map(|path| MarkdownNavigationTarget::File {
            path,
            anchor: anchor
                .filter(|anchor| !anchor.is_empty())
                .map(str::to_string),
        })
    }

    pub(crate) fn markdown_target_is_navigable(&self, target: &str) -> bool {
        match self.resolve_markdown_navigation_target(target) {
            Some(MarkdownNavigationTarget::ExternalUrl(_)) => true,
            Some(MarkdownNavigationTarget::CurrentAnchor(anchor)) => {
                self.find_anchor_line_in_current_buffer(&anchor).is_some()
            }
            Some(MarkdownNavigationTarget::File { .. }) => true,
            None => false,
        }
    }

    pub(crate) fn navigate_to_footnote_label(
        &mut self,
        label: &str,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(target_line) = self.find_footnote_definition_line(label) else {
            return false;
        };
        self.reveal_line_in_current_buffer(target_line, cx);
        true
    }

    pub(crate) fn navigate_markdown_target(
        &mut self,
        target: &str,
        focus_editor: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(target) = self.resolve_markdown_navigation_target(target) else {
            return false;
        };

        match target {
            MarkdownNavigationTarget::ExternalUrl(url) => {
                if let Err(error) = crate::platform::shell::open_url(&url) {
                    log_error!("failed to open URL: {error}");
                    return false;
                }
                true
            }
            MarkdownNavigationTarget::CurrentAnchor(anchor) => {
                let Some(target_line) = self.find_anchor_line_in_current_buffer(&anchor) else {
                    return false;
                };
                self.reveal_line_in_current_buffer(target_line, cx);
                true
            }
            MarkdownNavigationTarget::File { path, anchor } => {
                self.open_path_in_permanent_tab(path, focus_editor, cx);
                if let Some(anchor) = anchor
                    && let Some(target_line) = self.find_anchor_line_in_current_buffer(&anchor)
                {
                    self.reveal_line_in_current_buffer(target_line, cx);
                }
                true
            }
        }
    }
}

fn edit_may_change_leading_indent(
    buffer: &Buffer,
    replaced_range: &Range<usize>,
    inserted_text: &str,
) -> bool {
    if inserted_text.contains('\n') {
        return true;
    }

    let start = replaced_range.start.min(buffer.len());
    let end = replaced_range.end.min(buffer.len());
    let (start_line, start_column) = buffer.offset_to_line_col(start);
    let (end_line, _) = buffer.offset_to_line_col(end);
    if start_line != end_line {
        return true;
    }

    let leading_bytes = buffer
        .line_content(start_line)
        .bytes()
        .take_while(|byte| matches!(byte, b' ' | b'\t'))
        .count();
    start_column < leading_bytes
        || (start_column == leading_bytes
            && inserted_text
                .as_bytes()
                .first()
                .is_some_and(|byte| matches!(byte, b' ' | b'\t')))
}

pub(crate) fn default_document_text(ui_language: UiLanguage) -> String {
    let help_path = help_document_path(ui_language);
    crate::resolve_runtime_path(help_path)
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_else(|| bundled_help_document(ui_language).to_string())
}

fn help_document_path(ui_language: UiLanguage) -> &'static str {
    match ui_language {
        UiLanguage::English => "docs/help-en.md",
        UiLanguage::ChineseSimplified => "docs/help-zh-cn.md",
        UiLanguage::ChineseTraditional => "docs/help-zh-tw.md",
        UiLanguage::Japanese => "docs/help-ja.md",
        UiLanguage::Korean => "docs/help-ko.md",
        UiLanguage::French => "docs/help-fr.md",
        UiLanguage::German => "docs/help-de.md",
        UiLanguage::Spanish => "docs/help-es.md",
        UiLanguage::Portuguese => "docs/help-pt.md",
        UiLanguage::Russian => "docs/help-ru.md",
        UiLanguage::Arabic => "docs/help-ar.md",
        UiLanguage::Hindi => "docs/help-hi.md",
    }
}

fn bundled_help_document(ui_language: UiLanguage) -> &'static str {
    match ui_language {
        UiLanguage::English => include_str!("../../docs/help-en.md"),
        UiLanguage::ChineseSimplified => include_str!("../../docs/help-zh-cn.md"),
        UiLanguage::ChineseTraditional => include_str!("../../docs/help-zh-tw.md"),
        UiLanguage::Japanese => include_str!("../../docs/help-ja.md"),
        UiLanguage::Korean => include_str!("../../docs/help-ko.md"),
        UiLanguage::French => include_str!("../../docs/help-fr.md"),
        UiLanguage::German => include_str!("../../docs/help-de.md"),
        UiLanguage::Spanish => include_str!("../../docs/help-es.md"),
        UiLanguage::Portuguese => include_str!("../../docs/help-pt.md"),
        UiLanguage::Russian => include_str!("../../docs/help-ru.md"),
        UiLanguage::Arabic => include_str!("../../docs/help-ar.md"),
        UiLanguage::Hindi => include_str!("../../docs/help-hi.md"),
    }
}

pub(super) fn builtin_welcome_tab(ui_language: UiLanguage) -> OpenTab {
    let initial_text = default_document_text(ui_language);
    let first_line_end = initial_text.find('\n').unwrap_or(initial_text.len());

    OpenTab {
        id: WELCOME_TAB_ID.to_string(),
        title: crate::i18n::translate(ui_language, TextKey::WelcomeTitle).to_string(),
        kind: TabKind::Editor,
        path: None,
        file_kind: Some(FileKind::Markdown),
        editor_left_aligned: false,
        snapshot: EditorSnapshot {
            text: initial_text,
            cursor_offset: first_line_end,
            cursor_anchor: first_line_end,
            scroll_offset: 0.0,
            extra_cursors: Vec::new(),
        },
        image_view: ImageViewState::default(),
        whiteboard: None,
        git_diff: None,
        inline_diff: None,
        is_preview: false,
        is_dirty: false,
        is_deleted: false,
        is_pinned: false,
        recovery: config_store::TabRecoveryState::default(),
        history: history_store::TabHistory::new(
            Some(history_store::default_source_identity(WELCOME_TAB_ID, None)),
            Some(history_store::default_journal_path(WELCOME_TAB_ID, None)),
        ),
    }
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

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{
        EditorView, SparseLineGeometry, classify_file_kind_for_bytes,
        compute_incremental_markdown_decorations, edit_may_change_leading_indent,
        generated_preview_intersects_range, heading_matches_anchor,
        line_display_transfer_line_bounds, normalize_ordered_list_numbers_in_buffer,
        resolve_markdown_target_path, split_external_paths, split_markdown_target,
        text_tab_is_dirty, unique_import_path,
    };
    use crate::buffer::Buffer;
    use crate::markdown::parse_decorations;

    #[test]
    fn cursor_blink_switches_only_between_opacity_endpoints() {
        assert_eq!(EditorView::next_cursor_blink_opacity(1.0, 0.0, 1.0), 0.0);
        assert_eq!(EditorView::next_cursor_blink_opacity(0.0, 0.0, 1.0), 1.0);
    }

    #[test]
    fn fs_rename_events_preserve_old_and_new_paths() {
        use notify::EventKind;
        use notify::event::{ModifyKind, RenameMode};

        let both = notify::Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
            .add_path("/workspace/old.md".into())
            .add_path("/workspace/new.md".into());
        assert_eq!(
            EditorView::fs_rename_mappings(&[both]),
            vec![("/workspace/old.md".into(), "/workspace/new.md".into())]
        );

        let from = notify::Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::From)))
            .add_path("/workspace/old-dir".into())
            .set_tracker(7);
        let to = notify::Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::To)))
            .add_path("/workspace/new-dir".into())
            .set_tracker(7);
        assert_eq!(
            EditorView::fs_rename_mappings(&[from, to]),
            vec![("/workspace/old-dir".into(), "/workspace/new-dir".into())]
        );
    }

    #[test]
    fn markdown_extensions_include_mdx() {
        for path in ["note.md", "note.markdown", "note.mdx", "NOTE.MDX"] {
            assert_eq!(
                classify_file_kind_for_bytes(Path::new(path), b"# document"),
                super::FileKind::Markdown,
                "{path}",
            );
        }
    }

    #[test]
    fn external_drop_splits_markdown_files_from_attachments() {
        let paths = [
            "note.md",
            "guide.MARKDOWN",
            "page.mdx",
            "image.png",
            "data.pdf",
        ]
        .map(PathBuf::from);
        let (markdown, attachments) = split_external_paths(&paths);
        assert_eq!(
            markdown,
            ["note.md", "guide.MARKDOWN", "page.mdx"].map(PathBuf::from)
        );
        assert_eq!(attachments, ["image.png", "data.pdf"].map(PathBuf::from));
    }

    #[test]
    fn generated_previews_render_only_when_they_intersect_the_viewport() {
        assert!(generated_preview_intersects_range(10, 14, 12, 20));
        assert!(generated_preview_intersects_range(12, 20, 10, 14));
        assert!(generated_preview_intersects_range(10, 20, 10, 20));
        assert!(!generated_preview_intersects_range(1, 9, 10, 20));
        assert!(!generated_preview_intersects_range(21, 30, 10, 20));
    }

    #[test]
    fn indent_guide_step_cache_only_invalidates_for_leading_whitespace_edits() {
        let buffer = Buffer::from_text("fn main() {\n    println!(\"ok\");\n}\n");
        let body_offset = buffer.line_start(1) + 8;
        assert!(!edit_may_change_leading_indent(
            &buffer,
            &(body_offset..body_offset + 1),
            "x",
        ));

        let indent_end = buffer.line_start(1) + 4;
        assert!(edit_may_change_leading_indent(
            &buffer,
            &(buffer.line_start(1)..indent_end),
            "  ",
        ));
        assert!(edit_may_change_leading_indent(
            &buffer,
            &(indent_end..indent_end),
            "\t",
        ));
        assert!(edit_may_change_leading_indent(
            &buffer,
            &(body_offset..body_offset),
            "\n",
        ));
        assert!(edit_may_change_leading_indent(
            &buffer,
            &(buffer.line_end(0)..buffer.line_start(2)),
            "",
        ));
    }

    #[test]
    fn local_history_transaction_records_multiple_buffer_edits() {
        let mut buffer = Buffer::from_text("alpha\nbeta\n");
        buffer.begin_edit_recording();
        buffer.insert(0, "X");
        buffer.replace(7..11, "B");
        let edits = buffer.finish_edit_recording();

        let before = super::EditorSnapshot {
            text: "alpha\nbeta\n".to_string(),
            cursor_offset: 0,
            cursor_anchor: 0,
            scroll_offset: 0.0,
            extra_cursors: Vec::new(),
        };
        let after = super::EditorSnapshot {
            text: buffer.text().to_string(),
            cursor_offset: buffer.len(),
            cursor_anchor: buffer.len(),
            scroll_offset: 10.0,
            extra_cursors: Vec::new(),
        };
        let mut history = super::history_store::TabHistory::default();
        history.push_undo_transaction(
            edits,
            super::history_store::SnapshotViewState::from(&before),
            super::history_store::SnapshotViewState::from(&after),
            false,
        );
        assert_eq!(history.take_undo(&after), Some(before.clone()));
        assert_eq!(history.take_redo(&before), Some(after));
    }

    #[test]
    fn sparse_line_geometry_roundtrips_measured_heights_across_blocks() {
        let mut geometry = SparseLineGeometry::default();
        geometry.reset(2_000_001, 20.0);
        geometry.set_measured_height(10, 40.0);
        geometry.set_measured_height(300, 10.0);
        geometry.set_measured_height(1_999_999, 60.0);

        assert_eq!(geometry.y_offset(10, 8.0), 208.0);
        assert_eq!(geometry.y_offset(11, 8.0), 248.0);
        assert_eq!(geometry.y_offset(301, 8.0), 6_038.0);
        assert_eq!(geometry.line_at_y(6_037.9, 8.0), 300);
        assert_eq!(geometry.line_at_y(6_038.0, 8.0), 301);

        let last_line_y = geometry.y_offset(1_999_999, 8.0);
        assert_eq!(geometry.line_at_y(last_line_y, 8.0), 1_999_999);
        assert_eq!(geometry.line_at_y(last_line_y + 59.9, 8.0), 1_999_999);

        geometry.clear_range(300..301);
        assert_eq!(geometry.y_offset(301, 8.0), 6_048.0);
    }

    #[test]
    fn sparse_line_geometry_splices_measured_heights_around_structural_edits() {
        let mut geometry = SparseLineGeometry::default();
        geometry.reset(8, 20.0);
        geometry.set_measured_height(1, 30.0);
        geometry.set_measured_height(3, 40.0);
        geometry.set_measured_height(6, 50.0);

        geometry.splice_lines(3, 4, 5, 3..5, 9, 20.0);

        assert_eq!(geometry.measured_deltas.get(&1), Some(&10.0));
        assert!(!geometry.measured_deltas.contains_key(&3));
        assert_eq!(geometry.measured_deltas.get(&7), Some(&30.0));
        assert_eq!(geometry.y_offset(8, 0.0), 200.0);

        geometry.splice_lines(3, 5, 4, 3..4, 8, 20.0);
        assert_eq!(geometry.measured_deltas.get(&1), Some(&10.0));
        assert_eq!(geometry.measured_deltas.get(&6), Some(&30.0));
        assert_eq!(geometry.y_offset(7, 0.0), 180.0);

        let mut uninitialized = SparseLineGeometry::default();
        uninitialized.splice_lines(0, 1, 2, 0..2, 9, 20.0);
        assert_eq!(uninitialized.line_count, 9);
        assert_eq!(uninitialized.default_height, 20.0);
        assert!(uninitialized.measured_deltas.is_empty());
    }

    /// Drives `compute_incremental_markdown_decorations` for an `old -> new`
    /// edit derived from the byte-level diff of the two texts, then asserts the
    /// incremental result is byte-for-byte identical (via `Debug`) to a full
    /// `parse_decorations` of the new text. Returns `true` when the incremental
    /// path was taken (it may legitimately bail, in which case the test only
    /// checks that the caller would fall back to a full parse).
    fn assert_incremental_matches_full(old_text: &str, new_text: &str) -> bool {
        let prefix = old_text
            .bytes()
            .zip(new_text.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        let max_suffix = old_text.len().min(new_text.len()) - prefix;
        let suffix = old_text
            .bytes()
            .rev()
            .zip(new_text.bytes().rev())
            .take_while(|(a, b)| a == b)
            .take(max_suffix)
            .count();

        let replaced_range = prefix..(old_text.len() - suffix);
        let inserted_text = &new_text[prefix..(new_text.len() - suffix)];

        let old_buffer = Buffer::from_text(old_text);
        let (start_line, old_end_line_exclusive, new_end_line_exclusive) =
            line_display_transfer_line_bounds(&old_buffer, &replaced_range, inserted_text);

        let new_buffer = Buffer::from_text(new_text);
        let old_decorations = parse_decorations(old_text);
        let byte_delta = new_text.len() as isize - old_text.len() as isize;

        let incremental = compute_incremental_markdown_decorations(
            old_decorations,
            &new_buffer,
            new_end_line_exclusive,
            old_end_line_exclusive,
            start_line,
            new_end_line_exclusive,
            byte_delta,
        );

        match incremental {
            Ok(result) => {
                let full = parse_decorations(new_text);
                assert_eq!(
                    format!("{result:#?}"),
                    format!("{full:#?}"),
                    "incremental decorations diverged from full parse for edit\nOLD:\n{old_text}\nNEW:\n{new_text}"
                );
                true
            }
            Err(_) => false,
        }
    }

    #[test]
    fn incremental_markdown_matches_full_for_inline_edit() {
        assert!(assert_incremental_matches_full(
            "# Title\n\npara one\npara two\n\nmore\n",
            "# Title\n\npara **one**\npara two\n\nmore\n",
        ));
    }

    #[test]
    fn markdown_plain_transfer_rejects_edits_on_lines_with_inline_delimiters() {
        assert!(
            super::markdown_plain_edit_can_change_inline_delimiter_flanking(" ", "**生态改进：**")
        );
        assert!(super::markdown_plain_edit_can_change_inline_delimiter_flanking("内容", "~~~~"));
        assert!(super::markdown_plain_edit_can_change_inline_delimiter_flanking("text", "**"));
        assert!(super::markdown_plain_edit_can_change_inline_delimiter_flanking(" ", "*重点：*"));
        assert!(super::markdown_plain_edit_can_change_inline_delimiter_flanking(" ", "~~删除：~~"));
        assert!(
            super::markdown_plain_edit_can_change_inline_delimiter_flanking(
                "高",
                "**生态改进：** "
            )
        );
        assert!(!super::markdown_plain_edit_can_change_inline_delimiter_flanking(" ", "普通文本"));
    }

    #[test]
    fn incremental_markdown_matches_full_for_heading_edit() {
        assert!(assert_incremental_matches_full(
            "intro\n\n## Section\n\nbody text here\n\noutro\n",
            "intro\n\n### Section\n\nbody text here\n\noutro\n",
        ));
    }

    #[test]
    fn incremental_markdown_matches_full_for_newline_insertion() {
        assert!(assert_incremental_matches_full(
            "alpha\n\nfirst line\n\nbeta\n\ngamma\n",
            "alpha\n\nfirst\nline\n\nbeta\n\ngamma\n",
        ));
    }

    #[test]
    fn incremental_markdown_matches_full_for_line_deletion() {
        assert!(assert_incremental_matches_full(
            "alpha\n\none\ntwo\nthree\n\nbeta\n",
            "alpha\n\none\nthree\n\nbeta\n",
        ));
    }

    #[test]
    fn incremental_markdown_matches_full_for_list_edit() {
        assert!(assert_incremental_matches_full(
            "intro\n\n- one\n- two\n- three\n\nend\n",
            "intro\n\n- one\n- two two\n- three\n\nend\n",
        ));
    }

    #[test]
    fn incremental_markdown_matches_full_for_reference_link_with_external_def() {
        // A reference-style link usage in the edited window whose definition
        // lives far above (outside the window) must still resolve to a link.
        // The prepend-definitions logic makes the isolated window parse match
        // the full-document parse.
        assert!(assert_incremental_matches_full(
            "[site]: https://example.com\n\nintro\n\nsee [the site][site] now\n\nmore text\n\nend\n",
            "[site]: https://example.com\n\nintro\n\nsee [the site][site] today\n\nmore text\n\nend\n",
        ));
    }

    #[test]
    fn incremental_markdown_matches_full_for_footnote_with_external_def() {
        // A footnote reference in the edited window whose definition lives
        // elsewhere must still resolve as a footnote.
        assert!(assert_incremental_matches_full(
            "intro[^a]\n\npara one\n\npara two\n\n[^a]: the footnote text\n",
            "intro[^a]\n\npara ONE\n\npara two\n\n[^a]: the footnote text\n",
        ));
    }

    #[test]
    fn incremental_markdown_matches_full_for_code_block_edit() {
        // Editing a line inside a fence whose blank-line block boundary lies
        // outside the fence: the window grows to include the whole fence, so
        // incremental parsing is valid and must match the full parse.
        assert!(assert_incremental_matches_full(
            "intro\n\n```rust\nlet a = 1;\nlet b = 2;\n```\n\nend\n",
            "intro\n\n```rust\nlet a = 1;\nlet b = 22;\n```\n\nend\n",
        ));
    }

    #[test]
    fn incremental_markdown_falls_back_inside_code_block_with_inner_blank() {
        // A fenced code block that contains a blank line: editing a line below
        // that inner blank means the blank-line block-boundary walk would stop
        // *inside* the fence. The function must bail (return None) so the caller
        // performs a full parse.
        let old_text = "intro\n\n```text\nline a\n\nline b\nline c\n```\n\nend\n";
        let new_text = "intro\n\n```text\nline a\n\nline b\nline cc\n```\n\nend\n";
        let new_buffer = Buffer::from_text(new_text);
        let old_decorations = parse_decorations(old_text);
        // Edited line index 6 ("line c"), single-line replacement, +1 byte.
        let result =
            compute_incremental_markdown_decorations(old_decorations, &new_buffer, 7, 7, 6, 7, 1);
        assert!(
            result.is_err(),
            "expected fall back to full parse for edit below an inner-blank code-block line"
        );
    }

    #[test]
    fn text_tab_is_dirty_detects_saved_file_revert() {
        assert!(text_tab_is_dirty(Some("hello"), "hello world"));
        assert!(!text_tab_is_dirty(Some("hello"), "hello"));
    }

    #[test]
    fn text_tab_is_dirty_treats_empty_untitled_buffer_as_clean() {
        assert!(text_tab_is_dirty(None, "draft"));
        assert!(!text_tab_is_dirty(None, ""));
    }

    #[test]
    fn line_display_transfer_includes_merged_line_after_newline_delete() {
        let buffer = Buffer::from_text("alpha\nbeta\ngamma");
        let range = buffer.line_end(0)..buffer.line_start(1);

        assert_eq!(
            line_display_transfer_line_bounds(&buffer, &range, ""),
            (0, 2, 1)
        );
    }

    #[test]
    fn line_display_transfer_refreshes_shifted_neighbor_after_whole_line_delete() {
        let buffer = Buffer::from_text("alpha\nbeta\ngamma");
        let range = buffer.line_start(1)..buffer.line_start(2);

        assert_eq!(
            line_display_transfer_line_bounds(&buffer, &range, ""),
            (1, 3, 2)
        );
    }

    #[test]
    fn line_display_transfer_keeps_inline_edit_bounds_local() {
        let buffer = Buffer::from_text("alpha\nbeta\ngamma");
        let range = 1..3;

        assert_eq!(
            line_display_transfer_line_bounds(&buffer, &range, "LP"),
            (0, 1, 1)
        );
    }

    #[test]
    fn unique_import_path_appends_incrementing_suffix_on_conflict() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let directory = temp_dir.path();
        std::fs::write(directory.join("20260507153045.png"), b"one").expect("write first");
        std::fs::write(directory.join("20260507153045-1.png"), b"two").expect("write second");

        let next_path = unique_import_path(directory, "20260507153045", Some("png"));

        assert_eq!(
            next_path.file_name().and_then(|name| name.to_str()),
            Some("20260507153045-2.png")
        );
    }

    #[test]
    fn split_markdown_target_extracts_path_and_anchor() {
        assert_eq!(
            split_markdown_target("notes/file.md#intro"),
            ("notes/file.md", Some("intro"))
        );
        assert_eq!(split_markdown_target("#intro"), ("", Some("intro")));
        assert_eq!(
            split_markdown_target("notes/file.md"),
            ("notes/file.md", None)
        );
    }

    #[test]
    fn heading_matches_anchor_supports_slug_and_explicit_id() {
        assert!(heading_matches_anchor("## Hello World", "hello-world"));
        assert!(heading_matches_anchor("## 标题 {#custom-id}", "custom-id"));
    }

    #[test]
    fn resolve_markdown_target_path_strips_fragment_and_prefers_current_file_dir() {
        let temp_dir = tempfile::tempdir().expect("tempdir");
        let notes_dir = temp_dir.path().join("notes");
        std::fs::create_dir_all(&notes_dir).expect("mkdir");
        let current_file = notes_dir.join("current.md");
        let linked_file = notes_dir.join("other.md");
        std::fs::write(&current_file, b"# current").expect("write current");
        std::fs::write(&linked_file, b"# other").expect("write other");

        let resolved = resolve_markdown_target_path(
            temp_dir.path(),
            Some(current_file.as_path()),
            "other.md#intro",
        );

        assert_eq!(resolved.as_deref(), Some(linked_file.as_path()));
    }

    #[test]
    fn normalize_ordered_list_numbers_fixes_gap_after_deleted_item() {
        let mut buffer = Buffer::from_text("1. one\n3. three\n4. four\n");

        normalize_ordered_list_numbers_in_buffer(&mut buffer);

        assert_eq!(buffer.text(), "1. one\n2. three\n3. four\n");
    }

    #[test]
    fn normalize_ordered_list_numbers_keeps_outer_sequence_across_nested_content() {
        let mut buffer = Buffer::from_text("1. one\n   - child\n4. four\n");

        normalize_ordered_list_numbers_in_buffer(&mut buffer);

        assert_eq!(buffer.text(), "1. one\n   - child\n2. four\n");
    }
}
