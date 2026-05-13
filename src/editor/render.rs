use gpui::{
    AnyElement, App, Bounds, CursorStyle, ExternalPaths, MouseButton, MouseDownEvent,
    MouseMoveEvent, Render, SharedString, Window, deferred, div, img, prelude::*, px,
};

use super::tooltip::{TooltipState, toolbar_anchor_bounds};
use super::*;

impl EditorView {
    fn render_right_panels(&mut self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("right-panels")
            .h_full()
            .flex()
            .flex_row()
            .when(self.should_render_theme_debugger_panel(), |this| {
                this.child(
                    div()
                        .id("theme-debugger-wrap")
                        .h_full()
                        .w(px(self.theme_debugger_width()))
                        .overflow_hidden()
                        .flex_shrink_0()
                        .child(self.render_theme_debugger(cx)),
                )
            })
            .when(self.should_render_utility_panel(), |this| {
                let ups = &self.styles.utility_panel;
                let panel_w = self.utility_panel_width();
                let panel_layout_w = self.utility_panel_layout_width();
                let hover_indicator_color = ups.utility_panel_resize_active_indicator_color;
                let hover_indicator_width =
                    self.scaled(ups.utility_panel_resize_active_indicator_width_px);
                let handle_width = self.scaled(ups.utility_panel_resize_handle_width_px);
                this.child(
                    div()
                        .id("utility-panel-wrap")
                        .relative()
                        .h_full()
                        .w(px(panel_w))
                        .overflow_hidden()
                        .flex_shrink_0()
                        .child(
                            div()
                                .h_full()
                                .w(px(panel_layout_w))
                                .child(self.render_utility_panel(cx)),
                        )
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
                                            let window_width =
                                                f32::from(window.bounds().size.width);
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
                                            let new_width = new_width.clamp(min_w, max_w);
                                            this.utility_panel_custom_width = Some(new_width);
                                            this.set_utility_panel_width_immediately(new_width);
                                            cx.notify();
                                        }
                                    },
                                ))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                        this.utility_panel_resizing = true;
                                        cx.notify();
                                    }),
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
            })
            .into_any_element()
    }

    fn render_toolbar_button(
        &self,
        id: &'static str,
        button_width: f32,
        button_height: f32,
        icon_size: f32,
        base_opacity: f32,
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
        let is_hovered = self.hovered_toolbar_button == Some(id);

        div()
            .id(id)
            .w(px(button_width))
            .h(px(button_height))
            .flex()
            .items_center()
            .justify_center()
            .bg(gpui::transparent_black())
            .cursor(CursorStyle::Arrow)
            .on_hover({
                let editor = editor.clone();
                let title = title.clone();
                let subtitle = subtitle.clone();
                move |hovering: &bool, _: &mut Window, cx: &mut App| {
                    let _ = editor.update(cx, |this, cx| {
                        let next_hover = if *hovering { Some(id) } else { None };
                        if this.hovered_toolbar_button == Some(id) || next_hover.is_some() {
                            if this.hovered_toolbar_button != next_hover {
                                this.hovered_toolbar_button = next_hover;
                                cx.notify();
                            }
                        }
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
                    .h(px(icon_size))
                    .opacity(if is_hovered {
                        hover_opacity
                    } else {
                        base_opacity
                    }),
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
        if self.pending_chat_input_focus {
            window.focus(&self.chat_panel_focus_handle);
            self.pending_chat_input_focus = false;
        }
        let resizing = self.sidebar_resizing || self.utility_panel_resizing;
        let toolbar_base_visible =
            self.tabs.len() > 0 && !self.active_tab_is_settings() && !self.active_tab_is_binary();
        let show_editor_toolbar = self.should_show_editor_toolbar(toolbar_base_visible);
        let render_editor_toolbar = toolbar_base_visible
            && (show_editor_toolbar
                || self.toolbar_opacity_task.is_some()
                || self.toolbar_opacity > self.styles.toolbar.toolbar_hidden_opacity + 0.01);
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
            .when(self.should_render_sidebar_panel(), |this| {
                let sidebar_w = self.sidebar_width();
                let sidebar_layout_w = self.sidebar_layout_width();
                let ss = &self.styles.sidebar;
                let hover_indicator_color = ss.sidebar_resize_active_indicator_color;
                let hover_indicator_width =
                    self.scaled(ss.sidebar_resize_active_indicator_width_px);
                let handle_width = self.scaled(ss.sidebar_resize_handle_width_px);
                this.child(
                    div()
                        .id("sidebar-wrap")
                        .w(px(sidebar_w))
                        .h_full()
                        .overflow_hidden()
                        .flex_none()
                        .child(
                            div()
                                .h_full()
                                .w(px(sidebar_layout_w))
                                .child(self.render_sidebar(cx)),
                        ),
                )
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
                                    let new_width = new_width.clamp(min_w, max_w);
                                    this.sidebar_custom_width = Some(new_width);
                                    this.set_sidebar_width_immediately(new_width);
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
                        let file_drop_bg = self.styles.editor.editor_file_drop_bg;
                        this.child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .items_center()
                                .justify_center()
                                .drag_over::<ExternalPaths>(move |style, _, _, _| {
                                    style.bg(file_drop_bg)
                                })
                                .on_drop(cx.listener(
                                    |this, paths: &ExternalPaths, _, cx| {
                                        this.open_external_paths_in_tabs(paths.paths(), cx);
                                    },
                                ))
                                .cursor(CursorStyle::Arrow)
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
                                        .child(self.t(crate::i18n::TextKey::NewFile))
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, _: &MouseDownEvent, _, cx| {
                                                this.create_scratch_tab(cx);
                                            }),
                                        ),
                                )
                        )
                    })
                    .when(
                        !self.tabs.is_empty() && self.active_tab_is_settings(),
                        |this| this.child(self.render_settings_panel(window, cx)),
                    )
                    .when(
                        !self.tabs.is_empty() && !self.active_tab_is_settings(),
                        |this| {
                            let sidebar_width = self.sidebar_width();
                            let trailing_width = if !self.tabs.is_empty() {
                                self.right_panel_width()
                            } else {
                                0.0
                            };
                            let editor_pane_width =
                                (self.last_window_width - sidebar_width - trailing_width).max(1.0);
                            this.child(
                                div()
                                    .id("workspace-content-row")
                                    .flex_1()
                                    .flex()
                                    .flex_row()
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .id("editor-pane")
                                            .flex_1()
                                            .overflow_hidden()
                                            .flex()
                                            .flex_col()
                                            .child(self.render_active_tab_path_bar(cx))
                                            .child(
                                                div()
                                                    .id("editor-pane-content")
                                                    .relative()
                                                    .flex_1()
                                                    .overflow_hidden()
                                                    .cursor(
                                                        if self.active_tab_is_image()
                                                            || self.active_tab_is_binary()
                                                            || self.active_tab_is_diff()
                                                            || self.hovering_checkbox
                                                            || self.hovering_fold_line.is_some()
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
                                                        .on_hover(cx.listener(
                                                            |this, hovering: &bool, _window, cx| {
                                                                if !*hovering {
                                                                    let had_hover = this
                                                                        .hovered_editor_position
                                                                        .take()
                                                                        .is_some();
                                                                    let toolbar_changed = if this
                                                                        .toolbar_corner_hovered
                                                                    {
                                                                        this.toolbar_corner_hovered =
                                                                            false;
                                                                        this.sync_toolbar_opacity(cx);
                                                                        true
                                                                    } else {
                                                                        false
                                                                    };
                                                                    this.clear_gutter_fold_hover(cx);
                                                                    if had_hover || toolbar_changed {
                                                                        cx.notify();
                                                                    }
                                                                }
                                                            },
                                                        ))
                                                        .on_mouse_move(cx.listener(Self::on_mouse_move))
                                                        .on_scroll_wheel(cx.listener(Self::on_scroll))
                                                    })
                                                    .drag_over::<DraggedSidebarItems>(
                                                        move |style, _, _, _| style,
                                                    )
                                                    .drag_over::<ExternalPaths>({
                                                        let file_drop_bg =
                                                            self.styles.editor.editor_file_drop_bg;
                                                        move |style, _, _, _| style.bg(file_drop_bg)
                                                    })
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
                                                    .on_drop(cx.listener(
                                                        |this, paths: &ExternalPaths, _, cx| {
                                                            this.open_external_paths_in_tabs(paths.paths(), cx);
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
                                                    .when(render_editor_toolbar, |this| this.child({
                                                        let primary_hover = self
                                                            .styles
                                                            .toolbar
                                                            .toolbar_primary_hover_opacity;
                                                        let secondary_hover = self
                                                            .styles
                                                            .toolbar
                                                            .toolbar_secondary_hover_opacity;
                                                        let toolbar_top = self.scaled(
                                                            self.styles.toolbar.toolbar_offset_top_px,
                                                        );
                                                        let toolbar_left = self.scaled(
                                                            self.styles.toolbar.toolbar_offset_left_px,
                                                        );
                                                        let primary_btn_size = self.scaled(
                                                            self.styles.toolbar.toolbar_primary_btn_size_px,
                                                        );
                                                        let secondary_btn_size = self.scaled(
                                                            self.styles
                                                                .toolbar
                                                                .toolbar_secondary_btn_size_px,
                                                        );
                                                        let toolbar_gap =
                                                            self.scaled(self.styles.toolbar.toolbar_gap_px);
                                                        let primary_icon_size = self.scaled(
                                                            self.styles
                                                                .toolbar
                                                                .toolbar_primary_icon_size_px,
                                                        );
                                                        let secondary_icon_size = self.scaled(
                                                            self.styles
                                                                .toolbar
                                                                .toolbar_secondary_icon_size_px,
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
                                                        div()
                                                            .absolute()
                                                            .left(px(toolbar_left))
                                                            .top(px(toolbar_top))
                                                            .flex()
                                                            .items_center()
                                                            .gap(px(toolbar_gap))
                                                            .opacity(self.toolbar_opacity)
                                                            .child(self.render_toolbar_button(
                                                                "toolbar-sidebar-toggle",
                                                                primary_btn_size,
                                                                primary_btn_size,
                                                                primary_icon_size,
                                                                1.0,
                                                                primary_hover,
                                                                SIDEBAR_TOGGLE_SVG_PATH,
                                                                sidebar_anchor,
                                                                self.t(crate::i18n::TextKey::ToolbarSidebarTooltip),
                                                                Some("⌘\\"),
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
                                                                secondary_btn_size,
                                                                secondary_icon_size,
                                                                1.0,
                                                                secondary_hover,
                                                                EDITOR_ALIGN_LEFT_SVG_PATH,
                                                                align_anchor,
                                                                self.t(crate::i18n::TextKey::ToolbarAlignTooltip),
                                                                Some("⌘⇧L"),
                                                                cx,
                                                                |this, cx| {
                                                                    this.toggle_editor_alignment(cx);
                                                                },
                                                            ))
                                                    }))
                                                    .when(render_editor_toolbar, |this| this.child({
                                                        let secondary_hover = self
                                                            .styles
                                                            .toolbar
                                                            .toolbar_secondary_hover_opacity;
                                                        let toolbar_gap =
                                                            self.scaled(self.styles.toolbar.toolbar_gap_px);
                                                        let toolbar_top = self.scaled(
                                                            self.styles.toolbar.toolbar_offset_top_px,
                                                        );
                                                        let toolbar_right = self.scaled(
                                                            self.styles.toolbar.toolbar_offset_left_px,
                                                        );
                                                        let secondary_btn_size = self.scaled(
                                                            self.styles
                                                                .toolbar
                                                                .toolbar_secondary_btn_size_px,
                                                        );
                                                        let secondary_icon_size = self.scaled(
                                                            self.styles
                                                                .toolbar
                                                                .toolbar_secondary_icon_size_px,
                                                        );
                                                        let secondary_btn_width = secondary_btn_size;
                                                        let outline_active = self.utility_panel
                                                            == Some(RightUtilityPanel::Outline);
                                                        let outline_x = editor_pane_width
                                                            - toolbar_right
                                                            - secondary_btn_width * 2.0
                                                            - toolbar_gap;
                                                        let outline_anchor = toolbar_anchor_bounds(
                                                            outline_x,
                                                            toolbar_top,
                                                            secondary_btn_width,
                                                        );
                                                        let theme_x = editor_pane_width
                                                            - toolbar_right
                                                            - secondary_btn_width;
                                                        let theme_anchor = toolbar_anchor_bounds(
                                                            theme_x,
                                                            toolbar_top,
                                                            secondary_btn_width,
                                                        );
                                                        self.sync_tooltip_anchor(
                                                            "toolbar-outline-toggle",
                                                            outline_anchor,
                                                        );
                                                        self.sync_tooltip_anchor(
                                                            "toolbar-theme-toggle",
                                                            theme_anchor,
                                                        );
                                                        div()
                                                            .absolute()
                                                            .right(px(toolbar_right))
                                                            .top(px(toolbar_top))
                                                            .flex()
                                                            .items_center()
                                                            .gap(px(toolbar_gap))
                                                            .opacity(self.toolbar_opacity)
                                                            .child(self.render_toolbar_button(
                                                                "toolbar-outline-toggle",
                                                                secondary_btn_width,
                                                                secondary_btn_size,
                                                                secondary_icon_size,
                                                                if outline_active { 1.0 } else { 0.55 },
                                                                secondary_hover,
                                                                OUTLINE_TOGGLE_SVG_PATH,
                                                                outline_anchor,
                                                                self.t(crate::i18n::TextKey::ToolbarOutlineTooltip),
                                                                None,
                                                                cx,
                                                                |this, cx| {
                                                                    this.toggle_outline_visibility(cx);
                                                                },
                                                            ))
                                                            .child(self.render_toolbar_button(
                                                                "toolbar-theme-toggle",
                                                                secondary_btn_width,
                                                                secondary_btn_size,
                                                                secondary_icon_size,
                                                                1.0,
                                                                secondary_hover,
                                                                if self.theme_kind.is_dark() {
                                                                    THEME_DARK_SVG_PATH
                                                                } else {
                                                                    THEME_LIGHT_SVG_PATH
                                                                },
                                                                theme_anchor,
                                                                self.t(crate::i18n::TextKey::ToolbarThemeTooltip),
                                                                Some("⌘⌥\\"),
                                                                cx,
                                                                |this, cx| {
                                                                    this.toggle_theme(cx);
                                                                },
                                                            ))
                                                    }))
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
                                            ),
                                    )
                                    .when(
                                        self.should_render_right_panels(),
                                        |this| this.child(self.render_right_panels(cx)),
                                    ),
                            )
                        },
                    ),
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
                } else if self.utility_panel_resizing {
                    let iw = self.scaled(
                        self.styles
                            .utility_panel
                            .utility_panel_resize_active_indicator_width_px,
                    );
                    let ic = self
                        .styles
                        .utility_panel
                        .utility_panel_resize_active_indicator_color;
                    let x = (self.last_window_width - self.utility_panel_width()).max(0.0);
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
                        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                            if this.sidebar_resizing {
                                let new_width = f32::from(event.position.x);
                                let min_w =
                                    this.scaled(this.styles.sidebar.sidebar_resize_min_width_px);
                                let max_w =
                                    this.scaled(this.styles.sidebar.sidebar_resize_max_width_px);
                                let new_width = new_width.clamp(min_w, max_w);
                                this.sidebar_custom_width = Some(new_width);
                                this.set_sidebar_width_immediately(new_width);
                                cx.notify();
                            } else if this.utility_panel_resizing {
                                let window_width = f32::from(window.bounds().size.width);
                                let min_w = this.scaled(
                                    this.styles.utility_panel.utility_panel_resize_min_width_px,
                                );
                                let max_w = this.scaled(
                                    this.styles.utility_panel.utility_panel_resize_max_width_px,
                                );
                                let new_width = window_width - f32::from(event.position.x);
                                let new_width = new_width.clamp(min_w, max_w);
                                this.utility_panel_custom_width = Some(new_width);
                                this.set_utility_panel_width_immediately(new_width);
                                cx.notify();
                            }
                        }))
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.sidebar_resizing = false;
                                this.utility_panel_resizing = false;
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
                    .child(self.t(crate::i18n::TextKey::BinaryFileMessage)),
            )
            .child(
                div()
                    .text_size(px(self.scaled(self.styles.ui_font_size_px - 1.0)))
                    .child(
                        path.as_ref()
                            .map(|value| value.display().to_string())
                            .unwrap_or_else(|| {
                                self.t(crate::i18n::TextKey::CurrentFile).to_string()
                            }),
                    ),
            )
            .child(
                div()
                    .px(px(self.scaled(12.0)))
                    .py(px(self.scaled(4.0)))
                    .rounded(px(self.scaled(8.0)))
                    .bg(self.theme.bg_hover)
                    .text_color(self.theme.text_primary)
                    .cursor(CursorStyle::PointingHand)
                    .child(self.t(crate::i18n::TextKey::OpenFile))
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
