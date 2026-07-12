//! FileList pane: shows the markdown documents of the directory currently
//! selected in the FileTree, with a title / content preview / modified time
//! per item (Bear-style note list).

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use gpui::{
    AnyElement, Context, CursorStyle, FontWeight, MouseButton, MouseDownEvent, SharedString, div,
    prelude::*, px,
};

use super::*;

/// Maximum number of bytes read from a file to extract title/preview.
const FILE_LIST_READ_LIMIT: usize = 16 * 1024;
/// Preview length in characters (markdown syntax stripped).
const FILE_LIST_PREVIEW_CHARS: usize = 120;

#[derive(Clone, Debug)]
pub(crate) struct FileListItem {
    pub path: PathBuf,
    pub name: String,
    pub modified: SystemTime,
    pub details: Option<FileListDetails>,
}

#[derive(Clone, Debug)]
pub(crate) struct FileListDetails {
    /// Text of the first ATX heading in the document, if any.
    pub title: Option<String>,
    /// Plain-text preview with markdown syntax removed.
    pub preview: String,
}

/// Builds a lightweight index of the markdown files directly inside `dir`.
/// File contents are loaded separately when rows approach the viewport.
pub(crate) fn load_file_list_items(dir: &Path, sort_mode: SidebarSortMode) -> Vec<FileListItem> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut entries: Vec<std::fs::DirEntry> = entries
        .filter_map(Result::ok)
        .filter(|entry| {
            let path = entry.path();
            let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            !is_dir && !should_skip_path(&path, false) && is_markdown_file(&path)
        })
        .collect();
    entries.sort_by(|left, right| super::sidebar_tree::compare_dir_entries(sort_mode, left, right));

    entries
        .into_iter()
        .map(|entry| {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let modified = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            FileListItem {
                path,
                name,
                modified,
                details: None,
            }
        })
        .collect()
}

fn load_file_list_details(
    items: Vec<(PathBuf, SystemTime)>,
) -> Vec<(PathBuf, SystemTime, FileListDetails)> {
    items
        .into_iter()
        .map(|(path, modified)| {
            let text = read_file_head(&path);
            let (title, preview) = extract_title_and_preview(&text);
            (path, modified, FileListDetails { title, preview })
        })
        .collect()
}

/// Reads up to `FILE_LIST_READ_LIMIT` bytes and converts them lossily; a
/// trailing replacement char from a split UTF-8 sequence is dropped.
fn read_file_head(path: &Path) -> String {
    use std::io::Read;

    let Ok(file) = std::fs::File::open(path) else {
        return String::new();
    };
    let mut buffer = Vec::with_capacity(FILE_LIST_READ_LIMIT.min(4096));
    if file
        .take(FILE_LIST_READ_LIMIT as u64)
        .read_to_end(&mut buffer)
        .is_err()
    {
        return String::new();
    }
    let mut text = String::from_utf8_lossy(&buffer).into_owned();
    if text.ends_with('\u{fffd}') {
        text.pop();
    }
    text
}

/// Extracts the first ATX heading as the title and builds a plain-text preview
/// (markdown syntax stripped) of up to `FILE_LIST_PREVIEW_CHARS` characters.
pub(crate) fn extract_title_and_preview(text: &str) -> (Option<String>, String) {
    let mut title: Option<String> = None;
    let mut preview = String::new();
    let mut preview_chars = 0usize;
    let mut lines = text.lines().peekable();

    // Skip YAML front matter.
    if lines.peek().map(|line| line.trim()) == Some("---") {
        lines.next();
        for line in lines.by_ref() {
            if line.trim() == "---" {
                break;
            }
        }
    }

    let mut in_code_fence = false;
    let mut title_candidate_open = true;
    for line in lines {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_code_fence = !in_code_fence;
            continue;
        }
        if in_code_fence {
            continue;
        }
        if let Some(heading_text) = atx_heading_text(trimmed) {
            if title_candidate_open && title.is_none() {
                let stripped = strip_inline_markdown(heading_text);
                let stripped = stripped.trim();
                if !stripped.is_empty() {
                    title = Some(stripped.to_string());
                }
            }
            continue;
        }
        if trimmed.is_empty() || preview_chars >= FILE_LIST_PREVIEW_CHARS {
            if title.is_some() && preview_chars >= FILE_LIST_PREVIEW_CHARS {
                break;
            }
            continue;
        }
        title_candidate_open = false;
        let content = strip_block_prefix(trimmed);
        let stripped = strip_inline_markdown(content);
        let stripped = stripped.trim();
        if stripped.is_empty() {
            continue;
        }
        if preview_chars > 0 {
            preview.push(' ');
            preview_chars += 1;
        }
        for ch in stripped.chars() {
            if preview_chars >= FILE_LIST_PREVIEW_CHARS {
                break;
            }
            preview.push(ch);
            preview_chars += 1;
        }
    }

    (title, preview)
}

/// Returns the text of an ATX heading line (`# ` .. `###### `), if it is one.
fn atx_heading_text(trimmed: &str) -> Option<&str> {
    let hashes = trimmed.chars().take_while(|&ch| ch == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = &trimmed[hashes..];
    rest.strip_prefix(' ').map(str::trim)
}

/// Removes block-level markdown prefixes (quote, list, task markers).
fn strip_block_prefix(trimmed: &str) -> &str {
    let mut content = trimmed;
    while let Some(rest) = content.strip_prefix('>') {
        content = rest.trim_start();
    }
    if let Some(rest) = content
        .strip_prefix("- ")
        .or_else(|| content.strip_prefix("* "))
        .or_else(|| content.strip_prefix("+ "))
    {
        content = rest.trim_start();
    } else {
        let digits = content.chars().take_while(char::is_ascii_digit).count();
        if digits > 0 {
            let rest = &content[digits..];
            if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
                content = rest.trim_start();
            }
        }
    }
    if let Some(rest) = content
        .strip_prefix("[ ] ")
        .or_else(|| content.strip_prefix("[x] "))
        .or_else(|| content.strip_prefix("[X] "))
    {
        content = rest.trim_start();
    }
    content
}

/// Removes inline markdown syntax markers, keeping the visible text.
fn strip_inline_markdown(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];
        match ch {
            '*' | '`' => {
                i += 1;
            }
            '~' | '^' => {
                i += 1;
            }
            '=' if chars.get(i + 1) == Some(&'=') => {
                i += 2;
            }
            '+' if chars.get(i + 1) == Some(&'+') => {
                i += 2;
            }
            '_' if chars.get(i + 1) == Some(&'_') => {
                i += 2;
            }
            '!' if chars.get(i + 1) == Some(&'[') => {
                // Image: skip `![alt](url)` entirely.
                i += 2;
                while i < chars.len() && chars[i] != ']' {
                    i += 1;
                }
                i += 1;
                if chars.get(i) == Some(&'(') {
                    while i < chars.len() && chars[i] != ')' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            '[' if chars.get(i + 1) == Some(&'[') => {
                // Wiki link: keep the target name.
                i += 2;
                while i < chars.len() && !(chars[i] == ']' && chars.get(i + 1) == Some(&']')) {
                    out.push(chars[i]);
                    i += 1;
                }
                i += 2;
            }
            '[' => {
                // Link: keep the label, drop the URL part.
                i += 1;
                while i < chars.len() && chars[i] != ']' {
                    out.push(chars[i]);
                    i += 1;
                }
                i += 1;
                if chars.get(i) == Some(&'(') {
                    while i < chars.len() && chars[i] != ')' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            _ => {
                out.push(ch);
                i += 1;
            }
        }
    }
    out
}

impl EditorView {
    // ── Width / layout ──────────────────────────────────────────────

    pub(crate) fn file_list_target_width(&self) -> f32 {
        self.file_list_custom_width
            .unwrap_or_else(|| self.scaled(self.styles.sidebar.file_list_width_px))
    }

    pub(crate) fn toggle_file_list_visible(&mut self, cx: &mut Context<Self>) {
        let reveal_dir = self.file_list_effective_dir();
        self.file_list_visible = !self.file_list_visible;
        self.file_list_resizing = false;
        self.app_settings.file_list_visible = self.file_list_visible;
        self.app_settings.save();
        self.invalidate_sidebar_flattened_items();
        self.reveal_file_list_dir_in_sidebar(&reveal_dir, cx);
        if self.file_list_visible {
            self.file_list_dirty = true;
        } else {
            self.file_list_pending_reveal_path = None;
        }
        self.sync_file_list_width_animation(cx);
        cx.notify();
    }

    fn reveal_file_list_dir_in_sidebar(&mut self, dir: &Path, cx: &mut Context<Self>) {
        let items = self.sidebar_items();
        if let Some(index) = items.iter().position(|item| item.path == dir) {
            if self.ensure_sidebar_item_visible(index, items.len()) {
                cx.notify();
            }
        }
    }

    fn file_list_item_height_px(&self) -> f32 {
        let ss = &self.styles.sidebar;
        self.scaled(
            2.0 * ss.file_list_item_padding_y_px
                + ss.file_list_item_title_font_size_px
                    .max(ss.file_list_item_time_font_size_px)
                + ss.file_list_item_gap_px
                + ss.file_list_item_preview_line_height_px
                    * ss.file_list_item_preview_max_lines as f32
                + ss.file_list_item_separator_width_px,
        )
    }

    pub(crate) fn file_list_layout_width(&self) -> f32 {
        if self.file_list_resizing {
            self.file_list_display_width.max(0.0)
        } else if self.file_list_width_task.is_some() {
            self.file_list_layout_width.max(0.0)
        } else {
            self.file_list_display_width.max(0.0)
        }
    }

    pub(crate) fn sync_file_list_width_animation(&mut self, cx: &mut Context<Self>) {
        let target = if self.file_list_visible {
            self.file_list_target_width()
        } else {
            0.0
        };
        if (self.file_list_display_width - target).abs() <= 0.01
            && (self.file_list_width_target - target).abs() <= 0.01
        {
            self.file_list_display_width = target;
            self.file_list_width_target = target;
            return;
        }

        let start = self.file_list_display_width;
        self.file_list_width_target = target;
        self.file_list_layout_width = if target > start { target } else { start };
        self.file_list_width_epoch = self.file_list_width_epoch.wrapping_add(1);
        let epoch = self.file_list_width_epoch;
        let duration_ms = self.styles.sidebar.sidebar_visibility_animation_ms.max(1);
        self.file_list_width_task = Some(cx.spawn(async move |this, cx| {
            let started_at = std::time::Instant::now();
            loop {
                let t = (started_at.elapsed().as_secs_f32() * 1000.0 / duration_ms as f32)
                    .clamp(0.0, 1.0);
                let ease = ease_in_out_cubic(t);
                let done = this
                    .update(cx, |this, cx| {
                        if this.file_list_width_epoch != epoch {
                            return true;
                        }

                        this.file_list_display_width = start + (target - start) * ease;
                        if t >= 1.0 {
                            this.file_list_display_width = target;
                            this.file_list_width_task = None;
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

    /// Rendered width of the FileList pane. Follows the sidebar show/hide
    /// animation proportionally and collapses in workspace-search mode.
    pub(crate) fn file_list_width(&self) -> f32 {
        if self.workspace_search_visible {
            return 0.0;
        }
        if self.sidebar_root.as_os_str().is_empty() {
            return 0.0;
        }
        let sidebar_target = self.sidebar_target_width();
        if sidebar_target <= 0.0 {
            return 0.0;
        }
        let ratio = (self.sidebar_width() / sidebar_target).clamp(0.0, 1.0);
        self.file_list_display_width.max(0.0) * ratio
    }

    pub(crate) fn finish_file_list_resize(&mut self, cx: &mut Context<Self>) {
        if !self.file_list_resizing {
            return;
        }
        self.file_list_resizing = false;
        self.file_list_resize_grab_offset_x = 0.0;
        self.app_settings.file_list_width = self.file_list_custom_width;
        self.app_settings.save();
        cx.notify();
    }

    // ── Current directory ───────────────────────────────────────────

    /// The directory the FileList shows, validated against the workspace root.
    pub(crate) fn file_list_effective_dir(&self) -> PathBuf {
        let dir = &self.file_list_dir;
        if !dir.as_os_str().is_empty() && dir.starts_with(&self.sidebar_root) && dir.is_dir() {
            dir.clone()
        } else {
            self.sidebar_root.clone()
        }
    }

    /// Points the FileList at `path` (a directory, or the parent when given a
    /// file). Called from FileTree clicks and tab-activation sync. When `path`
    /// is a markdown file its FileList item is scrolled into view once the
    /// directory's items are loaded.
    pub(crate) fn set_file_list_dir_for_path(&mut self, path: &Path, cx: &mut Context<Self>) {
        let dir = if path.is_dir() {
            path.to_path_buf()
        } else {
            path.parent()
                .map(Path::to_path_buf)
                .unwrap_or_else(|| self.sidebar_root.clone())
        };
        if !path.is_dir() && is_markdown_file(path) {
            self.file_list_pending_reveal_path = Some(path.to_path_buf());
        }
        if self.file_list_dir == dir {
            cx.notify();
            return;
        }
        self.file_list_dir = dir;
        self.file_list_dirty = true;
        self.file_list_scroll_handle
            .set_offset(gpui::point(px(0.0), px(0.0)));
        cx.notify();
    }

    // ── Data loading ────────────────────────────────────────────────

    pub(crate) fn refresh_file_list_if_needed(&mut self, cx: &mut Context<Self>) {
        let dir = self.file_list_effective_dir();
        if dir.as_os_str().is_empty() {
            return;
        }
        if !self.file_list_dirty && self.file_list_loaded_dir.as_deref() == Some(dir.as_path()) {
            return;
        }
        self.file_list_dirty = false;
        self.file_list_epoch = self.file_list_epoch.wrapping_add(1);
        self.file_list_detail_task = None;
        let epoch = self.file_list_epoch;
        let sort_mode = self.sidebar_sort_mode;
        let loaded_dir = dir.clone();
        let task = cx.spawn(async move |this, cx| {
            let background_dir = loaded_dir.clone();
            let items = cx
                .background_executor()
                .spawn(async move { load_file_list_items(&background_dir, sort_mode) })
                .await;
            this.update(cx, |this, cx| {
                if this.file_list_epoch != epoch {
                    return;
                }
                this.file_list_items = items;
                this.file_list_loaded_dir = Some(loaded_dir);
                this.file_list_task = None;
                cx.notify();
            })
            .ok();
        });
        self.file_list_task = Some(task);
    }

    fn load_file_list_details_for_range(
        &mut self,
        range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) {
        if self.file_list_detail_task.is_some() || range.is_empty() {
            return;
        }
        let pending = self.file_list_items[range]
            .iter()
            .filter(|item| item.details.is_none())
            .map(|item| (item.path.clone(), item.modified))
            .collect::<Vec<_>>();
        if pending.is_empty() {
            return;
        }

        let epoch = self.file_list_epoch;
        self.file_list_detail_task = Some(cx.spawn(async move |this, cx| {
            let details = cx
                .background_executor()
                .spawn(async move { load_file_list_details(pending) })
                .await;
            this.update(cx, |this, cx| {
                if this.file_list_epoch != epoch {
                    return;
                }
                for (path, modified, detail) in details {
                    if let Some(item) = this
                        .file_list_items
                        .iter_mut()
                        .find(|item| item.path == path && item.modified == modified)
                    {
                        item.details = Some(detail);
                    }
                }
                this.file_list_detail_task = None;
                cx.notify();
            })
            .ok();
        }));
    }

    // ── Rendering ───────────────────────────────────────────────────

    fn render_file_list_header(&self, dir: &Path, cx: &mut Context<Self>) -> AnyElement {
        let ss = &self.styles.sidebar;
        let name = if dir == self.sidebar_root.as_path() {
            self.sidebar_root_name().to_uppercase()
        } else {
            dir.file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_else(|| dir.display().to_string())
        };
        let create_parent = dir.to_path_buf();

        div()
            .w_full()
            .h(px(self.scaled(ss.file_list_header_height_px)))
            .pl(px(self.scaled(ss.file_list_header_padding_left_px)))
            .pr(px(self.scaled(ss.file_list_header_padding_right_px)))
            .flex()
            .items_center()
            .gap(px(self.scaled(ss.file_list_header_gap_px)))
            .flex_none()
            .border_b(px(ss.file_list_header_border_width_px))
            .border_color(ss.file_list_header_border_color)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_this, event: &MouseDownEvent, window, _cx| {
                    if event.click_count >= 2 {
                        window.zoom_window();
                    } else {
                        super::perform_window_drag(window);
                    }
                }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .text_size(px(self.scaled(ss.file_list_header_font_size_px)))
                    .text_color(ss.file_list_header_text_color)
                    .overflow_hidden()
                    .truncate()
                    .child(name),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(ss.sidebar_header_action_gap_px)))
                    .child(self.render_sidebar_header_action_button(
                        "file-list-header-sort",
                        FILETREE_SORT_SVG_PATH,
                        crate::i18n::sidebar_action_label(self.ui_language, "sort"),
                        None,
                        cx,
                        |this, event, _window, cx| {
                            this.open_sidebar_sort_menu(event.position, cx);
                        },
                    ))
                    .child(self.render_sidebar_header_action_button(
                        "file-list-header-create",
                        FILETREE_ADD_SVG_PATH,
                        crate::i18n::sidebar_action_label(self.ui_language, "create"),
                        None,
                        cx,
                        move |this, _event, window, cx| {
                            this.start_create_file(create_parent.clone(), window, cx);
                            this.sidebar_edit_in_file_list = true;
                        },
                    )),
            )
            .into_any_element()
    }

    fn render_file_list_edit_row(
        &self,
        editor: &gpui::Entity<crate::text_input::TextInput>,
    ) -> AnyElement {
        let ss = &self.styles.sidebar;
        // Offset the container by the input's own horizontal padding so the
        // typed text lines up with the item titles below.
        let padding_x = self
            .scaled((ss.file_list_item_padding_x_px - ss.sidebar_edit_input_padding_x_px).max(0.0));
        div()
            .w_full()
            .flex_none()
            .px(px(padding_x))
            .py(px(self.scaled(ss.file_list_item_padding_y_px)))
            .bg(ss.file_list_item_selected_bg)
            .child(div().w_full().child(editor.clone()))
            .on_mouse_down(MouseButton::Left, |_event, _window, _cx| {})
            .into_any_element()
    }

    fn render_file_list_item(&self, item: &FileListItem, cx: &mut Context<Self>) -> AnyElement {
        if let Some(SidebarEditMode::Rename {
            path: ref rename_path,
            ref editor,
        }) = self.sidebar_edit
            && self.sidebar_edit_in_file_list
            && *rename_path == item.path
        {
            return self.render_file_list_edit_row(editor);
        }

        let ss = &self.styles.sidebar;
        let path = item.path.clone();
        let is_selected = self.is_sidebar_item_selected(&path);
        let is_active = self.current_file_path.as_deref() == Some(path.as_path());
        let row_bg = if is_selected || is_active {
            ss.file_list_item_selected_bg
        } else {
            gpui::transparent_black()
        };
        let hover_bg = ss.file_list_item_hover_bg;
        let title_color = if is_selected || is_active {
            ss.file_list_item_selected_title_color
        } else {
            ss.file_list_item_title_color
        };
        let title = item
            .details
            .as_ref()
            .and_then(|details| details.title.clone())
            .clone()
            .unwrap_or_else(|| split_file_name_for_display(&item.name).0.to_string());
        let time_label = crate::i18n::format_relative_time(self.ui_language, item.modified);
        let preview_max_height = self.scaled(
            ss.file_list_item_preview_line_height_px * ss.file_list_item_preview_max_lines as f32,
        );
        let click_path = path.clone();
        let context_menu_path = path.clone();

        div()
            .id(SharedString::from(format!("file-list-{}", path.display())))
            .w_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(self.scaled(ss.file_list_item_gap_px)))
            .px(px(self.scaled(ss.file_list_item_padding_x_px)))
            .py(px(self.scaled(ss.file_list_item_padding_y_px)))
            // Uniform dashed separator on every row (selection never changes
            // the border, so selecting an item cannot shift the layout).
            .border_b(px(ss.file_list_item_separator_width_px))
            .border_dashed()
            .border_color(ss.file_list_item_separator_color)
            .bg(row_bg)
            .cursor(CursorStyle::Arrow)
            .when(!self.sidebar_resizing && !self.file_list_resizing, |this| {
                this.hover(move |style| style.bg(hover_bg))
            })
            .child(
                div()
                    .w_full()
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(ss.file_list_item_title_gap_px)))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .overflow_hidden()
                            .truncate()
                            .whitespace_nowrap()
                            .text_size(px(self.scaled(ss.file_list_item_title_font_size_px)))
                            .text_color(title_color)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(self.scaled(ss.file_list_item_time_font_size_px)))
                            .text_color(ss.file_list_item_time_color)
                            .whitespace_nowrap()
                            .child(time_label),
                    ),
            )
            .child({
                let preview = item
                    .details
                    .as_ref()
                    .map(|details| details.preview.clone())
                    .unwrap_or_default();
                div()
                    .w_full()
                    .h(px(preview_max_height))
                    .overflow_hidden()
                    .text_size(px(self.scaled(ss.file_list_item_preview_font_size_px)))
                    .line_height(px(self.scaled(ss.file_list_item_preview_line_height_px)))
                    .text_color(ss.file_list_item_preview_color)
                    .child(preview)
            })
            .on_click(
                cx.listener(move |this, event: &gpui::ClickEvent, window, cx| {
                    cx.stop_propagation();
                    let mods = event.modifiers();
                    let has_modifier = mods.platform || mods.shift;
                    this.sidebar_select_item(&click_path, mods.shift, mods.platform);
                    if !has_modifier {
                        // Unlike FileTree clicks this does not suppress the
                        // sidebar sync: activating the tab should reveal the
                        // file in the FileTree so the two panes stay linked.
                        if event.click_count() >= 2 {
                            this.focused_sidebar_path = None;
                            this.open_path_in_permanent_tab(click_path.clone(), true, cx);
                            this.move_cursor_to_first_line_end();
                            this.request_editor_focus(cx);
                        } else if !this.switch_to_existing_editor_tab_for_path(
                            &click_path,
                            false,
                            cx,
                        ) {
                            this.open_path_in_preview_tab(click_path.clone(), false, cx);
                        }
                    }
                    window.prevent_default();
                    cx.notify();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    if !this.selected_sidebar_paths.contains(&context_menu_path) {
                        this.sidebar_select_item(&context_menu_path, false, false);
                    }
                    this.focused_sidebar_path = Some(context_menu_path.clone());
                    this.open_file_list_context_menu(context_menu_path.clone(), event.position, cx);
                    this.file_list_menu_origin = true;
                }),
            )
            .into_any_element()
    }

    pub(crate) fn render_file_list(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.refresh_file_list_if_needed(cx);

        let ss = self.styles.sidebar.clone();
        let dir = self.file_list_effective_dir();

        let create_editor = match &self.sidebar_edit {
            Some(SidebarEditMode::CreateFile { parent, editor })
                if self.sidebar_edit_in_file_list && *parent == dir =>
            {
                Some(editor.clone())
            }
            _ => None,
        };

        let mut list_children: Vec<AnyElement> = Vec::new();
        if let Some(editor) = &create_editor {
            list_children.push(self.render_file_list_edit_row(editor));
        }
        let item_height = self.file_list_item_height_px();
        let prefix_height = if create_editor.is_some() {
            item_height
        } else {
            0.0
        };
        let list_window = super::list_viewport::fixed_list_window(
            &self.file_list_scroll_handle,
            self.last_window_height,
            prefix_height,
            item_height,
            self.file_list_items.len(),
            8,
            16,
        );
        self.load_file_list_details_for_range(list_window.preload.clone(), cx);
        if list_window.top_spacer_px > 0.0 {
            list_children.push(
                div()
                    .w_full()
                    .h(px(list_window.top_spacer_px))
                    .flex_none()
                    .into_any_element(),
            );
        }
        for idx in list_window.render.clone() {
            let item = self.file_list_items[idx].clone();
            list_children.push(self.render_file_list_item(&item, cx));
        }
        if list_window.bottom_spacer_px > 0.0 {
            list_children.push(
                div()
                    .w_full()
                    .h(px(list_window.bottom_spacer_px))
                    .flex_none()
                    .into_any_element(),
            );
        }

        // Scroll the linked item (active tab / FileTree selection) into view
        // once this directory's items are loaded. `scroll_to_item` records the
        // target index and resolves it during this frame's prepaint, after the
        // fresh child bounds have been captured.
        if let Some(reveal_path) = self.file_list_pending_reveal_path.clone()
            && !self.file_list_dirty
            && self.file_list_task.is_none()
            && self.file_list_loaded_dir.as_deref() == Some(dir.as_path())
        {
            self.file_list_pending_reveal_path = None;
            if let Some(index) = self
                .file_list_items
                .iter()
                .position(|item| item.path == reveal_path)
            {
                let child_index = index + usize::from(create_editor.is_some());
                self.file_list_scroll_handle.scroll_to_item(child_index);
            }
        }

        let is_empty = list_children.is_empty();
        let file_list_scrollbar = self.render_scrollbar_for_handle(
            PanelScrollbarKind::FileList,
            &self.file_list_scroll_handle,
            self.last_window_height,
            0.0,
            self.scaled(self.styles.editor.editor_scrollbar_right_margin_px),
            cx,
        );

        div()
            .id("file-list")
            .w(px(self.file_list_layout_width()))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .relative()
            .bg(ss.file_list_bg)
            .border_r(px(ss.sidebar_resize_indicator_width_px))
            .border_color(ss.sidebar_resize_indicator_color)
            .child(self.render_file_list_header(&dir, cx))
            .child(if is_empty {
                let menu_dir = dir.clone();
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(self.scaled(ss.file_list_empty_font_size_px)))
                    .text_color(ss.file_list_empty_text_color)
                    .child(self.t(crate::i18n::TextKey::FileListEmpty))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            window.prevent_default();
                            cx.stop_propagation();
                            this.open_sidebar_context_menu(
                                menu_dir.clone(),
                                true,
                                event.position,
                                cx,
                            );
                            this.file_list_menu_origin = true;
                        }),
                    )
                    .into_any_element()
            } else {
                let menu_dir = dir.clone();
                div()
                    .id("file-list-scroll")
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&self.file_list_scroll_handle)
                    .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                        this.reveal_panel_scrollbar(PanelScrollbarKind::FileList, cx);
                    }))
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            window.prevent_default();
                            cx.stop_propagation();
                            this.open_sidebar_context_menu(
                                menu_dir.clone(),
                                true,
                                event.position,
                                cx,
                            );
                            this.file_list_menu_origin = true;
                        }),
                    )
                    .children(list_children)
                    .child(
                        div()
                            .w_full()
                            .h(px(self.scaled(ss.sidebar_bottom_spacer_height_px)))
                            .flex_none(),
                    )
                    .into_any_element()
            })
            .when_some(file_list_scrollbar, |this, scrollbar| this.child(scrollbar))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, time::SystemTime};

    use super::{extract_title_and_preview, load_file_list_items};
    use crate::editor::SidebarSortMode;

    fn temp_dir(name: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "moeka-{name}-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn file_list_index_does_not_read_document_details() {
        let dir = temp_dir("file-list-index");
        fs::write(dir.join("note.md"), "# Expensive title\n\nPreview body").unwrap();

        let items = load_file_list_items(&dir, SidebarSortMode::NameAsc);

        assert_eq!(items.len(), 1);
        assert_eq!(items[0].name, "note.md");
        assert!(items[0].details.is_none());
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn extracts_first_heading_and_strips_syntax() {
        let text = "---\ntitle: meta\n---\n# 小语文稿 **指南**\n\n这是一个 *优雅* 的 [写作](https://example.com) 工具，`Markdown` ~~删除~~ ==高亮==。";
        let (title, preview) = extract_title_and_preview(text);
        assert_eq!(title.as_deref(), Some("小语文稿 指南"));
        assert_eq!(preview, "这是一个 优雅 的 写作 工具，Markdown 删除 高亮。");
    }

    #[test]
    fn preview_truncates_at_char_boundary_for_cjk() {
        let text = format!("# 标题\n\n{}", "汉".repeat(300));
        let (title, preview) = extract_title_and_preview(&text);
        assert_eq!(title.as_deref(), Some("标题"));
        assert_eq!(preview.chars().count(), 120);
    }

    #[test]
    fn strips_list_and_quote_prefixes_and_skips_fences() {
        let text = "- [x] 任务一\n> 引用文本\n```rust\nlet code = 1;\n```\n1. 有序项";
        let (title, preview) = extract_title_and_preview(text);
        assert_eq!(title, None);
        assert_eq!(preview, "任务一 引用文本 有序项");
    }

    #[test]
    fn title_requires_first_nonblank_body_line_to_be_heading() {
        let (title, preview) = extract_title_and_preview("正文先出现\n# 后续标题\n内容");
        assert_eq!(title, None);
        assert_eq!(preview, "正文先出现 内容");

        let (title, preview) = extract_title_and_preview("\n\n## 第一标题\n正文");
        assert_eq!(title.as_deref(), Some("第一标题"));
        assert_eq!(preview, "正文");
    }
}
