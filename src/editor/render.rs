use gpui::{
    AnyElement, App, Bounds, CursorStyle, MouseButton, MouseDownEvent, MouseMoveEvent, Render,
    SharedString, Window, deferred, div, img, prelude::*, px,
};

use super::tooltip::{TooltipState, toolbar_anchor_bounds};
use super::*;

const TOOLBAR_SIDEBAR_TOOLTIP: (&str, Option<&str>) = ("显示或隐藏边栏", Some("⌘\\"));
const TOOLBAR_ALIGN_TOOLTIP: (&str, Option<&str>) = ("切换左对齐", Some("⌘⇧L"));
const TOOLBAR_THEME_TOOLTIP: (&str, Option<&str>) = ("切换浅色或深色主题", Some("⌘⌥\\"));

impl EditorView {
    fn render_toolbar_button(
        &self,
        id: &'static str,
        button_size: f32,
        icon_size: f32,
        hover_opacity: f32,
        icon_path: &'static str,
        anchor_bounds: Bounds<gpui::Pixels>,
        tooltip_title: &'static str,
        tooltip_subtitle: Option<&'static str>,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let editor = cx.entity().clone();
        let title = SharedString::from(tooltip_title);
        let subtitle = tooltip_subtitle.map(SharedString::from);

        div()
            .id(id)
            .w(px(button_size))
            .h(px(button_size))
            .flex()
            .items_center()
            .justify_center()
            .cursor(CursorStyle::Arrow)
            .hover(move |style| style.opacity(hover_opacity))
            .on_hover({
                let editor = editor.clone();
                let title = title.clone();
                let subtitle = subtitle.clone();
                move |hovering: &bool, _: &mut Window, cx: &mut App| {
                    let _ = editor.update(cx, |this, cx| {
                        if *hovering {
                            this.begin_tooltip_hover(
                                TooltipState {
                                    id,
                                    title: title.clone(),
                                    subtitle: subtitle.clone(),
                                    anchor_bounds,
                                },
                                cx,
                            );
                        } else {
                            this.end_tooltip_hover(id, cx);
                        }
                    });
                }
            })
            .child(
                img(helpers::resolve_asset_image_path(icon_path))
                    .w(px(icon_size))
                    .h(px(icon_size)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.clear_tooltip(cx);
                    cx.stop_propagation();
                    on_click(this, cx);
                }),
            )
            .into_any_element()
    }
}

impl Render for EditorView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.pending_editor_focus {
            window.focus(&self.focus_handle(cx));
            self.pending_editor_focus = false;
        }
        let resizing = self.sidebar_resizing || self.utility_panel_resizing;
        let show_editor_toolbar =
            self.tabs.len() > 0 && !self.active_tab_is_settings() && !self.active_tab_is_binary();
        let ws = &self.styles.window;
        // Store window bounds for menu positioning
        let wb = window.bounds();
        self.last_window_width = f32::from(wb.size.width);
        self.last_window_height = f32::from(wb.size.height);
        div()
            .id("editor-root")
            .key_context("MarkdownEditor")
            .track_focus(&self.focus_handle(cx))
            .relative()
            .size_full()
            .flex()
            .flex_row()
            .overflow_hidden()
            .font_family(SharedString::from(self.styles.ui_font_family))
            .text_size(px(self.scaled(self.styles.ui_font_size_px)))
            .bg(ws.window_bg)
            .border(px(ws.window_border_width_px))
            .border_color(ws.window_border_color)
            .rounded(px(ws.window_radius_px))
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
            .on_action(cx.listener(Self::on_back_tab))
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
            .on_action(cx.listener(Self::on_toggle_sidebar))
            .on_action(cx.listener(Self::on_toggle_theme))
            .on_action(cx.listener(Self::on_toggle_editor_left_align))
            .on_action(cx.listener(Self::on_select_line))
            .on_action(cx.listener(Self::on_insert_line_below))
            .on_action(cx.listener(Self::on_insert_line_above))
            .on_action(cx.listener(Self::on_duplicate_line))
            .on_action(cx.listener(Self::on_delete_line))
            .on_action(cx.listener(Self::on_move_line_up))
            .on_action(cx.listener(Self::on_move_line_down))
            .on_action(cx.listener(Self::on_save))
            .on_action(cx.listener(Self::on_new_tab))
            .on_action(cx.listener(Self::on_close_active_tab))
            .on_action(cx.listener(Self::on_reopen_closed_tab))
            .on_action(cx.listener(Self::on_activate_previous_tab))
            .on_action(cx.listener(Self::on_activate_next_tab))
            .on_action(cx.listener(Self::on_find))
            .on_action(cx.listener(Self::on_find_next))
            .on_action(cx.listener(Self::on_find_previous))
            .on_action(cx.listener(Self::on_dismiss_search))
            .on_action(cx.listener(Self::on_toggle_replace))
            .on_action(cx.listener(Self::on_replace_next))
            .on_action(cx.listener(Self::on_replace_all))
            .on_action(cx.listener(Self::on_find_in_files))
            .on_action(cx.listener(Self::on_quick_open))
            .on_action(cx.listener(Self::on_escape))
            .on_action(cx.listener(Self::on_toggle_outline))
            .on_action(cx.listener(Self::on_toggle_info_panel))
            .on_action(cx.listener(Self::on_toggle_git_history))
            .on_action(cx.listener(Self::on_toggle_theme_debugger))
            .on_action(cx.listener(Self::on_add_cursor_above))
            .on_action(cx.listener(Self::on_add_cursor_below))
            .on_action(cx.listener(Self::on_select_next_occurrence))
            .on_action(cx.listener(Self::on_select_all_occurrences))
            .when(self.sidebar_visible, |this| {
                let sidebar_w = self.sidebar_width();
                let ss = &self.styles.sidebar;
                let hover_indicator_color = ss.sidebar_resize_active_indicator_color;
                let hover_indicator_width =
                    self.scaled(ss.sidebar_resize_active_indicator_width_px);
                let handle_width = self.scaled(ss.sidebar_resize_handle_width_px);
                this.child(self.render_sidebar(cx))
                    // Transparent sensing area for resize drag
                    .child(
                        div()
                            .id("sidebar-resize-handle")
                            .absolute()
                            .left(px(sidebar_w - handle_width / 2.0))
                            .top_0()
                            .w(px(handle_width))
                            .h_full()
                            .cursor(CursorStyle::ResizeLeftRight)
                            .hover(move |style| {
                                style
                                    .border_l(px(hover_indicator_width))
                                    .border_color(hover_indicator_color)
                            })
                            .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                                if this.sidebar_resizing {
                                    let new_width = f32::from(event.position.x);
                                    let min_w = this
                                        .scaled(this.styles.sidebar.sidebar_resize_min_width_px);
                                    let max_w = this
                                        .scaled(this.styles.sidebar.sidebar_resize_max_width_px);
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
                            ),
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
                    .bg(self.styles.editor.editor_bg)
                    .when(!self.tabs.is_empty(), |this| {
                        this.child(self.render_tab_bar(cx))
                    })
                    .when(self.tabs.is_empty(), |this| {
                        // Empty state: no tabs open
                        let bg_hover = self.styles.sidebar.sidebar_item_hover_bg;
                        let btn_color = self.styles.sidebar.sidebar_empty_btn_text_color;
                        let border_color = self.styles.sidebar.sidebar_empty_btn_border_color;
                        this.child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                        if event.click_count >= 2 {
                                            this.create_scratch_tab(cx);
                                        }
                                    }),
                                )
                                .child(
                                    div()
                                        .id("empty-workspace-btn")
                                        .px(px(20.0))
                                        .py(px(8.0))
                                        .border(px(1.5))
                                        .border_dashed()
                                        .border_color(border_color)
                                        .rounded_full()
                                        .text_size(px(self.scaled(
                                            self.styles.sidebar.sidebar_empty_btn_font_size_px,
                                        )))
                                        .text_color(btn_color)
                                        .cursor(CursorStyle::Arrow)
                                        .hover(move |s| s.bg(bg_hover))
                                        .child("新建文件...")
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                                if event.click_count >= 2 {
                                                    this.create_scratch_tab(cx);
                                                }
                                            }),
                                        ),
                                ),
                        )
                    })
                    .when(
                        !self.tabs.is_empty() && self.active_tab_is_settings(),
                        |this| this.child(self.render_settings_panel(window, cx)),
                    )
                    .when(
                        !self.tabs.is_empty() && !self.active_tab_is_settings(),
                        |this| {
                            let sidebar_width = if self.sidebar_visible {
                                self.sidebar_width()
                            } else {
                                0.0
                            };
                            let trailing_width = if !self.tabs.is_empty() {
                                let theme_debugger_width = if self.theme_debugger_visible {
                                    self.theme_debugger_width()
                                } else {
                                    0.0
                                };
                                let utility_panel_width = if self.utility_panel.is_some() {
                                    self.utility_panel_width()
                                } else {
                                    0.0
                                };
                                theme_debugger_width + utility_panel_width
                            } else {
                                0.0
                            };
                            let editor_pane_width =
                                (self.last_window_width - sidebar_width - trailing_width).max(1.0);
                            this.child(
                                div()
                                    .id("editor-pane")
                                    .relative()
                                    .flex_1()
                                    .overflow_hidden()
                                    .cursor(
                                        if self.active_tab_is_image()
                                            || self.active_tab_is_binary()
                                            || self.active_tab_is_diff()
                                            || self.hovering_checkbox
                                            || self.hovering_image_edit_line.is_some()
                                        {
                                            CursorStyle::Arrow
                                        } else {
                                            CursorStyle::IBeam
                                        },
                                    )
                                    .when(!self.active_tab_is_diff(), |this| {
                                        this.on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, event, window, cx| {
                                                this.focused_sidebar_path = None;
                                                this.on_mouse_down(event, window, cx);
                                            }),
                                        )
                                        .on_mouse_down(
                                            MouseButton::Right,
                                            cx.listener(Self::on_editor_context_menu),
                                        )
                                        .on_mouse_up(
                                            MouseButton::Left,
                                            cx.listener(Self::on_mouse_up),
                                        )
                                        .on_mouse_up_out(
                                            MouseButton::Left,
                                            cx.listener(Self::on_mouse_up),
                                        )
                                        .on_mouse_move(cx.listener(Self::on_mouse_move))
                                        .on_scroll_wheel(cx.listener(Self::on_scroll))
                                    })
                                    .drag_over::<DraggedSidebarItems>(move |style, _, _, _| style)
                                    .on_drop(cx.listener(
                                        |this, dragged: &DraggedSidebarItems, _, cx| {
                                            this.clear_sidebar_drag_state(cx);
                                            this.focused_sidebar_path = None;
                                            this.insert_sidebar_items_as_wiki_links(
                                                &dragged.paths,
                                                cx,
                                            );
                                        },
                                    ))
                                    .child(if self.active_tab_is_image() {
                                        image_view::ImageViewElement {
                                            editor: cx.entity().clone(),
                                        }
                                        .into_any_element()
                                    } else if self.active_tab_is_diff() {
                                        self.render_git_diff_view(cx)
                                    } else if self.active_tab_is_binary() {
                                        self.render_binary_placeholder(cx)
                                    } else {
                                        element::EditorElement {
                                            editor: cx.entity().clone(),
                                        }
                                        .into_any_element()
                                    })
                                    .child({
                                        let primary_hover =
                                            self.styles.toolbar.toolbar_primary_hover_opacity;
                                        let secondary_hover =
                                            self.styles.toolbar.toolbar_secondary_hover_opacity;
                                        let toolbar_top =
                                            self.scaled(self.styles.toolbar.toolbar_offset_top_px);
                                        let toolbar_left =
                                            self.scaled(self.styles.toolbar.toolbar_offset_left_px);
                                        let primary_btn_size = self.scaled(
                                            self.styles.toolbar.toolbar_primary_btn_size_px,
                                        );
                                        let secondary_btn_size = self.scaled(
                                            self.styles.toolbar.toolbar_secondary_btn_size_px,
                                        );
                                        let toolbar_gap =
                                            self.scaled(self.styles.toolbar.toolbar_gap_px);
                                        let primary_icon_size = self.scaled(
                                            self.styles.toolbar.toolbar_primary_icon_size_px,
                                        );
                                        let secondary_icon_size = self.scaled(
                                            self.styles.toolbar.toolbar_secondary_icon_size_px,
                                        );
                                        let sidebar_anchor = toolbar_anchor_bounds(
                                            toolbar_left,
                                            toolbar_top,
                                            primary_btn_size,
                                        );
                                        let align_anchor = toolbar_anchor_bounds(
                                            toolbar_left + primary_btn_size + toolbar_gap,
                                            toolbar_top,
                                            secondary_btn_size,
                                        );
                                        self.sync_tooltip_anchor(
                                            "toolbar-sidebar-toggle",
                                            sidebar_anchor,
                                        );
                                        self.sync_tooltip_anchor(
                                            "toolbar-align-toggle",
                                            align_anchor,
                                        );
                                        let toolbar_opacity = if show_editor_toolbar {
                                            self.toolbar_opacity
                                        } else {
                                            self.styles.toolbar.toolbar_hidden_opacity
                                        };
                                        div()
                                            .absolute()
                                            .left(px(toolbar_left))
                                            .top(px(toolbar_top))
                                            .flex()
                                            .items_center()
                                            .gap(px(toolbar_gap))
                                            .opacity(toolbar_opacity)
                                            .child(self.render_toolbar_button(
                                                "toolbar-sidebar-toggle",
                                                primary_btn_size,
                                                primary_icon_size,
                                                primary_hover,
                                                SIDEBAR_TOGGLE_SVG_PATH,
                                                sidebar_anchor,
                                                TOOLBAR_SIDEBAR_TOOLTIP.0,
                                                TOOLBAR_SIDEBAR_TOOLTIP.1,
                                                cx,
                                                |this, cx| {
                                                    if this.workspaces.is_empty() {
                                                        this.pick_workspace_folder(cx);
                                                    } else {
                                                        this.toggle_sidebar_visibility(cx);
                                                    }
                                                },
                                            ))
                                            .child(self.render_toolbar_button(
                                                "toolbar-align-toggle",
                                                secondary_btn_size,
                                                secondary_icon_size,
                                                secondary_hover,
                                                EDITOR_ALIGN_LEFT_SVG_PATH,
                                                align_anchor,
                                                TOOLBAR_ALIGN_TOOLTIP.0,
                                                TOOLBAR_ALIGN_TOOLTIP.1,
                                                cx,
                                                |this, cx| {
                                                    this.toggle_editor_alignment(cx);
                                                },
                                            ))
                                    })
                                    .child({
                                        let secondary_hover =
                                            self.styles.toolbar.toolbar_secondary_hover_opacity;
                                        let toolbar_top =
                                            self.scaled(self.styles.toolbar.toolbar_offset_top_px);
                                        let toolbar_right =
                                            self.scaled(self.styles.toolbar.toolbar_offset_left_px);
                                        let secondary_btn_size = self.scaled(
                                            self.styles.toolbar.toolbar_secondary_btn_size_px,
                                        );
                                        let secondary_icon_size = self.scaled(
                                            self.styles.toolbar.toolbar_secondary_icon_size_px,
                                        );
                                        let theme_x =
                                            editor_pane_width - toolbar_right - secondary_btn_size;
                                        let theme_anchor = toolbar_anchor_bounds(
                                            theme_x,
                                            toolbar_top,
                                            secondary_btn_size,
                                        );
                                        self.sync_tooltip_anchor(
                                            "toolbar-theme-toggle",
                                            theme_anchor,
                                        );
                                        let toolbar_opacity = if show_editor_toolbar {
                                            self.toolbar_opacity
                                        } else {
                                            self.styles.toolbar.toolbar_hidden_opacity
                                        };
                                        div()
                                            .absolute()
                                            .right(px(toolbar_right))
                                            .top(px(toolbar_top))
                                            .flex()
                                            .items_center()
                                            .opacity(toolbar_opacity)
                                            .child(self.render_toolbar_button(
                                                "toolbar-theme-toggle",
                                                secondary_btn_size,
                                                secondary_icon_size,
                                                secondary_hover,
                                                if self.theme_kind.is_dark() {
                                                    THEME_DARK_SVG_PATH
                                                } else {
                                                    THEME_LIGHT_SVG_PATH
                                                },
                                                theme_anchor,
                                                TOOLBAR_THEME_TOOLTIP.0,
                                                TOOLBAR_THEME_TOOLTIP.1,
                                                cx,
                                                |this, cx| {
                                                    this.toggle_theme(cx);
                                                },
                                            ))
                                    })
                                    .when_some(
                                        self.render_tooltip_overlay(editor_pane_width),
                                        |this, tooltip| this.child(tooltip),
                                    )
                                    .when(
                                        self.search_visible
                                            && self.active_tab_supports_text_search(),
                                        |this: gpui::Stateful<gpui::Div>| {
                                            this.child(self.render_search_bar(cx))
                                        },
                                    ),
                            )
                        },
                    ),
            )
            .when(
                (self.theme_debugger_visible || self.utility_panel.is_some()) && !self.tabs.is_empty(),
                |this| {
                    this.child(
                        div()
                            .id("right-panels")
                            .h_full()
                            .flex()
                            .flex_row()
                            .when(self.theme_debugger_visible, |this| {
                                this.child(self.render_theme_debugger(cx))
                            })
                            .when_some(self.utility_panel, |this, _panel| {
                                let ups = &self.styles.utility_panel;
                                let panel_w = self.utility_panel_width();
                                let hover_indicator_color =
                                    ups.utility_panel_resize_active_indicator_color;
                                let hover_indicator_width = self.scaled(
                                    ups.utility_panel_resize_active_indicator_width_px,
                                );
                                let handle_width =
                                    self.scaled(ups.utility_panel_resize_handle_width_px);
                                this.child(
                                    div()
                                        .id("utility-panel-wrap")
                                        .relative()
                                        .h_full()
                                        .w(px(panel_w))
                                        .flex_shrink_0()
                                        .child(self.render_utility_panel(cx))
                                        .child(
                                            div()
                                                .id("utility-panel-resize-handle")
                                                .absolute()
                                                .left(px(-handle_width / 2.0))
                                                .top_0()
                                                .w(px(handle_width))
                                                .h_full()
                                                .cursor(CursorStyle::ResizeLeftRight)
                                                .hover(move |style| {
                                                    style
                                                        .border_l(px(hover_indicator_width))
                                                        .border_color(hover_indicator_color)
                                                })
                                                .on_mouse_move(cx.listener(
                                                    |this, event: &MouseMoveEvent, window, cx| {
                                                        if this.utility_panel_resizing {
                                                            let window_width = f32::from(
                                                                window.bounds().size.width,
                                                            );
                                                            let min_w = this.scaled(
                                                                this.styles
                                                                    .utility_panel
                                                                    .utility_panel_resize_min_width_px,
                                                            );
                                                            let max_w = this.scaled(
                                                                this.styles
                                                                    .utility_panel
                                                                    .utility_panel_resize_max_width_px,
                                                            );
                                                            let new_width =
                                                                window_width - f32::from(event.position.x);
                                                            this.utility_panel_custom_width = Some(
                                                                new_width.clamp(min_w, max_w),
                                                            );
                                                            cx.notify();
                                                        }
                                                    },
                                                ))
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(
                                                        |this, _: &MouseDownEvent, _, cx| {
                                                            this.utility_panel_resizing = true;
                                                            cx.notify();
                                                        },
                                                    ),
                                                )
                                                .on_mouse_up(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.utility_panel_resizing = false;
                                                        cx.notify();
                                                    }),
                                                )
                                                .on_mouse_up_out(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.utility_panel_resizing = false;
                                                        cx.notify();
                                                    }),
                                                ),
                                        ),
                                )
                            }),
                    )
                },
            )
            .when(resizing, |this| {
                let indicator_line = if self.sidebar_resizing {
                    let iw =
                        self.scaled(self.styles.sidebar.sidebar_resize_active_indicator_width_px);
                    let ic = self.styles.sidebar.sidebar_resize_active_indicator_color;
                    let x = self.sidebar_width() - iw;
                    Some(
                        div()
                            .absolute()
                            .left(px(x))
                            .top_0()
                            .w(px(iw))
                            .h_full()
                            .bg(ic)
                            .into_any_element(),
                    )
                } else {
                    None
                };
                this.child(deferred(
                    div()
                        .occlude()
                        .absolute()
                        .left(px(0.0))
                        .top(px(0.0))
                        .w_full()
                        .h_full()
                        .cursor(CursorStyle::ResizeLeftRight)
                        .children(indicator_line)
                        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _window, cx| {
                            if this.sidebar_resizing {
                                let new_width = f32::from(event.position.x);
                                let min_w =
                                    this.scaled(this.styles.sidebar.sidebar_resize_min_width_px);
                                let max_w =
                                    this.scaled(this.styles.sidebar.sidebar_resize_max_width_px);
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
                        .on_mouse_down(MouseButton::Left, cx.listener(|_this, _, _, _cx| {})),
                ))
            })
            .when(self.context_menu.is_some(), |this| {
                this.child(deferred(
                    div()
                        .occlude()
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
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(|this, _, _, cx| {
                                this.close_context_menu(cx);
                            }),
                        )
                        .child(self.render_context_menu(cx)),
                ))
            })
            .when(self.settings_font_picker.is_some(), |this| {
                if let Some(picker) = &self.settings_font_picker {
                    this.child(deferred(self.render_font_picker_overlay(picker, cx)))
                } else {
                    this
                }
            })
            .when(self.quick_open_visible, |this| {
                this.child(deferred(self.render_quick_open_overlay(cx)))
            })
            .when(self.dialog.is_some(), |this| {
                this.child(self.render_dialog(cx))
            })
    }
}

impl EditorView {
    fn render_binary_placeholder(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        let path = self
            .active_tab_index()
            .and_then(|index| self.tabs.get(index))
            .and_then(|tab| tab.path.clone());

        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(self.scaled(10.0)))
            .text_color(self.theme.text_secondary)
            .child(
                div()
                    .text_size(px(self.scaled(self.styles.ui_font_size_px + 1.0)))
                    .text_color(self.theme.text_primary)
                    .child("该文件不是文本文件"),
            )
            .child(
                div()
                    .text_size(px(self.scaled(self.styles.ui_font_size_px - 1.0)))
                    .child(
                        path.as_ref()
                            .map(|value| value.display().to_string())
                            .unwrap_or_else(|| "当前文件".to_string()),
                    ),
            )
            .child(
                div()
                    .px(px(self.scaled(12.0)))
                    .py(px(self.scaled(8.0)))
                    .rounded(px(self.scaled(8.0)))
                    .bg(self.theme.bg_hover)
                    .text_color(self.theme.text_primary)
                    .cursor(CursorStyle::PointingHand)
                    .child("打开该非文本文件")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            if let Some(path) = this
                                .active_tab_index()
                                .and_then(|index| this.tabs.get(index))
                                .and_then(|tab| tab.path.clone())
                            {
                                let _ = std::process::Command::new("open").arg(path).spawn();
                            }
                            cx.notify();
                        }),
                    ),
            )
            .into_any_element()
    }
}
