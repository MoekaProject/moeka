use std::collections::HashSet;

use gpui::{
    AnyElement, Context, CursorStyle, MouseButton, Pixels, Point, Window, div, prelude::*, px,
};

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LanguagePickerMode {
    Language,
    Indentation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum LanguagePickerKind {
    Markdown,
    PlainText,
    Code(String),
    Indentation(IndentSettings),
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

fn all_indentation_picker_items() -> Vec<LanguagePickerItem> {
    [
        (
            "缩进: 空格 2",
            "将文档缩进转换为每级 2 个空格",
            IndentSettings::spaces(2),
        ),
        (
            "缩进: 空格 4",
            "将文档缩进转换为每级 4 个空格",
            IndentSettings::spaces(4),
        ),
        (
            "缩进: 制表符 2",
            "将文档缩进转换为制表符，宽度 2",
            IndentSettings::tabs(2),
        ),
        (
            "缩进: 制表符 4",
            "将文档缩进转换为制表符，宽度 4",
            IndentSettings::tabs(4),
        ),
    ]
    .into_iter()
    .map(|(name, detail, settings)| LanguagePickerItem {
        name: name.to_string(),
        detail: detail.to_string(),
        kind: LanguagePickerKind::Indentation(settings),
    })
    .collect()
}

fn picker_items_for_mode(mode: LanguagePickerMode) -> Vec<LanguagePickerItem> {
    match mode {
        LanguagePickerMode::Language => all_language_picker_items(),
        LanguagePickerMode::Indentation => all_indentation_picker_items(),
    }
}

fn picker_placeholder(mode: LanguagePickerMode) -> &'static str {
    match mode {
        LanguagePickerMode::Language => "搜索语言或扩展名",
        LanguagePickerMode::Indentation => "搜索缩进方式",
    }
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
        let theme = self.app_settings.dark_theme_kind().build();
        let mut styles = self.styles.text_input.clone();
        styles.text_input_height_px = self.scaled(24.0);
        styles.text_input_font_size_px = self.scaled(12.0);
        styles.text_input_bg = gpui::Hsla::transparent_black();
        styles.text_input_focus_bg = gpui::Hsla::transparent_black();
        styles.text_input_text_color = theme.text_primary;
        styles.text_input_placeholder_color = theme.text_secondary;
        styles.text_input_border_width_px = 0.0;
        styles.text_input_cursor_color = theme.cursor;
        styles.text_input_cursor_height_ratio = 0.72;
        styles
    }

    pub(crate) fn open_language_picker(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_status_picker(LanguagePickerMode::Language, position, window, cx);
    }

    pub(crate) fn open_indentation_picker(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_status_picker(LanguagePickerMode::Indentation, position, window, cx);
    }

    fn open_status_picker(
        &mut self,
        mode: LanguagePickerMode,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab_supports_text_buffer() {
            return;
        }

        self.language_picker_visible = true;
        self.language_picker_mode = mode;
        self.language_picker_anchor = Some(position);
        self.language_picker_query.clear();
        self.language_picker_selected_index = 0;
        self.update_language_picker_results(cx);

        if self.language_picker_editor.is_none() {
            let styles = self.language_picker_text_input_styles();
            let editor = cx.new(move |cx| {
                crate::text_input::TextInput::new(styles, picker_placeholder(mode), cx)
            });
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
                input.placeholder =
                    SharedString::from(picker_placeholder(self.language_picker_mode));
                input.set_text(&self.language_picker_query, cx);
                input.select_all(cx);
            });
            editor.focus_handle(cx).focus(window);
        }
        cx.notify();
    }

    pub(crate) fn close_language_picker(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.language_picker_visible = false;
        self.language_picker_anchor = None;
        window.focus(&self.focus_handle(cx));
        cx.notify();
    }

    fn update_language_picker_results(&mut self, cx: &mut Context<Self>) {
        self.language_picker_items = picker_items_for_mode(self.language_picker_mode)
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
        if let LanguagePickerKind::Indentation(settings) = item.kind {
            self.language_picker_visible = false;
            self.language_picker_anchor = None;
            self.set_indent_settings(settings, cx);
            return;
        }

        let Some(index) = self.active_tab_index() else {
            return;
        };
        let file_kind = match item.kind {
            LanguagePickerKind::Markdown => FileKind::Markdown,
            LanguagePickerKind::PlainText => FileKind::Text,
            LanguagePickerKind::Code(language) => FileKind::Code { language },
            LanguagePickerKind::Indentation(_) => unreachable!(),
        };
        let is_code_or_text = matches!(file_kind, FileKind::Code { .. } | FileKind::Text);

        if let Some(tab) = self.tabs.get_mut(index) {
            tab.file_kind = Some(file_kind);
            tab.editor_left_aligned = is_code_or_text;
        }
        self.editor_left_aligned = is_code_or_text;
        self.language_picker_visible = false;
        self.language_picker_anchor = None;
        self.layout_dirty = true;
        self.clear_all_tab_layout_caches();
        self.reparse();
        session_store::persist_session(self);
        cx.notify();
    }

    pub(crate) fn render_language_picker_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let theme = self.app_settings.dark_theme_kind().build();
        let bg = theme.bg_elevated;
        let border = theme.border_primary;
        let selected_bg = theme.bg_selected;
        let hover_bg = theme.bg_hover;
        let text_primary = theme.text_primary;
        let text_secondary = theme.text_secondary;
        let row_h = self.scaled(24.0);
        let name_font = self.scaled(12.0);
        let input_h = self.scaled(36.0);
        let max_visible = 12usize;
        let visible_rows = self
            .language_picker_items
            .len()
            .min(200)
            .min(max_visible)
            .max(1);
        let desired_h = input_h + visible_rows as f32 * row_h;
        let (target_w, min_w) = match self.language_picker_mode {
            LanguagePickerMode::Indentation => (120.0, 112.0),
            LanguagePickerMode::Language => (160.0, 140.0),
        };
        let panel_w = self
            .scaled(target_w)
            .min(self.last_window_width - 16.0)
            .max(self.scaled(min_w));
        let anchor = self.language_picker_anchor.unwrap_or_else(|| {
            gpui::point(
                px(self.last_window_width - 8.0),
                px(self.last_window_height - 8.0),
            )
        });
        let anchor_x = f32::from(anchor.x);
        let anchor_y = f32::from(anchor.y);
        let gap = self.scaled(4.0);
        let available_below = (self.last_window_height - anchor_y - gap - 8.0).max(0.0);
        let available_above = (anchor_y - gap - 8.0).max(0.0);
        let show_above = desired_h > available_below && available_above > available_below;
        let panel_h = if show_above {
            desired_h.min(available_above).max(self.scaled(100.0))
        } else {
            desired_h.min(available_below).max(self.scaled(100.0))
        };
        let panel_left =
            (anchor_x - panel_w).clamp(8.0, (self.last_window_width - panel_w - 8.0).max(8.0));
        let panel_top = if show_above {
            (anchor_y - panel_h - gap).max(8.0)
        } else {
            (anchor_y + gap).min((self.last_window_height - panel_h - 8.0).max(8.0))
        };
        let max_results_h = (panel_h - input_h).max(row_h);

        let mut rows = Vec::new();
        if self.language_picker_items.is_empty() {
            rows.push(
                div()
                    .w_full()
                    .h(px(self.scaled(56.0)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(self.scaled(13.0)))
                    .text_color(text_secondary)
                    .child("没有匹配项")
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
                        .px(px(self.scaled(10.0)))
                        .flex()
                        .items_center()
                        .cursor(CursorStyle::Arrow)
                        .when(is_selected, |this| this.bg(selected_bg))
                        .hover(move |style| style.bg(hover_bg))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .overflow_hidden()
                                .truncate()
                                .text_size(px(name_font))
                                .text_color(text_primary)
                                .child(item.name.clone()),
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
                    .rounded(px(self.scaled(8.0)))
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
                            .px(px(self.scaled(6.0)))
                            .py(px(self.scaled(6.0)))
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
