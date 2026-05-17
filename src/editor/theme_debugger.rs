use gpui::{
    AnyElement, Bounds, Context, CursorStyle, Hsla, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, SharedString, Window, div, linear_color_stop, linear_gradient,
    prelude::*, px,
};

use crate::theme::{self, ThemeColorField};

use super::*;

pub(crate) const THEME_DEBUGGER_WIDTH_PX: f32 = 320.0;
const THEME_DEBUGGER_HEADER_HEIGHT_PX: f32 = 30.0;
const THEME_DEBUGGER_ROW_HEIGHT_PX: f32 = 30.0;
const THEME_DEBUGGER_SLIDER_HEIGHT_PX: f32 = 16.0;
const THEME_DEBUGGER_SLIDER_HANDLE_WIDTH_PX: f32 = 12.0;
const THEME_DEBUGGER_SV_HANDLE_SIZE_PX: f32 = 14.0;
const THEME_DEBUGGER_HUE_SEGMENTS: usize = 6;

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThemeDebuggerSection {
    Interface,
    Editor,
}

impl ThemeDebuggerSection {
    fn label_key(self) -> crate::i18n::TextKey {
        match self {
            Self::Interface => crate::i18n::TextKey::ThemeDebuggerInterface,
            Self::Editor => crate::i18n::TextKey::ThemeDebuggerEditor,
        }
    }

    fn fields(self) -> &'static [ThemeColorField] {
        match self {
            Self::Interface => &ThemeColorField::UI_FIELDS,
            Self::Editor => &ThemeColorField::EDITOR_FIELDS,
        }
    }

    fn default_field(self) -> ThemeColorField {
        match self {
            Self::Interface => ThemeColorField::TextPrimary,
            Self::Editor => ThemeColorField::Heading,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThemeDebuggerDragTarget {
    SaturationValue,
    Hue,
    Alpha,
}

#[derive(Clone, Copy)]
struct ThemeDebuggerHsva {
    h: f32,
    s: f32,
    v: f32,
    a: f32,
}

impl EditorView {
    pub(crate) fn refresh_theme_styles(
        &mut self,
        recreate_settings_editors: bool,
        cx: &mut Context<Self>,
    ) {
        self.styles = crate::style::Styles::from_theme(&self.theme);
        self.styles.editor.editor_font_size_px = self.app_settings.font_size;
        self.styles.editor.editor_paragraph_line_height_factor = self.app_settings.line_height;
        self.styles.editor.editor_heading_line_height_factor = self.app_settings.line_height;
        self.styles.editor.editor_letter_spacing_px = self.app_settings.letter_spacing;
        self.styles.ui_font_family = Box::leak(self.app_settings.ui_font.clone().into_boxed_str());
        self.styles.editor.editor_body_latin_font_family =
            Box::leak(self.app_settings.body_latin_font.clone().into_boxed_str());
        self.styles.editor.editor_body_cjk_font_family =
            Box::leak(self.app_settings.body_cjk_font.clone().into_boxed_str());
        self.styles.editor.editor_heading_latin_font_family =
            self.styles.editor.editor_body_latin_font_family;
        self.styles.editor.editor_heading_cjk_font_family =
            self.styles.editor.editor_body_cjk_font_family;
        self.styles.editor.editor_monospace_font_family =
            Box::leak(self.app_settings.code_font.clone().into_boxed_str());
        self.line_display_info.clear();
        self.refresh_generated_block_previews();
        self.sync_overlay_text_input_styles(cx);

        if recreate_settings_editors {
            self.settings_font_size_editor = None;
            self.settings_ui_font_size_editor = None;
            self.settings_line_height_editor = None;
            self.settings_letter_spacing_editor = None;
            self.ensure_font_size_editor(cx);
            self.ensure_ui_font_size_editor(cx);
        }
    }

    pub(crate) fn right_panel_width(&self) -> f32 {
        if self.tabs.is_empty() {
            return 0.0;
        }

        let utility_width = if self.should_render_utility_panel_on_right() {
            self.utility_panel_width()
        } else {
            0.0
        };
        utility_width + self.theme_debugger_width()
    }

    pub(crate) fn toggle_theme_debugger_visibility(&mut self, cx: &mut Context<Self>) {
        self.theme_debugger_visible = !self.theme_debugger_visible;
        if self.theme_debugger_visible {
            self.displayed_theme_debugger = true;
            self.ensure_theme_debugger_editors(cx);
            self.sync_theme_debugger_editors(cx);
        } else {
            self.theme_debugger_drag_target = None;
        }
        self.sync_theme_debugger_width_animation(cx);
        cx.notify();
    }

    fn theme_debugger_text_input_styles(&self) -> crate::style::TextInputStyles {
        let mut styles = self.styles.text_input.clone();
        styles.text_input_height_px = 28.0;
        styles.text_input_font_size_px = self.styles.ui_font_size_px;
        styles
    }

    pub(crate) fn update_theme_debugger_input_styles(&mut self, cx: &mut Context<Self>) {
        let input_styles = self.theme_debugger_text_input_styles();
        if let Some(editor) = &self.theme_debugger_hex_editor {
            let styles = input_styles.clone();
            editor.update(cx, |ti, cx| ti.set_styles(styles, cx));
        }
        if let Some(editor) = &self.theme_debugger_alpha_editor {
            editor.update(cx, |ti, cx| ti.set_styles(input_styles, cx));
        }
    }

    fn ensure_theme_debugger_editors(&mut self, cx: &mut Context<Self>) {
        if self.theme_debugger_hex_editor.is_none() {
            let styles = self.theme_debugger_text_input_styles();
            let editor = cx.new(|cx| crate::text_input::TextInput::new(styles, "#RRGGBBAA", cx));
            cx.subscribe(
                &editor,
                |this, editor, event: &crate::text_input::TextInputEvent, cx| match event {
                    crate::text_input::TextInputEvent::Changed => {
                        let text = editor.read(cx).text().to_string();
                        this.apply_theme_debugger_hex(&text, false, cx);
                    }
                    crate::text_input::TextInputEvent::Submit => {
                        let text = editor.read(cx).text().to_string();
                        this.apply_theme_debugger_hex(&text, true, cx);
                    }
                    crate::text_input::TextInputEvent::Escape => {
                        this.sync_theme_debugger_editors(cx);
                    }
                    _ => {}
                },
            )
            .detach();
            self.theme_debugger_hex_editor = Some(editor);
        }

        if self.theme_debugger_alpha_editor.is_none() {
            let styles = self.theme_debugger_text_input_styles();
            let editor = cx.new(|cx| crate::text_input::TextInput::new(styles, "0-100", cx));
            cx.subscribe(
                &editor,
                |this, editor, event: &crate::text_input::TextInputEvent, cx| match event {
                    crate::text_input::TextInputEvent::Changed => {
                        let text = editor.read(cx).text().to_string();
                        this.apply_theme_debugger_alpha(&text, false, cx);
                    }
                    crate::text_input::TextInputEvent::Submit => {
                        let text = editor.read(cx).text().to_string();
                        this.apply_theme_debugger_alpha(&text, true, cx);
                    }
                    crate::text_input::TextInputEvent::Escape => {
                        this.sync_theme_debugger_editors(cx);
                    }
                    _ => {}
                },
            )
            .detach();
            self.theme_debugger_alpha_editor = Some(editor);
        }
    }

    fn sync_theme_debugger_editors(&mut self, cx: &mut Context<Self>) {
        self.ensure_theme_debugger_editors(cx);
        self.update_theme_debugger_input_styles(cx);

        let color = self.theme.color(self.theme_debugger_selected_field);
        let hex = theme::hsla_to_hex_rgba(color);
        let alpha = format!("{:.0}", color.a.clamp(0.0, 1.0) * 100.0);

        if let Some(editor) = &self.theme_debugger_hex_editor {
            editor.update(cx, |ti, cx| {
                if ti.text() != hex {
                    ti.set_text(&hex, cx);
                }
            });
        }
        if let Some(editor) = &self.theme_debugger_alpha_editor {
            editor.update(cx, |ti, cx| {
                if ti.text() != alpha {
                    ti.set_text(&alpha, cx);
                }
            });
        }
    }

    fn select_theme_debugger_field(&mut self, field: ThemeColorField, cx: &mut Context<Self>) {
        if self.theme_debugger_selected_field == field {
            return;
        }
        self.theme_debugger_selected_field = field;
        self.theme_debugger_drag_target = None;
        self.sync_theme_debugger_editors(cx);
        cx.notify();
    }

    fn select_theme_debugger_section(
        &mut self,
        section: ThemeDebuggerSection,
        cx: &mut Context<Self>,
    ) {
        if self.theme_debugger_section == section {
            return;
        }
        self.theme_debugger_section = section;
        if !section
            .fields()
            .iter()
            .any(|field| *field == self.theme_debugger_selected_field)
        {
            self.theme_debugger_selected_field = section.default_field();
        }
        self.sync_theme_debugger_editors(cx);
        cx.notify();
    }

    fn start_theme_debugger_drag(
        &mut self,
        target: ThemeDebuggerDragTarget,
        position: gpui::Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.theme_debugger_drag_target = Some(target);
        self.update_theme_debugger_drag(target, position, cx);
        window.prevent_default();
        cx.stop_propagation();
    }

    fn stop_theme_debugger_drag(&mut self, cx: &mut Context<Self>) {
        if self.theme_debugger_drag_target.take().is_some() {
            cx.notify();
        }
    }

    fn on_theme_debugger_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(target) = self.theme_debugger_drag_target {
            self.update_theme_debugger_drag(target, event.position, cx);
        }
    }

    fn update_theme_debugger_drag(
        &mut self,
        target: ThemeDebuggerDragTarget,
        position: gpui::Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        let mut hsva = hsla_to_hsva(self.theme.color(self.theme_debugger_selected_field));

        match target {
            ThemeDebuggerDragTarget::SaturationValue => {
                let Some(bounds) = self.theme_debugger_sv_bounds.get() else {
                    return;
                };
                hsva.s = horizontal_fraction(bounds, position);
                hsva.v = 1.0 - vertical_fraction(bounds, position);
            }
            ThemeDebuggerDragTarget::Hue => {
                let Some(bounds) = self.theme_debugger_hue_bounds.get() else {
                    return;
                };
                hsva.h = horizontal_fraction(bounds, position);
            }
            ThemeDebuggerDragTarget::Alpha => {
                let Some(bounds) = self.theme_debugger_alpha_bounds.get() else {
                    return;
                };
                hsva.a = horizontal_fraction(bounds, position);
            }
        }

        self.apply_theme_debugger_picker_color(hsva_to_hsla(hsva), cx);
    }

    fn apply_theme_debugger_picker_color(&mut self, color: Hsla, cx: &mut Context<Self>) {
        let field = self.theme_debugger_selected_field;
        self.theme.set_color(field, color);
        self.refresh_theme_styles(false, cx);
        self.sync_theme_debugger_editors(cx);
        cx.notify();
    }

    fn apply_theme_debugger_hex(
        &mut self,
        text: &str,
        normalize_after_apply: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(parsed) = theme::parse_hex_color(text) {
            let field = self.theme_debugger_selected_field;
            let color = if theme::hex_color_has_alpha(text) {
                parsed
            } else {
                Hsla {
                    a: self.theme.color(field).a,
                    ..parsed
                }
            };
            self.theme.set_color(field, color);
            self.refresh_theme_styles(false, cx);
            if normalize_after_apply {
                self.sync_theme_debugger_editors(cx);
            } else if theme::hex_color_has_alpha(text) {
                let alpha = format!("{:.0}", color.a.clamp(0.0, 1.0) * 100.0);
                if let Some(editor) = &self.theme_debugger_alpha_editor {
                    editor.update(cx, |ti, cx| {
                        if ti.text() != alpha {
                            ti.set_text(&alpha, cx);
                        }
                    });
                }
            }
            cx.notify();
        } else if normalize_after_apply {
            self.sync_theme_debugger_editors(cx);
        }
    }

    fn apply_theme_debugger_alpha(
        &mut self,
        text: &str,
        normalize_after_apply: bool,
        cx: &mut Context<Self>,
    ) {
        if let Some(alpha) = parse_theme_debugger_alpha(text) {
            let field = self.theme_debugger_selected_field;
            let color = crate::theme::with_alpha(self.theme.color(field), alpha);
            self.theme.set_color(field, color);
            self.refresh_theme_styles(false, cx);
            if normalize_after_apply {
                self.sync_theme_debugger_editors(cx);
            }
            cx.notify();
        } else if normalize_after_apply {
            self.sync_theme_debugger_editors(cx);
        }
    }

    fn reset_theme_debugger_theme(&mut self, cx: &mut Context<Self>) {
        self.theme_debugger_drag_target = None;
        self.theme = crate::theme::apply_editor_theme_mode(
            &self.theme_kind.build(),
            self.editor_theme_mode,
            self.theme_kind.is_dark(),
        );
        self.refresh_theme_styles(false, cx);
        self.sync_theme_debugger_editors(cx);
        cx.notify();
    }

    fn export_theme_debugger_theme(&self, cx: &mut Context<Self>) {
        let text = format_theme_debugger_export(&self.theme);
        cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
    }

    pub(crate) fn render_theme_debugger(&mut self, cx: &mut Context<Self>) -> AnyElement {
        self.ensure_theme_debugger_editors(cx);

        let width = self.theme_debugger_layout_width();
        let header_height = self.scaled(THEME_DEBUGGER_HEADER_HEIGHT_PX);
        let row_height = self.scaled(THEME_DEBUGGER_ROW_HEIGHT_PX);
        let header_padding_x = self.scaled(14.0);
        let section_gap = self.scaled(12.0);
        let card_padding = self.scaled(12.0);
        let input_gap = self.scaled(8.0);
        let border_color = self.theme.border_secondary;
        let divider_color = self.theme.border_primary;
        let panel_bg = self.theme.bg_secondary;
        let card_bg = self.theme.bg_elevated;
        let hover_bg = self.theme.bg_hover;
        let active_bg = self.theme.bg_selected;
        let text_primary = self.theme.text_primary;
        let text_secondary = self.theme.text_secondary;
        let detail_label_font = self.scaled(12.0);
        let row_font = self.scaled(12.0);
        let debugger_section = self.theme_debugger_section;
        let selected_field = self.theme_debugger_selected_field;
        let selected_color = self.theme.color(selected_field);
        let selected_hsva = hsla_to_hsva(selected_color);
        let card_inner_width =
            (width - header_padding_x * 2.0 - card_padding * 2.0).max(self.scaled(120.0));
        let picker_size = card_inner_width;
        let slider_width = card_inner_width;
        let slider_height = self.scaled(THEME_DEBUGGER_SLIDER_HEIGHT_PX);
        let slider_handle_width = self.scaled(THEME_DEBUGGER_SLIDER_HANDLE_WIDTH_PX);
        let sv_handle_size = self.scaled(THEME_DEBUGGER_SV_HANDLE_SIZE_PX);
        let pure_hue = theme_debugger_hue_color(selected_hsva.h);
        let opaque_selected = Hsla {
            a: 1.0,
            ..selected_color
        };
        let transparent_white = Hsla {
            a: 0.0,
            ..gpui::white()
        };
        let transparent_black = Hsla {
            a: 0.0,
            ..gpui::black()
        };
        let transparent_selected = Hsla {
            a: 0.0,
            ..opaque_selected
        };
        let sv_handle_left = (selected_hsva.s * picker_size - sv_handle_size * 0.5)
            .clamp(0.0, (picker_size - sv_handle_size).max(0.0));
        let sv_handle_top = ((1.0 - selected_hsva.v) * picker_size - sv_handle_size * 0.5)
            .clamp(0.0, (picker_size - sv_handle_size).max(0.0));
        let hue_handle_left = (selected_hsva.h.clamp(0.0, 1.0) * slider_width
            - slider_handle_width * 0.5)
            .clamp(0.0, (slider_width - slider_handle_width).max(0.0));
        let alpha_handle_left = (selected_hsva.a.clamp(0.0, 1.0) * slider_width
            - slider_handle_width * 0.5)
            .clamp(0.0, (slider_width - slider_handle_width).max(0.0));

        let hue_segments: Vec<AnyElement> = (0..THEME_DEBUGGER_HUE_SEGMENTS)
            .map(|index| {
                let from =
                    theme_debugger_hue_color(index as f32 / THEME_DEBUGGER_HUE_SEGMENTS as f32);
                let to = theme_debugger_hue_color(
                    (index + 1) as f32 / THEME_DEBUGGER_HUE_SEGMENTS as f32,
                );

                div()
                    .flex_1()
                    .h_full()
                    .bg(linear_gradient(
                        90.0,
                        linear_color_stop(from, 0.0),
                        linear_color_stop(to, 1.0),
                    ))
                    .into_any_element()
            })
            .collect();

        let rows: Vec<AnyElement> = debugger_section
            .fields()
            .iter()
            .copied()
            .map(|field| {
                let is_selected = field == selected_field;
                let row_color = self.theme.color(field);
                let row_hex = theme::hsla_to_hex_rgba(row_color);
                let bg = if is_selected {
                    active_bg
                } else {
                    gpui::transparent_black()
                };

                div()
                    .id(SharedString::from(format!("theme-color-{}", field.label())))
                    .h(px(row_height))
                    .w_full()
                    .px(px(header_padding_x))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(self.scaled(10.0)))
                    .cursor(CursorStyle::Arrow)
                    .bg(bg)
                    .hover(move |style| style.bg(hover_bg))
                    .child(
                        div()
                            .w(px(self.scaled(14.0)))
                            .h(px(self.scaled(14.0)))
                            .rounded(px(self.scaled(4.0)))
                            .bg(row_color)
                            .border(px(1.0))
                            .border_color(divider_color),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_size(px(row_font))
                            .text_color(text_primary)
                            .child(field.label()),
                    )
                    .child(
                        div()
                            .text_size(px(self.scaled(11.0)))
                            .text_color(text_secondary)
                            .child(row_hex),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.select_theme_debugger_field(field, cx);
                        }),
                    )
                    .into_any_element()
            })
            .collect();

        div()
            .id("theme-debugger-panel")
            .h_full()
            .w(px(width))
            .flex_shrink_0()
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .track_scroll(&self.theme_debugger_scroll_handle)
            .bg(panel_bg)
            .border_l(px(1.0))
            .border_color(border_color)
            .on_mouse_move(cx.listener(Self::on_theme_debugger_mouse_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    this.stop_theme_debugger_drag(cx);
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| {
                    this.stop_theme_debugger_drag(cx);
                }),
            )
            .child(
                div()
                    .h(px(header_height))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(header_padding_x))
                    .text_size(px(self.scaled(11.0)))
                    .text_color(text_secondary)
                    .child(self.t(crate::i18n::TextKey::ThemeDebugger))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(self.scaled(8.0)))
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .rounded(px(self.scaled(6.0)))
                                    .border(px(1.0))
                                    .border_color(border_color)
                                    .overflow_hidden()
                                    .children([
                                        ThemeDebuggerSection::Interface,
                                        ThemeDebuggerSection::Editor,
                                    ]
                                    .into_iter()
                                    .enumerate()
                                    .map(|(index, section)| {
                                        let is_active = section == debugger_section;
                                        div()
                                            .id(SharedString::from(format!(
                                                "theme-debugger-section-{}",
                                                self.t(section.label_key())
                                            )))
                                            .px(px(self.scaled(10.0)))
                                            .py(px(self.scaled(5.0)))
                                            .text_color(if is_active {
                                                panel_bg
                                            } else {
                                                text_primary
                                            })
                                            .bg(if is_active {
                                                text_primary
                                            } else {
                                                gpui::transparent_black()
                                            })
                                            .cursor(CursorStyle::Arrow)
                                            .when(index > 0, |this| {
                                                this.border_l(px(1.0)).border_color(border_color)
                                            })
                                            .when(!is_active, |this| {
                                                this.hover(move |style| style.bg(hover_bg))
                                            })
                                            .child(self.t(section.label_key()))
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(move |this, _, _, cx| {
                                                    this.select_theme_debugger_section(section, cx);
                                                }),
                                            )
                                            .into_any_element()
                                    })),
                            )
                            .gap(px(self.scaled(12.0)))
                            .child(
                                div()
                                    .cursor(CursorStyle::Arrow)
                                    .text_color(text_primary)
                                    .hover(move |style| style.opacity(0.75))
                                    .child(self.t(crate::i18n::TextKey::Export))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            this.export_theme_debugger_theme(cx);
                                        }),
                                    ),
                            )
                            .child(
                                div()
                                    .cursor(CursorStyle::Arrow)
                                    .text_color(text_primary)
                                    .hover(move |style| style.opacity(0.75))
                                    .child(self.t(crate::i18n::TextKey::Reset))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _, _, cx| {
                                            this.reset_theme_debugger_theme(cx);
                                        }),
                                    ),
                            ),
                    ),
            )
            .child(
                div()
                    .mx(px(header_padding_x))
                    .mb(px(section_gap))
                    .p(px(card_padding))
                    .flex()
                    .flex_col()
                    .gap(px(section_gap))
                    .bg(card_bg)
                    .border(px(1.0))
                    .border_color(divider_color)
                    .rounded(px(self.scaled(10.0)))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(self.scaled(8.0)))
                            .child(
                                div()
                                    .text_size(px(detail_label_font))
                                    .text_color(text_secondary)
                                    .child(self.t(crate::i18n::TextKey::Picker)),
                            )
                            .child({
                                let bounds_cell = self.theme_debugger_sv_bounds.clone();
                                div()
                                    .w(px(picker_size))
                                    .h(px(picker_size))
                                    .on_children_prepainted({
                                        let bounds_cell = bounds_cell.clone();
                                        move |bounds: Vec<Bounds<Pixels>>, _, _| {
                                            if let Some(bounds) = bounds.first() {
                                                bounds_cell.set(Some(*bounds));
                                            }
                                        }
                                    })
                                    .child(
                                        div()
                                            .id("theme-debugger-sv-picker")
                                            .relative()
                                            .w_full()
                                            .h_full()
                                            .overflow_hidden()
                                            .rounded(px(self.scaled(10.0)))
                                            .border(px(1.0))
                                            .border_color(divider_color)
                                            .cursor(CursorStyle::Crosshair)
                                            .on_mouse_move(
                                                cx.listener(Self::on_theme_debugger_mouse_move),
                                            )
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(
                                                    |this,
                                                     event: &MouseDownEvent,
                                                     window,
                                                     cx| {
                                                        this.start_theme_debugger_drag(
                                                            ThemeDebuggerDragTarget::SaturationValue,
                                                            event.position,
                                                            window,
                                                            cx,
                                                        );
                                                    },
                                                ),
                                            )
                                            .on_mouse_up(
                                                MouseButton::Left,
                                                cx.listener(
                                                    |this, _: &MouseUpEvent, _, cx| {
                                                        this.stop_theme_debugger_drag(cx);
                                                    },
                                                ),
                                            )
                                            .on_mouse_up_out(
                                                MouseButton::Left,
                                                cx.listener(
                                                    |this, _: &MouseUpEvent, _, cx| {
                                                        this.stop_theme_debugger_drag(cx);
                                                    },
                                                ),
                                            )
                                            .child(
                                                div()
                                                    .absolute()
                                                    .left(px(0.0))
                                                    .top(px(0.0))
                                                    .w_full()
                                                    .h_full()
                                                    .bg(pure_hue),
                                            )
                                            .child(
                                                div()
                                                    .absolute()
                                                    .left(px(0.0))
                                                    .top(px(0.0))
                                                    .w_full()
                                                    .h_full()
                                                    .bg(linear_gradient(
                                                        90.0,
                                                        linear_color_stop(gpui::white(), 0.0),
                                                        linear_color_stop(transparent_white, 1.0),
                                                    )),
                                            )
                                            .child(
                                                div()
                                                    .absolute()
                                                    .left(px(0.0))
                                                    .top(px(0.0))
                                                    .w_full()
                                                    .h_full()
                                                    .bg(linear_gradient(
                                                        180.0,
                                                        linear_color_stop(transparent_black, 0.0),
                                                        linear_color_stop(gpui::black(), 1.0),
                                                    )),
                                            )
                                            .child(
                                                div()
                                                    .absolute()
                                                    .left(px(sv_handle_left))
                                                    .top(px(sv_handle_top))
                                                    .w(px(sv_handle_size))
                                                    .h(px(sv_handle_size))
                                                    .rounded_full()
                                                    .bg(selected_color)
                                                    .border(px(2.0))
                                                    .border_color(gpui::white())
                                                    .shadow_sm(),
                                            ),
                                    )
                            })
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(self.scaled(6.0)))
                                    .child(
                                        div()
                                            .text_size(px(detail_label_font))
                                            .text_color(text_secondary)
                                            .child(self.t(crate::i18n::TextKey::Hue)),
                                    )
                                    .child({
                                        let bounds_cell = self.theme_debugger_hue_bounds.clone();
                                        div()
                                            .w(px(slider_width))
                                            .h(px(slider_height))
                                            .on_children_prepainted({
                                                let bounds_cell = bounds_cell.clone();
                                                move |bounds: Vec<Bounds<Pixels>>, _, _| {
                                                    if let Some(bounds) = bounds.first() {
                                                        bounds_cell.set(Some(*bounds));
                                                    }
                                                }
                                            })
                                            .child(
                                                div()
                                                    .id("theme-debugger-hue-slider")
                                                    .relative()
                                                    .w_full()
                                                    .h_full()
                                                    .overflow_hidden()
                                                    .rounded(px(slider_height * 0.5))
                                                    .border(px(1.0))
                                                    .border_color(divider_color)
                                                    .cursor(CursorStyle::ResizeLeftRight)
                                                    .on_mouse_move(
                                                        cx.listener(Self::on_theme_debugger_mouse_move),
                                                    )
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener(
                                                            |this,
                                                             event: &MouseDownEvent,
                                                             window,
                                                             cx| {
                                                                this.start_theme_debugger_drag(
                                                                    ThemeDebuggerDragTarget::Hue,
                                                                    event.position,
                                                                    window,
                                                                    cx,
                                                                );
                                                            },
                                                        ),
                                                    )
                                                    .on_mouse_up(
                                                        MouseButton::Left,
                                                        cx.listener(
                                                            |this, _: &MouseUpEvent, _, cx| {
                                                                this.stop_theme_debugger_drag(cx);
                                                            },
                                                        ),
                                                    )
                                                    .on_mouse_up_out(
                                                        MouseButton::Left,
                                                        cx.listener(
                                                            |this, _: &MouseUpEvent, _, cx| {
                                                                this.stop_theme_debugger_drag(cx);
                                                            },
                                                        ),
                                                    )
                                                    .child(
                                                        div()
                                                            .absolute()
                                                            .left(px(0.0))
                                                            .top(px(0.0))
                                                            .w_full()
                                                            .h_full()
                                                            .flex()
                                                            .flex_row()
                                                            .gap(px(0.0))
                                                            .children(hue_segments),
                                                    )
                                                    .child(
                                                        div()
                                                            .absolute()
                                                            .left(px(hue_handle_left))
                                                            .top(px(0.0))
                                                            .w(px(slider_handle_width))
                                                            .h_full()
                                                            .rounded(px(slider_height * 0.5))
                                                            .bg(gpui::white())
                                                            .border(px(1.0))
                                                            .border_color(divider_color)
                                                            .shadow_sm(),
                                                    ),
                                            )
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(self.scaled(6.0)))
                                    .child(
                                        div()
                                            .text_size(px(detail_label_font))
                                            .text_color(text_secondary)
                                            .child(self.t(crate::i18n::TextKey::Opacity)),
                                    )
                                    .child({
                                        let bounds_cell = self.theme_debugger_alpha_bounds.clone();
                                        div()
                                            .w(px(slider_width))
                                            .h(px(slider_height))
                                            .on_children_prepainted({
                                                let bounds_cell = bounds_cell.clone();
                                                move |bounds: Vec<Bounds<Pixels>>, _, _| {
                                                    if let Some(bounds) = bounds.first() {
                                                        bounds_cell.set(Some(*bounds));
                                                    }
                                                }
                                            })
                                            .child(
                                                div()
                                                    .id("theme-debugger-alpha-slider")
                                                    .relative()
                                                    .w_full()
                                                    .h_full()
                                                    .overflow_hidden()
                                                    .rounded(px(slider_height * 0.5))
                                                    .border(px(1.0))
                                                    .border_color(divider_color)
                                                    .bg(panel_bg)
                                                    .cursor(CursorStyle::ResizeLeftRight)
                                                    .on_mouse_move(
                                                        cx.listener(Self::on_theme_debugger_mouse_move),
                                                    )
                                                    .on_mouse_down(
                                                        MouseButton::Left,
                                                        cx.listener(
                                                            |this,
                                                             event: &MouseDownEvent,
                                                             window,
                                                             cx| {
                                                                this.start_theme_debugger_drag(
                                                                    ThemeDebuggerDragTarget::Alpha,
                                                                    event.position,
                                                                    window,
                                                                    cx,
                                                                );
                                                            },
                                                        ),
                                                    )
                                                    .on_mouse_up(
                                                        MouseButton::Left,
                                                        cx.listener(
                                                            |this, _: &MouseUpEvent, _, cx| {
                                                                this.stop_theme_debugger_drag(cx);
                                                            },
                                                        ),
                                                    )
                                                    .on_mouse_up_out(
                                                        MouseButton::Left,
                                                        cx.listener(
                                                            |this, _: &MouseUpEvent, _, cx| {
                                                                this.stop_theme_debugger_drag(cx);
                                                            },
                                                        ),
                                                    )
                                                    .child(
                                                        div()
                                                            .absolute()
                                                            .left(px(0.0))
                                                            .top(px(0.0))
                                                            .w_full()
                                                            .h_full()
                                                            .bg(linear_gradient(
                                                                90.0,
                                                                linear_color_stop(
                                                                    transparent_selected,
                                                                    0.0,
                                                                ),
                                                                linear_color_stop(
                                                                    opaque_selected,
                                                                    1.0,
                                                                ),
                                                            )),
                                                    )
                                                    .child(
                                                        div()
                                                            .absolute()
                                                            .left(px(alpha_handle_left))
                                                            .top(px(0.0))
                                                            .w(px(slider_handle_width))
                                                            .h_full()
                                                            .rounded(px(slider_height * 0.5))
                                                            .bg(gpui::white())
                                                            .border(px(1.0))
                                                            .border_color(divider_color)
                                                            .shadow_sm(),
                                                    ),
                                            )
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(self.scaled(6.0)))
                            .child(
                                div()
                                    .text_size(px(detail_label_font))
                                    .text_color(text_secondary)
                                    .child(self.t(crate::i18n::TextKey::Hex))
                            )
                            .when_some(self.theme_debugger_hex_editor.clone(), |this, editor| {
                                this.child(editor)
                            }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(self.scaled(6.0)))
                            .child(
                                div()
                                    .text_size(px(detail_label_font))
                                    .text_color(text_secondary)
                                    .child(self.t(crate::i18n::TextKey::AlphaPercent))
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .gap(px(input_gap))
                                    .items_center()
                                    .when_some(self.theme_debugger_alpha_editor.clone(), |this, editor| {
                                        this.child(div().w(px(self.scaled(92.0))).child(editor))
                                    })
                                    .child(
                                        div()
                                            .text_size(px(self.scaled(11.0)))
                                            .text_color(text_secondary)
                                            .child(self.t(crate::i18n::TextKey::AlphaHint)),
                                    ),
                            ),
                    ),
            )
            .children(rows)
            .into_any_element()
    }
}

fn parse_theme_debugger_alpha(text: &str) -> Option<f32> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }

    let is_percent = trimmed.ends_with('%');
    let numeric = trimmed.trim_end_matches('%').parse::<f32>().ok()?;
    let alpha = if is_percent || numeric > 1.0 {
        numeric / 100.0
    } else {
        numeric
    };

    Some(alpha.clamp(0.0, 1.0))
}

fn format_theme_debugger_export(theme: &crate::theme::Theme) -> String {
    let mut lines = vec!["{".to_string()];

    append_theme_debugger_export_section(
        &mut lines,
        "Text",
        &[
            ThemeColorField::TextPrimary,
            ThemeColorField::TextSecondary,
            ThemeColorField::TextBright,
            ThemeColorField::TextAccent,
            ThemeColorField::TextDanger,
        ],
        theme,
    );
    append_theme_debugger_export_section(
        &mut lines,
        "Background",
        &[
            ThemeColorField::BgPrimary,
            ThemeColorField::BgSecondary,
            ThemeColorField::BgElevated,
            ThemeColorField::BgHover,
            ThemeColorField::BgSelected,
            ThemeColorField::BgBar,
            ThemeColorField::BgTab,
            ThemeColorField::BgTabHover,
            ThemeColorField::BgTabActive,
            ThemeColorField::BgCode,
            ThemeColorField::BgCodeInline,
        ],
        theme,
    );
    append_theme_debugger_export_section(
        &mut lines,
        "Border",
        &[
            ThemeColorField::BorderPrimary,
            ThemeColorField::BorderSecondary,
            ThemeColorField::BorderTertiary,
            ThemeColorField::BorderDecorative,
            ThemeColorField::BorderCodeBlock,
        ],
        theme,
    );
    append_theme_debugger_export_section(
        &mut lines,
        "Editor/Markdown",
        &[
            ThemeColorField::Cursor,
            ThemeColorField::Selection,
            ThemeColorField::LineHighlight,
            ThemeColorField::Heading,
            ThemeColorField::Bold,
            ThemeColorField::Italic,
            ThemeColorField::Strikethrough,
            ThemeColorField::InlineCode,
            ThemeColorField::Link,
            ThemeColorField::SyntaxMarker,
            ThemeColorField::ListMarker,
            ThemeColorField::CheckboxChecked,
            ThemeColorField::GutterHeading,
        ],
        theme,
    );

    lines.push("}".to_string());
    lines.join("\n")
}

fn append_theme_debugger_export_section(
    lines: &mut Vec<String>,
    title: &str,
    fields: &[ThemeColorField],
    theme: &crate::theme::Theme,
) {
    lines.push(format!("    // {title}"));
    for &field in fields {
        lines.push(format_theme_debugger_export_field(theme, field));
    }
}

fn format_theme_debugger_export_field(
    theme: &crate::theme::Theme,
    field: ThemeColorField,
) -> String {
    let color = theme.color(field);
    let hex = theme::hsla_to_hex_rgba(color);
    let rgb_hex = &hex[..7];

    if theme_debugger_export_uses_with_alpha(field) {
        format!(
            "    {}: with_alpha(crate::hex_hsla!(\"{}\"), {}),",
            field.label(),
            rgb_hex,
            format_theme_debugger_alpha_value(color.a),
        )
    } else {
        let literal = if (color.a - 1.0).abs() <= 0.000_1 {
            rgb_hex
        } else {
            hex.as_str()
        };
        format!("    {}: crate::hex_hsla!(\"{}\"),", field.label(), literal,)
    }
}

fn theme_debugger_export_uses_with_alpha(field: ThemeColorField) -> bool {
    matches!(
        field,
        ThemeColorField::BorderDecorative
            | ThemeColorField::BorderCodeBlock
            | ThemeColorField::Selection
            | ThemeColorField::LineHighlight
            | ThemeColorField::SyntaxMarker
            | ThemeColorField::GutterHeading
    )
}

fn format_theme_debugger_alpha_value(alpha: f32) -> String {
    let mut text = format!("{alpha:.3}");
    while text.contains('.') && text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.push('0');
    }
    text
}

fn horizontal_fraction(bounds: Bounds<Pixels>, position: gpui::Point<Pixels>) -> f32 {
    let width = f32::from(bounds.size.width).max(f32::EPSILON);
    ((f32::from(position.x) - f32::from(bounds.origin.x)) / width).clamp(0.0, 1.0)
}

fn vertical_fraction(bounds: Bounds<Pixels>, position: gpui::Point<Pixels>) -> f32 {
    let height = f32::from(bounds.size.height).max(f32::EPSILON);
    ((f32::from(position.y) - f32::from(bounds.origin.y)) / height).clamp(0.0, 1.0)
}

fn theme_debugger_hue_color(hue: f32) -> Hsla {
    hsva_to_hsla(ThemeDebuggerHsva {
        h: hue.clamp(0.0, 1.0),
        s: 1.0,
        v: 1.0,
        a: 1.0,
    })
}

fn hsla_to_hsva(color: Hsla) -> ThemeDebuggerHsva {
    let (r, g, b) = hsl_to_rgb(color.h, color.s, color.l);
    let max = r.max(g.max(b));
    let min = r.min(g.min(b));
    let delta = max - min;

    let hue = if delta <= f32::EPSILON {
        color.h
    } else if (max - r).abs() <= f32::EPSILON {
        (((g - b) / delta).rem_euclid(6.0)) / 6.0
    } else if (max - g).abs() <= f32::EPSILON {
        (((b - r) / delta) + 2.0) / 6.0
    } else {
        (((r - g) / delta) + 4.0) / 6.0
    };

    ThemeDebuggerHsva {
        h: hue.clamp(0.0, 1.0),
        s: if max <= f32::EPSILON {
            0.0
        } else {
            delta / max
        },
        v: max,
        a: color.a.clamp(0.0, 1.0),
    }
}

fn hsva_to_hsla(color: ThemeDebuggerHsva) -> Hsla {
    let (r, g, b) = hsv_to_rgb(color.h, color.s, color.v);
    let mut hsla = rgb_to_hsla(r, g, b, color.a.clamp(0.0, 1.0));
    if color.s <= f32::EPSILON || color.v <= f32::EPSILON {
        hsla.h = color.h.clamp(0.0, 1.0);
    }
    hsla
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let h = h.rem_euclid(1.0);
    let s = s.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);

    if s <= f32::EPSILON {
        return (v, v, v);
    }

    let sector = h * 6.0;
    let index = sector.floor();
    let fraction = sector - index;
    let p = v * (1.0 - s);
    let q = v * (1.0 - s * fraction);
    let t = v * (1.0 - s * (1.0 - fraction));

    match index as i32 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    if s <= f32::EPSILON {
        return (l, l, l);
    }

    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;

    (
        hue_to_rgb(p, q, h + 1.0 / 3.0),
        hue_to_rgb(p, q, h),
        hue_to_rgb(p, q, h - 1.0 / 3.0),
    )
}

fn rgb_to_hsla(r: f32, g: f32, b: f32, a: f32) -> Hsla {
    let max = r.max(g.max(b));
    let min = r.min(g.min(b));
    let l = (max + min) / 2.0;

    if (max - min).abs() <= f32::EPSILON {
        return Hsla {
            h: 0.0,
            s: 0.0,
            l,
            a,
        };
    }

    let delta = max - min;
    let s = delta / (1.0 - (2.0 * l - 1.0).abs());
    let h = if (max - r).abs() <= f32::EPSILON {
        (((g - b) / delta).rem_euclid(6.0)) / 6.0
    } else if (max - g).abs() <= f32::EPSILON {
        (((b - r) / delta) + 2.0) / 6.0
    } else {
        (((r - g) / delta) + 4.0) / 6.0
    };

    Hsla { h, s, l, a }
}

fn hue_to_rgb(p: f32, q: f32, mut t: f32) -> f32 {
    if t < 0.0 {
        t += 1.0;
    }
    if t > 1.0 {
        t -= 1.0;
    }
    if t < 1.0 / 6.0 {
        return p + (q - p) * 6.0 * t;
    }
    if t < 1.0 / 2.0 {
        return q;
    }
    if t < 2.0 / 3.0 {
        return p + (q - p) * (2.0 / 3.0 - t) * 6.0;
    }
    p
}
