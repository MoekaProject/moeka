use gpui::{
    CursorStyle, MouseButton, MouseDownEvent, MouseMoveEvent, Render, Window, div, img, prelude::*,
    px, svg,
};

use super::*;

impl Render for EditorView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let resizing = self.sidebar_resizing;
        let show_editor_toolbar = self.tabs.len() > 0 && self.scroll_offset <= self.scaled(18.0);
        div()
            .id("editor-root")
            .key_context("MarkdownEditor")
            .track_focus(&self.focus_handle(cx))
            .relative()
            .size_full()
            .flex()
            .flex_row()
            .overflow_hidden()
            .bg(self.theme.background)
            .border_1()
            .border_color(self.theme.panel_border)
            .rounded(px(16.0))
            .when(resizing, |this| {
                this.cursor(CursorStyle::ResizeLeftRight)
                    .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                        if this.sidebar_resizing {
                            let new_width = f32::from(event.position.x);
                            let min_w = this.scaled(140.0);
                            let max_w = this.scaled(600.0);
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
            })
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
            .on_action(cx.listener(Self::on_toggle_editor_left_align))
            .on_action(cx.listener(Self::on_escape))
            .when(self.sidebar_visible, |this| {
                let sidebar_w = self.sidebar_width();
                let indicator_color = if self.sidebar_resizing {
                    self.theme.accent
                } else {
                    self.theme.panel_border
                };
                let indicator_width = if self.sidebar_resizing { 3.0 } else { 1.0 };
                this.child(
                    self.render_sidebar(cx),
                )
                .child(
                    div()
                        .id("sidebar-resize-handle")
                        .absolute()
                        .left(px(sidebar_w - 1.0))
                        .top_0()
                        .w(px(5.0))
                        .h_full()
                        .cursor(CursorStyle::ResizeLeftRight)
                        .flex()
                        .items_start()
                        .child(div().w(px(indicator_width)).h_full().bg(indicator_color))
                        .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                            if this.sidebar_resizing {
                                let new_width = f32::from(event.position.x);
                                let min_w = this.scaled(140.0);
                                let max_w = this.scaled(600.0);
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
                    .bg(self.theme.background)
                    .child(self.render_tab_bar(cx))
                    .child(
                        div()
                            .id("editor-pane")
                            .relative()
                            .flex_1()
                            .overflow_hidden()
                            .cursor(
                                if self.hovering_checkbox || self.hovering_image_edit_line.is_some()
                                {
                                    CursorStyle::Arrow
                                } else {
                                    CursorStyle::IBeam
                                },
                            )
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                            .on_mouse_down(
                                MouseButton::Right,
                                cx.listener(Self::on_editor_context_menu),
                            )
                            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
                            .on_mouse_move(cx.listener(Self::on_mouse_move))
                            .on_scroll_wheel(cx.listener(Self::on_scroll))
                            .when(show_editor_toolbar, |this| {
                                let tb = &self.styles.toolbar;
                                this.child(
                                    div()
                                        .absolute()
                                        .left(px(self.scaled(tb.left_offset)))
                                        .top(px(self.scaled(tb.top_offset)))
                                        .flex()
                                        .items_center()
                                        .gap(px(self.scaled(tb.button_gap)))
                                        .child(
                                            div()
                                                .id("toolbar-sidebar-toggle")
                                                .w(px(self.scaled(tb.primary_button_size)))
                                                .h(px(self.scaled(tb.primary_button_size)))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .cursor(CursorStyle::Arrow)
                                                .hover(|style| style.opacity(0.9))
                                                .child(
                                                    img(PathBuf::from(SIDEBAR_TOGGLE_SVG_PATH))
                                                        .w(px(self.scaled(tb.primary_icon_size)))
                                                        .h(px(self.scaled(tb.primary_icon_size))),
                                                )
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.toggle_sidebar_visibility(cx);
                                                    }),
                                                ),
                                        )
                                        .child(
                                            div()
                                                .id("toolbar-align-toggle")
                                                .w(px(self.scaled(tb.secondary_button_size)))
                                                .h(px(self.scaled(tb.secondary_button_size)))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .cursor(CursorStyle::Arrow)
                                                .hover(|style| style.opacity(0.8))
                                                .child(
                                                    img(PathBuf::from(EDITOR_ALIGN_LEFT_SVG_PATH))
                                                        .w(px(self.scaled(tb.secondary_icon_size)))
                                                        .h(px(self.scaled(tb.secondary_icon_size))),
                                                )
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.toggle_editor_alignment(cx);
                                                    }),
                                                ),
                                        ),
                                )
                                .child(
                                    div()
                                        .absolute()
                                        .right(px(self.scaled(tb.left_offset)))
                                        .top(px(self.scaled(tb.top_offset)))
                                        .flex()
                                        .items_center()
                                        .child(
                                            div()
                                                .id("toolbar-theme-toggle")
                                                .w(px(self.scaled(tb.secondary_button_size)))
                                                .h(px(self.scaled(tb.secondary_button_size)))
                                                .flex()
                                                .items_center()
                                                .justify_center()
                                                .cursor(CursorStyle::Arrow)
                                                .hover(|style| style.opacity(0.8))
                                                .child(
                                                    img(PathBuf::from(THEME_TOGGLE_SVG_PATH))
                                                        .w(px(self.scaled(tb.secondary_icon_size)))
                                                        .h(px(self.scaled(tb.secondary_icon_size))),
                                                )
                                                .on_mouse_down(
                                                    MouseButton::Left,
                                                    cx.listener(|this, _, _, cx| {
                                                        this.toggle_theme(cx);
                                                    }),
                                                ),
                                        ),
                                )
                            })
                            .child(element::EditorElement {
                                editor: cx.entity().clone(),
                            }),
                    ),
            )
            .when(self.context_menu.is_some(), |this| {
                this.child(
                    div()
                        .absolute()
                        .left(px(0.0))
                        .top(px(0.0))
                        .w_full()
                        .h_full()
                        .cursor(CursorStyle::Arrow)
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                eprintln!("[menu-debug] backdrop left click");
                                this.close_context_menu(cx);
                            }),
                        )
                        .on_mouse_up(MouseButton::Left, cx.listener(|_, _, _, _| {}))
                        .on_mouse_up(MouseButton::Right, cx.listener(|_, _, _, _| {}))
                        .on_mouse_down(MouseButton::Right, cx.listener(|_, _, _, _| {}))
                        .on_mouse_move(cx.listener(|_, _, _, _| {}))
                        .on_scroll_wheel(cx.listener(|_, _, _, _| {}))
                        .child(self.render_context_menu(cx)),
                )
            })
    }
}
