use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::{Arc, Weak};

use gpui::{
    AnyElement, Context, Focusable, MouseButton, ScrollHandle, SharedString, Window, div,
    prelude::*, px, svg,
};

use crate::i18n::{
    TextKey, UiLanguage, attachment_settings_label, language_picker_options,
    terminal_settings_label,
};
use crate::theme::{EditorThemeMode, ThemeKind};
use crate::ui::switch::Switch;

use super::app_settings::ThemeMode;
#[cfg(not(target_os = "macos"))]
use super::helpers;
use super::{
    APP_VERSION, ChatAgentCli, DOC_SVG_PATH, EditorView, FILE_INFO_SECTION_INFO_SVG_PATH,
    FLOWER_SVG_PATH, MOUNTAIN_SVG_PATH, SITE_SVG_PATH, SYNC_SVG_PATH, SearchMenuConfig,
    SearchMenuRow, THEME_AUTO_MINI_SVG_PATH, THEME_DARK_MINI_SVG_PATH, THEME_LIGHT_MINI_SVG_PATH,
    TerminalCursorShape, attachments, tooltip,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThemeAppearanceChoice {
    Auto,
    Light,
    Dark,
}

// ── Font picker state ───────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum FontRole {
    UiFont,
    BodyFont,
    CodeFont,
    InterfaceLanguage,
    DateTimeFormat,
    AttachmentLocation,
    LinkSyntax,
    LinkPathStyle,
    TerminalCursorShape,
    TerminalLaunchCommand,
    TerminalFont,
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
    DefaultLeftAlign(bool),
    AutoSave(bool),
    SimpleMode(bool),
    SidebarAlwaysDark(bool),
    UiElevation(bool),
    TerminalCopyOnSelect(bool),
}

thread_local! {
    static ATTACHMENT_FOLDER_REQUESTS: RefCell<HashMap<gpui::EntityId, Weak<()>>> = RefCell::default();
}

fn attachment_folder_result_is_current(
    request: &Arc<()>,
    latest: Option<&Weak<()>>,
    original: &super::app_settings::AppSettings,
    current: &super::app_settings::AppSettings,
) -> bool {
    latest.is_some_and(|latest| latest.ptr_eq(&Arc::downgrade(request))) && original == current
}

fn capture_attachment_folder_bookmark(path: &std::path::Path) -> std::io::Result<String> {
    let metadata = std::fs::metadata(path)?;
    if !metadata.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotADirectory,
            "the selected attachment folder is not a directory",
        ));
    }
    crate::platform::fs_task::check_deadline()?;
    #[cfg(target_os = "macos")]
    {
        Ok(objc::rc::autoreleasepool(|| {
            // Keep the known directory hint without asking NSURL to stat it again.
            let directory_path = path.join("");
            crate::platform::security_scope::bookmark_for(&directory_path).unwrap_or_default()
        }))
    }
    #[cfg(not(target_os = "macos"))]
    {
        // Other platforms intentionally use plain paths rather than bookmarks.
        Ok(String::new())
    }
}

fn settings_header_row(gap: f32) -> gpui::Div {
    div()
        .w_full()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_end()
        .justify_between()
        .gap(px(gap))
}

fn settings_header_actions(gap: f32) -> gpui::Stateful<gpui::Div> {
    div()
        .id("settings-header-actions")
        .ml_auto()
        .flex_none()
        .max_w_full()
        .flex()
        .flex_row()
        .flex_nowrap()
        .overflow_x_scroll()
        .items_center()
        .gap(px(gap))
}

fn settings_header_button_max_width(content_width: f32, padding: f32, limit: f32) -> f32 {
    limit.min((content_width - padding * 2.0).max(0.0))
}

// ── Settings panel rendering ────────────────────────────────────────

impl EditorView {
    fn settings_content_width(&self, window_width: f32) -> f32 {
        let pane_width = self
            .pane_bounds
            .get(&self.current_pane_id())
            .map(|bounds| f32::from(bounds.size.width))
            .filter(|width| *width > 0.0)
            .unwrap_or_else(|| {
                (window_width
                    - self.sidebar_width()
                    - self.right_panel_width()
                    - 2.0 * self.styles.window.window_border_width_px)
                    .max(1.0)
            });
        pane_width
            .min(self.scaled(self.styles.ui.spec.settings_max_width_px))
            .max(1.0)
    }

    pub(crate) fn render_settings_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bg = self.styles.settings_background();
        let sps = &self.styles.settings_panel;
        let version_label = format!("v{APP_VERSION}");
        let content_width = self.settings_content_width(f32::from(window.bounds().size.width));
        let button_max_width = settings_header_button_max_width(
            content_width,
            self.scaled(self.styles.ui.spec.settings_padding_x_px),
            self.scaled(sps.settings_header_button_max_width_px),
        );

        div()
            .id("settings-panel")
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .overflow_y_scroll()
            .track_scroll(&self.settings_scroll_handle)
            .bg(bg)
            .text_size(px(self.scaled(self.styles.ui_font_size_px)))
            .text_color(sps.settings_primary_text_color)
            .child(
                div()
                    .w(px(content_width))
                    .flex_none()
                    .max_w(px(self.scaled(self.styles.ui.spec.settings_max_width_px)))
                    .mx_auto()
                    .py(px(self.scaled(self.styles.ui.spec.settings_padding_y_px)))
                    .px(px(self.scaled(self.styles.ui.spec.settings_padding_x_px)))
                    .flex()
                    .flex_col()
                    .gap(px(self.scaled(self.styles.ui.spec.settings_section_gap_px)))
                    .child(
                        settings_header_row(self.scaled(sps.settings_header_gap_px))
                            .child(
                                div()
                                    .flex_none()
                                    .max_w_full()
                                    .min_w(px(0.0))
                                    .flex()
                                    .items_end()
                                    .gap(px(self.scaled(sps.settings_title_version_gap_px)))
                                    .child(
                                        div()
                                            .min_w(px(0.0))
                                            .overflow_hidden()
                                            .truncate()
                                            .whitespace_nowrap()
                                            .text_size(px(
                                                self.scaled(self.styles.ui_font_size_px + 9.0)
                                            ))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(sps.settings_primary_text_color)
                                            .child(self.t(TextKey::SettingsPanelTitle)),
                                    )
                                    .child(
                                        div()
                                            .flex_none()
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
                                settings_header_actions(
                                    self.scaled(sps.settings_header_actions_gap_px),
                                )
                                .when(crate::platform::capabilities::SELF_UPDATE, |this| {
                                    this.child(
                                        self.render_settings_update_button(button_max_width, cx),
                                    )
                                })
                                .child(self.render_settings_config_button(button_max_width, cx))
                                .child(self.render_settings_help_button(button_max_width, cx))
                                .child(self.render_settings_site_button(button_max_width, cx)),
                            ),
                    )
                    .child(self.render_theme_section(cx))
                    .child(self.render_font_section(window, cx))
                    .child(self.render_format_section(cx))
                    .when(crate::platform::capabilities::CHILD_PROCESSES, |this| {
                        this.child(self.render_terminal_settings_section(cx))
                    })
                    .child(self.render_attachment_settings_section(cx)),
            )
            .into_any_element()
    }

    fn render_settings_update_button(&self, max_width: f32, cx: &mut Context<Self>) -> AnyElement {
        let sps = &self.styles.settings_panel;
        let checking = self.update_check_in_progress();
        let label = if checking {
            crate::i18n::checking_updates_label(self.ui_language)
        } else {
            crate::i18n::check_updates_label(self.ui_language)
        };

        let button = crate::ui::button::button(
            "settings-update-button",
            self.styles.settings_header_button(),
            crate::style::ControlState {
                loading: checking,
                ..Default::default()
            },
            self.ui_scale,
        )
        .max_w(px(max_width))
        .child(crate::ui::button::icon(
            self.styles.settings_header_button(),
            SYNC_SVG_PATH,
            sps.settings_site_button_icon_color,
            self.ui_scale,
        ))
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
                this.start_manual_update_check(cx);
            }),
        )
        .into_any_element();
        tooltip::with_tooltip(
            button,
            cx.entity().clone(),
            crate::i18n::check_updates_label(self.ui_language),
            None::<&'static str>,
        )
    }

    fn render_settings_config_button(&self, max_width: f32, cx: &mut Context<Self>) -> AnyElement {
        let sps = &self.styles.settings_panel;

        let button = crate::ui::button::button(
            "settings-config-button",
            self.styles.settings_header_button(),
            crate::style::ControlState::default(),
            self.ui_scale,
        )
        .max_w(px(max_width))
        .child(crate::ui::button::icon(
            self.styles.settings_header_button(),
            FILE_INFO_SECTION_INFO_SVG_PATH,
            sps.settings_site_button_icon_color,
            self.ui_scale,
        ))
        .child(
            div()
                .min_w(px(0.0))
                .overflow_hidden()
                .truncate()
                .whitespace_nowrap()
                .child(crate::i18n::config_file_label(self.ui_language)),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| {
                this.open_config_file_tab(cx);
            }),
        )
        .into_any_element();
        tooltip::with_tooltip(
            button,
            cx.entity().clone(),
            crate::i18n::config_file_label(self.ui_language),
            None::<&'static str>,
        )
    }

    fn render_settings_help_button(&self, max_width: f32, cx: &mut Context<Self>) -> AnyElement {
        let sps = &self.styles.settings_panel;

        let button = crate::ui::button::button(
            "settings-help-button",
            self.styles.settings_header_button(),
            crate::style::ControlState::default(),
            self.ui_scale,
        )
        .max_w(px(max_width))
        .child(crate::ui::button::icon(
            self.styles.settings_header_button(),
            DOC_SVG_PATH,
            sps.settings_site_button_icon_color,
            self.ui_scale,
        ))
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
        .into_any_element();
        tooltip::with_tooltip(
            button,
            cx.entity().clone(),
            self.t(TextKey::HelpManual),
            None::<&'static str>,
        )
    }

    fn render_settings_site_button(&self, max_width: f32, cx: &mut Context<Self>) -> AnyElement {
        let sps = &self.styles.settings_panel;
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

        let tooltip_label = label.clone();
        let button = crate::ui::button::button(
            "settings-site-button",
            self.styles.settings_header_button(),
            crate::style::ControlState::default(),
            self.ui_scale,
        )
        .max_w(px(max_width))
        .text_color(text_color)
        .child(crate::ui::button::icon(
            self.styles.settings_header_button(),
            SITE_SVG_PATH,
            icon_color,
            self.ui_scale,
        ))
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
                } else if let Err(error) = crate::platform::shell::open_url("https://moeka.app/") {
                    log_error!("failed to open official website: {error}");
                }
            }),
        )
        .into_any_element();
        tooltip::with_tooltip(
            button,
            cx.entity().clone(),
            tooltip_label,
            None::<&'static str>,
        )
    }

    fn render_format_section(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.date_time_format_preview_label(self.app_settings.date_time_format());

        div()
            .w_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(self.scaled(
                self.styles.ui.space(crate::style::SpaceStep::Three),
            )))
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
        self.ensure_terminal_font_size_editor(cx);
        let cursor_shape = self.app_settings.terminal_cursor_shape();
        let current = terminal_settings_label(self.ui_language, cursor_shape.as_str());
        let launch_cli = self.app_settings.terminal_launch_cli();
        let terminal_font = if self.app_settings.terminal_font.is_empty() {
            self.app_settings.code_font.clone()
        } else {
            self.app_settings.terminal_font.clone()
        };

        div()
            .w_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(self.scaled(
                self.styles.ui.space(crate::style::SpaceStep::Three),
            )))
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
                terminal_settings_label(self.ui_language, "font"),
                &terminal_font,
                FontRole::TerminalFont,
                0,
                cx,
            ))
            .child(self.render_terminal_font_size_row(cx))
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
        let control =
            crate::ui::card::input_frame(self.styles.settings_input_frame(), self.ui_scale)
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
        let syntax = self.app_settings.link_syntax.clone();
        let syntax_controls =
            self.render_picker_control(label(&syntax), FontRole::LinkSyntax, 0, cx);
        let path_style = self.app_settings.link_path_style.clone();
        let path_controls =
            self.render_picker_control(label(&path_style), FontRole::LinkPathStyle, 0, cx);

        div()
            .flex()
            .flex_col()
            .gap(px(self.scaled(
                self.styles.ui.space(crate::style::SpaceStep::Three),
            )))
            .child(self.render_section_heading(label("section")))
            .child(self.render_attachment_settings_row(label("location"), location_controls))
            .when(
                matches!(
                    location.as_str(),
                    "workspace_folder" | "current_file_subfolder"
                ),
                |this| this.child(self.render_attachment_folder_row()),
            )
            .when(location == "external_folder", |this| {
                this.child(self.render_external_attachment_folder_row(cx))
            })
            .child(self.render_attachment_settings_row(label("syntax"), syntax_controls))
            .child(self.render_attachment_settings_row(label("path_style"), path_controls))
            .into_any_element()
    }

    fn render_attachment_settings_row(&self, label: &str, control: AnyElement) -> AnyElement {
        crate::ui::field::field(
            self.styles.settings_field(),
            self.ui_scale,
            label.to_string(),
            control,
        )
        .into_any_element()
    }

    fn render_attachment_folder_row(&self) -> AnyElement {
        let editor = self.settings_attachment_folder_editor.clone();
        let control =
            crate::ui::card::input_frame(self.styles.settings_input_frame(), self.ui_scale)
                .when_some(editor, |this, editor| this.child(editor))
                .into_any_element();
        self.render_attachment_settings_row(
            attachment_settings_label(self.ui_language, "folder"),
            control,
        )
    }

    fn render_external_attachment_folder_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let path = if self.app_settings.attachment_external_folder.is_empty() {
            attachment_settings_label(self.ui_language, "choose_external_folder").to_string()
        } else {
            self.app_settings.attachment_external_folder.clone()
        };
        let control = crate::ui::button::button(
            "settings-external-attachment-folder",
            self.styles.settings_folder_picker(),
            Default::default(),
            self.ui_scale,
        )
        .w_full()
        .justify_start()
        .child(
            div()
                .min_w(px(0.0))
                .overflow_hidden()
                .truncate()
                .whitespace_nowrap()
                .child(path),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(|this, _, _, cx| this.pick_external_attachment_folder(cx)),
        )
        .into_any_element();
        self.render_attachment_settings_row(
            attachment_settings_label(self.ui_language, "external_folder_path"),
            control,
        )
    }

    fn pick_external_attachment_folder(&mut self, cx: &mut Context<Self>) {
        let entity_id = cx.entity_id();
        let request = Arc::new(());
        ATTACHMENT_FOLDER_REQUESTS.with_borrow_mut(|requests| {
            requests.retain(|_, request| request.strong_count() > 0);
            requests.insert(entity_id, Arc::downgrade(&request));
        });
        let original_settings = self.app_settings.clone();
        let tab_id = self.active_tab_id.clone();
        let pane_id = self.current_pane_id();
        #[cfg(not(target_os = "macos"))]
        let title =
            attachment_settings_label(self.ui_language, "choose_external_folder").to_string();
        cx.spawn(async move |this, cx| {
            let is_current = |editor: &EditorView| {
                editor.active_tab_is_settings()
                    && editor.active_tab_id == tab_id
                    && editor.current_pane_id() == pane_id
                    && ATTACHMENT_FOLDER_REQUESTS.with_borrow(|requests| {
                        attachment_folder_result_is_current(
                            &request,
                            requests.get(&entity_id),
                            &original_settings,
                            &editor.app_settings,
                        )
                    })
            };
            #[cfg(target_os = "macos")]
            let selected: anyhow::Result<Option<std::path::PathBuf>> =
                match this.update(cx, |_, cx| {
                    cx.prompt_for_paths(gpui::PathPromptOptions {
                        files: false,
                        directories: true,
                        multiple: false,
                        prompt: None,
                    })
                }) {
                    Ok(receiver) => match receiver.await {
                        Ok(result) => result.map(|paths| paths.and_then(|mut paths| paths.pop())),
                        Err(error) => Err(anyhow::anyhow!(
                            "attachment folder picker was interrupted: {error}"
                        )),
                    },
                    Err(_) => return,
                };
            #[cfg(not(target_os = "macos"))]
            let selected: anyhow::Result<Option<std::path::PathBuf>> = Ok(cx
                .background_executor()
                .spawn(async move { helpers::pick_folder_blocking(&title) })
                .await);
            let path = match selected {
                Ok(Some(path)) => path,
                Ok(None) => return,
                Err(error) => {
                    let _ = this.update(cx, |this, cx| {
                        if is_current(this) {
                            this.notify_error(
                                format!("{}: {error:#}", this.t(TextKey::OpenFailed)),
                                cx,
                            );
                        }
                    });
                    return;
                }
            };
            if !this.update(cx, |this, _| is_current(this)).unwrap_or(false) {
                return;
            }
            let bookmark_path = path.clone();
            let result = crate::platform::fs_task::run(
                "capture attachment folder bookmark",
                path.clone(),
                move || capture_attachment_folder_bookmark(&bookmark_path),
            )
            .await;
            let _ = this.update(cx, |this, cx| {
                if !is_current(this) {
                    return;
                }
                match result {
                    Ok(bookmark) => {
                        this.app_settings.attachment_external_folder =
                            path.to_string_lossy().into_owned();
                        this.app_settings.attachment_external_folder_bookmark = bookmark;
                        this.finalize_settings_change(cx);
                    }
                    Err(error) => this.notify_error(
                        format!(
                            "{} — {}: {error}",
                            this.t(TextKey::OpenFailed),
                            path.display(),
                        ),
                        cx,
                    ),
                }
            });
        })
        .detach();
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
            .gap(px(
                self.scaled(self.styles.ui.space(crate::style::SpaceStep::Two))
            ))
            .child(self.render_attachment_settings_row(
                crate::i18n::settings_label(self.ui_language, "auto_save"),
                self.render_boolean_settings_checkbox(
                    "settings-auto-save",
                    self.app_settings.auto_save,
                    SettingsOptionAction::AutoSave(false),
                    SettingsOptionAction::AutoSave(true),
                    cx,
                ),
            ))
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
            .child(self.render_attachment_settings_row(
                crate::i18n::settings_label(self.ui_language, "default_left_align"),
                self.render_boolean_settings_checkbox(
                    "settings-default-left-align",
                    self.app_settings.default_left_align,
                    SettingsOptionAction::DefaultLeftAlign(false),
                    SettingsOptionAction::DefaultLeftAlign(true),
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
        Switch::new(
            id,
            crate::style::ControlState {
                checked,
                ..Default::default()
            },
            value_label,
            self.ui_scale,
            self.styles
                .switch
                .for_size(self.styles.ui.spec.application.switch_size),
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
            SettingsOptionAction::DefaultLeftAlign(value) => {
                self.app_settings.default_left_align = value;
                self.apply_default_left_alignment(value);
            }
            SettingsOptionAction::AutoSave(value) => {
                self.app_settings.auto_save = value;
                if value {
                    self.schedule_auto_save_for_active_tab(cx);
                } else {
                    self.cancel_all_auto_saves();
                    self.schedule_temp_save_for_active_tab(cx);
                }
            }
            SettingsOptionAction::SimpleMode(value) => {
                self.app_settings.simple_mode = value;
            }
            SettingsOptionAction::SidebarAlwaysDark(value) => {
                self.app_settings.sidebar_always_dark = value;
                self.refresh_theme_styles(false, cx);
            }
            SettingsOptionAction::UiElevation(value) => {
                self.app_settings.ui_elevation = value;
                self.set_ui_style_spec(self.app_settings.ui_style_spec(), cx);
            }
            SettingsOptionAction::TerminalCopyOnSelect(value) => {
                self.app_settings.terminal_copy_on_select = value;
            }
        }
        self.finalize_settings_change(cx);
    }

    fn apply_default_left_alignment(&mut self, value: bool) {
        for pane_id in self.pane_ids() {
            self.with_pane(pane_id, |editor| {
                for tab in &mut editor.tabs {
                    if tab
                        .file_kind
                        .as_ref()
                        .is_some_and(super::FileKind::is_markdown)
                    {
                        tab.editor_left_aligned = value;
                    }
                }
                if editor
                    .active_file_kind()
                    .is_some_and(super::FileKind::is_markdown)
                {
                    editor.editor_left_aligned = value;
                    editor.layout_dirty = true;
                }
            });
        }
        super::session_store::persist_session(self);
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
        let heading_label = if self.ui_language == UiLanguage::English {
            label.to_uppercase()
        } else {
            label.to_string()
        };
        crate::ui::field::section_heading(
            self.styles.settings_section_heading(),
            self.ui_scale,
            heading_label,
        )
        .into_any_element()
    }

    fn settings_text_input_styles(&self) -> crate::style::TextInputStyles {
        self.styles
            .input_styles(crate::style::InputRole::Settings, self.ui_scale)
    }

    fn inline_stepper_input_styles(&self) -> crate::style::TextInputStyles {
        self.styles
            .input_styles(crate::style::InputRole::Stepper, self.ui_scale)
    }

    pub(crate) fn sync_settings_text_input_styles(&mut self, cx: &mut Context<Self>) {
        let stepper_styles = self.inline_stepper_input_styles();
        for editor in [
            self.settings_font_size_editor.as_ref(),
            self.settings_ui_font_size_editor.as_ref(),
            self.settings_line_height_editor.as_ref(),
            self.settings_letter_spacing_editor.as_ref(),
            self.settings_editor_content_width_editor.as_ref(),
            self.settings_terminal_font_size_editor.as_ref(),
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
        // The config file sits outside the watched workspace, so push the write
        // through the same reload path the fs watcher would use.
        self.handle_fs_events(&[crate::editor::config_paths::settings_file_path()], cx);
        cx.notify();
    }

    pub(crate) fn open_config_file_tab(&mut self, cx: &mut Context<Self>) {
        let path = crate::editor::config_paths::settings_file_path();
        // The file only exists once something has been persisted, so write the
        // current settings first to give the user a complete document to edit.
        if !path.exists() {
            self.app_settings.save();
        }
        self.open_path_in_permanent_tab(path, true, cx);
    }

    /// Re-reads the config file the user just saved and applies everything the
    /// settings panel owns; session and window state stay untouched.
    pub(crate) fn apply_saved_config_file(&mut self, cx: &mut Context<Self>) {
        let settings = match crate::editor::config_store::load_settings_strict() {
            Ok(settings) => settings,
            Err(error) => {
                let message = format!(
                    "{}: {error}",
                    crate::i18n::config_invalid_label(self.ui_language)
                );
                self.notify_error(message, cx);
                return;
            }
        };

        let previous_left_alignment = self.app_settings.default_left_align;
        self.app_settings = settings;
        if self.app_settings.default_left_align != previous_left_alignment {
            self.apply_default_left_alignment(self.app_settings.default_left_align);
        }
        self.ui_language = self.app_settings.ui_language();
        self.theme_kind = self.app_settings.theme_kind();
        self.editor_theme_mode = self
            .app_settings
            .editor_theme_mode_for_appearance(self.theme_kind.is_dark());
        self.theme = crate::theme::apply_editor_theme_mode(
            &self.theme_kind.build(),
            self.editor_theme_mode,
            self.theme_kind.is_dark(),
        );
        self.ui_scale = self.app_settings.ui_scale;
        if self.styles.ui.base_spec.appearance != self.app_settings.ui_style_spec().appearance {
            self.styles.ui.base_spec = self.app_settings.ui_style_spec();
        }
        self.refresh_theme_styles(true, cx);
        self.refresh_builtin_tab_titles();

        let message = crate::i18n::config_applied_label(self.ui_language).to_string();
        self.notify_info(message, cx);
    }

    // ── Theme section ───────────────────────────────────────────

    fn render_ui_corners_control(&self, cx: &mut Context<Self>) -> AnyElement {
        use crate::style::{ControlState, GroupPosition, Orientation, UiCorners};
        let style = self
            .styles
            .raised_segment(self.styles.editor_theme_toggle());
        crate::ui::button::toggle_group(self.styles.segmented_group(style.radius_px), self.ui_scale)
            .children(
                UiCorners::ALL
                    .into_iter()
                    .enumerate()
                    .map(|(index, corners)| {
                        let key = match corners {
                            UiCorners::Square => "corners_square",
                            UiCorners::Half => "corners_half",
                            UiCorners::Full => "corners_full",
                        };
                        crate::ui::button::toggle_segment(
                            ("settings-ui-corners", index),
                            style,
                            ControlState {
                                selected: self.app_settings.ui_corners == corners,
                                ..Default::default()
                            },
                            GroupPosition {
                                orientation: Orientation::Horizontal,
                                first: index == 0,
                                last: index + 1 == UiCorners::ALL.len(),
                            },
                            self.ui_scale,
                        )
                        .child(crate::i18n::settings_label(self.ui_language, key))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                let elevation = this.app_settings.ui_elevation;
                                this.set_ui_appearance(corners, elevation, cx);
                            }),
                        )
                    }),
            )
            .into_any_element()
    }

    fn render_theme_appearance_segment(
        &self,
        choice: ThemeAppearanceChoice,
        active: bool,
        first: bool,
        last: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let styles = &self.styles.settings_panel;
        let style = self.styles.raised_segment(self.styles.settings_toggle());
        let text_secondary = style.normal.foreground;
        let active_text = style.selected.foreground;
        let icon_size = self.scaled(styles.settings_theme_toggle_icon_size_px);
        let (id, icon, label) = match choice {
            ThemeAppearanceChoice::Auto => (
                "theme-toggle-auto",
                THEME_AUTO_MINI_SVG_PATH,
                crate::i18n::theme_auto_label(self.ui_language),
            ),
            ThemeAppearanceChoice::Light => (
                "theme-toggle-light",
                THEME_LIGHT_MINI_SVG_PATH,
                self.t(TextKey::ThemeLight),
            ),
            ThemeAppearanceChoice::Dark => (
                "theme-toggle-dark",
                THEME_DARK_MINI_SVG_PATH,
                self.t(TextKey::ThemeDark),
            ),
        };

        crate::ui::button::toggle_segment(
            id,
            style,
            crate::style::ControlState {
                selected: active,
                ..Default::default()
            },
            crate::style::GroupPosition {
                first,
                last,
                orientation: crate::style::Orientation::Horizontal,
            },
            self.ui_scale,
        )
        .child(svg().path(icon).size(px(icon_size)).text_color(if active {
            active_text
        } else {
            text_secondary
        }))
        .child(label.to_string())
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |this, _, window, cx| {
                this.select_theme_appearance(choice, window, cx);
            }),
        )
        .into_any_element()
    }

    fn select_theme_appearance(
        &mut self,
        choice: ThemeAppearanceChoice,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match choice {
            ThemeAppearanceChoice::Auto => self.set_theme_mode(ThemeMode::Auto, window, cx),
            ThemeAppearanceChoice::Light | ThemeAppearanceChoice::Dark => {
                let dark = choice == ThemeAppearanceChoice::Dark;
                self.set_theme_mode(ThemeMode::Manual, window, cx);
                if dark != self.theme_kind.is_dark() {
                    let kind = self.theme_kind_for_appearance(dark);
                    self.set_theme(kind, cx);
                }
            }
        }
    }

    fn render_theme_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.theme_kind;
        let is_dark_mode = self.settings_theme_dark_mode;
        let theme_mode = self.app_settings.theme_mode();
        let editor_theme_mode = self.editor_theme_mode;
        let settings_styles = &self.styles.settings_panel;
        let themes = if is_dark_mode {
            ThemeKind::dark_themes()
        } else {
            ThemeKind::light_themes()
        };
        let icon_size = self.scaled(settings_styles.settings_theme_toggle_icon_size_px);
        let option_border_width =
            self.scaled(settings_styles.settings_theme_option_border_width_px);
        let editor_mode_style = self
            .styles
            .raised_segment(self.styles.editor_theme_toggle());
        let raised = self.styles.ui.spec.appearance.elevation;
        let option_text_color = editor_mode_style.normal.foreground;
        let option_active_text_color = editor_mode_style.selected.foreground;

        div()
            .w_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(
                self.scaled(self.styles.ui.space(crate::style::SpaceStep::Four))
            ))
            .child(self.render_section_heading(self.t(TextKey::ThemeSection)))
            .child(self.render_attachment_settings_row(
                crate::i18n::settings_label(self.ui_language, "ui_corners"),
                self.render_ui_corners_control(cx),
            ))
            .child(self.render_attachment_settings_row(
                crate::i18n::settings_label(self.ui_language, "ui_elevation"),
                self.render_boolean_settings_checkbox(
                    "settings-ui-elevation",
                    self.app_settings.ui_elevation,
                    SettingsOptionAction::UiElevation(false),
                    SettingsOptionAction::UiElevation(true),
                    cx,
                ),
            ))
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
            .child(self.render_attachment_settings_row(
                crate::i18n::settings_label(self.ui_language, "sidebar_always_dark"),
                self.render_boolean_settings_checkbox(
                    "settings-sidebar-always-dark",
                    self.app_settings.sidebar_always_dark,
                    SettingsOptionAction::SidebarAlwaysDark(false),
                    SettingsOptionAction::SidebarAlwaysDark(true),
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
                        crate::ui::button::toggle_group(
                            self.styles.segmented_group(
                                self.styles
                                    .raised_segment(self.styles.settings_toggle())
                                    .radius_px,
                            ),
                            self.ui_scale,
                        )
                        .child(self.render_theme_appearance_segment(
                            ThemeAppearanceChoice::Auto,
                            theme_mode == ThemeMode::Auto,
                            true,
                            false,
                            cx,
                        ))
                        .child(self.render_theme_appearance_segment(
                            ThemeAppearanceChoice::Light,
                            theme_mode != ThemeMode::Auto && !is_dark_mode,
                            false,
                            false,
                            cx,
                        ))
                        .child(self.render_theme_appearance_segment(
                            ThemeAppearanceChoice::Dark,
                            theme_mode != ThemeMode::Auto && is_dark_mode,
                            false,
                            true,
                            cx,
                        )),
                    )
                    .child(
                        crate::ui::button::toggle_group(
                            self.styles.segmented_group(editor_mode_style.radius_px),
                            self.ui_scale,
                        )
                        .children(
                            EditorThemeMode::all()
                                .iter()
                                .enumerate()
                                .map(|(index, mode)| {
                                    let is_active = *mode == editor_theme_mode;
                                    let mode = *mode;
                                    crate::ui::button::toggle_segment(
                                        SharedString::from(format!(
                                            "editor-theme-mode-{}",
                                            mode.as_str()
                                        )),
                                        editor_mode_style,
                                        crate::style::ControlState {
                                            selected: is_active,
                                            ..Default::default()
                                        },
                                        crate::style::GroupPosition {
                                            orientation: crate::style::Orientation::Horizontal,
                                            first: index == 0,
                                            last: index + 1 == EditorThemeMode::all().len(),
                                        },
                                        self.ui_scale,
                                    )
                                    .when(index > 0 && !raised, |this| {
                                        this.border_l(px(option_border_width))
                                    })
                                    .child(
                                        svg()
                                            .path(match mode {
                                                EditorThemeMode::Colorful => FLOWER_SVG_PATH,
                                                EditorThemeMode::Monochrome => MOUNTAIN_SVG_PATH,
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
                    .w_full()
                    .flex_none()
                    .grid()
                    .grid_cols(4)
                    .gap_x(px(self.scaled(8.0)))
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
        let style = self.styles.theme_card(&preview, is_active);
        crate::ui::card::theme_card(style, self.ui_scale)
            .id(SharedString::from(format!("theme-{}", kind.label())))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| this.set_theme(kind, cx)),
            )
            .child(crate::ui::card::swatch_row(style, self.ui_scale))
            .child(
                crate::ui::typography::label(style.title, self.ui_scale)
                    .w_full()
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
            .w_full()
            .flex_none()
            .flex()
            .flex_col()
            .gap(px(self.scaled(
                self.styles.ui.space(crate::style::SpaceStep::Three),
            )))
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
            // Editor font size
            .child(self.render_editor_font_size_row(cx))
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
        let control = self.render_picker_control(current_value, role, option_index, cx);
        crate::ui::field::field(
            self.styles.settings_field(),
            self.ui_scale,
            label.to_string(),
            control,
        )
        .w_full()
        .into_any_element()
    }

    fn render_picker_control(
        &mut self,
        current_value: &str,
        role: FontRole,
        option_index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let current_value = current_value.to_string();
        let bounds_cell = self
            .font_picker_button_bounds
            .entry((role, option_index))
            .or_insert_with(|| Rc::new(Cell::new(None)))
            .clone();

        div()
            .flex_1()
            .min_w(px(0.0))
            .on_children_prepainted({
                let bounds_cell = bounds_cell.clone();
                move |bounds: Vec<gpui::Bounds<gpui::Pixels>>, _, _| {
                    if let Some(bounds) = bounds.first() {
                        bounds_cell.set(Some(*bounds));
                    }
                }
            })
            .child(
                crate::ui::button::button(
                    SharedString::from(format!("font-picker-{:?}-{}", role as u8, option_index)),
                    self.styles.settings_picker(),
                    Default::default(),
                    self.ui_scale,
                )
                .w_full()
                .justify_start()
                .overflow_hidden()
                .truncate()
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
            .gap(px(self.scaled(
                self.styles.ui.space(crate::style::SpaceStep::Three),
            )))
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
                        Some(crate::editor::shortcut::hint("secondary-minus")),
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
                        Some(crate::editor::shortcut::hint("secondary-=")),
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
        shortcut: Option<SharedString>,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let style = self.styles.settings_stepper();
        let button = crate::ui::button::icon_button(id, style, Default::default(), self.ui_scale)
            .rounded(px(0.0))
            .when(increase, |this| {
                this.rounded_r(px(self.scaled(style.radius_px)))
            })
            .when(!increase, |this| {
                this.rounded_l(px(self.scaled(style.radius_px)))
            })
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
            .gap(px(self.scaled(
                self.styles.ui.space(crate::style::SpaceStep::Three),
            )))
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

    // ── Editor font size stepper ────────────────────────────────

    fn render_editor_font_size_row(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.ensure_font_size_editor(cx);
        let text_secondary = self.styles.settings_panel.settings_secondary_text_color;
        let border_color = self.styles.settings_panel.settings_contrast_border_color;

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(self.scaled(
                self.styles.ui.space(crate::style::SpaceStep::Three),
            )))
            .child(
                div()
                    .w(px(self.scaled(120.0)))
                    .text_size(px(self.scaled(13.0)))
                    .text_color(text_secondary)
                    .child(self.t(TextKey::EditorFontSize)),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(0.0))
                    .child(self.render_settings_stepper_button(
                        "editor-font-size-dec",
                        "−",
                        false,
                        None,
                        cx,
                        |this, cx| {
                            let current = this.app_settings.font_size;
                            this.apply_font_size(current - 1.0, cx);
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
                            .when_some(self.settings_font_size_editor.clone(), |this, editor| {
                                this.child(editor)
                            }),
                    )
                    .child(self.render_settings_stepper_button(
                        "editor-font-size-inc",
                        "+",
                        true,
                        None,
                        cx,
                        |this, cx| {
                            let current = this.app_settings.font_size;
                            this.apply_font_size(current + 1.0, cx);
                        },
                    )),
            )
            .into_any_element()
    }

    // ── Terminal font size stepper ──────────────────────────────

    fn render_terminal_font_size_row(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let text_secondary = self.styles.settings_panel.settings_secondary_text_color;
        let border_color = self.styles.settings_panel.settings_contrast_border_color;

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(self.scaled(
                self.styles.ui.space(crate::style::SpaceStep::Three),
            )))
            .child(
                div()
                    .w(px(self.scaled(120.0)))
                    .text_size(px(self.scaled(13.0)))
                    .text_color(text_secondary)
                    .child(terminal_settings_label(self.ui_language, "font_size").to_string()),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(0.0))
                    .child(self.render_settings_stepper_button(
                        "terminal-font-size-dec",
                        "−",
                        false,
                        None,
                        cx,
                        |this, cx| {
                            let current = this.app_settings.terminal_font_size;
                            this.apply_terminal_font_size(current - 1.0, cx);
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
                                self.settings_terminal_font_size_editor.clone(),
                                |this, editor| this.child(editor),
                            ),
                    )
                    .child(self.render_settings_stepper_button(
                        "terminal-font-size-inc",
                        "+",
                        true,
                        None,
                        cx,
                        |this, cx| {
                            let current = this.app_settings.terminal_font_size;
                            this.apply_terminal_font_size(current + 1.0, cx);
                        },
                    )),
            )
            .into_any_element()
    }

    pub(crate) fn apply_terminal_font_size(&mut self, value: f32, cx: &mut Context<Self>) {
        let value = value.round().clamp(8.0, 32.0);
        self.app_settings.terminal_font_size = value;
        if let Some(editor) = &self.settings_terminal_font_size_editor {
            self.sync_inline_stepper_editor(editor, format!("{value:.0}"), cx);
        }
        self.finalize_settings_change(cx);
    }

    fn ensure_terminal_font_size_editor(&mut self, cx: &mut Context<Self>) {
        if self.settings_terminal_font_size_editor.is_some() {
            return;
        }
        let editor = self.build_inline_stepper_editor(
            format!("{:.0}", self.app_settings.terminal_font_size),
            cx,
        );
        cx.subscribe(
            &editor,
            |this, editor, event: &crate::text_input::TextInputEvent, cx| match event {
                crate::text_input::TextInputEvent::Submit => {
                    let text = editor.read(cx).text().to_string();
                    if let Some(val) = text.parse::<f32>().ok().filter(|value| value.is_finite()) {
                        this.apply_terminal_font_size(val, cx);
                    }
                }
                crate::text_input::TextInputEvent::Escape => {
                    let current = this.app_settings.terminal_font_size;
                    this.sync_inline_stepper_editor(&editor, format!("{current:.0}"), cx);
                }
                _ => {}
            },
        )
        .detach();
        self.settings_terminal_font_size_editor = Some(editor);
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
            .gap(px(self.scaled(
                self.styles.ui.space(crate::style::SpaceStep::Three),
            )))
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
            .gap(px(self.scaled(
                self.styles.ui.space(crate::style::SpaceStep::Three),
            )))
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
                            .w(px(self.scaled(
                                self.styles
                                    .settings_panel
                                    .settings_content_width_input_width_px,
                            )))
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
                [
                    "current_file",
                    "workspace_root",
                    "workspace_folder",
                    "current_file_subfolder",
                    "external_folder",
                ]
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
            FontRole::LinkSyntax | FontRole::LinkPathStyle => {
                let values: &[&str] = if role == FontRole::LinkSyntax {
                    &["markdown", "wikilink"]
                } else {
                    &["relative", "shortest"]
                };
                (
                    values
                        .iter()
                        .map(|value| {
                            let label =
                                attachment_settings_label(self.ui_language, value).to_string();
                            PickerOption {
                                value: (*value).to_string(),
                                search_text: label.to_lowercase(),
                                label,
                                preview_font: None,
                                icon_path: None,
                            }
                        })
                        .collect(),
                    attachment_settings_label(self.ui_language, "search").to_string(),
                )
            }
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
            (FontRole::TerminalFont, _) => {
                if self.app_settings.terminal_font.is_empty() {
                    self.app_settings.code_font.clone()
                } else {
                    self.app_settings.terminal_font.clone()
                }
            }
            (FontRole::InterfaceLanguage, _) => self.ui_language.code().to_string(),
            (FontRole::DateTimeFormat, _) => self.app_settings.date_time_format().to_string(),
            (FontRole::AttachmentLocation, _) => self.app_settings.attachment_location.clone(),
            (FontRole::LinkSyntax, _) => self.app_settings.link_syntax.clone(),
            (FontRole::LinkPathStyle, _) => self.app_settings.link_path_style.clone(),
            (FontRole::TerminalCursorShape, _) => self
                .app_settings
                .terminal_cursor_shape()
                .as_str()
                .to_string(),
            (FontRole::TerminalLaunchCommand, _) => {
                self.app_settings.terminal_launch_command.clone()
            }
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
        let settings_px = self.scaled(self.styles.ui.spec.settings_padding_x_px);
        let settings_content_w = self.settings_content_width(self.last_window_width);
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
            desired_width: f32::from(button_bounds.size.width).max(self.scaled(260.0)),
            match_anchor_width: false,
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
                    self.styles.ui_font_family = crate::platform::fonts::intern_family(&font_name);
                    self.app_settings.ui_font = font_name;
                }
                (FontRole::BodyFont, 0) => {
                    let interned = crate::platform::fonts::intern_family(&font_name);
                    self.styles.editor.editor_body_latin_font_family = interned;
                    self.styles.editor.editor_heading_latin_font_family = interned;
                    self.app_settings.body_latin_font = font_name;
                }
                (FontRole::BodyFont, 1) => {
                    let interned = crate::platform::fonts::intern_family(&font_name);
                    self.styles.editor.editor_body_cjk_font_family = interned;
                    self.styles.editor.editor_heading_cjk_font_family = interned;
                    self.app_settings.body_cjk_font = font_name;
                }
                (FontRole::CodeFont, _) => {
                    self.styles.editor.editor_monospace_font_family =
                        crate::platform::fonts::intern_family(&font_name);
                    self.app_settings.code_font = font_name;
                }
                (FontRole::TerminalFont, _) => {
                    self.app_settings.terminal_font = font_name;
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
                (FontRole::LinkSyntax, _) => {
                    self.app_settings.link_syntax = font_name;
                }
                (FontRole::LinkPathStyle, _) => {
                    self.app_settings.link_path_style = font_name;
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
                _ => {}
            }
            self.app_settings.save();
            self.clear_all_tab_layout_caches();
            if matches!(picker.role, FontRole::CodeFont)
                || matches!((picker.role, picker.option_index), (FontRole::BodyFont, 1))
            {
                self.invalidate_generated_block_preview_render_caches();
                self.rerender_generated_block_previews();
            }
            cx.notify();
        }
    }
}

/// Applies the default application icon tile to the macOS Dock at runtime via
/// `NSApplication setApplicationIconImage:`. No-op on other platforms.
#[cfg(target_os = "macos")]
pub(crate) fn apply_app_icon_to_dock() {
    use objc::{class, msg_send, sel, sel_impl};

    let Some(png) = build_padded_dock_icon() else {
        return;
    };

    unsafe {
        let data: *mut objc::runtime::Object =
            msg_send![class!(NSData), dataWithBytes: png.as_ptr() length: png.len()];
        let image: *mut objc::runtime::Object = msg_send![class!(NSImage), alloc];
        let image: *mut objc::runtime::Object = msg_send![image, initWithData: data];
        if image.is_null() {
            return;
        }
        let app: *mut objc::runtime::Object = msg_send![class!(NSApplication), sharedApplication];
        let () = msg_send![app, setApplicationIconImage: image];
        let () = msg_send![image, release];
    }
}

/// Renders the embedded tile onto a padded, transparent canvas so the Dock
/// shows it at the standard macOS app-icon size, returning PNG bytes in memory.
/// A soft drop shadow is baked in so the Dock icon gains the same depth as the
/// bundled app icon.
#[cfg(target_os = "macos")]
fn build_padded_dock_icon() -> Option<Vec<u8>> {
    use image::{Rgba, RgbaImage, imageops};

    // Match the .icns margin: artwork fills ~80.5% of the canvas, centered.
    const CANVAS: u32 = 1024;
    const CONTENT: u32 = 824;
    // Soft drop shadow tuned to sit within the transparent margin.
    const SHADOW_OFFSET_Y: i64 = 10;
    const SHADOW_BLUR: f32 = 16.0;
    const SHADOW_STRENGTH: f32 = 0.28;

    let tile_bytes = crate::embedded_asset(crate::editor::helpers::APP_ICON_PATH)?;
    let tile = image::load_from_memory(tile_bytes).ok()?.to_rgba8();
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

    let mut output = std::io::Cursor::new(Vec::new());
    canvas.write_to(&mut output, image::ImageFormat::Png).ok()?;
    Some(output.into_inner())
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn apply_app_icon_to_dock() {}

#[cfg(test)]
mod tests {
    use super::super::app_settings::AppSettings;
    use super::*;
    use gpui::Styled;

    #[test]
    fn settings_header_keeps_actions_in_one_scrollable_row() {
        for scale in [1.0, 1.5, 2.0] {
            let mut row = settings_header_row(12.0 * scale);
            assert_eq!(row.style().flex_direction, Some(gpui::FlexDirection::Row));
            assert_eq!(row.style().flex_wrap, Some(gpui::FlexWrap::Wrap));
            let mut actions = settings_header_actions(8.0 * scale);
            assert_eq!(
                actions.style().flex_direction,
                Some(gpui::FlexDirection::Row)
            );
            assert_eq!(actions.style().flex_wrap, Some(gpui::FlexWrap::NoWrap));
            assert_eq!(actions.style().overflow.x, Some(gpui::Overflow::Scroll));
            assert_eq!(actions.style().justify_content, None);
            assert_eq!(
                actions.style().max_size.width,
                Some(gpui::relative(1.0).into())
            );
            assert_eq!(
                settings_header_button_max_width(640.0 * scale, 40.0 * scale, 150.0 * scale),
                150.0 * scale
            );
            assert_eq!(
                settings_header_button_max_width(120.0 * scale, 40.0 * scale, 150.0 * scale),
                40.0 * scale
            );
        }
    }

    #[test]
    fn settings_header_original_labels_are_localized() {
        for &language in UiLanguage::supported() {
            for label in [
                crate::i18n::check_updates_label(language),
                crate::i18n::config_file_label(language),
                crate::i18n::translate(language, TextKey::HelpManual),
                crate::i18n::translate(language, TextKey::OfficialWebsite),
            ] {
                assert!(!label.trim().is_empty(), "{language:?}");
            }
        }
        assert_eq!(
            crate::i18n::check_updates_label(UiLanguage::ChineseSimplified),
            "检查更新"
        );
        assert_eq!(
            crate::i18n::config_file_label(UiLanguage::ChineseSimplified),
            "配置文件"
        );
        assert_eq!(
            crate::i18n::translate(UiLanguage::ChineseSimplified, TextKey::HelpManual),
            "帮助手册"
        );
        assert_eq!(
            crate::i18n::translate(UiLanguage::ChineseSimplified, TextKey::OfficialWebsite),
            "官方网站"
        );
    }

    #[test]
    fn attachment_folder_results_reject_superseded_requests_and_changed_settings() {
        let original = AppSettings::default();
        let first = Arc::new(());
        let latest = Arc::new(());
        let latest_token = Arc::downgrade(&latest);
        assert!(!attachment_folder_result_is_current(
            &first,
            Some(&latest_token),
            &original,
            &original
        ));
        assert!(attachment_folder_result_is_current(
            &latest,
            Some(&latest_token),
            &original,
            &original
        ));
        assert!(!attachment_folder_result_is_current(
            &latest, None, &original, &original
        ));
        let mut changed = original.clone();
        changed.attachment_external_folder = "/newer/folder".into();
        assert!(!attachment_folder_result_is_current(
            &latest,
            Some(&latest_token),
            &original,
            &changed
        ));
        changed = original.clone();
        changed.attachment_location = "current_folder".into();
        assert!(!attachment_folder_result_is_current(
            &latest,
            Some(&latest_token),
            &original,
            &changed
        ));
        changed = original.clone();
        changed.attachment_external_folder_bookmark = "newer bookmark".into();
        assert!(!attachment_folder_result_is_current(
            &latest,
            Some(&latest_token),
            &original,
            &changed
        ));
    }

    #[test]
    fn attachment_folder_bookmark_preserves_missing_and_not_directory_errors() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        assert_eq!(
            capture_attachment_folder_bookmark(&missing)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::NotFound
        );
        let file = dir.path().join("not-a-folder");
        std::fs::write(&file, b"test").unwrap();
        assert_eq!(
            capture_attachment_folder_bookmark(&file)
                .unwrap_err()
                .kind(),
            std::io::ErrorKind::NotADirectory
        );
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn attachment_folders_do_not_require_bookmarks_on_other_platforms() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(capture_attachment_folder_bookmark(dir.path()).unwrap(), "");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn dock_icon_is_built_from_embedded_pixels_as_padded_png_bytes() {
        let png = build_padded_dock_icon().expect("embedded Dock artwork");
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        let icon = image::load_from_memory(&png).unwrap().to_rgba8();
        assert_eq!(icon.dimensions(), (1024, 1024));
        assert_eq!(icon.get_pixel(0, 0)[3], 0);
        assert_eq!(icon.get_pixel(1023, 1023)[3], 0);
        assert!(icon.get_pixel(512, 512)[3] > 0);
    }
}
