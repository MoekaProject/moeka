use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, Context, CursorStyle, Focusable, MouseButton, ScrollHandle, SharedString, Window,
    div, img, prelude::*, px, svg,
};

use crate::i18n::{TextKey, UiLanguage, language_picker_options};
use crate::theme::{EditorThemeMode, ThemeKind, hex_to_hsla};

use super::{
    DOC_SVG_PATH, EditorView, FLOWER_SVG_PATH, MOUNTAIN_SVG_PATH, SITE_SVG_PATH,
    THEME_DARK_MINI_SVG_PATH, THEME_LIGHT_MINI_SVG_PATH,
};

/// Duration of the app-icon tile opacity fade on hover enter/leave.
const APP_ICON_HOVER_SECONDS: f32 = 0.16;
/// Resting and hovered opacity for unselected app-icon tiles.
const APP_ICON_REST_OPACITY: f32 = 0.5;
const APP_ICON_HOVER_OPACITY: f32 = 0.9;

// ── Font picker state ───────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum FontRole {
    UiFont,
    BodyFont,
    CodeFont,
    InterfaceLanguage,
    DateTimeFormat,
    /// Font family for selected whiteboard elements (whiteboard panel).
    WhiteboardFont,
}

#[derive(Clone)]
pub(crate) struct PickerOption {
    pub value: String,
    pub label: String,
    pub search_text: String,
    pub preview_font: Option<String>,
}

#[derive(Clone)]
pub(crate) struct FontPickerState {
    pub role: FontRole,
    pub option_index: usize,
    pub all_options: Vec<PickerOption>,
    pub filtered: Vec<PickerOption>,
    pub selected_index: usize,
    pub scroll_handle: ScrollHandle,
    /// Locked direction: once decided, keep dropdown above or below.
    pub show_above: std::cell::Cell<Option<bool>>,
}

// ── Settings panel rendering ────────────────────────────────────────

impl EditorView {
    pub(crate) fn render_settings_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bg = self.styles.editor.editor_bg;

        div()
            .id("settings-panel")
            .flex_1()
            .size_full()
            .overflow_y_scroll()
            .track_scroll(&self.settings_scroll_handle)
            .bg(bg)
            .text_size(px(self.scaled(self.styles.ui_font_size_px)))
            .child(
                div()
                    .max_w(px(self.scaled(560.0)))
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
                            .items_center()
                            .justify_between()
                            .gap(px(self.scaled(12.0)))
                            .child(
                                div()
                                    .text_size(px(self.scaled(self.styles.ui_font_size_px + 9.0)))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(self.theme.text_bright)
                                    .child(self.t(TextKey::SettingsPanelTitle)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .gap(px(self.scaled(8.0)))
                                    .child(self.render_settings_help_button(cx))
                                    .child(self.render_settings_site_button(cx)),
                            ),
                    )
                    .child(self.render_theme_section(cx))
                    .child(self.render_font_section(window, cx))
                    .child(self.render_format_section(cx))
                    .child(self.render_app_icon_section(cx)),
            )
            .into_any_element()
    }

    fn render_settings_help_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let height = self.scaled(30.0);
        let icon_size = self.scaled(15.0);

        div()
            .id("settings-help-button")
            .h(px(height))
            .px(px(self.scaled(12.0)))
            .flex()
            .items_center()
            .gap(px(self.scaled(6.0)))
            .rounded(px(self.scaled(8.0)))
            .border(px(1.0))
            .border_color(self.theme.border_primary)
            .bg(self.theme.bg_secondary)
            .text_size(px(self.scaled(self.styles.ui_font_size_px - 1.0)))
            .text_color(self.theme.text_primary)
            .cursor(CursorStyle::Arrow)
            .hover({
                let hover_bg = self.theme.bg_hover;
                move |style| style.bg(hover_bg)
            })
            .child(
                svg()
                    .path(DOC_SVG_PATH)
                    .size(px(icon_size))
                    .text_color(self.theme.text_secondary),
            )
            .child(self.t(TextKey::HelpManual))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.open_welcome_tab(cx);
                }),
            )
            .into_any_element()
    }

    fn render_settings_site_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let height = self.scaled(30.0);
        let icon_size = self.scaled(15.0);
        let update_ready = self.prepared_update().is_some();
        let update_color = hex_to_hsla("#d6a100");
        let text_color = if update_ready {
            update_color
        } else {
            self.theme.text_primary
        };
        let icon_color = if update_ready {
            update_color
        } else {
            self.theme.text_secondary
        };
        let label = if let Some(update) = self.prepared_update() {
            crate::i18n::restart_to_install_label(self.ui_language, Some(&update.version))
        } else {
            self.t(TextKey::OfficialWebsite).to_string()
        };

        div()
            .id("settings-site-button")
            .h(px(height))
            .px(px(self.scaled(12.0)))
            .flex()
            .items_center()
            .gap(px(self.scaled(6.0)))
            .rounded(px(self.scaled(8.0)))
            .border(px(1.0))
            .border_color(if update_ready {
                update_color
            } else {
                self.theme.border_primary
            })
            .bg(self.theme.bg_secondary)
            .text_size(px(self.scaled(self.styles.ui_font_size_px - 1.0)))
            .text_color(text_color)
            .cursor(CursorStyle::Arrow)
            .hover({
                let hover_bg = self.theme.bg_hover;
                move |style| style.bg(hover_bg)
            })
            .child(
                svg()
                    .path(SITE_SVG_PATH)
                    .size(px(icon_size))
                    .text_color(icon_color),
            )
            .child(label)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    if this.prepared_update().is_some() {
                        this.install_prepared_update();
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

    // ── App icon section ────────────────────────────────────────

    fn render_app_icon_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let selected = self.app_settings.app_icon_index;

        div()
            .flex()
            .flex_col()
            .gap(px(self.scaled(12.0)))
            .child(self.render_section_heading(self.t(TextKey::AppIconSection)))
            .child(
                div().flex().flex_row().justify_between().children(
                    (0..crate::editor::helpers::APP_ICON_COUNT)
                        .map(|index| self.render_app_icon_card(index, index == selected, cx)),
                ),
            )
            .into_any_element()
    }

    fn render_app_icon_card(
        &self,
        index: usize,
        is_active: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        // Square button sized so all nine sit in a single row; the corner radius
        // matches the standard macOS icon (~22.37% of the side).
        let size = self.scaled(46.0);
        let radius = size * 0.2237;
        let opacity = self.app_icon_tile_opacity(index, is_active);
        let image_path = crate::editor::helpers::resolve_asset_image_path(
            &crate::editor::helpers::app_icon_asset_path(index),
        );

        div()
            .id(SharedString::from(format!("app-icon-{index}")))
            .w(px(size))
            .h(px(size))
            .rounded(px(radius))
            .cursor(CursorStyle::Arrow)
            .on_hover(cx.listener(move |this, hovering: &bool, _, cx| {
                this.set_app_icon_hovered(index, *hovering, cx);
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    this.set_app_icon(index, cx);
                }),
            )
            .child(
                img(image_path)
                    .size_full()
                    .rounded(px(radius))
                    .opacity(opacity),
            )
            .into_any_element()
    }

    /// Opacity for an app-icon tile: selected tiles stay fully opaque, others
    /// rest at 0.5 and animate toward 0.9 while hovered (and back on leave).
    fn app_icon_tile_opacity(&self, index: usize, is_active: bool) -> f32 {
        if is_active {
            return 1.0;
        }
        let progress = self
            .app_icon_hover_since
            .get()
            .map(|since| (since.elapsed().as_secs_f32() / APP_ICON_HOVER_SECONDS).clamp(0.0, 1.0))
            .unwrap_or(1.0);
        let ease = super::ease_in_out_cubic(progress);
        let span = APP_ICON_HOVER_OPACITY - APP_ICON_REST_OPACITY;
        if self.app_icon_hovered == Some(index) {
            APP_ICON_REST_OPACITY + span * ease
        } else if self.app_icon_hover_prev == Some(index) {
            APP_ICON_HOVER_OPACITY - span * ease
        } else {
            APP_ICON_REST_OPACITY
        }
    }

    fn set_app_icon_hovered(&mut self, index: usize, hovering: bool, cx: &mut Context<Self>) {
        let target = if hovering {
            Some(index)
        } else if self.app_icon_hovered == Some(index) {
            None
        } else {
            self.app_icon_hovered
        };
        if target == self.app_icon_hovered {
            return;
        }
        self.app_icon_hover_prev = self.app_icon_hovered;
        self.app_icon_hovered = target;
        self.app_icon_hover_since
            .set(Some(std::time::Instant::now()));

        // Drive the opacity fade by re-rendering every frame for its duration.
        cx.spawn(async move |this, cx| {
            let frames = ((APP_ICON_HOVER_SECONDS / 0.016).ceil() as usize).max(1) + 1;
            for _ in 0..frames {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn set_app_icon(&mut self, index: usize, cx: &mut Context<Self>) {
        if index >= crate::editor::helpers::APP_ICON_COUNT
            || self.app_settings.app_icon_index == index
        {
            return;
        }
        self.app_settings.app_icon_index = index;
        self.app_settings.save();
        apply_app_icon_to_dock(index);
        cx.notify();
    }

    fn render_editor_display_options(&self, cx: &mut Context<Self>) -> AnyElement {
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(self.scaled(12.0)))
            .child(
                div()
                    .w(px(self.scaled(120.0)))
                    .text_size(px(self.scaled(13.0)))
                    .text_color(self.theme.text_secondary)
                    .child(crate::i18n::settings_label(
                        self.ui_language,
                        "editor_display",
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(
                        self.styles.settings_panel.settings_option_button_gap_px,
                    )))
                    .child(self.render_settings_option_button(
                        "settings-show-whitespace",
                        crate::i18n::settings_label(self.ui_language, "show_whitespace"),
                        self.app_settings.show_whitespace,
                        cx,
                        |this, cx| {
                            this.app_settings.show_whitespace = !this.app_settings.show_whitespace;
                            this.finalize_settings_change(cx);
                        },
                    ))
                    .child(self.render_settings_option_button(
                        "settings-highlight-line-background",
                        crate::i18n::settings_label(self.ui_language, "highlight_line"),
                        self.app_settings.highlight_line_background,
                        cx,
                        |this, cx| {
                            this.app_settings.highlight_line_background =
                                !this.app_settings.highlight_line_background;
                            this.finalize_settings_change(cx);
                        },
                    )),
            )
            .into_any_element()
    }

    fn render_settings_option_button(
        &self,
        id: &'static str,
        label: &'static str,
        active: bool,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let styles = &self.styles.settings_panel;
        let bg = if active {
            styles.settings_option_button_active_bg
        } else {
            styles.settings_option_button_bg
        };
        let text_color = if active {
            styles.settings_option_button_active_text_color
        } else {
            styles.settings_option_button_text_color
        };
        let border_color = if active {
            styles.settings_option_button_active_border_color
        } else {
            styles.settings_option_button_border_color
        };
        let hover_bg = styles.settings_option_button_hover_bg;

        div()
            .id(id)
            .h(px(self.scaled(styles.settings_option_button_height_px)))
            .px(px(self.scaled(styles.settings_option_button_padding_x_px)))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(self.scaled(styles.settings_option_button_radius_px)))
            .border(px(
                self.scaled(styles.settings_option_button_border_width_px)
            ))
            .border_color(border_color)
            .bg(bg)
            .text_color(text_color)
            .text_size(px(self.scaled(styles.settings_option_button_font_size_px)))
            .cursor(CursorStyle::Arrow)
            .when(!active, move |this| {
                this.hover(move |style| style.bg(hover_bg))
            })
            .child(label)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    on_click(this, cx);
                }),
            )
            .into_any_element()
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
                }
            })
            .collect()
    }

    fn date_time_format_preview_label(&self, format: &str) -> String {
        chrono::Local::now().format(format).to_string()
    }

    // ── Section heading ─────────────────────────────────────────

    fn render_section_heading(&self, label: &str) -> AnyElement {
        let border_color = self.theme.border_secondary;
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
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(self.theme.text_secondary)
                    .child(heading_label),
            )
            .child(div().w_full().h(px(1.0)).bg(border_color))
            .into_any_element()
    }

    fn inline_stepper_input_styles(&self) -> crate::style::TextInputStyles {
        let mut styles = self.styles.text_input.clone();
        styles.text_input_bg = gpui::Hsla::transparent_black();
        styles.text_input_focus_bg = gpui::Hsla::transparent_black();
        styles.text_input_border_width_px = 0.0;
        styles.text_input_padding_left_px = 2.0;
        styles.text_input_padding_right_px = 2.0;
        styles.text_input_height_px = 28.0;
        styles.text_input_center = true;
        styles.text_input_cursor_height_ratio = 0.6;
        styles
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
        let text_secondary = self.theme.text_secondary;
        let hover_bg = self.theme.bg_hover;
        let toggle_active_bg = self.theme.text_primary;
        let toggle_active_text = self.theme.bg_primary;
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
                            .rounded(px(toggle_radius))
                            .border(px(toggle_border_width))
                            .border_color(self.theme.border_primary)
                            .overflow_hidden()
                            .child(
                                div()
                                    .id("theme-toggle-light")
                                    .rounded_l(px(toggle_radius))
                                    .h(px(toggle_height))
                                    .px(px(toggle_padding_x))
                                    .flex()
                                    .items_center()
                                    .gap(px(toggle_gap))
                                    .text_size(px(option_font_size))
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
                                    .h(px(toggle_height))
                                    .px(px(toggle_padding_x))
                                    .flex()
                                    .items_center()
                                    .gap(px(toggle_gap))
                                    .text_size(px(option_font_size))
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
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(0.0))
                            .rounded(px(option_radius))
                            .border(px(option_border_width))
                            .border_color(option_border_color)
                            .overflow_hidden()
                            .children(EditorThemeMode::all().iter().enumerate().map(
                                |(index, mode)| {
                                    let is_active = *mode == editor_theme_mode;
                                    let mode = *mode;
                                    div()
                                        .id(SharedString::from(format!(
                                            "editor-theme-mode-{}",
                                            mode.as_str()
                                        )))
                                        .h(px(option_height))
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
                                            gpui::transparent_black()
                                        })
                                        .cursor(CursorStyle::Arrow)
                                        .when(!is_active, |this| {
                                            this.hover(move |s| s.bg(option_hover_bg))
                                        })
                                        .when(index > 0, |this| {
                                            this.border_l(px(option_border_width))
                                                .border_color(option_border_color)
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
                                },
                            )),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .justify_around()
                    .gap(px(self.scaled(10.0)))
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

        let swatch_size = self.scaled(12.0);

        div()
            .id(SharedString::from(format!("theme-{}", kind.label())))
            .flex_1()
            .py(px(self.scaled(10.0)))
            .px(px(self.scaled(10.0)))
            .flex()
            .flex_col()
            .gap(px(self.scaled(8.0)))
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
                    .text_size(px(self.scaled(11.0)))
                    .text_color(swatch_text)
                    .child(kind.label()),
            )
            .into_any_element()
    }

    // ── Font section ────────────────────────────────────────────

    fn render_font_section(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
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
            .into_any_element()
    }

    fn render_picker_row(
        &mut self,
        label: &str,
        current_value: &str,
        role: FontRole,
        option_index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let hover_bg = self.theme.bg_hover;
        let text_primary = self.theme.text_primary;
        let text_secondary = self.theme.text_secondary;
        let border_color = self.theme.border_primary;
        let current_value = current_value.to_string();

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
            .child({
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
                            if let Some(b) = bounds.first() {
                                bounds_cell.set(Some(*b));
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
                            .h(px(self.scaled(30.0)))
                            .px(px(self.scaled(10.0)))
                            .flex()
                            .items_center()
                            .rounded(px(self.scaled(6.0)))
                            .border(px(1.0))
                            .border_color(border_color)
                            .text_size(px(self.scaled(13.0)))
                            .text_color(text_primary)
                            .cursor(CursorStyle::Arrow)
                            .hover(move |s| s.bg(hover_bg))
                            .child(current_value)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &gpui::MouseDownEvent, window, cx| {
                                    window.prevent_default();
                                    this.open_font_picker(role, option_index, window, cx);
                                }),
                            ),
                    )
            })
            .into_any_element()
    }

    pub(crate) fn apply_font_size(&mut self, size: f32, cx: &mut Context<Self>) {
        let size = size.clamp(10.0, 30.0);
        self.styles.editor.editor_font_size_px = size;
        self.app_settings.font_size = size;
        if let Some(editor) = &self.settings_font_size_editor {
            self.sync_inline_stepper_editor(editor, format!("{size:.0}"), cx);
        }
        self.finalize_settings_change(cx);
    }

    pub(crate) fn ensure_font_size_editor(&mut self, cx: &mut Context<Self>) {
        if self.settings_font_size_editor.is_some() {
            return;
        }
        let editor = self.build_inline_stepper_editor(
            format!("{:.0}", self.styles.editor.editor_font_size_px),
            cx,
        );
        cx.subscribe(
            &editor,
            |this, editor, event: &crate::text_input::TextInputEvent, cx| match event {
                crate::text_input::TextInputEvent::Submit => {
                    let text = editor.read(cx).text().to_string();
                    if let Ok(size) = text.parse::<f32>() {
                        this.apply_font_size(size, cx);
                    }
                }
                crate::text_input::TextInputEvent::Escape => {
                    let current = this.styles.editor.editor_font_size_px;
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
        let text_primary = self.theme.text_primary;
        let text_secondary = self.theme.text_secondary;
        let hover_bg = self.theme.bg_hover;
        let border_color = self.theme.border_primary;

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
                    .child(
                        div()
                            .id("ui-scale-dec")
                            .w(px(self.scaled(30.0)))
                            .h(px(self.scaled(30.0)))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_l(px(self.scaled(6.0)))
                            .border(px(1.0))
                            .border_color(border_color)
                            .text_size(px(self.scaled(14.0)))
                            .text_color(text_primary)
                            .cursor(CursorStyle::Arrow)
                            .hover(move |s| s.bg(hover_bg))
                            .child("−")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    let current = this.ui_scale;
                                    this.apply_ui_scale(current - 0.1, cx);
                                }),
                            ),
                    )
                    .child(
                        div()
                            .w(px(self.scaled(50.0)))
                            .h(px(self.scaled(30.0)))
                            .border_y(px(1.0))
                            .border_color(border_color)
                            .flex()
                            .items_center()
                            .justify_center()
                            .when_some(
                                self.settings_ui_font_size_editor.clone(),
                                |this, editor| this.child(editor),
                            ),
                    )
                    .child(
                        div()
                            .id("ui-scale-inc")
                            .w(px(self.scaled(30.0)))
                            .h(px(self.scaled(30.0)))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_r(px(self.scaled(6.0)))
                            .border(px(1.0))
                            .border_color(border_color)
                            .text_size(px(self.scaled(14.0)))
                            .text_color(text_primary)
                            .cursor(CursorStyle::Arrow)
                            .hover(move |s| s.bg(hover_bg))
                            .child("+")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    let current = this.ui_scale;
                                    this.apply_ui_scale(current + 0.1, cx);
                                }),
                            ),
                    ),
            )
            .into_any_element()
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
                    if let Ok(scale) = text.parse::<f32>() {
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
        let text_primary = self.theme.text_primary;
        let text_secondary = self.theme.text_secondary;
        let hover_bg = self.theme.bg_hover;
        let border_color = self.theme.border_primary;

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
                    .child(
                        div()
                            .id("line-height-dec")
                            .w(px(self.scaled(30.0)))
                            .h(px(self.scaled(30.0)))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_l(px(self.scaled(6.0)))
                            .border(px(1.0))
                            .border_color(border_color)
                            .text_size(px(self.scaled(14.0)))
                            .text_color(text_primary)
                            .cursor(CursorStyle::Arrow)
                            .hover(move |s| s.bg(hover_bg))
                            .child("−")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    let current = this.app_settings.line_height;
                                    this.apply_line_height(current - 0.1, cx);
                                }),
                            ),
                    )
                    .child(
                        div()
                            .w(px(self.scaled(50.0)))
                            .h(px(self.scaled(30.0)))
                            .border_y(px(1.0))
                            .border_color(border_color)
                            .flex()
                            .items_center()
                            .justify_center()
                            .when_some(self.settings_line_height_editor.clone(), |this, editor| {
                                this.child(editor)
                            }),
                    )
                    .child(
                        div()
                            .id("line-height-inc")
                            .w(px(self.scaled(30.0)))
                            .h(px(self.scaled(30.0)))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_r(px(self.scaled(6.0)))
                            .border(px(1.0))
                            .border_color(border_color)
                            .text_size(px(self.scaled(14.0)))
                            .text_color(text_primary)
                            .cursor(CursorStyle::Arrow)
                            .hover(move |s| s.bg(hover_bg))
                            .child("+")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    let current = this.app_settings.line_height;
                                    this.apply_line_height(current + 0.1, cx);
                                }),
                            ),
                    ),
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
                    if let Ok(val) = text.parse::<f32>() {
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
        let text_primary = self.theme.text_primary;
        let text_secondary = self.theme.text_secondary;
        let hover_bg = self.theme.bg_hover;
        let border_color = self.theme.border_primary;

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
                    .child(
                        div()
                            .id("letter-spacing-dec")
                            .w(px(self.scaled(30.0)))
                            .h(px(self.scaled(30.0)))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_l(px(self.scaled(6.0)))
                            .border(px(1.0))
                            .border_color(border_color)
                            .text_size(px(self.scaled(14.0)))
                            .text_color(text_primary)
                            .cursor(CursorStyle::Arrow)
                            .hover(move |s| s.bg(hover_bg))
                            .child("−")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    let current = this.app_settings.letter_spacing;
                                    this.apply_letter_spacing(current - 0.5, cx);
                                }),
                            ),
                    )
                    .child(
                        div()
                            .w(px(self.scaled(50.0)))
                            .h(px(self.scaled(30.0)))
                            .border_y(px(1.0))
                            .border_color(border_color)
                            .flex()
                            .items_center()
                            .justify_center()
                            .when_some(
                                self.settings_letter_spacing_editor.clone(),
                                |this, editor| this.child(editor),
                            ),
                    )
                    .child(
                        div()
                            .id("letter-spacing-inc")
                            .w(px(self.scaled(30.0)))
                            .h(px(self.scaled(30.0)))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_r(px(self.scaled(6.0)))
                            .border(px(1.0))
                            .border_color(border_color)
                            .text_size(px(self.scaled(14.0)))
                            .text_color(text_primary)
                            .cursor(CursorStyle::Arrow)
                            .hover(move |s| s.bg(hover_bg))
                            .child("+")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    let current = this.app_settings.letter_spacing;
                                    this.apply_letter_spacing(current + 0.5, cx);
                                }),
                            ),
                    ),
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
                    if let Ok(val) = text.parse::<f32>() {
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
            FontRole::InterfaceLanguage => (
                language_picker_options(self.ui_language)
                    .into_iter()
                    .map(|option| PickerOption {
                        value: option.language.code().to_string(),
                        label: option.label,
                        search_text: option.search_text,
                        preview_font: None,
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
                        })
                        .collect::<Vec<_>>(),
                    self.t(TextKey::SearchFonts).to_string(),
                )
            }
        };

        self.settings_font_picker = Some(FontPickerState {
            role,
            option_index,
            filtered: all_options.clone(),
            all_options,
            selected_index: 0,
            scroll_handle: ScrollHandle::new(),
            show_above: std::cell::Cell::new(None),
        });

        // Create TextInput for search
        let mut styles = self.styles.text_input.clone();
        styles.text_input_bg = gpui::Hsla::transparent_black();
        styles.text_input_focus_bg = gpui::Hsla::transparent_black();
        styles.text_input_border_width_px = 0.0;
        styles.text_input_height_px = self.scaled(28.0);
        styles.text_input_font_size_px = self.scaled(13.0);
        styles.text_input_cursor_height_ratio = 1.0;
        styles.text_input_cursor_height_adjust_px = -self.scaled(15.0);
        let editor = cx.new(move |cx| crate::text_input::TextInput::new(styles, &placeholder, cx));
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
        let bg = self.theme.bg_elevated;
        let border_color = self.theme.border_primary;
        let text_primary = self.theme.text_primary;
        let hover_bg = self.theme.bg_hover;
        let selected_bg = self.theme.bg_selected;

        let max_visible = 12;
        let item_h = self.scaled(28.0);
        let items: Vec<AnyElement> = picker
            .filtered
            .iter()
            .take(200)
            .enumerate()
            .map(|(idx, option)| {
                let value = option.value.clone();
                let label = option.label.clone();
                let preview_font = option.preview_font.clone();
                let is_selected = idx == picker.selected_index;
                div()
                    .id(SharedString::from(format!("fp-item-{idx}")))
                    .w_full()
                    .h(px(item_h))
                    .px(px(self.scaled(10.0)))
                    .flex()
                    .items_center()
                    .text_size(px(self.scaled(13.0)))
                    .text_color(text_primary)
                    .when_some(preview_font, |this, preview_font| {
                        this.font_family(SharedString::from(preview_font))
                    })
                    .when(is_selected, |this| this.bg(selected_bg))
                    .hover(move |s| s.bg(hover_bg))
                    .cursor(CursorStyle::Arrow)
                    .child(label)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            window.prevent_default();
                            this.apply_font_selection(value.clone(), cx);
                        }),
                    )
                    .into_any_element()
            })
            .collect();

        // Determine position: left-align with the trigger button's left edge.
        // Layout: sidebar? | [workspace-pane: settings_panel(max_w=560, mx_auto, px=40, font_row: label=120 + gap=12 + button)] | utility panel?
        let border_w = self.styles.window.window_border_width_px;
        let sidebar_w = self.sidebar_width();
        let right_panel_w = self.right_panel_width();
        let pane_w = self.last_window_width - sidebar_w - right_panel_w - 2.0 * border_w;
        let settings_content_max_w = self.scaled(560.0);
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
        let (button_x, button_y, button_w_val, button_h) = match button_bounds {
            Some(b) => (
                f32::from(b.origin.x),
                f32::from(b.origin.y),
                f32::from(b.size.width),
                f32::from(b.size.height),
            ),
            None => (button_left, 200.0, button_w, self.scaled(30.0)),
        };

        // The whiteboard panel's picker matches its trigger button exactly;
        // the settings pickers keep a comfortable min/max width.
        let dropdown_w = if picker.role == FontRole::WhiteboardFont {
            button_w_val
        } else {
            button_w_val.min(self.scaled(400.0)).max(self.scaled(260.0))
        };
        let gap = self.scaled(4.0);
        let desired_visible = picker.filtered.len().min(200).min(max_visible).max(1);
        let desired_h = desired_visible as f32 * item_h + self.scaled(42.0);
        let window_h = self.last_window_height - 2.0 * border_w;
        let final_x = button_x.max(8.0);
        let below_y = button_y + button_h + gap;
        let available_below = (window_h - below_y - 8.0).max(0.0);
        let available_above = (button_y - gap - 8.0).max(0.0);
        // Lock direction on first render, keep it stable as height changes
        let show_above = match picker.show_above.get() {
            Some(locked) => locked,
            None => {
                let initial = desired_h > available_below && available_above > available_below;
                picker.show_above.set(Some(initial));
                initial
            }
        };
        let final_max_h = if show_above {
            desired_h.min(available_above).max(100.0)
        } else {
            desired_h.min(available_below).max(100.0)
        };
        let dropdown_y = if show_above {
            (button_y - final_max_h - gap).max(8.0)
        } else {
            below_y
        };

        let dropdown = div()
            .id("font-picker-dropdown")
            .absolute()
            .left(px(final_x))
            .top(px(dropdown_y))
            .w(px(dropdown_w))
            .max_h(px(final_max_h))
            .bg(bg)
            .border(px(1.0))
            .border_color(border_color)
            .rounded(px(self.scaled(8.0)))
            .shadow_md()
            .flex()
            .flex_col()
            .overflow_hidden()
            .cursor(CursorStyle::Arrow)
            // Stop mouse events from reaching the backdrop
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
            .on_mouse_move(cx.listener(|_, _, _, _| {}))
            .on_scroll_wheel(cx.listener(|_, _, _, _| {}))
            // Search input using TextInput component
            .child(
                div()
                    .w_full()
                    .px(px(self.scaled(6.0)))
                    .py(px(self.scaled(6.0)))
                    .border_b(px(1.0))
                    .border_color(border_color)
                    .when_some(self.settings_font_picker_editor.clone(), |this, editor| {
                        this.child(editor)
                    }),
            )
            // Font list
            .child(
                div()
                    .id("font-picker-list")
                    .flex_1()
                    .overflow_y_scroll()
                    .track_scroll(&picker.scroll_handle)
                    .children(items),
            );

        // Full-screen occluded backdrop — clicking it closes the picker
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
                    this.close_font_picker(cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| {
                    this.close_font_picker(cx);
                }),
            )
            .child(dropdown)
            .into_any_element()
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

/// Applies the selected application icon tile to the macOS Dock at runtime via
/// `NSApplication setApplicationIconImage:`. No-op on other platforms.
#[cfg(target_os = "macos")]
pub(crate) fn apply_app_icon_to_dock(index: usize) {
    use objc::{class, msg_send, sel, sel_impl};

    let Some(padded_path) = build_padded_dock_icon(index) else {
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
fn build_padded_dock_icon(index: usize) -> Option<std::path::PathBuf> {
    use image::{Rgba, RgbaImage, imageops};

    // Match the .icns margin: artwork fills ~80.5% of the canvas, centered.
    const CANVAS: u32 = 1024;
    const CONTENT: u32 = 824;
    // Soft drop shadow tuned to sit within the transparent margin.
    const SHADOW_OFFSET_Y: i64 = 10;
    const SHADOW_BLUR: f32 = 16.0;
    const SHADOW_STRENGTH: f32 = 0.28;

    let tile_path = crate::editor::helpers::resolve_asset_image_path(
        &crate::editor::helpers::app_icon_asset_path(index),
    );
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

    let out_path = std::env::temp_dir().join(format!("moeka-dock-icon-{index}.png"));
    canvas.save(&out_path).ok()?;
    Some(out_path)
}

#[cfg(not(target_os = "macos"))]
pub(crate) fn apply_app_icon_to_dock(_index: usize) {}
