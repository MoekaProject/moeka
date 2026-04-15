mod element;
mod fold;
mod handlers;
mod helpers;
mod input;
mod menus;
mod outline;
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
    ClipboardItem, Context, FocusHandle, Focusable, Pixels, Point, Render, RenderImage,
    ScrollHandle, SharedString, Task, WrappedLine,
};

use crate::actions::*;
use crate::buffer::Buffer;
use crate::cursor::Cursor;
use crate::markdown::{self, BlockType, LineDecoration};
use crate::theme::{Theme, ThemeKind};

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

// ── Layout constants ────────────────────────────────────────────────
// NOTE: Numeric layout/style constants are now in style.rs (Styles struct).
// Only non-style constants remain here.

// ── Cursor blink constants ──────────────────────────────────────────
// NOTE: Cursor timing is now in style.rs (CursorStyles).

// ── Editor element constants ────────────────────────────────────────
// NOTE: Element layout constants are now in style.rs.

// ── SVG asset paths ─────────────────────────────────────────────────

pub(crate) const HASH_SVG_PATH: &str = "assets/icons/hash.svg";
pub(crate) const COMMENT_SVG_PATH: &str = "assets/icons/comment.svg";
pub(crate) const CHECK_SVG_PATH: &str = "assets/icons/check.svg";
pub(crate) const FOLD_SVG_PATH: &str = "assets/icons/fold.svg";
pub(crate) const OPEN_SVG_PATH: &str = "assets/icons/open.svg";
pub(crate) const LOGO_SVG_PATH: &str = "assets/icons/logo.svg";
pub(crate) const ROOT_SVG_PATH: &str = "assets/icons/filetree/all.svg";
pub(crate) const FILETREE_FOLD_SVG_PATH: &str = "assets/icons/filetree/fold.svg";
pub(crate) const SIDEBAR_TOGGLE_SVG_PATH: &str = "assets/icons/sidebar-toggle.svg";
pub(crate) const EDITOR_ALIGN_LEFT_SVG_PATH: &str = "assets/icons/editor-align-left.svg";
pub(crate) const THEME_LIGHT_SVG_PATH: &str = "assets/icons/theme-light.svg";
pub(crate) const THEME_DARK_SVG_PATH: &str = "assets/icons/theme-dark.svg";
pub(crate) const FOLDER_OPEN_SVG_PATH: &str = "assets/icons/filetree/folder-open.svg";
pub(crate) const FOLDER_CLOSED_SVG_PATH: &str = "assets/icons/filetree/folder-closed.svg";
pub(crate) const MARKDOWN_SVG_PATH: &str = "assets/icons/filetree/markdown.svg";
pub(crate) const WORKSPACE_EXPAND_SVG_PATH: &str = "assets/icons/filetree/workspace-expand.svg";
pub(crate) const OUTLINE_TOGGLE_SVG_PATH: &str = "assets/icons/outline-toggle.svg";

// ── Format menu SVG icons ───────────────────────────────────────────
pub(crate) const FMT_BOLD_SVG: &str = "assets/icons/format/bold.svg";
pub(crate) const FMT_ITALIC_SVG: &str = "assets/icons/format/italic.svg";
pub(crate) const FMT_STRIKETHROUGH_SVG: &str = "assets/icons/format/strikethrough.svg";
pub(crate) const FMT_INLINE_CODE_SVG: &str = "assets/icons/format/inline-code.svg";
pub(crate) const FMT_CODE_SVG: &str = "assets/icons/format/code.svg";
pub(crate) const FMT_QUOTE_SVG: &str = "assets/icons/format/quote.svg";
pub(crate) const FMT_LIST_SVG: &str = "assets/icons/format/list.svg";
pub(crate) const FMT_ORDER_LIST_SVG: &str = "assets/icons/format/order-list.svg";
pub(crate) const FMT_TODO_SVG: &str = "assets/icons/format/todo.svg";
pub(crate) const FMT_LINK_SVG: &str = "assets/icons/format/link.svg";
pub(crate) const FMT_IMAGE_SVG: &str = "assets/icons/format/image.svg";
pub(crate) const FMT_HEAD_SVG: &str = "assets/icons/format/head.svg";

// ── UI constants ────────────────────────────────────────────────────
// NOTE: UI dimension constants are now in style.rs.

// ── Data types ──────────────────────────────────────────────────────

#[derive(Clone)]
pub(crate) struct EditorSnapshot {
    pub text: String,
    pub cursor_offset: usize,
    pub cursor_anchor: usize,
    pub scroll_offset: f32,
}

#[derive(Clone)]
pub(crate) struct OpenTab {
    pub id: String,
    pub title: String,
    pub path: Option<PathBuf>,
    pub snapshot: EditorSnapshot,
    pub is_dirty: bool,
    pub is_deleted: bool,
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
    Workspace,
}

#[derive(Clone, Copy, PartialEq)]
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

// ── Sidebar drag-and-drop types ─────────────────────────────────────

#[derive(Clone)]
pub(crate) struct DraggedSidebarItems {
    pub paths: Vec<PathBuf>,
}

pub(crate) struct DraggedSidebarItemsView {
    pub label: String,
    pub count: usize,
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
            .px(px(8.0))
            .py(px(4.0))
            .bg(gpui::rgb(0x2d2d2d))
            .text_color(gpui::rgb(0xcccccc))
            .text_size(px(13.0))
            .rounded(px(4.0))
            .shadow_md()
            .child(text)
    }
}

#[derive(Clone)]
pub(crate) enum DialogAction {
    CloseTabUnsaved { tab_id: String },
    SaveAndCloseTab { tab_id: String },
    DeletePath { path: PathBuf },
}

#[derive(Clone)]
pub(crate) struct Dialog {
    pub title: String,
    pub message: String,
    pub save_label: Option<String>,
    pub confirm_label: String,
    pub cancel_label: String,
    pub is_danger: bool,
    pub action: DialogAction,
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
    pub(crate) extra_cursors: Vec<Cursor>,
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
    pub(crate) dialog: Option<Dialog>,
    /// 多选：最后一个选中的 focus 路径
    pub(crate) focused_sidebar_path: Option<PathBuf>,
    /// 多选锚点（shift 连续选择的起点）
    pub(crate) sidebar_selection_anchor: Option<PathBuf>,
    /// 拖拽悬停的目标目录（用于高亮和自动展开）
    pub(crate) sidebar_drag_hover_target: Option<PathBuf>,
    /// 拖拽悬停自动展开的延迟 task
    pub(crate) sidebar_drag_expand_task: Option<Task<()>>,
    pub(crate) sidebar_scroll_handle: ScrollHandle,
    pub(crate) workspaces: Vec<PathBuf>,
    pub(crate) _watcher_task: Option<Task<()>>,
    pub(crate) outline_visible: bool,
    pub(crate) outline_custom_width: Option<f32>,
    pub(crate) outline_resizing: bool,
    pub(crate) outline_scroll_handle: ScrollHandle,
    pub(crate) scroll_anim_epoch: usize,
    pub(crate) last_window_width: f32,
    pub(crate) last_window_height: f32,
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
        };
        let theme_kind = ThemeKind::OneDark;
        let theme = theme_kind.build();
        let styles = crate::style::Styles::from_theme(&theme);
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
            theme,
            styles,
            decorations,
            scroll_offset: 0.0,
            is_selecting: false,
            line_display_info: Vec::new(),
            last_viewport_height: 600.0,
            last_viewport_width: init_max_width,
            cursor_opacity: init_cursor_opacity,
            cursor_opacity_direction: -1.0,
            blink_epoch: 0,
            marked_range: None,
            checkbox_areas: Vec::new(),
            hovering_checkbox: false,
            image_click_areas: Vec::new(),
            image_cache: HashMap::new(),
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            sidebar_root: sidebar_root.clone(),
            expanded_dirs,
            selected_sidebar_paths: HashSet::new(),
            current_file_path: None,
            sidebar_error,
            tabs: vec![OpenTab {
                id: "welcome-1".to_string(),
                title: "欢迎使用".to_string(),
                path: None,
                snapshot: initial_snapshot,
                is_dirty: false,
                is_deleted: false,
            }],
            active_tab_id: Some("welcome-1".to_string()),
            next_tab_id: 2,
            last_editor_left: 0.0,
            last_editor_width: init_max_width,
            last_content_left: init_padding_left,
            last_text_width: init_max_width - init_padding_left - init_padding_right,
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
            dialog: None,
            focused_sidebar_path: None,
            sidebar_selection_anchor: None,
            sidebar_drag_hover_target: None,
            sidebar_drag_expand_task: None,
            sidebar_scroll_handle: ScrollHandle::new(),
            workspaces: Vec::new(),
            _watcher_task: None,
            outline_visible: false,
            outline_custom_width: None,
            outline_resizing: false,
            outline_scroll_handle: ScrollHandle::new(),
            scroll_anim_epoch: 0,
            last_window_width: 1360.0,
            last_window_height: 820.0,
        };
        this.start_blinking(cx);
        this.start_watching(cx);

        this
    }

    pub(crate) fn queue_image_load(&mut self, path: String, cx: &mut Context<Self>) {
        if self.image_cache.contains_key(&path) {
            return;
        }

        self.image_cache.insert(path.clone(), None);
        cx.spawn(async move |this, cx| {
            let image_path = path.clone();
            let maybe_image = cx
                .background_executor()
                .spawn(async move {
                    if helpers::is_remote_image_url(&image_path) {
                        helpers::load_render_image_from_url(&image_path).ok()
                    } else {
                        helpers::resolve_local_image_path(&image_path)
                            .and_then(|resolved| helpers::load_render_image_from_path(&resolved).ok())
                    }
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
        self.theme_kind = self.theme_kind.toggle();
        self.theme = self.theme_kind.build();
        self.styles = crate::style::Styles::from_theme(&self.theme);
        cx.notify();
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

    pub(crate) fn handle_fs_events(&mut self, changed_paths: &[PathBuf], cx: &mut Context<Self>) {
        let mut needs_notify = false;

        for tab in &mut self.tabs {
            if let Some(ref tab_path) = tab.path {
                if changed_paths
                    .iter()
                    .any(|p| p == tab_path || tab_path.starts_with(p))
                {
                    if !tab_path.exists() {
                        if !tab.is_deleted {
                            tab.is_deleted = true;
                            needs_notify = true;
                        }
                    } else {
                        if tab.is_deleted {
                            tab.is_deleted = false;
                            needs_notify = true;
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
                                }
                            }
                        }
                    }
                }
            }
        }

        // Reload the active tab's buffer if it was externally modified
        if needs_notify {
            if let Some(ref active_id) = self.active_tab_id {
                if let Some(tab) = self.tabs.iter().find(|t| &t.id == active_id) {
                    if !tab.is_dirty && !tab.is_deleted {
                        let snapshot = tab.snapshot.clone();
                        self.buffer = crate::buffer::Buffer::from_text(&snapshot.text);
                        self.cursor.offset = snapshot.cursor_offset.min(self.buffer.len());
                        self.cursor.anchor = snapshot.cursor_anchor.min(self.buffer.len());
                        self.reparse();
                    }
                }
            }
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
                    }
                }
                self.close_tab(&tab_id, cx);
            }
            DialogAction::DeletePath { .. } => {
                // No save action for delete
            }
        }
        cx.notify();
    }

    pub(crate) fn render_dialog(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        use gpui::{CursorStyle, FontWeight, MouseButton, SharedString, div, prelude::*, px};

        let ds = &self.styles.dialog;
        let backdrop_color = ds.dialog_backdrop_color;
        let btn_hover_opacity = ds.dialog_btn_hover_opacity;

        let Some(ref dialog) = self.dialog else {
            return div().into_any_element();
        };

        let is_danger = dialog.is_danger;
        let btn_bg = if is_danger {
            ds.dialog_btn_danger_bg
        } else {
            ds.dialog_btn_primary_bg
        };
        let btn_text = if is_danger {
            ds.dialog_btn_danger_text_color
        } else {
            ds.dialog_btn_primary_text_color
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
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            this.dismiss_dialog(cx);
                                        }),
                                    ),
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
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, _, _, cx| {
                                                this.save_and_close_dialog(cx);
                                            }),
                                        ),
                                )
                            })
                            .child(
                                div()
                                    .id("dialog-confirm")
                                    .h(px(self.scaled(ds.dialog_btn_height_px)))
                                    .px(px(self.scaled(ds.dialog_btn_padding_x_px)))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .bg(btn_bg)
                                    .text_color(btn_text)
                                    .text_size(px(self.scaled(ds.dialog_btn_font_size_px)))
                                    .rounded(px(ds.dialog_btn_radius_px))
                                    .cursor(CursorStyle::Arrow)
                                    .hover(move |s| s.opacity(btn_hover_opacity))
                                    .child(confirm_label)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            this.confirm_dialog(cx);
                                        }),
                                    ),
                            ),
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
        self.decorations = markdown::parse_decorations(self.buffer.text());
        self.auto_align_table();
    }

    pub(crate) fn cursor_line(&self) -> usize {
        self.buffer.offset_to_line_col(self.cursor.offset).0
    }

    pub(crate) fn has_extra_cursors(&self) -> bool {
        !self.extra_cursors.is_empty()
    }

    pub(crate) fn clear_extra_cursors(&mut self) {
        self.extra_cursors.clear();
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
        let mut all: Vec<(usize, usize, bool)> = Vec::with_capacity(1 + self.extra_cursors.len());
        all.push((self.cursor.offset, self.cursor.anchor, true));
        for c in self.extra_cursors.iter() {
            all.push((c.offset, c.anchor, false));
        }
        // Sort by selection start, descending
        all.sort_by(|a, b| {
            let a_start = a.0.min(a.1);
            let b_start = b.0.min(b.1);
            b_start.cmp(&a_start)
        });

        let text_len = text.len();
        let mut new_positions: Vec<(usize, bool)> = Vec::new();

        for (offset, anchor, is_primary) in &all {
            let range_start = (*offset).min(*anchor);
            let range_end = (*offset).max(*anchor);
            self.buffer.replace(range_start..range_end, text);
            new_positions.push((range_start + text_len, *is_primary));
        }

        // Reverse to go ascending order for offset adjustments
        new_positions.reverse();

        // Assign positions back
        self.extra_cursors.clear();
        for (pos, is_primary) in &new_positions {
            let clamped = (*pos).min(self.buffer.len());
            if *is_primary {
                self.cursor.offset = clamped;
                self.cursor.anchor = clamped;
            } else {
                self.extra_cursors.push(Cursor {
                    offset: clamped,
                    anchor: clamped,
                });
            }
        }
    }

    /// Apply backspace across all cursors.
    pub(crate) fn multi_cursor_backspace(&mut self) {
        if self.extra_cursors.is_empty() {
            self.cursor.backspace(&mut self.buffer);
            return;
        }
        let mut all: Vec<(usize, usize, bool)> = Vec::with_capacity(1 + self.extra_cursors.len());
        all.push((self.cursor.offset, self.cursor.anchor, true));
        for c in &self.extra_cursors {
            all.push((c.offset, c.anchor, false));
        }
        all.sort_by(|a, b| {
            let a_start = a.0.min(a.1);
            let b_start = b.0.min(b.1);
            b_start.cmp(&a_start)
        });

        let mut new_positions: Vec<(usize, bool)> = Vec::new();

        for (offset, anchor, is_primary) in &all {
            if *offset != *anchor {
                let start = (*offset).min(*anchor);
                let end = (*offset).max(*anchor);
                self.buffer.delete(start..end);
                new_positions.push((start, *is_primary));
            } else if *offset > 0 {
                let text = self.buffer.text().to_string();
                let prev = text[..*offset]
                    .char_indices()
                    .next_back()
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                self.buffer.delete(prev..*offset);
                new_positions.push((prev, *is_primary));
            } else {
                new_positions.push((0, *is_primary));
            }
        }

        new_positions.reverse();

        self.extra_cursors.clear();
        for (pos, is_primary) in &new_positions {
            let clamped = (*pos).min(self.buffer.len());
            if *is_primary {
                self.cursor.offset = clamped;
                self.cursor.anchor = clamped;
            } else {
                self.extra_cursors.push(Cursor {
                    offset: clamped,
                    anchor: clamped,
                });
            }
        }
    }

    /// Apply delete (forward) across all cursors.
    pub(crate) fn multi_cursor_delete(&mut self) {
        if self.extra_cursors.is_empty() {
            self.cursor.delete(&mut self.buffer);
            return;
        }
        let mut all: Vec<(usize, usize, bool)> = Vec::with_capacity(1 + self.extra_cursors.len());
        all.push((self.cursor.offset, self.cursor.anchor, true));
        for c in &self.extra_cursors {
            all.push((c.offset, c.anchor, false));
        }
        all.sort_by(|a, b| {
            let a_start = a.0.min(a.1);
            let b_start = b.0.min(b.1);
            b_start.cmp(&a_start)
        });

        let mut new_positions: Vec<(usize, bool)> = Vec::new();

        for (offset, anchor, is_primary) in &all {
            if *offset != *anchor {
                let start = (*offset).min(*anchor);
                let end = (*offset).max(*anchor);
                self.buffer.delete(start..end);
                new_positions.push((start, *is_primary));
            } else if *offset < self.buffer.len() {
                let text = self.buffer.text().to_string();
                let next = text[*offset..]
                    .char_indices()
                    .nth(1)
                    .map(|(i, _)| *offset + i)
                    .unwrap_or(text.len());
                self.buffer.delete(*offset..next);
                new_positions.push((*offset, *is_primary));
            } else {
                new_positions.push((*offset, *is_primary));
            }
        }

        new_positions.reverse();

        self.extra_cursors.clear();
        for (pos, is_primary) in &new_positions {
            let clamped = (*pos).min(self.buffer.len());
            if *is_primary {
                self.cursor.offset = clamped;
                self.cursor.anchor = clamped;
            } else {
                self.extra_cursors.push(Cursor {
                    offset: clamped,
                    anchor: clamped,
                });
            }
        }
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

    pub(crate) fn current_snapshot(&self) -> EditorSnapshot {
        EditorSnapshot {
            text: self.buffer.text().to_string(),
            cursor_offset: self.cursor.offset,
            cursor_anchor: self.cursor.anchor,
            scroll_offset: self.scroll_offset,
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
