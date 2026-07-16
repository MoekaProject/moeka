use gpui::{AnyElement, Context, CursorStyle, Focusable, MouseButton, Window, div, prelude::*, px};

use super::{
    EditorView,
    shortcut::{COMMAND_SPECS, CommandSpec, ShortcutRenderStyle, render_shortcut},
};
use crate::actions::ToggleCommandPalette;
use crate::i18n::TextKey;

impl EditorView {
    pub(crate) fn on_toggle_command_palette(
        &mut self,
        _: &ToggleCommandPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_command_palette(window, cx);
    }

    pub(crate) fn toggle_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.command_palette_visible {
            self.close_command_palette(window, cx);
            return;
        }

        if self.quick_open_visible {
            self.close_quick_open(window, cx);
        }

        if self.command_palette_editor.is_none() {
            let styles = self.quick_open_text_input_styles();
            let placeholder = self.t(TextKey::CommandPalettePlaceholder);
            let editor =
                cx.new(move |cx| crate::text_input::TextInput::new(styles, placeholder, cx));
            cx.subscribe(
                &editor,
                |this, _editor, event: &crate::text_input::TextInputEvent, cx| match event {
                    crate::text_input::TextInputEvent::Changed => {
                        this.command_palette_query = this
                            .command_palette_editor
                            .as_ref()
                            .map(|editor| editor.read(cx).text().to_string())
                            .unwrap_or_default();
                        this.command_palette_selected_index = 0;
                        this.command_palette_scroll_handle.scroll_to_item(0);
                        cx.notify();
                    }
                    crate::text_input::TextInputEvent::Submit
                    | crate::text_input::TextInputEvent::SubmitBackward => {
                        this.queue_selected_command(cx);
                    }
                    crate::text_input::TextInputEvent::Escape => {
                        this.command_palette_visible = false;
                        this.pending_editor_focus = true;
                        cx.notify();
                    }
                    crate::text_input::TextInputEvent::MoveUp => {
                        this.move_command_palette_selection(-1, cx);
                    }
                    crate::text_input::TextInputEvent::MoveDown => {
                        this.move_command_palette_selection(1, cx);
                    }
                },
            )
            .detach();
            self.command_palette_editor = Some(editor);
        }

        self.command_palette_visible = true;
        self.command_palette_selected_index = 0;
        self.command_palette_scroll_handle.scroll_to_item(0);
        if let Some(editor) = &self.command_palette_editor {
            editor.update(cx, |input, cx| {
                input.styles = self.quick_open_text_input_styles();
                input.set_text("", cx);
            });
            editor.focus_handle(cx).focus(window);
        }
        cx.notify();
    }

    pub(crate) fn close_command_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.command_palette_visible = false;
        window.focus(&self.focus_handle(cx));
        cx.notify();
    }

    pub(crate) fn execute_pending_command(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(command) = self.command_palette_pending.take() else {
            return;
        };
        self.command_palette_visible = false;
        window.focus(&self.focus_handle(cx));
        let action = command.action();
        window.defer(cx, move |window, cx| {
            window.dispatch_action(action, cx);
        });
    }

    fn visible_command_specs(&self) -> Vec<&'static CommandSpec> {
        let query = self.command_palette_query.trim().to_lowercase();
        COMMAND_SPECS
            .iter()
            .filter(|spec| !spec.markdown_only || self.active_tab_is_markdown())
            .filter(|spec| {
                if query.is_empty() {
                    return true;
                }
                let title = self.command_title(spec).to_lowercase();
                title.contains(&query)
                    || spec
                        .id
                        .english_description()
                        .to_lowercase()
                        .contains(&query)
                    || spec.category.to_lowercase().contains(&query)
            })
            .collect()
    }

    fn command_title(&self, spec: &CommandSpec) -> &'static str {
        spec.localized_title(self.ui_language)
    }

    fn move_command_palette_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let item_count = self.visible_command_specs().len();
        if item_count == 0 {
            self.command_palette_selected_index = 0;
            cx.notify();
            return;
        }
        let index = (self.command_palette_selected_index as isize + delta)
            .clamp(0, item_count as isize - 1) as usize;
        self.command_palette_selected_index = index;
        self.command_palette_scroll_handle.scroll_to_item(index);
        cx.notify();
    }

    fn queue_selected_command(&mut self, cx: &mut Context<Self>) {
        let command = self
            .visible_command_specs()
            .get(self.command_palette_selected_index)
            .map(|spec| spec.id);
        if let Some(command) = command {
            self.command_palette_pending = Some(command);
            self.command_palette_visible = false;
            cx.notify();
        }
    }

    pub(crate) fn render_command_palette_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let styles = &self.styles.quick_open;
        let panel_width = self.scaled(styles.quick_open_panel_width_px);
        let sidebar_width = self.sidebar_width();
        let right_panel_width = self.right_panel_width();
        let editor_width =
            (self.last_window_width - sidebar_width - right_panel_width).max(panel_width);
        let panel_left = sidebar_width + (editor_width - panel_width) / 2.0;
        let panel_top =
            (self.last_window_height / 5.0).max(self.scaled(styles.quick_open_panel_top_offset_px));
        let row_height = self.scaled(styles.quick_open_result_row_height_px);
        let title_size = self.scaled(styles.quick_open_result_name_font_size_px);
        let description_size = self.scaled(styles.quick_open_result_path_font_size_px);
        let selected_bg = styles.quick_open_result_selected_bg;
        let hover_bg = styles.quick_open_result_hover_bg;
        let shortcut_style = ShortcutRenderStyle {
            font_size_px: description_size,
            font_family: self.styles.ui_font_family,
            text_color: styles.quick_open_result_secondary_text_color,
            gap_px: self.scaled(2.0),
        };
        let specs = self.visible_command_specs();
        let shortcut_bg = styles.quick_open_result_shortcut_bg;

        let rows = specs.into_iter().enumerate().map(|(index, spec)| {
            let selected = index == self.command_palette_selected_index;
            let command = spec.id;
            let shortcuts: Vec<String> =
                match self.app_settings.keybindings.get(spec.id.config_key()) {
                    Some(keystroke) => {
                        vec![super::shortcut::display_shortcut_from_keystroke(keystroke)]
                    }
                    None => spec
                        .shortcut
                        .into_iter()
                        .chain(spec.alternate_shortcuts.iter().copied())
                        .map(str::to_string)
                        .collect(),
                };
            div()
                .id(("command-palette", index))
                .mx(px(1.0))
                .h(px(row_height))
                .px(px(self.scaled(styles.quick_open_result_padding_x_px)))
                .flex()
                .items_center()
                .gap(px(self.scaled(12.0)))
                .cursor(CursorStyle::Arrow)
                .when(selected, |this| this.bg(selected_bg))
                .hover(move |style| style.bg(hover_bg))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .flex()
                        .items_center()
                        .gap(px(self.scaled(8.0)))
                        .child(
                            div()
                                .flex_none()
                                .truncate()
                                .text_size(px(title_size))
                                .text_color(styles.quick_open_result_text_color)
                                .child(self.command_title(spec)),
                        )
                        .child(
                            div()
                                .min_w(px(0.0))
                                .truncate()
                                .text_size(px(description_size))
                                .text_color(styles.quick_open_result_secondary_text_color)
                                .child(spec.id.english_description()),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .items_center()
                        .gap(px(self.scaled(4.0)))
                        .children(shortcuts.into_iter().map(|shortcut| {
                            div()
                                .h(px(self.scaled(22.0)))
                                .px(px(self.scaled(6.0)))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(self.scaled(5.0)))
                                .bg(shortcut_bg)
                                .child(render_shortcut(&shortcut, shortcut_style))
                        })),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        this.command_palette_pending = Some(command);
                        this.command_palette_visible = false;
                        cx.notify();
                    }),
                )
        });

        div()
            .occlude()
            .absolute()
            .left_0()
            .top_0()
            .size_full()
            .child(
                div()
                    .absolute()
                    .left_0()
                    .top_0()
                    .size_full()
                    .bg(styles.quick_open_backdrop_bg)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            this.close_command_palette(window, cx);
                        }),
                    ),
            )
            .child(
                div()
                    .id("command-palette-overlay")
                    .absolute()
                    .top(px(panel_top))
                    .left(px(panel_left.max(0.0)))
                    .w(px(panel_width))
                    .bg(styles.quick_open_panel_bg)
                    .border(px(1.0))
                    .border_color(styles.quick_open_panel_border_color)
                    .rounded(px(self.scaled(styles.quick_open_panel_radius_px)))
                    .shadow_md()
                    .overflow_hidden()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_, _, window, cx| {
                            cx.stop_propagation();
                            window.prevent_default();
                        }),
                    )
                    .child(
                        div()
                            .w_full()
                            .px(px(self.scaled(styles.quick_open_input_padding_x_px)))
                            .py(px(self.scaled(styles.quick_open_input_padding_y_px)))
                            .border_b(px(1.0))
                            .border_color(styles.quick_open_panel_border_color)
                            .when_some(self.command_palette_editor.clone(), |this, editor| {
                                this.child(editor)
                            }),
                    )
                    .child(
                        div()
                            .id("command-palette-results")
                            .w_full()
                            .max_h(px(self.scaled(styles.quick_open_results_max_height_px)))
                            .overflow_y_scroll()
                            .track_scroll(&self.command_palette_scroll_handle)
                            .children(rows),
                    )
                    .child(self.render_search_shortcut_footer(
                        crate::i18n::search_menu_footer_label(self.ui_language, "execute"),
                    )),
            )
            .into_any_element()
    }
}
