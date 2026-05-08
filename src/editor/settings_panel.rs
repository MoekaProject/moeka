use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, Context, CursorStyle, Focusable, MouseButton, ScrollHandle, SharedString, Window,
    div, prelude::*, px,
};

use crate::theme::ThemeKind;

use super::EditorView;

// ── Font picker state ───────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum FontRole {
    UiFont,
    BodyFont,
    CodeFont,
}

#[derive(Clone)]
pub(crate) struct FontPickerState {
    pub role: FontRole,
    pub font_index: usize,
    pub filtered: Vec<String>,
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
                    // Title
                    .child(
                        div()
                            .text_size(px(self.scaled(self.styles.ui_font_size_px + 9.0)))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(self.theme.text_bright)
                            .child("设置"),
                    )
                    // Theme section
                    .child(self.render_theme_section(cx))
                    // Font settings
                    .child(self.render_font_section(window, cx)),
            )
            .into_any_element()
    }

    // ── Section heading ─────────────────────────────────────────

    fn render_section_heading(&self, label: &str) -> AnyElement {
        let border_color = self.theme.border_secondary;
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
                    .child(label.to_uppercase()),
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

    fn finalize_settings_change(&mut self, cx: &mut Context<Self>) {
        self.app_settings.save();
        self.line_display_info.clear();
        cx.notify();
    }

    // ── Theme section ───────────────────────────────────────────

    fn render_theme_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.theme_kind;
        let is_dark_mode = self.settings_theme_dark_mode;
        let themes = if is_dark_mode {
            ThemeKind::dark_themes()
        } else {
            ThemeKind::light_themes()
        };
        let text_secondary = self.theme.text_secondary;
        let border_color = self.theme.border_secondary;
        let hover_bg = self.theme.bg_hover;
        let toggle_active_bg = self.theme.text_primary;
        let toggle_active_text = self.theme.bg_primary;

        div()
            .flex()
            .flex_col()
            .gap(px(self.scaled(12.0)))
            .child(
                div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(self.scaled(8.0)))
                    .child(
                        div()
                            .w_full()
                            .flex()
                            .flex_row()
                            .items_end()
                            .gap(px(self.scaled(12.0)))
                            .child(
                                div()
                                    .flex_1()
                                    .text_size(px(self.scaled(12.0)))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(text_secondary)
                                    .child("主题".to_uppercase()),
                            )
                            .child(
                                div()
                                    .mb(px(self.scaled(5.0)))
                                    .flex()
                                    .flex_row()
                                    .gap(px(0.0))
                                    .rounded(px(self.scaled(5.0)))
                                    .border(px(1.0))
                                    .border_color(self.theme.border_primary)
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .id("theme-toggle-light")
                                            .rounded_l(px(self.scaled(5.0)))
                                            .px(px(self.scaled(12.0)))
                                            .py(px(self.scaled(4.0)))
                                            .text_size(px(self.scaled(11.0)))
                                            .cursor(CursorStyle::Arrow)
                                            .when(!is_dark_mode, |this| {
                                                this.bg(toggle_active_bg)
                                                    .text_color(toggle_active_text)
                                            })
                                            .when(is_dark_mode, |this| {
                                                this.text_color(text_secondary)
                                                    .hover(move |s| s.bg(hover_bg))
                                            })
                                            .child("浅色")
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
                                            .rounded_r(px(self.scaled(5.0)))
                                            .px(px(self.scaled(12.0)))
                                            .py(px(self.scaled(4.0)))
                                            .text_size(px(self.scaled(11.0)))
                                            .cursor(CursorStyle::Arrow)
                                            .when(is_dark_mode, |this| {
                                                this.bg(toggle_active_bg)
                                                    .text_color(toggle_active_text)
                                            })
                                            .when(!is_dark_mode, |this| {
                                                this.text_color(text_secondary)
                                                    .hover(move |s| s.bg(hover_bg))
                                            })
                                            .child("深色")
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
                            ),
                    )
                    .child(div().w_full().h(px(1.0)).bg(border_color)),
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
            .child(self.render_section_heading("字体"))
            // UI font
            .child(self.render_font_row("界面字体", ui_font, FontRole::UiFont, 0, cx))
            // UI scale
            .child(self.render_ui_font_size_row(cx))
            // Body font (Latin)
            .child(self.render_font_row("编辑器字体", body_latin, FontRole::BodyFont, 0, cx))
            // Body font (CJK)
            .child(self.render_font_row("编辑器字体(CJK)", body_cjk, FontRole::BodyFont, 1, cx))
            // Code font
            .child(self.render_font_row("编辑器代码字体", monospace, FontRole::CodeFont, 0, cx))
            // Line height
            .child(self.render_line_height_row(cx))
            // Letter spacing
            .child(self.render_letter_spacing_row(cx))
            .into_any_element()
    }

    fn render_font_row(
        &mut self,
        label: &str,
        current_font: &str,
        role: FontRole,
        font_index: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let hover_bg = self.theme.bg_hover;
        let text_primary = self.theme.text_primary;
        let text_secondary = self.theme.text_secondary;
        let border_color = self.theme.border_primary;
        let font_name = current_font.to_string();

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
                    .entry((role, font_index))
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
                                role as u8, font_index
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
                            .child(font_name)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _: &gpui::MouseDownEvent, window, cx| {
                                    window.prevent_default();
                                    this.open_font_picker(role, font_index, window, cx);
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
                    .child("界面缩放系数"),
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
                    .child("编辑器行间距"),
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
                    .child("编辑器字符间距"),
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
        font_index: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // Cache font names on first open
        if self.cached_font_names.is_none() {
            let mut names = window.text_system().all_font_names();
            names.sort_by(|a, b| a.to_lowercase().cmp(&b.to_lowercase()));
            names.dedup();
            // Filter out hidden/system fonts
            names.retain(|n| !n.starts_with('.'));
            self.cached_font_names = Some(names);
        }
        let all_fonts = self.cached_font_names.clone().unwrap_or_default();

        self.settings_font_picker = Some(FontPickerState {
            role,
            font_index,
            filtered: all_fonts,
            selected_index: 0,
            scroll_handle: ScrollHandle::new(),
            show_above: std::cell::Cell::new(None),
        });

        // Create TextInput for search
        let mut styles = self.styles.text_input.clone();
        styles.text_input_bg = gpui::Hsla::transparent_black();
        styles.text_input_focus_bg = gpui::Hsla::transparent_black();
        styles.text_input_border_width_px = 0.0;
        styles.text_input_cursor_height_ratio = 0.6;
        let editor = cx.new(|cx| crate::text_input::TextInput::new(styles, "搜索字体...", cx));
        cx.subscribe(
            &editor,
            |this, editor, event: &crate::text_input::TextInputEvent, cx| {
                match event {
                    crate::text_input::TextInputEvent::Changed => {
                        let query = editor.read(cx).text().to_lowercase();
                        if let (Some(picker), Some(all_fonts)) =
                            (&mut this.settings_font_picker, &this.cached_font_names)
                        {
                            if query.is_empty() {
                                picker.filtered = all_fonts.clone();
                            } else {
                                picker.filtered = all_fonts
                                    .iter()
                                    .filter(|name| name.to_lowercase().contains(&query))
                                    .cloned()
                                    .collect();
                            }
                            picker.selected_index = 0;
                            cx.notify();
                        }
                    }
                    crate::text_input::TextInputEvent::Submit => {
                        // Select current highlighted font
                        if let Some(picker) = &this.settings_font_picker {
                            if let Some(name) = picker.filtered.get(picker.selected_index).cloned()
                            {
                                this.apply_font_selection(name, cx);
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
            .map(|(idx, font_name)| {
                let name = font_name.clone();
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
                    .font_family(SharedString::from(name.clone()))
                    .when(is_selected, |this| this.bg(selected_bg))
                    .hover(move |s| s.bg(hover_bg))
                    .cursor(CursorStyle::Arrow)
                    .child(name.clone())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            window.prevent_default();
                            this.apply_font_selection(name.clone(), cx);
                        }),
                    )
                    .into_any_element()
            })
            .collect();

        // Determine position: left-align with the trigger button's left edge.
        // Layout: sidebar? | [workspace-pane: settings_panel(max_w=560, mx_auto, px=40, font_row: label=120 + gap=12 + button)] | outline?
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
            .get(&(picker.role, picker.font_index))
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

        let dropdown_w = button_w_val.min(self.scaled(400.0)).max(self.scaled(260.0));
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
            match (picker.role, picker.font_index) {
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
                _ => {}
            }
            self.app_settings.save();
            self.line_display_info.clear();
            cx.notify();
        }
    }
}
