use gpui::{
    AnyElement, Context, CursorStyle, Focusable, MouseButton, ScrollHandle, SharedString, Window,
    div, prelude::*, px,
};

use crate::theme::ThemeKind;

use super::EditorView;

// ── Font picker state ───────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
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
}

// ── Settings panel rendering ────────────────────────────────────────

impl EditorView {
    pub(crate) fn render_settings_panel(
        &self,
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
            .child(
                div()
                    .max_w(px(560.0))
                    .mx_auto()
                    .py(px(60.0))
                    .px(px(40.0))
                    .flex()
                    .flex_col()
                    .gap(px(32.0))
                    // Title
                    .child(
                        div()
                            .text_size(px(22.0))
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
            .gap(px(8.0))
            .child(
                div()
                    .text_size(px(12.0))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(self.theme.text_secondary)
                    .child(label.to_uppercase()),
            )
            .child(div().w_full().h(px(1.0)).bg(border_color))
            .into_any_element()
    }

    // ── Theme section ───────────────────────────────────────────

    fn render_theme_section(&self, cx: &mut Context<Self>) -> AnyElement {
        let themes = ThemeKind::all();
        let current = self.theme_kind;

        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(self.render_section_heading("主题"))
            .child(
                div().flex().flex_row().flex_wrap().gap(px(10.0)).children(
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
        let hover_bg = self.theme.bg_hover;

        let swatch_size = 12.0;

        div()
            .id(SharedString::from(format!("theme-{}", kind.label())))
            .w(px(100.0))
            .py(px(10.0))
            .px(px(10.0))
            .flex()
            .flex_col()
            .gap(px(8.0))
            .rounded(px(8.0))
            .border(px(if is_active { 2.0 } else { 1.0 }))
            .border_color(if is_active {
                active_accent
            } else {
                self.theme.border_primary
            })
            .bg(card_bg)
            .cursor(CursorStyle::Arrow)
            .hover(move |style| {
                if !is_active {
                    style.border_color(hover_bg)
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
            // Color swatches row
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(4.0))
                    .child(
                        div()
                            .w(px(swatch_size))
                            .h(px(swatch_size))
                            .rounded(px(2.0))
                            .bg(swatch_secondary_bg),
                    )
                    .child(
                        div()
                            .w(px(swatch_size))
                            .h(px(swatch_size))
                            .rounded(px(2.0))
                            .bg(swatch_text),
                    )
                    .child(
                        div()
                            .w(px(swatch_size))
                            .h(px(swatch_size))
                            .rounded(px(2.0))
                            .bg(swatch_accent),
                    )
                    .child(
                        div()
                            .w(px(swatch_size))
                            .h(px(swatch_size))
                            .rounded(px(2.0))
                            .bg(swatch_heading),
                    )
                    .child(
                        div()
                            .w(px(swatch_size))
                            .h(px(swatch_size))
                            .rounded(px(2.0))
                            .bg(swatch_code),
                    ),
            )
            // Theme name
            .child(
                div()
                    .text_size(px(11.0))
                    .text_color(swatch_text)
                    .child(kind.label()),
            )
            .into_any_element()
    }

    // ── Font section ────────────────────────────────────────────

    fn render_font_section(&self, _window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let es = &self.styles.editor;

        div()
            .flex()
            .flex_col()
            .gap(px(12.0))
            .child(self.render_section_heading("字体"))
            // UI font
            .child(self.render_font_row(
                "界面字体",
                self.styles.ui_font_family,
                FontRole::UiFont,
                0,
                cx,
            ))
            // UI font size
            .child(self.render_ui_font_size_row(cx))
            // Body font (Latin)
            .child(self.render_font_row(
                "编辑器正文字体",
                es.editor_body_latin_font_family,
                FontRole::BodyFont,
                0,
                cx,
            ))
            // Body font (CJK)
            .child(self.render_font_row(
                "编辑器正文字体 (CJK)",
                es.editor_body_cjk_font_family,
                FontRole::BodyFont,
                1,
                cx,
            ))
            // Code font
            .child(self.render_font_row(
                "编辑器代码块字体",
                es.editor_monospace_font_family,
                FontRole::CodeFont,
                0,
                cx,
            ))
            // Editor font size
            .child(self.render_font_size_row(cx))
            .into_any_element()
    }

    fn render_font_row(
        &self,
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
            .gap(px(12.0))
            .child(
                div()
                    .w(px(120.0))
                    .text_size(px(13.0))
                    .text_color(text_secondary)
                    .child(label.to_string()),
            )
            .child(
                div()
                    .id(SharedString::from(format!(
                        "font-picker-{:?}-{}",
                        role as u8, font_index
                    )))
                    .flex_1()
                    .h(px(30.0))
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .rounded(px(6.0))
                    .border(px(1.0))
                    .border_color(border_color)
                    .text_size(px(13.0))
                    .text_color(text_primary)
                    .cursor(CursorStyle::Arrow)
                    .hover(move |s| s.bg(hover_bg))
                    .child(font_name)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                            window.prevent_default();
                            this.open_font_picker(role, font_index, event.position, window, cx);
                        }),
                    ),
            )
            .into_any_element()
    }

    fn render_font_size_row(&self, cx: &mut Context<Self>) -> AnyElement {
        let text_primary = self.theme.text_primary;
        let text_secondary = self.theme.text_secondary;
        let hover_bg = self.theme.bg_hover;
        let border_color = self.theme.border_primary;

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.0))
            .child(
                div()
                    .w(px(120.0))
                    .text_size(px(13.0))
                    .text_color(text_secondary)
                    .child("编辑器字体大小"),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(0.0))
                    // Minus button
                    .child(
                        div()
                            .id("font-size-dec")
                            .w(px(30.0))
                            .h(px(30.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_l(px(6.0))
                            .border(px(1.0))
                            .border_color(border_color)
                            .text_size(px(14.0))
                            .text_color(text_primary)
                            .cursor(CursorStyle::Arrow)
                            .hover(move |s| s.bg(hover_bg))
                            .child("−")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    let current = this.styles.editor.editor_font_size_px;
                                    if current > 10.0 {
                                        this.apply_font_size(current - 1.0, cx);
                                    }
                                }),
                            ),
                    )
                    // Value display — editable TextInput
                    .child(
                        div()
                            .w(px(50.0))
                            .h(px(30.0))
                            .border_y(px(1.0))
                            .border_color(border_color)
                            .flex()
                            .items_center()
                            .justify_center()
                            .when_some(self.settings_font_size_editor.clone(), |this, editor| {
                                this.child(editor)
                            }),
                    )
                    // Plus button
                    .child(
                        div()
                            .id("font-size-inc")
                            .w(px(30.0))
                            .h(px(30.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_r(px(6.0))
                            .border(px(1.0))
                            .border_color(border_color)
                            .text_size(px(14.0))
                            .text_color(text_primary)
                            .cursor(CursorStyle::Arrow)
                            .hover(move |s| s.bg(hover_bg))
                            .child("+")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    let current = this.styles.editor.editor_font_size_px;
                                    if current < 30.0 {
                                        this.apply_font_size(current + 1.0, cx);
                                    }
                                }),
                            ),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn apply_font_size(&mut self, size: f32, cx: &mut Context<Self>) {
        let size = size.clamp(10.0, 30.0);
        self.styles.editor.editor_font_size_px = size;
        self.app_settings.font_size = size;
        self.app_settings.save();
        self.line_display_info.clear();
        // Sync the text input display
        if let Some(editor) = &self.settings_font_size_editor {
            editor.update(cx, |ti, cx| {
                let display = format!("{:.0}", size);
                if ti.text() != display {
                    ti.set_text(&display, cx);
                }
            });
        }
        cx.notify();
    }

    pub(crate) fn ensure_font_size_editor(&mut self, cx: &mut Context<Self>) {
        if self.settings_font_size_editor.is_some() {
            return;
        }
        let mut styles = self.styles.text_input.clone();
        // Customize for inline stepper: no border, transparent bg, centered text
        styles.text_input_bg = gpui::Hsla::transparent_black();
        styles.text_input_focus_bg = gpui::Hsla::transparent_black();
        styles.text_input_border_width_px = 0.0;
        styles.text_input_padding_x_px = 2.0;
        styles.text_input_height_px = 28.0;
        styles.text_input_center = true;
        styles.text_input_cursor_height_ratio = 0.6;
        let editor = cx.new(|cx| {
            let mut ti = crate::text_input::TextInput::new(styles, "", cx);
            ti.set_text(
                &format!("{:.0}", self.styles.editor.editor_font_size_px),
                cx,
            );
            ti
        });
        cx.subscribe(
            &editor,
            |this, editor, event: &crate::text_input::TextInputEvent, cx| {
                match event {
                    crate::text_input::TextInputEvent::Submit => {
                        let text = editor.read(cx).text().to_string();
                        if let Ok(size) = text.parse::<f32>() {
                            this.apply_font_size(size, cx);
                        }
                    }
                    crate::text_input::TextInputEvent::Escape => {
                        // Revert to current value
                        let current = this.styles.editor.editor_font_size_px;
                        editor.update(cx, |ti, cx| {
                            ti.set_text(&format!("{:.0}", current), cx);
                        });
                    }
                    crate::text_input::TextInputEvent::Changed => {}
                    _ => {}
                }
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
            .gap(px(12.0))
            .child(
                div()
                    .w(px(120.0))
                    .text_size(px(13.0))
                    .text_color(text_secondary)
                    .child("界面字体大小"),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(0.0))
                    .child(
                        div()
                            .id("ui-font-size-dec")
                            .w(px(30.0))
                            .h(px(30.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_l(px(6.0))
                            .border(px(1.0))
                            .border_color(border_color)
                            .text_size(px(14.0))
                            .text_color(text_primary)
                            .cursor(CursorStyle::Arrow)
                            .hover(move |s| s.bg(hover_bg))
                            .child("−")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    let current = this.styles.ui_font_size_px;
                                    if current > 10.0 {
                                        this.apply_ui_font_size(current - 1.0, cx);
                                    }
                                }),
                            ),
                    )
                    .child(
                        div()
                            .w(px(50.0))
                            .h(px(30.0))
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
                            .id("ui-font-size-inc")
                            .w(px(30.0))
                            .h(px(30.0))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_r(px(6.0))
                            .border(px(1.0))
                            .border_color(border_color)
                            .text_size(px(14.0))
                            .text_color(text_primary)
                            .cursor(CursorStyle::Arrow)
                            .hover(move |s| s.bg(hover_bg))
                            .child("+")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    let current = this.styles.ui_font_size_px;
                                    if current < 30.0 {
                                        this.apply_ui_font_size(current + 1.0, cx);
                                    }
                                }),
                            ),
                    ),
            )
            .into_any_element()
    }

    pub(crate) fn apply_ui_font_size(&mut self, size: f32, cx: &mut Context<Self>) {
        let size = size.clamp(10.0, 30.0);
        self.styles.ui_font_size_px = size;
        self.app_settings.ui_font_size = size;
        self.app_settings.save();
        if let Some(editor) = &self.settings_ui_font_size_editor {
            editor.update(cx, |ti, cx| {
                let display = format!("{:.0}", size);
                if ti.text() != display {
                    ti.set_text(&display, cx);
                }
            });
        }
        cx.notify();
    }

    pub(crate) fn ensure_ui_font_size_editor(&mut self, cx: &mut Context<Self>) {
        if self.settings_ui_font_size_editor.is_some() {
            return;
        }
        let mut styles = self.styles.text_input.clone();
        styles.text_input_bg = gpui::Hsla::transparent_black();
        styles.text_input_focus_bg = gpui::Hsla::transparent_black();
        styles.text_input_border_width_px = 0.0;
        styles.text_input_padding_x_px = 2.0;
        styles.text_input_height_px = 28.0;
        styles.text_input_center = true;
        styles.text_input_cursor_height_ratio = 0.6;
        let editor = cx.new(|cx| {
            let mut ti = crate::text_input::TextInput::new(styles, "", cx);
            ti.set_text(&format!("{:.0}", self.styles.ui_font_size_px), cx);
            ti
        });
        cx.subscribe(
            &editor,
            |this, editor, event: &crate::text_input::TextInputEvent, cx| {
                match event {
                    crate::text_input::TextInputEvent::Submit => {
                        let text = editor.read(cx).text().to_string();
                        if let Ok(size) = text.parse::<f32>() {
                            this.apply_ui_font_size(size, cx);
                        }
                    }
                    crate::text_input::TextInputEvent::Escape => {
                        let current = this.styles.ui_font_size_px;
                        editor.update(cx, |ti, cx| {
                            ti.set_text(&format!("{:.0}", current), cx);
                        });
                    }
                    crate::text_input::TextInputEvent::Changed => {}
                    _ => {}
                }
            },
        )
        .detach();
        self.settings_ui_font_size_editor = Some(editor);
    }

    // ── Font picker overlay ─────────────────────────────────────

    pub(crate) fn open_font_picker(
        &mut self,
        role: FontRole,
        font_index: usize,
        position: gpui::Point<gpui::Pixels>,
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
        });
        self.font_picker_trigger_position = Some(position);

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
        let item_h = 28.0_f32;
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
                    .px(px(10.0))
                    .flex()
                    .items_center()
                    .text_size(px(13.0))
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
        let sidebar_w = if self.sidebar_visible {
            self.sidebar_width()
        } else {
            0.0
        };
        let outline_w = if self.outline_visible {
            self.outline_width()
        } else {
            0.0
        };
        let pane_w = self.last_window_width - sidebar_w - outline_w - 2.0 * border_w;
        let settings_content_max_w = 560.0_f32;
        let settings_px = 40.0_f32;
        let settings_content_w = settings_content_max_w.min(pane_w);
        // mx_auto centers the content within the pane
        let settings_left = sidebar_w + border_w + (pane_w - settings_content_w) / 2.0;
        let button_left = settings_left + settings_px + 120.0 + 12.0;
        let button_w = settings_content_w - 2.0 * settings_px - 120.0 - 12.0;

        // Use the click position for Y, compute dropdown below the button
        let pos = self
            .font_picker_trigger_position
            .unwrap_or(gpui::point(gpui::px(200.0), gpui::px(200.0)));
        let click_y: f32 = pos.y.into();
        // Button is 30px tall — position the dropdown right below the button bottom
        // Estimate button top from click Y (click can be anywhere in the 30px button)
        // Use a fixed offset that looks good regardless of click position
        let dropdown_y = click_y + 18.0;

        let dropdown_w = button_w.min(400.0).max(260.0);
        let max_h = (max_visible as f32) * item_h + 42.0;
        let window_h = self.last_window_height - 2.0 * border_w;
        let final_x = button_left.max(8.0);
        // If dropdown would go below window, flip it above the button
        let dropdown_y = if dropdown_y + max_h > window_h - 8.0 {
            (click_y - max_h - 4.0).max(8.0)
        } else {
            dropdown_y
        };
        let final_max_h = max_h.min(window_h - dropdown_y - 8.0).max(100.0);

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
            .rounded(px(8.0))
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
                    .px(px(6.0))
                    .py(px(6.0))
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
                    self.styles.ui_font_family =
                        Box::leak(font_name.clone().into_boxed_str());
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
