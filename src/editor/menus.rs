use std::collections::HashMap;
use std::io::Cursor;
use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, ClipboardItem, Context, CursorStyle, FontWeight, Image, ImageFormat, MouseButton,
    MouseDownEvent, MouseMoveEvent, Pixels, Point, SharedString, div, prelude::*, px, svg,
};

use crate::markdown::{BlockType, InlineKind, LineDecoration, raw_wiki_link_ranges};

use super::shortcut::{ShortcutRenderStyle, render_shortcut};
use super::*;

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

struct SlashMenuItem {
    label: String,
    filter_text: &'static str,
    icon: Option<&'static str>,
    shortcut: Option<SharedString>,
    action: FormatMenuAction,
}

#[derive(Default)]
pub(crate) struct SelectionToolbarMenuHover {
    regions: HashMap<&'static str, gpui::Hitbox>,
    open_region: Option<&'static str>,
    open_epoch: usize,
    open_task: Option<Task<()>>,
    close_task: Option<Task<()>>,
}

struct SelectionToolbarHoverRegion {
    child: AnyElement,
    editor: gpui::Entity<EditorView>,
    region: &'static str,
    menu_kind: Option<ContextMenuKind>,
}

impl IntoElement for SelectionToolbarHoverRegion {
    type Element = Self;

    fn into_element(self) -> Self {
        self
    }
}

impl gpui::Element for SelectionToolbarHoverRegion {
    type RequestLayoutState = ();
    type PrepaintState = gpui::Hitbox;

    fn id(&self) -> Option<gpui::ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> (gpui::LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) -> gpui::Hitbox {
        self.child.prepaint(window, cx);
        window.insert_hitbox(bounds, gpui::HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        _: Option<&gpui::GlobalElementId>,
        _: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        hitbox: &mut gpui::Hitbox,
        window: &mut gpui::Window,
        cx: &mut gpui::App,
    ) {
        self.child.paint(window, cx);
        let editor = self.editor.clone();
        let region = self.region;
        let menu_kind = self.menu_kind.clone();
        let hitbox = hitbox.clone();
        window.on_mouse_event(move |_: &MouseMoveEvent, phase, window, cx| {
            if phase != gpui::DispatchPhase::Capture {
                return;
            }
            let hovered = hitbox.is_hovered(window) && !cx.has_active_drag();
            editor.update(cx, |this, cx| {
                if hovered && let Some(kind) = &menu_kind {
                    if matches!(kind, ContextMenuKind::FormatHeading) {
                        this.schedule_selection_toolbar_menu(region, kind.clone(), bounds, cx);
                    } else {
                        this.cancel_selection_toolbar_menu_open(region);
                        this.open_selection_toolbar_menu(kind.clone(), Some(bounds), cx);
                    }
                } else if !hovered {
                    this.cancel_selection_toolbar_menu_open(region);
                }
                this.update_selection_toolbar_menu_hover(region, hovered, &hitbox, window, cx);
            });
        });
    }
}

#[derive(Clone, PartialEq)]
enum FormatToolbarEntry {
    HeadingMenu,
    MoreMenu,
    Action {
        action: FormatMenuAction,
        icon: &'static str,
        shortcut: Option<SharedString>,
    },
}

impl FormatToolbarEntry {
    fn menu_kind(&self) -> Option<ContextMenuKind> {
        match self {
            Self::HeadingMenu => Some(ContextMenuKind::FormatHeading),
            Self::MoreMenu => Some(ContextMenuKind::FormatMore),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
struct FormatToolbarMetrics {
    button: f32,
    gap: f32,
    padding: f32,
    border: f32,
}

impl FormatToolbarMetrics {
    fn width(self, entries: &[FormatToolbarEntry]) -> f32 {
        let content = entries.len() as f32 * self.button;
        content
            + entries.len().saturating_sub(1) as f32 * self.gap
            + 2.0 * (self.padding + self.border)
    }
}

fn format_toolbar_layout(metrics: FormatToolbarMetrics) -> ([Vec<FormatToolbarEntry>; 4], f32) {
    use FormatToolbarEntry::{HeadingMenu, MoreMenu};

    let hint = crate::editor::shortcut::hint;
    let action = |action, icon, shortcut: Option<SharedString>| FormatToolbarEntry::Action {
        action,
        icon,
        shortcut,
    };
    let first_row = vec![HeadingMenu];
    let second_row = vec![
        action(
            FormatMenuAction::Bold,
            FMT_BOLD_SVG,
            Some(hint("secondary-b")),
        ),
        action(
            FormatMenuAction::Italic,
            FMT_ITALIC_SVG,
            Some(hint("secondary-i")),
        ),
        action(
            FormatMenuAction::Strikethrough,
            FMT_STRIKETHROUGH_SVG,
            Some(hint("secondary-shift-x")),
        ),
        action(
            FormatMenuAction::Underline,
            FMT_UNDERLINE_SVG,
            Some(hint("secondary-u")),
        ),
        action(
            FormatMenuAction::InlineCode,
            FMT_INLINE_CODE_SVG,
            Some(hint("secondary-e")),
        ),
    ];
    let third_row = vec![
        action(
            FormatMenuAction::Todo,
            FMT_TODO_SVG,
            Some(hint("secondary-alt-enter")),
        ),
        action(
            FormatMenuAction::UnorderedList,
            FMT_LIST_SVG,
            Some(hint("secondary-alt-u")),
        ),
        action(
            FormatMenuAction::OrderedList,
            FMT_ORDER_LIST_SVG,
            Some(hint("secondary-alt-o")),
        ),
        action(
            FormatMenuAction::Link,
            FMT_LINK_SVG,
            Some(hint("secondary-k")),
        ),
        action(FormatMenuAction::Image, FMT_IMAGE_SVG, None),
    ];
    let fourth_row = vec![
        action(
            FormatMenuAction::Table,
            FMT_TABLE_SVG,
            Some(hint("secondary-alt-t")),
        ),
        action(
            FormatMenuAction::CodeBlock,
            FMT_CODE_SVG,
            Some(hint("secondary-alt-c")),
        ),
        action(
            FormatMenuAction::Quote,
            FMT_QUOTE_SVG,
            Some(hint("secondary-alt-q")),
        ),
        action(FormatMenuAction::HorizontalRule, FMT_SPLIT_SVG, None),
        MoreMenu,
    ];
    let width = metrics.width(&second_row);
    ([first_row, second_row, third_row, fourth_row], width)
}

fn aligned_submenu_top(
    trigger_top: f32,
    trigger_bottom: f32,
    submenu_height: f32,
    overlay_height: f32,
    margin: f32,
) -> f32 {
    let desired = if overlay_height - margin - trigger_top >= submenu_height {
        trigger_top
    } else if trigger_bottom - margin >= submenu_height {
        trigger_bottom - submenu_height
    } else {
        trigger_top
    };
    desired.clamp(
        margin,
        (overlay_height - margin - submenu_height).max(margin),
    )
}

fn toolbar_side_menu_position(
    toolbar_left: f32,
    toolbar_top: f32,
    toolbar_width: f32,
    toolbar_height: f32,
    menu_width: f32,
    menu_height: f32,
    overlay_width: f32,
    overlay_height: f32,
    gap: f32,
    margin: f32,
    align_bottom: bool,
) -> (f32, f32) {
    let toolbar_right = toolbar_left + toolbar_width;
    let right = toolbar_right + gap;
    let left = toolbar_left - gap - menu_width;
    let space_right = overlay_width - margin - right;
    let space_left = toolbar_left - gap - margin;
    let x = if space_right >= menu_width || space_right >= space_left {
        right
    } else {
        left
    }
    .clamp(margin, (overlay_width - margin - menu_width).max(margin));
    let desired_y = if align_bottom {
        toolbar_top + toolbar_height - menu_height
    } else {
        toolbar_top
    };
    let y = desired_y.clamp(margin, (overlay_height - margin - menu_height).max(margin));
    (x, y)
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

/// Innermost `[desc](url)` containing `col`, skipping image syntax (`![`).
fn markdown_link_ranges(
    line_content: &str,
    col: usize,
) -> Option<(Range<usize>, Range<usize>, Range<usize>)> {
    let bytes = line_content.as_bytes();
    let mut search_from = 0;
    while let Some(offset) = line_content[search_from..].find('[') {
        let full_start = search_from + offset;
        search_from = full_start + 1;
        if full_start > 0 && bytes[full_start - 1] == b'!' {
            continue;
        }
        let desc_start = full_start + 1;
        let Some(desc_len) = line_content[desc_start..].find("](") else {
            continue;
        };
        let desc_end = desc_start + desc_len;
        let url_start = desc_end + 2;
        let Some(url_len) = line_content[url_start..].find(')') else {
            continue;
        };
        let url_end = url_start + url_len;
        if (full_start..=url_end + 1).contains(&col) {
            return Some((
                full_start..url_end + 1,
                desc_start..desc_end,
                url_start..url_end,
            ));
        }
    }
    None
}

#[derive(Clone)]
pub(super) struct ImageMenuTarget {
    pub(super) line: usize,
    pub(super) full_range: Range<usize>,
    pub(super) desc_range: Range<usize>,
    pub(super) url_range: Range<usize>,
}

impl EditorView {
    fn suppress_editor_mouse_up_for_current_menu_event(&mut self, cx: &mut Context<Self>) {
        self.suppress_next_editor_mouse_up = true;
        let editor = cx.entity().clone();
        cx.defer(move |cx| {
            editor.update(cx, |this, _| {
                this.suppress_next_editor_mouse_up = false;
            });
        });
    }

    pub(crate) fn open_context_menu(
        &mut self,
        menu: ContextMenu,
        clear_sidebar_edit: bool,
        cx: &mut Context<Self>,
    ) {
        self.selection_toolbar_menu_hover = SelectionToolbarMenuHover::default();
        self.clear_selection_toolbar_menu_owner();
        self.context_menu = Some(menu);
        self.context_submenu = None;
        self.context_submenu_aim = None;
        self.context_submenu_hovered = false;
        self.format_heading_trigger_bounds.set(None);
        self.format_more_trigger_bounds.set(None);
        self.export_trigger_bounds.set(None);
        // Menus opened from the FileList set this back to true right after.
        self.file_list_menu_origin = false;
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
        self.selection_toolbar_menu_hover = SelectionToolbarMenuHover::default();
        self.clear_selection_toolbar_menu_owner();
        let had_menu = self.context_menu.take().is_some();
        let had_submenu = self.context_submenu.take().is_some();
        let had_submenu_aim = self.context_submenu_aim.take().is_some();
        let had_submenu_hovered = std::mem::replace(&mut self.context_submenu_hovered, false);
        let had_pane_target = self.context_menu_pane_target.take().is_some();
        self.table_size_hover = None;
        if had_menu || had_submenu || had_submenu_aim || had_submenu_hovered || had_pane_target {
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

    pub(crate) fn current_heading_action(&self) -> Option<FormatMenuAction> {
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
        self.copy_image_path_to_clipboard(&path, cx);
    }

    fn copy_image_path_to_clipboard(&mut self, path: &Path, cx: &mut Context<Self>) {
        let ext = path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.to_ascii_lowercase());
        let format = match ext.as_deref() {
            Some("png") => Some(ImageFormat::Png),
            Some("jpg" | "jpeg") => Some(ImageFormat::Jpeg),
            Some("webp") => Some(ImageFormat::Webp),
            Some("gif") => Some(ImageFormat::Gif),
            Some("svg") => Some(ImageFormat::Svg),
            Some("bmp") => Some(ImageFormat::Bmp),
            Some("tif" | "tiff") => Some(ImageFormat::Tiff),
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
        if let Ok(bytes) = std::fs::read(path)
            && let Ok(decoded) = image::load_from_memory(&bytes)
        {
            let mut png = Cursor::new(Vec::new());
            if decoded.write_to(&mut png, image::ImageFormat::Png).is_ok() {
                self.context_menu = None;
                cx.write_to_clipboard(ClipboardItem::new_image(&Image::from_bytes(
                    ImageFormat::Png,
                    png.into_inner(),
                )));
                cx.notify();
                return;
            }
        }
        self.copy_text_to_clipboard(path.to_string_lossy().into_owned(), cx);
    }

    fn current_markdown_image_path(&self) -> Option<PathBuf> {
        let raw_path = self.current_markdown_image_source()?;
        if helpers::is_remote_image_url(raw_path) {
            return None;
        }
        let file_dir = self
            .current_file_path
            .as_ref()
            .and_then(|path| path.parent().map(Path::to_path_buf));
        let mut roots = self.workspaces.clone();
        if !self.sidebar_root.as_os_str().is_empty() && !roots.contains(&self.sidebar_root) {
            roots.push(self.sidebar_root.clone());
        }
        helpers::resolve_image_to_path(raw_path, file_dir.as_ref(), &roots)
    }

    fn current_markdown_image_source(&self) -> Option<&str> {
        let target = self.current_image_menu_target()?;
        self.decorations.get(target.line)?.image_path.as_deref()
    }

    fn copy_markdown_image_to_clipboard(&mut self, source: String, cx: &mut Context<Self>) {
        if !helpers::is_remote_image_url(&source) {
            if let Some(path) = self.current_markdown_image_path() {
                self.copy_image_path_to_clipboard(&path, cx);
            } else {
                self.copy_text_to_clipboard(source, cx);
            }
            return;
        }

        self.context_menu = None;
        cx.spawn(async move |this, cx| {
            let source_for_download = source.clone();
            let image = cx
                .background_executor()
                .spawn(async move {
                    let response = ureq::get(&source_for_download).call().ok()?;
                    let mut bytes = Vec::new();
                    std::io::Read::read_to_end(&mut response.into_reader(), &mut bytes).ok()?;
                    let path = source_for_download
                        .split(['?', '#'])
                        .next()
                        .unwrap_or(source_for_download.as_str());
                    if path.to_ascii_lowercase().ends_with(".svg") {
                        return Some(Image::from_bytes(ImageFormat::Svg, bytes));
                    }
                    let decoded = image::load_from_memory(&bytes).ok()?;
                    let mut png = Cursor::new(Vec::new());
                    decoded.write_to(&mut png, image::ImageFormat::Png).ok()?;
                    Some(Image::from_bytes(ImageFormat::Png, png.into_inner()))
                })
                .await;

            let _ = this.update(cx, |editor, cx| {
                if let Some(image) = image {
                    cx.write_to_clipboard(ClipboardItem::new_image(&image));
                } else {
                    cx.write_to_clipboard(ClipboardItem::new_string(source));
                }
                editor.context_menu = None;
                cx.notify();
            });
        })
        .detach();
        cx.notify();
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
            current if current == crate::i18n::add_file_to_chat_label(self.ui_language) => {
                Some(MENU_AT_SVG_PATH)
            }
            current if current == crate::i18n::add_selection_to_chat_label(self.ui_language) => {
                Some(CHAT_SVG_PATH)
            }
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
        let key = match action {
            FormatMenuAction::Highlight => "highlight",
            FormatMenuAction::Underline => "underline",
            FormatMenuAction::Superscript => "superscript",
            FormatMenuAction::Subscript => "subscript",
            FormatMenuAction::Footnote => "footnote",
            FormatMenuAction::CalloutTip => "callout_tip",
            _ => "",
        };
        crate::i18n::format_extra_label(self.ui_language, key)
    }

    fn more_formats_menu_label(&self) -> &'static str {
        crate::i18n::format_extra_label(self.ui_language, "more_formats")
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
        if source_path != destination_path
            && let Err(error) = std::fs::copy(source_path, &destination_path)
        {
            let message = format!(
                "{}: {error}",
                self.t(crate::i18n::TextKey::FileOperationFailed)
            );
            self.notify_error(message, cx);
            return;
        }

        let relative_path = file_name.to_string_lossy().to_string();
        self.replace_image_url_on_line_with_text(line, &relative_path, cx);
    }

    fn pick_image_for_current_markdown_image(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.current_image_menu_target() else {
            return;
        };
        let Some(document_path) = self.current_file_path.clone() else {
            log_error!("select image requires a saved markdown file");
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
    /// `(full syntax range, text to keep)` for the `[desc](url)` the caret is
    /// inside, in buffer offsets.
    fn link_syntax_at_cursor(&self) -> Option<(Range<usize>, Range<usize>)> {
        let line = self.cursor_line();
        let line_start = self.buffer.line_start(line);
        let col = self.cursor.offset.saturating_sub(line_start);
        let line_content = self.buffer.line_content(line);
        // An image is a link with a leading `!`; leave those to the image action.
        let (full, desc, _url) = markdown_link_ranges(line_content, col)?;
        Some((
            line_start + full.start..line_start + full.end,
            line_start + desc.start..line_start + desc.end,
        ))
    }

    /// `(full syntax range, target to keep)` for the `[[target]]` at the caret.
    fn wiki_link_syntax_at_cursor(&self) -> Option<(Range<usize>, Range<usize>)> {
        let line = self.cursor_line();
        let line_start = self.buffer.line_start(line);
        let decoration = self.decorations.get(line)?;
        let col = self.cursor.offset.saturating_sub(line_start);
        let wiki_link = decoration
            .wiki_links
            .iter()
            .find(|link| (link.full_range.start..=link.full_range.end).contains(&col))?;
        Some((
            line_start + wiki_link.full_range.start..line_start + wiki_link.full_range.end,
            line_start + wiki_link.full_range.start + 2
                ..line_start + wiki_link.full_range.end.saturating_sub(2),
        ))
    }

    /// `(full syntax range, label to keep)` for the `[^label]` at the caret.
    fn footnote_syntax_at_cursor(&self) -> Option<(Range<usize>, Range<usize>)> {
        let line = self.cursor_line();
        let line_start = self.buffer.line_start(line);
        let decoration = self.decorations.get(line)?;
        let col = self.cursor.offset.saturating_sub(line_start);
        let footnote = decoration.footnote_references.iter().find(|reference| {
            (reference.full_range.start..=reference.full_range.end).contains(&col)
        })?;
        Some((
            line_start + footnote.full_range.start..line_start + footnote.full_range.end,
            line_start + footnote.full_range.start + 2
                ..line_start + footnote.full_range.end.saturating_sub(1),
        ))
    }

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

    fn active_format_actions_for_line(&self, line: usize) -> Vec<FormatMenuAction> {
        let Some(decoration) = self.decorations.get(line) else {
            return Vec::new();
        };

        vec![line_format_action(&decoration.block_type)]
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

    pub(crate) fn open_file_list_context_menu(
        &mut self,
        path: PathBuf,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.open_context_menu(
            ContextMenu {
                position,
                kind: ContextMenuKind::FileList { path },
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

    /// Horizontally pinned to the `/` so the menu does not drift while the
    /// filter is typed; vertically pinned to the bottom of the cursor's row so
    /// the layout can flip it directly above or below that row.
    pub(super) fn slash_menu_position(&self) -> Option<Point<Pixels>> {
        let start = self.slash_menu_start?;
        let start_position = self.screen_position_for_offset(start);
        let cursor_position = self.cursor_screen_position();
        Some(gpui::point(start_position.x, cursor_position.y))
    }

    pub(crate) fn open_slash_menu(&mut self, cx: &mut Context<Self>) {
        self.slash_menu_start = Some(self.cursor.offset - 1);
        self.slash_filter = String::new();
        self.menu_selected_index = 0;
        self.heading_submenu_open = false;
        let pos = self
            .slash_menu_position()
            .unwrap_or_else(|| self.cursor_screen_position());
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
            let pos = self
                .slash_menu_position()
                .unwrap_or_else(|| self.cursor_screen_position());
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
                .min(self.buffer.len())
                .max(query_start);
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
        // Keep the previous query's results visible while the new search runs;
        // replacing `wiki_link_search_task` cancels the stale task, so results
        // can only be written by the latest query. This avoids the per-keystroke
        // results -> "Searching" -> results flicker.
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
        tab_id: String,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.open_context_menu(
            ContextMenu {
                position,
                kind: ContextMenuKind::Terminal { tab_id },
            },
            true,
            cx,
        );
    }

    pub(crate) fn open_terminal_tab_create_context_menu(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if !crate::platform::capabilities::CHILD_PROCESSES {
            return;
        }

        if self
            .context_menu
            .as_ref()
            .is_some_and(|menu| menu.kind == ContextMenuKind::TerminalTabCreate)
        {
            self.close_context_menu(cx);
            return;
        }

        self.open_context_menu(
            ContextMenu {
                position,
                kind: ContextMenuKind::TerminalTabCreate,
            },
            true,
            cx,
        );
    }

    pub(crate) fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.context_menu_pane_target.take().is_some() {
            cx.notify();
        }
        if self.context_menu.is_none() {
            self.finish_close_context_menu(cx);
            self.context_menu_opacity = 0.0;
            self.context_menu_opacity_target = 0.0;
            return;
        }
        self.animate_context_menu_opacity(0.0, true, cx);
    }

    /// Dismisses without the fade-out. The slash menu must use this: during a
    /// fade the menu is still mounted but `slash_menu_start` is already cleared,
    /// so it would repaint as the full format menu.
    pub(crate) fn close_context_menu_immediately(&mut self, cx: &mut Context<Self>) {
        self.context_menu_opacity_epoch += 1;
        self.context_menu_opacity_task = None;
        self.context_menu_opacity = 0.0;
        self.context_menu_opacity_target = 0.0;
        self.finish_close_context_menu(cx);
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
        let retain_popup = self.selection_toolbar_format_session();
        if !retain_popup
            && let Some(line) = self.line_format_toolbar_line
            && line < self.buffer.line_count()
        {
            let line_start = self.buffer.line_start(line);
            let line_end = self.buffer.line_end(line);
            let block_action = matches!(
                action,
                FormatMenuAction::Heading1
                    | FormatMenuAction::Heading2
                    | FormatMenuAction::Heading3
                    | FormatMenuAction::Heading4
                    | FormatMenuAction::Heading5
                    | FormatMenuAction::Heading6
                    | FormatMenuAction::Paragraph
                    | FormatMenuAction::Todo
                    | FormatMenuAction::UnorderedList
                    | FormatMenuAction::OrderedList
                    | FormatMenuAction::HorizontalRule
                    | FormatMenuAction::CodeBlock
                    | FormatMenuAction::Quote
                    | FormatMenuAction::Table
                    | FormatMenuAction::Image
                    | FormatMenuAction::CalloutTip
            );
            if block_action {
                self.cursor.move_to(line_end, &self.buffer);
            } else {
                self.cursor.anchor = line_start;
                self.cursor.offset = line_end;
            }
            self.reset_cursor_visual_targeting();
        }
        self.execute_format_menu_action_inner(action, cx);
        if retain_popup {
            self.retain_selection_toolbar_after_format();
        }
    }

    fn execute_format_menu_action_inner(
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
                self.checkpoint();
                self.buffer.replace(start..end, "");
                self.cursor.move_to(start, &self.buffer);
                self.reparse();
                self.commit_history_transaction();
            }
            self.slash_filter.clear();
            self.close_context_menu_immediately(cx);
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
                if !self.remove_marker_wrap_at_cursor("==") {
                    self.apply_wrap("==", "==");
                }
                self.pause_blinking(cx);
            }
            FormatMenuAction::CodeBlock => {
                self.checkpoint();
                if !self.unwrap_code_block_at_cursor() {
                    self.wrap_current_lines_as_code_block();
                }
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
                if let Some((full, keep)) = self.link_syntax_at_cursor() {
                    self.unwrap_syntax_range(full, keep);
                } else {
                    self.insert_link_or_image("[", "](https://)", "title");
                }
                self.pause_blinking(cx);
            }
            FormatMenuAction::WikiLink => {
                self.checkpoint();
                if let Some((full, keep)) = self.wiki_link_syntax_at_cursor() {
                    self.unwrap_syntax_range(full, keep);
                    self.pause_blinking(cx);
                    self.close_context_menu(cx);
                    return;
                }
                let range = self.cursor.selection_range();
                let selected_text = self.buffer.text_for_range(range.clone()).to_string();
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
                if let Some(target) = self.current_image_menu_target() {
                    let line_start = self.buffer.line_start(target.line);
                    self.unwrap_syntax_range(
                        line_start + target.full_range.start..line_start + target.full_range.end,
                        line_start + target.desc_range.start..line_start + target.desc_range.end,
                    );
                } else {
                    self.insert_link_or_image("![", "](image.png)", "alt");
                }
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
                if !self.remove_marker_wrap_at_cursor("^") {
                    self.apply_wrap("^", "^");
                }
                self.pause_blinking(cx);
            }
            FormatMenuAction::Subscript => {
                self.checkpoint();
                if !self.remove_marker_wrap_at_cursor("~") {
                    self.apply_wrap("~", "~");
                }
                self.pause_blinking(cx);
            }
            FormatMenuAction::Footnote => {
                self.checkpoint();
                if let Some((full, keep)) = self.footnote_syntax_at_cursor() {
                    self.unwrap_syntax_range(full, keep);
                    self.pause_blinking(cx);
                    self.close_context_menu(cx);
                    return;
                }
                let range = self.cursor.selection_range();
                if range.is_empty() {
                    let snippet = "[^note]";
                    self.buffer.replace(range.clone(), snippet);
                    let select_start = range.start + 2;
                    let select_end = select_start + "note".len();
                    self.cursor.anchor = select_start;
                    self.cursor.offset = select_end;
                } else {
                    let selected_text = self.buffer.text_for_range(range.clone()).to_string();
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

    fn slash_menu_items(&self) -> Vec<SlashMenuItem> {
        vec![
            SlashMenuItem {
                label: self.format_action_label(FormatMenuAction::Todo),
                filter_text: "todo task checkbox",
                icon: Some(FMT_TODO_SVG),
                shortcut: Some(crate::editor::shortcut::hint("secondary-alt-enter")),
                action: FormatMenuAction::Todo,
            },
            SlashMenuItem {
                label: self.format_action_label(FormatMenuAction::CodeBlock),
                filter_text: "code block",
                icon: Some(FMT_CODE_SVG),
                shortcut: Some(crate::editor::shortcut::hint("secondary-alt-c")),
                action: FormatMenuAction::CodeBlock,
            },
            SlashMenuItem {
                label: self.format_action_label(FormatMenuAction::Table),
                filter_text: "table grid 2x2",
                icon: Some(FMT_TABLE_SVG),
                shortcut: Some(crate::editor::shortcut::hint("secondary-alt-t")),
                action: FormatMenuAction::Table,
            },
            SlashMenuItem {
                label: self.format_action_label(FormatMenuAction::Link),
                filter_text: "link url",
                icon: Some(FMT_LINK_SVG),
                shortcut: Some(crate::editor::shortcut::hint("secondary-k")),
                action: FormatMenuAction::Link,
            },
            SlashMenuItem {
                label: self.format_action_label(FormatMenuAction::DateTime),
                filter_text: "date time now today timestamp calendar",
                icon: Some(FMT_DATE_TIME_SVG),
                shortcut: Some("now".into()),
                action: FormatMenuAction::DateTime,
            },
            SlashMenuItem {
                label: self.format_action_label(FormatMenuAction::CalloutTip),
                filter_text: "tip callout alert admonition quote",
                icon: Some(FMT_CALLOUT_TIP_SVG),
                shortcut: Some("> [!TIP]".into()),
                action: FormatMenuAction::CalloutTip,
            },
            SlashMenuItem {
                label: self.format_action_label(FormatMenuAction::WikiLink),
                filter_text: "wiki link backlink bidirectional",
                icon: Some(FMT_WIKI_LINK_SVG),
                shortcut: Some("[[".into()),
                action: FormatMenuAction::WikiLink,
            },
            SlashMenuItem {
                label: self.format_action_label(FormatMenuAction::Image),
                filter_text: "image photo",
                icon: Some(FMT_IMAGE_SVG),
                shortcut: None,
                action: FormatMenuAction::Image,
            },
        ]
    }

    fn slash_menu_matched_items(&self) -> Vec<SlashMenuItem> {
        let filter = self.slash_filter.to_lowercase();
        self.slash_menu_items()
            .into_iter()
            .filter(|item| {
                filter.is_empty()
                    || item.label.to_lowercase().contains(&filter)
                    || item.filter_text.contains(&filter)
            })
            .collect()
    }

    pub(crate) fn slash_menu_matched_actions(&self) -> Vec<FormatMenuAction> {
        self.slash_menu_matched_items()
            .into_iter()
            .map(|item| item.action)
            .collect()
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

    fn render_context_menu_pane_item(
        &self,
        label: &str,
        target: (PaneId, Option<SplitDirection>),
        cx: &mut Context<Self>,
        handler: impl Fn(&mut Self, &mut gpui::Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        div()
            .id(SharedString::from(format!("pane-menu-target-{label}")))
            .w_full()
            .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                let next = if *hovering {
                    Some(target)
                } else if this.context_menu_pane_target == Some(target) {
                    None
                } else {
                    this.context_menu_pane_target
                };
                if this.context_menu_pane_target != next {
                    this.context_menu_pane_target = next;
                    cx.notify();
                }
            }))
            .child(self.render_context_menu_item(label, false, cx, handler))
            .into_any_element()
    }

    fn append_tab_pane_context_menu_items(
        &self,
        items: &mut Vec<AnyElement>,
        tab_id: &str,
        cx: &mut Context<Self>,
    ) {
        let Some(source_pane) = self.pane_id_for_tab(tab_id) else {
            return;
        };
        items.push(self.render_context_menu_separator());
        for (label, direction) in [
            (
                self.t(crate::i18n::TextKey::SplitTabLeft),
                SplitDirection::Left,
            ),
            (
                self.t(crate::i18n::TextKey::SplitTabRight),
                SplitDirection::Right,
            ),
            (self.t(crate::i18n::TextKey::SplitTabUp), SplitDirection::Up),
            (
                self.t(crate::i18n::TextKey::SplitTabDown),
                SplitDirection::Down,
            ),
        ] {
            let id = tab_id.to_string();
            items.push(self.render_context_menu_pane_item(
                label,
                (source_pane, Some(direction)),
                cx,
                move |this, _, cx| {
                    this.close_context_menu(cx);
                    this.split_pane_with_tab(source_pane, direction, &id, cx);
                },
            ));
        }

        let target_panes = self
            .pane_layout
            .leaf_ids()
            .into_iter()
            .enumerate()
            .filter(|(_, pane_id)| *pane_id != source_pane)
            .collect::<Vec<_>>();
        if target_panes.is_empty() {
            return;
        }
        items.push(self.render_context_menu_separator());
        for (index, target_pane) in target_panes {
            let id = tab_id.to_string();
            let label = crate::i18n::move_tab_to_pane_label(self.ui_language, index + 1);
            items.push(self.render_context_menu_pane_item(
                &label,
                (target_pane, None),
                cx,
                move |this, _, cx| {
                    this.close_context_menu(cx);
                    this.move_tab_to_pane(&id, target_pane, None, cx);
                },
            ));
        }
    }

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
            .on_mouse_move(cx.listener(|this, _, _, cx| {
                this.dismiss_export_submenu(cx);
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _event, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    this.suppress_editor_mouse_up_for_current_menu_event(cx);
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
        self.render_context_menu_item_with_icon_behavior(
            label,
            icon_path,
            is_destructive,
            true,
            cx,
            handler,
        )
    }

    fn render_context_menu_item_with_icon_behavior(
        &self,
        label: &str,
        icon_path: &'static str,
        is_destructive: bool,
        dismiss_export_submenu: bool,
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
            .when(dismiss_export_submenu, |this| {
                this.on_mouse_move(cx.listener(|this, _, _, cx| {
                    this.dismiss_export_submenu(cx);
                }))
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _event, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    this.suppress_editor_mouse_up_for_current_menu_event(cx);
                    this.close_context_menu(cx);
                    handler(this, window, cx);
                }),
            )
            .into_any_element()
    }

    fn dismiss_export_submenu(&mut self, cx: &mut Context<Self>) {
        if matches!(self.context_submenu, Some(ContextMenuKind::Export { .. })) {
            self.context_submenu = None;
            self.context_submenu_aim = None;
            self.context_submenu_hovered = false;
            cx.notify();
        }
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
                    window.prevent_default();
                    cx.stop_propagation();
                    this.suppress_editor_mouse_up_for_current_menu_event(cx);
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
        shortcut: Option<SharedString>,
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
        shortcut: Option<SharedString>,
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
                if shortcut
                    .chars()
                    .any(|ch| matches!(ch, '⌘' | '⌥' | '⌃' | '⇧'))
                {
                    this.child(div().child(render_shortcut(&shortcut, shortcut_style)))
                } else {
                    this.child(
                        div()
                            .font_family(self.styles.editor.editor_monospace_font_family)
                            .text_size(px(self.scaled(ms.menu_shortcut_font_size_px)))
                            .text_color(ms.menu_shortcut_text_color)
                            .child(shortcut.to_string()),
                    )
                }
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
                    window.prevent_default();
                    cx.stop_propagation();
                    this.suppress_editor_mouse_up_for_current_menu_event(cx);
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
        let wss = &self.styles.workspace_switcher;
        let is_open = self.context_submenu.as_ref() == Some(&submenu_kind);
        let icon_size = self.scaled(ms.menu_format_icon_size_px);
        // Export lives in the sidebar-style menus, the rest in the format menu;
        // each has to match its host's icon column or the label drifts.
        let (icon_col_w, icon_gap) = if matches!(submenu_kind, ContextMenuKind::Export { .. }) {
            let (column, gap) = menu_icon_column_metrics(
                ms.menu_format_icon_size_px,
                wss.ws_menu_item_check_size_px,
                wss.ws_menu_item_action_icon_gap_px,
            );
            (self.scaled(column), self.scaled(gap))
        } else {
            (icon_size, self.scaled(ms.menu_format_icon_gap_px))
        };
        let arrow_size = (icon_size - self.scaled(2.0)).max(self.scaled(10.0));
        let hover_bg = ms.menu_item_hover_bg;
        let text_color = ms.menu_item_primary_text_color;
        let bg = if is_open {
            ms.menu_item_selected_bg
        } else {
            gpui::transparent_black()
        };
        let icon_shared = icon_path.map(SharedString::from);
        let trigger_bounds = match submenu_kind {
            ContextMenuKind::FormatHeading => self.format_heading_trigger_bounds.clone(),
            ContextMenuKind::FormatMore => self.format_more_trigger_bounds.clone(),
            ContextMenuKind::Export { .. } => self.export_trigger_bounds.clone(),
            _ => Rc::new(Cell::new(None)),
        };

        let row = div()
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
                            .w(px(icon_col_w))
                            .h(px(icon_col_w))
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
                    .path("assets/icons/navigation/right.svg")
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
            .into_any_element();

        div()
            .w_full()
            .h(px(self.scaled(ms.menu_item_height_px)))
            .on_children_prepainted(move |bounds: Vec<Bounds<Pixels>>, _, _| {
                trigger_bounds.set(bounds.first().copied());
            })
            .child(row)
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
                let chat_path = path.clone();
                items.push(self.render_context_menu_item(
                    crate::i18n::add_file_to_chat_label(self.ui_language),
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
                    self.t(crate::i18n::TextKey::RevealInFinder),
                    false,
                    cx,
                    move |this, _, _cx| {
                        this.reveal_in_file_manager(&p);
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
            if is_markdown_file(&path) {
                items.push(self.render_format_submenu_trigger_item(
                    self.t(crate::i18n::TextKey::Export),
                    None,
                    ContextMenuKind::Export { path: path.clone() },
                    cx,
                ));
                items.push(self.render_context_menu_separator());
            }
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
                crate::i18n::add_file_to_chat_label(self.ui_language),
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
                    this.reveal_in_file_manager(&p);
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

        if is_markdown_file(&path) {
            items.push(self.render_format_submenu_trigger_item(
                self.t(crate::i18n::TextKey::Export),
                None,
                ContextMenuKind::Export { path: path.clone() },
                cx,
            ));
            items.push(self.render_context_menu_separator());
        }

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
                    this.reveal_paths_in_file_manager(&reveal_paths);
                },
            ));
        }

        items
    }

    fn render_file_list_context_menu(
        &self,
        path: PathBuf,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let parent_dir = path
            .parent()
            .map(PathBuf::from)
            .unwrap_or_else(|| self.file_list_effective_dir());
        let mut items: Vec<AnyElement> = Vec::new();

        {
            let parent = parent_dir.clone();
            items.push(self.render_context_menu_item(
                self.t(crate::i18n::TextKey::NewFile),
                false,
                cx,
                move |this, window, cx| {
                    this.start_create_file(parent.clone(), window, cx);
                    this.sidebar_edit_in_file_list = true;
                },
            ));
        }
        {
            let chat_path = path.clone();
            items.push(self.render_context_menu_item(
                crate::i18n::add_file_to_chat_label(self.ui_language),
                false,
                cx,
                move |this, _, cx| {
                    this.close_context_menu(cx);
                    this.add_path_to_chat(&chat_path, cx);
                },
            ));
        }
        if is_markdown_file(&path) {
            items.push(self.render_format_submenu_trigger_item(
                self.t(crate::i18n::TextKey::Export),
                None,
                ContextMenuKind::Export { path: path.clone() },
                cx,
            ));
        }
        if self.selected_sidebar_paths.len() <= 1 {
            let rename_path = path.clone();
            items.push(self.render_context_menu_item(
                self.t(crate::i18n::TextKey::Rename),
                false,
                cx,
                move |this, window, cx| {
                    this.file_list_menu_origin = true;
                    this.start_rename(rename_path.clone(), window, cx);
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
                    this.show_sidebar_delete_dialog(vec![delete_path.clone()], cx);
                },
            ));
        }
        {
            let star_path = path.clone();
            let normalized = Self::normalize_sidebar_path(&star_path);
            let is_starred = self.app_settings.is_starred_file(&normalized);
            let label = if is_starred {
                self.t(crate::i18n::TextKey::Unstar)
            } else {
                self.t(crate::i18n::TextKey::Star)
            };
            items.push(
                self.render_context_menu_item(label, false, cx, move |this, _, cx| {
                    let normalized = Self::normalize_sidebar_path(&star_path);
                    if is_starred {
                        this.app_settings.remove_starred_file(&normalized);
                    } else {
                        this.app_settings.add_starred_file(normalized);
                        this.sidebar_starred_collapsed = false;
                    }
                    this.app_settings.save();
                    this.close_context_menu(cx);
                    cx.notify();
                }),
            );
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
        {
            let reveal_path = path.clone();
            items.push(self.render_context_menu_item(
                self.t(crate::i18n::TextKey::RevealInFinder),
                false,
                cx,
                move |this, _, cx| {
                    this.close_context_menu(cx);
                    this.reveal_in_file_manager(&reveal_path);
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

        if let Some(image_source) = self.current_markdown_image_source() {
            let image_source = image_source.to_string();
            let label = self.t(crate::i18n::TextKey::CopyToClipboard).to_string();
            items.push(self.render_format_context_menu_item(
                &label,
                Some(MENU_COPY_SVG_PATH),
                None,
                false,
                false,
                cx,
                move |this, _, cx| {
                    this.copy_markdown_image_to_clipboard(image_source.clone(), cx);
                },
            ));
        }

        if let Some(image_path) = self.current_markdown_image_path() {
            if self.workspace_root_for_path(&image_path).is_some() {
                let reveal_path = image_path.clone();
                let label = self.t(crate::i18n::TextKey::RevealInFileTree).to_string();
                items.push(self.render_format_context_menu_item(
                    &label,
                    Some(MENU_LOCATE_SVG_PATH),
                    None,
                    false,
                    false,
                    cx,
                    move |this, _, cx| {
                        this.close_context_menu(cx);
                        this.locate_path_in_sidebar(reveal_path.clone(), cx);
                    },
                ));
            }

            {
                let reveal_path = image_path;
                let label = self.t(crate::i18n::TextKey::RevealInFinder).to_string();
                items.push(self.render_format_context_menu_item(
                    &label,
                    Some(MENU_LOCATE_SVG_PATH),
                    None,
                    false,
                    false,
                    cx,
                    move |this, _, cx| {
                        this.context_menu = None;
                        this.reveal_in_file_manager(&reveal_path);
                        cx.notify();
                    },
                ));
            }
        }

        items
    }

    pub(crate) fn render_format_toolbar(
        &self,
        pane_width: f32,
        opacity: f32,
        interactive: bool,
        _track_line_hover: bool,
        active_line: Option<usize>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let fts = &self.styles.format_toolbar;
        let active_actions = active_line
            .map(|line| self.active_format_actions_for_line(line))
            .unwrap_or_else(|| self.active_format_actions());
        let metrics = FormatToolbarMetrics {
            button: self.scaled(fts.format_toolbar_btn_size_px),
            gap: self.scaled(fts.format_toolbar_gap_px),
            padding: self.scaled(fts.format_toolbar_padding_x_px),
            border: self.scaled(fts.format_toolbar_border_width_px),
        };
        let available_width = if pane_width.is_finite() {
            pane_width.max(0.0)
        } else {
            0.0
        };
        let (rows, natural_width) = format_toolbar_layout(metrics);
        if natural_width > available_width {
            return div().into_any_element();
        }

        let mut bar = div()
            .id("format-toolbar")
            .w(px(natural_width))
            .min_w(px(natural_width))
            .max_w(px(available_width))
            .flex_none()
            .flex()
            .flex_col()
            .items_start()
            .py(px(metrics.padding))
            .gap(px(metrics.gap))
            .opacity(opacity)
            .when(interactive, |this| this.occlude())
            .bg(fts.format_toolbar_bg)
            .border(px(metrics.border))
            .border_color(fts.format_toolbar_border_color)
            .rounded(px(self.scaled(fts.format_toolbar_radius_px)))
            .shadow_sm()
            .cursor(CursorStyle::Arrow)
            .when(interactive, |this| {
                this.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|_, _, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                    }),
                )
            });

        for (row_index, entries) in rows.into_iter().enumerate() {
            let mut row = div()
                .id(SharedString::from(format!(
                    "format-toolbar-row-{row_index}"
                )))
                .h(px(metrics.button))
                .w_full()
                .flex()
                .items_center()
                .gap(px(metrics.gap))
                .when(row_index > 0, |this| this.px(px(metrics.padding)));
            for entry in entries {
                row = match entry {
                    FormatToolbarEntry::HeadingMenu => {
                        row.child(self.render_format_toolbar_heading_button(interactive, cx))
                    }
                    FormatToolbarEntry::MoreMenu => {
                        row.child(self.render_format_toolbar_more_button(interactive, cx))
                    }
                    FormatToolbarEntry::Action {
                        action,
                        icon,
                        shortcut,
                    } => {
                        let is_active = active_actions.contains(&action);
                        row.child(self.render_format_toolbar_button(
                            self.format_action_label(action),
                            Some(icon),
                            shortcut,
                            is_active,
                            interactive,
                            cx,
                            move |this, cx| this.execute_format_menu_action(action, cx),
                        ))
                    }
                };
            }
            bar = bar.child(row);
        }

        bar.into_any_element()
    }

    fn render_format_toolbar_button(
        &self,
        label: String,
        icon: Option<&'static str>,
        shortcut: Option<SharedString>,
        is_active: bool,
        interactive: bool,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let fts = &self.styles.format_toolbar;
        let group_name = SharedString::from(format!("format-toolbar-{label}"));
        let hover_color = fts.format_toolbar_icon_hover_color;
        let mut icon_color = if is_active {
            fts.format_toolbar_icon_active_color
        } else {
            fts.format_toolbar_icon_color
        };
        if !is_active {
            icon_color.a *= 0.76;
        }
        let button_size = self.scaled(fts.format_toolbar_btn_size_px);
        let icon_size = self.scaled(fts.format_toolbar_icon_size_px);

        let button = div()
            .id(SharedString::from(format!("format-toolbar-btn-{label}")))
            .group(group_name.clone())
            .w(px(button_size))
            .h(px(button_size))
            .flex()
            .flex_none()
            .items_center()
            .justify_center()
            .cursor(CursorStyle::Arrow)
            .when_some(icon, |this, icon| {
                this.child(
                    svg()
                        .path(icon)
                        .size(px(icon_size))
                        .text_color(icon_color)
                        .group_hover(group_name.clone(), move |style| {
                            style.text_color(hover_color)
                        }),
                )
            })
            .when(icon.is_none(), |this| {
                this.child(
                    div()
                        .flex()
                        .gap(px(self.scaled(2.0)))
                        .children((0..3).map(|_| {
                            div()
                                .size(px(self.scaled(2.0)))
                                .rounded_full()
                                .bg(icon_color)
                        })),
                )
            })
            .when(interactive, |this| {
                this.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                        on_click(this, cx);
                    }),
                )
            });

        tooltip::with_tooltip_above(button, cx.entity().clone(), label, shortcut)
    }

    pub(crate) fn format_toolbar_owns_hover_menu(&self) -> bool {
        (self.selection_toolbar_format_session() || self.line_format_toolbar_line.is_some())
            && self.context_menu.as_ref().is_some_and(|menu| {
                matches!(
                    menu.kind,
                    ContextMenuKind::FormatHeading | ContextMenuKind::FormatMore
                )
            })
    }

    fn open_selection_toolbar_menu(
        &mut self,
        kind: ContextMenuKind,
        bounds: Option<Bounds<Pixels>>,
        cx: &mut Context<Self>,
    ) {
        let selection_session = self.selection_toolbar_format_session();
        if !selection_session && self.line_format_toolbar_line.is_none() {
            return;
        }
        self.selection_toolbar_menu_hover.close_task = None;
        if self
            .context_menu
            .as_ref()
            .is_some_and(|menu| menu.kind == kind)
        {
            return;
        }
        let position = bounds
            .map(|bounds| Point::new(bounds.left(), bounds.bottom()))
            .unwrap_or_else(|| self.cursor_screen_position());
        self.open_context_menu(ContextMenu { position, kind }, true, cx);
        if selection_session {
            self.mark_selection_toolbar_menu();
        }
    }

    fn schedule_selection_toolbar_menu(
        &mut self,
        region: &'static str,
        kind: ContextMenuKind,
        bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        if self
            .context_menu
            .as_ref()
            .is_some_and(|menu| menu.kind == kind)
            || self.selection_toolbar_menu_hover.open_region == Some(region)
        {
            return;
        }
        self.selection_toolbar_menu_hover.open_epoch =
            self.selection_toolbar_menu_hover.open_epoch.wrapping_add(1);
        let epoch = self.selection_toolbar_menu_hover.open_epoch;
        self.selection_toolbar_menu_hover.open_region = Some(region);
        self.selection_toolbar_menu_hover.open_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_millis(180))
                .await;
            let _ = this.update(cx, |this, cx| {
                if this.selection_toolbar_menu_hover.open_epoch == epoch
                    && this.selection_toolbar_menu_hover.open_region == Some(region)
                {
                    this.selection_toolbar_menu_hover.open_region = None;
                    this.open_selection_toolbar_menu(kind, Some(bounds), cx);
                }
            });
        }));
    }

    fn cancel_selection_toolbar_menu_open(&mut self, region: &'static str) {
        if self.selection_toolbar_menu_hover.open_region == Some(region) {
            self.selection_toolbar_menu_hover.open_epoch =
                self.selection_toolbar_menu_hover.open_epoch.wrapping_add(1);
            self.selection_toolbar_menu_hover.open_region = None;
            self.selection_toolbar_menu_hover.open_task = None;
        }
    }

    fn update_selection_toolbar_menu_hover(
        &mut self,
        region: &'static str,
        hovered: bool,
        hitbox: &gpui::Hitbox,
        window: &gpui::Window,
        cx: &mut Context<Self>,
    ) {
        if !self.format_toolbar_owns_hover_menu() {
            return;
        }
        let state = &mut self.selection_toolbar_menu_hover;
        state.regions.retain(|_, hitbox| hitbox.is_hovered(window));
        if hovered {
            state.regions.insert(region, hitbox.clone());
        } else {
            state.regions.remove(region);
        }
        if !state.regions.is_empty() {
            state.close_task = None;
            return;
        }
        if state.close_task.is_none() {
            state.close_task = Some(cx.spawn(async move |this, cx| {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(200))
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if this.format_toolbar_owns_hover_menu()
                        && this.selection_toolbar_menu_hover.regions.is_empty()
                    {
                        this.close_context_menu_immediately(cx);
                    }
                });
            }));
        }
    }

    fn render_format_toolbar_more_button(
        &self,
        interactive: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let trigger_bounds = Rc::new(Cell::new(None::<Bounds<Pixels>>));
        let click_bounds = trigger_bounds.clone();
        let button = self.render_format_toolbar_button(
            self.more_formats_menu_label().to_string(),
            None,
            None,
            false,
            interactive,
            cx,
            move |this, cx| {
                this.open_selection_toolbar_menu(
                    FormatToolbarEntry::MoreMenu.menu_kind().unwrap(),
                    click_bounds.get(),
                    cx,
                );
            },
        );
        SelectionToolbarHoverRegion {
            child: div()
                .flex_none()
                .on_children_prepainted(move |bounds: Vec<Bounds<Pixels>>, _, _| {
                    trigger_bounds.set(bounds.first().copied());
                })
                .child(button)
                .into_any_element(),
            editor: cx.entity().clone(),
            region: "more-trigger",
            menu_kind: Some(ContextMenuKind::FormatMore),
        }
        .into_any_element()
    }

    fn render_format_toolbar_heading_button(
        &self,
        interactive: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let trigger_bounds = Rc::new(Cell::new(None::<Bounds<Pixels>>));
        let click_bounds = trigger_bounds.clone();
        let label = self.t(crate::i18n::TextKey::Heading).to_string();
        let group_name = SharedString::from("format-toolbar-heading");
        let icon_color = self.styles.format_toolbar.format_toolbar_icon_color;
        let hover_color = self.styles.format_toolbar.format_toolbar_icon_hover_color;
        let hover_bg = self.styles.context_menu.menu_item_hover_bg;
        let padding = self.scaled(self.styles.format_toolbar.format_toolbar_padding_x_px);
        let button_size = self.scaled(self.styles.format_toolbar.format_toolbar_btn_size_px);
        let icon_size = self.scaled(self.styles.format_toolbar.format_toolbar_icon_size_px);
        let bg = if self
            .context_menu
            .as_ref()
            .is_some_and(|menu| menu.kind == ContextMenuKind::FormatHeading)
        {
            self.styles.context_menu.menu_item_selected_bg
        } else {
            gpui::transparent_black()
        };
        let heading_icon = tooltip::with_tooltip_above(
            div()
                .size(px(button_size))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .child(
                    svg()
                        .path(HEADING_1_SVG_PATH)
                        .size(px(icon_size))
                        .text_color(icon_color)
                        .group_hover(group_name.clone(), move |style| {
                            style.text_color(hover_color)
                        }),
                ),
            cx.entity().clone(),
            label.clone(),
            None::<SharedString>,
        );
        let button = div()
            .id("format-toolbar-heading")
            .group(group_name.clone())
            .w_full()
            .h(px(self.scaled(
                self.styles.format_toolbar.format_toolbar_btn_size_px,
            )))
            .bg(bg)
            .hover(move |style| style.bg(hover_bg))
            .text_size(px(
                self.scaled(self.styles.context_menu.menu_item_font_size_px)
            ))
            .text_color(icon_color)
            .group_hover(group_name.clone(), move |style| {
                style.text_color(hover_color)
            })
            .cursor(CursorStyle::Arrow)
            .child(
                div()
                    .w_full()
                    .h_full()
                    .px(px(padding))
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(6.0)))
                    .child(heading_icon)
                    .child(label),
            )
            .when(interactive, |this| {
                this.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                        this.open_selection_toolbar_menu(
                            ContextMenuKind::FormatHeading,
                            click_bounds.get(),
                            cx,
                        );
                    }),
                )
            });
        SelectionToolbarHoverRegion {
            child: div()
                .w_full()
                .on_children_prepainted(move |bounds: Vec<Bounds<Pixels>>, _, _| {
                    trigger_bounds.set(bounds.first().copied());
                })
                .child(button)
                .into_any_element(),
            editor: cx.entity().clone(),
            region: "heading-trigger",
            menu_kind: Some(ContextMenuKind::FormatHeading),
        }
        .into_any_element()
    }

    fn render_format_context_menu(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let table_submenu_kind = self
            .context_menu
            .as_ref()
            .and_then(|menu| match &menu.kind {
                ContextMenuKind::Table { .. } => Some(menu.kind.clone()),
                _ => None,
            });
        if table_submenu_kind.is_none() && self.current_image_menu_target().is_some() {
            return self.render_image_context_menu(cx);
        }

        let mut items: Vec<AnyElement> = Vec::new();
        let is_slash_menu = self.slash_menu_start.is_some();
        let selected_index = self.menu_selected_index;
        let mut actionable_index: usize = 0;
        let active_actions = self.active_format_actions();

        if !is_slash_menu {
            for (label, shortcut, action) in [
                (
                    self.format_action_label(FormatMenuAction::Cut),
                    Some(crate::editor::shortcut::hint("secondary-x")),
                    FormatMenuAction::Cut,
                ),
                (
                    self.format_action_label(FormatMenuAction::Copy),
                    Some(crate::editor::shortcut::hint("secondary-c")),
                    FormatMenuAction::Copy,
                ),
                (
                    self.format_action_label(FormatMenuAction::Paste),
                    Some(crate::editor::shortcut::hint("secondary-v")),
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

            if let Some(current_path) = self.current_file_path.clone() {
                items.push(self.render_context_menu_separator());
                let chat_path = current_path.clone();
                items.push(self.render_format_context_menu_item_with_hover_behavior(
                    crate::i18n::add_file_to_chat_label(self.ui_language),
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

                let selection_path = current_path;
                let selection_range = self.cursor.selection_range();
                items.push(self.render_format_context_menu_item_with_hover_behavior(
                    crate::i18n::add_selection_to_chat_label(self.ui_language),
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

            // Markdown-specific actions below do not apply to plain text or
            // source files.
            if self.active_tab_is_non_markdown_text() {
                return items;
            }

            // Copy the document's text with all markdown syntax markers removed.
            // Operates on the selection when present (still excluding any syntax
            // characters inside it), otherwise on the whole document.
            items.push(self.render_format_context_menu_item_with_hover_behavior(
                self.t(crate::i18n::TextKey::CopyPlainText),
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

            items.push(self.render_format_context_menu_item_with_hover_behavior(
                crate::i18n::format_markdown_label(self.ui_language),
                Some(MARKDOWN_SVG_PATH),
                None,
                false,
                false,
                FormatMenuHoverBehavior::ClearSubmenu,
                cx,
                move |this, _, cx| {
                    this.close_context_menu(cx);
                    this.format_markdown_selection_or_prompt(cx);
                },
            ));

            items.push(self.render_context_menu_separator());
            items.push(self.render_format_submenu_trigger_item(
                &self.t(crate::i18n::TextKey::Heading).to_string(),
                Some(HEADING_1_SVG_PATH),
                ContextMenuKind::FormatHeading,
                cx,
            ));
            if let Some(table_submenu_kind) = table_submenu_kind {
                items.push(self.render_format_submenu_trigger_item(
                    &self.t(crate::i18n::TextKey::Table).to_string(),
                    Some(FMT_TABLE_SVG),
                    table_submenu_kind,
                    cx,
                ));
            }

            for (label, icon, shortcut, action) in [
                (
                    self.format_action_label(FormatMenuAction::Bold),
                    Some(FMT_BOLD_SVG),
                    Some(crate::editor::shortcut::hint("secondary-b")),
                    FormatMenuAction::Bold,
                ),
                (
                    self.format_action_label(FormatMenuAction::Italic),
                    Some(FMT_ITALIC_SVG),
                    Some(crate::editor::shortcut::hint("secondary-i")),
                    FormatMenuAction::Italic,
                ),
                (
                    self.format_action_label(FormatMenuAction::Strikethrough),
                    Some(FMT_STRIKETHROUGH_SVG),
                    Some(crate::editor::shortcut::hint("secondary-shift-x")),
                    FormatMenuAction::Strikethrough,
                ),
                (
                    self.format_action_label(FormatMenuAction::Underline),
                    Some(FMT_UNDERLINE_SVG),
                    Some(crate::editor::shortcut::hint("secondary-u")),
                    FormatMenuAction::Underline,
                ),
                (
                    self.format_action_label(FormatMenuAction::InlineCode),
                    Some(FMT_INLINE_CODE_SVG),
                    Some(crate::editor::shortcut::hint("secondary-e")),
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
                    Some(crate::editor::shortcut::hint("secondary-alt-enter")),
                    FormatMenuAction::Todo,
                ),
                (
                    self.format_action_label(FormatMenuAction::Link),
                    Some(FMT_LINK_SVG),
                    Some(crate::editor::shortcut::hint("secondary-k")),
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

        for slash_item in self.slash_menu_matched_items() {
            let is_selected = actionable_index == selected_index;
            let is_active = active_actions.contains(&slash_item.action);
            let action = slash_item.action;
            items.push(self.render_format_context_menu_item(
                &slash_item.label,
                slash_item.icon,
                slash_item.shortcut,
                is_selected,
                is_active,
                cx,
                move |this, _, cx| {
                    this.execute_format_menu_action(action, cx);
                },
            ));
            actionable_index += 1;
        }

        items
    }

    fn render_format_more_context_menu(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let active_actions = self.active_format_actions();
        let mut items = Vec::new();

        for (label, icon, shortcut, action) in [(
            self.format_action_label(FormatMenuAction::WikiLink),
            Some(FMT_WIKI_LINK_SVG),
            Some("[[".into()),
            FormatMenuAction::WikiLink,
        )] {
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
                Some("^text^".into()),
                FormatMenuAction::Superscript,
            ),
            (
                self.format_action_label(FormatMenuAction::Subscript),
                Some(FMT_SUBSCRIPT_SVG),
                Some("~text~".into()),
                FormatMenuAction::Subscript,
            ),
            (
                self.format_action_label(FormatMenuAction::Footnote),
                Some(FMT_FOOTNOTE_SVG),
                Some("[^note]".into()),
                FormatMenuAction::Footnote,
            ),
            (
                self.format_action_label(FormatMenuAction::DateTime),
                Some(FMT_DATE_TIME_SVG),
                None,
                FormatMenuAction::DateTime,
            ),
            (
                self.format_action_label(FormatMenuAction::CalloutTip),
                Some(FMT_CALLOUT_TIP_SVG),
                Some("> [!TIP]".into()),
                FormatMenuAction::CalloutTip,
            ),
            (
                self.format_action_label(FormatMenuAction::Highlight),
                Some(FMT_HIGHLIGHT_SVG),
                Some("==text==".into()),
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
                    .mx(px(ms.menu_border_width_px))
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

    fn button_context_menu_panel_layout(
        &self,
        anchor: Bounds<Pixels>,
        width: f32,
        estimated_menu_height: f32,
    ) -> ContextMenuPanelLayout {
        let margin = self.scaled(self.styles.anchored_popup_margin_px);
        let placement = helpers::anchored_popup_placement(
            self.last_window_width,
            self.last_window_height,
            anchor,
            width,
            estimated_menu_height,
            false,
            self.scaled(self.styles.anchored_popup_gap_px),
            margin,
        );
        let (estimated_top, bottom_anchor) = match placement.vertical {
            helpers::AnchoredPopupVertical::Above { bottom } => (0.0, Some(bottom)),
            helpers::AnchoredPopupVertical::Below { top } => (top, None),
        };
        ContextMenuPanelLayout {
            left: placement.left,
            estimated_top,
            max_height: placement.max_height,
            bottom_anchor,
            overlay_width: self.last_window_width,
            overlay_height: self.last_window_height,
            margin,
        }
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
        if !matches!(
            menu.kind,
            ContextMenuKind::Format | ContextMenuKind::Table { .. }
        ) || !self.can_render_format_submenu()
        {
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
        let main_visible_height = main_menu_height.min(main_layout.max_height);
        let submenu_visible_height = submenu_height
            .min((main_layout.overlay_height - 2.0 * main_layout.margin).max(self.scaled(100.0)));
        let main_menu_top = self.context_menu_layout_top(main_layout, main_visible_height);
        let main_menu_bottom = main_menu_top + main_visible_height;
        let item_height = self.scaled(ms.menu_item_height_px);
        let padding_y = self.scaled(ms.menu_padding_y_px);
        let separator_height = self.context_menu_separator_block_height();
        let heading_trigger_top = main_menu_top
            + padding_y
            + item_height * 4.0
            + separator_height
            + if self.current_file_path.is_some() {
                item_height * 2.0 + separator_height
            } else {
                0.0
            }
            + separator_height;
        let table_trigger_top = heading_trigger_top + item_height;
        let more_trigger_bottom = main_menu_bottom - padding_y;
        let measured_trigger = match submenu_kind {
            ContextMenuKind::FormatHeading => self.format_heading_trigger_bounds.get(),
            ContextMenuKind::FormatMore => self.format_more_trigger_bounds.get(),
            _ => None,
        };
        let right_space = main_layout.overlay_width
            - (main_layout.left + main_menu_width)
            - submenu_gap
            - main_layout.margin;
        let submenu_left = if right_space >= submenu_width {
            main_layout.left + main_menu_width + submenu_gap
        } else {
            (main_layout.left - submenu_width - submenu_gap).max(main_layout.margin)
        };
        let desired_top = if let Some(bounds) = measured_trigger {
            let (_, trigger_top) = self.context_menu_overlay_coordinates(bounds.origin);
            let trigger_bottom = trigger_top + f32::from(bounds.size.height);
            aligned_submenu_top(
                trigger_top,
                trigger_bottom,
                submenu_visible_height,
                main_layout.overlay_height,
                main_layout.margin,
            )
        } else {
            match submenu_kind {
                ContextMenuKind::FormatHeading => heading_trigger_top,
                ContextMenuKind::Table { .. } => table_trigger_top,
                ContextMenuKind::FormatMore => more_trigger_bottom - submenu_visible_height,
                _ => main_menu_bottom - submenu_visible_height,
            }
        };
        let submenu_top = desired_top.clamp(
            main_layout.margin,
            (main_layout.overlay_height - main_layout.margin - submenu_visible_height)
                .max(main_layout.margin),
        );

        Some(ContextMenuPanelLayout {
            left: submenu_left,
            estimated_top: submenu_top,
            max_height: (main_layout.overlay_height - 2.0 * main_layout.margin)
                .max(self.scaled(100.0)),
            bottom_anchor: None,
            overlay_width: main_layout.overlay_width,
            overlay_height: main_layout.overlay_height,
            margin: main_layout.margin,
        })
    }

    fn should_preserve_format_submenu(&self, cursor_position: Point<Pixels>) -> bool {
        let Some(menu) = &self.context_menu else {
            return false;
        };
        if !matches!(
            menu.kind,
            ContextMenuKind::Format | ContextMenuKind::Table { .. }
        ) || self.context_submenu.is_none()
        {
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
        let table_resize_panel = self
            .context_menu
            .as_ref()
            .is_some_and(|menu| matches!(menu.kind, ContextMenuKind::TableResize { .. }));

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
            .when(table_resize_panel, |this| this.overflow_hidden())
            .when(!table_resize_panel, |this| {
                this.overflow_y_scroll()
                    .py(px(self.scaled(ms.menu_padding_y_px)))
            })
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

        let menu_div = if self
            .context_menu
            .as_ref()
            .is_some_and(|menu| matches!(menu.kind, ContextMenuKind::TableResize { .. }))
        {
            menu_div.on_hover(cx.listener(|this, hovered: &bool, _window, cx| {
                if !*hovered {
                    this.close_context_menu(cx);
                }
            }))
        } else {
            menu_div
        };

        if self.format_toolbar_owns_hover_menu() {
            return SelectionToolbarHoverRegion {
                child: menu_div.into_any_element(),
                editor: cx.entity().clone(),
                region: panel_id,
                menu_kind: None,
            }
            .into_any_element();
        }

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
        let table_context = self
            .context_menu
            .as_ref()
            .is_some_and(|menu| matches!(menu.kind, ContextMenuKind::Table { .. }));
        self.slash_menu_start.is_none()
            && (table_context || self.current_image_menu_target().is_none())
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
        let table_rows = self
            .context_menu
            .as_ref()
            .is_some_and(|menu| matches!(menu.kind, ContextMenuKind::Table { .. }))
            as u8 as f32;
        padding
            + item_height * (3.0 + chat_rows + 5.0 + 2.0 + 1.0 + table_rows)
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
            ContextMenuKind::Export { .. } => {
                self.scaled(self.styles.context_menu.menu_padding_y_px) * 2.0
                    + self.scaled(self.styles.context_menu.menu_item_height_px) * 4.0
            }
            ContextMenuKind::FormatHeading => self.format_heading_menu_height(),
            ContextMenuKind::FormatMore => self.format_more_menu_height(),
            ContextMenuKind::Table {
                table_start_line, ..
            } => {
                let is_front_matter = self
                    .decorations
                    .get(*table_start_line)
                    .and_then(|dec| dec.table_info.as_ref())
                    .is_some_and(|info| info.is_front_matter);
                let item_count = if is_front_matter { 4.0 } else { 7.0 };
                self.scaled(self.styles.context_menu.menu_padding_y_px) * 2.0
                    + self.scaled(self.styles.context_menu.menu_item_height_px) * item_count
                    + self.context_menu_separator_block_height() * 2.0
            }
            _ => self.format_more_menu_height(),
        }
    }

    fn render_export_context_submenu(
        &self,
        path: &Path,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        [
            (
                "Markdown",
                "assets/icons/menu/markdown.svg",
                super::info_panel::ExportFormat::Markdown,
            ),
            (
                "HTML",
                "assets/icons/menu/html.svg",
                super::info_panel::ExportFormat::Html,
            ),
            (
                "PDF",
                "assets/icons/menu/pdf.svg",
                super::info_panel::ExportFormat::Pdf,
            ),
            (
                "PNG",
                "assets/icons/menu/png.svg",
                super::info_panel::ExportFormat::Png,
            ),
        ]
        .into_iter()
        .map(|(label, icon_path, format)| {
            let path = path.to_path_buf();
            self.render_context_menu_item_with_icon_behavior(
                label,
                icon_path,
                false,
                false,
                cx,
                move |this, _, cx| {
                    this.close_context_menu(cx);
                    this.export_path(path.clone(), format, cx);
                },
            )
        })
        .collect()
    }

    fn export_submenu_layout(
        &self,
        main_layout: ContextMenuPanelLayout,
        main_menu_width: f32,
        submenu_width: f32,
        submenu_height: f32,
    ) -> ContextMenuPanelLayout {
        let gap = self.scaled(self.styles.context_menu.menu_submenu_gap_px);
        let right_space = main_layout.overlay_width
            - (main_layout.left + main_menu_width)
            - gap
            - main_layout.margin;
        let left = if right_space >= submenu_width {
            main_layout.left + main_menu_width + gap
        } else {
            (main_layout.left - submenu_width - gap).max(main_layout.margin)
        };
        let visible_height = submenu_height
            .min((main_layout.overlay_height - 2.0 * main_layout.margin).max(self.scaled(100.0)));
        let desired_top = self
            .export_trigger_bounds
            .get()
            .map(|bounds| {
                let (_, trigger_top) = self.context_menu_overlay_coordinates(bounds.origin);
                aligned_submenu_top(
                    trigger_top,
                    trigger_top + f32::from(bounds.size.height),
                    visible_height,
                    main_layout.overlay_height,
                    main_layout.margin,
                )
            })
            .unwrap_or(main_layout.estimated_top);
        let estimated_top = desired_top.clamp(
            main_layout.margin,
            (main_layout.overlay_height - main_layout.margin - visible_height)
                .max(main_layout.margin),
        );

        ContextMenuPanelLayout {
            left,
            estimated_top,
            max_height: (main_layout.overlay_height - 2.0 * main_layout.margin)
                .max(self.scaled(100.0)),
            bottom_anchor: None,
            overlay_width: main_layout.overlay_width,
            overlay_height: main_layout.overlay_height,
            margin: main_layout.margin,
        }
    }

    fn render_table_context_submenu(
        &self,
        table_start_line: usize,
        row: usize,
        col: usize,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut items = Vec::new();
        let is_front_matter = self
            .decorations
            .get(table_start_line)
            .and_then(|dec| dec.table_info.as_ref())
            .is_some_and(|info| info.is_front_matter);
        if !is_front_matter {
            items.push(self.render_context_menu_item_with_icon(
                self.t(crate::i18n::TextKey::AlignLeft),
                TABLE_ALIGN_LEFT_SVG,
                false,
                cx,
                move |this, _, cx| {
                    this.table_set_column_alignment(
                        table_start_line,
                        col,
                        crate::markdown::TableAlign::Left,
                        cx,
                    )
                },
            ));
            items.push(self.render_context_menu_item_with_icon(
                self.t(crate::i18n::TextKey::AlignCenter),
                TABLE_ALIGN_CENTER_SVG,
                false,
                cx,
                move |this, _, cx| {
                    this.table_set_column_alignment(
                        table_start_line,
                        col,
                        crate::markdown::TableAlign::Center,
                        cx,
                    )
                },
            ));
            items.push(self.render_context_menu_item_with_icon(
                self.t(crate::i18n::TextKey::AlignRight),
                TABLE_ALIGN_RIGHT_SVG,
                false,
                cx,
                move |this, _, cx| {
                    this.table_set_column_alignment(
                        table_start_line,
                        col,
                        crate::markdown::TableAlign::Right,
                        cx,
                    )
                },
            ));
            items.push(self.render_context_menu_separator());
        }
        items.push(self.render_context_menu_item_with_icon(
            self.t(crate::i18n::TextKey::AddRowAbove),
            TABLE_ROWS_SVG,
            false,
            cx,
            move |this, _, cx| this.table_insert_row(table_start_line, row, true, cx),
        ));
        items.push(self.render_context_menu_item_with_icon(
            self.t(crate::i18n::TextKey::AddRowBelow),
            TABLE_ROWS_SVG,
            false,
            cx,
            move |this, _, cx| this.table_insert_row(table_start_line, row, false, cx),
        ));
        if !is_front_matter {
            items.push(self.render_context_menu_item_with_icon(
                self.t(crate::i18n::TextKey::AddColumnLeft),
                TABLE_COLUMNS_SVG,
                false,
                cx,
                move |this, _, cx| this.table_insert_column(table_start_line, col, true, cx),
            ));
            items.push(self.render_context_menu_item_with_icon(
                self.t(crate::i18n::TextKey::AddColumnRight),
                TABLE_COLUMNS_SVG,
                false,
                cx,
                move |this, _, cx| this.table_insert_column(table_start_line, col, false, cx),
            ));
        }
        items.push(self.render_context_menu_separator());
        items.push(self.render_context_menu_item_with_icon(
            self.t(crate::i18n::TextKey::DeleteRow),
            TABLE_DELETE_SVG,
            false,
            cx,
            move |this, _, cx| this.table_delete_row(table_start_line, row, cx),
        ));
        if !is_front_matter {
            items.push(self.render_context_menu_item_with_icon(
                self.t(crate::i18n::TextKey::DeleteColumn),
                TABLE_DELETE_SVG,
                false,
                cx,
                move |this, _, cx| this.table_delete_column(table_start_line, col, cx),
            ));
        }
        items.push(self.render_context_menu_separator());
        items.push(self.render_context_menu_item_with_icon(
            if is_front_matter {
                self.t(crate::i18n::TextKey::DeleteFrontMatter)
            } else {
                self.t(crate::i18n::TextKey::DeleteTable)
            },
            TABLE_GRID_SVG,
            false,
            cx,
            move |this, _, cx| this.table_delete_table(table_start_line, cx),
        ));
        items
    }

    fn render_table_size_picker(
        &self,
        table_start_line: usize,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        const MAX_ROWS: usize = crate::table_ops::TABLE_SIZE_PICKER_MAX_ROWS;
        const MAX_COLUMNS: usize = crate::table_ops::TABLE_SIZE_PICKER_MAX_COLUMNS;

        let (current_rows, current_columns) = self
            .table_lines_for_block(table_start_line)
            .map(|(_, _, lines)| {
                let model = crate::table_ops::parse_table_block(&lines);
                (model.data_rows.len().max(1), model.col_count.max(1))
            })
            .unwrap_or((1, 1));
        let (hover_rows, hover_columns) = self
            .table_size_hover
            .unwrap_or((current_rows.min(MAX_ROWS), current_columns.min(MAX_COLUMNS)));
        let ms = &self.styles.context_menu;
        let cell_size = self.scaled(9.0);
        let gap = self.scaled(3.0);
        let padding = self.scaled(ms.menu_padding_y_px);
        let current_bg = crate::theme::with_alpha(ms.menu_format_active_text_color, 0.46);
        let hover_bg = crate::theme::with_alpha(ms.menu_format_active_text_color, 0.22);
        let overlap_bg = crate::theme::with_alpha(ms.menu_format_active_text_color, 0.62);
        let mut rows = Vec::with_capacity(MAX_ROWS);

        for row in 1..=MAX_ROWS {
            let mut cells = Vec::with_capacity(MAX_COLUMNS);
            for column in 1..=MAX_COLUMNS {
                let hovered = row <= hover_rows && column <= hover_columns;
                let currently_covered = row <= current_rows && column <= current_columns;
                cells.push(
                    div()
                        .id(SharedString::from(format!(
                            "table-size-{table_start_line}-{row}-{column}"
                        )))
                        .w(px(cell_size))
                        .h(px(cell_size))
                        .border(px(self.scaled(1.0)))
                        .border_color(if hovered || currently_covered {
                            ms.menu_format_active_text_color
                        } else {
                            ms.menu_border_color
                        })
                        .bg(if hovered && currently_covered {
                            overlap_bg
                        } else if currently_covered {
                            current_bg
                        } else if hovered {
                            hover_bg
                        } else {
                            gpui::transparent_black()
                        })
                        .cursor(CursorStyle::Arrow)
                        .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                            if *hovered && this.table_size_hover != Some((row, column)) {
                                this.table_size_hover = Some((row, column));
                                cx.notify();
                            }
                        }))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, window, cx| {
                                window.prevent_default();
                                cx.stop_propagation();
                                this.table_resize(table_start_line, row, column, cx);
                            }),
                        ),
                );
            }
            rows.push(div().flex().gap(px(gap)).children(cells));
        }

        vec![
            div()
                .p(px(padding))
                .flex()
                .flex_col()
                .gap(px(gap))
                .child(
                    div()
                        .h(px(self.scaled(ms.menu_item_height_px) - padding))
                        .flex()
                        .items_center()
                        .text_size(px(self.scaled(ms.menu_item_font_size_px)))
                        .text_color(ms.menu_item_primary_text_color)
                        .child(format!("{} × {}", hover_columns, hover_rows)),
                )
                .children(rows)
                .into_any_element(),
        ]
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
            ContextMenuKind::FileList { path } => (
                self.render_file_list_context_menu(path.clone(), cx),
                self.scaled(self.styles.context_menu.menu_sidebar_width_px),
            ),
            ContextMenuKind::SidebarSort => {
                let mut items = Vec::new();
                for mode in [
                    SidebarSortMode::CreatedDesc,
                    SidebarSortMode::CreatedAsc,
                    SidebarSortMode::NameAsc,
                    SidebarSortMode::NameDesc,
                ] {
                    items.push(self.render_context_menu_choice_item(
                        self.t(mode.label_key()),
                        self.sidebar_sort_mode == mode,
                        cx,
                        move |this, _, cx| {
                            this.sidebar_sort_mode = mode;
                            this.app_settings.set_sidebar_sort_mode(mode);
                            this.app_settings.save();
                            if this.workspace_search_visible {
                                this.clear_workspace_search_preview_state();
                                this.sort_workspace_search_results();
                                this.workspace_search_scroll_handle.scroll_to_item(0);
                                this.workspace_search_render_limit =
                                    super::list_viewport::SEARCH_LIST_POLICY.initial_count;
                            }
                            this.invalidate_sidebar_items_cache();
                            this.context_menu = None;
                            cx.notify();
                        },
                    ));
                }
                items.push(self.render_context_menu_separator());
                for mode in [
                    super::FileListDisplayMode::Title,
                    super::FileListDisplayMode::FileName,
                ] {
                    items.push(self.render_context_menu_choice_item(
                        self.t(mode.label_key()),
                        self.file_list_display_mode == mode,
                        cx,
                        move |this, _, cx| {
                            this.file_list_display_mode = mode;
                            this.app_settings.set_file_list_display_mode(mode);
                            this.app_settings.save();
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
                            this.reveal_in_file_manager(&path);
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
            ContextMenuKind::Export { .. } => (
                Vec::new(),
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
                self.append_tab_pane_context_menu_items(&mut items, tab_id, cx);
                if let Some(tab) = self.tabs.iter().find(|t| t.id == *tab_id) {
                    if let Some(path) = &tab.path {
                        if is_markdown_file(path) {
                            items.push(self.render_context_menu_separator());
                            items.push(self.render_format_submenu_trigger_item(
                                self.t(crate::i18n::TextKey::Export),
                                None,
                                ContextMenuKind::Export { path: path.clone() },
                                cx,
                            ));
                        }
                        let chat_path = path.clone();
                        items.push(self.render_context_menu_item(
                            crate::i18n::add_file_to_chat_label(self.ui_language),
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
                                this.locate_path_in_sidebar(reveal_path.clone(), cx);
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
                                    this.reveal_in_file_manager(&reveal_path);
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
            ContextMenuKind::Terminal { tab_id } => {
                let mut items = Vec::new();
                {
                    let id = tab_id.clone();
                    items.push(self.render_context_menu_item(
                        self.t(crate::i18n::TextKey::Copy),
                        false,
                        cx,
                        move |this, _, cx| {
                            this.close_context_menu(cx);
                            let _ = this.copy_chat_terminal_selection(&id, cx);
                        },
                    ));
                }
                {
                    let id = tab_id.clone();
                    items.push(self.render_context_menu_item(
                        self.t(crate::i18n::TextKey::Paste),
                        false,
                        cx,
                        move |this, _, cx| {
                            this.close_context_menu(cx);
                            this.paste_chat_terminal_from_clipboard(&id, cx);
                        },
                    ));
                }
                {
                    let id = tab_id.clone();
                    items.push(self.render_context_menu_item(
                        self.t(crate::i18n::TextKey::SelectAll),
                        false,
                        cx,
                        move |this, _, cx| {
                            this.close_context_menu(cx);
                            let _ = this.select_all_chat_terminal(&id, cx);
                        },
                    ));
                }
                {
                    let id = tab_id.clone();
                    items.push(self.render_context_menu_item(
                        self.t(crate::i18n::TextKey::ClearTerminal),
                        false,
                        cx,
                        move |this, _, cx| {
                            this.close_context_menu(cx);
                            this.clear_terminal(&id, cx);
                        },
                    ));
                }
                self.append_tab_pane_context_menu_items(&mut items, tab_id, cx);
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
            ContextMenuKind::TerminalTabCreate => {
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
                            this.create_terminal_tab(agent_cli, cx);
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
                                            this.suppress_editor_mouse_up_for_current_menu_event(
                                                cx,
                                            );
                                            this.close_workspace(&ws_close, cx);
                                        }),
                                    )
                                    .on_mouse_up(
                                        MouseButton::Left,
                                        cx.listener(|_, _, window, cx| {
                                            window.prevent_default();
                                            cx.stop_propagation();
                                        }),
                                    ),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, window, cx| {
                                    window.prevent_default();
                                    cx.stop_propagation();
                                    this.suppress_editor_mouse_up_for_current_menu_event(cx);
                                    this.close_context_menu(cx);
                                    this.switch_workspace(ws_switch.clone(), cx);
                                }),
                            )
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|_, _, window, cx| {
                                    window.prevent_default();
                                    cx.stop_propagation();
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
                    "assets/icons/appearance/setting.svg",
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
                table_start_line: _,
                row: _,
                col: _,
            } => (
                self.render_format_context_menu(cx),
                self.scaled(self.styles.context_menu.menu_format_width_px),
            ),
            ContextMenuKind::TableResize { table_start_line } => (
                self.render_table_size_picker(*table_start_line, cx),
                self.scaled(105.0),
            ),
        };

        let estimated_menu_height = self.estimated_context_menu_height(items.len());
        let anchor_height = self.context_menu_anchor_height(&menu.kind);
        let toolbar_bounds = self
            .format_toolbar_owns_hover_menu()
            .then(|| {
                self.line_format_toolbar_line
                    .and_then(|_| self.line_format_toolbar_bounds)
                    .or_else(|| {
                        self.selection_toolbar_geometry
                            .get(&self.focused_pane_id)
                            .and_then(|geometry| geometry.toolbar_bounds)
                    })
            })
            .flatten();
        // Recompute from the live layout: filtering changes the row count and
        // the cached position can predate the frame that laid out this line.
        let anchor_position = self
            .slash_menu_position()
            .filter(|_| matches!(menu.kind, ContextMenuKind::Format))
            .unwrap_or(menu.position);
        let mut layout = if let Some(bounds) = toolbar_bounds.filter(|_| {
            matches!(
                menu.kind,
                ContextMenuKind::FormatHeading | ContextMenuKind::FormatMore
            )
        }) {
            let border_w = self.styles.window.window_border_width_px;
            let overlay_width = self.last_window_width - 2.0 * border_w;
            let overlay_height = self.last_window_height - 2.0 * border_w;
            let margin = self
                .scaled(self.styles.context_menu.menu_max_height_margin_px)
                .max(4.0);
            let gap = self.scaled(self.styles.context_menu.menu_submenu_gap_px);
            let (toolbar_left, toolbar_top) = self.context_menu_overlay_coordinates(bounds.origin);
            let (left, top) = toolbar_side_menu_position(
                toolbar_left,
                toolbar_top,
                f32::from(bounds.size.width),
                f32::from(bounds.size.height),
                width,
                estimated_menu_height,
                overlay_width,
                overlay_height,
                gap,
                margin,
                matches!(menu.kind, ContextMenuKind::FormatMore),
            );
            let align_bottom = matches!(menu.kind, ContextMenuKind::FormatMore);
            ContextMenuPanelLayout {
                left,
                estimated_top: if align_bottom { 0.0 } else { top },
                max_height: (overlay_height - 2.0 * margin).max(self.scaled(100.0)),
                bottom_anchor: align_bottom.then(|| {
                    (overlay_height - (toolbar_top + f32::from(bounds.size.height))).max(0.0)
                }),
                overlay_width,
                overlay_height,
                margin,
            }
        } else if matches!(menu.kind, ContextMenuKind::Workspace) {
            self.workspace_bar_bounds
                .get()
                .map(|anchor| {
                    self.button_context_menu_panel_layout(anchor, width, estimated_menu_height)
                })
                .unwrap_or_else(|| {
                    self.context_menu_panel_layout_with_anchor(
                        anchor_position,
                        width,
                        estimated_menu_height,
                        anchor_height,
                    )
                })
        } else {
            self.context_menu_panel_layout_with_anchor(
                anchor_position,
                width,
                estimated_menu_height,
                anchor_height,
            )
        };
        if matches!(menu.kind, ContextMenuKind::TableResize { .. }) {
            let (anchor_x, anchor_y) = self.context_menu_overlay_coordinates(anchor_position);
            layout.left = anchor_x;
            layout.estimated_top = anchor_y;
            layout.bottom_anchor = None;
            layout.max_height = (layout.overlay_height - anchor_y).max(1.0);
        }
        let main_menu = self.render_context_menu_panel("context-menu", layout, width, items, cx);

        let export_submenu_open =
            matches!(self.context_submenu, Some(ContextMenuKind::Export { .. }));
        let format_submenu_open = matches!(
            menu.kind,
            ContextMenuKind::Format | ContextMenuKind::Table { .. }
        ) && self.context_submenu.is_some()
            && self.can_render_format_submenu();
        let submenu = if export_submenu_open || format_submenu_open {
            let submenu_kind = self.context_submenu.as_ref().cloned();
            let submenu_items = match submenu_kind.as_ref() {
                Some(ContextMenuKind::Export { path }) => {
                    self.render_export_context_submenu(path, cx)
                }
                Some(ContextMenuKind::FormatHeading) => self.render_heading_context_menu(cx),
                Some(ContextMenuKind::FormatMore) => self.render_format_more_context_menu(cx),
                Some(ContextMenuKind::Table {
                    table_start_line,
                    row,
                    col,
                }) => self.render_table_context_submenu(*table_start_line, *row, *col, cx),
                _ => Vec::new(),
            };
            let submenu_width = if export_submenu_open {
                self.scaled(self.styles.context_menu.menu_export_width_px)
            } else {
                self.scaled(self.styles.context_menu.menu_format_width_px)
            };
            let submenu_layout = match submenu_kind.as_ref() {
                Some(kind @ ContextMenuKind::Export { .. }) => self.export_submenu_layout(
                    layout,
                    width,
                    submenu_width,
                    self.format_submenu_height(kind),
                ),
                Some(kind) => {
                    self.format_submenu_layout(menu, kind)
                        .unwrap_or(ContextMenuPanelLayout {
                            left: layout.left + width,
                            estimated_top: layout.estimated_top,
                            max_height: layout.max_height,
                            bottom_anchor: None,
                            overlay_width: layout.overlay_width,
                            overlay_height: layout.overlay_height,
                            margin: layout.margin,
                        })
                }
                None => layout,
            };
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

        let mut backdrop = Vec::new();
        if let Some(bounds) = toolbar_bounds {
            let (left, top) = self.context_menu_overlay_coordinates(bounds.origin);
            let right = left + f32::from(bounds.size.width);
            let bottom = top + f32::from(bounds.size.height);
            for (x, y, width, height) in [
                (0.0, 0.0, layout.overlay_width, top),
                (
                    0.0,
                    bottom,
                    layout.overlay_width,
                    layout.overlay_height - bottom,
                ),
                (0.0, top, left, bottom - top),
                (right, top, layout.overlay_width - right, bottom - top),
            ] {
                backdrop.push(
                    div()
                        .absolute()
                        .left(px(x))
                        .top(px(y))
                        .w(px(width.max(0.0)))
                        .h(px(height.max(0.0)))
                        .occlude()
                        .on_scroll_wheel(cx.listener(
                            |this, event: &gpui::ScrollWheelEvent, window, cx| {
                                let over_editor = this
                                    .selection_toolbar_geometry
                                    .get(&this.focused_pane_id)
                                    .is_some_and(|geometry| {
                                        geometry.viewport.contains(&event.position)
                                    });
                                if over_editor {
                                    this.on_scroll(event, window, cx);
                                }
                                cx.stop_propagation();
                            },
                        ))
                        .into_any_element(),
                );
            }
        }

        div()
            .id("context-menu-anchor")
            .absolute()
            .left(px(0.0))
            .top(px(0.0))
            .w_full()
            .h_full()
            .when(toolbar_bounds.is_none(), |this| this.occlude())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    if toolbar_bounds
                        .is_some_and(|bounds| bounds.contains(&window.mouse_position()))
                    {
                        return;
                    }
                    window.prevent_default();
                    cx.stop_propagation();
                    this.close_context_menu(cx);
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|_, _, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|_, _, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                }),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|_, _, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                }),
            )
            .children(backdrop)
            .child(main_menu)
            .when_some(submenu, |this, submenu| this.child(submenu))
            .into_any_element()
    }
}

fn line_format_action(block_type: &BlockType) -> FormatMenuAction {
    match block_type {
        BlockType::Heading(1) => FormatMenuAction::Heading1,
        BlockType::Heading(2) => FormatMenuAction::Heading2,
        BlockType::Heading(3) => FormatMenuAction::Heading3,
        BlockType::Heading(4) => FormatMenuAction::Heading4,
        BlockType::Heading(5) => FormatMenuAction::Heading5,
        BlockType::Heading(6) => FormatMenuAction::Heading6,
        BlockType::Blockquote => FormatMenuAction::Quote,
        BlockType::UnorderedList => FormatMenuAction::UnorderedList,
        BlockType::OrderedList => FormatMenuAction::OrderedList,
        BlockType::TaskList(_) => FormatMenuAction::Todo,
        BlockType::CodeBlock | BlockType::CodeBlockFence => FormatMenuAction::CodeBlock,
        BlockType::HorizontalRule => FormatMenuAction::HorizontalRule,
        BlockType::Table => FormatMenuAction::Table,
        BlockType::Normal | BlockType::Heading(_) => FormatMenuAction::Paragraph,
    }
}

#[cfg(test)]
mod format_toolbar_layout_tests {
    use super::*;

    fn metrics() -> FormatToolbarMetrics {
        FormatToolbarMetrics {
            button: 22.0,
            gap: 1.0,
            padding: 8.0,
            border: 1.0,
        }
    }

    #[test]
    fn toolbar_has_the_requested_four_rows() {
        let ([first, second, third, fourth], width) = format_toolbar_layout(metrics());
        let actions = |entries: &[FormatToolbarEntry]| {
            entries
                .iter()
                .filter_map(|entry| match entry {
                    FormatToolbarEntry::Action { action, .. } => Some(*action),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert!(matches!(
            first.as_slice(),
            [FormatToolbarEntry::HeadingMenu]
        ));
        assert!(
            actions(&second)
                == vec![
                    FormatMenuAction::Bold,
                    FormatMenuAction::Italic,
                    FormatMenuAction::Strikethrough,
                    FormatMenuAction::Underline,
                    FormatMenuAction::InlineCode,
                ]
        );
        assert!(
            actions(&third)
                == vec![
                    FormatMenuAction::Todo,
                    FormatMenuAction::UnorderedList,
                    FormatMenuAction::OrderedList,
                    FormatMenuAction::Link,
                    FormatMenuAction::Image,
                ]
        );
        assert!(matches!(fourth.last(), Some(FormatToolbarEntry::MoreMenu)));
        assert!(
            actions(&fourth)
                == vec![
                    FormatMenuAction::Table,
                    FormatMenuAction::CodeBlock,
                    FormatMenuAction::Quote,
                    FormatMenuAction::HorizontalRule,
                ]
        );
        assert_eq!(width, metrics().width(&second));
    }

    #[test]
    fn line_toolbar_uses_only_the_block_level_action() {
        assert!(line_format_action(&BlockType::Heading(2)) == FormatMenuAction::Heading2);
        assert!(line_format_action(&BlockType::Normal) == FormatMenuAction::Paragraph);
        assert!(line_format_action(&BlockType::Table) == FormatMenuAction::Table);
    }
}

#[cfg(test)]
mod submenu_layout_tests {
    use super::{aligned_submenu_top, toolbar_side_menu_position};

    #[test]
    fn submenu_top_aligns_with_trigger_when_space_below_is_available() {
        assert_eq!(aligned_submenu_top(120.0, 144.0, 180.0, 600.0, 8.0), 120.0);
    }

    #[test]
    fn submenu_bottom_aligns_with_trigger_when_only_space_above_is_available() {
        assert_eq!(aligned_submenu_top(520.0, 544.0, 180.0, 600.0, 8.0), 364.0);
    }

    #[test]
    fn toolbar_menu_uses_available_side_and_requested_vertical_edge() {
        assert_eq!(
            toolbar_side_menu_position(
                200.0, 100.0, 120.0, 140.0, 160.0, 90.0, 800.0, 600.0, 4.0, 8.0, false,
            ),
            (324.0, 100.0)
        );
        assert_eq!(
            toolbar_side_menu_position(
                650.0, 300.0, 120.0, 140.0, 160.0, 90.0, 800.0, 600.0, 4.0, 8.0, true,
            ),
            (486.0, 350.0)
        );
    }
}
