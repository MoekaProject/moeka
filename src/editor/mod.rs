mod app_settings;
mod assets;
mod chat_panel;
mod config_paths;
mod config_store;
mod element;
mod fold;
mod handlers;
mod helpers;
mod history_store;
mod image_view;
mod info_panel;
mod input;
mod input_entity;
mod input_ime;
mod input_table;
mod input_wrap;
mod menus;
mod outline;
mod profile;
mod quick_open;
mod render;
mod scaling;
mod search;
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
mod tabs;
mod temp_store;
mod theme_debugger;
mod tooltip;
mod types;
mod workspace_search;

pub(crate) use app_settings::{AppSettings, theme_kind_to_string};
pub(crate) use assets::*;
pub(crate) use types::*;

use std::collections::{HashMap, HashSet};
use std::io::Cursor as IoCursor;
use std::ops::Range;
use std::path::{Path, PathBuf};
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

const CODE_BLOCK_COPY_FEEDBACK_MS: u64 = 1200;
const PANEL_ANIMATION_FRAME_MS: u64 = 8;

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

// ── Platform helpers ────────────────────────────────────────────────

#[cfg(target_os = "macos")]
pub(crate) fn perform_window_drag(window: &gpui::Window) {
    use objc::{msg_send, sel, sel_impl};
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    if let Ok(handle) = HasWindowHandle::window_handle(window) {
        if let RawWindowHandle::AppKit(appkit_handle) = handle.as_raw() {
            unsafe {
                let ns_view = appkit_handle.ns_view.as_ptr() as *mut objc::runtime::Object;
                let ns_window: *mut objc::runtime::Object = msg_send![ns_view, window];
                let current_event: *mut objc::runtime::Object = msg_send![ns_window, currentEvent];
                if !current_event.is_null() {
                    let () = msg_send![ns_window, performWindowDragWithEvent: current_event];
                }
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn perform_window_drag(_window: &gpui::Window) {}

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

fn markdown_reference_for_import_name(file_name: &str, is_image: bool) -> String {
    if is_image {
        format!("![]({file_name})")
    } else {
        format!("[{file_name}]({file_name})")
    }
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

fn decode_percent_escapes(text: &str) -> String {
    let mut decoded = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut index = 0usize;

    while index < bytes.len() {
        if bytes[index] == b'%' && index + 2 < bytes.len() {
            if let Ok(byte) = u8::from_str_radix(&text[index + 1..index + 3], 16) {
                decoded.push(byte as char);
                index += 3;
                continue;
            }
        }
        decoded.push(bytes[index] as char);
        index += 1;
    }

    decoded
}

fn parse_file_urls_from_text(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let path = line.strip_prefix("file://")?;
            let decoded = decode_percent_escapes(path);
            #[cfg(target_os = "windows")]
            let decoded = decoded.trim_start_matches('/').to_string();
            Some(PathBuf::from(decoded))
        })
        .collect()
}

#[cfg(target_os = "macos")]
fn macos_nsstring(text: &str) -> *mut objc::runtime::Object {
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let c_string = std::ffi::CString::new(text).expect("NSString input contains interior NUL");
        msg_send![class!(NSString), stringWithUTF8String: c_string.as_ptr()]
    }
}

#[cfg(target_os = "macos")]
fn macos_path_from_nsstring(value: *mut objc::runtime::Object) -> Option<PathBuf> {
    use objc::{msg_send, sel, sel_impl};

    if value.is_null() {
        return None;
    }

    unsafe {
        let utf8: *const std::os::raw::c_char = msg_send![value, UTF8String];
        if utf8.is_null() {
            None
        } else {
            Some(PathBuf::from(
                std::ffi::CStr::from_ptr(utf8)
                    .to_string_lossy()
                    .into_owned(),
            ))
        }
    }
}

#[cfg(target_os = "macos")]
fn macos_clipboard_file_paths() -> Vec<PathBuf> {
    use objc::{class, msg_send, sel, sel_impl};

    unsafe {
        let pasteboard: *mut objc::runtime::Object =
            msg_send![class!(NSPasteboard), generalPasteboard];
        if pasteboard.is_null() {
            return Vec::new();
        }

        let items: *mut objc::runtime::Object = msg_send![pasteboard, pasteboardItems];
        if items.is_null() {
            return Vec::new();
        }

        let count: usize = msg_send![items, count];
        let file_url_type = macos_nsstring("public.file-url");
        let mut paths = Vec::new();

        for index in 0..count {
            let item: *mut objc::runtime::Object = msg_send![items, objectAtIndex: index];
            if item.is_null() {
                continue;
            }

            let url_string: *mut objc::runtime::Object =
                msg_send![item, stringForType: file_url_type];
            if url_string.is_null() {
                continue;
            }

            let url: *mut objc::runtime::Object =
                msg_send![class!(NSURL), URLWithString: url_string];
            if url.is_null() {
                continue;
            }

            let path_string: *mut objc::runtime::Object = msg_send![url, path];
            if let Some(path) = macos_path_from_nsstring(path_string) {
                paths.push(path);
            }
        }

        paths
    }
}

#[cfg(not(target_os = "macos"))]
fn macos_clipboard_file_paths() -> Vec<PathBuf> {
    Vec::new()
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

#[derive(Clone)]
pub(crate) enum ContextMenuKind {
    Sidebar {
        path: PathBuf,
        is_dir: bool,
    },
    SidebarStarred {
        path: PathBuf,
    },
    SidebarSort,
    SidebarCreate,
    Format,
    FormatMore,
    WikiLink,
    Tab {
        tab_id: String,
    },
    Workspace,
    Table {
        table_start_line: usize,
        row: usize,
        col: usize,
    },
    Terminal,
    ChatTabCreate,
    ChatTab {
        tab_id: String,
    },
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
    Superscript,
    Subscript,
    Footnote,
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
                    .bg(gpui::rgb(0x2d2d2d))
                    .text_color(gpui::rgb(0xcccccc))
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

#[derive(Clone)]
pub(crate) struct WorkspaceSearchResult {
    pub path: PathBuf,
    pub matches: Vec<WorkspaceSearchMatch>,
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
    pub path: Option<PathBuf>,
    pub source_index: usize,
}

#[derive(Clone)]
pub(crate) struct DraggedChatTab {
    pub tab_id: String,
    pub title: String,
    pub source_index: usize,
}

pub(crate) struct DraggedTabView {
    pub title: String,
    pub font_size_px: f32,
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
                    .bg(gpui::rgb(0x2d2d2d))
                    .text_color(gpui::rgb(0xcccccc))
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
    CloseTabUnsaved { tab_id: String },
    SaveAndCloseTab { tab_id: String },
    DeletePath { path: PathBuf },
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
    pub wrap_boundary_upstream_xs: HashMap<usize, f32>,
    pub extra_height: f32,
    pub indent_px: f32,
    pub content_x: f32,
    pub selection_padding_left_px: f32,
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

// ── EditorView ──────────────────────────────────────────────────────

pub struct EditorView {
    pub buffer: Buffer,
    pub cursor: Cursor,
    pub(crate) extra_cursors: Vec<Cursor>,
    pub(crate) focus_handle: FocusHandle,
    pub(crate) theme_kind: ThemeKind,
    pub(crate) editor_theme_mode: EditorThemeMode,
    pub(crate) theme: Theme,
    pub(crate) styles: crate::style::Styles,
    pub(crate) decorations: Vec<LineDecoration>,
    pub(crate) scroll_offset: f32,
    pub(crate) is_selecting: bool,
    pub(crate) vertical_move_goal_x: Option<f32>,
    pub(crate) wrap_boundary_affinity: Option<WrapBoundaryAffinity>,
    pub(crate) line_display_info: Vec<Option<LineDisplayInfo>>,
    pub(crate) layout_dirty: bool,
    pub(crate) pending_scroll_to_cursor: bool,
    pub(crate) pending_preview_reveal: Option<PendingPreviewReveal>,
    pub(crate) last_viewport_height: f32,
    pub(crate) last_viewport_width: f32,
    // Cursor blink state
    pub(crate) cursor_opacity: f32,
    pub(crate) cursor_opacity_direction: f32,
    pub(crate) blink_epoch: usize,
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
    pub(crate) hovering_image_preview_line: Option<usize>,
    pub(crate) hovering_generated_preview_line: Option<usize>,
    pub(crate) hovering_generated_preview_zoom_line: Option<usize>,
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
    pub(crate) sidebar_root: PathBuf,
    pub(crate) expanded_dirs: HashSet<PathBuf>,
    /// 多选：所有选中的 sidebar 路径
    pub(crate) selected_sidebar_paths: HashSet<PathBuf>,
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
    pub(crate) last_stable_input_bounds: Option<Bounds<Pixels>>,
    pub(crate) pending_input_bounds: Option<Bounds<Pixels>>,
    pub(crate) preview_overlay: Option<PreviewOverlayState>,
    pub(crate) preview_overlay_bounds: Option<Bounds<Pixels>>,
    pub(crate) preview_overlay_image_bounds: Option<Bounds<Pixels>>,
    pub(crate) last_mouse_down_target: Option<(Point<Pixels>, usize)>,
    pub(crate) context_menu: Option<ContextMenu>,
    pub(crate) context_submenu: Option<ContextMenuKind>,
    pub(crate) context_submenu_aim: Option<ContextSubmenuAim>,
    pub(crate) context_submenu_hovered: bool,
    pub(crate) sidebar_edit: Option<SidebarEditMode>,
    pub(crate) sidebar_edit_blur_sub: Option<gpui::Subscription>,
    pub(crate) file_clipboard: Option<FileClipboard>,
    pub(crate) multi_cursor_clipboard: Option<MultiCursorClipboard>,
    pub(crate) ui_scale: f32,
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
    pub(crate) sidebar_custom_width: Option<f32>,
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
    pub(crate) tab_drag_candidate_id: Option<String>,
    pub(crate) tab_dragging_id: Option<String>,
    pub(crate) recently_closed_tabs: Vec<OpenTab>,
    pub(crate) tab_navigation_history: Vec<TabNavigationTarget>,
    pub(crate) tab_navigation_index: usize,
    pub(crate) tab_navigation_suspended: bool,
    pub(crate) dialog: Option<Dialog>,
    pub(crate) pending_close_tab_ids: Vec<String>,
    /// 多选：最后一个选中的 focus 路径
    pub(crate) focused_sidebar_path: Option<PathBuf>,
    pub(crate) suppress_next_sidebar_sync_path: Option<PathBuf>,
    /// 多选锚点（shift 连续选择的起点）
    pub(crate) sidebar_selection_anchor: Option<PathBuf>,
    pub(crate) sidebar_sort_mode: SidebarSortMode,
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
    /// 拖拽悬停自动展开的延迟 task
    pub(crate) sidebar_drag_expand_task: Option<Task<()>>,
    /// 拖拽悬停时自动展开、离开后可回折的目录集合
    pub(crate) sidebar_drag_auto_expanded_dirs: HashSet<PathBuf>,
    pub(crate) sidebar_items_cache: Vec<SidebarItem>,
    pub(crate) sidebar_items_dirty: bool,
    pub(crate) sidebar_scroll_handle: ScrollHandle,
    pub(crate) tab_bar_scroll_handle: ScrollHandle,
    pub(crate) sidebar_render_limit: usize,
    pub(crate) sidebar_git_statuses: HashMap<PathBuf, SidebarGitStatus>,
    pub(crate) sidebar_git_statuses_dirty: bool,
    pub(crate) sidebar_git_statuses_scanned_at: Option<SystemTime>,
    pub(crate) workspaces: Vec<PathBuf>,
    pub(crate) _watcher_task: Option<Task<()>>,
    pub(crate) temp_save_tasks: HashMap<String, Task<()>>,
    pub(crate) temp_save_epochs: HashMap<String, usize>,
    pub(crate) outline_resizing: bool,
    pub(crate) outline_scroll_handle: ScrollHandle,
    pub(crate) git_history_scroll_handle: ScrollHandle,
    pub(crate) info_panel_cache_tab_id: Option<String>,
    pub(crate) info_panel_cache_result: Option<Option<info_panel::InfoPanelData>>,
    pub(crate) git_history_cache_path: Option<PathBuf>,
    pub(crate) git_history_cache_result: Option<Option<Vec<info_panel::GitHistoryItem>>>,
    pub(crate) git_history_cache_scanned_at: Option<SystemTime>,
    pub(crate) utility_panel: Option<RightUtilityPanel>,
    pub(crate) displayed_utility_panel: Option<RightUtilityPanel>,
    pub(crate) chat_panel_visible: bool,
    pub(crate) last_non_chat_utility_panel: Option<RightUtilityPanel>,
    pub(crate) chat_panel_placement: ChatPanelPlacement,
    pub(crate) utility_panel_custom_width: Option<f32>,
    pub(crate) chat_panel_custom_size: Option<f32>,
    pub(crate) info_panel_custom_width: Option<f32>,
    pub(crate) git_history_panel_custom_width: Option<f32>,
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
    pub(crate) tooltip_pending: Option<tooltip::TooltipState>,
    pub(crate) tooltip_visible: Option<tooltip::TooltipState>,
    pub(crate) tooltip_task: Option<Task<()>>,
    pub(crate) last_window_width: f32,
    pub(crate) last_window_height: f32,

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
    pub(crate) workspace_search_scroll_handle: ScrollHandle,
    pub(crate) workspace_search_render_limit: usize,

    // ── Quick open state ───────────────────────────────────────────
    pub(crate) quick_open_visible: bool,
    pub(crate) quick_open_query: String,
    pub(crate) quick_open_results: Vec<QuickOpenResult>,
    pub(crate) quick_open_selected_index: usize,
    pub(crate) quick_open_editor: Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) quick_open_task: Option<Task<()>>,
    pub(crate) quick_open_scroll_handle: ScrollHandle,
    pub(crate) quick_open_render_limit: usize,

    // ── Chat panel state ───────────────────────────────────────────
    pub(crate) chat_tabs: Vec<chat_panel::ChatTab>,
    pub(crate) active_chat_tab_id: Option<String>,
    pub(crate) chat_tab_drag_candidate_id: Option<String>,
    pub(crate) chat_tab_dragging_id: Option<String>,
    pub(crate) chat_tab_bar_scroll_handle: ScrollHandle,
    pub(crate) chat_panel_focus_handle: FocusHandle,
    pub(crate) pending_chat_input_focus: bool,
    pub(crate) chat_terminal_scroll_px: Pixels,
    pub(crate) chat_terminal_mouse_scroll_px: Point<Pixels>,
    pub(crate) chat_terminal_cell_width_px: f32,
    pub(crate) chat_terminal_cell_height_px: f32,
    pub(crate) chat_terminal_marked_text: Option<String>,
    pub(crate) chat_terminal_selection: Option<chat_panel::ChatTerminalSelection>,
    pub(crate) chat_terminal_selecting: bool,
    pub(crate) chat_terminal_mouse_left_pressed: bool,
    pub(crate) chat_terminal_mouse_middle_pressed: bool,
    pub(crate) chat_terminal_mouse_right_pressed: bool,
    pub(crate) chat_terminal_last_mouse_cell: Option<(usize, usize)>,
    pub(crate) chat_terminal_input_bounds: Option<gpui::Bounds<gpui::Pixels>>,

    // Settings panel state
    pub(crate) settings_scroll_handle: ScrollHandle,
    pub(crate) settings_theme_dark_mode: bool,
    pub(crate) settings_font_picker: Option<settings_panel::FontPickerState>,
    pub(crate) settings_font_picker_editor: Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) settings_font_size_editor: Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) settings_ui_font_size_editor: Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) settings_line_height_editor: Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) settings_letter_spacing_editor: Option<gpui::Entity<crate::text_input::TextInput>>,
    pub(crate) font_picker_trigger_position: Option<gpui::Point<gpui::Pixels>>,
    pub(crate) font_picker_button_bounds: std::collections::HashMap<
        (crate::editor::settings_panel::FontRole, usize),
        std::rc::Rc<std::cell::Cell<Option<gpui::Bounds<gpui::Pixels>>>>,
    >,
    pub(crate) cached_font_names: Option<Vec<String>>,
    pub(crate) ui_language: UiLanguage,
    pub(crate) app_settings: AppSettings,
}

impl Drop for EditorView {
    fn drop(&mut self) {
        self.stop_chat_terminal();
    }
}

impl EditorView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let sidebar_root = PathBuf::new();
        let expanded_dirs = HashSet::new();
        let sidebar_error = None;
        let initial_text = default_document_text();
        let first_line_end = initial_text.find('\n').unwrap_or(initial_text.len());
        let buffer = Buffer::from_text(&initial_text);
        let decorations = markdown::parse_decorations(buffer.text());
        let initial_snapshot = EditorSnapshot {
            text: initial_text,
            cursor_offset: first_line_end,
            cursor_anchor: first_line_end,
            scroll_offset: 0.0,
            extra_cursors: Vec::new(),
        };
        // Load persisted settings
        let app_settings = AppSettings::load();
        let ui_language = app_settings.ui_language();
        let theme_kind = app_settings.theme_kind();
        let editor_theme_mode = app_settings.editor_theme_mode_for_appearance(theme_kind.is_dark());
        let theme = crate::theme::apply_editor_theme_mode(
            &theme_kind.build(),
            editor_theme_mode,
            theme_kind.is_dark(),
        );
        let mut styles = crate::style::Styles::from_theme(&theme);
        // Apply saved font settings
        styles.editor.editor_font_size_px = app_settings.font_size;
        styles.editor.editor_paragraph_line_height_factor = app_settings.line_height;
        styles.editor.editor_heading_line_height_factor = app_settings.line_height;
        styles.editor.editor_letter_spacing_px = app_settings.letter_spacing;
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
        let init_max_width = styles.editor.editor_max_width_px;
        let init_padding_left = styles.editor.editor_padding_left_px;
        let init_padding_right = styles.editor.editor_padding_right_px;
        let init_cursor_opacity = styles.cursor.cursor_opacity_max;
        let mut this = Self {
            buffer,
            cursor: Cursor::new(),
            extra_cursors: Vec::new(),
            focus_handle: cx.focus_handle(),
            theme_kind,
            editor_theme_mode,
            theme,
            styles,
            decorations,
            scroll_offset: 0.0,
            is_selecting: false,
            vertical_move_goal_x: None,
            wrap_boundary_affinity: None,
            line_display_info: Vec::new(),
            layout_dirty: false,
            pending_scroll_to_cursor: false,
            pending_preview_reveal: None,
            last_viewport_height: 600.0,
            last_viewport_width: init_max_width,
            cursor_opacity: init_cursor_opacity,
            cursor_opacity_direction: -1.0,
            blink_epoch: 0,
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
            hovering_image_preview_line: None,
            hovering_generated_preview_line: None,
            hovering_generated_preview_zoom_line: None,
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
            sidebar_root: sidebar_root.clone(),
            expanded_dirs,
            selected_sidebar_paths: HashSet::new(),
            current_file_path: None,
            sidebar_error,
            tabs: vec![
                OpenTab {
                    id: "welcome-1".to_string(),
                    title: crate::i18n::translate(ui_language, TextKey::WelcomeTitle).to_string(),
                    kind: TabKind::Editor,
                    path: None,
                    file_kind: Some(FileKind::Markdown),
                    editor_left_aligned: false,
                    snapshot: initial_snapshot,
                    image_view: ImageViewState::default(),
                    git_diff: None,
                    is_preview: false,
                    is_dirty: false,
                    is_deleted: false,
                    is_pinned: false,
                    recovery: config_store::TabRecoveryState::default(),
                    history: history_store::TabHistory::new(
                        Some(history_store::default_source_identity("welcome-1", None)),
                        Some(history_store::default_journal_path("welcome-1", None)),
                    ),
                },
                OpenTab {
                    id: "settings-1".to_string(),
                    title: crate::i18n::translate(ui_language, TextKey::SettingsTitle).to_string(),
                    kind: TabKind::Settings,
                    path: None,
                    file_kind: None,
                    editor_left_aligned: false,
                    snapshot: EditorSnapshot {
                        text: String::new(),
                        cursor_offset: 0,
                        cursor_anchor: 0,
                        scroll_offset: 0.0,
                        extra_cursors: Vec::new(),
                    },
                    image_view: ImageViewState::default(),
                    git_diff: None,
                    is_preview: false,
                    is_dirty: false,
                    is_deleted: false,
                    is_pinned: false,
                    recovery: config_store::TabRecoveryState::default(),
                    history: history_store::TabHistory::default(),
                },
            ],
            active_tab_id: Some("settings-1".to_string()),
            next_tab_id: 2,
            last_editor_left: 0.0,
            last_editor_width: init_max_width,
            last_content_left: init_padding_left,
            last_text_width: init_max_width - init_padding_left - init_padding_right,
            content_origin: Point::default(),
            last_stable_input_bounds: None,
            pending_input_bounds: None,
            preview_overlay: None,
            preview_overlay_bounds: None,
            preview_overlay_image_bounds: None,
            last_mouse_down_target: None,
            context_menu: None,
            context_submenu: None,
            context_submenu_aim: None,
            context_submenu_hovered: false,
            sidebar_edit: None,
            sidebar_edit_blur_sub: None,
            file_clipboard: None,
            multi_cursor_clipboard: None,
            ui_scale: app_settings.ui_scale,
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
            sidebar_custom_width: None,
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
            tab_drag_candidate_id: None,
            tab_dragging_id: None,
            recently_closed_tabs: Vec::new(),
            tab_navigation_history: vec![TabNavigationTarget::Settings],
            tab_navigation_index: 0,
            tab_navigation_suspended: false,
            dialog: None,
            pending_close_tab_ids: Vec::new(),
            focused_sidebar_path: None,
            suppress_next_sidebar_sync_path: None,
            sidebar_selection_anchor: None,
            sidebar_sort_mode: app_settings.sidebar_sort_mode(),
            sidebar_starred_collapsed: false,
            sidebar_drag_hover_target: None,
            sidebar_drag_expand_hover_dir: None,
            sidebar_drag_child_hover_position: None,
            sidebar_drag_scroll_task: None,
            selection_drag_scroll_task: None,
            selection_drag_position: None,
            sidebar_drag_expand_task: None,
            sidebar_drag_auto_expanded_dirs: HashSet::new(),
            sidebar_items_cache: Vec::new(),
            sidebar_items_dirty: true,
            sidebar_scroll_handle: ScrollHandle::new(),
            tab_bar_scroll_handle: ScrollHandle::new(),
            sidebar_render_limit: 200,
            sidebar_git_statuses: HashMap::new(),
            sidebar_git_statuses_dirty: true,
            sidebar_git_statuses_scanned_at: None,
            workspaces: Vec::new(),
            _watcher_task: None,
            temp_save_tasks: HashMap::new(),
            temp_save_epochs: HashMap::new(),
            outline_resizing: false,
            outline_scroll_handle: ScrollHandle::new(),
            git_history_scroll_handle: ScrollHandle::new(),
            info_panel_cache_tab_id: None,
            info_panel_cache_result: None,
            git_history_cache_path: None,
            git_history_cache_result: None,
            git_history_cache_scanned_at: None,
            utility_panel: None,
            displayed_utility_panel: None,
            chat_panel_visible: false,
            last_non_chat_utility_panel: None,
            chat_panel_placement: ChatPanelPlacement::Right,
            utility_panel_custom_width: None,
            chat_panel_custom_size: None,
            info_panel_custom_width: None,
            git_history_panel_custom_width: None,
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
            tooltip_pending: None,
            tooltip_visible: None,
            tooltip_task: None,
            last_window_width: 1360.0,
            last_window_height: 820.0,

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
            workspace_search_scroll_handle: ScrollHandle::new(),
            workspace_search_render_limit: 100,

            quick_open_visible: false,
            quick_open_query: String::new(),
            quick_open_results: Vec::new(),
            quick_open_selected_index: 0,
            quick_open_editor: None,
            quick_open_task: None,
            quick_open_scroll_handle: ScrollHandle::new(),
            quick_open_render_limit: 100,
            chat_tabs: Vec::new(),
            active_chat_tab_id: None,
            chat_tab_drag_candidate_id: None,
            chat_tab_dragging_id: None,
            chat_tab_bar_scroll_handle: ScrollHandle::new(),
            chat_panel_focus_handle: cx.focus_handle(),
            pending_chat_input_focus: false,
            chat_terminal_scroll_px: Pixels::ZERO,
            chat_terminal_mouse_scroll_px: gpui::point(Pixels::ZERO, Pixels::ZERO),
            chat_terminal_cell_width_px: 0.0,
            chat_terminal_cell_height_px: 0.0,
            chat_terminal_marked_text: None,
            chat_terminal_selection: None,
            chat_terminal_selecting: false,
            chat_terminal_mouse_left_pressed: false,
            chat_terminal_mouse_middle_pressed: false,
            chat_terminal_mouse_right_pressed: false,
            chat_terminal_last_mouse_cell: None,
            chat_terminal_input_bounds: None,

            // Settings panel state
            settings_scroll_handle: ScrollHandle::new(),
            settings_theme_dark_mode: theme_kind.is_dark(),
            settings_font_picker: None,
            settings_font_picker_editor: None,
            settings_font_size_editor: None,
            settings_ui_font_size_editor: None,
            settings_line_height_editor: None,
            settings_letter_spacing_editor: None,
            font_picker_trigger_position: None,
            font_picker_button_bounds: std::collections::HashMap::new(),
            cached_font_names: None,
            ui_language,
            app_settings,
        };
        session_store::restore_session(&mut this);
        this.refresh_builtin_tab_titles();
        this.sync_panel_display_state();
        this.start_blinking(cx);
        if !this.sidebar_root.as_os_str().is_empty() {
            this.start_watching(cx);
        }
        this.ensure_font_size_editor(cx);
        this.ensure_ui_font_size_editor(cx);
        this.toolbar_opacity = this.desired_toolbar_opacity();
        this.toolbar_opacity_target = this.toolbar_opacity;

        this
    }

    pub(crate) fn t(&self, key: TextKey) -> &'static str {
        crate::i18n::translate(self.ui_language, key)
    }

    pub(crate) fn refresh_builtin_tab_titles(&mut self) {
        let welcome_title = self.t(TextKey::WelcomeTitle).to_string();
        let settings_title = self.t(TextKey::SettingsTitle).to_string();
        for tab in &mut self.tabs {
            if tab.kind == TabKind::Settings {
                tab.title = settings_title.clone();
            } else if tab.id == "welcome-1" && tab.path.is_none() {
                tab.title = welcome_title.clone();
            }
        }
    }

    pub(crate) fn apply_ui_language(&mut self, language: UiLanguage, cx: &mut Context<Self>) {
        if self.ui_language == language {
            self.close_font_picker(cx);
            return;
        }
        self.ui_language = language;
        self.app_settings.language = language.code().to_string();
        self.refresh_builtin_tab_titles();
        self.finalize_settings_change(cx);
    }

    pub(crate) fn should_show_editor_toolbar(&self, base_visible: bool) -> bool {
        base_visible
            && (self.scroll_offset <= 0.5
                || !self.toolbar_hidden_by_scroll
                || self.toolbar_corner_hovered)
    }

    fn toolbar_base_visible(&self) -> bool {
        !self.tabs.is_empty() && !self.active_tab_is_settings() && !self.active_tab_is_binary()
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

    pub(crate) fn sidebar_target_width(&self) -> f32 {
        self.sidebar_custom_width
            .unwrap_or_else(|| self.scaled(self.styles.sidebar.sidebar_width_px))
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
        self.utility_panel
            .and_then(|panel| self.utility_panel_custom_width_for(panel))
            .unwrap_or_else(|| self.scaled(self.styles.utility_panel.utility_panel_width_px))
    }

    pub(crate) fn utility_panel_custom_width_for(&self, panel: RightUtilityPanel) -> Option<f32> {
        match panel {
            RightUtilityPanel::Outline => self.utility_panel_custom_width,
            RightUtilityPanel::Chat => self.chat_panel_custom_size,
            RightUtilityPanel::Info => self.info_panel_custom_width,
            RightUtilityPanel::GitHistory => self.git_history_panel_custom_width,
        }
    }

    pub(crate) fn set_utility_panel_custom_width_for(
        &mut self,
        panel: RightUtilityPanel,
        width: Option<f32>,
    ) {
        match panel {
            RightUtilityPanel::Outline => self.utility_panel_custom_width = width,
            RightUtilityPanel::Chat => self.chat_panel_custom_size = width,
            RightUtilityPanel::Info => self.info_panel_custom_width = width,
            RightUtilityPanel::GitHistory => self.git_history_panel_custom_width = width,
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

    pub(crate) fn should_render_bottom_chat_panel(&self) -> bool {
        self.should_render_utility_panel()
            && self.displayed_utility_panel == Some(RightUtilityPanel::Chat)
            && self.chat_panel_placement == ChatPanelPlacement::Bottom
    }

    pub(crate) fn should_render_utility_panel_on_right(&self) -> bool {
        self.should_render_utility_panel() && !self.should_render_bottom_chat_panel()
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
        self.displayed_theme_debugger
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
        let content_right =
            (self.last_content_left + self.last_text_width).clamp(content_left, right);
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
        let hovering_side_gutters = y >= top
            && y <= bottom
            && x >= left
            && x <= right
            && (x < content_left || x > content_right);
        let hovering_top_reveal = toolbar_overlaps_content
            && y >= top
            && y <= (top + self.scaled(self.styles.toolbar.toolbar_reveal_hover_height_px))
                .min(bottom)
            && x >= left
            && x <= right;
        let hovering = hovering_side_gutters || hovering_top_reveal;
        let changed = hovering != self.toolbar_corner_hovered;
        self.toolbar_corner_hovered = hovering;
        if changed {
            self.sync_toolbar_opacity(cx);
        }
        changed
    }

    pub(crate) fn queue_image_load(&mut self, path: String, cx: &mut Context<Self>) {
        if self.image_cache.contains_key(&path) {
            return;
        }

        self.image_cache.insert(path.clone(), None);
        let current_file = self.current_file_path.clone();
        let workspaces = self.workspaces.clone();
        cx.spawn(async move |this, cx| {
            let image_path = path.clone();
            let file_dir = current_file
                .as_ref()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()));
            let maybe_image = cx
                .background_executor()
                .spawn(async move {
                    helpers::load_image(&image_path, file_dir.as_ref(), &workspaces)
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
        if let Some(sidebar_edit) = &self.sidebar_edit {
            let styles = self.sidebar_edit_text_input_styles();
            sidebar_edit
                .editor()
                .update(cx, |input, cx| input.set_styles(styles, cx));
        }
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
        self.refresh_theme_styles(true, cx);
        self.app_settings.theme = theme_kind_to_string(kind);
        if kind.is_dark() {
            self.app_settings.dark_theme = theme_kind_to_string(kind);
        } else {
            self.app_settings.light_theme = theme_kind_to_string(kind);
        }
        self.app_settings
            .set_editor_theme_mode_for_appearance(kind.is_dark(), self.editor_theme_mode);
        self.app_settings.save();
        cx.notify();
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

        let pending = Arc::new(std::sync::Mutex::new(Vec::<PathBuf>::new()));
        let pending_writer = pending.clone();

        let mut watcher = match notify::recommended_watcher(
            move |result: Result<notify::Event, notify::Error>| {
                if let Ok(event) = result {
                    if !event.paths.is_empty() {
                        if let Ok(mut guard) = pending_writer.lock() {
                            guard.extend(event.paths);
                        }
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

                let changed_paths: Vec<PathBuf> = {
                    let mut guard = match pending.lock() {
                        Ok(g) => g,
                        Err(_) => break,
                    };
                    std::mem::take(&mut *guard)
                };

                if changed_paths.is_empty() {
                    continue;
                }

                let result = this.update(cx, |editor, cx| {
                    editor.handle_fs_events(&changed_paths, cx);
                });
                if result.is_err() {
                    break;
                }
            }
        }));
    }

    fn is_internal_temp_change_path(&self, path: &Path) -> bool {
        if path
            .components()
            .any(|component| component.as_os_str().to_str() == Some(".markdown-editor-cache"))
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

    fn should_ignore_fs_change_path(&self, path: &Path) -> bool {
        if !path.starts_with(&self.sidebar_root) {
            return true;
        }

        self.is_internal_temp_change_path(path)
            || self.path_contains_ignored_workspace_component(path)
    }

    fn fs_change_affects_git_status(&self, path: &Path) -> bool {
        match std::fs::symlink_metadata(path) {
            Ok(metadata) => !metadata.is_dir(),
            Err(_) => true,
        }
    }

    pub(crate) fn handle_fs_events(&mut self, changed_paths: &[PathBuf], cx: &mut Context<Self>) {
        let changed_paths: Vec<&Path> = changed_paths
            .iter()
            .map(PathBuf::as_path)
            .filter(|path| !self.should_ignore_fs_change_path(path))
            .collect();
        if changed_paths.is_empty() {
            return;
        }

        let active_tab_id = self.active_tab_id.clone();
        let current_file_hit = self.current_file_path.as_ref().is_some_and(|current_path| {
            changed_paths
                .iter()
                .any(|path| *path == current_path || current_path.starts_with(path))
        });
        let mut needs_notify = true;
        let mut active_tab_needs_reload = false;
        self.invalidate_sidebar_items_cache();
        if changed_paths
            .iter()
            .any(|path| self.fs_change_affects_git_status(path))
        {
            self.invalidate_sidebar_git_statuses();
        }
        if current_file_hit {
            self.invalidate_active_file_panel_caches();
        }

        for tab in &mut self.tabs {
            if let Some(ref tab_path) = tab.path {
                if changed_paths
                    .iter()
                    .any(|path| *path == tab_path || tab_path.starts_with(path))
                {
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

        // Reload the active tab's buffer if it was externally modified
        if active_tab_needs_reload {
            if let Some(ref active_id) = self.active_tab_id {
                if let Some(tab) = self.tabs.iter().find(|t| &t.id == active_id) {
                    if !tab.is_dirty && !tab.is_deleted {
                        let snapshot = tab.snapshot.clone();
                        self.buffer = crate::buffer::Buffer::from_text(&snapshot.text);
                        self.cursor.offset = snapshot.cursor_offset.min(self.buffer.len());
                        self.cursor.anchor = snapshot.cursor_anchor.min(self.buffer.len());
                        self.reset_cursor_visual_targeting();
                        self.reparse();
                    }
                }
            }
        }

        if needs_notify {
            cx.notify();
        }
    }

    // ── Dialog ──────────────────────────────────────────────────────

    pub(crate) fn show_dialog(&mut self, dialog: Dialog, cx: &mut Context<Self>) {
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
                if let Some(index) = self.tabs.iter().position(|t| t.id == tab_id) {
                    self.tabs[index].is_dirty = false;
                    self.close_tab(&tab_id, cx);
                }
            }
            DialogAction::SaveAndCloseTab { .. } => {
                // Save action is handled separately via save_dialog
            }
            DialogAction::DeletePath { path } => {
                self.delete_path(path, cx);
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
                if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == tab_id) {
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
                        self.clear_temp_file_for_tab(&tab_id);
                        session_store::persist_session(self);
                        self.close_tab(&tab_id, cx);
                    } else {
                        let text = tab.snapshot.text.clone();
                        let default_name = format!("{}.md", tab.title);
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
                                            editor.tabs.iter_mut().find(|t| t.id == tab_id_clone)
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
                } else {
                    self.close_tab(&tab_id, cx);
                }
            }
            DialogAction::DeletePath { .. } => {
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
        file_paths.extend(macos_clipboard_file_paths());
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
            .as_ref()
            .and_then(|path| path.parent())
            .map(Path::to_path_buf)
        else {
            self.show_save_before_import_dialog(cx);
            return Ok(ClipboardPasteOutcome::Consumed);
        };

        let mut markdown_lines = Vec::new();
        for image in images {
            let target_path = unique_import_path(
                &target_dir,
                &timestamp_file_stem(),
                Some(image_extension(image.format)),
            );
            std::fs::write(&target_path, &image.bytes)?;
            let file_name = target_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            markdown_lines.push(markdown_reference_for_import_name(file_name, true));
        }

        for source_path in file_paths {
            let extension = source_path
                .extension()
                .and_then(|extension| extension.to_str());
            let target_path = unique_import_path(&target_dir, &timestamp_file_stem(), extension);
            std::fs::copy(&source_path, &target_path)?;
            let file_name = target_path
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or_default();
            markdown_lines.push(markdown_reference_for_import_name(
                file_name,
                is_image_path(&source_path),
            ));
        }

        if markdown_lines.is_empty() {
            Ok(ClipboardPasteOutcome::Consumed)
        } else {
            Ok(ClipboardPasteOutcome::Insert(markdown_lines.join("\n")))
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

    pub(crate) fn indent_line_indices(&mut self, line_indices: &[usize], spaces: &str) {
        if line_indices.is_empty() || spaces.is_empty() {
            return;
        }

        let inserted = spaces.len();
        let mut cursor_states = self.cursor_states_with_primary_flag();
        for &line in line_indices {
            let line_start = self.buffer.line_start(line);
            self.buffer.insert(line_start, spaces);
            for (cursor, _) in &mut cursor_states {
                cursor.offset = Self::adjust_offset_for_line_prefix_change(
                    cursor.offset,
                    line_start,
                    0,
                    inserted,
                )
                .min(self.buffer.len());
                cursor.anchor = Self::adjust_offset_for_line_prefix_change(
                    cursor.anchor,
                    line_start,
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

            let line_start = self.buffer.line_start(line);
            let spaces = " ".repeat(width);
            self.buffer.insert(line_start, &spaces);
            for (cursor, _) in &mut cursor_states {
                cursor.offset =
                    Self::adjust_offset_for_line_prefix_change(cursor.offset, line_start, 0, width)
                        .min(self.buffer.len());
                cursor.anchor =
                    Self::adjust_offset_for_line_prefix_change(cursor.anchor, line_start, 0, width)
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
            let line_start = self.buffer.line_start(line);
            let strip = self
                .buffer
                .line_content(line)
                .bytes()
                .take(max_spaces)
                .take_while(|&byte| byte == b' ')
                .count();
            if strip == 0 {
                continue;
            }

            self.buffer.replace(line_start..line_start + strip, "");
            for (cursor, _) in &mut cursor_states {
                cursor.offset =
                    Self::adjust_offset_for_line_prefix_change(cursor.offset, line_start, strip, 0)
                        .min(self.buffer.len());
                cursor.anchor =
                    Self::adjust_offset_for_line_prefix_change(cursor.anchor, line_start, strip, 0)
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

            let line_start = self.buffer.line_start(line);
            let strip = self
                .buffer
                .line_content(line)
                .bytes()
                .take(width)
                .take_while(|&byte| byte == b' ')
                .count();
            if strip == 0 {
                continue;
            }

            self.buffer.replace(line_start..line_start + strip, "");
            for (cursor, _) in &mut cursor_states {
                cursor.offset =
                    Self::adjust_offset_for_line_prefix_change(cursor.offset, line_start, strip, 0)
                        .min(self.buffer.len());
                cursor.anchor =
                    Self::adjust_offset_for_line_prefix_change(cursor.anchor, line_start, strip, 0)
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
            .on_mouse_down(MouseButton::Left, cx.listener(|_, _, _, _| {}))
            .on_mouse_up(MouseButton::Left, cx.listener(|_, _, _, _| {}))
            .on_mouse_down(MouseButton::Right, cx.listener(|_, _, _, _| {}))
            .on_mouse_move(cx.listener(|_, _, _, _| {}))
            .on_scroll_wheel(cx.listener(|_, _, _, _| {}))
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
        self.cursor_opacity_direction = -1.0;
        let epoch = self.next_blink_epoch();
        self.schedule_blink(epoch, cx);
    }

    pub(crate) fn schedule_blink(&self, epoch: usize, cx: &mut Context<Self>) {
        let tick = self.styles.cursor.blink_tick();
        let opacity_step = self.styles.cursor.cursor_opacity_step;
        let min_opacity = self.styles.cursor.cursor_opacity_min;
        let max_opacity = self.styles.cursor.cursor_opacity_max;
        let transparent_dur = self.styles.cursor.blink_transparent_duration();
        let visible_dur = self.styles.cursor.blink_visible_duration();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(tick).await;
            this.update(cx, |this, cx| {
                if this.blink_epoch == epoch {
                    this.cursor_opacity += opacity_step * this.cursor_opacity_direction;
                    if this.cursor_opacity <= min_opacity {
                        this.cursor_opacity = min_opacity;
                        this.cursor_opacity_direction = 1.0;
                        cx.notify();
                        this.schedule_blink_pause(epoch, transparent_dur, cx);
                        return;
                    } else if this.cursor_opacity >= max_opacity {
                        this.cursor_opacity = max_opacity;
                        this.cursor_opacity_direction = -1.0;
                        cx.notify();
                        this.schedule_blink_pause(epoch, visible_dur, cx);
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
        self.cursor_opacity = self.styles.cursor.cursor_opacity_max;
        self.cursor_opacity_direction = -1.0;
        let epoch = self.next_blink_epoch();
        self.persist_active_tab_state();
        self.schedule_temp_save_for_active_tab(cx);
        cx.notify();
        let pause_delay = self.styles.cursor.blink_pause_delay();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(pause_delay).await;
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
        let file_kind = self
            .active_file_kind()
            .cloned()
            .unwrap_or(FileKind::Markdown);
        let git_diff_rows = self.active_git_diff_state().map(|diff| diff.rows.clone());
        if let Some(rows) = git_diff_rows {
            self.decorations = rows.into_iter().map(|row| row.decoration).collect();
            if self.decorations.is_empty() {
                self.decorations.push(LineDecoration::default());
            }
        } else {
            self.decorations = parse_decorations_for_file_kind(self.buffer.text(), &file_kind);
        }
        self.refresh_generated_block_previews();
        self.layout_dirty = true;
        self.invalidate_info_panel_cache();
    }

    fn refresh_generated_block_previews(&mut self) {
        self.refresh_generated_block_previews_with_mode(false);
    }

    pub(crate) fn rerender_generated_block_previews(&mut self) {
        self.refresh_generated_block_previews_with_mode(true);
    }

    fn refresh_generated_block_previews_with_mode(&mut self, force: bool) {
        let mut next_generated_keys = HashSet::new();

        for decoration in &self.decorations {
            let Some(preview) = decoration.block_preview.as_ref() else {
                continue;
            };
            if !preview.is_anchor || preview.source_text.is_empty() {
                continue;
            }
            next_generated_keys.insert(preview.cache_key.clone());
            if !force
                && self.image_cache.contains_key(&preview.cache_key)
                && self
                    .generated_preview_svg_cache
                    .contains_key(&preview.cache_key)
            {
                continue;
            }

            let svg = helpers::build_generated_block_preview_svg(
                preview.kind,
                &preview.source_text,
                &self.theme,
                helpers::GeneratedPreviewFontFamilies {
                    monospace: self.styles.editor.editor_monospace_font_family,
                    body_cjk: self.styles.editor.editor_body_cjk_font_family,
                },
            );
            self.generated_preview_svg_cache
                .insert(preview.cache_key.clone(), Arc::<str>::from(svg.clone()));
            let image = helpers::load_generated_preview_image_from_svg(&svg, preview.kind)
                .ok()
                .map(Arc::new);
            self.image_cache.insert(preview.cache_key.clone(), image);
            self.generated_preview_overlay_cache
                .remove(&preview.cache_key);
        }

        self.generated_preview_keys = next_generated_keys;
    }

    pub(crate) fn active_file_kind(&self) -> Option<&FileKind> {
        self.active_tab_index()
            .and_then(|index| self.tabs.get(index))
            .and_then(|tab| tab.file_kind.as_ref())
    }

    pub(crate) fn active_tab_supports_text_buffer(&self) -> bool {
        self.active_file_kind()
            .is_some_and(FileKind::supports_text_buffer)
    }

    pub(crate) fn active_tab_is_non_markdown_text(&self) -> bool {
        self.active_file_kind()
            .is_some_and(|file_kind| file_kind.supports_text_buffer() && !file_kind.is_markdown())
    }

    pub(crate) fn active_tab_supports_text_search(&self) -> bool {
        self.active_file_kind()
            .is_some_and(FileKind::supports_text_search)
    }

    pub(crate) fn visible_uniform_list_range(
        &self,
        scroll_handle: &ScrollHandle,
        prefix_height: f32,
        item_height: f32,
        item_count: usize,
        overscan: usize,
    ) -> (usize, usize, f32, f32) {
        if item_count == 0 || item_height <= 0.0 {
            return (0, 0, 0.0, 0.0);
        }

        let viewport_height: f32 = scroll_handle.bounds().size.height.into();
        let viewport_height = if viewport_height > 0.0 {
            viewport_height
        } else {
            self.last_window_height.max(item_height)
        };
        let scroll_y = (-f32::from(scroll_handle.offset().y)).max(0.0);
        let content_start = (scroll_y - prefix_height).max(0.0);
        let content_end = (scroll_y + viewport_height - prefix_height).max(0.0);
        let start = ((content_start / item_height).floor() as usize).saturating_sub(overscan);
        let end = ((content_end / item_height).ceil() as usize)
            .saturating_add(overscan)
            .min(item_count)
            .max(start);
        let top_spacer_height = start as f32 * item_height;
        let bottom_spacer_height = item_count.saturating_sub(end) as f32 * item_height;
        (start, end, top_spacer_height, bottom_spacer_height)
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

    pub(crate) fn active_tab_is_diff(&self) -> bool {
        self.active_tab_id
            .as_ref()
            .and_then(|id| self.tabs.iter().find(|tab| &tab.id == id))
            .is_some_and(|tab| tab.kind == TabKind::Diff)
    }

    pub(crate) fn active_tab_is_read_only(&self) -> bool {
        self.active_tab_is_diff()
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
                let fit_scale = (viewport_width / image_width)
                    .min(viewport_height / image_height)
                    .min(1.0);
                let display_width = image_width * fit_scale * image_view.zoom_factor;
                let display_height = image_height * fit_scale * image_view.zoom_factor;
                let limit_x = ((display_width - viewport_width) / 2.0).max(0.0);
                let limit_y = ((display_height - viewport_height) / 2.0).max(0.0);
                image_view.pan_x = image_view.pan_x.clamp(-limit_x, limit_x);
                image_view.pan_y = image_view.pan_y.clamp(-limit_y, limit_y);
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
        self.preview_overlay = None;
        self.preview_overlay_bounds = None;
        self.preview_overlay_image_bounds = None;
        self.image_drag_state = None;
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
        view.zoom_factor = view.zoom_factor.clamp(1.0, 6.0);

        let viewport_width = f32::from(bounds.size.width).max(1.0);
        let viewport_height = f32::from(bounds.size.height).max(1.0);
        let fit_scale = (viewport_width / image_width)
            .min(viewport_height / image_height)
            .min(1.0);
        let display_width = image_width * fit_scale * view.zoom_factor;
        let display_height = image_height * fit_scale * view.zoom_factor;
        let limit_x = ((display_width - viewport_width) / 2.0).max(0.0);
        let limit_y = ((display_height - viewport_height) / 2.0).max(0.0);

        view.pan_x = view.pan_x.clamp(-limit_x, limit_x);
        view.pan_y = view.pan_y.clamp(-limit_y, limit_y);
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
        let content_start = self.line_content_start_col(line);
        if content_start > 0 && cursor_col < content_start {
            self.cursor
                .move_to(line_start + content_start, &self.buffer);
            if self.cursor.offset != previous_offset {
                self.clear_wrap_boundary_affinity();
            }
        }
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

        self.gutter_fold_reveal_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let done = this
                    .update(cx, |this, cx| {
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
        if let Some(index) = self.active_tab_index() {
            self.tabs[index].history.push_undo_snapshot(snapshot);
        }
        self.mark_active_tab_dirty();
    }

    pub(crate) fn checkpoint(&mut self) {
        self.ime_composition_snapshot = None;
        self.push_undo_snapshot(self.current_snapshot());
    }

    pub(crate) fn copy_selection_to_clipboard(&mut self, cx: &mut Context<Self>) {
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
                eprintln!("failed to import clipboard resources: {error}");
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
        self.ui_scale = clamped;
        self.sync_overlay_text_input_styles(cx);
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

    pub(crate) fn wrap_current_lines_as_code_block(&mut self) {
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
            format!("{leading_gap}```\n{block_text}```{trailing_gap}")
        } else {
            format!("{leading_gap}```\n{block_text}\n```{trailing_gap}")
        };
        let prefix_len = "```\n".len();
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

    pub(crate) fn apply_wrap(&mut self, prefix: &str, suffix: &str) {
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
                    // Remove existing format
                    let (new_text, new_start, new_end) = toggle_wrap(
                        self.buffer.text(),
                        sel_start,
                        sel_end,
                        existing_prefix,
                        existing_suffix,
                    );
                    self.buffer = Buffer::from_text(&new_text);
                    if existing_prefix == prefix && existing_suffix == suffix {
                        // Same format: select the inner text
                        self.cursor.anchor = new_start;
                        self.cursor.offset = new_end;
                    } else {
                        // Different format: apply the new wrap around inner text
                        let (new_text2, new_start2, new_end2) =
                            toggle_wrap(self.buffer.text(), new_start, new_end, prefix, suffix);
                        self.buffer = Buffer::from_text(&new_text2);
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

        let links: Vec<String> = paths
            .iter()
            .map(|path| {
                let relative = path
                    .strip_prefix(&self.sidebar_root)
                    .unwrap_or(path.as_path())
                    .to_string_lossy()
                    .replace('\\', "/");
                format!("[[{relative}]]")
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

    fn replace_table_block(&mut self, table_start: usize, table_end: usize, new_lines: &[String]) {
        let text_start = self.buffer.line_start(table_start);
        let text_end = self.buffer.line_end(table_end.saturating_sub(1));
        let new_text = new_lines.join("\n");
        self.ime_composition_snapshot = None;
        self.push_undo_snapshot(self.current_snapshot());
        self.buffer.replace(text_start..text_end, &new_text);
        self.decorations = markdown::parse_decorations(self.buffer.text());
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
        self.replace_table_block(start, end, &new_lines);
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
        self.replace_table_block(start, end, &new_lines);
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
        self.replace_table_block(start, end, &new_lines);
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
        self.replace_table_block(start, end, &new_lines);
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
        self.push_undo_snapshot(self.current_snapshot());
        self.buffer.replace(text_start..text_end, "");
        self.cursor.offset = text_start.min(self.buffer.len());
        self.cursor.anchor = self.cursor.offset;
        self.decorations = markdown::parse_decorations(self.buffer.text());
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

        let starting_number = if target_line > 0 {
            self.ordered_list_prefix_info(target_line - 1)
                .filter(|(_, prev_indent, _)| *prev_indent == indent)
                .map(|(_, _, prev_number)| prev_number + 1)
                .unwrap_or(1)
        } else {
            1
        };

        self.renumber_ordered_list_block_from(target_line, starting_number, indent);
    }
}

// ── Free functions ──────────────────────────────────────────────────

pub(crate) fn is_markdown_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "md" | "markdown"))
}

pub(crate) fn is_image_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            matches!(
                ext.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif" | "webp" | "bmp" | "ico" | "tif" | "tiff" | "avif"
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
    if matches!(ext.to_ascii_lowercase().as_str(), "md" | "markdown") {
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

    let relative = PathBuf::from(path_part);
    let candidates = if relative.is_absolute() {
        vec![relative]
    } else {
        let mut candidates = Vec::new();
        if let Some(current_file_path) = current_file_path
            && let Some(parent) = current_file_path.parent()
        {
            candidates.push(parent.join(&relative));
        }
        candidates.push(sidebar_root.join(&relative));
        candidates
    };

    candidates.into_iter().find(|candidate| candidate.exists())
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
        FileKind::Text | FileKind::Image | FileKind::Binary => plain_text_decorations(text, None),
    }
}

fn plain_text_decorations(text: &str, code_language: Option<String>) -> Vec<LineDecoration> {
    let line_count = text.lines().count().max(1);
    let line_count = if text.ends_with('\n') {
        line_count + 1
    } else {
        line_count
    };

    let mut decorations: Vec<LineDecoration> =
        (0..line_count).map(|_| LineDecoration::default()).collect();
    if let Some(language) = code_language {
        for decoration in &mut decorations {
            decoration.code_language = Some(language.clone());
        }
    }
    decorations
}

impl EditorView {
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
                if let Err(error) = std::process::Command::new("open").arg(&url).spawn() {
                    eprintln!("failed to open URL: {error}");
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

pub(crate) fn default_document_text() -> String {
    crate::resolve_runtime_path("EDITOR.md")
        .and_then(|path| std::fs::read_to_string(path).ok())
        .unwrap_or_else(|| include_str!("../../EDITOR.md").to_string())
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
    use super::{
        heading_matches_anchor, markdown_reference_for_import_name, resolve_markdown_target_path,
        split_markdown_target, text_tab_is_dirty, unique_import_path,
    };

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
    fn markdown_reference_for_import_name_formats_images_and_files() {
        assert_eq!(
            markdown_reference_for_import_name("20260507153045.png", true),
            "![](20260507153045.png)"
        );
        assert_eq!(
            markdown_reference_for_import_name("20260507153045.pdf", false),
            "[20260507153045.pdf](20260507153045.pdf)"
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
}
