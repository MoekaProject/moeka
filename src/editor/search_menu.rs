//! Unified searchable menu component: the single implementation of the
//! anchored "search input + filtered rows" overlay used by the language /
//! indentation picker, the git branch picker, and the settings / whiteboard
//! pickers (fonts, interface language, formats, …). All shared styling
//! (panel, input, rows, empty state) lives here so every searchable menu
//! looks and behaves identically.

use gpui::{
    AnyElement, Bounds, Context, CursorStyle, ElementId, Entity, MouseButton, Pixels, ScrollHandle,
    SharedString, Window, div, prelude::*, px, svg,
};

use super::*;

/// Fixed metrics shared by every searchable menu (before UI scaling).
pub(crate) const SEARCH_MENU_ROW_HEIGHT_PX: f32 = 24.0;
pub(crate) const SEARCH_MENU_INPUT_HEIGHT_PX: f32 = 33.0;
pub(crate) const SEARCH_MENU_MAX_VISIBLE_ROWS: usize = 12;
const SEARCH_MENU_LIST_PADDING_BOTTOM_PX: f32 = 4.0;
const SEARCH_MENU_RADIUS_PX: f32 = 8.0;

/// Content and state of a single searchable-menu row.
#[derive(Default)]
pub(crate) struct SearchMenuRow {
    pub label: SharedString,
    /// Optional leading icon (terminal command choices, semantic options).
    pub icon_path: Option<&'static str>,
    /// Trailing secondary text (e.g. "Current" branch tag, option description).
    pub detail: Option<SharedString>,
    /// Keyboard-selected row: selected background + accent label.
    pub selected: bool,
    /// Row representing the currently applied value: accent label.
    pub checked: bool,
    /// Trailing "▸" marker for rows that open a second-level menu.
    pub submenu: bool,
    /// Leading "‹" marker for back rows inside a second-level menu.
    pub back: bool,
    /// Render the label using this font family (font preview rows).
    pub preview_font: Option<SharedString>,
}

impl SearchMenuRow {
    pub(crate) fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            ..Self::default()
        }
    }

    pub(crate) fn detail(mut self, detail: Option<impl Into<SharedString>>) -> Self {
        self.detail = detail.map(Into::into);
        self
    }

    pub(crate) fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    pub(crate) fn icon(mut self, icon_path: Option<&'static str>) -> Self {
        self.icon_path = icon_path;
        self
    }

    pub(crate) fn preview_font(mut self, preview_font: Option<impl Into<SharedString>>) -> Self {
        self.preview_font = preview_font.map(Into::into);
        self
    }
}

/// Anchored placement configuration for a searchable menu.
pub(crate) struct SearchMenuConfig {
    /// Element id of the panel (results list id is derived from it).
    pub id: &'static str,
    /// Anchor bounds in the same coordinate space as the viewport size.
    pub anchor: Bounds<Pixels>,
    /// Coordinate space the menu is positioned in (window or panel).
    pub viewport_width: f32,
    pub viewport_height: f32,
    /// Desired panel width (already UI-scaled).
    pub desired_width: f32,
    /// Match the anchor width instead of `desired_width` (settings pickers).
    pub match_anchor_width: bool,
    /// Total number of rows, used to size the panel before clamping.
    pub row_count: usize,
}

/// Hook invoked on scroll to extend virtualized row limits.
pub(crate) type SearchMenuScrollExtend = Box<dyn Fn(&mut EditorView, &mut Context<EditorView>)>;

impl EditorView {
    /// Text-input styles shared by every searchable menu's query field.
    pub(crate) fn search_menu_text_input_styles(&self) -> crate::style::TextInputStyles {
        let mut styles = self.styles.text_input.clone();
        let menu = &self.styles.quick_open;
        styles.text_input_bg = menu.quick_open_input_bg;
        styles.text_input_hover_bg = Some(menu.quick_open_input_focus_bg);
        styles.text_input_focus_bg = menu.quick_open_input_focus_bg;
        styles.text_input_text_color = menu.quick_open_input_text_color;
        styles.text_input_placeholder_color = menu.quick_open_result_secondary_text_color;
        styles.text_input_cursor_color = menu.quick_open_input_text_color;
        styles.text_input_border_width_px = 0.0;
        styles.text_input_height_px = self.scaled(24.0);
        styles.text_input_padding_left_px = menu.quick_open_input_text_padding_left_px;
        styles.text_input_padding_right_px = menu.quick_open_input_text_padding_left_px;
        styles.text_input_font_size_px = self.scaled(menu.quick_open_result_name_font_size_px);
        styles.text_input_cursor_height_ratio = menu.quick_open_input_cursor_height_ratio;
        styles.text_input_cursor_height_adjust_px = menu.quick_open_input_cursor_height_adjust_px;
        styles
    }

    /// The standard "No matches" row shown when a query filters everything out.
    pub(crate) fn render_search_menu_empty_row(&self) -> AnyElement {
        let styles = &self.styles.quick_open;
        div()
            .w_full()
            .h(px(self.scaled(styles.quick_open_empty_height_px)))
            .flex()
            .items_center()
            .justify_center()
            .text_size(px(self.scaled(styles.quick_open_empty_font_size_px)))
            .text_color(styles.quick_open_empty_text_color)
            .child(crate::i18n::no_matches_label(self.ui_language))
            .into_any_element()
    }

    /// A single row of a searchable menu.
    pub(crate) fn render_search_menu_row(
        &self,
        id: impl Into<ElementId>,
        row: SearchMenuRow,
        on_click: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let styles = &self.styles.quick_open;
        let selected_bg = styles.quick_open_result_selected_bg;
        let hover_bg = styles.quick_open_result_hover_bg;
        let text_secondary = styles.quick_open_result_secondary_text_color;
        let name_font = self.scaled(styles.quick_open_result_name_font_size_px);
        let detail_font = self.scaled(styles.quick_open_result_path_font_size_px);
        let label_color = if row.selected || row.checked {
            self.theme.text_accent
        } else {
            styles.quick_open_result_text_color
        };

        div()
            .id(id)
            .h(px(self.scaled(SEARCH_MENU_ROW_HEIGHT_PX)))
            .px(px(self.scaled(styles.quick_open_result_padding_x_px)))
            .flex()
            .items_center()
            .gap(px(self.scaled(6.0)))
            .cursor(CursorStyle::Arrow)
            .when(row.selected || row.checked, |this| this.bg(selected_bg))
            .hover(move |style| style.bg(hover_bg))
            .when(row.back, |this| {
                this.child(
                    div()
                        .flex_none()
                        .text_size(px(detail_font))
                        .text_color(text_secondary)
                        .child("‹"),
                )
            })
            .when_some(row.icon_path, |this, icon_path| {
                this.child(
                    svg()
                        .path(icon_path)
                        .size(px(self.scaled(14.0)))
                        .flex_none()
                        .text_color(label_color),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .truncate()
                    .whitespace_nowrap()
                    .text_size(px(name_font))
                    .text_color(label_color)
                    .when_some(row.preview_font, |this, font| this.font_family(font))
                    .child(row.label),
            )
            .when_some(row.detail, |this, detail| {
                this.child(
                    div()
                        .flex_none()
                        .max_w(px(self.scaled(140.0)))
                        .overflow_hidden()
                        .truncate()
                        .whitespace_nowrap()
                        .text_size(px(detail_font))
                        .text_color(text_secondary)
                        .child(detail),
                )
            })
            .when(row.submenu, |this| {
                this.child(
                    div()
                        .flex_none()
                        .text_size(px(detail_font))
                        .text_color(text_secondary)
                        .child("▸"),
                )
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    on_click(this, window, cx);
                }),
            )
            .into_any_element()
    }

    /// The anchored menu panel: search input on top of a scrollable row list.
    /// Positioned above or below the anchor by `anchored_popup_placement`.
    pub(crate) fn render_search_menu_panel(
        &self,
        config: &SearchMenuConfig,
        editor: Option<Entity<crate::text_input::TextInput>>,
        scroll_handle: Option<&ScrollHandle>,
        on_scroll_extend: Option<SearchMenuScrollExtend>,
        rows: Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let styles = &self.styles.quick_open;
        let bg = styles.quick_open_panel_bg;
        let border = styles.quick_open_panel_border_color;
        let input_h = self.scaled(SEARCH_MENU_INPUT_HEIGHT_PX);
        let row_h = self.scaled(SEARCH_MENU_ROW_HEIGHT_PX);
        let visible_rows = config.row_count.clamp(1, SEARCH_MENU_MAX_VISIBLE_ROWS);
        let desired_h =
            input_h + visible_rows as f32 * row_h + self.scaled(SEARCH_MENU_LIST_PADDING_BOTTOM_PX);
        let placement = helpers::anchored_popup_placement(
            config.viewport_width,
            config.viewport_height,
            config.anchor,
            config.desired_width,
            desired_h,
            config.match_anchor_width,
            self.scaled(self.styles.anchored_popup_gap_px),
            self.scaled(self.styles.anchored_popup_margin_px),
        );
        let max_results_h = (placement.max_height - input_h).max(row_h);

        let rows = if rows.is_empty() {
            vec![self.render_search_menu_empty_row()]
        } else {
            rows
        };

        let panel = div()
            .id(config.id)
            .absolute()
            .left(px(placement.left))
            .w(px(placement.width))
            .max_h(px(placement.max_height))
            .bg(bg)
            .border(px(self.ui_border_width(1.0)))
            .border_color(border)
            .rounded(px(self.scaled(SEARCH_MENU_RADIUS_PX)))
            .shadow_md()
            .flex()
            .flex_col()
            .overflow_hidden()
            .cursor(CursorStyle::Arrow)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_, _, window, cx| {
                    cx.stop_propagation();
                    window.prevent_default();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|_, _, window, cx| {
                    cx.stop_propagation();
                    window.prevent_default();
                }),
            )
            .child(
                div()
                    .w_full()
                    .flex_none()
                    .px(px(self.scaled(styles.quick_open_input_padding_x_px)))
                    .py(px(self.scaled(4.0)))
                    .border_b(px(self.ui_border_width(1.0)))
                    .border_color(border)
                    .when_some(editor, |this, editor| this.child(editor)),
            )
            .child(
                div()
                    .id(SharedString::from(format!("{}-results", config.id)))
                    .w_full()
                    .max_h(px(max_results_h))
                    .overflow_y_scroll()
                    .pb(px(self.scaled(SEARCH_MENU_LIST_PADDING_BOTTOM_PX)))
                    .when_some(scroll_handle, |this, handle| this.track_scroll(handle))
                    .when_some(on_scroll_extend, |this, extend| {
                        this.on_scroll_wheel(cx.listener(move |this, _, _, cx| {
                            extend(this, cx);
                        }))
                    })
                    .children(rows),
            );
        match placement.vertical {
            helpers::AnchoredPopupVertical::Above { bottom } => panel.bottom(px(bottom)),
            helpers::AnchoredPopupVertical::Below { top } => panel.top(px(top)),
        }
        .into_any_element()
    }

    /// A searchable menu wrapped in a full-viewport occluded backdrop that
    /// closes the menu on any outside click.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn render_search_menu_overlay(
        &self,
        config: &SearchMenuConfig,
        editor: Option<Entity<crate::text_input::TextInput>>,
        scroll_handle: Option<&ScrollHandle>,
        on_scroll_extend: Option<SearchMenuScrollExtend>,
        rows: Vec<AnyElement>,
        on_close: impl Fn(&mut Self, &mut Window, &mut Context<Self>) + Clone + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let panel = self.render_search_menu_panel(
            config,
            editor,
            scroll_handle,
            on_scroll_extend,
            rows,
            cx,
        );
        let close_left = on_close.clone();
        div()
            .occlude()
            .absolute()
            .left(px(0.0))
            .top(px(0.0))
            .w_full()
            .h_full()
            .child(
                div()
                    .absolute()
                    .left(px(0.0))
                    .top(px(0.0))
                    .w_full()
                    .h_full()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            close_left(this, window, cx);
                        }),
                    )
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            on_close(this, window, cx);
                        }),
                    ),
            )
            .child(panel)
            .into_any_element()
    }
}
