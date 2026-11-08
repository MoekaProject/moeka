use gpui::{AnyElement, Context, MouseButton, MouseDownEvent, div, prelude::*, px};

use crate::markdown::{self, BlockType};

use super::chat_panel::TerminalStatusSummary;
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IndentKind {
    Spaces,
    Tabs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct IndentSettings {
    pub(crate) kind: IndentKind,
    pub(crate) size: usize,
}

impl Default for IndentSettings {
    fn default() -> Self {
        Self::spaces(2)
    }
}

impl IndentSettings {
    pub(crate) const fn spaces(size: usize) -> Self {
        Self {
            kind: IndentKind::Spaces,
            size,
        }
    }

    pub(crate) const fn tabs(size: usize) -> Self {
        Self {
            kind: IndentKind::Tabs,
            size,
        }
    }

    pub(crate) fn indent_text(self) -> String {
        match self.kind {
            IndentKind::Spaces => " ".repeat(self.size.max(1)),
            IndentKind::Tabs => "\t".to_string(),
        }
    }

    pub(crate) fn status_parts(self, language: crate::i18n::UiLanguage) -> (&'static str, String) {
        crate::i18n::status_indent_parts(language, matches!(self.kind, IndentKind::Tabs), self.size)
    }
}

fn leading_columns(line: &str, tab_size: usize) -> (usize, usize) {
    let mut bytes = 0usize;
    let mut columns = 0usize;
    let tab_size = tab_size.max(1);
    for ch in line.chars() {
        match ch {
            ' ' => {
                bytes += 1;
                columns += 1;
            }
            '\t' => {
                bytes += 1;
                columns += tab_size - (columns % tab_size);
            }
            _ => break,
        }
    }
    (bytes, columns)
}

fn indent_for_columns(columns: usize, old_size: usize, settings: IndentSettings) -> String {
    let old_size = old_size.max(1);
    let levels = columns / old_size;
    let remainder = columns % old_size;
    match settings.kind {
        IndentKind::Spaces => " ".repeat(levels * settings.size.max(1) + remainder),
        IndentKind::Tabs => format!("{}{}", "\t".repeat(levels), " ".repeat(remainder)),
    }
}

fn ordered_list_indent_for_level(level: usize, settings: IndentSettings) -> String {
    match settings.kind {
        IndentKind::Spaces => " ".repeat(level * 3),
        IndentKind::Tabs => format!("{}{}", "\t".repeat(level), " ".repeat(level)),
    }
}

fn convert_line_indentation(
    content: &str,
    old_settings: IndentSettings,
    new_settings: IndentSettings,
) -> String {
    let (leading_bytes, columns) = leading_columns(content, old_settings.size);
    let body = &content[leading_bytes..];
    let ordered_prefix_len = markdown::ordered_list_prefix_len(body);
    let new_indent = if ordered_prefix_len >= 2 {
        ordered_list_indent_for_level(columns / 3, new_settings)
    } else {
        indent_for_columns(columns, old_settings.size, new_settings)
    };

    format!("{}{}", new_indent, body)
}

fn convert_text_indentation(
    text: &str,
    old_settings: IndentSettings,
    new_settings: IndentSettings,
) -> String {
    let mut converted = String::with_capacity(text.len());
    for segment in text.split_inclusive('\n') {
        let (line, newline) = segment
            .strip_suffix('\n')
            .map(|line| (line, "\n"))
            .unwrap_or((segment, ""));
        let (content, carriage_return) = line
            .strip_suffix('\r')
            .map(|line| (line, "\r"))
            .unwrap_or((line, ""));
        converted.push_str(&convert_line_indentation(
            content,
            old_settings,
            new_settings,
        ));
        converted.push_str(carriage_return);
        converted.push_str(newline);
    }
    converted
}

impl EditorView {
    pub(crate) fn status_bar_height(&self) -> f32 {
        self.scaled(self.styles.status_bar().height_px)
    }

    pub(crate) fn editor_visible_height(&self) -> f32 {
        (self.last_viewport_height - self.status_bar_height()).max(1.0)
    }

    pub(crate) fn current_indent_text(&self) -> String {
        self.indent_settings.indent_text()
    }

    pub(crate) fn current_unindent_width(&self) -> usize {
        self.indent_settings.size.max(1)
    }

    pub(crate) fn set_indent_settings(&mut self, settings: IndentSettings, cx: &mut Context<Self>) {
        if self.indent_settings == settings {
            self.close_context_menu(cx);
            return;
        }

        let old_settings = self.indent_settings;
        let mut text_changed = false;
        if self.active_tab_supports_text_buffer() {
            let converted = convert_text_indentation(self.buffer.text(), old_settings, settings);
            if converted != self.buffer.text() {
                self.checkpoint();
                self.buffer.replace(0..self.buffer.len(), &converted);
                self.cursor.offset = self.cursor.offset.min(self.buffer.len());
                self.cursor.anchor = self.cursor.anchor.min(self.buffer.len());
                for cursor in &mut self.extra_cursors {
                    cursor.offset = cursor.offset.min(self.buffer.len());
                    cursor.anchor = cursor.anchor.min(self.buffer.len());
                }
                text_changed = true;
            }
        }

        self.indent_settings = settings;
        self.clear_active_tab_layout_cache();
        self.reparse();
        if text_changed {
            self.scroll_to_cursor();
        }
        self.pause_blinking(cx);
        self.close_context_menu(cx);
    }

    pub(crate) fn render_status_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let ss = self.styles.status_bar();
        let title_color = ss.title;
        let value_color = ss.value;
        if let Some(summary) = self.active_terminal_status_summary() {
            return self.render_terminal_status_bar(summary, title_color, value_color);
        }
        let cursor_label = self.cursor_status_label();
        let (indent_title, indent_value) = self.indent_settings.status_parts(self.ui_language);
        let language_value = self.language_status_label();
        crate::ui::chrome::status_bar(
            self.styles.status_bar(),
            self.ui_scale,
            self.styles.status_bar_padding_left_px,
        )
        .id("editor-status-bar")
        .justify_end()
        .child(div().flex_1())
        .child(self.render_status_bar_label(cursor_label, value_color))
        .child(self.render_status_bar_button(
            indent_title,
            indent_value,
            title_color,
            value_color,
            Some(StatusBarAction::OpenIndentationMenu),
            cx,
        ))
        .child(self.render_status_bar_button(
            crate::i18n::status_language_label(self.ui_language),
            language_value,
            title_color,
            value_color,
            Some(StatusBarAction::OpenLanguagePicker),
            cx,
        ))
        .into_any_element()
    }

    fn render_terminal_status_bar(
        &self,
        summary: TerminalStatusSummary,
        title_color: gpui::Hsla,
        value_color: gpui::Hsla,
    ) -> AnyElement {
        let agent_label = summary.agent.label();
        let show_title = !summary.title.is_empty() && summary.title != agent_label;
        let scrollback = summary.scrollback().map(|(offset, history_size)| {
            crate::i18n::terminal_scrollback_label(self.ui_language, offset, history_size)
        });

        crate::ui::chrome::status_bar(
            self.styles.status_bar(),
            self.ui_scale,
            self.styles.status_bar_padding_left_px,
        )
        .id("terminal-status-bar")
        .child(
            div()
                .h_full()
                .flex_1()
                .min_w(px(0.0))
                .px(px(self.scaled(self.styles.status_bar().padding_x_px)))
                .flex()
                .items_center()
                .gap(px(self.scaled(self.styles.status_bar().item_gap_px)))
                .child(
                    gpui::svg()
                        .path(summary.agent.icon_path())
                        .size(px(self.scaled(self.styles.status_bar().icon_size_px)))
                        .flex_none()
                        .text_color(title_color),
                )
                .child(
                    div()
                        .flex_none()
                        .whitespace_nowrap()
                        .text_color(title_color)
                        .child(agent_label),
                )
                .when(show_title, |this| {
                    this.child(div().flex_none().text_color(value_color).child("·"))
                        .child(
                            div()
                                .min_w(px(0.0))
                                .overflow_hidden()
                                .truncate()
                                .whitespace_nowrap()
                                .text_color(value_color)
                                .child(summary.title),
                        )
                }),
        )
        .when_some(scrollback, |this, scrollback| {
            this.child(self.render_status_bar_label(scrollback, value_color))
        })
        .into_any_element()
    }

    fn render_status_bar_label(&self, label: String, color: gpui::Hsla) -> AnyElement {
        div()
            .px(px(self.scaled(self.styles.status_bar().padding_x_px)))
            .h_full()
            .flex()
            .items_center()
            .text_color(color)
            .child(label)
            .into_any_element()
    }

    fn render_status_bar_button(
        &self,
        title: &'static str,
        value: String,
        title_color: gpui::Hsla,
        value_color: gpui::Hsla,
        action: Option<StatusBarAction>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let button = crate::ui::button::button_frame(
            self.styles.status_button(),
            Default::default(),
            self.ui_scale,
        )
        .gap(px(0.0))
        .child(div().text_color(title_color).child(format!("{}: ", title)))
        .child(div().text_color(value_color).child(value))
        .when_some(action, |this, action| {
            this.on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    match action {
                        StatusBarAction::OpenIndentationMenu => {
                            this.open_indentation_picker(event.position, window, cx);
                        }
                        StatusBarAction::OpenLanguagePicker => {
                            this.open_language_picker(event.position, window, cx);
                        }
                    }
                }),
            )
        })
        .into_any_element();
        let bounds_cell = match action {
            Some(StatusBarAction::OpenIndentationMenu) => {
                Some(self.status_bar_indent_button_bounds.clone())
            }
            Some(StatusBarAction::OpenLanguagePicker) => {
                Some(self.status_bar_language_button_bounds.clone())
            }
            None => None,
        };
        div()
            .h_full()
            .flex_none()
            .when_some(bounds_cell, |this, bounds_cell| {
                this.on_children_prepainted(move |bounds: Vec<gpui::Bounds<gpui::Pixels>>, _, _| {
                    bounds_cell.set(bounds.first().copied());
                })
            })
            .child(button)
            .into_any_element()
    }

    fn cursor_status_label(&self) -> String {
        let (line, col) = self.position_components(self.cursor.offset);
        let selected_chars = self.selected_character_count();
        crate::i18n::cursor_status_label(self.ui_language, line, col, selected_chars)
    }

    fn position_components(&self, offset: usize) -> (usize, usize) {
        let clamped = self.buffer.clamp_offset(offset.min(self.buffer.len()));
        let (line, byte_col) = self.buffer.offset_to_line_col(clamped);
        let line_content = self.buffer.line_content(line);
        let col = line_content[..byte_col.min(line_content.len())]
            .chars()
            .count()
            + 1;
        (line + 1, col)
    }

    fn selected_character_count(&self) -> usize {
        std::iter::once(self.cursor)
            .chain(self.extra_cursors.iter().copied())
            .map(|cursor| safe_selected_character_count(&self.buffer, cursor.selection_range()))
            .sum()
    }

    fn language_status_label(&self) -> String {
        if self.active_tab_is_markdown()
            && let Some(decoration) = self.decorations.get(self.cursor_line())
            && matches!(
                decoration.block_type,
                BlockType::CodeBlock | BlockType::CodeBlockFence
            )
        {
            return decoration
                .code_language
                .clone()
                .filter(|language| !language.trim().is_empty())
                .unwrap_or_else(|| self.message(crate::i18n::MessageKey::PlainText).to_string());
        }

        match self.active_file_kind() {
            Some(FileKind::Markdown) => "Markdown".to_string(),
            Some(FileKind::Code { language }) => language.clone(),
            Some(FileKind::Text) => self.message(crate::i18n::MessageKey::PlainText).to_string(),
            Some(FileKind::Image) => self.t(crate::i18n::TextKey::Image).to_string(),
            Some(FileKind::Binary) => self.message(crate::i18n::MessageKey::Binary).to_string(),
            None => String::new(),
        }
    }
}

fn safe_selected_character_count(
    buffer: &crate::buffer::Buffer,
    range: std::ops::Range<usize>,
) -> usize {
    if range.is_empty() || range.start >= buffer.len() {
        return 0;
    }
    let start = buffer.clamp_grapheme_offset(range.start);
    let end = buffer.clamp_grapheme_offset(range.end.min(buffer.len()));
    buffer
        .character_offset_for_offset(end)
        .saturating_sub(buffer.character_offset_for_offset(start.min(end)))
}

#[derive(Clone, Copy)]
enum StatusBarAction {
    OpenIndentationMenu,
    OpenLanguagePicker,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn converts_space_indentation_to_tabs() {
        assert_eq!(
            convert_text_indentation(
                "    a\n  b",
                IndentSettings::spaces(2),
                IndentSettings::tabs(2)
            ),
            "\t\ta\n\tb"
        );
    }

    #[test]
    fn converts_tabs_to_spaces() {
        assert_eq!(
            convert_text_indentation(
                "\t\ta\n\tb",
                IndentSettings::tabs(4),
                IndentSettings::spaces(2)
            ),
            "    a\n  b"
        );
    }

    #[test]
    fn ordered_list_indentation_stays_visually_aligned() {
        assert_eq!(
            convert_text_indentation(
                "   1. child\n      1. grandchild",
                IndentSettings::spaces(2),
                IndentSettings::tabs(2)
            ),
            "\t 1. child\n\t\t  1. grandchild"
        );
        assert_eq!(
            convert_text_indentation(
                "\t 1. child\n\t\t  1. grandchild",
                IndentSettings::tabs(2),
                IndentSettings::spaces(4)
            ),
            "   1. child\n      1. grandchild"
        );
    }

    #[test]
    fn selected_character_count_snaps_invalid_cjk_byte_offsets() {
        let buffer = crate::buffer::Buffer::from_text("ab。中文");
        assert_eq!(safe_selected_character_count(&buffer, 2..5), 1);
        assert_eq!(safe_selected_character_count(&buffer, 3..8), 2);
        assert_eq!(safe_selected_character_count(&buffer, 999..1000), 0);
    }
}
