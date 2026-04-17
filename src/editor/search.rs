use gpui::{AnyElement, Context, MouseButton, div, prelude::*, px};

use super::*;

impl EditorView {
    // ── Search actions ──────────────────────────────────────────────

    pub(crate) fn on_find(&mut self, _: &Find, _: &mut gpui::Window, cx: &mut Context<Self>) {
        self.open_search(false, cx);
    }

    pub(crate) fn on_find_next(
        &mut self,
        _: &FindNext,
        _: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.search_navigate(true, cx);
    }

    pub(crate) fn on_find_previous(
        &mut self,
        _: &FindPrevious,
        _: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.search_navigate(false, cx);
    }

    pub(crate) fn on_dismiss_search(
        &mut self,
        _: &DismissSearch,
        _: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_search(cx);
    }

    pub(crate) fn on_toggle_replace(
        &mut self,
        _: &ToggleReplace,
        _: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        if !self.search_visible {
            self.open_search(true, cx);
        } else {
            self.search_replace_visible = !self.search_replace_visible;
            cx.notify();
        }
    }

    pub(crate) fn on_replace_next(
        &mut self,
        _: &ReplaceNext,
        _: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_current_match(cx);
    }

    pub(crate) fn on_replace_all(
        &mut self,
        _: &ReplaceAll,
        _: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.replace_all_matches(cx);
    }

    // ── Search logic ────────────────────────────────────────────────

    pub(crate) fn open_search(&mut self, with_replace: bool, cx: &mut Context<Self>) {
        let was_visible = self.search_visible;
        self.search_visible = true;
        self.search_replace_visible = with_replace;

        if self.search_query_editor.is_none() {
            let styles = self.styles.text_input.clone();
            let editor = cx.new(|cx| {
                let mut ti = crate::text_input::TextInput::new(styles, "搜索...", cx);
                if !self.search_query.is_empty() {
                    ti.set_text(&self.search_query, cx);
                    ti.select_all(cx);
                }
                ti
            });
            cx.subscribe(
                &editor,
                |this, _editor, event: &crate::text_input::TextInputEvent, cx| match event {
                    crate::text_input::TextInputEvent::Changed => {
                        let query = this
                            .search_query_editor
                            .as_ref()
                            .map(|e| e.read(cx).text().to_string())
                            .unwrap_or_default();
                        this.search_query = query;
                        this.run_search(cx);
                    }
                    crate::text_input::TextInputEvent::Submit => {
                        this.search_navigate(true, cx);
                    }
                    crate::text_input::TextInputEvent::Escape => {
                        this.dismiss_search(cx);
                    }
                    _ => {}
                },
            )
            .detach();
            self.search_query_editor = Some(editor);
        } else if was_visible {
            if let Some(editor) = &self.search_query_editor {
                editor.update(cx, |ti, cx| {
                    ti.select_all(cx);
                });
            }
        }

        // Seed search with selected text
        if !was_visible {
            let sel = self.cursor.selection_range();
            if !sel.is_empty() && sel.len() < 200 {
                let selected_text = self.buffer.text()[sel.clone()].to_string();
                if !selected_text.contains('\n') {
                    self.search_query = selected_text.clone();
                    if let Some(editor) = &self.search_query_editor {
                        editor.update(cx, |ti, cx| {
                            ti.set_text(&selected_text, cx);
                            ti.select_all(cx);
                        });
                    }
                    self.run_search(cx);
                }
            }
        }

        if self.search_replace_editor.is_none() {
            let styles = self.styles.text_input.clone();
            let editor = cx.new(|cx| {
                let mut ti = crate::text_input::TextInput::new(styles, "替换...", cx);
                if !self.search_replace.is_empty() {
                    ti.set_text(&self.search_replace, cx);
                }
                ti
            });
            cx.subscribe(
                &editor,
                |this, _editor, event: &crate::text_input::TextInputEvent, cx| match event {
                    crate::text_input::TextInputEvent::Changed => {
                        let replace = this
                            .search_replace_editor
                            .as_ref()
                            .map(|e| e.read(cx).text().to_string())
                            .unwrap_or_default();
                        this.search_replace = replace;
                    }
                    crate::text_input::TextInputEvent::Submit => {
                        this.replace_current_match(cx);
                    }
                    crate::text_input::TextInputEvent::Escape => {
                        this.dismiss_search(cx);
                    }
                    _ => {}
                },
            )
            .detach();
            self.search_replace_editor = Some(editor);
        }

        cx.notify();
    }

    pub(crate) fn dismiss_search(&mut self, cx: &mut Context<Self>) {
        self.search_visible = false;
        self.search_matches.clear();
        self.search_current_index = None;
        cx.notify();
    }

    pub(crate) fn run_search(&mut self, cx: &mut Context<Self>) {
        self.search_matches.clear();
        self.search_current_index = None;

        let query = &self.search_query;
        if query.is_empty() {
            cx.notify();
            return;
        }

        let text = self.buffer.text();

        if self.search_use_regex {
            if let Ok(re) = if self.search_case_sensitive {
                regex::Regex::new(query)
            } else {
                regex::RegexBuilder::new(query)
                    .case_insensitive(true)
                    .build()
            } {
                for m in re.find_iter(text) {
                    if m.start() != m.end() {
                        self.search_matches.push(m.start()..m.end());
                    }
                }
            }
        } else {
            let hay = if self.search_case_sensitive {
                text.to_string()
            } else {
                text.to_lowercase()
            };
            let needle = if self.search_case_sensitive {
                query.to_string()
            } else {
                query.to_lowercase()
            };
            if !needle.is_empty() {
                let mut start = 0;
                while let Some(pos) = hay[start..].find(&needle) {
                    let match_start = start + pos;
                    let match_end = match_start + needle.len();

                    if self.search_whole_word {
                        let before_ok = match_start == 0
                            || !text[..match_start]
                                .chars()
                                .next_back()
                                .is_some_and(|c| c.is_alphanumeric() || c == '_');
                        let after_ok = match_end >= text.len()
                            || !text[match_end..]
                                .chars()
                                .next()
                                .is_some_and(|c| c.is_alphanumeric() || c == '_');
                        if !before_ok || !after_ok {
                            start = match_start + 1;
                            continue;
                        }
                    }

                    self.search_matches.push(match_start..match_end);
                    start = match_end;
                }
            }
        }

        if !self.search_matches.is_empty() {
            let cursor_offset = self.cursor.offset;
            self.search_current_index = Some(
                self.search_matches
                    .iter()
                    .position(|r| r.start >= cursor_offset)
                    .unwrap_or(0),
            );
        }

        cx.notify();
    }

    pub(crate) fn search_navigate(&mut self, forward: bool, cx: &mut Context<Self>) {
        if self.search_matches.is_empty() {
            return;
        }
        let count = self.search_matches.len();
        let idx = match self.search_current_index {
            Some(i) => {
                if forward {
                    (i + 1) % count
                } else {
                    (i + count - 1) % count
                }
            }
            None => 0,
        };
        self.search_current_index = Some(idx);
        let range = &self.search_matches[idx];
        self.cursor.offset = range.start;
        self.cursor.anchor = range.start;
        self.scroll_to_cursor();
        cx.notify();
    }

    pub(crate) fn replace_current_match(&mut self, cx: &mut Context<Self>) {
        let Some(idx) = self.search_current_index else {
            return;
        };
        if idx >= self.search_matches.len() {
            return;
        }
        let range = self.search_matches[idx].clone();
        let replacement = self.search_replace.clone();
        let len_diff = replacement.len() as isize - (range.end - range.start) as isize;

        self.checkpoint();
        self.buffer.replace(range.clone(), &replacement);

        self.search_matches.remove(idx);
        for m in &mut self.search_matches[idx..] {
            m.start = (m.start as isize + len_diff) as usize;
            m.end = (m.end as isize + len_diff) as usize;
        }

        if self.search_matches.is_empty() {
            self.search_current_index = None;
        } else {
            self.search_current_index = Some(idx.min(self.search_matches.len() - 1));
        }

        self.cursor.offset = range.start + replacement.len();
        self.cursor.anchor = self.cursor.offset;
        self.mark_active_tab_dirty();
        self.reparse();
        cx.notify();
    }

    pub(crate) fn replace_all_matches(&mut self, cx: &mut Context<Self>) {
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

    // ── Search UI ───────────────────────────────────────────────────

    pub(crate) fn render_search_bar(&self, cx: &mut Context<Self>) -> AnyElement {
        let ss = &self.styles.search;

        let match_count = self.search_matches.len();
        let current_idx = self.search_current_index;
        let counter_text = if match_count > 0 {
            format!(
                "{} / {}",
                current_idx.map(|i| i + 1).unwrap_or(0),
                match_count
            )
        } else if !self.search_query.is_empty() {
            "无结果".to_string()
        } else {
            String::new()
        };

        let case_active = self.search_case_sensitive;
        let word_active = self.search_whole_word;
        let regex_active = self.search_use_regex;

        let btn_active_bg = ss.search_option_btn_active_bg;
        let btn_hover_bg = ss.search_option_btn_hover_bg;
        let btn_size = self.scaled(ss.search_option_btn_size_px);
        let btn_icon_size = self.scaled(ss.search_option_btn_icon_size_px);
        let btn_color = ss.search_option_btn_icon_color;
        let btn_active_color = ss.search_option_btn_active_icon_color;
        let bar_radius = self.scaled(ss.search_bar_radius_px);
        let close_hover_bg = ss.search_close_btn_hover_bg;
        let nav_btn_size = self.scaled(ss.search_nav_btn_size_px);
        let nav_btn_icon_size = self.scaled(ss.search_nav_btn_icon_size_px);
        let nav_icon_color = ss.search_nav_btn_icon_color;
        let nav_hover_bg = ss.search_nav_btn_hover_bg;

        let query_row = div()
            .flex()
            .items_center()
            .gap(px(self.scaled(4.0)))
            .flex_1()
            .when_some(self.search_query_editor.clone(), |this, editor| {
                this.child(
                    div()
                        .flex_1()
                        .h(px(self.scaled(self.styles.text_input.text_input_height_px)))
                        .child(editor),
                )
            })
            .child(
                div()
                    .id("search-case-btn")
                    .w(px(btn_size))
                    .h(px(btn_size))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(self.scaled(3.0)))
                    .text_size(px(btn_icon_size))
                    .cursor(gpui::CursorStyle::PointingHand)
                    .when(case_active, |this: gpui::Stateful<gpui::Div>| {
                        this.bg(btn_active_bg)
                    })
                    .text_color(if case_active {
                        btn_active_color
                    } else {
                        btn_color
                    })
                    .hover(move |s| s.bg(btn_hover_bg))
                    .child("Aa")
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
                    .w(px(btn_size))
                    .h(px(btn_size))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(self.scaled(3.0)))
                    .text_size(px(btn_icon_size))
                    .cursor(gpui::CursorStyle::PointingHand)
                    .when(word_active, |this: gpui::Stateful<gpui::Div>| {
                        this.bg(btn_active_bg)
                    })
                    .text_color(if word_active {
                        btn_active_color
                    } else {
                        btn_color
                    })
                    .hover(move |s| s.bg(btn_hover_bg))
                    .child("Ab")
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
                    .w(px(btn_size))
                    .h(px(btn_size))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(self.scaled(3.0)))
                    .text_size(px(btn_icon_size))
                    .cursor(gpui::CursorStyle::PointingHand)
                    .when(regex_active, |this: gpui::Stateful<gpui::Div>| {
                        this.bg(btn_active_bg)
                    })
                    .text_color(if regex_active {
                        btn_active_color
                    } else {
                        btn_color
                    })
                    .hover(move |s| s.bg(btn_hover_bg))
                    .child(".*")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.search_use_regex = !this.search_use_regex;
                            this.run_search(cx);
                        }),
                    ),
            );

        let nav_row = div()
            .flex()
            .items_center()
            .gap(px(self.scaled(4.0)))
            .child(
                div()
                    .text_size(px(self.scaled(ss.search_counter_font_size_px)))
                    .text_color(ss.search_counter_text_color)
                    .min_w(px(self.scaled(50.0)))
                    .text_ellipsis()
                    .whitespace_nowrap()
                    .child(counter_text),
            )
            .child(
                div()
                    .id("search-prev-btn")
                    .w(px(nav_btn_size))
                    .h(px(nav_btn_size))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(self.scaled(3.0)))
                    .text_size(px(nav_btn_icon_size))
                    .text_color(nav_icon_color)
                    .cursor(gpui::CursorStyle::PointingHand)
                    .hover(move |s| s.bg(nav_hover_bg))
                    .child("↑")
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
                    .rounded(px(self.scaled(3.0)))
                    .text_size(px(nav_btn_icon_size))
                    .text_color(nav_icon_color)
                    .cursor(gpui::CursorStyle::PointingHand)
                    .hover(move |s| s.bg(nav_hover_bg))
                    .child("↓")
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
                    .w(px(self.scaled(ss.search_close_btn_size_px)))
                    .h(px(self.scaled(ss.search_close_btn_size_px)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(self.scaled(3.0)))
                    .text_size(px(self.scaled(ss.search_close_btn_size_px * 0.6)))
                    .text_color(ss.search_close_btn_icon_color)
                    .cursor(gpui::CursorStyle::PointingHand)
                    .hover(move |s| s.bg(close_hover_bg))
                    .child("✕")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| {
                            cx.stop_propagation();
                            this.dismiss_search(cx);
                        }),
                    ),
            );

        let bar_padding = self.scaled(ss.search_bar_padding_px);

        let mut container = div()
            .id("search-bar")
            .absolute()
            .top(px(self.scaled(ss.search_bar_offset_top_px)))
            .right(px(self.scaled(ss.search_bar_offset_right_px)))
            .w(px(self.scaled(ss.search_bar_width_px)))
            .bg(ss.search_bar_bg)
            .border(px(ss.search_bar_border_width_px))
            .border_color(ss.search_bar_border_color)
            .rounded(px(bar_radius))
            .shadow_md()
            .p(px(bar_padding))
            .flex()
            .flex_col()
            .gap(px(self.scaled(ss.search_bar_gap_px)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(ss.search_bar_gap_px)))
                    .child(query_row)
                    .child(nav_row),
            );

        if self.search_replace_visible {
            let replace_btn_hover = ss.search_replace_btn_hover_bg;
            let replace_btn_font = self.scaled(ss.search_replace_btn_font_size_px);
            let replace_btn_color = ss.search_replace_btn_text_color;
            let replace_btn_height = self.scaled(ss.search_replace_btn_height_px);

            container = container.child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(4.0)))
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
                            .px(px(self.scaled(6.0)))
                            .h(px(replace_btn_height))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(self.scaled(3.0)))
                            .text_size(px(replace_btn_font))
                            .text_color(replace_btn_color)
                            .cursor(gpui::CursorStyle::PointingHand)
                            .hover(move |s| s.bg(replace_btn_hover))
                            .child("替换")
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
                            .px(px(self.scaled(6.0)))
                            .h(px(replace_btn_height))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(self.scaled(3.0)))
                            .text_size(px(replace_btn_font))
                            .text_color(replace_btn_color)
                            .cursor(gpui::CursorStyle::PointingHand)
                            .hover(move |s| s.bg(replace_btn_hover))
                            .child("全部替换")
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
