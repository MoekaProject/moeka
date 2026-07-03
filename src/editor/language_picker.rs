use std::collections::HashSet;

use gpui::{AnyElement, Context, CursorStyle, MouseButton, Window, div, prelude::*, px};

use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LanguagePickerKind {
    Markdown,
    PlainText,
    Code(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LanguagePickerItem {
    pub(crate) name: String,
    pub(crate) detail: String,
    pub(crate) kind: LanguagePickerKind,
}

fn all_language_picker_items() -> Vec<LanguagePickerItem> {
    let mut items = vec![
        LanguagePickerItem {
            name: "Markdown".to_string(),
            detail: ".md .markdown".to_string(),
            kind: LanguagePickerKind::Markdown,
        },
        LanguagePickerItem {
            name: "Plain Text".to_string(),
            detail: ".txt".to_string(),
            kind: LanguagePickerKind::PlainText,
        },
    ];

    let mut seen = HashSet::new();
    seen.insert("Markdown".to_string());
    seen.insert("Plain Text".to_string());

    for syntax in helpers::syntax_set().syntaxes() {
        if !seen.insert(syntax.name.clone()) {
            continue;
        }
        let detail = syntax
            .file_extensions
            .iter()
            .take(8)
            .map(|extension| format!(".{extension}"))
            .collect::<Vec<_>>()
            .join(" ");
        items.push(LanguagePickerItem {
            name: syntax.name.clone(),
            detail,
            kind: LanguagePickerKind::Code(syntax.name.clone()),
        });
    }

    items.sort_by(|left, right| left.name.to_lowercase().cmp(&right.name.to_lowercase()));
    items
}

fn language_item_matches(item: &LanguagePickerItem, query: &str) -> bool {
    let query = query.trim().to_ascii_lowercase();
    if query.is_empty() {
        return true;
    }
    item.name.to_ascii_lowercase().contains(&query)
        || item.detail.to_ascii_lowercase().contains(&query)
        || matches!(&item.kind, LanguagePickerKind::Code(language) if language.to_ascii_lowercase().contains(&query))
}

impl EditorView {
    fn language_picker_text_input_styles(&self) -> crate::style::TextInputStyles {
        let mut styles = self.styles.text_input.clone();
        styles.text_input_padding_left_px =
            self.styles.quick_open.quick_open_input_text_padding_left_px;
        styles.text_input_bg = self.styles.quick_open.quick_open_input_bg;
        styles.text_input_focus_bg = self.styles.quick_open.quick_open_input_focus_bg;
        styles.text_input_text_color = self.styles.quick_open.quick_open_input_text_color;
        styles.text_input_cursor_height_ratio =
            self.styles.quick_open.quick_open_input_cursor_height_ratio;
        styles.text_input_cursor_height_adjust_px = self
            .styles
            .quick_open
            .quick_open_input_cursor_height_adjust_px;
        styles
    }

    pub(crate) fn open_language_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.active_tab_supports_text_buffer() {
            return;
        }

        self.language_picker_visible = true;
        self.language_picker_selected_index = 0;
        self.update_language_picker_results(cx);

        if self.language_picker_editor.is_none() {
            let styles = self.language_picker_text_input_styles();
            let editor =
                cx.new(move |cx| crate::text_input::TextInput::new(styles, "搜索语言或扩展名", cx));
            cx.subscribe(
                &editor,
                |this, _editor, event: &crate::text_input::TextInputEvent, cx| match event {
                    crate::text_input::TextInputEvent::Changed => {
                        this.language_picker_query = this
                            .language_picker_editor
                            .as_ref()
                            .map(|editor| editor.read(cx).text().to_string())
                            .unwrap_or_default();
                        this.update_language_picker_results(cx);
                    }
                    crate::text_input::TextInputEvent::Submit
                    | crate::text_input::TextInputEvent::SubmitBackward => {
                        this.confirm_language_picker(cx);
                    }
                    crate::text_input::TextInputEvent::Escape => {}
                    crate::text_input::TextInputEvent::MoveUp => {
                        this.move_language_picker_selection(-1, cx);
                    }
                    crate::text_input::TextInputEvent::MoveDown => {
                        this.move_language_picker_selection(1, cx);
                    }
                },
            )
            .detach();
            self.language_picker_editor = Some(editor);
        }

        if let Some(editor) = &self.language_picker_editor {
            editor.update(cx, |input, cx| {
                input.styles = self.language_picker_text_input_styles();
                input.set_text(&self.language_picker_query, cx);
                input.select_all(cx);
            });
            editor.focus_handle(cx).focus(window);
        }
        cx.notify();
    }

    pub(crate) fn close_language_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.language_picker_visible = false;
        window.focus(&self.focus_handle(cx));
        cx.notify();
    }

    fn update_language_picker_results(&mut self, cx: &mut Context<Self>) {
        self.language_picker_items = all_language_picker_items()
            .into_iter()
            .filter(|item| language_item_matches(item, &self.language_picker_query))
            .take(240)
            .collect();
        self.language_picker_selected_index = self
            .language_picker_selected_index
            .min(self.language_picker_items.len().saturating_sub(1));
        cx.notify();
    }

    fn move_language_picker_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.language_picker_items.is_empty() {
            self.language_picker_selected_index = 0;
            cx.notify();
            return;
        }
        let len = self.language_picker_items.len() as isize;
        let current = self.language_picker_selected_index as isize;
        self.language_picker_selected_index = (current + delta).clamp(0, len - 1) as usize;
        cx.notify();
    }

    fn confirm_language_picker(&mut self, cx: &mut Context<Self>) {
        let Some(item) = self
            .language_picker_items
            .get(self.language_picker_selected_index)
            .cloned()
        else {
            self.language_picker_visible = false;
            cx.notify();
            return;
        };
        self.apply_language_picker_item(item, cx);
    }

    fn apply_language_picker_item(&mut self, item: LanguagePickerItem, cx: &mut Context<Self>) {
        let Some(index) = self.active_tab_index() else {
            return;
        };
        let file_kind = match item.kind {
            LanguagePickerKind::Markdown => FileKind::Markdown,
            LanguagePickerKind::PlainText => FileKind::Text,
            LanguagePickerKind::Code(language) => FileKind::Code { language },
        };
        let is_code_or_text = matches!(file_kind, FileKind::Code { .. } | FileKind::Text);

        if let Some(tab) = self.tabs.get_mut(index) {
            tab.file_kind = Some(file_kind);
            tab.editor_left_aligned = is_code_or_text;
        }
        self.editor_left_aligned = is_code_or_text;
        self.language_picker_visible = false;
        self.layout_dirty = true;
        self.reparse();
        session_store::persist_session(self);
        cx.notify();
    }

    pub(crate) fn render_language_picker_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let styles = &self.styles.quick_open;
        let bg = styles.quick_open_panel_bg;
        let border = styles.quick_open_panel_border_color;
        let backdrop = styles.quick_open_backdrop_bg;
        let selected_bg = styles.quick_open_result_selected_bg;
        let hover_bg = styles.quick_open_result_hover_bg;
        let text_primary = styles.quick_open_result_text_color;
        let text_secondary = styles.quick_open_result_secondary_text_color;
        let panel_w = self
            .scaled(styles.quick_open_panel_width_px)
            .min(self.last_window_width - 24.0);
        let panel_left = (self.last_window_width - panel_w) / 2.0;
        let panel_top =
            (self.last_window_height / 5.0).max(self.scaled(styles.quick_open_panel_top_offset_px));
        let row_h = self.scaled(styles.quick_open_result_row_height_px);
        let name_font = self.scaled(styles.quick_open_result_name_font_size_px);
        let detail_font = self.scaled(styles.quick_open_result_path_font_size_px);
        let max_results_h = self.scaled(styles.quick_open_results_max_height_px);

        let mut rows = Vec::new();
        if self.language_picker_items.is_empty() {
            rows.push(
                div()
                    .w_full()
                    .h(px(self.scaled(styles.quick_open_empty_height_px)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(self.scaled(styles.quick_open_empty_font_size_px)))
                    .text_color(styles.quick_open_empty_text_color)
                    .child("没有匹配的语言")
                    .into_any_element(),
            );
        } else {
            for (index, item) in self.language_picker_items.iter().enumerate() {
                let is_selected = index == self.language_picker_selected_index;
                rows.push(
                    div()
                        .id(SharedString::from(format!("language-picker-{index}")))
                        .w_full()
                        .h(px(row_h))
                        .px(px(self.scaled(styles.quick_open_result_padding_x_px)))
                        .flex()
                        .items_center()
                        .cursor(CursorStyle::Arrow)
                        .when(is_selected, |this| this.bg(selected_bg))
                        .hover(move |style| style.bg(hover_bg))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .flex()
                                .flex_col()
                                .child(
                                    div()
                                        .text_size(px(name_font))
                                        .text_color(text_primary)
                                        .child(item.name.clone()),
                                )
                                .child(
                                    div()
                                        .text_size(px(detail_font))
                                        .text_color(text_secondary)
                                        .child(item.detail.clone()),
                                ),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.language_picker_selected_index = index;
                                this.confirm_language_picker(cx);
                            }),
                        )
                        .into_any_element(),
                );
            }
        }

        div()
            .occlude()
            .absolute()
            .left(px(0.0))
            .top(px(0.0))
            .w_full()
            .h_full()
            .child(
                div()
                    .absolute()
                    .left(px(0.0))
                    .top(px(0.0))
                    .w_full()
                    .h_full()
                    .bg(backdrop)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            this.close_language_picker(window, cx);
                        }),
                    ),
            )
            .child(
                div()
                    .id("language-picker-overlay")
                    .absolute()
                    .top(px(panel_top))
                    .left(px(panel_left.max(0.0)))
                    .w(px(panel_w.max(240.0)))
                    .bg(bg)
                    .border(px(1.0))
                    .border_color(border)
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
                            .border_color(border)
                            .when_some(self.language_picker_editor.clone(), |this, editor| {
                                this.child(editor)
                            }),
                    )
                    .child(
                        div()
                            .id("language-picker-results")
                            .w_full()
                            .max_h(px(max_results_h))
                            .overflow_y_scroll()
                            .children(rows),
                    ),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn language_item_matches_extensions() {
        let item = LanguagePickerItem {
            name: "Rust".to_string(),
            detail: ".rs".to_string(),
            kind: LanguagePickerKind::Code("Rust".to_string()),
        };
        assert!(language_item_matches(&item, "rust"));
        assert!(language_item_matches(&item, "rs"));
        assert!(!language_item_matches(&item, "python"));
    }
}
