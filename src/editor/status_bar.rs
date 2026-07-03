use gpui::{AnyElement, Context, CursorStyle, MouseButton, MouseDownEvent, div, prelude::*, px};

use crate::markdown::BlockType;
use crate::theme::with_alpha;

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

    pub(crate) fn status_label(self) -> String {
        match self.kind {
            IndentKind::Spaces => format!("空格: {}", self.size),
            IndentKind::Tabs => format!("制表符: {}", self.size),
        }
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
        let (leading_bytes, columns) = leading_columns(content, old_settings.size);
        converted.push_str(&indent_for_columns(
            columns,
            old_settings.size,
            new_settings,
        ));
        converted.push_str(&content[leading_bytes..]);
        converted.push_str(carriage_return);
        converted.push_str(newline);
    }
    converted
}

impl EditorView {
    pub(crate) fn status_bar_height(&self) -> f32 {
        self.scaled(24.0)
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

        if self.active_tab_supports_text_buffer() {
            let old_settings = self.indent_settings;
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
                self.reparse();
                self.scroll_to_cursor();
            }
        }

        self.indent_settings = settings;
        self.pause_blinking(cx);
        self.close_context_menu(cx);
    }

    pub(crate) fn open_indentation_context_menu(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.open_context_menu(
            ContextMenu {
                position,
                kind: ContextMenuKind::Indentation,
            },
            false,
            cx,
        );
    }

    pub(crate) fn render_status_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let bg = self.theme.bg_elevated;
        let border = with_alpha(self.theme.border_secondary, 0.55);
        let text_color = self.theme.text_secondary;
        let hover_bg = with_alpha(self.theme.text_primary, 0.08);
        let cursor_label = self.cursor_status_label();
        let indent_label = self.indent_settings.status_label();
        let language_label = self.language_status_label();

        div()
            .id("editor-status-bar")
            .h(px(self.status_bar_height()))
            .w_full()
            .flex_none()
            .flex()
            .items_center()
            .justify_end()
            .pl(px(self.scaled(10.0)))
            .pr(px(0.0))
            .border_t(px(1.0))
            .border_color(border)
            .bg(bg)
            .text_size(px(self.scaled(12.0)))
            .text_color(text_color)
            .child(self.render_status_bar_button(cursor_label, hover_bg, None, cx))
            .child(self.render_status_bar_button(
                indent_label,
                hover_bg,
                Some(StatusBarAction::OpenIndentationMenu),
                cx,
            ))
            .child(self.render_status_bar_button(
                language_label,
                hover_bg,
                Some(StatusBarAction::OpenLanguagePicker),
                cx,
            ))
            .into_any_element()
    }

    fn render_status_bar_button(
        &self,
        label: String,
        hover_bg: gpui::Hsla,
        action: Option<StatusBarAction>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .px(px(self.scaled(6.0)))
            .h_full()
            .flex()
            .items_center()
            .cursor(CursorStyle::Arrow)
            .hover(move |style| style.bg(hover_bg))
            .child(label)
            .when_some(action, |this, action| {
                this.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        match action {
                            StatusBarAction::OpenIndentationMenu => {
                                this.open_indentation_context_menu(event.position, cx);
                            }
                            StatusBarAction::OpenLanguagePicker => {
                                this.open_language_picker(window, cx);
                            }
                        }
                    }),
                )
            })
            .into_any_element()
    }

    fn cursor_status_label(&self) -> String {
        let range = self.cursor.selection_range();
        let start = self.position_label(range.start);
        if range.is_empty() {
            start
        } else {
            format!("{}-{}", start, self.position_label(range.end))
        }
    }

    fn position_label(&self, offset: usize) -> String {
        let clamped = self.buffer.clamp_offset(offset.min(self.buffer.len()));
        let (line, byte_col) = self.buffer.offset_to_line_col(clamped);
        let line_content = self.buffer.line_content(line);
        let col = line_content[..byte_col.min(line_content.len())]
            .chars()
            .count()
            + 1;
        format!("L{}:C{}", line + 1, col)
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
                .unwrap_or_else(|| "Plain Text".to_string());
        }

        match self.active_file_kind() {
            Some(FileKind::Markdown) => "Markdown".to_string(),
            Some(FileKind::Code { language }) => language.clone(),
            Some(FileKind::Text) => "Plain Text".to_string(),
            Some(FileKind::Image) => "Image".to_string(),
            Some(FileKind::Binary) => "Binary".to_string(),
            Some(FileKind::Whiteboard) => "Whiteboard".to_string(),
            None => String::new(),
        }
    }
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
}
