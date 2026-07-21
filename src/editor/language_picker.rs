use std::collections::HashSet;

use gpui::{AnyElement, Bounds, Context, Pixels, Point, Window, point, prelude::*, px, size};

use super::status_bar::IndentKind;
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

fn all_indentation_picker_items(language: UiLanguage) -> Vec<LanguagePickerItem> {
    [
        IndentSettings::spaces(2),
        IndentSettings::spaces(4),
        IndentSettings::tabs(2),
        IndentSettings::tabs(4),
    ]
    .into_iter()
    .map(|settings| {
        let (name, detail) = crate::i18n::indentation_picker_item(
            language,
            matches!(settings.kind, IndentKind::Tabs),
            settings.size,
        );
        LanguagePickerItem {
            name,
            detail,
            kind: LanguagePickerKind::Indentation(settings),
        }
    })
    .collect()
}

fn picker_items_for_mode(
    mode: LanguagePickerMode,
    language: UiLanguage,
) -> Vec<LanguagePickerItem> {
    match mode {
        LanguagePickerMode::Language => all_language_picker_items(),
        LanguagePickerMode::Indentation => all_indentation_picker_items(language),
    }
}

fn picker_placeholder(mode: LanguagePickerMode, language: UiLanguage) -> &'static str {
    crate::i18n::language_picker_placeholder(language, mode == LanguagePickerMode::Indentation)
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
        self.language_picker_anchor = match mode {
            LanguagePickerMode::Indentation => self.status_bar_indent_button_bounds.get(),
            LanguagePickerMode::Language => self.status_bar_language_button_bounds.get(),
        }
        .or_else(|| {
            Some(Bounds::new(
                point(
                    position.x,
                    px(self.last_window_height - self.status_bar_height()),
                ),
                size(px(0.0), px(self.status_bar_height())),
            ))
        });
        self.language_picker_query.clear();
        self.language_picker_selected_index = 0;
        self.update_language_picker_results(cx);

        if self.language_picker_editor.is_none() {
            let styles = self.search_menu_text_input_styles();
            let ui_language = self.ui_language;
            let editor = cx.new(move |cx| {
                let mut input = crate::text_input::TextInput::new(
                    styles,
                    picker_placeholder(mode, ui_language),
                    cx,
                );
                // Hosted inside a deferred overlay; a deferred context menu
                // would nest defer_draw and panic.
                input.context_menu_enabled = false;
                input
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
                input.styles = self.search_menu_text_input_styles();
                input.placeholder = SharedString::from(picker_placeholder(
                    self.language_picker_mode,
                    self.ui_language,
                ));
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
        self.language_picker_items =
            picker_items_for_mode(self.language_picker_mode, self.ui_language)
                .into_iter()
                .filter(|item| language_item_matches(item, &self.language_picker_query))
                .take(240)
                .collect();
        self.language_picker_selected_index = if self.language_picker_query.trim().is_empty() {
            self.language_picker_items
                .iter()
                .position(|item| match (&self.language_picker_mode, &item.kind) {
                    (
                        LanguagePickerMode::Indentation,
                        LanguagePickerKind::Indentation(settings),
                    ) => *settings == self.indent_settings,
                    (LanguagePickerMode::Language, LanguagePickerKind::Markdown) => {
                        self.active_file_kind().is_some_and(FileKind::is_markdown)
                    }
                    (LanguagePickerMode::Language, LanguagePickerKind::PlainText) => {
                        matches!(self.active_file_kind(), Some(FileKind::Text))
                    }
                    (LanguagePickerMode::Language, LanguagePickerKind::Code(language)) => matches!(
                        self.active_file_kind(),
                        Some(FileKind::Code { language: active }) if active == language
                    ),
                    _ => false,
                })
                .unwrap_or(0)
        } else {
            0
        };
        self.language_picker_scroll_handle
            .scroll_to_item(self.language_picker_selected_index);
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
        self.language_picker_scroll_handle
            .scroll_to_item(self.language_picker_selected_index);
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
        self.active_file_kind_cache = Some(file_kind.clone());

        if let Some(tab) = self.tabs.get_mut(index) {
            tab.file_kind = Some(file_kind);
            tab.editor_left_aligned = is_code_or_text;
        }
        self.editor_left_aligned = is_code_or_text;
        self.language_picker_visible = false;
        self.language_picker_anchor = None;
        self.clear_active_tab_layout_cache();
        self.reparse();
        session_store::persist_session(self);
        cx.notify();
    }

    pub(crate) fn render_language_picker_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let anchor = self.language_picker_anchor.unwrap_or_else(|| {
            Bounds::new(
                point(
                    px(self.last_window_width - 8.0),
                    px(self.last_window_height),
                ),
                size(px(0.0), px(0.0)),
            )
        });
        let config = SearchMenuConfig {
            id: "language-picker-overlay",
            anchor,
            viewport_width: self.last_window_width,
            viewport_height: self.last_window_height,
            desired_width: self.scaled(240.0),
            match_anchor_width: false,
            row_count: self.language_picker_items.len(),
        };

        let rows = self
            .language_picker_items
            .iter()
            .enumerate()
            .map(|(index, item)| {
                let row = SearchMenuRow::new(item.name.clone())
                    .selected(index == self.language_picker_selected_index);
                self.render_search_menu_row(
                    SharedString::from(format!("language-picker-{index}")),
                    row,
                    move |this, _, cx| {
                        this.language_picker_selected_index = index;
                        this.confirm_language_picker(cx);
                    },
                    cx,
                )
            })
            .collect();

        self.render_search_menu_overlay(
            &config,
            self.language_picker_editor.clone(),
            Some(&self.language_picker_scroll_handle),
            None,
            rows,
            |this, window, cx| this.close_language_picker(window, cx),
            cx,
        )
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
