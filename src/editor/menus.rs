use std::collections::HashMap;
use std::io::Cursor;
use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, ClipboardItem, Context, CursorStyle, Image, ImageFormat, MouseButton,
    MouseDownEvent, MouseMoveEvent, Pixels, Point, SharedString, Window, div, prelude::*, px, svg,
};

use crate::markdown::{BlockType, InlineKind, LineDecoration, raw_wiki_link_ranges};

use super::shortcut::{ShortcutRenderStyle, render_shortcut};
use super::*;

// Menu construction must not probe mounts just to decide which actions to show.
fn lexical_menu_image_path(
    raw: &str,
    file_dir: Option<&Path>,
    root: Option<&Path>,
) -> Option<PathBuf> {
    if raw.is_empty() || helpers::is_remote_image_url(raw) {
        return None;
    }
    if crate::platform::paths::is_file_url(raw) {
        return crate::platform::paths::path_from_file_url(raw);
    }
    let decoded = percent_encoding::percent_decode_str(raw).decode_utf8_lossy();
    let path = crate::platform::paths::expand_tilde(&decoded)?;
    Some(if path.is_absolute() {
        path
    } else if let Some(base) = file_dir.or(root) {
        base.join(path)
    } else {
        path
    })
}

#[cfg(test)]
mod filesystem_free_menu_tests {
    use super::lexical_menu_image_path;
    use std::path::{Path, PathBuf};

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn export_submenu_has_eight_rows_and_reserves_their_height(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext;
        let editor = cx.update(|cx| {
            cx.new(|cx| super::EditorView::new_with_mode(cx, true, false, Some("draft"), false))
        });
        editor.update(cx, |editor, cx| {
            let path = PathBuf::from("unavailable/note.md");
            let rows = editor.render_export_context_submenu(&path, cx);
            assert_eq!(rows.len(), 8);
            assert_eq!(
                editor.format_submenu_height(&super::ContextMenuKind::Export { path }),
                editor.scaled(editor.styles.context_menu.menu_padding_y_px) * 2.0
                    + editor.scaled(editor.styles.context_menu.menu_item_height_px) * 10.0,
            );
        });
    }

    #[test]
    fn image_paths_are_lexical_even_when_the_mount_is_unavailable() {
        let base = Path::new("unavailable-image-mount");
        assert_eq!(
            lexical_menu_image_path("images/missing%20image.png", Some(base), None),
            Some(base.join("images/missing image.png"))
        );
        assert_eq!(
            lexical_menu_image_path("missing.png", None, Some(base)),
            Some(base.join("missing.png"))
        );
        assert_eq!(
            lexical_menu_image_path("missing.png", None, None),
            Some(PathBuf::from("missing.png"))
        );
    }

    #[test]
    fn remote_images_do_not_become_local_menu_paths() {
        assert!(lexical_menu_image_path("https://example.test/image.png", None, None).is_none());
        assert!(lexical_menu_image_path("", None, None).is_none());
    }

    #[test]
    fn file_urls_are_decoded_without_probing_the_target() {
        let expected =
            crate::platform::paths::path_from_file_url("file:///unavailable/image%20one.png");
        assert_eq!(
            lexical_menu_image_path("file:///unavailable/image%20one.png", None, None),
            expected
        );
    }
}

fn wrapped_menu_anchor(
    wrapped: &WrappedLine,
    position: Point<Pixels>,
    line_height: f32,
    content_x: f32,
    text_top: f32,
) -> Point<Pixels> {
    let row = wrapped.row_for_y(position.y, px(line_height));
    gpui::point(
        px(content_x) + position.x,
        px(text_top)
            + wrapped.row_top(row, px(line_height))
            + wrapped.row_height(row, px(line_height)),
    )
}

#[cfg(test)]
#[test]
fn input_menu_anchors_follow_actual_row_bottoms() {
    let wrapped = super::input_wrap::test_wrapped_line(&[24.0, 60.0, 24.0]);
    let tall_row = wrapped_menu_anchor(
        &wrapped,
        gpui::point(px(10.0), px(24.0)),
        24.0,
        100.0,
        200.0,
    );
    assert_eq!(tall_row, gpui::point(px(110.0), px(284.0)));
    let following_row =
        wrapped_menu_anchor(&wrapped, gpui::point(px(8.0), px(84.0)), 24.0, 100.0, 200.0);
    assert_eq!(following_row, gpui::point(px(108.0), px(308.0)));
    let uniform = super::input_wrap::test_wrapped_line(&[]);
    assert_eq!(
        wrapped_menu_anchor(&uniform, gpui::point(px(8.0), px(48.0)), 24.0, 100.0, 200.0),
        gpui::point(px(108.0), px(272.0))
    );
}

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

fn front_matter_type_label_key(ty: front_matter::FrontMatterType) -> &'static str {
    use front_matter::FrontMatterType::*;
    match ty {
        Text => "text",
        MultilineText => "multiline_text",
        Checkbox => "checkbox",
        Date => "date",
        DateTime => "date_time",
        List => "list",
        Number => "number",
        Empty => "empty",
        Object => "object",
        Raw => "raw",
    }
}

fn front_matter_type_menu_matches(
    current_pane: PaneId,
    current_tab: Option<&str>,
    current_source: &str,
    pane: PaneId,
    tab: Option<&str>,
    source: &str,
) -> bool {
    current_pane == pane && current_tab == tab && current_source == source
}

fn front_matter_type_choice_enabled(
    current: Option<front_matter::FrontMatterType>,
    target: front_matter::FrontMatterType,
    read_only: bool,
    can_convert: impl FnOnce() -> bool,
) -> bool {
    current.is_some()
        && target != front_matter::FrontMatterType::Raw
        && !read_only
        // The planner validates the property span and may reset incompatible values.
        && (current == Some(target) || can_convert())
}

// Global icon tooltips are suppressed while menus are open. Menu explanations
// use GPUI's item tooltip instead, with the same theme and wrapping behavior.
struct FrontMatterTypeMenuTooltip {
    editor: gpui::Entity<EditorView>,
    text: SharedString,
}

impl gpui::Render for FrontMatterTypeMenuTooltip {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let editor = self.editor.read(cx);
        let styles = &editor.styles.tooltip;
        let font_size = editor.scaled(styles.tooltip_title_font_size_px);
        let mut surface = editor.styles.tooltip_surface();
        surface.shadow = crate::style::ShadowStyle::default();
        crate::ui::surface::surface(&surface, editor.ui_scale)
            .max_w(px(editor.scaled(styles.tooltip_max_width_px)))
            .px(px(editor.scaled(styles.tooltip_padding_x_px)))
            .py(px(editor.scaled(styles.tooltip_padding_y_px)))
            .font_family(SharedString::from(editor.styles.ui_font_family))
            .text_size(px(font_size))
            .line_height(px(
                font_size * editor.styles.ui.spec.tooltip_line_height_factor
            ))
            .text_color(styles.tooltip_title_text_color)
            .child(tooltip::render_tooltip_text(
                editor,
                window,
                &self.text,
                false,
                editor.scaled(
                    styles.tooltip_max_width_px
                        - styles.tooltip_padding_x_px * 2.0
                        - styles.tooltip_border_width_px * 2.0,
                ),
            ))
    }
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

#[derive(Default)]
pub(crate) struct TableAdjustmentMenu {
    popup_table: Option<usize>,
    bounds: Cell<Option<Bounds<Pixels>>>,
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
        if self.region == "table-adjustment-menu" {
            self.editor
                .read(cx)
                .table_adjustment_menu
                .bounds
                .set(Some(bounds));
        }
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
                    if let ContextMenuKind::TableResize { table_start_line } = kind
                        && region == "table-cells-trigger"
                    {
                        if this.table_adjustment_menu.popup_table != Some(*table_start_line) {
                            this.table_adjustment_menu.popup_table = Some(*table_start_line);
                            this.table_size_hover = None;
                            cx.notify();
                        }
                    } else if matches!(kind, ContextMenuKind::FormatHeading) {
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

fn format_toolbar_layout(metrics: FormatToolbarMetrics) -> ([Vec<FormatToolbarEntry>; 3], f32) {
    use FormatToolbarEntry::MoreMenu;

    let hint = crate::editor::shortcut::hint;
    let action = |action, icon, shortcut: Option<SharedString>| FormatToolbarEntry::Action {
        action,
        icon,
        shortcut,
    };
    let first_row = vec![
        action(
            FormatMenuAction::Heading1,
            HEADING_1_SVG_PATH,
            Some(hint("secondary-1")),
        ),
        action(
            FormatMenuAction::Heading2,
            HEADING_2_SVG_PATH,
            Some(hint("secondary-2")),
        ),
        action(
            FormatMenuAction::Heading3,
            HEADING_3_SVG_PATH,
            Some(hint("secondary-3")),
        ),
        action(
            FormatMenuAction::Heading4,
            HEADING_4_SVG_PATH,
            Some(hint("secondary-4")),
        ),
        action(
            FormatMenuAction::Heading5,
            HEADING_5_SVG_PATH,
            Some(hint("secondary-5")),
        ),
        action(
            FormatMenuAction::Heading6,
            HEADING_6_SVG_PATH,
            Some(hint("secondary-6")),
        ),
        action(
            FormatMenuAction::Paragraph,
            FMT_PARAGRAPH_SVG,
            Some(hint("secondary-0")),
        ),
    ];
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
        action(
            FormatMenuAction::Link,
            FMT_LINK_SVG,
            Some(hint("secondary-k")),
        ),
        action(FormatMenuAction::Image, FMT_IMAGE_SVG, None),
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
            FormatMenuAction::Quote,
            FMT_QUOTE_SVG,
            Some(hint("secondary-alt-q")),
        ),
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
        MoreMenu,
    ];
    let width = metrics.width(&second_row);
    ([first_row, second_row, third_row], width)
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

fn context_more_actions() -> [(FormatMenuAction, &'static str, Option<SharedString>); 7] {
    let hint = crate::editor::shortcut::hint;
    [
        (
            FormatMenuAction::UnorderedList,
            FMT_LIST_SVG,
            Some(hint("secondary-alt-u")),
        ),
        (
            FormatMenuAction::OrderedList,
            FMT_ORDER_LIST_SVG,
            Some(hint("secondary-alt-o")),
        ),
        (FormatMenuAction::Image, FMT_IMAGE_SVG, None),
        (
            FormatMenuAction::Table,
            FMT_TABLE_SVG,
            Some(hint("secondary-alt-t")),
        ),
        (
            FormatMenuAction::CodeBlock,
            FMT_CODE_SVG,
            Some(hint("secondary-alt-c")),
        ),
        (
            FormatMenuAction::Quote,
            FMT_QUOTE_SVG,
            Some(hint("secondary-alt-q")),
        ),
        (FormatMenuAction::HorizontalRule, FMT_SPLIT_SVG, None),
    ]
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

fn table_cells_popup_layout(
    parent: Bounds<Pixels>,
    overlay: ContextMenuPanelLayout,
    width: f32,
    gap: f32,
) -> ContextMenuPanelLayout {
    let (left, top) = toolbar_side_menu_position(
        parent.left().into(),
        parent.top().into(),
        parent.size.width.into(),
        parent.size.height.into(),
        width,
        0.0,
        overlay.overlay_width,
        overlay.overlay_height,
        gap,
        overlay.margin,
        false,
    );
    ContextMenuPanelLayout {
        left,
        estimated_top: top,
        bottom_anchor: None,
        max_height: (overlay.overlay_height - top - overlay.margin).max(1.0),
        ..overlay
    }
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

const TABLE_CONTEXT_MENU_WIDTH_PX: f32 = 188.0;

#[derive(Clone, Copy)]
struct TableMenuLayout {
    rows: bool,
    columns: bool,
    alignment: bool,
    paste_insert: bool,
    clear: bool,
}

impl TableMenuLayout {
    fn new(is_front_matter: bool) -> Self {
        Self {
            rows: true,
            columns: !is_front_matter,
            alignment: !is_front_matter,
            paste_insert: !is_front_matter,
            clear: true,
        }
    }

    fn counts(self) -> (usize, usize) {
        let has_structure_actions = self.rows || self.columns;
        let items = 3 * self.alignment as usize
            + 3 * self.rows as usize
            + 3 * self.columns as usize
            + 1 // Delete Table / Front Matter
            + 3 // Copy, Cut, Paste Overwrite
            + self.paste_insert as usize
            + self.clear as usize;
        let separators = self.alignment as usize
            + 2 * has_structure_actions as usize
            + 1 // Structure / clipboard
            + self.clear as usize;
        (items, separators)
    }
}

struct TableMenuSelection {
    table: usize,
    rows: Vec<usize>,
    columns: Vec<usize>,
    first_row: usize,
    last_row: usize,
    first_column: usize,
    last_column: usize,
    is_front_matter: bool,
    border_axis: Option<bool>,
}

impl TableMenuSelection {
    fn for_border(
        table: usize,
        is_column: bool,
        index: usize,
        border: Option<&super::table_handles::TableBorderSelection>,
        is_front_matter: bool,
    ) -> Self {
        let (first, last) = border
            .and_then(|selection| selection.insertion_edges())
            .unwrap_or((index, index));
        let indices = border
            .map(|selection| selection.indices.clone())
            .unwrap_or_else(|| vec![index]);
        if is_column {
            Self {
                table,
                rows: vec![0],
                columns: indices,
                first_row: 0,
                last_row: 0,
                first_column: first,
                last_column: last,
                is_front_matter,
                border_axis: Some(true),
            }
        } else {
            Self {
                table,
                rows: indices,
                columns: vec![0],
                first_row: first,
                last_row: last,
                first_column: 0,
                last_column: 0,
                is_front_matter,
                border_axis: Some(false),
            }
        }
    }
}

impl EditorView {
    fn render_table_structure_menu(
        &self,
        selection: TableMenuSelection,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        use crate::i18n::TextKey;
        let TableMenuSelection {
            table,
            rows,
            columns,
            first_row,
            last_row,
            first_column,
            last_column,
            is_front_matter,
            border_axis,
        } = selection;
        let layout = TableMenuLayout::new(is_front_matter);
        let mut items = Vec::new();
        if layout.alignment {
            for (label, align, icon) in [
                (
                    TextKey::AlignLeft,
                    crate::markdown::TableAlign::Left,
                    TABLE_ALIGN_LEFT_SVG,
                ),
                (
                    TextKey::AlignCenter,
                    crate::markdown::TableAlign::Center,
                    TABLE_ALIGN_CENTER_SVG,
                ),
                (
                    TextKey::AlignRight,
                    crate::markdown::TableAlign::Right,
                    TABLE_ALIGN_RIGHT_SVG,
                ),
            ] {
                let columns = columns.clone();
                items.push(self.render_context_menu_item_with_icon(
                    self.t(label),
                    icon,
                    false,
                    cx,
                    move |this, _, cx| this.table_set_columns_alignment(table, &columns, align, cx),
                ));
            }
            items.push(self.render_context_menu_separator());
        }
        if layout.rows {
            for (label, row, above) in [
                (TextKey::AddRowAbove, first_row, true),
                (TextKey::AddRowBelow, last_row, false),
            ] {
                items.push(self.render_context_menu_item_with_icon(
                    self.t(label),
                    TABLE_ROWS_SVG,
                    false,
                    cx,
                    move |this, _, cx| this.table_insert_row(table, row, above, cx),
                ));
            }
        }
        if layout.columns {
            for (label, column, left) in [
                (TextKey::AddColumnLeft, first_column, true),
                (TextKey::AddColumnRight, last_column, false),
            ] {
                items.push(self.render_context_menu_item_with_icon(
                    self.t(label),
                    TABLE_COLUMNS_SVG,
                    false,
                    cx,
                    move |this, _, cx| this.table_insert_column(table, column, left, cx),
                ));
            }
        }
        if layout.rows || layout.columns {
            items.push(self.render_context_menu_separator());
        }
        if layout.rows {
            items.push(self.render_context_menu_item_with_icon(
                self.t(TextKey::DeleteRow),
                TABLE_DELETE_SVG,
                false,
                cx,
                move |this, _, cx| {
                    if border_axis == Some(false) {
                        this.delete_table_border_selection(cx);
                    } else {
                        this.table_delete_rows(table, &rows, cx);
                    }
                },
            ));
        }
        if layout.columns {
            items.push(self.render_context_menu_item_with_icon(
                self.t(TextKey::DeleteColumn),
                TABLE_DELETE_SVG,
                false,
                cx,
                move |this, _, cx| {
                    if border_axis == Some(true) {
                        this.delete_table_border_selection(cx);
                    } else {
                        this.table_delete_columns(table, &columns, cx);
                    }
                },
            ));
        }
        if layout.rows || layout.columns {
            items.push(self.render_context_menu_separator());
        }
        items.push(self.render_context_menu_item_with_icon(
            self.t(if is_front_matter {
                TextKey::DeleteFrontMatter
            } else {
                TextKey::DeleteTable
            }),
            TABLE_GRID_SVG,
            false,
            cx,
            move |this, _, cx| this.table_delete_table(table, cx),
        ));
        items
    }

    fn render_table_cells_context_menu(
        &self,
        table: usize,
        row: usize,
        col: usize,
        border_axis: Option<bool>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let mut items = Vec::new();
        items.push(self.render_context_menu_item_with_icon(
            self.t(crate::i18n::TextKey::Copy),
            MENU_COPY_SVG_PATH,
            false,
            cx,
            move |this, _, cx| {
                if border_axis.is_none()
                    && this.table_cell_selection.is_none()
                    && this.cursor.has_selection()
                {
                    this.copy_selection_to_clipboard(cx);
                    return;
                }
                let text = if border_axis.is_some() {
                    this.copy_table_border_selection()
                } else {
                    this.ensure_table_cell_context_selection(table, row, col);
                    this.copy_table_cell_selection()
                };
                if let Some(text) = text {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
                }
            },
        ));
        items.push(self.render_context_menu_item_with_icon(
            self.t(crate::i18n::TextKey::Cut),
            MENU_CLIP_SVG_PATH,
            false,
            cx,
            move |this, _, cx| {
                if border_axis.is_none()
                    && this.table_cell_selection.is_none()
                    && this.cursor.has_selection()
                {
                    this.cut_selection_to_clipboard(cx);
                    return;
                }
                if border_axis.is_some() {
                    this.cut_table_border_selection(cx);
                } else {
                    this.ensure_table_cell_context_selection(table, row, col);
                    this.cut_table_cell_selection(cx);
                }
            },
        ));
        if TableMenuLayout::new(self.block_is_front_matter(table)).paste_insert {
            items.push(self.render_context_menu_item_with_icon(
                crate::i18n::table_paste_label(self.ui_language, false),
                MENU_PASTE_SVG_PATH,
                false,
                cx,
                move |this, _, cx| {
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        if border_axis.is_some() {
                            this.paste_table_border_selection(&text, cx);
                        } else {
                            this.ensure_table_cell_context_selection(table, row, col);
                            this.paste_table_cell_selection(&text, false, cx);
                        }
                    }
                },
            ));
        }
        items.push(self.render_context_menu_item_with_icon(
            crate::i18n::table_paste_label(self.ui_language, true),
            MENU_PASTE_SVG_PATH,
            false,
            cx,
            move |this, _, cx| {
                if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                    if border_axis.is_some() {
                        this.paste_table_border_selection_overwrite(&text, cx);
                    } else {
                        this.ensure_table_cell_context_selection(table, row, col);
                        this.paste_table_cell_selection(&text, true, cx);
                    }
                }
            },
        ));
        items.push(self.render_context_menu_separator());
        items.push(self.render_context_menu_item_with_icon(
            crate::i18n::table_clear_cells_label(self.ui_language),
            TABLE_DELETE_SVG,
            true,
            cx,
            move |this, _, cx| {
                if border_axis.is_some() {
                    this.clear_table_border_selection(cx);
                } else {
                    this.ensure_table_cell_context_selection(table, row, col);
                    this.clear_table_cell_selection(cx);
                }
            },
        ));
        items
    }

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
        self.table_adjustment_menu = TableAdjustmentMenu::default();
        self.context_menu = Some(menu);
        self.context_submenu = None;
        self.context_submenu_aim = None;
        self.context_submenu_hovered = false;
        self.format_context_menu_bounds.set(None);
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
        if !self.animations_enabled() {
            self.context_menu_opacity_epoch = self.context_menu_opacity_epoch.wrapping_add(1);
            self.context_menu_opacity_task = None;
            self.context_menu_opacity = target.clamp(0.0, 1.0);
            self.context_menu_opacity_target = self.context_menu_opacity;
            if close_when_done {
                self.finish_close_context_menu(cx);
            }
            cx.notify();
            return;
        }
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
        self.table_adjustment_menu = TableAdjustmentMenu::default();
        let had_menu = self.context_menu.take().is_some();
        let had_submenu = self.context_submenu.take().is_some();
        let had_submenu_aim = self.context_submenu_aim.take().is_some();
        let had_submenu_hovered = std::mem::replace(&mut self.context_submenu_hovered, false);
        let had_pane_target = self.context_menu_pane_target.take().is_some();
        self.table_size_hover = None;
        if had_menu || had_submenu || had_submenu_aim || had_submenu_hovered || had_pane_target {
            // Dismissing a suggestion never edits the document. Only executing a
            // slash action consumes its query.
            self.slash_menu_start = None;
            self.slash_menu_anchor = None;
            self.slash_filter.clear();
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

    pub(super) fn copy_text_to_clipboard(&mut self, text: String, cx: &mut Context<Self>) {
        self.context_menu = None;
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
        cx.notify();
    }

    pub(super) fn copy_active_image_to_clipboard(&mut self, cx: &mut Context<Self>) {
        let Some(path) = self.current_file_path.clone() else {
            self.context_menu = None;
            cx.notify();
            return;
        };
        self.copy_image_path_to_clipboard(&path, cx);
    }

    fn copy_image_path_to_clipboard(&mut self, path: &Path, cx: &mut Context<Self>) {
        self.copy_local_image_source_to_clipboard(path.to_string_lossy().into_owned(), cx);
    }

    fn copy_local_image_source_to_clipboard(&mut self, source: String, cx: &mut Context<Self>) {
        let path = PathBuf::from(&source);
        let file_dir = self
            .current_file_path
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf);
        let mut roots = self.workspaces.clone();
        if !self.sidebar_root.as_os_str().is_empty() && !roots.contains(&self.sidebar_root) {
            roots.push(self.sidebar_root.clone());
        }
        self.context_menu = None;
        cx.spawn(async move |this, cx| {
            let read_path = path.clone();
            let image =
                crate::platform::fs_task::run("copy image to clipboard", path.clone(), move || {
                    let path = helpers::resolve_image_to_path(&source, file_dir.as_ref(), &roots)
                        .ok_or_else(|| {
                        std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            crate::i18n::runtime::rt!(FileMissing, read_path.display()),
                        )
                    })?;
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
                    let bytes = std::fs::read(&path)?;
                    let image = if let Some(format) = format {
                        Image::from_bytes(format, bytes)
                    } else {
                        let decoded = crate::platform::image_decode::decode(&bytes, ext.as_deref())
                            .map_err(std::io::Error::other)?;
                        let mut png = Cursor::new(Vec::new());
                        decoded
                            .write_to(&mut png, image::ImageFormat::Png)
                            .map_err(std::io::Error::other)?;
                        Image::from_bytes(ImageFormat::Png, png.into_inner())
                    };
                    Ok(image)
                })
                .await;
            this.update(cx, |editor, cx| {
                match image {
                    Ok(image) => cx.write_to_clipboard(ClipboardItem::new_image(&image)),
                    Err(error) => {
                        editor.copy_text_to_clipboard(path.to_string_lossy().into_owned(), cx);
                        editor.notify_diagnostic(error, cx);
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn current_markdown_image_path(&self) -> Option<PathBuf> {
        let raw_path = self.current_markdown_image_source()?;
        if helpers::is_remote_image_url(raw_path) {
            return None;
        }
        lexical_menu_image_path(
            raw_path,
            self.current_file_path.as_deref().and_then(Path::parent),
            self.workspaces.first().map(PathBuf::as_path).or_else(|| {
                (!self.sidebar_root.as_os_str().is_empty()).then_some(self.sidebar_root.as_path())
            }),
        )
    }

    fn current_markdown_image_source(&self) -> Option<&str> {
        let target = self.current_image_menu_target()?;
        self.decorations.get(target.line)?.image_path.as_deref()
    }

    fn copy_markdown_image_to_clipboard(&mut self, source: String, cx: &mut Context<Self>) {
        if !helpers::is_remote_image_url(&source) {
            self.copy_local_image_source_to_clipboard(source, cx);
            return;
        }

        self.context_menu = None;
        cx.spawn(async move |this, cx| {
            let source_for_download = source.clone();
            let image = cx
                .background_executor()
                .spawn(async move {
                    let response = ureq::AgentBuilder::new()
                        .timeout(crate::platform::fs_task::DEFAULT_TIMEOUT)
                        .build()
                        .get(&source_for_download)
                        .call()?;
                    let mut bytes = Vec::new();
                    std::io::Read::read_to_end(&mut response.into_reader(), &mut bytes)?;
                    let path = source_for_download
                        .split(['?', '#'])
                        .next()
                        .unwrap_or(source_for_download.as_str());
                    if path.to_ascii_lowercase().ends_with(".svg") {
                        return Ok::<_, anyhow::Error>(Image::from_bytes(ImageFormat::Svg, bytes));
                    }
                    let decoded = crate::platform::image_decode::decode(
                        &bytes,
                        std::path::Path::new(path)
                            .extension()
                            .and_then(|extension| extension.to_str()),
                    )?;
                    let mut png = Cursor::new(Vec::new());
                    decoded.write_to(&mut png, image::ImageFormat::Png)?;
                    Ok(Image::from_bytes(ImageFormat::Png, png.into_inner()))
                })
                .await;

            let _ = this.update(cx, |editor, cx| {
                match image {
                    Ok(image) => cx.write_to_clipboard(ClipboardItem::new_image(&image)),
                    Err(error) => {
                        editor.notify_error(
                            format!(
                                "{} {source}: {error:#}",
                                editor.message(crate::i18n::MessageKey::CopyImageFailed)
                            ),
                            cx,
                        );
                    }
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

    pub(super) fn path_copy_text(&self, path: &Path, relative: bool) -> Option<String> {
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

    fn save_current_markdown_image_as(&mut self, cx: &mut Context<Self>) {
        let Some(source) = self.current_markdown_image_source().map(str::to_string) else {
            return;
        };
        let file = self.current_file_path.clone();
        let root = self.sidebar_root.clone();
        let workspaces = self.workspaces.clone();
        let default_name = source
            .split(['?', '#'])
            .next()
            .and_then(|path| Path::new(path).file_name())
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("image")
            .to_string();
        #[cfg(target_os = "macos")]
        let directory = file
            .as_deref()
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .or_else(crate::platform::paths::home_dir)
            .unwrap_or_default();
        #[cfg(not(target_os = "macos"))]
        let title = self.t(crate::i18n::TextKey::SaveFileDialog).to_string();
        self.close_context_menu(cx);
        cx.spawn(async move |this, cx| {
            #[cfg(target_os = "macos")]
            let destination = match this.update(cx, |_, cx| {
                cx.prompt_for_new_path(&directory, Some(&default_name))
            }) {
                Ok(receiver) => receiver.await.ok().and_then(Result::ok).flatten(),
                Err(_) => None,
            };
            #[cfg(not(target_os = "macos"))]
            let destination = cx
                .background_executor()
                .spawn(async move { helpers::save_file_blocking(&default_name, &title) })
                .await;
            let Some(destination) = destination else {
                return;
            };
            let path = destination.clone();
            let result =
                crate::platform::fs_task::run_anyhow("save image as", destination, move || {
                    let bytes = navigation_io::read_document_image_bytes(
                        &source,
                        file.as_deref(),
                        &root,
                        &workspaces,
                    )?;
                    file_ops::atomic_write(&path, &bytes)?;
                    Ok(())
                })
                .await;
            if let Err(error) = result {
                this.update(cx, |editor, cx| {
                    editor.notify_error(
                        format!(
                            "{}: {error}",
                            editor.t(crate::i18n::TextKey::FileOperationFailed)
                        ),
                        cx,
                    );
                })
                .ok();
            }
        })
        .detach();
    }

    fn pick_image_for_current_markdown_image(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.current_image_menu_target() else {
            return;
        };
        let Some(document_path) = self.current_file_path.clone() else {
            self.pick_image_for_line(target.line, cx);
            return;
        };
        let document = navigation_io::DocumentContext::capture(self);
        let original = self.buffer.line_content(target.line).to_string();
        let language = self.ui_language;

        cx.spawn(async move |this, cx| {
            #[cfg(target_os = "macos")]
            let maybe_path = match this.update(cx, |_, cx| {
                cx.prompt_for_paths(gpui::PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: false,
                    prompt: Some(crate::i18n::MessageKey::ChooseImage.text(language).into()),
                })
            }) {
                Ok(receiver) => receiver
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .flatten()
                    .and_then(|mut paths| paths.pop())
                    .filter(|path| super::is_image_path(path)),
                Err(_) => None,
            };
            #[cfg(not(target_os = "macos"))]
            let maybe_path = cx
                .background_executor()
                .spawn(async move { helpers::pick_image_file_blocking(language) })
                .await;
            if let Some(path) = maybe_path {
                let Some(file_name) = path.file_name() else {
                    return;
                };
                let relative_path = file_name.to_string_lossy().into_owned();
                let Some(document_dir) = document_path.parent() else {
                    return;
                };
                let destination_path = document_dir.join(file_name);
                if path != destination_path {
                    let result = crate::platform::fs_task::run(
                        "copy selected image",
                        destination_path.clone(),
                        move || file_ops::copy_file_data(&path, &destination_path, true),
                    )
                    .await;
                    if let Err(error) = result {
                        this.update(cx, |editor, cx| {
                            let message = format!(
                                "{}: {error}",
                                editor.t(crate::i18n::TextKey::FileOperationFailed)
                            );
                            editor.notify_error(message, cx);
                        })
                        .ok();
                        return;
                    }
                }
                this.update(cx, |editor, cx| {
                    editor.with_pane_if_alive(document.pane_id(), |editor| {
                        if document.is_current(editor)
                            && editor.buffer.line_content(target.line) == original
                        {
                            editor.replace_image_url_on_line_with_text(
                                target.line,
                                &relative_path,
                                cx,
                            );
                        }
                    });
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

    fn selected_wiki_link_path(&self) -> Option<PathBuf> {
        if self.wiki_link_query.trim().is_empty() {
            self.wiki_link_recent_results
                .get(self.menu_selected_index)
                .map(|candidate| candidate.path.clone())
        } else {
            self.wiki_link_search_results
                .get(self.menu_selected_index)
                .map(|result| result.path.clone())
        }
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
        if let Some(Some(info)) = self.line_display_info.get(line) {
            let disp_col = helpers::buffer_to_display_offset(&info.buffer_to_display, col);
            let affinity = self
                .wrap_boundary_affinity
                .and_then(|current| (current.offset == offset).then_some(current.kind));
            let pos = Self::visual_position_for_wrapped_display(
                &info.wrapped_line,
                &info.display_text,
                disp_col,
                info.line_height,
                info.indent_px,
                &info.wrap_boundary_upstream_xs,
                affinity,
            )
            .unwrap_or_default();
            // Anchor to the cursor's visual row, including wrap affinity and block padding.
            return wrapped_menu_anchor(
                &info.wrapped_line,
                pos,
                info.line_height,
                info.content_x,
                text_top,
            );
        }
        gpui::point(
            px(self.last_content_left + col as f32 * self.base_font_size() * 0.6),
            px(text_top + self.visual_height_for_line(line)),
        )
    }

    pub(crate) fn cursor_screen_position(&self) -> Point<Pixels> {
        if self.table_cursor_cell().is_some()
            && let Some(geometry) = self.table_cursor_paint_bounds
            && geometry.offset == self.cursor.offset
        {
            return gpui::point(geometry.paint_bounds.left(), geometry.paint_bounds.bottom());
        }
        self.screen_position_for_offset(self.cursor.offset)
    }

    pub(crate) fn sync_table_input_menu_after_layout(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.layout_dirty
            || self.current_pane_id() != self.focused_pane_id
            || self.table_cursor_cell().is_none()
            || !self
                .table_cursor_paint_bounds
                .is_some_and(|geometry| geometry.offset == self.cursor.offset)
            || !self
                .context_menu
                .as_ref()
                .is_some_and(|menu| matches!(menu.kind, ContextMenuKind::WikiLink))
        {
            return;
        }
        let Some(start) = self.wiki_link_start else {
            return;
        };
        let Some(start_bounds) = self.table_row_bounds_for_offset(
            window,
            start,
            Some(WrapBoundaryAffinityKind::Downstream),
        ) else {
            return;
        };
        let Some(cursor_bounds) = self.table_row_bounds_for_offset(
            window,
            self.cursor.offset,
            self.wrap_boundary_affinity_for_offset(self.cursor.offset),
        ) else {
            return;
        };
        let position = gpui::point(start_bounds.left(), cursor_bounds.bottom());
        if let Some(menu) = self.context_menu.as_mut()
            && menu.position != position
        {
            menu.position = position;
            cx.notify();
        }
    }

    fn wiki_link_menu_position(&self, start: usize) -> Point<Pixels> {
        let start_position = self.screen_position_for_offset(start);
        let cursor_position = self.cursor_screen_position();
        gpui::point(start_position.x, cursor_position.y)
    }

    fn slash_menu_anchor_at(&self, offset: usize) -> SlashMenuAnchor {
        let row_height = self
            .visual_row_vertical_bounds_for_offset(offset)
            .map(|(top, bottom)| bottom - top)
            .unwrap_or_else(|| self.line_height_for_block(&BlockType::Normal));
        SlashMenuAnchor {
            position: self.screen_position_for_offset(offset),
            row_height,
            scroll_offset: self.scroll_offset,
        }
    }

    fn measured_slash_menu_anchor(&self) -> Option<SlashMenuAnchor> {
        Some(self.slash_menu_anchor_at(self.slash_menu_start?))
    }

    pub(crate) fn capture_slash_menu_anchor_before_edit(&mut self) {
        let (line, column) = self.buffer.offset_to_line_col(self.cursor.offset);
        let content = self.buffer.line_content(line);
        if self.slash_menu_start.is_none()
            && self.active_tab_is_markdown()
            && !self.embedded_editor
            && column == content.len()
            && content.trim().is_empty()
            && !self.offset_in_code_context(self.cursor.offset)
        {
            self.slash_menu_anchor = Some(self.slash_menu_anchor_at(self.cursor.offset));
        }
    }

    /// Use the last measured row while an edit invalidates layout. Computing
    /// its position from temporary line-height estimates here moves the menu
    /// for one frame, before editor prepaint puts the row back in place.
    pub(super) fn slash_menu_position(&self) -> Option<Point<Pixels>> {
        let start = self.slash_menu_start?;
        Some(slash_menu_position_for_layout(
            self.layout_dirty,
            self.slash_menu_anchor,
            self.scroll_offset,
            || self.slash_menu_anchor_at(start).position,
        ))
    }

    pub(crate) fn sync_slash_menu_anchor_after_layout(&mut self, cx: &mut Context<Self>) {
        if self.current_pane_id() != self.focused_pane_id {
            return;
        }
        if let Some(anchor) = self.measured_slash_menu_anchor() {
            self.slash_menu_anchor = Some(anchor);
            if let Some(menu) = self.context_menu.as_mut()
                && matches!(menu.kind, ContextMenuKind::Format)
                && menu.position != anchor.position
            {
                menu.position = anchor.position;
                cx.notify();
            }
        }
    }

    pub(crate) fn open_slash_menu(&mut self, cx: &mut Context<Self>) {
        self.slash_menu_start = Some(self.cursor.offset - 1);
        if self.slash_menu_anchor.is_none() {
            self.slash_menu_anchor = self.measured_slash_menu_anchor();
        }
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
        let Some(start) = self.slash_menu_start else {
            return;
        };
        let query = input_ime::slash_menu_query(
            self.buffer.text(),
            start,
            self.cursor.offset,
            self.active_tab_is_markdown() && !self.embedded_editor,
            self.offset_in_code_context(self.cursor.offset),
        );
        let Some(query) = query else {
            self.dismiss_slash_menu(cx);
            return;
        };
        self.slash_filter = query.to_string();
        self.menu_selected_index = 0;
        if self.slash_menu_matched_actions().is_empty() {
            // Keep the query alive so Backspace can reveal suggestions again.
            self.hide_slash_menu(cx);
            return;
        }
        let pos = self
            .slash_menu_position()
            .unwrap_or_else(|| self.cursor_screen_position());
        if let Some(menu) = &mut self.context_menu {
            menu.position = pos;
        } else {
            self.open_format_context_menu(pos, cx);
        }
        cx.notify();
    }

    pub(crate) fn dismiss_slash_menu(&mut self, cx: &mut Context<Self>) {
        self.slash_menu_start = None;
        self.slash_menu_anchor = None;
        self.slash_filter.clear();
        self.close_context_menu_immediately(cx);
        cx.notify();
    }

    /// Hide an empty result list without ending the slash query or touching text.
    fn hide_slash_menu(&mut self, cx: &mut Context<Self>) {
        self.context_menu_opacity_epoch += 1;
        self.context_menu_opacity_task = None;
        self.context_menu_opacity = 0.0;
        self.context_menu_opacity_target = 0.0;
        self.context_menu = None;
        self.context_submenu = None;
        self.format_context_menu_bounds.set(None);
        cx.notify();
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
        if root.as_os_str().is_empty() {
            self.wiki_link_recent_results.clear();
            cx.notify();
            return;
        }

        let requested_root = root.clone();
        let task = cx.spawn(async move |this, cx| {
            let results = crate::platform::fs_task::run(
                "search recent wiki links",
                root.clone(),
                move || {
                    std::fs::read_dir(&root)?;
                    Ok(crate::editor::quick_open::collect_wiki_link_recent_files(
                        &root, 10,
                    ))
                },
            )
            .await;

            this.update(cx, |this, cx| {
                if this.sidebar_root != requested_root || !this.wiki_link_query.trim().is_empty() {
                    return;
                }
                match results {
                    Ok(results) => this.wiki_link_recent_results = results,
                    Err(error) => {
                        this.wiki_link_recent_results.clear();
                        this.notify_diagnostic(error, cx);
                    }
                }
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
        self.wiki_link_recent_task = None;

        let root = self.sidebar_root.clone();
        let query = self.wiki_link_query.clone();
        if root.as_os_str().is_empty() || query.trim().is_empty() {
            self.wiki_link_search_results.clear();
            cx.notify();
            return;
        }

        let requested_root = root.clone();
        let requested_query = query.clone();
        let task = cx.spawn(async move |this, cx| {
            let results =
                crate::platform::fs_task::run("search wiki links", root.clone(), move || {
                    std::fs::read_dir(&root)?;
                    Ok(crate::editor::quick_open::collect_wiki_link_search_results(
                        &root, &query, 10,
                    ))
                })
                .await;

            this.update(cx, |this, cx| {
                if this.sidebar_root != requested_root || this.wiki_link_query != requested_query {
                    return;
                }
                match results {
                    Ok(results) => this.wiki_link_search_results = results,
                    Err(error) => {
                        this.wiki_link_search_results.clear();
                        this.notify_diagnostic(error, cx);
                    }
                }
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
        let Some(path) = self.selected_wiki_link_path() else {
            return;
        };
        let pane_id = self.current_pane_id();
        let tab_id = self.active_tab_id.clone();
        let query = self.buffer.text_for_range(start + 2..query_end).to_string();
        let root = self.sidebar_root.clone();
        let source = self
            .current_file_path
            .clone()
            .unwrap_or_else(|| root.join(".untitled.md"));
        let mut policy = self.app_settings.workspace_link_policy();
        policy.link_syntax = super::attachments::AttachmentLinkSyntax::WikiLink;
        cx.spawn(async move |this, cx| {
            let target: anyhow::Result<String> = crate::platform::fs_task::run_anyhow(
                "resolve wiki link target",
                root.clone(),
                move || {
                    std::fs::read_dir(&root)?;
                    let index =
                        if policy.path_style == super::attachments::AttachmentPathStyle::Shortest {
                            Some(super::attachments::WorkspaceLinkIndex::build(&root))
                        } else {
                            None
                        };
                    policy.target_for(&root, &source, &path, index.as_ref())
                },
            )
            .await;
            this.update(cx, |editor, cx| {
                editor.with_pane_if_alive(pane_id, |editor| {
                    if editor.active_tab_id != tab_id
                        || editor.wiki_link_start != Some(start)
                        || editor.wiki_link_query_end(start) != Some(query_end)
                        || editor.buffer.text_for_range(start + 2..query_end) != query
                    {
                        return;
                    }
                    let target = match target {
                        Ok(target) => target,
                        Err(error) => {
                            editor.notify_diagnostic(error, cx);
                            return;
                        }
                    };
                    let replace_start = start + 2;
                    editor.buffer.replace(replace_start..query_end, &target);
                    editor
                        .cursor
                        .move_to(replace_start + target.len(), &editor.buffer);
                    editor.reparse();
                    editor.close_context_menu(cx);
                    editor.scroll_to_cursor();
                    editor.pause_blinking(cx);
                });
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn execute_format_menu_action(
        &mut self,
        action: FormatMenuAction,
        cx: &mut Context<Self>,
    ) {
        if self.line_diff_selection.is_some() {
            if action == FormatMenuAction::Copy {
                self.copy_line_diff_selection_to_clipboard(cx);
            }
            self.close_context_menu(cx);
            return;
        }
        if let Some(line) = self.line_format_toolbar_line
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
            let cell_list_action = matches!(
                action,
                FormatMenuAction::Todo
                    | FormatMenuAction::UnorderedList
                    | FormatMenuAction::OrderedList
            ) && self.table_cursor_cell_range().is_some();
            if !cell_list_action {
                if block_action {
                    self.cursor.move_to(line_end, &self.buffer);
                } else {
                    self.cursor.anchor = line_start;
                    self.cursor.offset = line_end;
                }
            }
            self.reset_cursor_visual_targeting();
        }
        self.execute_format_menu_action_inner(action, cx);
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
        let action = if action == FormatMenuAction::Link
            && self.app_settings.link_syntax == "wikilink"
            && self.link_syntax_at_cursor().is_none()
            && !["https://", "http://", "mailto:"].iter().any(|prefix| {
                self.buffer
                    .text_for_range(self.cursor.selection_range())
                    .trim()
                    .starts_with(prefix)
            }) {
            FormatMenuAction::WikiLink
        } else {
            action
        };
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
                    let selected = self
                        .buffer
                        .text_for_range(self.cursor.selection_range())
                        .to_string();
                    if ["https://", "http://", "mailto:"]
                        .iter()
                        .any(|prefix| selected.trim().starts_with(prefix))
                    {
                        let range = self.cursor.selection_range();
                        let replacement = format!("[{selected}]({selected})");
                        self.buffer.replace(range.clone(), &replacement);
                        self.cursor
                            .move_to(range.start + replacement.len(), &self.buffer);
                        self.reparse();
                        self.scroll_to_cursor();
                    } else {
                        self.insert_link_or_image("[", "](https://)", "title");
                    }
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
                if self.app_settings.link_syntax == "wikilink"
                    && let Some((full, keep)) = self.wiki_link_syntax_at_cursor()
                    && full.start > 0
                    && self.buffer.text().as_bytes()[full.start - 1] == b'!'
                {
                    self.unwrap_syntax_range(full.start - 1..full.end, keep);
                } else if let Some(target) = self.current_image_menu_target() {
                    let line_start = self.buffer.line_start(target.line);
                    self.unwrap_syntax_range(
                        line_start + target.full_range.start..line_start + target.full_range.end,
                        line_start + target.desc_range.start..line_start + target.desc_range.end,
                    );
                } else {
                    if self.app_settings.link_syntax == "wikilink" {
                        self.insert_wiki_image();
                    } else {
                        self.insert_link_or_image("![", "](image.png)", "alt");
                    }
                }
                self.pause_blinking(cx);
            }
            FormatMenuAction::Table => {
                self.checkpoint();
                let range = self.cursor.selection_range();
                let column = self.message(crate::i18n::MessageKey::TableColumn);
                let cell = self.message(crate::i18n::MessageKey::TableCell);
                let snippet = format!(
                    "| {column}1 | {column}2 |\n| --- | --- |\n| {cell}1 | {cell}2 |\n| {cell}3 | {cell}4 |"
                );
                self.buffer.replace(range.clone(), &snippet);
                let select_start = range.start + 2;
                let select_end = select_start + column.len() + 1;
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
            .filter(|item| slash_menu_item_matches(item, &filter))
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

    fn render_chat_attachment_item(
        &self,
        path: Option<PathBuf>,
        selection: Option<std::ops::Range<usize>>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let label = if selection.is_some() {
            crate::i18n::add_selection_to_chat_label(self.ui_language)
        } else {
            crate::i18n::add_file_to_chat_label(self.ui_language)
        };
        let icon = if selection.is_some() {
            CHAT_SVG_PATH
        } else {
            MENU_AT_SVG_PATH
        };
        if let Some(path) = path {
            let source_tab = self.active_tab_id.clone();
            return self.render_format_context_menu_item_with_hover_behavior(
                label,
                Some(icon),
                None,
                false,
                false,
                FormatMenuHoverBehavior::ClearSubmenu,
                cx,
                move |this, _, cx| {
                    this.close_context_menu(cx);
                    if let Some(range) = &selection {
                        if this.active_tab_id != source_tab {
                            this.notify_error(
                                this.message(crate::i18n::MessageKey::ChatSourceChanged),
                                cx,
                            );
                            return;
                        }
                        this.add_selection_to_chat(&path, range.clone(), cx);
                    } else {
                        this.add_path_to_chat(&path, cx);
                    }
                },
            );
        }
        let row = crate::ui::list::list_item(
            self.styles.menu_item(false),
            crate::style::ControlState {
                disabled: true,
                ..Default::default()
            },
            self.ui_scale,
        )
        .id(SharedString::from(format!("chat-save-required-{label}")))
        .child(crate::ui::decoration::icon(
            crate::style::IconStyle::new(
                self.scaled(self.styles.context_menu.menu_format_icon_size_px),
                self.styles.context_menu.menu_format_icon_color,
            ),
            icon,
        ))
        .child(label);
        super::tooltip::with_tooltip(
            row,
            cx.entity(),
            crate::i18n::chat_save_required_label(self.ui_language),
            None::<SharedString>,
        )
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
        let icon_styles = &self.styles.context_menu;
        let item_id = SharedString::from(format!("menu-item-{}", label));
        let (icon_col_w, _gap) = menu_icon_column_metrics(
            ms.menu_format_icon_size_px,
            icon_styles.menu_item_check_size_px,
            icon_styles.menu_item_action_gap_px,
        );
        let icon_col_w = self.scaled(icon_col_w);
        crate::ui::list::list_item(
            self.styles.menu_item(is_destructive),
            Default::default(),
            self.ui_scale,
        )
        .id(item_id)
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
        let multiline_label = label.contains('\n');
        let ms = &self.styles.context_menu;
        let icon_styles = &self.styles.context_menu;
        let item_id = SharedString::from(format!("menu-item-{}", label));
        let (icon_col_w, _gap) = menu_icon_column_metrics(
            ms.menu_format_icon_size_px,
            icon_styles.menu_item_check_size_px,
            icon_styles.menu_item_action_gap_px,
        );
        let icon_size = self.scaled(ms.menu_format_icon_size_px);
        let icon_col_w = self.scaled(icon_col_w);
        let mut item_style = self.styles.menu_item(is_destructive);
        if multiline_label {
            item_style.control.height_px *= 1.5;
        }
        crate::ui::list::list_item(item_style, Default::default(), self.ui_scale)
            .when(multiline_label, |item| {
                item.h_auto()
                    .min_h(px(self.scaled(item_style.control.height_px)))
            })
            .id(item_id)
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
                            .text_color(icon_styles.menu_item_action_icon_color),
                    ),
            )
            .child(if multiline_label {
                div()
                    .min_w(px(0.0))
                    .flex_1()
                    .whitespace_normal()
                    .line_height(px(self.scaled(item_style.control.font_size_px * 1.2)))
                    .child(label.to_string())
                    .into_any_element()
            } else {
                label.to_string().into_any_element()
            })
            .when(dismiss_export_submenu, |this| {
                this.on_mouse_move(cx.listener(|this, _, _, cx| {
                    this.dismiss_export_submenu(cx);
                    if this.table_adjustment_menu.popup_table.take().is_some() {
                        this.table_size_hover = None;
                        cx.notify();
                    }
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
        let icon_styles = &self.styles.context_menu;
        let icon_size = self.scaled(icon_styles.menu_item_check_size_px);
        crate::ui::list::list_item(
            self.styles.menu_item(false),
            Default::default(),
            self.ui_scale,
        )
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
                            .text_color(icon_styles.menu_item_check_color),
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

    fn render_front_matter_type_choice_item(
        &self,
        target: front_matter::FrontMatterType,
        is_active: bool,
        disabled_reason: Option<&'static str>,
        cx: &mut Context<Self>,
        handler: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let ms = &self.styles.context_menu;
        let icon_styles = &self.styles.context_menu;
        let enabled = disabled_reason.is_none();
        let icon_size = self.scaled(ms.menu_format_icon_size_px);
        let check_size = self.scaled(icon_styles.menu_item_check_size_px);
        let (icon_col_w, gap) = menu_icon_column_metrics(
            ms.menu_format_icon_size_px,
            icon_styles.menu_item_check_size_px,
            icon_styles.menu_item_action_gap_px,
        );
        let mut row_style = ms.item();
        row_style.control.gap_px = gap;
        row_style.control.disabled_opacity = 1.0;
        let item = crate::ui::list::list_item(
            row_style,
            crate::style::ControlState {
                disabled: !enabled,
                ..Default::default()
            },
            self.ui_scale,
        )
        .id(SharedString::from(format!("front-matter-type-{target:?}")))
        .child(
            div()
                .w(px(self.scaled(icon_col_w)))
                .h(px(icon_size))
                .flex()
                .flex_none()
                .items_center()
                .justify_center()
                .opacity(if enabled {
                    1.0
                } else {
                    self.styles.ui.spec.application.menu_disabled_label_opacity
                })
                .child(
                    svg()
                        .path(front_matter_type_icon(target))
                        .size(px(icon_size))
                        .text_color(icon_styles.menu_item_action_icon_color),
                ),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .opacity(if enabled {
                    1.0
                } else {
                    self.styles.ui.spec.application.menu_disabled_label_opacity
                })
                .child(crate::i18n::front_matter_type_label(
                    self.ui_language,
                    front_matter_type_label_key(target),
                )),
        )
        .child(
            div()
                .size(px(check_size))
                .flex_none()
                .when(is_active, |this| {
                    this.child(
                        svg()
                            .path(CHECK_SVG_PATH)
                            .size(px(check_size))
                            .text_color(icon_styles.menu_item_check_color),
                    )
                }),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
                this.suppress_editor_mouse_up_for_current_menu_event(cx);
                if enabled {
                    this.close_context_menu(cx);
                    handler(this, cx);
                }
            }),
        );
        if let Some(reason) = disabled_reason {
            let editor = cx.entity().clone();
            item.tooltip(move |_, cx| {
                cx.new(|_| FrontMatterTypeMenuTooltip {
                    editor: editor.clone(),
                    text: reason.into(),
                })
                .into()
            })
            .into_any_element()
        } else {
            item.into_any_element()
        }
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
        crate::ui::list::list_item(
            self.styles.format_menu_item(is_active),
            crate::style::ControlState {
                selected: is_selected,
                ..Default::default()
            },
            self.ui_scale,
        )
        .justify_between()
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
                            this.child(svg().path(path).size(px(icon_size)).text_color(icon_color))
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
                this.on_mouse_move(
                    cx.listener(move |this, event: &MouseMoveEvent, _window, cx| {
                        match hover_behavior {
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
                        }
                    }),
                )
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
        let icon_styles = &self.styles.context_menu;
        let is_open = self.context_submenu.as_ref() == Some(&submenu_kind);
        let icon_size = self.scaled(ms.menu_format_icon_size_px);
        // Export lives in the sidebar-style menus, the rest in the format menu;
        // each has to match its host's icon column or the label drifts.
        let (icon_col_w, icon_gap) = if matches!(submenu_kind, ContextMenuKind::Export { .. }) {
            let (column, gap) = menu_icon_column_metrics(
                ms.menu_format_icon_size_px,
                icon_styles.menu_item_check_size_px,
                icon_styles.menu_item_action_gap_px,
            );
            (self.scaled(column), self.scaled(gap))
        } else {
            (icon_size, self.scaled(ms.menu_format_icon_gap_px))
        };
        let arrow_size = (icon_size - self.scaled(2.0)).max(self.scaled(10.0));
        let icon_shared = icon_path.map(SharedString::from);
        let trigger_bounds = match submenu_kind {
            ContextMenuKind::FormatHeading => self.format_heading_trigger_bounds.clone(),
            ContextMenuKind::FormatMore => self.format_more_trigger_bounds.clone(),
            ContextMenuKind::Export { .. } => self.export_trigger_bounds.clone(),
            _ => Rc::new(Cell::new(None)),
        };

        let row = crate::ui::list::list_item(
            self.styles.menu_item(false),
            crate::style::ControlState {
                open: is_open,
                ..Default::default()
            },
            self.ui_scale,
        )
        .justify_between()
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
                    this.table_adjustment_menu = TableAdjustmentMenu::default();
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
                this.table_adjustment_menu = TableAdjustmentMenu::default();
                this.context_submenu = Some(submenu_kind.clone());
                cx.notify();
            })
        })
        .into_any_element();

        let trigger = div()
            .w_full()
            .h(px(self.scaled(ms.menu_item_height_px)))
            .on_children_prepainted(move |bounds: Vec<Bounds<Pixels>>, _, _| {
                trigger_bounds.set(bounds.first().copied());
            })
            .child(row)
            .into_any_element();
        if matches!(submenu_kind, ContextMenuKind::Table { .. }) {
            SelectionToolbarHoverRegion {
                child: trigger,
                editor: cx.entity().clone(),
                region: "table-parent-trigger",
                menu_kind: None,
            }
            .into_any_element()
        } else {
            trigger
        }
    }

    fn render_context_menu_separator(&self) -> AnyElement {
        crate::ui::surface::separator(self.styles.context_menu.separator(), 1.0).into_any_element()
    }

    fn render_sidebar_context_menu(
        &self,
        path: PathBuf,
        is_dir: bool,
        workspace_root: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let has_clipboard = self.file_clipboard.is_some();
        let is_root = workspace_root || is_dir && path == self.sidebar_root;
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
                        this.switch_workspace(pd.clone(), cx);
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
                        this.switch_workspace(pd.clone(), cx);
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
                        this.switch_workspace(pd.clone(), cx);
                        this.paste_into_dir(pd.clone(), cx);
                    },
                ));
                items.push(self.render_context_menu_separator());
            }
            if path == self.sidebar_root && !self.expanded_dirs.is_empty() {
                items.push(self.render_context_menu_item(
                    self.t(crate::i18n::TextKey::CollapseAllDirectories),
                    false,
                    cx,
                    move |this, _, cx| {
                        this.collapse_all_dirs(cx);
                    },
                ));
            }
            let expand_root = path.clone();
            items.push(self.render_context_menu_item(
                self.t(crate::i18n::TextKey::ExpandAllDirectories),
                false,
                cx,
                move |this, _, cx| {
                    this.switch_workspace(expand_root.clone(), cx);
                    this.expand_all_dirs(cx);
                },
            ));
            items.push(self.render_context_menu_separator());
            let search_root = path.clone();
            items.push(self.render_context_menu_item(
                crate::i18n::sidebar_action_label(self.ui_language, "search"),
                false,
                cx,
                move |this, window, cx| {
                    this.switch_workspace(search_root.clone(), cx);
                    if !this.workspace_search_visible {
                        this.toggle_workspace_search(window, cx);
                    }
                },
            ));
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
                        this.switch_workspace(chat_path.clone(), cx);
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
                    move |this, _, cx| {
                        this.reveal_in_file_manager(&p, cx);
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
                .filter(|candidate| {
                    *candidate == &path
                        || self
                            .sidebar_items_cache
                            .iter()
                            .any(|item| item.path == **candidate && !item.is_dir)
                })
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
                move |this, _, cx| {
                    this.reveal_in_file_manager(&p, cx);
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
                move |this, _, cx| {
                    this.reveal_paths_in_file_manager(&reveal_paths, cx);
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
                    this.reveal_in_file_manager(&reveal_path, cx);
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
            (FormatMenuAction::EditImageDescription, false),
            (FormatMenuAction::EditImageUrl, false),
            (FormatMenuAction::DeleteImage, true),
            (FormatMenuAction::SelectImageFile, false),
        ]
        .into_iter()
        .map(|(action, is_destructive)| {
            let label = self.format_action_label(action);
            self.render_context_menu_item(&label, is_destructive, cx, move |this, _, cx| {
                this.execute_format_menu_action(action, cx);
            })
        })
        .collect();

        items.push(self.render_context_menu_item(
            &format!("{}...", self.t(crate::i18n::TextKey::SaveAs)),
            false,
            cx,
            |this, _, cx| this.save_current_markdown_image_as(cx),
        ));
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
                        this.reveal_in_file_manager(&reveal_path, cx);
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

        let mut bar =
            crate::ui::surface::surface(&self.styles.format_surface(opacity), self.ui_scale)
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
                .when(interactive, |this| this.occlude())
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
                .px(px(metrics.padding));
            for entry in entries {
                row = match entry {
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

        if let Some(ContextMenuKind::TableResize { table_start_line }) =
            self.format_toolbar_table_target(active_line)
        {
            let kind = ContextMenuKind::TableResize { table_start_line };
            let hover_kind = kind.clone();
            let trigger_bounds = Rc::new(Cell::new(None::<Bounds<Pixels>>));
            let click_bounds = trigger_bounds.clone();
            let label = crate::i18n::table_adjustment_label(self.ui_language, false);
            let row = crate::ui::list::list_item(
                self.styles.format_footer_row(),
                Default::default(),
                self.ui_scale,
            )
            .id("format-toolbar-table-adjustment")
            .h_auto()
            .min_h(px(self.scaled(self.styles.context_menu.menu_item_height_px)))
            .child(
                div()
                    .size(px(metrics.button))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        svg()
                            .path(FMT_TABLE_SVG)
                            .size(px(self.scaled(fts.format_toolbar_icon_size_px)))
                            .text_color(fts.format_toolbar_icon_color),
                    ),
            )
            .child(div().flex_1().min_w_0().child(label))
            .child(
                svg()
                    .path("assets/icons/navigation/right.svg")
                    .size(px(self.scaled(12.0)))
                    .text_color(fts.format_toolbar_icon_color),
            )
            .when(interactive, |this| {
                this.on_click(cx.listener(move |this, _, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                    this.open_selection_toolbar_menu(kind.clone(), click_bounds.get(), cx);
                }))
            });
            bar = bar.child(SelectionToolbarHoverRegion {
                child: div()
                    .w_full()
                    .on_children_prepainted(move |bounds: Vec<Bounds<Pixels>>, _, _| {
                        trigger_bounds.set(bounds.first().copied());
                    })
                    .child(row)
                    .into_any_element(),
                editor: cx.entity().clone(),
                region: "table-adjustment-trigger",
                menu_kind: interactive.then_some(hover_kind),
            });
            bar = bar.child(
                crate::ui::list::list_item(
                    self.styles.format_footer_row(),
                    Default::default(),
                    self.ui_scale,
                )
                .id("format-toolbar-table-source")
                .h_auto()
                .min_h(px(self.scaled(self.styles.context_menu.menu_item_height_px)))
                .child(
                    div()
                        .size(px(metrics.button))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(
                            svg()
                                .path(FMT_CODE_SVG)
                                .size(px(self.scaled(fts.format_toolbar_icon_size_px)))
                                .text_color(fts.format_toolbar_icon_color),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .child(crate::i18n::table_source_view_label(self.ui_language)),
                )
                .when(interactive, |this| {
                    this.on_click(cx.listener(move |this, _, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                        this.show_table_source(table_start_line, cx);
                    }))
                }),
            );
        }

        bar.into_any_element()
    }

    fn format_toolbar_table_target(&self, active_line: Option<usize>) -> Option<ContextMenuKind> {
        let line = active_line.unwrap_or_else(|| self.cursor_line());
        let info = self.decorations.get(line)?.table_info.as_ref()?;
        if info.is_front_matter {
            return None;
        }
        Some(ContextMenuKind::TableResize {
            table_start_line: info.table_start_line,
        })
    }

    fn render_format_toolbar_button(
        &self,
        label: String,
        icon: Option<&'static str>,
        _shortcut: Option<SharedString>,
        is_active: bool,
        interactive: bool,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let fts = &self.styles.format_toolbar;
        let group_name = SharedString::from(format!("format-toolbar-{label}"));
        let hover_color = fts.format_toolbar_icon_hover_color;
        let style = self.styles.format_button(is_active);
        let icon_color = style.normal.foreground;
        let icon_size = self.scaled(fts.format_toolbar_icon_size_px);

        let button = crate::ui::button::icon_button(
            SharedString::from(format!("format-toolbar-btn-{label}")),
            style,
            Default::default(),
            self.ui_scale,
        )
        .group(group_name.clone())
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
            this.child(crate::ui::decoration::ellipsis(
                self.styles.format_ellipsis(icon_color),
                self.ui_scale,
            ))
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

        button.into_any_element()
    }

    pub(crate) fn format_toolbar_owns_hover_menu(&self) -> bool {
        self.line_format_toolbar_line.is_some()
            && self.context_menu.as_ref().is_some_and(|menu| {
                matches!(
                    menu.kind,
                    ContextMenuKind::FormatHeading
                        | ContextMenuKind::FormatMore
                        | ContextMenuKind::Table { .. }
                        | ContextMenuKind::TableResize { .. }
                )
            })
    }

    fn open_selection_toolbar_menu(
        &mut self,
        kind: ContextMenuKind,
        bounds: Option<Bounds<Pixels>>,
        cx: &mut Context<Self>,
    ) {
        if self.line_format_toolbar_line.is_none() {
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
        if !self.format_toolbar_owns_hover_menu()
            && !matches!(self.context_submenu, Some(ContextMenuKind::Table { .. }))
            && self.table_adjustment_menu.popup_table.is_none()
        {
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
                    this.selection_toolbar_menu_hover.close_task = None;
                    if this.selection_toolbar_menu_hover.regions.is_empty() {
                        if this.format_toolbar_owns_hover_menu() {
                            this.close_context_menu_immediately(cx);
                        } else if matches!(
                            this.context_submenu,
                            Some(ContextMenuKind::Table { .. })
                        ) {
                            this.table_adjustment_menu = TableAdjustmentMenu::default();
                            this.context_submenu = None;
                            this.context_submenu_hovered = false;
                            this.table_size_hover = None;
                            cx.notify();
                        }
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

            items.push(self.render_context_menu_separator());
            items.push(self.render_chat_attachment_item(self.current_file_path.clone(), None, cx));
            items.push(self.render_chat_attachment_item(
                self.current_file_path.clone(),
                Some(self.cursor.selection_range()),
                cx,
            ));

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

    fn render_format_more_context_menu(
        &self,
        toolbar: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let active_actions = self.active_format_actions();
        let mut items = Vec::new();

        if !toolbar {
            for (action, icon, shortcut) in context_more_actions() {
                items.push(self.render_format_context_menu_item(
                    &self.format_action_label(action),
                    Some(icon),
                    shortcut,
                    false,
                    active_actions.contains(&action),
                    cx,
                    move |this, _, cx| this.execute_format_menu_action(action, cx),
                ));
            }
        } else {
            items.push(self.render_format_context_menu_item(
                &self.format_action_label(FormatMenuAction::HorizontalRule),
                Some(FMT_SPLIT_SVG),
                None,
                false,
                false,
                cx,
                |this, _, cx| this.execute_format_menu_action(FormatMenuAction::HorizontalRule, cx),
            ));
        }

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
                crate::ui::list::list_item(
                    ms.item(),
                    crate::style::ControlState {
                        selected: is_selected,
                        ..Default::default()
                    },
                    self.ui_scale,
                )
                .id(SharedString::from(format!("wiki-link-item-{}", index)))
                .when(
                    self.styles.ui.base_spec.appearance.corners != crate::style::UiCorners::Full,
                    |this| this.w_auto().mx(px(ms.menu_border_width_px)),
                )
                .gap(px(self.scaled(self.styles.sidebar.sidebar_item_gap_px)))
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
            return crate::ui::typography::badge(
                self.styles.sidebar_kind_badge(false),
                self.ui_scale,
            )
            .min_w(px(self.scaled(ss.sidebar_item_badge_min_width_px)))
            .h(px(self.scaled(ss.sidebar_item_badge_height_px)))
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
        if matches!(kind, ContextMenuKind::Format)
            && self.layout_dirty
            && let Some(anchor) = self.slash_menu_anchor
        {
            return anchor.row_height;
        }
        let anchor_offset = if matches!(kind, ContextMenuKind::Format) {
            self.slash_menu_start.unwrap_or(self.cursor.offset)
        } else {
            self.cursor.offset
        };
        if self.table_cell_for_offset(anchor_offset).is_some()
            && let Some(geometry) = self.table_cursor_paint_bounds
            && geometry.offset == anchor_offset
        {
            return f32::from(geometry.paint_bounds.size.height)
                / self.styles.cursor.cursor_height_factor.clamp(0.1, 1.0);
        }
        let (line, _) = self.buffer.offset_to_line_col(anchor_offset);
        self.visual_row_vertical_bounds_for_offset(anchor_offset)
            .map(|(top, bottom)| bottom - top)
            .unwrap_or_else(|| self.visual_height_for_line(line))
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
        let border_w = self.styles.window.window_border_width_px;
        context_menu_panel_layout_geometry(
            f32::from(position.x) - border_w,
            f32::from(position.y) - border_w,
            width,
            estimated_menu_height,
            anchor_height,
            self.last_window_width - 2.0 * border_w,
            self.last_window_height - 2.0 * border_w,
            self.scaled(self.styles.context_menu.menu_max_height_margin_px)
                .max(4.0),
            self.scaled(100.0),
        )
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
        let submenu_width = self.scaled(if matches!(submenu_kind, ContextMenuKind::Table { .. }) {
            TABLE_CONTEXT_MENU_WIDTH_PX
        } else {
            ms.menu_format_width_px
        });
        let submenu_height = self.format_submenu_height(submenu_kind);
        let submenu_gap = self.scaled(ms.menu_submenu_gap_px);
        let main_layout =
            self.context_menu_panel_layout(menu.position, main_menu_width, main_menu_height);
        let main_visible_height = main_menu_height.min(main_layout.max_height);
        let submenu_visible_height = submenu_height
            .min((main_layout.overlay_height - 2.0 * main_layout.margin).max(self.scaled(100.0)));
        let main_menu_top = self
            .format_context_menu_bounds
            .get()
            .map(|bounds| self.context_menu_overlay_coordinates(bounds.origin).1)
            .unwrap_or_else(|| self.context_menu_layout_top(main_layout, main_visible_height));
        let main_menu_bottom = self
            .format_context_menu_bounds
            .get()
            .map(|bounds| {
                self.context_menu_overlay_coordinates(bounds.bottom_left())
                    .1
            })
            .unwrap_or(main_menu_top + main_visible_height);
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
        let align_bottom = matches!(submenu_kind, ContextMenuKind::FormatMore);
        let desired_top = if align_bottom {
            main_menu_bottom - submenu_visible_height
        } else if let Some(bounds) = measured_trigger {
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
                ContextMenuKind::FormatMore => main_menu_bottom - submenu_visible_height,
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
            max_height: if align_bottom {
                (main_menu_bottom - main_layout.margin).max(1.0)
            } else {
                (main_layout.overlay_height - 2.0 * main_layout.margin).max(self.scaled(100.0))
            },
            bottom_anchor: align_bottom.then_some(main_layout.overlay_height - main_menu_bottom),
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

        let submenu_width = self.scaled(if matches!(submenu_kind, ContextMenuKind::Table { .. }) {
            TABLE_CONTEXT_MENU_WIDTH_PX
        } else {
            self.styles.context_menu.menu_format_width_px
        });
        let submenu_height = self
            .format_submenu_height(submenu_kind)
            .min(submenu_layout.max_height);
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
        let table_resize_panel = panel_id == "table-cells-popup"
            || self
                .context_menu
                .as_ref()
                .is_some_and(|menu| matches!(menu.kind, ContextMenuKind::TableResize { .. }));
        let fixed_corner_padding = !table_resize_panel
            && self.styles.ui.base_spec.appearance.corners == crate::style::UiCorners::Full;

        let mut surface = ms.surface();
        surface.radius_px = self.scaled(surface.radius_px);
        let mut menu_div = crate::ui::surface::surface(&surface, 1.0)
            .id(panel_id)
            .absolute()
            .left(px(layout.left))
            .w(px(width))
            .max_h(px(layout.max_height))
            .opacity(self.context_menu_opacity.clamp(0.0, 1.0))
            // Occlude so hovering a menu item does not also hover the sidebar /
            // file-tree item painted underneath the menu.
            .occlude()
            .when(table_resize_panel, |this| {
                if panel_id == "table-cells-popup" {
                    this.overflow_y_scroll()
                } else {
                    this.overflow_hidden()
                }
            })
            .when(!table_resize_panel && !fixed_corner_padding, |this| {
                this.overflow_y_scroll()
                    .py(px(self.scaled(ms.menu_padding_y_px)))
            })
            .when(fixed_corner_padding, |this| {
                this.overflow_hidden()
                    .flex()
                    .flex_col()
                    .py(px(self.scaled(ms.menu_padding_y_px)))
            });

        if let Some(bottom_anchor) = layout.bottom_anchor {
            menu_div = menu_div.bottom(px(bottom_anchor));
        } else {
            menu_div = menu_div.top(px(layout.estimated_top));
        }

        let menu_div = if fixed_corner_padding {
            menu_div.child(
                crate::ui::surface::menu_scroll_area(SharedString::from(format!(
                    "{panel_id}-scroll"
                )))
                .children(items),
            )
        } else {
            menu_div.children(items)
        };
        let menu_div = menu_div
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

        let menu_div = if !self.format_toolbar_owns_hover_menu()
            && self
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

        let is_table_panel = (panel_id == "context-submenu"
            && matches!(self.context_submenu, Some(ContextMenuKind::Table { .. })))
            || (panel_id == "context-menu"
                && self.format_toolbar_owns_hover_menu()
                && matches!(
                    self.context_menu.as_ref().map(|menu| &menu.kind),
                    Some(ContextMenuKind::Table { .. })
                ));
        if self.format_toolbar_owns_hover_menu()
            || is_table_panel
            || panel_id == "table-cells-popup"
        {
            return SelectionToolbarHoverRegion {
                child: menu_div.into_any_element(),
                editor: cx.entity().clone(),
                region: if is_table_panel {
                    "table-adjustment-menu"
                } else {
                    panel_id
                },
                menu_kind: None,
            }
            .into_any_element();
        }

        if panel_id == "context-menu" {
            let measured_bounds = self.format_context_menu_bounds.clone();
            return div()
                .absolute()
                .size_full()
                .on_children_prepainted(move |bounds: Vec<Bounds<Pixels>>, _, _| {
                    measured_bounds.set(bounds.first().copied());
                })
                .child(menu_div)
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
        let chat_rows = 2.0;
        let separator_count = 4.0;
        padding
            + item_height * (3.0 + chat_rows + 5.0 + 2.0 + 1.0)
            + separator_height * separator_count
    }

    fn format_more_menu_height(&self) -> f32 {
        let ms = &self.styles.context_menu;
        let item_height = self.scaled(ms.menu_item_height_px);
        let padding = self.scaled(ms.menu_padding_y_px) * 2.0;
        let separator_height = self.context_menu_separator_block_height();
        padding + item_height * (7 + context_more_actions().len()) as f32 + separator_height * 2.0
    }

    fn format_heading_menu_height(&self) -> f32 {
        let ms = &self.styles.context_menu;
        let item_height = self.scaled(ms.menu_item_height_px);
        let padding = self.scaled(ms.menu_padding_y_px) * 2.0;
        padding + item_height * 7.0
    }

    fn table_context_menu_height(&self, is_front_matter: bool) -> f32 {
        let (items, separators) = TableMenuLayout::new(is_front_matter).counts();
        self.scaled(self.styles.context_menu.menu_padding_y_px) * 2.0
            + self.scaled(self.styles.context_menu.menu_item_height_px) * items as f32
            + self.context_menu_separator_block_height() * separators as f32
    }

    fn format_submenu_height(&self, submenu_kind: &ContextMenuKind) -> f32 {
        match submenu_kind {
            ContextMenuKind::Export { .. } => {
                self.scaled(self.styles.context_menu.menu_padding_y_px) * 2.0
                    + self.scaled(self.styles.context_menu.menu_item_height_px) * 10.0
            }
            ContextMenuKind::FormatHeading => self.format_heading_menu_height(),
            ContextMenuKind::FormatMore => self.format_more_menu_height(),
            ContextMenuKind::TableBorder {
                table_start_line, ..
            } => self.table_context_menu_height(self.block_is_front_matter(*table_start_line)),
            ContextMenuKind::Table {
                table_start_line, ..
            } => self.table_context_menu_height(self.block_is_front_matter(*table_start_line)),
            _ => self.format_more_menu_height(),
        }
    }

    fn render_export_context_submenu(
        &self,
        path: &Path,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let formats = [
            (
                "Markdown",
                "assets/icons/menu/markdown.svg",
                super::export::ExportFormat::Markdown,
            ),
            (
                "HTML",
                "assets/icons/menu/html.svg",
                super::export::ExportFormat::Html,
            ),
            (
                "PDF",
                "assets/icons/menu/pdf.svg",
                super::export::ExportFormat::Pdf,
            ),
            (
                "PNG",
                "assets/icons/menu/png.svg",
                super::export::ExportFormat::Png,
            ),
        ];
        [
            super::export::ExportTarget::File,
            super::export::ExportTarget::Clipboard,
        ]
        .into_iter()
        .flat_map(|target| formats.into_iter().map(move |entry| (target, entry)))
        .map(|(target, (label, icon_path, format))| {
            let path = path.to_path_buf();
            let label = if target == super::export::ExportTarget::Clipboard {
                format!(
                    "{label}\n({})",
                    self.t(crate::i18n::TextKey::CopyToClipboard)
                )
            } else {
                label.to_string()
            };
            self.render_context_menu_item_with_icon_behavior(
                &label,
                icon_path,
                false,
                false,
                cx,
                move |this, _, cx| {
                    this.close_context_menu(cx);
                    this.export_path(path.clone(), format, target, cx);
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
        border_axis: Option<bool>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let border = border_axis.and_then(|is_column| {
            self.table_handles.selection.as_ref().filter(|selection| {
                selection.table_start_line == table_start_line
                    && selection.is_column == is_column
                    && selection.contains(if is_column { col } else { row })
            })
        });
        let selection = self
            .table_cell_selection
            .as_ref()
            .filter(|selection| selection.contains(table_start_line, row, col));
        let (first_row, last_row, first_column, last_column) = selection
            .map(|selection| selection.insertion_edges())
            .unwrap_or((row, row, col, col));
        let selected_rows = selection
            .map(|selection| selection.rows.clone().collect::<Vec<_>>())
            .unwrap_or_else(|| vec![row]);
        let selected_columns = selection
            .map(|selection| selection.columns.clone().collect::<Vec<_>>())
            .unwrap_or_else(|| vec![col]);
        let is_front_matter = self
            .decorations
            .get(table_start_line)
            .and_then(|dec| dec.table_info.as_ref())
            .is_some_and(|info| info.is_front_matter);
        let target = if let Some(is_column) = border_axis {
            TableMenuSelection::for_border(
                table_start_line,
                is_column,
                if is_column { col } else { row },
                border,
                is_front_matter,
            )
        } else {
            TableMenuSelection {
                table: table_start_line,
                rows: selected_rows,
                columns: selected_columns,
                first_row,
                last_row,
                first_column,
                last_column,
                is_front_matter,
                border_axis: None,
            }
        };
        let mut items = self.render_table_structure_menu(target, cx);
        items.push(self.render_context_menu_separator());
        items.extend(self.render_table_cells_context_menu(
            table_start_line,
            row,
            col,
            border_axis,
            cx,
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
        let grid_style = self.styles.table_size_grid();
        let gap = self.scaled(grid_style.gap_px);
        let padding = self.scaled(ms.menu_padding_y_px);
        let mut rows = Vec::with_capacity(MAX_ROWS);

        for row in 1..=MAX_ROWS {
            let mut cells = Vec::with_capacity(MAX_COLUMNS);
            for column in 1..=MAX_COLUMNS {
                let hovered = row <= hover_rows && column <= hover_columns;
                let currently_covered = row <= current_rows && column <= current_columns;
                let cell = crate::ui::list::selection_grid_cell(
                    grid_style,
                    currently_covered,
                    false,
                    self.ui_scale,
                )
                .id(SharedString::from(format!(
                    "table-size-{table_start_line}-{row}-{column}"
                )))
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
                );
                cells.push(crate::ui::motion::transition(
                    ("table-grid-hover", row * MAX_COLUMNS + column),
                    cell,
                    if hovered { 1.0 } else { 0.0 },
                    None,
                    self.styles
                        .ui
                        .spec
                        .motion_duration(self.styles.ui.spec.hover_motion_duration),
                    self.styles.ui.spec.easing,
                    move |cell, progress| {
                        cell.bg(gpui::interpolate_color(
                            if currently_covered {
                                grid_style.current
                            } else {
                                gpui::transparent_black()
                            },
                            if currently_covered {
                                grid_style.overlap
                            } else {
                                grid_style.hover
                            },
                            progress,
                        ))
                        .border_color(gpui::interpolate_color(
                            if currently_covered {
                                grid_style.active_border
                            } else {
                                grid_style.border
                            },
                            grid_style.active_border,
                            progress,
                        ))
                    },
                ));
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
            ContextMenuKind::WorkspaceRoot { path } => (
                self.render_sidebar_context_menu(path.clone(), true, true, cx),
                self.scaled(self.styles.context_menu.menu_sidebar_width_px),
            ),
            ContextMenuKind::Sidebar { path, is_dir } => (
                self.render_sidebar_context_menu(path.clone(), *is_dir, false, cx),
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
                self.scaled(if self.current_image_menu_target().is_some() {
                    188.0
                } else {
                    self.styles.context_menu.menu_format_width_px
                }),
            ),
            ContextMenuKind::FrontMatterType {
                line,
                pane_id,
                tab_id,
                source,
            } => {
                let context_matches = front_matter_type_menu_matches(
                    self.current_pane_id(),
                    self.active_tab_id.as_deref(),
                    self.buffer.text(),
                    *pane_id,
                    tab_id.as_deref(),
                    source.as_ref(),
                );
                let current = if context_matches {
                    front_matter::property_type(&self.buffer, &self.decorations, *line)
                } else {
                    None
                };
                let read_only = self.active_tab_is_read_only();
                let label = |key| crate::i18n::front_matter_type_label(self.ui_language, key);

                let ms = &self.styles.context_menu;
                let icon_styles = &self.styles.context_menu;
                let text_width = front_matter::TYPES
                    .iter()
                    .map(|ty| {
                        shortcut::estimate_text_width(
                            label(front_matter_type_label_key(*ty)),
                            self.scaled(ms.menu_item_font_size_px),
                        )
                    })
                    .fold(0.0, f32::max);
                let (icon_col_w, gap) = menu_icon_column_metrics(
                    ms.menu_format_icon_size_px,
                    icon_styles.menu_item_check_size_px,
                    icon_styles.menu_item_action_gap_px,
                );
                let width = (text_width
                    + self.scaled(
                        2.0 * ms.menu_border_width_px
                            + 2.0 * ms.menu_item_padding_x_px
                            + icon_col_w
                            + 2.0 * gap
                            + icon_styles.menu_item_check_size_px,
                    ))
                .ceil();
                let mut items = Vec::with_capacity(front_matter::TYPES.len());
                for target in front_matter::TYPES {
                    let enabled =
                        front_matter_type_choice_enabled(current, target, read_only, || {
                            front_matter::type_change_edit(
                                &self.buffer,
                                &self.decorations,
                                *line,
                                target,
                            )
                            .is_some()
                        });
                    let disabled_reason = if enabled {
                        None
                    } else if !context_matches || current.is_none() {
                        Some(label("changed"))
                    } else if read_only {
                        Some(label("read_only"))
                    } else {
                        Some(label("changed"))
                    };
                    let line = *line;
                    let pane_id = *pane_id;
                    let tab_id = tab_id.clone();
                    let source = source.clone();
                    items.push(self.render_front_matter_type_choice_item(
                        target,
                        current == Some(target),
                        disabled_reason,
                        cx,
                        move |this, cx| {
                            if !front_matter_type_menu_matches(
                                this.current_pane_id(),
                                this.active_tab_id.as_deref(),
                                this.buffer.text(),
                                pane_id,
                                tab_id.as_deref(),
                                source.as_ref(),
                            ) || this.active_tab_is_read_only()
                            {
                                return;
                            }
                            let Some(current) =
                                front_matter::property_type(&this.buffer, &this.decorations, line)
                            else {
                                return;
                            };
                            if current == target {
                                return;
                            }
                            this.change_front_matter_type(line, target, cx);
                        },
                    ));
                }
                (items, width)
            }
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
                            this.reveal_in_file_manager(&path, cx);
                        }
                        this.close_context_menu(cx);
                    },
                ));
                (
                    items,
                    self.scaled(self.styles.context_menu.menu_sidebar_width_px),
                )
            }
            ContextMenuKind::FormatMore => (
                self.render_format_more_context_menu(true, cx),
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
                    if tab.kind == TabKind::Editor {
                        items.push(self.render_chat_attachment_item(tab.path.clone(), None, cx));
                    }
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
                                    this.reveal_in_file_manager(&reveal_path, cx);
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
                        agent_cli.localized_label(self.ui_language),
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
            ContextMenuKind::TableBorder {
                table_start_line,
                is_column,
                index,
            } => (
                self.render_table_context_submenu(
                    *table_start_line,
                    if *is_column { 0 } else { *index },
                    if *is_column { *index } else { 0 },
                    Some(*is_column),
                    cx,
                ),
                self.scaled(TABLE_CONTEXT_MENU_WIDTH_PX),
            ),
            ContextMenuKind::Table {
                table_start_line,
                row,
                col,
            } => (
                self.render_table_context_submenu(*table_start_line, *row, *col, None, cx),
                self.scaled(TABLE_CONTEXT_MENU_WIDTH_PX),
            ),
            ContextMenuKind::TableResize { table_start_line } => (
                self.render_table_size_picker(*table_start_line, cx),
                self.scaled(105.0),
            ),
        };

        let estimated_menu_height =
            if matches!(menu.kind, ContextMenuKind::Format) && self.slash_menu_start.is_some() {
                // Choose the side using the unfiltered height. Otherwise narrowing
                // the query can flip the menu across the anchor row mid-typing.
                self.estimated_context_menu_height(self.slash_menu_items().len())
            } else if matches!(menu.kind, ContextMenuKind::TableResize { .. }) {
                self.scaled(105.0 + self.styles.context_menu.menu_item_height_px)
            } else if matches!(
                menu.kind,
                ContextMenuKind::Table { .. } | ContextMenuKind::TableBorder { .. }
            ) {
                self.format_submenu_height(&menu.kind)
            } else {
                self.estimated_context_menu_height(items.len())
            };
        let anchor_height = self.context_menu_anchor_height(&menu.kind);
        let toolbar_bounds = self
            .format_toolbar_owns_hover_menu()
            .then(|| {
                self.line_format_toolbar_line
                    .and_then(|_| self.line_format_toolbar_bounds)
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
                ContextMenuKind::FormatHeading
                    | ContextMenuKind::FormatMore
                    | ContextMenuKind::Table { .. }
                    | ContextMenuKind::TableResize { .. }
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
                matches!(
                    menu.kind,
                    ContextMenuKind::FormatMore | ContextMenuKind::Table { .. }
                ),
            );
            let align_bottom = matches!(
                menu.kind,
                ContextMenuKind::FormatMore | ContextMenuKind::Table { .. }
            );
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
        } else {
            self.context_menu_panel_layout_with_anchor(
                anchor_position,
                width,
                estimated_menu_height,
                anchor_height,
            )
        };
        if matches!(menu.kind, ContextMenuKind::TableResize { .. }) && toolbar_bounds.is_none() {
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
            && !self.format_toolbar_owns_hover_menu()
            && self.can_render_format_submenu();
        let submenu = if export_submenu_open || format_submenu_open {
            let submenu_kind = self.context_submenu.as_ref().cloned();
            let submenu_items = match submenu_kind.as_ref() {
                Some(ContextMenuKind::Export { path }) => {
                    self.render_export_context_submenu(path, cx)
                }
                Some(ContextMenuKind::FormatHeading) => self.render_heading_context_menu(cx),
                Some(ContextMenuKind::FormatMore) => {
                    self.render_format_more_context_menu(false, cx)
                }
                Some(ContextMenuKind::Table {
                    table_start_line,
                    row,
                    col,
                }) => self.render_table_context_submenu(*table_start_line, *row, *col, None, cx),
                _ => Vec::new(),
            };
            let submenu_width = if export_submenu_open {
                self.scaled(self.styles.context_menu.menu_export_width_px)
            } else if matches!(submenu_kind, Some(ContextMenuKind::Table { .. })) {
                self.scaled(TABLE_CONTEXT_MENU_WIDTH_PX)
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

        let active_table = match menu.kind {
            ContextMenuKind::Table {
                table_start_line, ..
            } => Some(table_start_line),
            _ => match self.context_submenu {
                Some(ContextMenuKind::Table {
                    table_start_line, ..
                }) => Some(table_start_line),
                _ => None,
            },
        };
        let cells_popup = self
            .table_adjustment_menu
            .popup_table
            .filter(|table| Some(*table) == active_table)
            .zip(self.table_adjustment_menu.bounds.get())
            .map(|(table_start_line, bounds)| {
                let (left, top) = self.context_menu_overlay_coordinates(bounds.origin);
                let popup_width = self.scaled(105.0);
                let gap = self.scaled(self.styles.context_menu.menu_submenu_gap_px);
                let popup_layout = table_cells_popup_layout(
                    Bounds::new(Point::new(px(left), px(top)), bounds.size),
                    layout,
                    popup_width,
                    gap,
                );
                let picker = self.render_table_size_picker(table_start_line, cx);
                self.render_context_menu_panel(
                    "table-cells-popup",
                    popup_layout,
                    popup_width,
                    picker,
                    cx,
                )
            });

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
                                    .line_format_toolbar_line
                                    .and_then(|line| {
                                        this.line_format_toolbar_geometries
                                            .iter()
                                            .find(|geometry| geometry.line == line)
                                    })
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
            .when_some(cells_popup, |this, popup| this.child(popup))
            .into_any_element()
    }
}

fn slash_menu_position_for_layout(
    layout_dirty: bool,
    last_measured: Option<SlashMenuAnchor>,
    scroll_offset: f32,
    current_measurement: impl FnOnce() -> Point<Pixels>,
) -> Point<Pixels> {
    if layout_dirty {
        if let Some(anchor) = last_measured {
            return anchor.at_scroll_offset(scroll_offset);
        }
    }
    current_measurement()
}

fn slash_menu_item_matches(item: &SlashMenuItem, lowercase_filter: &str) -> bool {
    lowercase_filter.is_empty()
        || item.label.to_lowercase().contains(lowercase_filter)
        || item.filter_text.contains(lowercase_filter)
}

fn context_menu_panel_layout_geometry(
    x: f32,
    y: f32,
    width: f32,
    estimated_menu_height: f32,
    anchor_height: f32,
    overlay_w: f32,
    overlay_h: f32,
    margin: f32,
    min_height: f32,
) -> ContextMenuPanelLayout {
    let pos_x = x.clamp(0.0, overlay_w);
    let pos_y = y.clamp(0.0, overlay_h);
    // Input-anchored positions are the bottom of the visual row.
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
    let available_height = if open_down { space_below } else { space_above };
    // Input menus should never cover the anchor row, even in a short window.
    let max_height = available_height.max(if anchor_height > 0.0 { 1.0 } else { min_height });
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
mod front_matter_type_menu_tests {
    use super::{
        front_matter, front_matter_type_choice_enabled, front_matter_type_label_key,
        front_matter_type_menu_matches,
    };
    use front_matter::FrontMatterType as Type;

    #[test]
    fn type_menu_requires_the_same_pane_tab_and_entire_source() {
        let source = "---\nvalue: hello\n---\nbody\n";
        assert!(front_matter_type_menu_matches(
            1,
            Some("tab"),
            source,
            1,
            Some("tab"),
            source,
        ));
        assert!(front_matter_type_menu_matches(
            1, None, source, 1, None, source
        ));
        assert!(!front_matter_type_menu_matches(
            2,
            Some("tab"),
            source,
            1,
            Some("tab"),
            source,
        ));
        assert!(!front_matter_type_menu_matches(
            1,
            Some("other"),
            source,
            1,
            Some("tab"),
            source,
        ));
        assert!(!front_matter_type_menu_matches(
            1,
            None,
            source,
            1,
            Some("tab"),
            source,
        ));
        assert!(!front_matter_type_menu_matches(
            1,
            Some("tab"),
            "---\nvalue: hello\n---\nchanged body\n",
            1,
            Some("tab"),
            source,
        ));
    }

    #[test]
    fn stale_context_disables_all_choices_without_planning() {
        let source = "---\nvalue: !custom hello\n---\nbody\n";
        let buffer = crate::buffer::Buffer::from_text(source);
        let decorations = crate::markdown::parse_decorations(source);
        for (pane, tab, current_source) in [
            (2, Some("tab"), source),
            (1, Some("other"), source),
            (1, None, source),
            (
                1,
                Some("tab"),
                "---\nvalue: !custom hello\n---\nchanged body\n",
            ),
            (1, Some("tab"), "---\nvalue: changed\n---\nbody\n"),
        ] {
            let context_matches =
                front_matter_type_menu_matches(pane, tab, current_source, 1, Some("tab"), source);
            let current = if context_matches {
                front_matter::property_type(&buffer, &decorations, 1)
            } else {
                None
            };
            assert!(!context_matches);
            for target in front_matter::TYPES {
                assert!(!front_matter_type_choice_enabled(
                    current,
                    target,
                    false,
                    || panic!("stale contexts must not plan edits"),
                ));
            }
        }
    }

    #[test]
    fn unavailable_and_current_choices_skip_conversion_planning() {
        let must_not_plan = || panic!("conversion planning should be skipped");
        for target in front_matter::TYPES {
            assert!(!front_matter_type_choice_enabled(
                None,
                target,
                false,
                must_not_plan,
            ));
            assert!(front_matter_type_choice_enabled(
                Some(target),
                target,
                false,
                must_not_plan,
            ));
            for current in front_matter::TYPES.into_iter().chain([Type::Raw]) {
                assert!(!front_matter_type_choice_enabled(
                    Some(current),
                    target,
                    true,
                    must_not_plan,
                ));
            }
        }
        assert!(!front_matter_type_choice_enabled(
            Some(Type::Text),
            Type::Raw,
            false,
            must_not_plan,
        ));
    }

    #[test]
    fn every_source_type_can_choose_every_selectable_type() {
        for current in front_matter::TYPES.into_iter().chain([Type::Raw]) {
            for target in front_matter::TYPES {
                assert!(front_matter_type_choice_enabled(
                    Some(current),
                    target,
                    false,
                    || true,
                ));
            }
        }
    }

    #[test]
    fn choices_still_require_a_lexically_valid_edit_plan() {
        for current in front_matter::TYPES.into_iter().chain([Type::Raw]) {
            for target in front_matter::TYPES {
                assert_eq!(
                    front_matter_type_choice_enabled(Some(current), target, false, || false),
                    current == target,
                );
            }
        }
    }

    #[test]
    fn lossy_raw_and_collection_choices_follow_the_reset_capable_plan() {
        for source in [
            "---\nvalue: hello\n---\n",
            "---\nvalue: |-\n  hello\n  world\n---\n",
            "---\nvalue: true\n---\n",
            "---\nvalue: 2026-10-13\n---\n",
            "---\nvalue: 2026-10-13T12:30:00\n---\n",
            "---\nvalue: [hello, world]\n---\n",
            "---\nvalue: 42\n---\n",
            "---\nvalue: null\n---\n",
            "---\nvalue: {nested: value}\n---\n",
            "---\nvalue: !custom hello\n---\n",
            "---\nvalue: &anchor hello\n---\n",
            "---\nvalue: *anchor\n---\n",
            "---\nvalue:\n  nested: value\n  # child comment\n---\n",
        ] {
            let buffer = crate::buffer::Buffer::from_text(source);
            let decorations = crate::markdown::parse_decorations(source);
            let current = front_matter::property_type(&buffer, &decorations, 1);
            assert!(current.is_some(), "{source}");
            for target in front_matter::TYPES {
                assert!(
                    front_matter_type_choice_enabled(current, target, false, || {
                        front_matter::type_change_edit(&buffer, &decorations, 1, target).is_some()
                    }),
                    "{current:?} -> {target:?}: {source}",
                );
                assert_eq!(buffer.text(), source);
            }
        }
    }

    #[test]
    fn non_property_contexts_disable_all_choices_without_planning() {
        let source = "---\n# comment\nvalue:\n  - child\n---\nbody\n";
        let buffer = crate::buffer::Buffer::from_text(source);
        let decorations = crate::markdown::parse_decorations(source);
        for line in [0, 1, 3, 4, 5, buffer.line_count()] {
            let current = front_matter::property_type(&buffer, &decorations, line);
            assert_eq!(current, None, "line {line}");
            for target in front_matter::TYPES {
                assert!(!front_matter_type_choice_enabled(
                    current,
                    target,
                    false,
                    || panic!("non-property contexts must not plan edits"),
                ));
                assert!(
                    front_matter::type_change_edit(&buffer, &decorations, line, target).is_none()
                );
            }
        }
    }

    #[test]
    fn exactly_nine_distinct_types_are_selectable_and_raw_is_not_one() {
        let keys: std::collections::HashSet<_> = front_matter::TYPES
            .iter()
            .copied()
            .map(front_matter_type_label_key)
            .collect();
        assert_eq!(keys.len(), 9);
        assert!(!keys.contains(front_matter_type_label_key(Type::Raw)));
        for &language in crate::i18n::UiLanguage::supported() {
            for key in &keys {
                assert!(!crate::i18n::front_matter_type_label(language, key).is_empty());
            }
        }
    }
}

#[cfg(test)]
mod slash_menu_tests {
    use super::*;

    #[test]
    fn zero_matches_keep_the_query_available_for_backspacing() {
        let item = SlashMenuItem {
            label: "Code Block".into(),
            filter_text: "code block",
            icon: None,
            shortcut: None,
            action: FormatMenuAction::CodeBlock,
        };
        let mut buffer = crate::buffer::Buffer::from_text("/codezzz");
        let query =
            input_ime::slash_menu_query(buffer.text(), 0, buffer.len(), true, false).unwrap();
        assert!(!slash_menu_item_matches(&item, &query.to_lowercase()));
        assert_eq!(buffer.text(), "/codezzz");

        buffer.replace(5..8, "");
        let query =
            input_ime::slash_menu_query(buffer.text(), 0, buffer.len(), true, false).unwrap();
        assert!(slash_menu_item_matches(&item, &query.to_lowercase()));
        assert_eq!(buffer.text(), "/code");
    }

    #[test]
    fn slash_anchor_ignores_provisional_geometry_until_prepaint_finishes() {
        let measured = SlashMenuAnchor {
            position: gpui::point(gpui::px(100.0), gpui::px(300.0)),
            row_height: 24.0,
            scroll_offset: 120.0,
        };
        let provisional = gpui::point(gpui::px(100.0), gpui::px(90.0));
        assert_eq!(
            slash_menu_position_for_layout(true, Some(measured), 120.0, || {
                panic!("must not measure invalid layout")
            }),
            measured.position
        );
        assert_eq!(
            slash_menu_position_for_layout(true, Some(measured), 140.0, || provisional),
            gpui::point(gpui::px(100.0), gpui::px(280.0))
        );
        let settled = gpui::point(gpui::px(100.0), gpui::px(280.0));
        assert_eq!(
            slash_menu_position_for_layout(true, None, 140.0, || provisional),
            provisional
        );
        assert_eq!(
            slash_menu_position_for_layout(false, Some(measured), 140.0, || settled),
            settled
        );
    }

    #[test]
    fn filtered_menu_keeps_the_same_row_edge_and_direction() {
        let layout = |y| {
            context_menu_panel_layout_geometry(
                120.0, y, 200.0, 250.0, 20.0, 800.0, 400.0, 8.0, 100.0,
            )
        };
        let above = layout(340.0);
        assert_eq!(above.bottom_anchor, Some(80.0));
        // The actual (filtered) panel shrinks toward the row, not away from it.
        for visible_height in [250.0, 80.0, 20.0] {
            let top = above.overlay_height - above.bottom_anchor.unwrap() - visible_height;
            assert_eq!(top + visible_height, 320.0);
        }
        assert_eq!(above.max_height, 312.0);
        let below = layout(100.0);
        assert_eq!(below.bottom_anchor, None);
        assert_eq!(below.estimated_top, 100.0);
        // In a constrained viewport, cap the popup rather than cover the row.
        let constrained = context_menu_panel_layout_geometry(
            120.0, 28.0, 200.0, 250.0, 20.0, 800.0, 90.0, 8.0, 100.0,
        );
        assert_eq!(constrained.estimated_top, 28.0);
        assert_eq!(constrained.max_height, 54.0);
    }
}

#[cfg(test)]
mod format_toolbar_layout_tests {
    use super::*;

    #[test]
    fn context_more_restores_actions_already_exposed_by_the_toolbar() {
        let actions = context_more_actions().map(|(action, _, _)| action);
        assert!(
            actions
                == [
                    FormatMenuAction::UnorderedList,
                    FormatMenuAction::OrderedList,
                    FormatMenuAction::Image,
                    FormatMenuAction::Table,
                    FormatMenuAction::CodeBlock,
                    FormatMenuAction::Quote,
                    FormatMenuAction::HorizontalRule,
                ]
        );
    }

    fn metrics() -> FormatToolbarMetrics {
        FormatToolbarMetrics {
            button: 22.0,
            gap: 1.0,
            padding: 8.0,
            border: 1.0,
        }
    }

    #[test]
    fn toolbar_has_the_requested_three_rows() {
        let ([first, second, third], width) = format_toolbar_layout(metrics());
        let actions = |entries: &[FormatToolbarEntry]| {
            entries
                .iter()
                .filter_map(|entry| match entry {
                    FormatToolbarEntry::Action { action, .. } => Some(*action),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert!(
            actions(&first)
                == vec![
                    FormatMenuAction::Heading1,
                    FormatMenuAction::Heading2,
                    FormatMenuAction::Heading3,
                    FormatMenuAction::Heading4,
                    FormatMenuAction::Heading5,
                    FormatMenuAction::Heading6,
                    FormatMenuAction::Paragraph
                ]
        );
        assert!(
            actions(&second)
                == vec![
                    FormatMenuAction::Bold,
                    FormatMenuAction::Italic,
                    FormatMenuAction::Strikethrough,
                    FormatMenuAction::Underline,
                    FormatMenuAction::InlineCode,
                    FormatMenuAction::Link,
                    FormatMenuAction::Image,
                ]
        );
        assert!(
            actions(&third)
                == vec![
                    FormatMenuAction::Todo,
                    FormatMenuAction::UnorderedList,
                    FormatMenuAction::OrderedList,
                    FormatMenuAction::Quote,
                    FormatMenuAction::Table,
                    FormatMenuAction::CodeBlock,
                ]
        );
        assert!(matches!(third.last(), Some(FormatToolbarEntry::MoreMenu)));
        assert_eq!([first.len(), second.len(), third.len()], [7, 7, 7]);
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
    fn cell_picker_stays_top_aligned_and_switches_sides_near_window_edges() {
        let overlay = super::ContextMenuPanelLayout {
            left: 0.0,
            estimated_top: 0.0,
            max_height: 600.0,
            bottom_anchor: None,
            overlay_width: 800.0,
            overlay_height: 600.0,
            margin: 8.0,
        };
        for (left, top, expected_left, expected_height) in
            [(200.0, 120.0, 424.0, 472.0), (550.0, 510.0, 441.0, 82.0)]
        {
            let parent = gpui::Bounds::new(
                gpui::point(gpui::px(left), gpui::px(top)),
                gpui::size(gpui::px(220.0), gpui::px(200.0)),
            );
            let layout = super::table_cells_popup_layout(parent, overlay, 105.0, 4.0);
            assert_eq!(layout.left, expected_left);
            assert_eq!(layout.estimated_top, top);
            assert_eq!(layout.max_height, expected_height);
            assert!(layout.bottom_anchor.is_none());
        }
    }

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

#[cfg(test)]
mod table_menu_tests {
    use super::{TableMenuLayout, TableMenuSelection};
    use crate::editor::table_handles::TableBorderSelection;

    #[test]
    fn handles_only_target_the_selected_structural_axis() {
        let columns = TableBorderSelection {
            table_start_line: 7,
            is_column: true,
            anchor: 3,
            indices: vec![1, 3],
        };
        let target = TableMenuSelection::for_border(7, true, 3, Some(&columns), false);
        assert_eq!(target.rows, [0]);
        assert_eq!(target.columns, [1, 3]);
        assert_eq!((target.first_column, target.last_column), (1, 3));
        let layout = TableMenuLayout::new(false);
        assert!(layout.rows && layout.columns && layout.alignment);

        let rows = TableBorderSelection {
            table_start_line: 7,
            is_column: false,
            anchor: 2,
            indices: vec![2, 4],
        };
        let target = TableMenuSelection::for_border(7, false, 4, Some(&rows), false);
        assert_eq!(target.rows, [2, 4]);
        assert_eq!(target.columns, [0]);
        assert_eq!((target.first_row, target.last_row), (2, 4));
        let layout = TableMenuLayout::new(false);
        assert!(layout.rows && layout.columns && layout.alignment);
    }

    #[test]
    fn front_matter_column_handle_has_the_cell_menu_structure() {
        let target = TableMenuSelection::for_border(7, true, 0, None, true);
        assert_eq!(target.rows, [0]);
        let layout = TableMenuLayout::new(target.is_front_matter);
        assert!(layout.rows && !layout.columns && !layout.alignment);
        assert_eq!(layout.counts(), (8, 4));
    }

    #[test]
    fn table_menu_height_counts_match_items_and_separators_by_context() {
        assert_eq!(TableMenuLayout::new(false).counts(), (15, 5));
        assert_eq!(TableMenuLayout::new(true).counts(), (8, 4));
    }

    #[test]
    fn cross_axis_delete_targets_only_the_clicked_row_or_column() {
        let lines = [
            "| A | B | C |",
            "| --- | --- | --- |",
            "| a | b | c |",
            "| d | e | f |",
        ]
        .map(str::to_string);
        let all_columns = TableBorderSelection {
            table_start_line: 0,
            is_column: true,
            anchor: 0,
            indices: vec![0, 1, 2],
        };
        let target = TableMenuSelection::for_border(0, true, 1, Some(&all_columns), false);
        assert_eq!(target.rows, [0]);
        let updated = crate::table_ops::delete_rows_at(&lines, &target.rows);
        let model = crate::table_ops::parse_table_block(&updated);
        assert_eq!(model.header_cells, ["a", "b", "c"]);
        assert_eq!(model.data_rows, [["d", "e", "f"]]);

        let all_rows = TableBorderSelection {
            table_start_line: 0,
            is_column: false,
            anchor: 0,
            indices: vec![0, 1, 2],
        };
        let target = TableMenuSelection::for_border(0, false, 1, Some(&all_rows), false);
        assert_eq!(target.columns, [0]);
        let updated = crate::table_ops::delete_columns_at(&lines, &target.columns);
        let model = crate::table_ops::parse_table_block(&updated);
        assert_eq!(model.header_cells, ["B", "C"]);
        assert_eq!(model.data_rows, [["b", "c"], ["e", "f"]]);
    }
}
