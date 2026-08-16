//! FileList pane: shows the markdown documents of the directory currently
//! selected in the FileTree, with a title / content preview / modified time
//! per item (Bear-style note list).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use chrono::{Datelike, Timelike};
use gpui::{
    AnyElement, App, Bounds, ContentMask, Context, Corners, CursorStyle, Element, ElementId,
    FontWeight, GlobalElementId, LayoutId, MouseButton, MouseDownEvent, Pixels, RenderImage,
    SharedString, Style, Window, div, prelude::*, px, relative,
};
use image::{Frame, RgbaImage};

use super::*;

/// Maximum number of bytes read from a file to extract title/preview.
const FILE_LIST_READ_LIMIT: usize = 16 * 1024;
/// Preview length in characters (markdown syntax stripped).
const FILE_LIST_PREVIEW_CHARS: usize = 120;

/// Centers an out-of-view item vertically; keeps fully visible items put.
fn scroll_y_to_center_uniform_item(
    current_scroll_y: f32,
    viewport_height: f32,
    prefix_height: f32,
    item_height: f32,
    index: usize,
    item_count: usize,
) -> Option<f32> {
    if viewport_height <= 0.0 || item_height <= 0.0 {
        return None;
    }
    let item_top = prefix_height + index as f32 * item_height;
    let item_bottom = item_top + item_height;
    if item_top >= current_scroll_y && item_bottom <= current_scroll_y + viewport_height {
        return None;
    }
    centered_scroll_y_for_uniform_item(
        viewport_height,
        prefix_height,
        item_height,
        index,
        item_count,
    )
}

/// Centers an item vertically regardless of its current visibility.
fn centered_scroll_y_for_uniform_item(
    viewport_height: f32,
    prefix_height: f32,
    item_height: f32,
    index: usize,
    item_count: usize,
) -> Option<f32> {
    if viewport_height <= 0.0 || item_height <= 0.0 {
        return None;
    }
    let item_top = prefix_height + index as f32 * item_height;
    let content_height = prefix_height + item_count as f32 * item_height;
    let max_scroll = (content_height - viewport_height).max(0.0);
    Some((item_top + item_height * 0.5 - viewport_height * 0.5).clamp(0.0, max_scroll))
}

#[derive(Clone, Debug)]
pub(crate) struct FileListItem {
    pub path: PathBuf,
    pub name: String,
    pub modified: SystemTime,
    pub created: SystemTime,
    pub details: Option<FileListDetails>,
}

#[derive(Clone, Debug)]
pub(crate) struct FileListDetails {
    /// Text of the first ATX heading in the document, if any.
    pub title: Option<String>,
    /// Plain-text preview with markdown syntax removed.
    pub preview: String,
    /// Markdown image targets, in document order.
    pub image_paths: Vec<String>,
}

struct FileListImageElement {
    image: Arc<RenderImage>,
    radius: Pixels,
}

impl IntoElement for FileListImageElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for FileListImageElement {
    type RequestLayoutState = ();
    type PrepaintState = Option<(Bounds<Pixels>, Arc<RenderImage>)>;

    fn id(&self) -> Option<ElementId> {
        None
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
        _window: &mut Window,
        _cx: &mut App,
    ) -> Self::PrepaintState {
        let box_width: f32 = bounds.size.width.into();
        let box_height: f32 = bounds.size.height.into();
        if box_width <= 0.0 || box_height <= 0.0 {
            return None;
        }
        Some((bounds, self.image.clone()))
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        _cx: &mut App,
    ) {
        let Some((image_bounds, image)) = prepaint else {
            return;
        };
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            let _ = window.paint_image(
                *image_bounds,
                Corners::all(self.radius),
                image.clone(),
                0,
                false,
            );
        });
    }
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
            let metadata = entry.metadata().ok();
            let modified = metadata
                .as_ref()
                .and_then(|meta| meta.modified().ok())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            let created = metadata
                .as_ref()
                .and_then(|meta| meta.created().ok())
                .unwrap_or(modified);
            FileListItem {
                path,
                name,
                modified,
                created,
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
            let image_paths = extract_image_paths(&text);
            (
                path,
                modified,
                FileListDetails {
                    title,
                    preview,
                    image_paths,
                },
            )
        })
        .collect()
}

fn extract_image_paths(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut paths = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars.get(i) == Some(&'!') && chars.get(i + 1) == Some(&'[') {
            if chars.get(i + 2) == Some(&'[') {
                i += 3;
                let start = i;
                while i + 1 < chars.len() && !(chars[i] == ']' && chars[i + 1] == ']') {
                    i += 1;
                }
                if i + 1 < chars.len() {
                    let target = chars[start..i].iter().collect::<String>();
                    if let Some(target) = clean_image_target(&target) {
                        paths.push(target);
                    }
                    i += 2;
                    continue;
                }
            } else {
                i += 2;
                while i < chars.len() && chars[i] != ']' {
                    i += 1;
                }
                if chars.get(i) == Some(&']') && chars.get(i + 1) == Some(&'(') {
                    i += 2;
                    let start = i;
                    while i < chars.len() && chars[i] != ')' {
                        i += 1;
                    }
                    if i < chars.len() {
                        let target = chars[start..i].iter().collect::<String>();
                        if let Some(target) = clean_image_target(&target) {
                            paths.push(target);
                        }
                        i += 1;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    paths
}

fn clean_image_target(target: &str) -> Option<String> {
    let target = target.trim();
    if target.is_empty() {
        return None;
    }
    let target = target
        .split_once('|')
        .map(|(path, _)| path)
        .unwrap_or(target)
        .trim();
    let target = target
        .split_once(char::is_whitespace)
        .map(|(path, _)| path)
        .unwrap_or(target)
        .trim_matches(['<', '>', '"', '\'']);
    (!target.is_empty()).then(|| target.to_string())
}

fn file_list_image_cache_key(markdown_path: &Path, image_path: &str) -> String {
    format!("file-list:{}\u{0}{}", markdown_path.display(), image_path)
}

fn square_crop_render_image(image: RenderImage) -> RenderImage {
    let size = image.size(0);
    let Ok(width) = u32::try_from(size.width.0) else {
        return image;
    };
    let Ok(height) = u32::try_from(size.height.0) else {
        return image;
    };
    let Some(bytes) = image.as_bytes(0) else {
        return image;
    };
    if width == 0 || height == 0 || bytes.len() < (width as usize) * (height as usize) * 4 {
        return image;
    }

    let side = width.min(height);
    let x0 = (width - side) / 2;
    let y0 = (height - side) / 2;
    let mut cropped = vec![0; (side as usize) * (side as usize) * 4];
    let source_stride = width as usize * 4;
    let dest_stride = side as usize * 4;
    for row in 0..side as usize {
        let source_start = (y0 as usize + row) * source_stride + x0 as usize * 4;
        let source_end = source_start + dest_stride;
        let dest_start = row * dest_stride;
        cropped[dest_start..dest_start + dest_stride]
            .copy_from_slice(&bytes[source_start..source_end]);
    }

    RgbaImage::from_raw(side, side, cropped)
        .map(|buffer| RenderImage::new([Frame::new(buffer)]))
        .unwrap_or(image)
}

fn preserve_file_list_details(previous: &[FileListItem], items: &mut [FileListItem]) {
    for item in items {
        if let Some(previous) = previous
            .iter()
            .find(|previous| previous.path == item.path && previous.modified == item.modified)
        {
            item.details = previous.details.clone();
        }
    }
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

/// Extracts the document title and builds a plain-text preview (markdown
/// syntax stripped) of up to `FILE_LIST_PREVIEW_CHARS` characters. A `title`
/// entry in the YAML front matter wins over the first ATX heading.
pub(crate) fn extract_title_and_preview(text: &str) -> (Option<String>, String) {
    let mut title: Option<String> = None;
    let mut preview = String::new();
    let mut preview_chars = 0usize;
    let mut lines = text.lines().peekable();

    // Skip YAML front matter (standard `---` or compact `-` delimiters),
    // capturing its `title:` value if present.
    let opening = lines
        .peek()
        .map(|line| line.trim_start_matches('\u{feff}').trim());
    let compact_delimiter = match opening {
        Some("---") => Some(false),
        Some("-") => Some(true),
        _ => None,
    };
    if let Some(compact) = compact_delimiter {
        lines.next();
        for line in lines.by_ref() {
            let trimmed = line.trim();
            let closed = if compact {
                trimmed == "-"
            } else {
                matches!(trimmed, "---" | "...")
            };
            if closed {
                break;
            }
            if title.is_none()
                && let Some(value) = front_matter_title_value(line)
            {
                title = Some(value);
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

/// Returns the value of a `title:` mapping entry in YAML front matter.
fn front_matter_title_value(line: &str) -> Option<String> {
    let content = line.trim();
    let colon = content.find(':')?;
    if !content[..colon].trim().eq_ignore_ascii_case("title") {
        return None;
    }
    let value = content[colon + 1..].trim();
    let value = value
        .strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
        })
        .unwrap_or(value)
        .trim();
    (!value.is_empty()).then(|| value.to_string())
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
            if self.ensure_sidebar_item_minimally_visible(index, items.len()) {
                cx.notify();
            }
        }
    }

    fn file_list_item_height_px(&self) -> f32 {
        let ss = &self.styles.sidebar;
        self.scaled(
            2.0 * ss.file_list_item_padding_y_px
                + self.file_list_item_content_height_px()
                + ss.file_list_item_gap_px
                + ss.file_list_item_time_line_height_px
                + ss.file_list_item_separator_width_px,
        )
    }

    fn file_list_item_content_height_px(&self) -> f32 {
        let ss = &self.styles.sidebar;
        ss.file_list_item_title_line_height_px
            + ss.file_list_item_gap_px
            + ss.file_list_item_preview_line_height_px * ss.file_list_item_preview_max_lines as f32
    }

    fn file_list_created_time_label(&self, created: SystemTime) -> String {
        let datetime: chrono::DateTime<chrono::Local> = created.into();
        let now = chrono::Local::now();
        if datetime.date_naive() == now.date_naive() {
            format!("{:02}:{:02}", datetime.hour(), datetime.minute())
        } else if datetime.year() == now.year() {
            format!("{:02}-{:02}", datetime.month(), datetime.day())
        } else {
            format!(
                "{:04}-{:02}-{:02}",
                datetime.year(),
                datetime.month(),
                datetime.day()
            )
        }
    }

    pub(crate) fn detailed_time_label(
        &self,
        key: crate::i18n::TextKey,
        time: SystemTime,
    ) -> String {
        let datetime: chrono::DateTime<chrono::Local> = time.into();
        let mut format = self.app_settings.date_time_format().to_string();
        if !format.contains("%S") && !format.contains("%T") {
            format.push_str(":%S");
        }
        format!("{} {}", self.t(key), datetime.format(&format))
    }

    fn file_list_preview_line_count(&self, preview: &str) -> usize {
        let max_lines = self.styles.sidebar.file_list_item_preview_max_lines;
        if preview.trim().is_empty() {
            0
        } else {
            max_lines
        }
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
            self.pending_file_list_scroll_top.set(None);
            self.file_list_pending_reveal_path = Some(path.to_path_buf());
        }
        if self.file_list_dir == dir {
            cx.notify();
            return;
        }
        self.file_list_dir = dir;
        self.file_list_dirty = true;
        self.pending_file_list_scroll_top.set(None);
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
        if self.pending_file_list_scroll_top.get().is_none() {
            self.pending_file_list_scroll_top.set(Some(
                (-f32::from(self.file_list_scroll_handle.offset().y)).max(0.0),
            ));
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
                let mut items = items;
                preserve_file_list_details(&this.file_list_items, &mut items);
                this.file_list_items = items;
                if this.file_list_items.is_empty() {
                    this.pending_file_list_scroll_top.set(None);
                    let offset = this.file_list_scroll_handle.offset();
                    this.file_list_scroll_handle
                        .set_offset(gpui::point(offset.x, px(0.0)));
                }
                this.file_list_loaded_dir = Some(loaded_dir);
                this.file_list_task = None;
                cx.notify();
            })
            .ok();
        });
        self.file_list_task = Some(task);
    }

    pub(crate) fn refresh_file_list_content_paths(
        &mut self,
        changed_paths: &[PathBuf],
        cx: &mut Context<Self>,
    ) {
        let mut pending = Vec::new();
        for item in &mut self.file_list_items {
            if !changed_paths.iter().any(|path| path == &item.path) {
                continue;
            }
            let Ok(modified) =
                std::fs::metadata(&item.path).and_then(|metadata| metadata.modified())
            else {
                continue;
            };
            item.modified = modified;
            pending.push((item.path.clone(), modified));
        }
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
                        crate::platform::window_chrome::toggle_maximize(window);
                    } else {
                        crate::platform::window_chrome::start_drag(window);
                    }
                }),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .min_w(px(0.0))
                            .text_size(px(self.scaled(ss.file_list_header_font_size_px)))
                            .text_color(ss.file_list_header_text_color)
                            .overflow_hidden()
                            .truncate()
                            .child(name),
                    )
                    .child(self.render_sidebar_count_badge(self.file_list_items.len(), true, 0.0)),
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
                            this.file_list_menu_origin = true;
                            this.start_create_file(create_parent.clone(), window, cx);
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

    fn file_list_item_image(
        &mut self,
        item: &FileListItem,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let details = item.details.as_ref()?;
        if details.image_paths.is_empty() {
            return None;
        }

        for image_path in &details.image_paths {
            let cache_key = file_list_image_cache_key(&item.path, image_path);
            match self.image_cache.get(&cache_key) {
                Some(Some(image)) => {
                    let ss = &self.styles.sidebar;
                    let image_size = self.scaled(self.file_list_item_content_height_px());
                    let image_radius = self.scaled(ss.file_list_item_thumbnail_radius_px);
                    return Some(
                        div()
                            .w(px(image_size))
                            .h(px(image_size))
                            .flex_none()
                            .overflow_hidden()
                            .rounded(px(image_radius))
                            .border(px(ss.file_list_item_thumbnail_border_width_px))
                            .border_color(ss.file_list_item_thumbnail_border_color)
                            .child(FileListImageElement {
                                image: image.clone(),
                                radius: px(image_radius),
                            })
                            .into_any_element(),
                    );
                }
                Some(None) => continue,
                None => {
                    self.queue_file_list_image_load(
                        item.path.clone(),
                        image_path.clone(),
                        cache_key,
                        cx,
                    );
                    return None;
                }
            }
        }

        None
    }

    fn queue_file_list_image_load(
        &mut self,
        markdown_path: PathBuf,
        image_path: String,
        cache_key: String,
        cx: &mut Context<Self>,
    ) {
        if self.image_cache.contains_key(&cache_key) {
            return;
        }

        self.image_cache.insert(cache_key.clone(), None);
        let workspace_root = self.sidebar_root.clone();
        let workspaces = self.workspaces.clone();
        cx.spawn(async move |this, cx| {
            let file_dir = markdown_path.parent().map(PathBuf::from);
            let load_path = if helpers::is_remote_image_url(&image_path) {
                image_path.clone()
            } else {
                attachments::WorkspaceLinkIndex::build(&workspace_root)
                    .resolve(Some(&markdown_path), &image_path)
                    .map(|path| path.to_string_lossy().into_owned())
                    .unwrap_or_else(|| image_path.clone())
            };
            let maybe_image = cx
                .background_executor()
                .spawn(async move {
                    helpers::load_image(&load_path, file_dir.as_ref(), &workspaces)
                        .ok()
                        .map(square_crop_render_image)
                        .map(Arc::new)
                })
                .await;

            this.update(cx, |this, cx| {
                this.image_cache.insert(cache_key, maybe_image);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn render_file_list_item(&mut self, item: &FileListItem, cx: &mut Context<Self>) -> AnyElement {
        if let Some(SidebarEditMode::Rename {
            path: ref rename_path,
            ref editor,
        }) = self.sidebar_edit
            && self.sidebar_edit_in_file_list
            && *rename_path == item.path
        {
            return self.render_file_list_edit_row(editor);
        }

        let thumbnail = self.file_list_item_image(item, cx);
        let ss = &self.styles.sidebar;
        let path = item.path.clone();
        let is_selected = self.is_sidebar_item_selected(&path);
        let is_active = self.current_file_path.as_deref() == Some(path.as_path());
        let row_bg = if is_selected || is_active {
            ss.file_list_item_selected_bg
        } else {
            ss.file_list_item_bg
        };
        let hover_bg = ss.file_list_item_hover_bg;
        let title_color = if is_selected || is_active {
            ss.file_list_item_selected_title_color
        } else {
            ss.file_list_item_title_color
        };
        let (title, title_suffix) =
            if self.file_list_display_mode == super::FileListDisplayMode::FileName {
                let (base, suffix) = split_file_name_for_display(&item.name);
                (base.to_string(), suffix.map(str::to_string))
            } else {
                let title = item
                    .details
                    .as_ref()
                    .and_then(|details| details.title.clone())
                    .unwrap_or_else(|| split_file_name_for_display(&item.name).0.to_string());
                (title, None)
            };
        let modified_time_label =
            crate::i18n::format_relative_time(self.ui_language, item.modified);
        let created_time_label = self.file_list_created_time_label(item.created);
        let modified_tooltip =
            self.detailed_time_label(crate::i18n::TextKey::InfoModified, item.modified);
        let created_tooltip =
            self.detailed_time_label(crate::i18n::TextKey::InfoCreated, item.created);
        let preview = item
            .details
            .as_ref()
            .map(|details| details.preview.clone())
            .unwrap_or_default();
        let preview_line_count = self.file_list_preview_line_count(&preview);
        let preview_height =
            self.scaled(ss.file_list_item_preview_line_height_px * preview_line_count as f32);
        let click_path = path.clone();
        let context_menu_path = path.clone();
        let drag_paths = if self.selected_sidebar_paths.contains(&path) {
            let mut paths = self
                .selected_sidebar_paths
                .iter()
                .cloned()
                .collect::<Vec<_>>();
            paths.sort();
            paths
        } else {
            vec![path.clone()]
        };
        let drag_count = drag_paths.len();
        let drag_label = item.name.clone();

        div()
            .id(SharedString::from(format!("file-list-{}", path.display())))
            .w_full()
            .h(px(self.file_list_item_height_px()))
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
                    .items_start()
                    .gap(px(self.scaled(ss.file_list_item_thumbnail_gap_px)))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .flex()
                            .flex_col()
                            .gap(px(self.scaled(ss.file_list_item_gap_px)))
                            .child(
                                div()
                                    .w_full()
                                    .h(px(self.scaled(ss.file_list_item_title_line_height_px)))
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .min_w(px(0.0))
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w(px(0.0))
                                            .overflow_hidden()
                                            .flex()
                                            .items_center()
                                            .text_size(px(
                                                self.scaled(ss.file_list_item_title_font_size_px)
                                            ))
                                            .line_height(px(
                                                self.scaled(ss.file_list_item_title_line_height_px)
                                            ))
                                            .text_color(title_color)
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .child(
                                                div()
                                                    .overflow_hidden()
                                                    .truncate()
                                                    .whitespace_nowrap()
                                                    .child(title),
                                            )
                                            .when_some(title_suffix, |this, suffix| {
                                                this.child(
                                                    div()
                                                        .flex_none()
                                                        .whitespace_nowrap()
                                                        .opacity(ss.sidebar_item_suffix_opacity)
                                                        .child(suffix),
                                                )
                                            }),
                                    ),
                            )
                            .when(preview_line_count > 0, |this| {
                                this.child(
                                    div()
                                        .w_full()
                                        .h(px(preview_height))
                                        .overflow_hidden()
                                        .text_size(px(
                                            self.scaled(ss.file_list_item_preview_font_size_px)
                                        ))
                                        .line_height(px(
                                            self.scaled(ss.file_list_item_preview_line_height_px)
                                        ))
                                        .text_color(ss.file_list_item_preview_color)
                                        .whitespace_normal()
                                        .line_clamp(ss.file_list_item_preview_max_lines)
                                        .child(preview.clone()),
                                )
                            }),
                    )
                    .when_some(thumbnail, |this, thumbnail| this.child(thumbnail)),
            )
            .child(
                div()
                    .w_full()
                    .h(px(self.scaled(ss.file_list_item_time_line_height_px)))
                    .flex()
                    .items_center()
                    .text_size(px(self.scaled(ss.file_list_item_time_font_size_px)))
                    .line_height(px(self.scaled(ss.file_list_item_time_line_height_px)))
                    .text_color(ss.file_list_item_time_color)
                    .whitespace_nowrap()
                    .child(tooltip::with_tooltip(
                        div()
                            .flex_none()
                            .whitespace_nowrap()
                            .child(modified_time_label),
                        cx.entity().clone(),
                        modified_tooltip,
                        None::<&'static str>,
                    ))
                    .child(div().flex_1().min_w(px(0.0)))
                    .child(tooltip::with_tooltip(
                        div()
                            .flex_none()
                            .whitespace_nowrap()
                            .child(created_time_label),
                        cx.entity().clone(),
                        created_tooltip,
                        None::<&'static str>,
                    )),
            )
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
            .when(!self.sidebar_resizing && !self.file_list_resizing, |this| {
                let ghost_font_size_px = self.scaled(self.styles.ui_font_size_px);
                let ghost_background_color = self.styles.tooltip.tooltip_bg;
                let ghost_text_color = self.styles.tooltip.tooltip_title_text_color;
                this.on_drag(
                    DraggedSidebarItems { paths: drag_paths },
                    move |_dragged, offset, _window, cx| {
                        cx.new(|_cx| DraggedSidebarItemsView {
                            label: drag_label.clone(),
                            count: drag_count,
                            font_size_px: ghost_font_size_px,
                            background_color: ghost_background_color,
                            text_color: ghost_text_color,
                            cursor_offset: offset,
                        })
                    },
                )
            })
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

        // Virtualized children contain spacers, so their child index does not
        // match the backing item index. Reveal by content geometry instead.
        if let Some(reveal_path) = self.file_list_pending_reveal_path.clone()
            && !self.file_list_dirty
            && self.file_list_task.is_none()
            && self.file_list_loaded_dir.as_deref() == Some(dir.as_path())
        {
            if let Some(index) = self
                .file_list_items
                .iter()
                .position(|item| item.path == reveal_path)
            {
                self.file_list_pending_reveal_path = None;
                let centered = self.file_list_pending_reveal_centered;
                self.file_list_pending_reveal_centered = false;
                let viewport_height = f32::from(self.file_list_scroll_handle.bounds().size.height);
                let current_scroll_y =
                    (-f32::from(self.file_list_scroll_handle.offset().y)).max(0.0);
                let next_scroll_y = if centered {
                    centered_scroll_y_for_uniform_item(
                        viewport_height,
                        prefix_height,
                        item_height,
                        index,
                        self.file_list_items.len(),
                    )
                } else {
                    scroll_y_to_center_uniform_item(
                        current_scroll_y,
                        viewport_height,
                        prefix_height,
                        item_height,
                        index,
                        self.file_list_items.len(),
                    )
                };
                if let Some(next_scroll_y) = next_scroll_y {
                    let offset = self.file_list_scroll_handle.offset();
                    self.file_list_scroll_handle
                        .set_offset(gpui::point(offset.x, px(-next_scroll_y)));
                }
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
        let refresh_ready = !self.file_list_dirty
            && self.file_list_task.is_none()
            && self.file_list_loaded_dir.as_deref() == Some(dir.as_path());
        let pending_scroll_top = self.pending_file_list_scroll_top.clone();
        let file_list_scroll_handle = self.file_list_scroll_handle.clone();

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
                    .flex_1()
                    .min_h(px(0.0))
                    .when(refresh_ready, |this| {
                        this.on_children_prepainted(move |_, _, _| {
                            let Some(scroll_top) = pending_scroll_top.take() else {
                                return;
                            };
                            let max_scroll = f32::from(file_list_scroll_handle.max_offset().height);
                            let offset = file_list_scroll_handle.offset();
                            file_list_scroll_handle
                                .set_offset(gpui::point(offset.x, px(-scroll_top.min(max_scroll))));
                        })
                    })
                    .child(
                        div()
                            .id("file-list-scroll")
                            .size_full()
                            .overflow_y_scroll()
                            .track_scroll(&self.file_list_scroll_handle)
                            .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                                this.pending_file_list_scroll_top.set(None);
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
                            ),
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

    use super::{
        FileListDetails, FileListItem, extract_image_paths, extract_title_and_preview,
        load_file_list_items, preserve_file_list_details, scroll_y_to_center_uniform_item,
    };
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
    fn reveal_does_not_scroll_an_already_visible_item() {
        assert_eq!(
            scroll_y_to_center_uniform_item(100.0, 200.0, 0.0, 50.0, 3, 20),
            None
        );
        assert_eq!(
            scroll_y_to_center_uniform_item(100.0, 200.0, 0.0, 50.0, 6, 20),
            Some(225.0)
        );
    }

    #[test]
    fn index_refresh_preserves_details_for_unchanged_files() {
        let modified = SystemTime::now();
        let path = std::path::PathBuf::from("note.md");
        let previous = vec![FileListItem {
            path: path.clone(),
            name: "note.md".to_string(),
            modified,
            created: modified,
            details: Some(FileListDetails {
                title: Some("Title".to_string()),
                preview: "Preview".to_string(),
                image_paths: Vec::new(),
            }),
        }];
        let mut refreshed = vec![FileListItem {
            path,
            name: "note.md".to_string(),
            modified,
            created: modified,
            details: None,
        }];

        preserve_file_list_details(&previous, &mut refreshed);

        assert_eq!(refreshed[0].details.as_ref().unwrap().preview, "Preview");
    }

    #[test]
    fn extracts_first_heading_and_strips_syntax() {
        let text = "---\ndate: 2026-01-01\n---\n# 小语文稿 **指南**\n\n这是一个 *优雅* 的 [写作](https://example.com) 工具，`Markdown` ~~删除~~ ==高亮==。";
        let (title, preview) = extract_title_and_preview(text);
        assert_eq!(title.as_deref(), Some("小语文稿 指南"));
        assert_eq!(preview, "这是一个 优雅 的 写作 工具，Markdown 删除 高亮。");
    }

    #[test]
    fn front_matter_title_overrides_first_heading() {
        let text = "---\ndate: 2026-01-01\ntitle: \"元数据标题\"\n---\n# 正文标题\n\n正文内容。";
        let (title, preview) = extract_title_and_preview(text);
        assert_eq!(title.as_deref(), Some("元数据标题"));
        assert_eq!(preview, "正文内容。");

        let compact = "-\ntitle: 紧凑标题\n-\n# 正文标题\n";
        let (title, _) = extract_title_and_preview(compact);
        assert_eq!(title.as_deref(), Some("紧凑标题"));
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
    fn extracts_markdown_and_wiki_image_paths_in_order() {
        let text = "正文 ![alt](assets/one.png)\n![[two.jpg|200]]\n![alt](<three.webp> \"title\")";
        assert_eq!(
            extract_image_paths(text),
            vec!["assets/one.png", "two.jpg", "three.webp"]
        );
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
