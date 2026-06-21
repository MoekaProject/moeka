use std::sync::Arc;

use gpui::{
    AnyElement, App, Bounds, ContentMask, Corners, CursorStyle, ExternalPaths, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Render, SharedString, Window, canvas, deferred,
    div, fill, img, prelude::*, px,
};

use super::tooltip::{TooltipState, toolbar_anchor_bounds};
use super::*;

fn clamp_resize_dimension(value: f32, minimum: f32, maximum: f32) -> f32 {
    let lower_bound = minimum.min(maximum);
    let upper_bound = minimum.max(maximum);
    value.clamp(lower_bound, upper_bound)
}

impl EditorView {
    fn render_chat_panel_drop_targets(
        &self,
        editor_pane_width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let zone_margin = self.scaled(
            self.styles
                .utility_panel
                .utility_panel_drop_target_margin_px,
        );
        let zone_radius = self.scaled(
            self.styles
                .utility_panel
                .utility_panel_drop_target_radius_px,
        );
        let right_zone_width =
            (editor_pane_width * 0.28).clamp(self.scaled(160.0), self.scaled(240.0));
        let bottom_zone_height =
            (self.last_window_height * 0.22).clamp(self.scaled(120.0), self.scaled(190.0));
        let border_width = self.scaled(
            self.styles
                .utility_panel
                .utility_panel_drop_target_border_width_px,
        );
        let drop_target_bg = self.styles.utility_panel.utility_panel_drop_target_bg;
        let drop_target_active_bg = self
            .styles
            .utility_panel
            .utility_panel_drop_target_active_bg;
        let drop_target_border_color = self
            .styles
            .utility_panel
            .utility_panel_drop_target_border_color;
        let drop_target_active_border_color = self
            .styles
            .utility_panel
            .utility_panel_drop_target_active_border_color;
        let drop_target_label_bg = self.styles.utility_panel.utility_panel_drop_target_label_bg;
        let drop_target_label_text_color = self
            .styles
            .utility_panel
            .utility_panel_drop_target_label_text_color;
        let label_font_size = self.scaled(
            self.styles
                .utility_panel
                .utility_panel_drop_target_label_font_size_px,
        );
        let label_padding_x = self.scaled(
            self.styles
                .utility_panel
                .utility_panel_drop_target_label_padding_x_px,
        );
        let label_padding_y = self.scaled(
            self.styles
                .utility_panel
                .utility_panel_drop_target_label_padding_y_px,
        );

        let right_zone = div()
            .absolute()
            .right(px(zone_margin))
            .top(px(zone_margin))
            .bottom(px(zone_margin))
            .w(px(right_zone_width))
            .rounded(px(zone_radius))
            .border(px(border_width))
            .border_color(drop_target_border_color)
            .bg(drop_target_bg)
            .cursor(CursorStyle::Arrow)
            .flex()
            .items_center()
            .justify_center()
            .drag_over::<DraggedChatTab>(move |style, _, _, _| {
                style
                    .bg(drop_target_active_bg)
                    .border_color(drop_target_active_border_color)
            })
            .on_drop(cx.listener(|this, dragged: &DraggedChatTab, _, cx| {
                this.switch_to_chat_tab(&dragged.tab_id, cx);
                this.place_chat_panel(ChatPanelPlacement::Right, cx);
                this.chat_tab_drag_candidate_id = None;
                this.chat_tab_dragging_id = None;
            }))
            .child(
                div()
                    .px(px(label_padding_x))
                    .py(px(label_padding_y))
                    .rounded_full()
                    .bg(drop_target_label_bg)
                    .text_size(px(label_font_size))
                    .text_color(drop_target_label_text_color)
                    .child("终端放在右侧"),
            )
            .into_any_element();

        let bottom_zone = div()
            .absolute()
            .left(px(zone_margin))
            .right(px(zone_margin))
            .bottom(px(zone_margin))
            .h(px(bottom_zone_height))
            .rounded(px(zone_radius))
            .border(px(border_width))
            .border_color(drop_target_border_color)
            .bg(drop_target_bg)
            .cursor(CursorStyle::Arrow)
            .flex()
            .items_center()
            .justify_center()
            .drag_over::<DraggedChatTab>(move |style, _, _, _| {
                style
                    .bg(drop_target_active_bg)
                    .border_color(drop_target_active_border_color)
            })
            .on_drop(cx.listener(|this, dragged: &DraggedChatTab, _, cx| {
                this.switch_to_chat_tab(&dragged.tab_id, cx);
                this.place_chat_panel(ChatPanelPlacement::Bottom, cx);
                this.chat_tab_drag_candidate_id = None;
                this.chat_tab_dragging_id = None;
            }))
            .child(
                div()
                    .px(px(label_padding_x))
                    .py(px(label_padding_y))
                    .rounded_full()
                    .bg(drop_target_label_bg)
                    .text_size(px(label_font_size))
                    .text_color(drop_target_label_text_color)
                    .child("终端放到下方"),
            )
            .into_any_element();

        div()
            .absolute()
            .left(px(0.0))
            .top(px(0.0))
            .right(px(0.0))
            .bottom(px(0.0))
            .child(match self.chat_panel_placement {
                ChatPanelPlacement::Right => bottom_zone,
                ChatPanelPlacement::Bottom => right_zone,
            })
            .into_any_element()
    }

    fn render_right_panels(
        &mut self,
        chat_panel_focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
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
            .when(self.should_render_utility_panel_on_right(), |this| {
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
                                .child(self.render_utility_panel(chat_panel_focused, cx)),
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
                                            let new_width =
                                                clamp_resize_dimension(new_width, min_w, max_w);
                                            if let Some(panel) =
                                                this.active_resizable_utility_panel()
                                            {
                                                this.set_utility_panel_custom_width_for(
                                                    panel,
                                                    Some(new_width),
                                                );
                                            }
                                            this.set_utility_panel_width_immediately(new_width);
                                            cx.notify();
                                        }
                                    },
                                ))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _: &MouseDownEvent, window, cx| {
                                        window.prevent_default();
                                        cx.stop_propagation();
                                        this.tab_drag_candidate_id = None;
                                        this.tab_dragging_id = None;
                                        this.chat_tab_drag_candidate_id = None;
                                        this.chat_tab_dragging_id = None;
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
        self.refresh_chat_terminal_metrics(window, cx);
        self.process_pending_inline_diff(cx);
        if self.pending_editor_focus {
            window.focus(&self.focus_handle(cx));
            self.pending_editor_focus = false;
        }
        if self.pending_chat_input_focus {
            window.focus(&self.chat_panel_focus_handle);
            window.invalidate_character_coordinates();
            self.pending_chat_input_focus = false;
        }
        if self.utility_panel == Some(RightUtilityPanel::Chat)
            && self.chat_panel_focus_handle.is_focused(window)
        {
            window.invalidate_character_coordinates();
        }
        if self.focus_handle.is_focused(window) && self.active_tab_supports_text_buffer() {
            window.invalidate_character_coordinates();
        }
        let chat_panel_focused = self.chat_panel_focus_handle.is_focused(window);
        let resizing = self.sidebar_resizing || self.utility_panel_resizing;
        let toolbar_base_visible =
            self.tabs.len() > 0 && !self.active_tab_is_settings() && !self.active_tab_is_binary();
        let outline_rail_active =
            self.outline_rail_hovered || self.outline_rail_label_opacity > 0.01;
        let show_editor_toolbar = self.should_show_editor_toolbar(toolbar_base_visible);
        let render_editor_toolbar = toolbar_base_visible
            && !outline_rail_active
            && (show_editor_toolbar
                || self.toolbar_opacity_task.is_some()
                || self.toolbar_opacity > self.styles.toolbar.toolbar_hidden_opacity + 0.01);
        let ws = &self.styles.window;
        let editor = cx.entity().clone();
        window.on_mouse_event({
            let editor = editor.clone();
            move |event: &MouseMoveEvent, phase, _window, cx| {
                if !phase.capture() {
                    return;
                }

                let handled = editor.update(cx, |this, cx| {
                    if this.editor_scrollbar_drag.is_none() {
                        return false;
                    }

                    if event.dragging() {
                        this.hovered_editor_position = Some(event.position);
                        this.set_editor_scrollbar_hovered(true, cx);
                        this.update_editor_scrollbar_drag(event.position, cx);
                    } else {
                        this.stop_editor_scrollbar_drag(event.position, cx);
                    }
                    true
                });
                if handled {
                    cx.stop_propagation();
                }
            }
        });
        window.on_mouse_event({
            let editor = editor.clone();
            move |event: &MouseMoveEvent, phase, _window, cx| {
                if !phase.capture() {
                    return;
                }

                let handled = editor.update(cx, |this, cx| {
                    if event.dragging() {
                        this.update_panel_scrollbar_drag(event.position, cx)
                    } else {
                        this.stop_panel_scrollbar_drag(cx)
                    }
                });
                if handled {
                    cx.stop_propagation();
                }
            }
        });
        window.on_mouse_event({
            let editor = editor.clone();
            move |event: &MouseUpEvent, phase, _window, cx| {
                if !phase.capture() || event.button != MouseButton::Left {
                    return;
                }

                let handled = editor.update(cx, |this, cx| {
                    this.stop_editor_scrollbar_drag(event.position, cx)
                });
                if handled {
                    cx.stop_propagation();
                }
            }
        });
        window.on_mouse_event({
            let editor = editor.clone();
            move |event: &MouseUpEvent, phase, _window, cx| {
                if !phase.capture() || event.button != MouseButton::Left {
                    return;
                }

                let handled = editor.update(cx, |this, cx| this.stop_panel_scrollbar_drag(cx));
                if handled {
                    cx.stop_propagation();
                }
            }
        });
        // Store window bounds for menu positioning
        let wb = window.bounds();
        self.last_window_width = f32::from(wb.size.width);
        self.last_window_height = f32::from(wb.size.height);
        div()
            .id("editor-root")
            .key_context("MarkdownEditor")
            .track_focus(&self.focus_handle(cx))
            .on_key_down(cx.listener(|this, event: &gpui::KeyDownEvent, _, cx| {
                if this.active_tab_is_whiteboard()
                    && event.keystroke.key == "space"
                    && this.whiteboard_text_editor.is_none()
                    && !this.whiteboard_space_down
                {
                    this.whiteboard_space_down = true;
                    cx.notify();
                }
            }))
            .on_key_up(cx.listener(|this, event: &gpui::KeyUpEvent, _, cx| {
                if event.keystroke.key == "space" && this.whiteboard_space_down {
                    this.whiteboard_space_down = false;
                    cx.notify();
                }
            }))
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
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.context_menu.is_some() {
                        this.close_context_menu(cx);
                    }
                }),
            )
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
            .on_action(cx.listener(Self::on_new_whiteboard))
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
            .on_action(cx.listener(Self::on_open_file_dialog))
            .on_action(cx.listener(Self::on_find_in_files))
            .on_action(cx.listener(Self::on_quick_open))
            .on_action(cx.listener(Self::on_escape))
            .on_key_down(cx.listener(Self::on_preview_overlay_key_down))
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
                                    let new_width =
                                        clamp_resize_dimension(new_width, min_w, max_w);
                                    this.set_active_sidebar_custom_width(new_width);
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
                                    .id("workspace-content-column")
                                    .flex_1()
                                    .flex()
                                    .flex_col()
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .id("workspace-content-row")
                                            .flex_1()
                                            .min_h(px(0.0))
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
                                                                    || self.hovering_checkbox
                                                                    || self.hovering_fold_line.is_some()
                                                                    || self.hovering_image_edit_line.is_some()
                                                                {
                                                                    CursorStyle::Arrow
                                                                } else {
                                                                    CursorStyle::IBeam
                                                                },
                                                            )
                                                            .on_mouse_down(
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
                                                                        if this.editor_scrollbar_drag.is_none() {
                                                                            this.set_editor_scrollbar_hovered(false, cx);
                                                                        }
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
                                                            .child(if self.active_tab_is_whiteboard() {
                                                                whiteboard_element::WhiteboardElement {
                                                                    editor: cx.entity().clone(),
                                                                }
                                                                .into_any_element()
                                                            } else if self.active_tab_is_image() {
                                                                image_view::ImageViewElement {
                                                                    editor: cx.entity().clone(),
                                                                }
                                                                .into_any_element()
                                                            } else if self.active_tab_is_binary() {
                                                                self.render_binary_placeholder(cx)
                                                            } else {
                                                                element::EditorElement {
                                                                    editor: cx.entity().clone(),
                                                                }
                                                                .into_any_element()
                                                            })
                                                            .when(self.active_tab_is_whiteboard(), |this| {
                                                                this.child(
                                                                    self.render_whiteboard_toolbar(cx),
                                                                )
                                                                .when_some(
                                                                    self.whiteboard_text_overlay_rect(),
                                                                    |this, rect| {
                                                                        let input = self
                                                                            .whiteboard_text_editor
                                                                            .as_ref()
                                                                            .map(|editor| {
                                                                                editor.input.clone()
                                                                            });
                                                                        this.when_some(
                                                                            input,
                                                                            |this, input| {
                                                                                this.child(
                                                                                    div()
                                                                                        .id("wb-text-overlay")
                                                                                        .occlude()
                                                                                        .absolute()
                                                                                        .left(px(rect.0))
                                                                                        .top(px(rect.1))
                                                                                        .w(px(rect.2))
                                                                                        .h(px(rect.3))
                                                                                        .on_mouse_down(
                                                                                            MouseButton::Left,
                                                                                            cx.listener(|_, _: &MouseDownEvent, _, cx| {
                                                                                                cx.stop_propagation();
                                                                                            }),
                                                                                        )
                                                                                        .child(input),
                                                                                )
                                                                            },
                                                                        )
                                                                    },
                                                                )
                                                            })
                                                            .when_some(
                                                                self.render_outline_rail(cx),
                                                                |this, rail| this.child(rail),
                                                            )
                                                            .when_some(
                                                                self.editor_scrollbar_thumb_geometry(),
                                                                |this, (thumb_top, thumb_height)| {
                                                                    let thumb_width = self.scaled(self.editor_scrollbar_width);
                                                                    let hit_width = self.editor_scrollbar_hit_width();
                                                                    let right_margin = self.scaled(
                                                                        self.styles
                                                                            .editor
                                                                            .editor_scrollbar_right_margin_px,
                                                                    );
                                                                    let thumb_radius = self.scaled(
                                                                        self.styles
                                                                            .editor
                                                                            .editor_scrollbar_radius_px,
                                                                    );
                                                                    let mut thumb_color = self
                                                                        .styles
                                                                        .editor
                                                                        .editor_scrollbar_thumb_color;
                                                                    thumb_color.a *= self
                                                                        .editor_scrollbar_opacity
                                                                        .clamp(0.0, 1.0);
                                                                    this.child(
                                                                        div()
                                                                            .absolute()
                                                                            .top_0()
                                                                            .right(px(0.0))
                                                                            .h_full()
                                                                            .w(px(hit_width + right_margin))
                                                                            .child(
                                                                                div()
                                                                                    .absolute()
                                                                                    .top(px(thumb_top))
                                                                                    .right(px(right_margin))
                                                                                    .w(px(thumb_width))
                                                                                    .h(px(thumb_height))
                                                                                    .cursor(CursorStyle::Arrow)
                                                                                    .on_mouse_down(MouseButton::Left, cx.listener(|this, event: &MouseDownEvent, _, cx| {
                                                                                        cx.stop_propagation();
                                                                                        this.start_editor_scrollbar_drag(event.position, cx);
                                                                                    }))
                                                                                    .rounded(px(thumb_radius))
                                                                                    .bg(thumb_color),
                                                                            ),
                                                                    )
                                                                },
                                                            )
                                                            .when(self.chat_tab_dragging_id.is_some(), |this| {
                                                                this.child(self.render_chat_panel_drop_targets(
                                                                    editor_pane_width,
                                                                    cx,
                                                                ))
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
                                                                let toolbar_min_top = self.scaled(
                                                            self.styles.toolbar.toolbar_offset_top_px,
                                                        );
                                                                let toolbar_min_left = self.scaled(
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
                                                        let toolbar_left = toolbar_min_left.round();
                                                        let toolbar_top = toolbar_min_top.round();
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
                                                        let align_tooltip = if self.active_tab_is_non_markdown_text() {
                                                            self.t(crate::i18n::TextKey::ToolbarWrapTooltip)
                                                        } else {
                                                            self.t(crate::i18n::TextKey::ToolbarAlignTooltip)
                                                        };
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
                                                            .when(self.active_tab_supports_text_buffer(), |this| this.child(self.render_toolbar_button(
                                                                "toolbar-align-toggle",
                                                                secondary_btn_size,
                                                                secondary_btn_size,
                                                                secondary_icon_size,
                                                                if self.editor_left_aligned { 1.0 } else { 0.55 },
                                                                secondary_hover,
                                                                EDITOR_ALIGN_LEFT_SVG_PATH,
                                                                align_anchor,
                                                                align_tooltip,
                                                                Some("⌘⇧L"),
                                                                cx,
                                                                |this, cx| {
                                                                    this.toggle_editor_alignment(cx);
                                                                },
                                                            )))
                                                    }))
                                                    .when(render_editor_toolbar, |this| this.child({
                                                        let secondary_hover = self
                                                            .styles
                                                            .toolbar
                                                            .toolbar_secondary_hover_opacity;
                                                        let toolbar_gap =
                                                            self.scaled(self.styles.toolbar.toolbar_gap_px);
                                                        let toolbar_min_top = self.scaled(
                                                            self.styles.toolbar.toolbar_offset_top_px,
                                                        );
                                                        let toolbar_min_right = self.scaled(
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
                                                        let toolbar_right = toolbar_min_right.round();
                                                        let toolbar_top = toolbar_min_top.round();
                                                        let outline_active = self.outline_rail_visible;
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
                                                            .when(self.active_tab_is_markdown(), |this| this.child(self.render_toolbar_button(
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
                                                            )))
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
                                                    .when(self.preview_overlay_state().is_some(), |this| {
                                                        let editor_handle = cx.entity().clone();
                                                        this.child(deferred(
                                                            canvas(
                                                                move |bounds, window, cx| {
                                                                    editor_handle.update(cx, |editor, _| {
                                                                        let Some(overlay) = editor.preview_overlay_state().cloned() else {
                                                                            editor.preview_overlay_bounds = None;
                                                                            editor.preview_overlay_image_bounds = None;
                                                                            return None;
                                                                        };
                                                                        let Some(base_image) = editor
                                                                            .image_cache
                                                                            .get(&overlay.cache_key)
                                                                            .and_then(|image| image.as_ref())
                                                                            .cloned()
                                                                        else {
                                                                            editor.preview_overlay_bounds = None;
                                                                            editor.preview_overlay_image_bounds = None;
                                                                            return None;
                                                                        };

                                                                        let base_size = base_image.size(0);
                                                                        let base_width = base_size.width.0 as f32;
                                                                        let base_height = base_size.height.0 as f32;
                                                                        let preview_kind = editor
                                                                            .decorations
                                                                            .get(overlay.line_idx)
                                                                            .and_then(|decoration| decoration.block_preview.as_ref())
                                                                            .map(|preview| preview.kind);
                                                                        let (intrinsic_width, intrinsic_height) =
                                                                            if let Some(preview_kind) = preview_kind {
                                                                                helpers::intrinsic_generated_preview_size_for_kind(
                                                                                    base_width,
                                                                                    base_height,
                                                                                    preview_kind,
                                                                                )
                                                                            } else {
                                                                                (base_width, base_height)
                                                                            };
                                                                        if intrinsic_width <= 0.0 || intrinsic_height <= 0.0 {
                                                                            editor.preview_overlay_bounds = None;
                                                                            editor.preview_overlay_image_bounds = None;
                                                                            return None;
                                                                        }

                                                                        let overlay_bounds = bounds;
                                                                        let overlay_width = f32::from(bounds.size.width).max(1.0);
                                                                        let overlay_height = f32::from(bounds.size.height).max(1.0);
                                                                        let generated_preview_overlay = preview_kind.is_some();
                                                                        let fit_scale = if generated_preview_overlay {
                                                                            overlay_width / intrinsic_width.max(1.0)
                                                                        } else {
                                                                            (overlay_width / intrinsic_width.max(1.0))
                                                                                .min(overlay_height / intrinsic_height.max(1.0))
                                                                                .min(1.0)
                                                                        };
                                                                        let base_display_width = intrinsic_width * fit_scale;
                                                                        let base_display_height = intrinsic_height * fit_scale;
                                                                        let display_width =
                                                                            base_display_width * overlay.view.zoom_factor;
                                                                        let display_height =
                                                                            base_display_height * overlay.view.zoom_factor;
                                                                        let image_left = f32::from(bounds.origin.x)
                                                                            + (overlay_width - display_width) * 0.5
                                                                            + overlay.view.pan_x;
                                                                        let image_top = if generated_preview_overlay {
                                                                            f32::from(bounds.origin.y) + overlay.view.pan_y
                                                                        } else {
                                                                            f32::from(bounds.origin.y)
                                                                                + (overlay_height - display_height) * 0.5
                                                                                + overlay.view.pan_y
                                                                        };
                                                                        let overlay_image_bounds = Bounds::new(
                                                                            gpui::point(px(image_left), px(image_top)),
                                                                            gpui::size(px(display_width), px(display_height)),
                                                                        );

                                                                        let image = if let Some(preview_kind) = preview_kind {
                                                                            let minimum_raster_scale = helpers::generated_preview_raster_scale(preview_kind);
                                                                            // Keep overlay zoom responsive by scaling an already-rasterized
                                                                            // image instead of regenerating Mermaid/Math SVG on every wheel tick.
                                                                            let overlay_raster_scale =
                                                                                window.scale_factor().max(minimum_raster_scale);
                                                                            let scale_key = ((overlay_raster_scale * 100.0)
                                                                                .round()
                                                                                .clamp(
                                                                                    minimum_raster_scale * 100.0,
                                                                                    1600.0,
                                                                                )) as u16;

                                                                            let cached_overlay_image = editor
                                                                                .generated_preview_overlay_cache
                                                                                .get(&overlay.cache_key)
                                                                                .and_then(|(cached_scale_key, cached_image)| {
                                                                                    (*cached_scale_key == scale_key)
                                                                                        .then(|| cached_image.clone())
                                                                                });
                                                                            let svg = editor
                                                                                .generated_preview_svg_cache
                                                                                .get(&overlay.cache_key)
                                                                                .cloned();
                                                                            if let Some(cached_overlay_image) = cached_overlay_image {
                                                                                cached_overlay_image
                                                                            } else if let Some(svg) = svg {
                                                                                match helpers::load_render_image_from_svg_with_scale(
                                                                                    svg.as_ref(),
                                                                                    scale_key as f32 / 100.0,
                                                                                )
                                                                                .map(Arc::new)
                                                                                {
                                                                                    Ok(image) => {
                                                                                        editor.generated_preview_overlay_cache.insert(
                                                                                            overlay.cache_key.clone(),
                                                                                            (scale_key, image.clone()),
                                                                                        );
                                                                                        image
                                                                                    }
                                                                                    Err(_) => base_image.clone(),
                                                                                }
                                                                            } else {
                                                                                base_image.clone()
                                                                            }
                                                                        } else {
                                                                            base_image.clone()
                                                                        };

                                                                        editor.preview_overlay_bounds = Some(overlay_bounds);
                                                                        editor.preview_overlay_image_bounds =
                                                                            Some(overlay_image_bounds);

                                                                        Some((
                                                                            overlay_bounds,
                                                                            overlay_image_bounds,
                                                                            image,
                                                                            editor.styles.editor.editor_bg,
                                                                            // Generated math/Mermaid previews are cropped tightly
                                                                            // to glyph ink, so a rounded-corner clip cuts into real
                                                                            // glyphs (e.g. E=mc²). Keep square corners for them.
                                                                            px(0.0),
                                                                        ))
                                                                    })
                                                                },
                                                                move |_bounds, overlay_paint, window, _cx| {
                                                                    if let Some((overlay_bounds, overlay_image_bounds, img_data, background, corner_radius)) = overlay_paint {
                                                                        window.paint_quad(fill(overlay_bounds, background));
                                                                        window.with_content_mask(
                                                                            Some(ContentMask {
                                                                                bounds: overlay_bounds,
                                                                            }),
                                                                            |window| {
                                                                                window
                                                                                    .paint_image(
                                                                                        overlay_image_bounds,
                                                                                        Corners::all(corner_radius),
                                                                                        img_data,
                                                                                        0,
                                                                                        false,
                                                                                    )
                                                                                    .ok();
                                                                            },
                                                                        );
                                                                    }
                                                                },
                                                            )
                                                            .absolute()
                                                            .left(px(0.0))
                                                            .top(px(0.0))
                                                            .right(px(0.0))
                                                            .bottom(px(0.0)),
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
                                                    ),
                                            )
                                            .when(
                                                self.should_render_right_panels(),
                                                |this| {
                                                    this.child(self.render_right_panels(
                                                        chat_panel_focused,
                                                        cx,
                                                    ))
                                                },
                                            ),
                                    )
                                    .when(self.should_render_bottom_chat_panel(), |this| {
                                        let panel_h = self.utility_panel_width();
                                        let panel_layout_h = self.utility_panel_layout_width();
                                        let ups = &self.styles.utility_panel;
                                        let hover_indicator_color =
                                            ups.utility_panel_resize_active_indicator_color;
                                        let hover_indicator_width = self.scaled(
                                            ups.utility_panel_resize_active_indicator_width_px,
                                        );
                                        let handle_width =
                                            self.scaled(ups.utility_panel_resize_handle_width_px);
                                        this.child(
                                            div()
                                                .id("bottom-chat-panel-wrap")
                                                .relative()
                                                .w_full()
                                                .h(px(panel_h))
                                                .overflow_hidden()
                                                .flex_none()
                                                .child(
                                                    div()
                                                        .w_full()
                                                        .h(px(panel_layout_h))
                                                        .child(self.render_utility_panel(
                                                            chat_panel_focused,
                                                            cx,
                                                        )),
                                                )
                                                .child(
                                                    div()
                                                        .id("bottom-chat-panel-resize-handle")
                                                        .absolute()
                                                        .left_0()
                                                        .top_0()
                                                        .w_full()
                                                        .h(px(handle_width))
                                                        .cursor(CursorStyle::ResizeUpDown)
                                                        .hover(move |style| {
                                                            style
                                                                .border_t(px(hover_indicator_width))
                                                                .border_color(hover_indicator_color)
                                                        })
                                                        .on_mouse_move(cx.listener(
                                                            |this,
                                                             event: &MouseMoveEvent,
                                                             window,
                                                             cx| {
                                                                if this.utility_panel_resizing {
                                                                    let window_height =
                                                                        f32::from(window.bounds().size.height);
                                                                    let min_h = this.scaled(
                                                                        this.styles
                                                                            .utility_panel
                                                                            .utility_panel_resize_min_width_px,
                                                                    );
                                                                    let max_h = this.scaled(
                                                                        this.styles
                                                                            .utility_panel
                                                                            .utility_panel_resize_max_width_px,
                                                                    );
                                                                    let new_height =
                                                                        window_height - f32::from(event.position.y);
                                                                    let new_height =
                                                                        clamp_resize_dimension(new_height, min_h, max_h);
                                                                    if let Some(panel) =
                                                                        this.active_resizable_utility_panel()
                                                                    {
                                                                        this.set_utility_panel_custom_width_for(
                                                                            panel,
                                                                            Some(new_height),
                                                                        );
                                                                    }
                                                                    this.set_utility_panel_width_immediately(
                                                                        new_height,
                                                                    );
                                                                    cx.notify();
                                                                }
                                                            },
                                                        ))
                                                        .on_mouse_down(
                                                            MouseButton::Left,
                                                            cx.listener(
                                                                |this: &mut Self,
                                                                 _: &MouseDownEvent,
                                                                 window,
                                                                 cx| {
                                                                    window.prevent_default();
                                                                    cx.stop_propagation();
                                                                    this.tab_drag_candidate_id = None;
                                                                    this.tab_dragging_id = None;
                                                                    this.chat_tab_drag_candidate_id = None;
                                                                    this.chat_tab_dragging_id = None;
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
                    if self.should_render_bottom_chat_panel() {
                        let left = if self.should_render_sidebar_panel() {
                            self.sidebar_width()
                        } else {
                            0.0
                        };
                        let width = (self.last_window_width - left).max(0.0);
                        let y = (self.last_window_height - self.utility_panel_width()).max(0.0);
                        Some(
                            div()
                                .absolute()
                                .left(px(left))
                                .top(px(y))
                                .w(px(width))
                                .h(px(iw))
                                .bg(ic)
                                .into_any_element(),
                        )
                    } else {
                        let top = if self.tabs.is_empty() {
                            0.0
                        } else {
                            self.tab_bar_height()
                        };
                        let height = (self.last_window_height - top).max(0.0);
                        let x = (self.last_window_width - self.utility_panel_width()).max(0.0);
                        Some(
                            div()
                                .absolute()
                                .left(px(x))
                                .top(px(top))
                                .w(px(iw))
                                .h(px(height))
                                .bg(ic)
                                .into_any_element(),
                        )
                    }
                } else {
                    None
                };
                let resize_cursor = if self.utility_panel_resizing
                    && self.should_render_bottom_chat_panel()
                {
                    CursorStyle::ResizeUpDown
                } else {
                    CursorStyle::ResizeLeftRight
                };
                this.child(deferred(
                    div()
                        .occlude()
                        .absolute()
                        .left(px(0.0))
                        .top(px(0.0))
                        .w_full()
                        .h_full()
                        .cursor(resize_cursor)
                        .children(indicator_line)
                        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, window, cx| {
                            if this.sidebar_resizing {
                                let new_width = f32::from(event.position.x);
                                let min_w =
                                    this.scaled(this.styles.sidebar.sidebar_resize_min_width_px);
                                let max_w =
                                    this.scaled(this.styles.sidebar.sidebar_resize_max_width_px);
                                let new_width = clamp_resize_dimension(new_width, min_w, max_w);
                                this.set_active_sidebar_custom_width(new_width);
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
                                let new_size = if this.should_render_bottom_chat_panel() {
                                    let window_height = f32::from(window.bounds().size.height);
                                    clamp_resize_dimension(
                                        window_height - f32::from(event.position.y),
                                        min_w,
                                        max_w,
                                    )
                                } else {
                                    clamp_resize_dimension(
                                        window_width - f32::from(event.position.x),
                                        min_w,
                                        max_w,
                                    )
                                };
                                if let Some(panel) = this.active_resizable_utility_panel() {
                                    this.set_utility_panel_custom_width_for(panel, Some(new_size));
                                }
                                this.set_utility_panel_width_immediately(new_size);
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
                this.child(deferred(self.render_context_menu(cx)))
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
