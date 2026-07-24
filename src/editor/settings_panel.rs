use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, Context, CursorStyle, Focusable, MouseButton, ScrollHandle, SharedString, Window,
    div, prelude::*, px, relative, svg,
};

use crate::checkbox::{Checkbox, CheckboxColors};
use crate::i18n::{
    TextKey, UiLanguage, attachment_settings_label, language_picker_options,
    terminal_settings_label,
};
use crate::theme::{EditorThemeMode, ThemeKind};

use super::{
    APP_VERSION, ChatAgentCli, DOC_SVG_PATH, EditorView, FLOWER_SVG_PATH, MOUNTAIN_SVG_PATH,
    SITE_SVG_PATH, SearchMenuConfig, SearchMenuRow, THEME_DARK_MINI_SVG_PATH,
    THEME_LIGHT_MINI_SVG_PATH, TerminalCursorShape, attachments, tooltip,
};

// ── Font picker state ───────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum FontRole {
    UiFont,
    BodyFont,
    CodeFont,
    InterfaceLanguage,
    DateTimeFormat,
    AttachmentLocation,
    TerminalCursorShape,
    TerminalLaunchCommand,
    /// Font family for selected whiteboard elements (whiteboard panel).
    WhiteboardFont,
}

#[derive(Clone)]
pub(crate) struct PickerOption {
    pub value: String,
    pub label: String,
    pub search_text: String,
    pub preview_font: Option<String>,
    pub icon_path: Option<&'static str>,
}

#[derive(Clone)]
pub(crate) struct FontPickerState {
    pub role: FontRole,
    pub option_index: usize,
    pub all_options: Vec<PickerOption>,
    pub filtered: Vec<PickerOption>,
    pub selected_index: usize,
    pub render_limit: usize,
    pub scroll_handle: ScrollHandle,
}

#[derive(Clone, Copy)]
enum SettingsOptionAction {
    ShowWhitespace(bool),
    HighlightLine(bool),
    SimpleMode(bool),
    TerminalCopyOnSelect(bool),
}

// ── Settings panel rendering ────────────────────────────────────────

impl EditorView {
    pub(crate) fn render_settings_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bg = self.styles.editor.editor_bg;
        let sps = &self.styles.settings_panel;
        let version_label = format!("v{APP_VERSION}");

        div()
            .id("settings-panel")
            .flex_1()
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.settings_scroll_handle)
            .bg(bg)
            .text_size(px(self.scaled(self.styles.ui_font_size_px)))
            .text_color(sps.settings_primary_text_color)
            .child(
                div()
                    .max_w(px(self.scaled(640.0)))
                    .mx_auto()
                    .py(px(self.scaled(60.0)))
                    .px(px(self.scaled(40.0)))
                    .flex()
                    .flex_col()
                    .gap(px(self.scaled(32.0)))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .items_end()
                            .justify_between()
                            .gap(px(self.scaled(sps.settings_header_gap_px)))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .flex()
                                    .items_end()
                                    .gap(px(self.scaled(sps.settings_title_version_gap_px)))
                                    .child(
                                        div()
                                            .text_size(px(
                                                self.scaled(self.styles.ui_font_size_px + 9.0)
                                            ))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(sps.settings_primary_text_color)
                                            .child(self.t(TextKey::SettingsPanelTitle)),
                                    )
                                    .child(
                                        div()
                                            .mb(px(self.scaled(
                                                sps.settings_title_version_margin_bottom_px,
                                            )))
                                            .text_size(px(self
                                                .scaled(sps.settings_title_version_font_size_px)))
                                            .text_color(sps.settings_title_version_text_color)
                                            .child(version_label),
                                    ),
                            )
                            .child(
                                div()
                                    .ml_auto()
                                    .flex_none()
                                    .flex()
                                    .items_center()
                                    .justify_end()
                                    .gap(px(self.scaled(sps.settings_header_actions_gap_px)))
                                    .child(self.render_settings_help_button(cx))
                                    .child(self.render_settings_site_button(cx)),
                            ),
                    )
                    .child(self.render_theme_section(cx))
                    .child(self.render_font_section(window, cx))
                    .child(self.render_format_section(cx))
                    .child(self.render_terminal_settings_section(cx))
                    .child(self.render_attachment_settings_section(cx)),
            )
            .into_any_element()
    }

    fn render_settings_help_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let sps = &self.styles.settings_panel;
        let height = self.scaled(sps.settings_site_button_height_px);
        let icon_size = self.scaled(sps.settings_site_button_icon_size_px);

        div()
            .id("settings-help-button")
            .h(px(height))
            .max_w(px(self.scaled(sps.settings_header_button_max_width_px)))
            .px(px(self.scaled(sps.settings_site_button_padding_x_px)))
            .flex()
            .items_center()
            .gap(px(self.scaled(sps.settings_site_button_gap_px)))
            .rounded(px(self.scaled(sps.settings_site_button_radius_px)))
            .border(px(sps.settings_site_button_border_width_px))
            .border_color(sps.settings_site_button_border_color)
            .bg(sps.settings_site_button_bg)
            .text_size(px(self.scaled(sps.settings_site_button_font_size_px)))
            .text_color(sps.settings_site_button_text_color)
            .cursor(CursorStyle::Arrow)
            .hover({
                let hover_bg = sps.settings_site_button_hover_bg;
                move |style| style.bg(hover_bg)
            })
            .child(
                svg()
                    .path(DOC_SVG_PATH)
                    .size(px(icon_size))
                    .text_color(sps.settings_site_button_icon_color),
            )
            .child(
                div()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .truncate()
                    .whitespace_nowrap()
                    .child(self.t(TextKey::HelpManual)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.open_welcome_tab(cx);
                }),
            )
            .into_any_element()
    }

    fn render_settings_site_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let sps = &self.styles.settings_panel;
        let height = self.scaled(sps.settings_site_button_height_px);
        let icon_size = self.scaled(sps.settings_site_button_icon_size_px);
        let update_ready = self.prepared_update().is_some();
        let text_color = if update_ready {
            sps.settings_site_button_update_text_color
        } else {
            sps.settings_site_button_text_color
        };
        let icon_color = if update_ready {
            sps.settings_site_button_update_icon_color
        } else {
            sps.settings_site_button_icon_color
        };
        let label = if let Some(update) = self.prepared_update() {
            crate::i18n::update_available_label(self.ui_language, Some(&update.version))
        } else {
            self.t(TextKey::OfficialWebsite).to_string()
        };

        div()
            .id("settings-site-button")
            .h(px(height))
            .max_w(px(self.scaled(sps.settings_header_button_max_width_px)))
            .px(px(self.scaled(sps.settings_site_button_padding_x_px)))
            .flex()
            .items_center()
            .gap(px(self.scaled(sps.settings_site_button_gap_px)))
            .rounded(px(self.scaled(sps.settings_site_button_radius_px)))
            .border(px(sps.settings_site_button_border_width_px))
            .border_color(sps.settings_site_button_border_color)
            .bg(sps.settings_site_button_bg)
            .text_size(px(self.scaled(sps.settings_site_button_font_size_px)))
            .text_color(text_color)
            .cursor(CursorStyle::Arrow)
            .hover({
                let hover_bg = sps.settings_site_button_hover_bg;
                move |style| style.bg(hover_bg)
            })
            .child(
                svg()
                    .path(SITE_SVG_PATH)
                    .size(px(icon_size))
                    .text_color(icon_color),
            )
            .child(
                div()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .truncate()
                    .whitespace_nowrap()
                    .child(label),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    if this.prepared_update().is_some() {
                        this.show_prepared_update_dialog(cx);
                    } else if let Err(error) = std::process::Command::new("open")
                        .arg("https://moeka.app/")
                        .spawn()
                    {
                        eprintln!("failed to open official website: {error}");
                    }
                }),
            )
            .into_any_element()
    }

    fn render_format_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.date_time_format_preview_label(self.app_settings.date_time_format());

        div()
            .flex()
            .flex_col()
            .gap(px(self.scaled(12.0)))
            .child(self.render_section_heading(self.t(TextKey::FormatSection)))
            .child(self.render_editor_display_options(cx))
            .child(self.render_picker_row(
                self.t(TextKey::DateTimeFormat),
                &current,
                FontRole::DateTimeFormat,
                0,
                cx,
            ))
            .into_any_element()
    }

    fn render_terminal_settings_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.ensure_terminal_command_args_editor(cx);
        let cursor_shape = self.app_settings.terminal_cursor_shape();
        let current = terminal_settings_label(self.ui_language, cursor_shape.as_str());
        let launch_cli = self.app_settings.terminal_launch_cli();

        div()
            .flex()
            .flex_col()
            .gap(px(self.scaled(12.0)))
            .child(
                self.render_section_heading(terminal_settings_label(self.ui_language, "section")),
            )
            .child(self.render_picker_row(
                terminal_settings_label(self.ui_language, "launch_command"),
                launch_cli.label(),
                FontRole::TerminalLaunchCommand,
                0,
                cx,
            ))
            .child(self.render_terminal_command_args_row())
            .child(self.render_picker_row(
                terminal_settings_label(self.ui_language, "cursor_shape"),
                current,
                FontRole::TerminalCursorShape,
                0,
                cx,
            ))
            .child(self.render_attachment_settings_row(
                terminal_settings_label(self.ui_language, "copy_on_select"),
                self.render_boolean_settings_checkbox(
                    "settings-terminal-copy-on-select",
                    self.app_settings.terminal_copy_on_select,
                    SettingsOptionAction::TerminalCopyOnSelect(false),
                    SettingsOptionAction::TerminalCopyOnSelect(true),
                    cx,
                ),
            ))
            .into_any_element()
    }

    fn render_terminal_command_args_row(&self) -> AnyElement {
        let settings = &self.styles.settings_panel;
        let control = div()
            .w_full()
            .h(px(self.scaled(settings.settings_input_height_px)))
            .border(px(settings.settings_option_button_border_width_px))
            .border_color(settings.settings_option_button_border_color)
            .rounded(px(self.scaled(settings.settings_input_radius_px)))
            .bg(settings.settings_option_button_bg)
            .overflow_hidden()
            .when_some(
                self.settings_terminal_command_args_editor.clone(),
                |this, editor| this.child(editor),
            )
            .into_any_element();
        self.render_attachment_settings_row(
            terminal_settings_label(self.ui_language, "command_args"),
            control,
        )
    }

    fn ensure_terminal_command_args_editor(&mut self, cx: &mut Context<Self>) {
        if self.settings_terminal_command_args_editor.is_some() {
            return;
        }
        let mut styles = self.settings_text_input_styles();
        styles.text_input_center = false;
        styles.text_input_padding_left_px = self.scaled(8.0);
        styles.text_input_padding_right_px = self.scaled(8.0);
        let initial = self.app_settings.terminal_command_args.clone();
        let editor = cx.new(move |cx| {
            let mut input = crate::text_input::TextInput::new(styles, "copilot --help", cx);
            input.set_text(&initial, cx);
            input
        });
        cx.subscribe(
            &editor,
            |this, editor, event: &crate::text_input::TextInputEvent, cx| {
                if matches!(
                    event,
                    crate::text_input::TextInputEvent::Changed
                        | crate::text_input::TextInputEvent::Submit
                ) {
                    this.app_settings.terminal_command_args = editor.read(cx).text().to_string();
                    this.app_settings.save();
                    cx.notify();
                }
            },
        )
        .detach();
        self.settings_terminal_command_args_editor = Some(editor);
    }

    fn render_attachment_settings_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.ensure_attachment_folder_editor(cx);
        let ui_language = self.ui_language;
        let label = |key| attachment_settings_label(ui_language, key);
        let location = self.app_settings.attachment_location.clone();
        let location_controls =
            self.render_picker_control(label(&location), FontRole::AttachmentLocation, 0, cx);

        div()
            .flex()
            .flex_col()
            .gap(px(self.scaled(12.0)))
            .child(self.render_section_heading(label("section")))
            .child(self.render_attachment_settings_row(label("location"), location_controls))
            .when(
                matches!(
                    location.as_str(),
                    "workspace_folder" | "current_file_subfolder"
                ),
                |this| this.child(self.render_attachment_folder_row()),
            )
            .into_any_element()
    }

    fn render_attachment_settings_row(&self, label: &str, control: AnyElement) -> AnyElement {
        div()
            .flex()
            .items_center()
            .gap(px(self.scaled(12.0)))
            .child(
                div()
                    .w(px(self.scaled(120.0)))
                    .text_size(px(self.scaled(13.0)))
                    .text_color(self.styles.settings_panel.settings_secondary_text_color)
                    .child(label.to_string()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .flex()
                    .items_center()
                    .child(control),
            )
            .into_any_element()
    }

    fn render_attachment_folder_row(&self) -> AnyElement {
        let settings = &self.styles.settings_panel;
        let editor = self.settings_attachment_folder_editor.clone();
        let control = div()
            .w_full()
            .h(px(self.scaled(settings.settings_input_height_px)))
            .border(px(settings.settings_option_button_border_width_px))
            .border_color(settings.settings_option_button_border_color)
            .rounded(px(self.scaled(settings.settings_input_radius_px)))
            .bg(settings.settings_option_button_bg)
            .overflow_hidden()
            .when_some(editor, |this, editor| this.child(editor))
            .into_any_element();
        self.render_attachment_settings_row(
            attachment_settings_label(self.ui_language, "folder"),
            control,
        )
    }

    fn ensure_attachment_folder_editor(&mut self, cx: &mut Context<Self>) {
        if self.settings_attachment_folder_editor.is_some() {
            return;
        }
        let mut styles = self.settings_text_input_styles();
        styles.text_input_center = false;
        styles.text_input_padding_left_px = self.scaled(8.0);
        styles.text_input_padding_right_px = self.scaled(8.0);
        let initial = self.app_settings.attachment_folder.clone();
        let editor = cx.new(move |cx| {
            let mut input = crate::text_input::TextInput::new(styles, "attachments", cx);
            input.set_text(&initial, cx);
            input
        });
        cx.subscribe(
            &editor,
            |this, editor, event: &crate::text_input::TextInputEvent, cx| match event {
                crate::text_input::TextInputEvent::Submit => {
                    let text = editor.read(cx).text().to_string();
                    if let Some(folder) = attachments::validated_attachment_folder(&text) {
                        this.app_settings.attachment_folder = folder;
                        this.finalize_settings_change(cx);
                    } else {
                        let current = this.app_settings.attachment_folder.clone();
                        editor.update(cx, |input, cx| input.set_text(&current, cx));
                    }
                }
                crate::text_input::TextInputEvent::Escape => {
                    let current = this.app_settings.attachment_folder.clone();
                    editor.update(cx, |input, cx| input.set_text(&current, cx));
                }
                _ => {}
            },
        )
        .detach();
        self.settings_attachment_folder_editor = Some(editor);
    }

    fn render_editor_display_options(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_col()
            .gap(px(self.scaled(8.0)))
            .child(self.render_attachment_settings_row(
                crate::i18n::settings_label(self.ui_language, "show_whitespace"),
                self.render_boolean_settings_checkbox(
                    "settings-show-whitespace",
                    self.app_settings.show_whitespace,
                    SettingsOptionAction::ShowWhitespace(false),
                    SettingsOptionAction::ShowWhitespace(true),
                    cx,
                ),
            ))
            .child(self.render_attachment_settings_row(
                crate::i18n::settings_label(self.ui_language, "highlight_line"),
                self.render_boolean_settings_checkbox(
                    "settings-highlight-line-background",
                    self.app_settings.highlight_line_background,
                    SettingsOptionAction::HighlightLine(false),
                    SettingsOptionAction::HighlightLine(true),
                    cx,
                ),
            ))
            .into_any_element()
    }

    fn render_boolean_settings_checkbox(
        &self,
        id_prefix: &'static str,
        enabled: bool,
        disabled_action: SettingsOptionAction,
        enabled_action: SettingsOptionAction,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_settings_checkbox(
            id_prefix,
            enabled,
            crate::i18n::boolean_option_label(self.ui_language, enabled),
            if enabled {
                disabled_action
            } else {
                enabled_action
            },
            cx,
        )
    }

    fn render_settings_checkbox(
        &self,
        id: &'static str,
        checked: bool,
        value_label: &'static str,
        action: SettingsOptionAction,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let styles = &self.styles.settings_panel;
        Checkbox::new(
            id,
            checked,
            value_label,
            self.ui_scale,
            CheckboxColors {
                checked_background: self.theme.text_primary,
                checked_hover_background: self.theme.text_bright,
                unchecked_background: styles.settings_option_button_bg,
                unchecked_hover_background: styles.settings_option_button_hover_bg,
                unchecked_border: styles.settings_contrast_border_color,
                border_width_px: self.styles.checkbox.checkbox_border_width_px,
                thumb: self.theme.bg_primary,
                description: styles.settings_theme_option_text_color,
            },
        )
        .render()
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, _, cx| {
                this.apply_settings_option_action(action, cx);
            }),
        )
        .into_any_element()
    }

    fn apply_settings_option_action(
        &mut self,
        action: SettingsOptionAction,
        cx: &mut Context<Self>,
    ) {
        match action {
            SettingsOptionAction::ShowWhitespace(value) => {
                self.app_settings.show_whitespace = value;
            }
            SettingsOptionAction::HighlightLine(value) => {
                self.app_settings.highlight_line_background = value;
            }
            SettingsOptionAction::SimpleMode(value) => {
                self.app_settings.simple_mode = value;
            }
            SettingsOptionAction::TerminalCopyOnSelect(value) => {
                self.app_settings.terminal_copy_on_select = value;
            }
        }
        self.finalize_settings_change(cx);
    }

    fn date_time_format_options(&self) -> Vec<PickerOption> {
        let now = chrono::Local::now();
        let formats = [
            "%Y-%m-%d %H:%M",
            "%Y/%m/%d %H:%M",
            "%m/%d/%Y %I:%M %p",
            "%d/%m/%Y %H:%M",
            "%d.%m.%Y %H:%M",
            "%d %b %Y %H:%M",
            "%B %d, %Y %I:%M %p",
            "%a, %d %b %Y %H:%M:%S %z",
            "%Y-%m-%dT%H:%M:%S%:z",
        ];

        formats
            .iter()
            .map(|format| {
                let preview = now.format(format).to_string();
                PickerOption {
                    value: (*format).to_string(),
                    label: preview.clone(),
                    search_text: format!("{preview} {format}").to_lowercase(),
                    preview_font: None,
                    icon_path: None,
                }
            })
            .collect()
    }

    fn date_time_format_preview_label(&self, format: &str) -> String {
        chrono::Local::now().format(format).to_string()
    }

    // ── Section heading ─────────────────────────────────────────

    fn render_section_heading(&self, label: &str) -> AnyElement {
        let border_color = self.styles.settings_panel.settings_contrast_border_color;
        let heading_label = if self.ui_language == UiLanguage::English {
            label.to_uppercase()
        } else {
            label.to_string()
        };
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(self.scaled(8.0)))
            .child(
                div()
                    .text_size(px(self.scaled(self.styles.ui_font_size_px - 1.0)))
                    .font_weight(gpui::FontWeight::BOLD)
                    .text_color(self.styles.settings_panel.settings_primary_text_color)
                    .child(heading_label),
            )
            .child(div().w_full().h(px(1.0)).bg(border_color))
            .into_any_element()
    }

    fn settings_text_input_styles(&self) -> crate::style::TextInputStyles {
        let mut styles = self.styles.text_input.clone();
        let settings = &self.styles.settings_panel;
        styles.text_input_bg = settings.settings_option_button_bg;
        styles.text_input_hover_bg = Some(settings.settings_option_button_hover_bg);
        styles.text_input_focus_bg = settings.settings_option_button_hover_bg;
        styles.text_input_border_width_px = 0.0;
        styles.text_input_height_px = self.scaled(settings.settings_input_height_px);
        styles.text_input_padding_left_px = self.scaled(8.0);
        styles.text_input_padding_right_px = self.scaled(8.0);
        styles.text_input_font_size_px = self.scaled(self.styles.ui_font_size_px + 1.0);
        styles.text_input_radius_px = self.scaled(settings.settings_input_radius_px);
        styles.text_input_cursor_width_px =
            self.scaled(self.styles.text_input.text_input_cursor_width_px);
        styles.text_input_cursor_height_ratio = self
            .styles
            .workspace_search
            .ws_search_input_cursor_height_ratio;
        styles.text_input_cursor_height_adjust_px = self.scaled(
            self.styles
                .workspace_search
                .ws_search_input_cursor_height_adjust_px,
        );
        styles
    }

    fn inline_stepper_input_styles(&self) -> crate::style::TextInputStyles {
        let mut styles = self.settings_text_input_styles();
        styles.text_input_padding_left_px = self.scaled(2.0);
        styles.text_input_padding_right_px = self.scaled(2.0);
        styles.text_input_radius_px =
            self.scaled(self.styles.settings_panel.settings_stepper_input_radius_px);
        styles.text_input_center = true;
        styles
    }

    pub(crate) fn sync_settings_text_input_styles(&mut self, cx: &mut Context<Self>) {
        let stepper_styles = self.inline_stepper_input_styles();
        for editor in [
            self.settings_font_size_editor.as_ref(),
            self.settings_ui_font_size_editor.as_ref(),
            self.settings_line_height_editor.as_ref(),
            self.settings_letter_spacing_editor.as_ref(),
            self.settings_editor_content_width_editor.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            let styles = stepper_styles.clone();
            editor.update(cx, |input, cx| input.set_styles(styles, cx));
        }

        for editor in [
            self.settings_attachment_folder_editor.as_ref(),
            self.settings_terminal_command_args_editor.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            let mut styles = self.settings_text_input_styles();
            styles.text_input_center = false;
            editor.update(cx, |input, cx| input.set_styles(styles, cx));
        }
        if let Some(editor) = &self.settings_font_picker_editor {
            let styles = self.search_menu_text_input_styles();
            editor.update(cx, |input, cx| input.set_styles(styles, cx));
        }
    }

    fn build_inline_stepper_editor(
        &self,
        initial_value: String,
        cx: &mut Context<Self>,
    ) -> gpui::Entity<crate::text_input::TextInput> {
        let styles = self.inline_stepper_input_styles();
        cx.new(move |cx| {
            let mut ti = crate::text_input::TextInput::new(styles, "", cx);
            ti.set_text(&initial_value, cx);
            ti
        })
    }

    fn sync_inline_stepper_editor(
        &self,
        editor: &gpui::Entity<crate::text_input::TextInput>,
        display: String,
        cx: &mut Context<Self>,
    ) {
        editor.update(cx, |ti, cx| {
            if ti.text() != display {
                ti.set_text(&display, cx);
            }
        });
    }

    pub(crate) fn finalize_settings_change(&mut self, cx: &mut Context<Self>) {
        self.app_settings.save();
        self.clear_all_tab_layout_caches();
        cx.notify();
    }

    // ── Theme section ───────────────────────────────────────────

    fn render_theme_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.theme_kind;
        let is_dark_mode = self.settings_theme_dark_mode;
        let editor_theme_mode = self.editor_theme_mode;
        let settings_styles = &self.styles.settings_panel;
        let themes = if is_dark_mode {
            ThemeKind::dark_themes()
        } else {
            ThemeKind::light_themes()
        };
        let text_secondary = settings_styles.settings_option_button_text_color;
        let hover_bg = settings_styles.settings_option_button_hover_bg;
        let toggle_active_bg = settings_styles.settings_option_button_active_bg;
        let toggle_active_text = settings_styles.settings_option_button_active_text_color;
        let icon_size = self.scaled(settings_styles.settings_theme_toggle_icon_size_px);
        let toggle_height = self.scaled(settings_styles.settings_theme_toggle_height_px);
        let toggle_padding_x = self.scaled(settings_styles.settings_theme_toggle_padding_x_px);
        let toggle_gap = self.scaled(settings_styles.settings_theme_toggle_gap_px);
        let toggle_radius = self.scaled(settings_styles.settings_theme_toggle_radius_px);
        let toggle_border_width =
            self.scaled(settings_styles.settings_theme_toggle_border_width_px);
        let option_height = self.scaled(settings_styles.settings_theme_option_height_px);
        let option_padding_x = self.scaled(settings_styles.settings_theme_option_padding_x_px);
        let option_radius = self.scaled(settings_styles.settings_theme_option_radius_px);
        let option_border_width =
            self.scaled(settings_styles.settings_theme_option_border_width_px);
        let option_font_size = self.scaled(settings_styles.settings_theme_option_font_size_px);
        let option_border_color = settings_styles.settings_theme_option_border_color;
        let option_text_color = settings_styles.settings_theme_option_text_color;
        let option_active_text_color = settings_styles.settings_theme_option_active_text_color;
        let option_active_bg = settings_styles.settings_theme_option_active_bg;
        let option_hover_bg = settings_styles.settings_theme_option_hover_bg;

        div()
            .flex()
            .flex_col()
            .gap(px(self.scaled(16.0)))
            .child(self.render_section_heading(self.t(TextKey::ThemeSection)))
            .child(self.render_attachment_settings_row(
                crate::i18n::settings_label(self.ui_language, "simple_mode"),
                self.render_boolean_settings_checkbox(
                    "settings-simple-mode",
                    self.app_settings.simple_mode,
                    SettingsOptionAction::SimpleMode(false),
                    SettingsOptionAction::SimpleMode(true),
                    cx,
                ),
            ))
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(self.scaled(settings_styles.settings_theme_group_gap_px)))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(0.0))
                            .child(
                                div()
                                    .id("theme-toggle-light")
                                    .rounded_l(px(toggle_radius))
                                    .border(px(toggle_border_width))
                                    .border_color(option_border_color)
                                    .h(px(toggle_height))
                                    .px(px(toggle_padding_x))
                                    .flex()
                                    .items_center()
                                    .gap(px(toggle_gap))
                                    .text_size(px(option_font_size))
                                    .bg(settings_styles.settings_option_button_bg)
                                    .cursor(CursorStyle::Arrow)
                                    .when(!is_dark_mode, |this| {
                                        this.bg(toggle_active_bg).text_color(toggle_active_text)
                                    })
                                    .when(is_dark_mode, |this| {
                                        this.text_color(text_secondary)
                                            .hover(move |s| s.bg(hover_bg))
                                    })
                                    .child(
                                        svg()
                                            .path(THEME_LIGHT_MINI_SVG_PATH)
                                            .size(px(icon_size))
                                            .text_color(if !is_dark_mode {
                                                toggle_active_text
                                            } else {
                                                text_secondary
                                            }),
                                    )
                                    .child(self.t(TextKey::ThemeLight))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            if this.settings_theme_dark_mode {
                                                let light_theme =
                                                    this.app_settings.light_theme_kind();
                                                this.set_theme(light_theme, cx);
                                            }
                                        }),
                                    ),
                            )
                            .child(
                                div()
                                    .id("theme-toggle-dark")
                                    .rounded_r(px(toggle_radius))
                                    .border_t(px(toggle_border_width))
                                    .border_r(px(toggle_border_width))
                                    .border_b(px(toggle_border_width))
                                    .border_color(option_border_color)
                                    .h(px(toggle_height))
                                    .px(px(toggle_padding_x))
                                    .flex()
                                    .items_center()
                                    .gap(px(toggle_gap))
                                    .text_size(px(option_font_size))
                                    .bg(settings_styles.settings_option_button_bg)
                                    .cursor(CursorStyle::Arrow)
                                    .when(is_dark_mode, |this| {
                                        this.bg(toggle_active_bg).text_color(toggle_active_text)
                                    })
                                    .when(!is_dark_mode, |this| {
                                        this.text_color(text_secondary)
                                            .hover(move |s| s.bg(hover_bg))
                                    })
                                    .child(
                                        svg()
                                            .path(THEME_DARK_MINI_SVG_PATH)
                                            .size(px(icon_size))
                                            .text_color(if is_dark_mode {
                                                toggle_active_text
                                            } else {
                                                text_secondary
                                            }),
                                    )
                                    .child(self.t(TextKey::ThemeDark))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            if !this.settings_theme_dark_mode {
                                                let dark_theme =
                                                    this.app_settings.dark_theme_kind();
                                                this.set_theme(dark_theme, cx);
                                            }
                                        }),
                                    ),
                            ),
                    )
                    .child(
                        div().flex().flex_row().gap(px(0.0)).children(
                            EditorThemeMode::all()
                                .iter()
                                .enumerate()
                                .map(|(index, mode)| {
                                    let is_active = *mode == editor_theme_mode;
                                    let mode = *mode;
                                    div()
                                        .id(SharedString::from(format!(
                                            "editor-theme-mode-{}",
                                            mode.as_str()
                                        )))
                                        .h(px(option_height))
                                        .border_t(px(option_border_width))
                                        .border_b(px(option_border_width))
                                        .border_color(option_border_color)
                                        .px(px(option_padding_x))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .gap(px(toggle_gap))
                                        .text_size(px(option_font_size))
                                        .text_color(if is_active {
                                            option_active_text_color
                                        } else {
                                            option_text_color
                                        })
                                        .bg(if is_active {
                                            option_active_bg
                                        } else {
                                            settings_styles.settings_option_button_bg
                                        })
                                        .cursor(CursorStyle::Arrow)
                                        .when(!is_active, |this| {
                                            this.hover(move |s| s.bg(option_hover_bg))
                                        })
                                        .when(index > 0, |this| {
                                            this.border_l(px(option_border_width))
                                                .border_color(option_border_color)
                                        })
                                        .when(index == 0, |this| {
                                            this.rounded_l(px(option_radius))
                                                .border_l(px(option_border_width))
                                        })
                                        .when(index + 1 == EditorThemeMode::all().len(), |this| {
                                            this.rounded_r(px(option_radius))
                                                .border_r(px(option_border_width))
                                        })
                                        .child(
                                            svg()
                                                .path(match mode {
                                                    EditorThemeMode::Colorful => FLOWER_SVG_PATH,
                                                    EditorThemeMode::Monochrome => {
                                                        MOUNTAIN_SVG_PATH
                                                    }
                                                })
                                                .size(px(icon_size))
                                                .text_color(if is_active {
                                                    option_active_text_color
                                                } else {
                                                    option_text_color
                                                }),
                                        )
                                        .child(match mode {
                                            EditorThemeMode::Colorful => {
                                                self.t(TextKey::ThemeModeColorful)
                                            }
                                            EditorThemeMode::Monochrome => {
                                                self.t(TextKey::ThemeModeMonochrome)
                                            }
                                        })
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(move |this, _, _, cx| {
                                                this.set_editor_theme_mode(mode, cx);
                                            }),
                                        )
                                        .into_any_element()
                                }),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .justify_between()
                    .gap_y(px(self.scaled(8.0)))
                    .children(
                        themes
                            .iter()
                            .map(|&kind| self.render_theme_card(kind, kind == current, cx)),
                    ),
            )
            .into_any_element()
    }

    fn render_theme_card(
        &self,
        kind: ThemeKind,
        is_active: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let preview = kind.build();
        let card_bg = preview.bg_primary;
        let swatch_text = preview.text_primary;
        let swatch_accent = preview.text_accent;
        let swatch_heading = preview.heading;
        let swatch_code = preview.inline_code;
        let swatch_secondary_bg = preview.bg_secondary;
        let active_accent = self.theme.text_accent;
        let inactive_border = preview.border_secondary;
        let inactive_hover_border = preview.border_tertiary;

        let swatch_size = self.scaled(10.0);

        div()
            .id(SharedString::from(format!("theme-{}", kind.label())))
            // Four cards per row; space-between distributes the remaining 6%.
            .w(relative(0.235))
            .py(px(self.scaled(8.0)))
            .px(px(self.scaled(8.0)))
            .flex()
            .flex_col()
            .gap(px(self.scaled(6.0)))
            .rounded(px(self.scaled(8.0)))
            .border(px(2.0))
            .border_color(if is_active {
                active_accent
            } else {
                inactive_border
            })
            .bg(card_bg)
            .cursor(CursorStyle::Arrow)
            .hover(move |style| {
                if !is_active {
                    style.border_color(inactive_hover_border)
                } else {
                    style
                }
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.set_theme(kind, cx);
                }),
            )
            // Color swatches row (reversed order, circular)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(self.scaled(4.0)))
                    .child(
                        div()
                            .w(px(swatch_size))
                            .h(px(swatch_size))
                            .rounded_full()
                            .bg(swatch_code),
                    )
                    .child(
                        div()
                            .w(px(swatch_size))
                            .h(px(swatch_size))
                            .rounded_full()
                            .bg(swatch_heading),
                    )
                    .child(
                        div()
                            .w(px(swatch_size))
                            .h(px(swatch_size))
                            .rounded_full()
                            .bg(swatch_accent),
                    )
                    .child(
                        div()
                            .w(px(swatch_size))
                            .h(px(swatch_size))
                            .rounded_full()
                            .bg(swatch_text),
                    )
                    .child(
                        div()
                            .w(px(swatch_size))
                            .h(px(swatch_size))
                            .rounded_full()
                            .bg(swatch_secondary_bg),
                    ),
            )
            // Theme name
            .child(
                div()
                    .w_full()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_ellipsis()
                    .text_size(px(self.scaled(11.0)))
                    .text_color(swatch_text)
                    .child(kind.label()),
            )
            .into_any_element()
    }

    // ── Font section ────────────────────────────────────────────

    fn render_font_section(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        self.ensure_font_size_editor(cx);
        self.ensure_ui_font_size_editor(cx);
        self.ensure_line_height_editor(cx);
        self.ensure_letter_spacing_editor(cx);
        self.ensure_editor_content_width_editor(cx);
        self.ensure_settings_stepper_blur_subscriptions(window, cx);

        let ui_font = self.styles.ui_font_family;
        let body_latin = self.styles.editor.editor_body_latin_font_family;
        let body_cjk = self.styles.editor.editor_body_cjk_font_family;
        let monospace = self.styles.editor.editor_monospace_font_family;

        div()
            .flex()
            .flex_col()
            .gap(px(self.scaled(12.0)))
            .child(self.render_section_heading(self.t(TextKey::FontsSection)))
            // UI font
            .child(self.render_picker_row(
                self.t(TextKey::UiFont),
                ui_font,
                FontRole::UiFont,
                0,
                cx,
            ))
            // Interface language
            .child(self.render_picker_row(
                self.t(TextKey::InterfaceLanguage),
                &self.ui_language.picker_label_for(self.ui_language),
                FontRole::InterfaceLanguage,
                0,
                cx,
            ))
            // UI scale
            .child(self.render_ui_font_size_row(cx))
            // Body font (Latin)
            .child(self.render_picker_row(
                self.t(TextKey::EditorFont),
                body_latin,
                FontRole::BodyFont,
                0,
                cx,
            ))
            // Body font (CJK)
            .child(self.render_picker_row(
                self.t(TextKey::EditorFontCjk),
                body_cjk,
                FontRole::BodyFont,
                1,
                cx,
            ))
            // Code font
            .child(self.render_picker_row(
                self.t(TextKey::CodeFont),
                monospace,
                FontRole::CodeFont,
                0,
                cx,
            ))
            // Line height
            .child(self.render_line_height_row(cx))
            // Letter spacing
            .child(self.render_letter_spacing_row(cx))
            // Editor content width
            .child(self.render_editor_content_width_row(cx))
            .into_any_element()
    }

    fn ensure_settings_stepper_blur_subscriptions(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.settings_stepper_blur_subscriptions.is_empty() {
            return;
        }

        let font_size_editor = self.settings_font_size_editor.clone().unwrap();
        let focus_handle = font_size_editor.read(cx).focus_handle.clone();
        self.settings_stepper_blur_subscriptions.push(cx.on_blur(
            &focus_handle,
            window,
            move |this, _, cx| {
                let text = font_size_editor.read(cx).text().to_string();
                if let Some(value) = text.parse::<f32>().ok().filter(|value| value.is_finite()) {
                    this.apply_font_size(value, cx);
                } else {
                    let current = this.app_settings.font_size;
                    this.sync_inline_stepper_editor(&font_size_editor, format!("{current:.0}"), cx);
                }
            },
        ));

        let ui_scale_editor = self.settings_ui_font_size_editor.clone().unwrap();
        let focus_handle = ui_scale_editor.read(cx).focus_handle.clone();
        self.settings_stepper_blur_subscriptions.push(cx.on_blur(
            &focus_handle,
            window,
            move |this, _, cx| {
                let text = ui_scale_editor.read(cx).text().to_string();
                if let Some(value) = text.parse::<f32>().ok().filter(|value| value.is_finite()) {
                    this.apply_ui_scale(value, cx);
                } else {
                    this.sync_inline_stepper_editor(
                        &ui_scale_editor,
                        format!("{:.1}", this.ui_scale),
                        cx,
                    );
                }
            },
        ));

        let line_height_editor = self.settings_line_height_editor.clone().unwrap();
        let focus_handle = line_height_editor.read(cx).focus_handle.clone();
        self.settings_stepper_blur_subscriptions.push(cx.on_blur(
            &focus_handle,
            window,
            move |this, _, cx| {
                let text = line_height_editor.read(cx).text().to_string();
                if let Some(value) = text.parse::<f32>().ok().filter(|value| value.is_finite()) {
                    this.apply_line_height(value, cx);
                } else {
                    let current = this.app_settings.line_height;
                    this.sync_inline_stepper_editor(
                        &line_height_editor,
                        format!("{current:.1}"),
                        cx,
                    );
                }
            },
        ));

        let letter_spacing_editor = self.settings_letter_spacing_editor.clone().unwrap();
        let focus_handle = letter_spacing_editor.read(cx).focus_handle.clone();
        self.settings_stepper_blur_subscriptions.push(cx.on_blur(
            &focus_handle,
            window,
            move |this, _, cx| {
                let text = letter_spacing_editor.read(cx).text().to_string();
                if let Some(value) = text.parse::<f32>().ok().filter(|value| value.is_finite()) {
                    this.apply_letter_spacing(value, cx);
                } else {
                    let current = this.app_settings.letter_spacing;
                    this.sync_inline_stepper_editor(
                        &letter_spacing_editor,
                        format!("{current:.1}"),
                        cx,
                    );
                }
            },
        ));

        let content_width_editor = self.settings_editor_content_width_editor.clone().unwrap();
        let focus_handle = content_width_editor.read(cx).focus_handle.clone();
        self.settings_stepper_blur_subscriptions.push(cx.on_blur(
            &focus_handle,
            window,
            move |this, _, cx| {
                let text = content_width_editor.read(cx).text().to_string();
                if let Some(value) = text.parse::<f32>().ok().filter(|value| value.is_finite()) {
                    this.apply_editor_content_width(value, cx);
                } else {
                    let current = this.app_settings.editor_content_width;
                    this.sync_inline_stepper_editor(
                        &content_width_editor,
                        format!("{current:.0}"),
                        cx,
                    );
                }
            },
        ));
    }

    fn render_picker_row(
        &mut self,
        label: &str,
        current_value: &str,
        role: FontRole,
        option_index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let text_secondary = self.styles.settings_panel.settings_secondary_text_color;
        let control = self.render_picker_control(current_value, role, option_index, cx);

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(self.scaled(12.0)))
            .child(
                div()
                    .w(px(self.scaled(120.0)))
                    .text_size(px(self.scaled(13.0)))
                    .text_color(text_secondary)
                    .child(label.to_string()),
            )
            .child(control)
            .into_any_element()
    }

    fn render_picker_control(
        &mut self,
        current_value: &str,
        role: FontRole,
        option_index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = &self.styles.settings_panel;
        let hover_bg = settings.settings_option_button_hover_bg;
        let text_primary = settings.settings_option_button_text_color;
        let border_color = settings.settings_option_button_border_color;
        let bg = settings.settings_option_button_bg;
        let current_value = current_value.to_string();
        let bounds_cell = self
            .font_picker_button_bounds
            .entry((role, option_index))
            .or_insert_with(|| Rc::new(Cell::new(None)))
            .clone();

        div()
            .flex_1()
            .on_children_prepainted({
                let bounds_cell = bounds_cell.clone();
                move |bounds: Vec<gpui::Bounds<gpui::Pixels>>, _, _| {
                    if let Some(bounds) = bounds.first() {
                        bounds_cell.set(Some(*bounds));
                    }
                }
            })
            .child(
                div()
                    .id(SharedString::from(format!(
                        "font-picker-{:?}-{}",
                        role as u8, option_index
                    )))
                    .w_full()
                    .h(px(self.scaled(settings.settings_input_height_px)))
                    .px(px(self.scaled(10.0)))
                    .flex()
                    .items_center()
                    .rounded(px(self.scaled(settings.settings_input_radius_px)))
                    .border(px(settings.settings_option_button_border_width_px))
                    .border_color(border_color)
                    .bg(bg)
                    .text_size(px(self.scaled(13.0)))
                    .text_color(text_primary)
                    .cursor(CursorStyle::Arrow)
                    .hover(move |style| style.bg(hover_bg))
                    .child(current_value)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &gpui::MouseDownEvent, window, cx| {
                            window.prevent_default();
                            this.open_font_picker(role, option_index, window, cx);
                        }),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn apply_font_size(&mut self, size: f32, cx: &mut Context<Self>) {
        let size = size.clamp(10.0, 30.0);
        self.app_settings.font_size = size;
        self.styles.editor.editor_font_size_px = self.app_settings.effective_editor_font_size();
        if let Some(editor) = &self.settings_font_size_editor {
            self.sync_inline_stepper_editor(editor, format!("{size:.0}"), cx);
        }
        self.finalize_settings_change(cx);
    }

    pub(crate) fn ensure_font_size_editor(&mut self, cx: &mut Context<Self>) {
        if self.settings_font_size_editor.is_some() {
            return;
        }
        let editor =
            self.build_inline_stepper_editor(format!("{:.0}", self.app_settings.font_size), cx);
        cx.subscribe(
            &editor,
            |this, editor, event: &crate::text_input::TextInputEvent, cx| match event {
                crate::text_input::TextInputEvent::Submit => {
                    let text = editor.read(cx).text().to_string();
                    if let Some(size) = text.parse::<f32>().ok().filter(|value| value.is_finite()) {
                        this.apply_font_size(size, cx);
                    }
                }
                crate::text_input::TextInputEvent::Escape => {
                    let current = this.app_settings.font_size;
                    this.sync_inline_stepper_editor(&editor, format!("{current:.0}"), cx);
                }
                crate::text_input::TextInputEvent::Changed => {}
                _ => {}
            },
        )
        .detach();
        self.settings_font_size_editor = Some(editor);
    }

    fn render_ui_font_size_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let text_secondary = self.styles.settings_panel.settings_secondary_text_color;
        let border_color = self.styles.settings_panel.settings_contrast_border_color;

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(self.scaled(12.0)))
            .child(
                div()
                    .w(px(self.scaled(120.0)))
                    .text_size(px(self.scaled(13.0)))
                    .text_color(text_secondary)
                    .child(self.t(TextKey::UiScale)),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(0.0))
                    .child(self.render_settings_stepper_button(
                        "ui-scale-dec",
                        "−",
                        false,
                        Some("⌘-"),
                        cx,
                        |this, cx| {
                            let current = this.ui_scale;
                            this.apply_ui_scale(current - 0.1, cx);
                        },
                    ))
                    .child(
                        div()
                            .w(px(self.scaled(50.0)))
                            .h(px(self.scaled(30.0)))
                            .border_y(px(self.scaled(
                                self.styles
                                    .settings_panel
                                    .settings_option_button_border_width_px,
                            )))
                            .border_color(border_color)
                            .flex()
                            .items_center()
                            .justify_center()
                            .when_some(
                                self.settings_ui_font_size_editor.clone(),
                                |this, editor| this.child(editor),
                            ),
                    )
                    .child(self.render_settings_stepper_button(
                        "ui-scale-inc",
                        "+",
                        true,
                        Some("⌘+"),
                        cx,
                        |this, cx| {
                            let current = this.ui_scale;
                            this.apply_ui_scale(current + 0.1, cx);
                        },
                    )),
            )
            .into_any_element()
    }

    fn render_settings_stepper_button(
        &self,
        id: &'static str,
        symbol: &'static str,
        increase: bool,
        shortcut: Option<&'static str>,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let hover_bg = self.theme.bg_hover;
        let button = div()
            .id(id)
            .w(px(self.scaled(30.0)))
            .h(px(self.scaled(30.0)))
            .flex()
            .items_center()
            .justify_center()
            .when(increase, |this| this.rounded_r(px(self.scaled(6.0))))
            .when(!increase, |this| this.rounded_l(px(self.scaled(6.0))))
            .border(px(self.scaled(
                self.styles
                    .settings_panel
                    .settings_option_button_border_width_px,
            )))
            .border_color(self.styles.settings_panel.settings_contrast_border_color)
            .text_size(px(self.scaled(14.0)))
            .text_color(self.styles.settings_panel.settings_primary_text_color)
            .cursor(CursorStyle::Arrow)
            .hover(move |style| style.bg(hover_bg))
            .child(symbol)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| on_click(this, cx)),
            );
        tooltip::with_tooltip(
            button,
            cx.entity().clone(),
            crate::i18n::adjustment_label(self.ui_language, increase),
            shortcut,
        )
    }

    pub(crate) fn apply_ui_scale(&mut self, scale: f32, cx: &mut Context<Self>) {
        let scale = (scale * 10.0).round() / 10.0; // Round to 0.1
        self.set_ui_scale(scale, cx);
        self.app_settings.ui_scale = self.ui_scale;
        if let Some(editor) = &self.settings_ui_font_size_editor {
            self.sync_inline_stepper_editor(editor, format!("{:.1}", self.ui_scale), cx);
        }
        self.finalize_settings_change(cx);
    }

    pub(crate) fn ensure_ui_font_size_editor(&mut self, cx: &mut Context<Self>) {
        if self.settings_ui_font_size_editor.is_some() {
            return;
        }
        let editor = self.build_inline_stepper_editor(format!("{:.1}", self.ui_scale), cx);
        cx.subscribe(
            &editor,
            |this, editor, event: &crate::text_input::TextInputEvent, cx| match event {
                crate::text_input::TextInputEvent::Submit => {
                    let text = editor.read(cx).text().to_string();
                    if let Some(scale) = text.parse::<f32>().ok().filter(|value| value.is_finite())
                    {
                        this.apply_ui_scale(scale, cx);
                    }
                }
                crate::text_input::TextInputEvent::Escape => {
                    let current = this.ui_scale;
                    this.sync_inline_stepper_editor(&editor, format!("{current:.1}"), cx);
                }
                crate::text_input::TextInputEvent::Changed => {}
                _ => {}
            },
        )
        .detach();
        self.settings_ui_font_size_editor = Some(editor);
    }

    // ── Line height stepper ─────────────────────────────────────

    fn render_line_height_row(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.ensure_line_height_editor(cx);
        let text_secondary = self.styles.settings_panel.settings_secondary_text_color;
        let border_color = self.styles.settings_panel.settings_contrast_border_color;

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(self.scaled(12.0)))
            .child(
                div()
                    .w(px(self.scaled(120.0)))
                    .text_size(px(self.scaled(13.0)))
                    .text_color(text_secondary)
                    .child(self.t(TextKey::LineHeight)),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(0.0))
                    .child(self.render_settings_stepper_button(
                        "line-height-dec",
                        "−",
                        false,
                        None,
                        cx,
                        |this, cx| {
                            let current = this.app_settings.line_height;
                            this.apply_line_height(current - 0.1, cx);
                        },
                    ))
                    .child(
                        div()
                            .w(px(self.scaled(50.0)))
                            .h(px(self.scaled(30.0)))
                            .border_y(px(self.scaled(
                                self.styles
                                    .settings_panel
                                    .settings_option_button_border_width_px,
                            )))
                            .border_color(border_color)
                            .flex()
                            .items_center()
                            .justify_center()
                            .when_some(self.settings_line_height_editor.clone(), |this, editor| {
                                this.child(editor)
                            }),
                    )
                    .child(self.render_settings_stepper_button(
                        "line-height-inc",
                        "+",
                        true,
                        None,
                        cx,
                        |this, cx| {
                            let current = this.app_settings.line_height;
                            this.apply_line_height(current + 0.1, cx);
                        },
                    )),
            )
            .into_any_element()
    }

    pub(crate) fn apply_line_height(&mut self, value: f32, cx: &mut Context<Self>) {
        let value = ((value * 10.0).round() / 10.0).clamp(1.0, 3.0);
        self.styles.editor.editor_paragraph_line_height_factor = value;
        self.styles.editor.editor_heading_line_height_factor = value;
        self.app_settings.line_height = value;
        if let Some(editor) = &self.settings_line_height_editor {
            self.sync_inline_stepper_editor(editor, format!("{value:.1}"), cx);
        }
        self.finalize_settings_change(cx);
    }

    fn ensure_line_height_editor(&mut self, cx: &mut Context<Self>) {
        if self.settings_line_height_editor.is_some() {
            return;
        }
        let editor =
            self.build_inline_stepper_editor(format!("{:.1}", self.app_settings.line_height), cx);
        cx.subscribe(
            &editor,
            |this, editor, event: &crate::text_input::TextInputEvent, cx| match event {
                crate::text_input::TextInputEvent::Submit => {
                    let text = editor.read(cx).text().to_string();
                    if let Some(val) = text.parse::<f32>().ok().filter(|value| value.is_finite()) {
                        this.apply_line_height(val, cx);
                    }
                }
                crate::text_input::TextInputEvent::Escape => {
                    let current = this.app_settings.line_height;
                    this.sync_inline_stepper_editor(&editor, format!("{current:.1}"), cx);
                }
                _ => {}
            },
        )
        .detach();
        self.settings_line_height_editor = Some(editor);
    }

    // ── Letter spacing stepper ──────────────────────────────────

    fn render_letter_spacing_row(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.ensure_letter_spacing_editor(cx);
        let text_secondary = self.styles.settings_panel.settings_secondary_text_color;
        let border_color = self.styles.settings_panel.settings_contrast_border_color;

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(self.scaled(12.0)))
            .child(
                div()
                    .w(px(self.scaled(120.0)))
                    .text_size(px(self.scaled(13.0)))
                    .text_color(text_secondary)
                    .child(self.t(TextKey::LetterSpacing)),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(0.0))
                    .child(self.render_settings_stepper_button(
                        "letter-spacing-dec",
                        "−",
                        false,
                        None,
                        cx,
                        |this, cx| {
                            let current = this.app_settings.letter_spacing;
                            this.apply_letter_spacing(current - 0.5, cx);
                        },
                    ))
                    .child(
                        div()
                            .w(px(self.scaled(50.0)))
                            .h(px(self.scaled(30.0)))
                            .border_y(px(self.scaled(
                                self.styles
                                    .settings_panel
                                    .settings_option_button_border_width_px,
                            )))
                            .border_color(border_color)
                            .flex()
                            .items_center()
                            .justify_center()
                            .when_some(
                                self.settings_letter_spacing_editor.clone(),
                                |this, editor| this.child(editor),
                            ),
                    )
                    .child(self.render_settings_stepper_button(
                        "letter-spacing-inc",
                        "+",
                        true,
                        None,
                        cx,
                        |this, cx| {
                            let current = this.app_settings.letter_spacing;
                            this.apply_letter_spacing(current + 0.5, cx);
                        },
                    )),
            )
            .into_any_element()
    }

    pub(crate) fn apply_letter_spacing(&mut self, value: f32, cx: &mut Context<Self>) {
        let value = ((value * 2.0).round() / 2.0).clamp(-2.0, 10.0);
        self.styles.editor.editor_letter_spacing_px = value;
        self.app_settings.letter_spacing = value;
        if let Some(editor) = &self.settings_letter_spacing_editor {
            self.sync_inline_stepper_editor(editor, format!("{value:.1}"), cx);
        }
        self.finalize_settings_change(cx);
    }

    fn ensure_letter_spacing_editor(&mut self, cx: &mut Context<Self>) {
        if self.settings_letter_spacing_editor.is_some() {
            return;
        }
        let editor = self
            .build_inline_stepper_editor(format!("{:.1}", self.app_settings.letter_spacing), cx);
        cx.subscribe(
            &editor,
            |this, editor, event: &crate::text_input::TextInputEvent, cx| match event {
                crate::text_input::TextInputEvent::Submit => {
                    let text = editor.read(cx).text().to_string();
                    if let Some(val) = text.parse::<f32>().ok().filter(|value| value.is_finite()) {
                        this.apply_letter_spacing(val, cx);
                    }
                }
                crate::text_input::TextInputEvent::Escape => {
                    let current = this.app_settings.letter_spacing;
                    this.sync_inline_stepper_editor(&editor, format!("{current:.1}"), cx);
                }
                _ => {}
            },
        )
        .detach();
        self.settings_letter_spacing_editor = Some(editor);
    }

    // ── Editor content width stepper ────────────────────────────

    fn render_editor_content_width_row(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.ensure_editor_content_width_editor(cx);
        let text_secondary = self.styles.settings_panel.settings_secondary_text_color;
        let border_color = self.styles.settings_panel.settings_contrast_border_color;

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(self.scaled(12.0)))
            .child(
                div()
                    .w(px(self.scaled(120.0)))
                    .text_size(px(self.scaled(13.0)))
                    .text_color(text_secondary)
                    .child(self.t(TextKey::EditorContentWidth)),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(0.0))
                    .child(self.render_settings_stepper_button(
                        "editor-content-width-dec",
                        "−",
                        false,
                        None,
                        cx,
                        |this, cx| {
                            let current = this.app_settings.editor_content_width;
                            this.apply_editor_content_width(current - 20.0, cx);
                        },
                    ))
                    .child(
                        div()
                            .w(px(self.scaled(60.0)))
                            .h(px(self.scaled(30.0)))
                            .border_y(px(self.scaled(
                                self.styles
                                    .settings_panel
                                    .settings_option_button_border_width_px,
                            )))
                            .border_color(border_color)
                            .flex()
                            .items_center()
                            .justify_center()
                            .when_some(
                                self.settings_editor_content_width_editor.clone(),
                                |this, editor| this.child(editor),
                            ),
                    )
                    .child(self.render_settings_stepper_button(
                        "editor-content-width-inc",
                        "+",
                        true,
                        None,
                        cx,
                        |this, cx| {
                            let current = this.app_settings.editor_content_width;
                            this.apply_editor_content_width(current + 20.0, cx);
                        },
                    )),
            )
            .into_any_element()
    }

    pub(crate) fn apply_editor_content_width(&mut self, value: f32, cx: &mut Context<Self>) {
        let value = value.round().clamp(320.0, 1600.0);
        self.styles.editor.editor_max_width_px = value;
        self.app_settings.editor_content_width = value;
        if let Some(editor) = &self.settings_editor_content_width_editor {
            self.sync_inline_stepper_editor(editor, format!("{value:.0}"), cx);
        }
        self.finalize_settings_change(cx);
    }

    fn ensure_editor_content_width_editor(&mut self, cx: &mut Context<Self>) {
        if self.settings_editor_content_width_editor.is_some() {
            return;
        }
        let editor = self.build_inline_stepper_editor(
            format!("{:.0}", self.app_settings.effective_editor_content_width()),
            cx,
        );
        cx.subscribe(
            &editor,
            |this, editor, event: &crate::text_input::TextInputEvent, cx| match event {
                crate::text_input::TextInputEvent::Submit => {
                    let text = editor.read(cx).text().to_string();
                    if let Some(value) = text.parse::<f32>().ok().filter(|value| value.is_finite())
                    {
                        this.apply_editor_content_width(value, cx);
                    }
                }
                crate::text_input::TextInputEvent::Escape => {
                    let current = this.app_settings.editor_content_width;
                    this.sync_inline_stepper_editor(&editor, format!("{current:.0}"), cx);
                }
                _ => {}
            },
        )
        .detach();
        self.settings_editor_content_width_editor = Some(editor);
    }

    // ── Font picker overlay ─────────────────────────────────────

    pub(crate) fn open_font_picker(
        &mut self,
        role: FontRole,
        option_index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (all_options, placeholder) = match role {
            FontRole::DateTimeFormat => (
                self.date_time_format_options(),
                self.t(TextKey::SearchDateTimeFormats).to_string(),
            ),
            FontRole::TerminalCursorShape => (
                TerminalCursorShape::ALL
                    .into_iter()
                    .map(|shape| {
                        let value = shape.as_str();
                        let label = terminal_settings_label(self.ui_language, value).to_string();
                        PickerOption {
                            value: value.to_string(),
                            search_text: format!("{label} {value}").to_lowercase(),
                            label,
                            preview_font: None,
                            icon_path: None,
                        }
                    })
                    .collect(),
                terminal_settings_label(self.ui_language, "search_cursor_shapes").to_string(),
            ),
            FontRole::TerminalLaunchCommand => (
                ChatAgentCli::all()
                    .into_iter()
                    .map(|agent_cli| PickerOption {
                        value: agent_cli.settings_value().to_string(),
                        label: agent_cli.label().to_string(),
                        search_text: format!(
                            "{} {} {}",
                            agent_cli.label(),
                            agent_cli.command_name(),
                            agent_cli.settings_value()
                        )
                        .to_lowercase(),
                        preview_font: None,
                        icon_path: Some(agent_cli.icon_path()),
                    })
                    .collect(),
                terminal_settings_label(self.ui_language, "search_commands").to_string(),
            ),
            FontRole::AttachmentLocation => (
                ["current_file", "workspace_folder", "current_file_subfolder"]
                    .into_iter()
                    .map(|value| {
                        let label = attachment_settings_label(self.ui_language, value).to_string();
                        PickerOption {
                            value: value.to_string(),
                            search_text: label.to_lowercase(),
                            label,
                            preview_font: None,
                            icon_path: None,
                        }
                    })
                    .collect(),
                attachment_settings_label(self.ui_language, "search").to_string(),
            ),
            FontRole::InterfaceLanguage => (
                language_picker_options(self.ui_language)
                    .into_iter()
                    .map(|option| PickerOption {
                        value: option.language.code().to_string(),
                        label: option.label,
                        search_text: option.search_text,
                        preview_font: None,
                        icon_path: None,
                    })
                    .collect::<Vec<_>>(),
                self.t(TextKey::SearchLanguages).to_string(),
            ),
            _ => {
                if self.cached_font_names.is_none() {
                    let mut names = window.text_system().all_font_names();
                    names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
                    names.dedup();
                    names.retain(|name| !name.starts_with('.'));
                    self.cached_font_names = Some(names);
                }
                (
                    self.cached_font_names
                        .clone()
                        .unwrap_or_default()
                        .into_iter()
                        .map(|name| PickerOption {
                            value: name.clone(),
                            label: name.clone(),
                            search_text: name.to_lowercase(),
                            preview_font: Some(name),
                            icon_path: None,
                        })
                        .collect::<Vec<_>>(),
                    self.t(TextKey::SearchFonts).to_string(),
                )
            }
        };

        let current_value = match (role, option_index) {
            (FontRole::UiFont, _) => self.app_settings.ui_font.clone(),
            (FontRole::BodyFont, 0) => self.app_settings.body_latin_font.clone(),
            (FontRole::BodyFont, _) => self.app_settings.body_cjk_font.clone(),
            (FontRole::CodeFont, _) => self.app_settings.code_font.clone(),
            (FontRole::InterfaceLanguage, _) => self.ui_language.code().to_string(),
            (FontRole::DateTimeFormat, _) => self.app_settings.date_time_format().to_string(),
            (FontRole::AttachmentLocation, _) => self.app_settings.attachment_location.clone(),
            (FontRole::TerminalCursorShape, _) => self
                .app_settings
                .terminal_cursor_shape()
                .as_str()
                .to_string(),
            (FontRole::TerminalLaunchCommand, _) => {
                self.app_settings.terminal_launch_command.clone()
            }
            (FontRole::WhiteboardFont, _) => self
                .whiteboard_current_style()
                .font
                .family
                .unwrap_or_default(),
        };
        let selected_index = all_options
            .iter()
            .position(|option| option.value == current_value)
            .unwrap_or(0);
        let render_limit = super::list_viewport::PICKER_LIST_POLICY.ensure_index(
            super::list_viewport::PICKER_LIST_POLICY.initial_count,
            all_options.len(),
            selected_index,
        );
        let scroll_handle = ScrollHandle::new();
        scroll_handle.scroll_to_item(selected_index);
        self.settings_font_picker = Some(FontPickerState {
            role,
            option_index,
            filtered: all_options.clone(),
            all_options,
            selected_index,
            render_limit,
            scroll_handle,
        });

        // Create TextInput for search
        let styles = self.search_menu_text_input_styles();
        let editor = cx.new(move |cx| {
            let mut input = crate::text_input::TextInput::new(styles, &placeholder, cx);
            // The font picker overlay is deferred; a deferred context menu
            // would nest defer_draw and panic.
            input.context_menu_enabled = false;
            input
        });
        cx.subscribe(
            &editor,
            |this, editor, event: &crate::text_input::TextInputEvent, cx| match event {
                crate::text_input::TextInputEvent::Changed => {
                    let query = editor.read(cx).text().to_lowercase();
                    if let Some(picker) = &mut this.settings_font_picker {
                        if query.is_empty() {
                            picker.filtered = picker.all_options.clone();
                        } else {
                            picker.filtered = picker
                                .all_options
                                .iter()
                                .filter(|option| option.search_text.contains(&query))
                                .cloned()
                                .collect();
                        }
                        picker.selected_index = 0;
                        picker.render_limit =
                            super::list_viewport::PICKER_LIST_POLICY.initial_count;
                        cx.notify();
                    }
                }
                crate::text_input::TextInputEvent::Submit
                | crate::text_input::TextInputEvent::SubmitBackward => {
                    if let Some(picker) = &this.settings_font_picker {
                        if let Some(option) = picker.filtered.get(picker.selected_index).cloned() {
                            this.apply_font_selection(option.value, cx);
                            return;
                        }
                    }
                    this.close_font_picker(cx);
                }
                crate::text_input::TextInputEvent::Escape => {
                    this.close_font_picker(cx);
                }
                crate::text_input::TextInputEvent::MoveUp => {
                    if let Some(picker) = &mut this.settings_font_picker {
                        if picker.selected_index > 0 {
                            picker.selected_index -= 1;
                            picker.scroll_handle.scroll_to_item(picker.selected_index);
                        }
                        cx.notify();
                    }
                }
                crate::text_input::TextInputEvent::MoveDown => {
                    if let Some(picker) = &mut this.settings_font_picker {
                        if picker.selected_index + 1 < picker.filtered.len() {
                            picker.selected_index += 1;
                            picker.render_limit = super::list_viewport::PICKER_LIST_POLICY
                                .ensure_index(
                                    picker.render_limit,
                                    picker.filtered.len(),
                                    picker.selected_index,
                                );
                            picker.scroll_handle.scroll_to_item(picker.selected_index);
                        }
                        cx.notify();
                    }
                }
            },
        )
        .detach();
        editor.focus_handle(cx).focus(window);
        self.settings_font_picker_editor = Some(editor);
        cx.notify();
    }

    pub(crate) fn close_font_picker(&mut self, cx: &mut Context<Self>) {
        self.settings_font_picker = None;
        self.settings_font_picker_editor = None;
        self.font_picker_trigger_position = None;
        cx.notify();
    }

    /// Render the font picker as a top-level overlay (called from render.rs).
    /// Follows the same deferred/occlude pattern as context menus.
    pub(crate) fn render_font_picker_overlay(
        &self,
        picker: &FontPickerState,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // Determine position: left-align with the trigger button's left edge.
        // Layout: sidebar? | [workspace-pane: settings_panel(max_w=560, mx_auto, px=40, font_row: label=120 + gap=12 + button)] | utility panel?
        let border_w = self.styles.window.window_border_width_px;
        let sidebar_w = self.sidebar_width();
        let right_panel_w = self.right_panel_width();
        let pane_w = self.last_window_width - sidebar_w - right_panel_w - 2.0 * border_w;
        let settings_content_max_w = self.scaled(640.0);
        let settings_px = self.scaled(40.0);
        let settings_content_w = settings_content_max_w.min(pane_w);
        // mx_auto centers the content within the pane
        let settings_left = sidebar_w + border_w + (pane_w - settings_content_w) / 2.0;
        let button_left = settings_left + settings_px + self.scaled(120.0) + self.scaled(12.0);
        let button_w =
            settings_content_w - 2.0 * settings_px - self.scaled(120.0) - self.scaled(12.0);

        // Use captured button bounds for precise positioning
        let button_bounds = self
            .font_picker_button_bounds
            .get(&(picker.role, picker.option_index))
            .and_then(|rc| rc.get());
        let button_bounds = button_bounds.unwrap_or_else(|| {
            gpui::Bounds::new(
                gpui::point(px(button_left), px(200.0)),
                gpui::size(px(button_w), px(self.scaled(30.0))),
            )
        });

        let config = SearchMenuConfig {
            id: "font-picker-dropdown",
            anchor: button_bounds,
            viewport_width: self.last_window_width,
            viewport_height: self.last_window_height,
            desired_width: f32::from(button_bounds.size.width),
            match_anchor_width: true,
            row_count: picker.filtered.len(),
        };

        let rows = picker
            .filtered
            .iter()
            .take(picker.render_limit)
            .enumerate()
            .map(|(idx, option)| {
                let value = option.value.clone();
                let row = SearchMenuRow::new(option.label.clone())
                    .selected(idx == picker.selected_index)
                    .icon(option.icon_path)
                    .preview_font(option.preview_font.clone());
                self.render_search_menu_row(
                    SharedString::from(format!("fp-item-{idx}")),
                    row,
                    move |this, _, cx| {
                        this.apply_font_selection(value.clone(), cx);
                    },
                    cx,
                )
            })
            .collect();

        let on_scroll_extend: super::search_menu::SearchMenuScrollExtend = Box::new(|this, cx| {
            if let Some(picker) = &mut this.settings_font_picker {
                let next = super::list_viewport::PICKER_LIST_POLICY.extend_near_end(
                    picker.render_limit,
                    picker.filtered.len(),
                    picker.scroll_handle.bottom_item(),
                );
                if next != picker.render_limit {
                    picker.render_limit = next;
                    cx.notify();
                }
            }
        });

        self.render_search_menu_overlay(
            &config,
            self.settings_font_picker_editor.clone(),
            Some(&picker.scroll_handle),
            Some(on_scroll_extend),
            rows,
            |this, _, cx| this.close_font_picker(cx),
            cx,
        )
    }

    fn apply_font_selection(&mut self, font_name: String, cx: &mut Context<Self>) {
        if let Some(picker) = self.settings_font_picker.take() {
            self.settings_font_picker_editor = None;
            self.font_picker_trigger_position = None;
            match (picker.role, picker.option_index) {
                (FontRole::UiFont, _) => {
                    self.styles.ui_font_family = Box::leak(font_name.clone().into_boxed_str());
                    self.app_settings.ui_font = font_name;
                }
                (FontRole::BodyFont, 0) => {
                    let leaked = Box::leak(font_name.clone().into_boxed_str());
                    self.styles.editor.editor_body_latin_font_family = leaked;
                    self.styles.editor.editor_heading_latin_font_family = leaked;
                    self.app_settings.body_latin_font = font_name;
                }
                (FontRole::BodyFont, 1) => {
                    let leaked = Box::leak(font_name.clone().into_boxed_str());
                    self.styles.editor.editor_body_cjk_font_family = leaked;
                    self.styles.editor.editor_heading_cjk_font_family = leaked;
                    self.app_settings.body_cjk_font = font_name;
                }
                (FontRole::CodeFont, _) => {
                    self.styles.editor.editor_monospace_font_family =
                        Box::leak(font_name.clone().into_boxed_str());
                    self.app_settings.code_font = font_name;
                }
                (FontRole::InterfaceLanguage, _) => {
                    if let Some(language) = UiLanguage::from_code(&font_name) {
                        self.apply_ui_language(language, cx);
                        return;
                    }
                }
                (FontRole::DateTimeFormat, _) => {
                    self.app_settings.set_date_time_format(font_name);
                }
                (FontRole::AttachmentLocation, _) => {
                    self.app_settings.attachment_location = font_name;
                }
                (FontRole::TerminalCursorShape, _) => {
                    if let Some(shape) = TerminalCursorShape::from_str(&font_name) {
                        self.set_terminal_cursor_shape(shape);
                    }
                }
                (FontRole::TerminalLaunchCommand, _) => {
                    if let Some(agent_cli) = ChatAgentCli::from_settings_value(&font_name) {
                        self.app_settings.terminal_launch_command = font_name;
                        self.app_settings.terminal_command_args =
                            agent_cli.command_name().to_string();
                        if let Some(editor) = self.settings_terminal_command_args_editor.clone() {
                            let command = self.app_settings.terminal_command_args.clone();
                            editor.update(cx, |input, cx| input.set_text(&command, cx));
                        }
                    }
                }
                (FontRole::WhiteboardFont, _) => {
                    self.whiteboard_apply_font_family(font_name, cx);
                    return;
                }
                _ => {}
            }
            self.app_settings.save();
            self.clear_all_tab_layout_caches();
            cx.notify();
        }
    }
}

/// Applies the default application icon tile to the macOS Dock at runtime via
/// `NSApplication setApplicationIconImage:`. No-op on other platforms.
#[cfg(target_os = "macos")]
pub(crate) fn apply_app_icon_to_dock() {
    use objc::{class, msg_send, sel, sel_impl};

    let Some(padded_path) = build_padded_dock_icon() else {
        return;
    };
    let Some(path_str) = padded_path.to_str() else {
        return;
    };
    let Ok(c_path) = std::ffi::CString::new(path_str) else {
        return;
    };

    unsafe {
        let ns_path: *mut objc::runtime::Object =
            msg_send![class!(NSString), stringWithUTF8String: c_path.as_ptr()];
        let image: *mut objc::runtime::Object = msg_send![class!(NSImage), alloc];
        let image: *mut objc::runtime::Object = msg_send![image, initWithContentsOfFile: ns_path];
        if image.is_null() {
            return;
        }
        let app: *mut objc::runtime::Object = msg_send![class!(NSApplication), sharedApplication];
        let () = msg_send![app, setApplicationIconImage: image];
    }
}

/// Renders the full-bleed tile onto a padded, transparent canvas so the Dock
/// shows it at the standard macOS app-icon size, and returns the temp PNG path.
/// A soft drop shadow is baked in so the Dock icon gains the same depth as the
/// bundled app icon.
#[cfg(target_os = "macos")]
fn build_padded_dock_icon() -> Option<std::path::PathBuf> {
    use image::{Rgba, RgbaImage, imageops};

    // Match the .icns margin: artwork fills ~80.5% of the canvas, centered.
    const CANVAS: u32 = 1024;
    const CONTENT: u32 = 824;
    // Soft drop shadow tuned to sit within the transparent margin.
    const SHADOW_OFFSET_Y: i64 = 10;
    const SHADOW_BLUR: f32 = 16.0;
    const SHADOW_STRENGTH: f32 = 0.28;

    let tile_path =
        crate::editor::helpers::resolve_asset_image_path(crate::editor::helpers::APP_ICON_PATH);
    let tile = image::open(&tile_path).ok()?.to_rgba8();
    let content = imageops::resize(&tile, CONTENT, CONTENT, imageops::FilterType::Lanczos3);
    let offset = ((CANVAS - CONTENT) / 2) as i64;

    // Build a black silhouette of the artwork, offset downward, then blur it
    // into a soft shadow.
    let mut shadow = RgbaImage::new(CANVAS, CANVAS);
    for (x, y, pixel) in content.enumerate_pixels() {
        let alpha = pixel[3];
        if alpha == 0 {
            continue;
        }
        let sx = offset + x as i64;
        let sy = offset + SHADOW_OFFSET_Y + y as i64;
        if sx < 0 || sy < 0 || sx >= CANVAS as i64 || sy >= CANVAS as i64 {
            continue;
        }
        let shadow_alpha = (alpha as f32 * SHADOW_STRENGTH).round() as u8;
        shadow.put_pixel(sx as u32, sy as u32, Rgba([0, 0, 0, shadow_alpha]));
    }
    let shadow = imageops::blur(&shadow, SHADOW_BLUR);

    let mut canvas = RgbaImage::new(CANVAS, CANVAS);
    imageops::overlay(&mut canvas, &shadow, 0, 0);
    imageops::overlay(&mut canvas, &content, offset, offset);

    let out_path = std::env::temp_dir().join("moeka-dock-icon.png");
    canvas.save(&out_path).ok()?;
    Some(out_path)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn apply_app_icon_to_dock() {}
