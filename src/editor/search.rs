use std::ops::Range;

use gpui::{AnyElement, Context, MouseButton, Window, div, prelude::*, px, svg};

use super::*;

pub(crate) const SEARCH_ICON_SVG_PATH: &str = "assets/icons/filetree/search.svg";
pub(crate) const SEARCH_CASE_ICON_SVG_PATH: &str = "assets/icons/search/case_sensitive.svg";
pub(crate) const SEARCH_WHOLE_WORD_ICON_SVG_PATH: &str = "assets/icons/search/whole_word.svg";
pub(crate) const SEARCH_REGEX_ICON_SVG_PATH: &str = "assets/icons/search/regex.svg";
pub(crate) const SEARCH_CLOSE_ICON_SVG_PATH: &str = "assets/icons/filetree/close.svg";

impl EditorView {
    pub(crate) fn on_find(&mut self, _: &Find, window: &mut Window, cx: &mut Context<Self>) {
        if !self.active_tab_supports_text_search() {
            return;
        }
        self.open_search(false, window, cx);
    }

    pub(crate) fn on_find_next(&mut self, _: &FindNext, _: &mut Window, cx: &mut Context<Self>) {
        self.search_navigate(true, cx);
    }

    pub(crate) fn on_find_previous(
        &mut self,
        _: &FindPrevious,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.search_navigate(false, cx);
    }

    pub(crate) fn on_dismiss_search(
        &mut self,
        _: &DismissSearch,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_search(window, cx);
    }

    pub(crate) fn on_toggle_replace(
        &mut self,
        _: &ToggleReplace,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab_supports_text_search() {
            return;
        }
        if !self.search_visible {
            self.open_search(true, window, cx);
        } else {
            self.search_replace_visible = !self.search_replace_visible;
            cx.notify();
        }
    }

    pub(crate) fn on_replace_next(
        &mut self,
        _: &ReplaceNext,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_current_match(cx);
    }

    pub(crate) fn on_replace_all(
        &mut self,
        _: &ReplaceAll,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_all_matches(cx);
    }

    pub(crate) fn open_search(
        &mut self,
        with_replace: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.active_tab_supports_text_search() {
            return;
        }
        let was_visible = self.search_visible;
        self.search_visible = true;
        self.search_replace_visible = with_replace;

        if self.search_query_editor.is_none() {
            let styles = self.search_query_text_input_styles();
            let initial_query = self.search_query.clone();
            let placeholder = self.t(crate::i18n::TextKey::SearchPlaceholder).to_string();
            let editor = cx.new(move |cx| {
                let mut input = crate::text_input::TextInput::new(styles, &placeholder, cx);
                if !initial_query.is_empty() {
                    input.set_text(&initial_query, cx);
                    input.select_all(cx);
                }
                input
            });
            cx.subscribe(
                &editor,
                |this, _editor, event: &crate::text_input::TextInputEvent, cx| match event {
                    crate::text_input::TextInputEvent::Changed => {
                        let query = this
                            .search_query_editor
                            .as_ref()
                            .map(|entity| entity.read(cx).text().to_string())
                            .unwrap_or_default();
                        this.search_query = query;
                        this.run_search(cx);
                    }
                    crate::text_input::TextInputEvent::Submit => {
                        this.search_navigate(true, cx);
                    }
                    crate::text_input::TextInputEvent::SubmitBackward => {
                        this.search_navigate(false, cx);
                    }
                    crate::text_input::TextInputEvent::Escape => {}
                    _ => {}
                },
            )
            .detach();
            self.search_query_editor = Some(editor);
        }

        if !was_visible {
            let selection = self.cursor.selection_range();
            if !selection.is_empty() && selection.len() < 200 {
                let selected_text = self.buffer.text()[selection.clone()].to_string();
                if !selected_text.contains('\n') {
                    self.search_query = selected_text;
                    self.run_search(cx);
                }
            }
        }

        if self.search_replace_editor.is_none() {
            let styles = self.search_replace_text_input_styles();
            let initial_replace = self.search_replace.clone();
            let placeholder = self.t(crate::i18n::TextKey::ReplacePlaceholder).to_string();
            let editor = cx.new(move |cx| {
                let mut input = crate::text_input::TextInput::new(styles, &placeholder, cx);
                if !initial_replace.is_empty() {
                    input.set_text(&initial_replace, cx);
                }
                input
            });
            cx.subscribe(
                &editor,
                |this, _editor, event: &crate::text_input::TextInputEvent, cx| match event {
                    crate::text_input::TextInputEvent::Changed => {
                        let replace = this
                            .search_replace_editor
                            .as_ref()
                            .map(|entity| entity.read(cx).text().to_string())
                            .unwrap_or_default();
                        this.search_replace = replace;
                    }
                    crate::text_input::TextInputEvent::Submit
                    | crate::text_input::TextInputEvent::SubmitBackward => {
                        this.replace_current_match(cx);
                    }
                    crate::text_input::TextInputEvent::Escape => {}
                    _ => {}
                },
            )
            .detach();
            self.search_replace_editor = Some(editor);
        }

        let query_styles = self.search_query_text_input_styles();
        let query = self.search_query.clone();
        if let Some(editor) = &self.search_query_editor {
            editor.update(cx, |input, cx| {
                input.styles = query_styles;
                input.set_text(&query, cx);
                input.select_all(cx);
            });
            editor.focus_handle(cx).focus(window);
        }

        let replace_styles = self.search_replace_text_input_styles();
        let replace = self.search_replace.clone();
        if let Some(editor) = &self.search_replace_editor {
            editor.update(cx, |input, cx| {
                input.styles = replace_styles;
                input.set_text(&replace, cx);
            });
        }

        cx.notify();
    }

    pub(crate) fn dismiss_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.search_visible = false;
        self.search_matches.clear();
        self.search_current_index = None;
        window.focus(&self.focus_handle(cx));
        cx.notify();
    }

    pub(crate) fn run_search(&mut self, cx: &mut Context<Self>) {
        self.search_matches = find_search_matches(
            self.buffer.text(),
            &self.search_query,
            self.search_case_sensitive,
            self.search_whole_word,
            self.search_use_regex,
        );
        self.search_current_index = None;

        if !self.search_matches.is_empty() {
            let cursor_offset = self.cursor.offset;
            self.search_current_index = Some(
                self.search_matches
                    .iter()
                    .position(|range| range.start >= cursor_offset)
                    .unwrap_or(0),
            );
        }

        cx.notify();
    }

    pub(crate) fn search_navigate(&mut self, forward: bool, cx: &mut Context<Self>) {
        let reran_search = if self.search_matches.is_empty() && !self.search_query.is_empty() {
            self.run_search(cx);
            true
        } else {
            false
        };

        if self.search_matches.is_empty() {
            return;
        }

        let count = self.search_matches.len();
        let index = match (reran_search, self.search_current_index) {
            (true, Some(current)) => {
                if forward {
                    current
                } else {
                    (current + count - 1) % count
                }
            }
            (true, None) => {
                if forward {
                    0
                } else {
                    count - 1
                }
            }
            (false, Some(current)) => {
                if forward {
                    (current + 1) % count
                } else {
                    (current + count - 1) % count
                }
            }
            (false, None) => 0,
        };

        self.search_current_index = Some(index);
        let range = &self.search_matches[index];
        self.cursor.offset = range.start;
        self.cursor.anchor = range.start;
        self.center_cursor_in_view(cx);
    }

    pub(crate) fn replace_current_match(&mut self, cx: &mut Context<Self>) {
        if self.active_tab_is_read_only() {
            return;
        }
        let Some(index) = self.search_current_index else {
            return;
        };
        if index >= self.search_matches.len() {
            return;
        }
        let range = self.search_matches[index].clone();
        let replacement = self.search_replace.clone();
        let len_diff = replacement.len() as isize - (range.end - range.start) as isize;

        self.checkpoint();
        self.buffer.replace(range.clone(), &replacement);

        self.search_matches.remove(index);
        for search_match in &mut self.search_matches[index..] {
            search_match.start = (search_match.start as isize + len_diff) as usize;
            search_match.end = (search_match.end as isize + len_diff) as usize;
        }

        if self.search_matches.is_empty() {
            self.search_current_index = None;
        } else {
            self.search_current_index = Some(index.min(self.search_matches.len() - 1));
        }

        self.cursor.offset = range.start + replacement.len();
        self.cursor.anchor = self.cursor.offset;
        self.mark_active_tab_dirty();
        self.reparse();
        cx.notify();
    }

    pub(crate) fn replace_all_matches(&mut self, cx: &mut Context<Self>) {
        if self.active_tab_is_read_only() {
            return;
        }
        if self.search_matches.is_empty() {
            return;
        }

        self.checkpoint();
        let replacement = self.search_replace.clone();
        let matches: Vec<_> = self.search_matches.drain(..).rev().collect();
        for range in &matches {
            self.buffer.replace(range.clone(), &replacement);
        }
        self.search_current_index = None;
        self.mark_active_tab_dirty();
        self.reparse();
        self.run_search(cx);
    }

    pub(crate) fn search_query_text_input_styles(&self) -> crate::style::TextInputStyles {
        let mut styles = self.styles.text_input.clone();
        styles.text_input_height_px = self.scaled(self.styles.text_input.text_input_height_px);
        styles.text_input_padding_left_px =
            self.scaled(self.styles.search.search_input_left_inset_px);
        styles.text_input_padding_right_px =
            self.scaled(self.styles.search.search_input_right_inset_px);
        styles.text_input_font_size_px =
            self.scaled(self.styles.text_input.text_input_font_size_px);
        styles.text_input_radius_px = self.scaled(self.styles.text_input.text_input_radius_px);
        styles.text_input_cursor_width_px =
            self.scaled(self.styles.text_input.text_input_cursor_width_px);
        styles.text_input_bg = gpui::transparent_black();
        styles.text_input_focus_bg = self.styles.search.search_input_focus_bg;
        styles.text_input_cursor_height_ratio = self.styles.search.search_input_cursor_height_ratio;
        styles
    }

    pub(crate) fn search_replace_text_input_styles(&self) -> crate::style::TextInputStyles {
        let mut styles = self.styles.text_input.clone();
        styles.text_input_height_px = self.scaled(self.styles.text_input.text_input_height_px);
        styles.text_input_padding_left_px =
            self.scaled(self.styles.text_input.text_input_padding_left_px);
        styles.text_input_padding_right_px =
            self.scaled(self.styles.text_input.text_input_padding_right_px);
        styles.text_input_font_size_px =
            self.scaled(self.styles.text_input.text_input_font_size_px);
        styles.text_input_radius_px = self.scaled(self.styles.text_input.text_input_radius_px);
        styles.text_input_cursor_width_px =
            self.scaled(self.styles.text_input.text_input_cursor_width_px);
        styles.text_input_bg = gpui::transparent_black();
        styles.text_input_focus_bg = gpui::transparent_black();
        styles.text_input_cursor_height_ratio = self.styles.search.search_input_cursor_height_ratio;
        styles
    }

    pub(crate) fn render_search_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let search_styles = &self.styles.search;

        let match_count = self.search_matches.len();
        let current_idx = self.search_current_index;
        let counter_text = if match_count > 0 {
            format!(
                "{} / {}",
                current_idx.map(|index| index + 1).unwrap_or(0),
                match_count
            )
        } else if !self.search_query.is_empty() {
            self.t(crate::i18n::TextKey::SearchNoResults).to_string()
        } else {
            String::new()
        };

        let input_height = self.scaled(self.styles.text_input.text_input_height_px);
        let button_size = self.scaled(search_styles.search_option_btn_size_px);
        let button_icon_size = self.scaled(search_styles.search_option_btn_icon_size_px);
        let button_radius = self.scaled(search_styles.search_option_btn_radius_px);
        let button_gap = self.scaled(search_styles.search_input_icon_gap_px);
        let icon_size = self.scaled(search_styles.search_input_icon_size_px);
        let input_left_inset = self.scaled(search_styles.search_input_left_inset_px);
        let input_left = (input_left_inset - icon_size).max(0.0) / 2.0;
        let icon_top = (input_height - icon_size).max(0.0) / 2.0;

        let button_active_bg = search_styles.search_option_btn_active_bg;
        let button_hover_bg = search_styles.search_option_btn_hover_bg;
        let button_icon_color = search_styles.search_option_btn_icon_color;
        let button_active_icon_color = search_styles.search_option_btn_active_icon_color;
        let nav_btn_size = self.scaled(search_styles.search_nav_btn_size_px);
        let nav_btn_icon_size = self.scaled(search_styles.search_nav_btn_icon_size_px);
        let nav_icon_color = search_styles.search_nav_btn_icon_color;
        let nav_hover_bg = search_styles.search_nav_btn_hover_bg;
        let close_hover_bg = search_styles.search_close_btn_hover_bg;

        let query_row = div()
            .w_full()
            .flex()
            .items_center()
            .gap(px(self.scaled(search_styles.search_input_gap_px)))
            .child(
                div()
                    .relative()
                    .flex_1()
                    .h(px(input_height))
                    .when_some(self.search_query_editor.clone(), |this, editor| {
                        this.child(editor)
                    })
                    .child(
                        div()
                            .absolute()
                            .left(px(input_left))
                            .top(px(icon_top))
                            .w(px(icon_size))
                            .h(px(icon_size))
                            .child(
                                svg()
                                    .path(SEARCH_ICON_SVG_PATH)
                                    .size(px(icon_size))
                                    .text_color(search_styles.search_input_icon_color),
                            ),
                    ),
            )
            .child(
                div()
                    .text_size(px(self.scaled(search_styles.search_counter_font_size_px)))
                    .text_color(search_styles.search_counter_text_color)
                    .min_w(px(
                        self.scaled(search_styles.search_input_counter_min_width_px)
                    ))
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(counter_text),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(button_gap))
                    .child(
                        div()
                            .id("search-case-btn")
                            .w(px(button_size))
                            .h(px(button_size))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(button_radius))
                            .cursor(gpui::CursorStyle::Arrow)
                            .when(
                                self.search_case_sensitive,
                                |this: gpui::Stateful<gpui::Div>| this.bg(button_active_bg),
                            )
                            .hover(move |style| style.bg(button_hover_bg))
                            .child(
                                svg()
                                    .path(SEARCH_CASE_ICON_SVG_PATH)
                                    .size(px(button_icon_size))
                                    .text_color(if self.search_case_sensitive {
                                        button_active_icon_color
                                    } else {
                                        button_icon_color
                                    }),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.search_case_sensitive = !this.search_case_sensitive;
                                    this.run_search(cx);
                                }),
                            ),
                    )
                    .child(
                        div()
                            .id("search-word-btn")
                            .w(px(button_size))
                            .h(px(button_size))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(button_radius))
                            .cursor(gpui::CursorStyle::Arrow)
                            .when(self.search_whole_word, |this: gpui::Stateful<gpui::Div>| {
                                this.bg(button_active_bg)
                            })
                            .hover(move |style| style.bg(button_hover_bg))
                            .child(
                                svg()
                                    .path(SEARCH_WHOLE_WORD_ICON_SVG_PATH)
                                    .size(px(button_icon_size))
                                    .text_color(if self.search_whole_word {
                                        button_active_icon_color
                                    } else {
                                        button_icon_color
                                    }),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.search_whole_word = !this.search_whole_word;
                                    this.run_search(cx);
                                }),
                            ),
                    )
                    .child(
                        div()
                            .id("search-regex-btn")
                            .w(px(button_size))
                            .h(px(button_size))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(button_radius))
                            .cursor(gpui::CursorStyle::Arrow)
                            .when(self.search_use_regex, |this: gpui::Stateful<gpui::Div>| {
                                this.bg(button_active_bg)
                            })
                            .hover(move |style| style.bg(button_hover_bg))
                            .child(
                                svg()
                                    .path(SEARCH_REGEX_ICON_SVG_PATH)
                                    .size(px(button_icon_size))
                                    .text_color(if self.search_use_regex {
                                        button_active_icon_color
                                    } else {
                                        button_icon_color
                                    }),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.search_use_regex = !this.search_use_regex;
                                    this.run_search(cx);
                                }),
                            ),
                    ),
            )
            .child(
                div()
                    .id("search-prev-btn")
                    .w(px(nav_btn_size))
                    .h(px(nav_btn_size))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(button_radius))
                    .cursor(gpui::CursorStyle::Arrow)
                    .hover(move |style| style.bg(nav_hover_bg))
                    .child(
                        svg()
                            .path("assets/icons/arrow-up.svg")
                            .size(px(nav_btn_icon_size))
                            .text_color(nav_icon_color),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.search_navigate(false, cx);
                        }),
                    ),
            )
            .child(
                div()
                    .id("search-next-btn")
                    .w(px(nav_btn_size))
                    .h(px(nav_btn_size))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(button_radius))
                    .cursor(gpui::CursorStyle::Arrow)
                    .hover(move |style| style.bg(nav_hover_bg))
                    .child(
                        svg()
                            .path("assets/icons/arrow-down.svg")
                            .size(px(nav_btn_icon_size))
                            .text_color(nav_icon_color),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.search_navigate(true, cx);
                        }),
                    ),
            )
            .child(
                div()
                    .id("search-close-btn")
                    .w(px(self.scaled(search_styles.search_close_btn_size_px)))
                    .h(px(self.scaled(search_styles.search_close_btn_size_px)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(button_radius))
                    .cursor(gpui::CursorStyle::Arrow)
                    .hover(move |style| style.bg(close_hover_bg))
                    .child(
                        svg()
                            .path(SEARCH_CLOSE_ICON_SVG_PATH)
                            .size(px(button_icon_size))
                            .text_color(search_styles.search_close_btn_icon_color),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            cx.stop_propagation();
                            this.dismiss_search(window, cx);
                        }),
                    ),
            );

        let mut container = div()
            .id("search-bar")
            .occlude()
            .absolute()
            .top(px(self.scaled(search_styles.search_bar_offset_top_px)))
            .right(px(self.scaled(search_styles.search_bar_offset_right_px)))
            .w(px(self.scaled(search_styles.search_bar_width_px)))
            .bg(search_styles.search_bar_bg)
            .border(px(search_styles.search_bar_border_width_px))
            .border_color(search_styles.search_bar_border_color)
            .rounded(px(self.scaled(search_styles.search_bar_radius_px)))
            .when(search_styles.search_bar_shadow, |this| this.shadow_md())
            .p(px(self.scaled(search_styles.search_bar_padding_px)))
            .flex()
            .flex_col()
            .gap(px(self.scaled(search_styles.search_bar_gap_px)))
            .child(query_row);

        if self.search_replace_visible {
            let replace_btn_hover = search_styles.search_replace_btn_hover_bg;
            let replace_btn_font = self.scaled(search_styles.search_replace_btn_font_size_px);
            let replace_btn_color = search_styles.search_replace_btn_text_color;
            let replace_btn_height = self.scaled(search_styles.search_replace_btn_height_px);
            let replace_btn_padding = self.scaled(search_styles.search_replace_btn_padding_x_px);

            container = container.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(search_styles.search_bar_gap_px)))
                    .w_full()
                    .when_some(self.search_replace_editor.clone(), |this, editor| {
                        this.child(
                            div()
                                .flex_1()
                                .h(px(self.scaled(self.styles.text_input.text_input_height_px)))
                                .child(editor),
                        )
                    })
                    .child(
                        div()
                            .id("search-replace-btn")
                            .px(px(replace_btn_padding))
                            .h(px(replace_btn_height))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(button_radius))
                            .text_size(px(replace_btn_font))
                            .text_color(replace_btn_color)
                            .cursor(gpui::CursorStyle::Arrow)
                            .hover(move |style| style.bg(replace_btn_hover))
                            .child(self.t(crate::i18n::TextKey::Replace))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.replace_current_match(cx);
                                }),
                            ),
                    )
                    .child(
                        div()
                            .id("search-replace-all-btn")
                            .px(px(replace_btn_padding))
                            .h(px(replace_btn_height))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(button_radius))
                            .text_size(px(replace_btn_font))
                            .text_color(replace_btn_color)
                            .cursor(gpui::CursorStyle::Arrow)
                            .hover(move |style| style.bg(replace_btn_hover))
                            .child(self.t(crate::i18n::TextKey::ReplaceAll))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.replace_all_matches(cx);
                                }),
                            ),
                    ),
            );
        }

        container.into_any_element()
    }
}

pub(crate) fn find_search_matches(
    text: &str,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    use_regex: bool,
) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }

    let mut matches = Vec::new();

    if use_regex {
        let regex = if case_sensitive {
            regex::Regex::new(query)
        } else {
            regex::RegexBuilder::new(query)
                .case_insensitive(true)
                .build()
        };
        if let Ok(regex) = regex {
            for found in regex.find_iter(text) {
                if found.start() != found.end() {
                    matches.push(found.start()..found.end());
                }
            }
        }
        return matches;
    }

    let haystack = if case_sensitive {
        text.to_string()
    } else {
        text.to_lowercase()
    };
    let needle = if case_sensitive {
        query.to_string()
    } else {
        query.to_lowercase()
    };

    let mut start = 0;
    while let Some(position) = haystack[start..].find(&needle) {
        let match_start = start + position;
        let match_end = match_start + needle.len();

        if whole_word
            && (!is_word_boundary(text, match_start) || !is_word_boundary(text, match_end))
        {
            start = match_start + 1;
            continue;
        }

        matches.push(match_start..match_end);
        start = match_end;
    }

    matches
}

fn is_word_boundary(text: &str, index: usize) -> bool {
    if index == 0 || index >= text.len() {
        return true;
    }

    let before = text[..index].chars().next_back();
    let after = text[index..].chars().next();
    !before.is_some_and(is_word_char) || !after.is_some_and(is_word_char)
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}
