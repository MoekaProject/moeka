use std::path::PathBuf;

use gpui::{
    AnyElement, Context, CursorStyle, FontWeight, MouseButton, Window, div, prelude::*, px, svg,
};

use super::search::{
    SEARCH_CASE_ICON_SVG_PATH, SEARCH_CLOSE_ICON_SVG_PATH, SEARCH_ICON_SVG_PATH,
    SEARCH_REGEX_ICON_SVG_PATH, SEARCH_WHOLE_WORD_ICON_SVG_PATH, find_search_matches,
};
use super::*;

impl EditorView {
    pub(crate) fn on_find_in_files(
        &mut self,
        _: &FindInFiles,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_workspace_search(window, cx);
    }

    pub(crate) fn toggle_workspace_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.workspaces.is_empty() {
            self.workspace_search_visible = false;
            self.workspace_search_task = None;
            cx.notify();
            return;
        }
        self.workspace_search_visible = !self.workspace_search_visible;
        if !self.workspace_search_visible {
            self.close_workspace_search(window, cx);
            return;
        }

        self.sidebar_visible = true;
        if self.workspace_search_editor.is_none() {
            let styles = self.workspace_search_text_input_styles();
            let initial_query = self.workspace_search_query.clone();
            let editor = cx.new(|cx| {
                let mut input = crate::text_input::TextInput::new(styles, "搜索...", cx);
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
                            .workspace_search_editor
                            .as_ref()
                            .map(|entity| entity.read(cx).text().to_string())
                            .unwrap_or_default();
                        this.workspace_search_query = query;
                        this.run_workspace_search(cx);
                    }
                    crate::text_input::TextInputEvent::Submit => {
                        this.commit_workspace_search_selected_match(cx);
                    }
                    crate::text_input::TextInputEvent::Escape => {}
                    crate::text_input::TextInputEvent::MoveUp => {
                        this.move_workspace_search_selection(-1, cx);
                    }
                    crate::text_input::TextInputEvent::MoveDown => {
                        this.move_workspace_search_selection(1, cx);
                    }
                },
            )
            .detach();
            self.workspace_search_editor = Some(editor);
        }

        if let Some(editor) = &self.workspace_search_editor {
            let focus_handle = editor.read(cx).focus_handle.clone();
            let focus_sub = cx.on_focus(&focus_handle, window, |this, _window, cx| {
                this.workspace_search_input_focused = true;
                cx.notify();
            });
            let blur_sub = cx.on_blur(&focus_handle, window, |this, _window, cx| {
                this.workspace_search_input_focused = false;
                this.close_workspace_search_if_trimmed_empty(cx);
            });
            self.workspace_search_focus_sub = Some(focus_sub);
            self.workspace_search_blur_sub = Some(blur_sub);
        }

        let styles = self.workspace_search_text_input_styles();
        let query = self.workspace_search_query.clone();
        if let Some(editor) = &self.workspace_search_editor {
            editor.update(cx, |input, cx| {
                input.styles = styles;
                input.set_text(&query, cx);
                input.select_all(cx);
            });
            editor.focus_handle(cx).focus(window);
        }

        cx.notify();
    }

    pub(crate) fn close_workspace_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.workspace_search_visible = false;
        self.workspace_search_focus_sub = None;
        self.workspace_search_task = None;
        self.workspace_search_blur_sub = None;
        self.workspace_search_input_focused = false;
        self.clear_workspace_search_preview_state();
        window.focus(&self.focus_handle(cx));
        cx.notify();
    }

    fn clear_workspace_search_preview_state(&mut self) {
        self.workspace_search_selected_match = None;
        self.workspace_search_highlight_path = None;
        self.workspace_search_highlight_ranges.clear();
        self.workspace_search_highlight_current_index = None;
    }

    fn close_workspace_search_if_trimmed_empty(&mut self, cx: &mut Context<Self>) {
        if !self.workspace_search_query.trim().is_empty() {
            return;
        }

        self.workspace_search_query.clear();
        self.workspace_search_visible = false;
        self.sidebar_visible = false;
        self.workspace_search_results.clear();
        self.workspace_search_focus_sub = None;
        self.workspace_search_task = None;
        self.workspace_search_blur_sub = None;
        self.workspace_search_input_focused = false;
        self.workspace_search_render_limit = 100;
        self.clear_workspace_search_preview_state();
        cx.notify();
    }

    fn workspace_search_total_match_count(&self) -> usize {
        self.workspace_search_results
            .iter()
            .map(|result| result.matches.iter().take(50).count())
            .sum()
    }

    fn workspace_search_match_at(
        &self,
        target_index: usize,
    ) -> Option<(PathBuf, WorkspaceSearchMatch)> {
        let mut current_index = 0usize;
        for result in &self.workspace_search_results {
            for search_match in result.matches.iter().take(50) {
                if current_index == target_index {
                    return Some((result.path.clone(), search_match.clone()));
                }
                current_index += 1;
            }
        }
        None
    }

    fn workspace_search_child_index_for_match(&self, target_index: usize) -> Option<usize> {
        let mut current_index = 0usize;
        let mut child_index = 2usize;
        for result in &self.workspace_search_results {
            child_index += 1;
            for _ in result.matches.iter().take(50) {
                if current_index == target_index {
                    return Some(child_index);
                }
                current_index += 1;
                child_index += 1;
            }
        }
        None
    }

    fn show_workspace_search_match(
        &mut self,
        path: PathBuf,
        search_match: WorkspaceSearchMatch,
        permanent: bool,
        focus_editor: bool,
        cx: &mut Context<Self>,
    ) {
        if permanent {
            self.open_path_in_permanent_tab(path.clone(), focus_editor, cx);
        } else {
            self.open_path_in_preview_tab(path.clone(), focus_editor, cx);
        }

        self.reveal_workspace_search_match(path, search_match, cx);
    }

    fn reveal_workspace_search_match(
        &mut self,
        path: PathBuf,
        search_match: WorkspaceSearchMatch,
        cx: &mut Context<Self>,
    ) {
        self.workspace_search_highlight_path = Some(path);
        self.workspace_search_highlight_ranges =
            vec![search_match.byte_offset..search_match.byte_end];
        self.workspace_search_highlight_current_index = Some(0);
        self.cursor.offset = search_match.byte_offset.min(self.buffer.len());
        self.cursor.anchor = self.cursor.offset;
        self.scroll_to_cursor();
        cx.notify();
    }

    fn preview_workspace_search_match_at(
        &mut self,
        match_index: usize,
        focus_editor: bool,
        permanent: bool,
        cx: &mut Context<Self>,
    ) {
        let Some((path, search_match)) = self.workspace_search_match_at(match_index) else {
            return;
        };
        self.workspace_search_selected_match = Some(match_index);
        if let Some(child_index) = self.workspace_search_child_index_for_match(match_index) {
            let needed_rows = child_index.saturating_sub(1);
            if needed_rows > self.workspace_search_render_limit {
                self.workspace_search_render_limit = needed_rows;
            }
            self.workspace_search_scroll_handle
                .scroll_to_item(child_index);
        }
        self.show_workspace_search_match(path, search_match, permanent, focus_editor, cx);
    }

    fn move_workspace_search_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        let total = self.workspace_search_total_match_count();
        if total == 0 {
            return;
        }

        let next_index = match self.workspace_search_selected_match {
            Some(current) => (current as isize + delta).clamp(0, total as isize - 1) as usize,
            None if delta >= 0 => 0,
            None => total - 1,
        };
        self.preview_workspace_search_match_at(next_index, false, false, cx);
    }

    fn commit_workspace_search_selected_match(&mut self, cx: &mut Context<Self>) {
        let total = self.workspace_search_total_match_count();
        if total == 0 {
            return;
        }

        let selected = self.workspace_search_selected_match.unwrap_or(0);
        self.preview_workspace_search_match_at(selected.min(total - 1), true, true, cx);
    }

    fn workspace_search_total_row_count(&self) -> usize {
        self.workspace_search_results
            .iter()
            .map(|result| 1 + result.matches.len().min(50))
            .sum()
    }

    fn workspace_search_rendered_child_count(&self) -> usize {
        let trimmed_query = self.workspace_search_query.trim();
        if trimmed_query.is_empty() {
            return 1;
        }
        if self.workspace_search_results.is_empty() {
            return 2;
        }

        2 + self
            .workspace_search_render_limit
            .min(self.workspace_search_total_row_count())
    }

    fn maybe_extend_workspace_search_results(&mut self, cx: &mut Context<Self>) {
        let total_rows = self.workspace_search_total_row_count();
        if total_rows <= self.workspace_search_render_limit {
            return;
        }

        let rendered_children = self.workspace_search_rendered_child_count();
        let bottom_item = self.workspace_search_scroll_handle.bottom_item();
        if bottom_item + 12 >= rendered_children.saturating_sub(1) {
            self.workspace_search_render_limit =
                (self.workspace_search_render_limit + 100).min(total_rows);
            cx.notify();
        }
    }

    pub(crate) fn run_workspace_search(&mut self, cx: &mut Context<Self>) {
        let query = self.workspace_search_query.trim().to_string();
        self.clear_workspace_search_preview_state();
        if query.is_empty() {
            self.workspace_search_results.clear();
            self.workspace_search_task = None;
            self.workspace_search_render_limit = 100;
            cx.notify();
            return;
        }

        let root = self.sidebar_root.clone();
        if !root.exists() {
            self.workspace_search_results.clear();
            self.workspace_search_task = None;
            self.workspace_search_render_limit = 100;
            cx.notify();
            return;
        }

        let case_sensitive = self.workspace_search_case_sensitive;
        let whole_word = self.workspace_search_whole_word;
        let use_regex = self.workspace_search_use_regex;
        let task = cx.spawn(async move |this, cx| {
            let results = cx
                .background_executor()
                .spawn(async move {
                    let mut results = Vec::new();
                    search_directory(
                        &root,
                        &query,
                        case_sensitive,
                        whole_word,
                        use_regex,
                        &mut results,
                    );
                    results
                })
                .await;

            this.update(cx, |this, cx| {
                this.workspace_search_results = results;
                this.workspace_search_task = None;
                this.workspace_search_render_limit = 100;
                this.workspace_search_selected_match = None;
                this.workspace_search_scroll_handle.scroll_to_item(0);
                cx.notify();
            })
            .ok();
        });
        self.workspace_search_task = Some(task);
    }

    pub(crate) fn workspace_search_text_input_styles(&self) -> crate::style::TextInputStyles {
        let mut styles = self.styles.text_input.clone();
        let workspace_styles = &self.styles.workspace_search;
        styles.text_input_height_px = self.scaled(
            workspace_styles.ws_search_input_height_px
                - 2.0 * workspace_styles.ws_search_input_border_width_px,
        );
        styles.text_input_padding_left_px =
            self.scaled(workspace_styles.ws_search_input_left_inset_px);
        styles.text_input_padding_right_px =
            self.scaled(self.styles.text_input.text_input_padding_right_px);
        styles.text_input_font_size_px =
            self.scaled(self.styles.text_input.text_input_font_size_px);
        styles.text_input_bg = gpui::transparent_black();
        styles.text_input_focus_bg = gpui::transparent_black();
        styles.text_input_radius_px = self.scaled(workspace_styles.ws_search_input_radius_px);
        styles.text_input_border_width_px = 0.0;
        styles.text_input_cursor_width_px =
            self.scaled(self.styles.text_input.text_input_cursor_width_px);
        styles.text_input_cursor_height_ratio =
            workspace_styles.ws_search_input_cursor_height_ratio;
        styles.text_input_cursor_height_adjust_px =
            workspace_styles.ws_search_input_cursor_height_adjust_px;
        styles
    }

    pub(crate) fn render_workspace_search(&self, cx: &mut Context<Self>) -> AnyElement {
        let trimmed_query = self.workspace_search_query.trim();
        let workspace_styles = &self.styles.workspace_search;
        let input_height = self.scaled(workspace_styles.ws_search_input_height_px);
        let button_size = self.scaled(workspace_styles.ws_search_btn_size_px);
        let button_icon_size = self.scaled(workspace_styles.ws_search_btn_icon_size_px);
        let button_gap = self.scaled(workspace_styles.ws_search_input_icon_gap_px);
        let input_gap = self.scaled(workspace_styles.ws_search_input_gap_px);
        let icon_size = self.scaled(workspace_styles.ws_search_input_icon_size_px);
        let chrome_padding_x = ((input_height - button_size).max(0.0) / 2.0).round();
        let input_left_inset = self.scaled(workspace_styles.ws_search_input_left_inset_px);
        let input_left = (input_left_inset - icon_size).max(0.0) / 2.0;
        let icon_top = (input_height - icon_size).max(0.0) / 2.0;
        let button_radius = self.scaled(4.0);
        let button_hover_bg = workspace_styles.ws_search_btn_hover_bg;
        let button_active_bg = workspace_styles.ws_search_btn_active_bg;
        let button_icon_color = workspace_styles.ws_search_btn_icon_color;
        let button_active_icon_color = workspace_styles.ws_search_btn_active_icon_color;
        let input_icon_color = workspace_styles.ws_search_input_icon_color;
        let search_border_width = self.scaled(workspace_styles.ws_search_input_border_width_px);
        let search_border_color = workspace_styles.ws_search_input_border_color;
        let search_bg = if self.workspace_search_input_focused {
            workspace_styles.ws_search_input_focus_bg
        } else {
            workspace_styles.ws_search_input_bg
        };
        let result_selected_bg = self.styles.sidebar.sidebar_item_selected_bg;

        let mut children: Vec<AnyElement> = Vec::new();
        children.push(
            div()
                .w_full()
                .p(px(self.scaled(workspace_styles.ws_search_padding_px)))
                .child(
                    div()
                        .w_full()
                        .h(px(input_height))
                        .px(px(chrome_padding_x))
                        .bg(search_bg)
                        .border(px(search_border_width))
                        .border_color(search_border_color)
                        .rounded(px(self.scaled(workspace_styles.ws_search_input_radius_px)))
                        .flex()
                        .items_center()
                        .gap(px(input_gap))
                        .child(
                            div()
                                .relative()
                                .flex_1()
                                .min_w(px(0.0))
                                .h_full()
                                .when_some(self.workspace_search_editor.clone(), |this, editor| {
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
                                                .text_color(input_icon_color),
                                        ),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_none()
                                .items_center()
                                .gap(px(button_gap))
                                .child(
                                    div()
                                        .id("workspace-search-case-btn")
                                        .w(px(button_size))
                                        .h(px(button_size))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(button_radius))
                                        .cursor(CursorStyle::Arrow)
                                        .when(
                                            self.workspace_search_case_sensitive,
                                            |this: gpui::Stateful<gpui::Div>| {
                                                this.bg(button_active_bg)
                                            },
                                        )
                                        .hover(move |style| style.bg(button_hover_bg))
                                        .child(
                                            svg()
                                                .path(SEARCH_CASE_ICON_SVG_PATH)
                                                .size(px(button_icon_size))
                                                .text_color(
                                                    if self.workspace_search_case_sensitive {
                                                        button_active_icon_color
                                                    } else {
                                                        button_icon_color
                                                    },
                                                ),
                                        )
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.workspace_search_case_sensitive =
                                                    !this.workspace_search_case_sensitive;
                                                this.run_workspace_search(cx);
                                            }),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("workspace-search-word-btn")
                                        .w(px(button_size))
                                        .h(px(button_size))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(button_radius))
                                        .cursor(CursorStyle::Arrow)
                                        .when(
                                            self.workspace_search_whole_word,
                                            |this: gpui::Stateful<gpui::Div>| {
                                                this.bg(button_active_bg)
                                            },
                                        )
                                        .hover(move |style| style.bg(button_hover_bg))
                                        .child(
                                            svg()
                                                .path(SEARCH_WHOLE_WORD_ICON_SVG_PATH)
                                                .size(px(button_icon_size))
                                                .text_color(if self.workspace_search_whole_word {
                                                    button_active_icon_color
                                                } else {
                                                    button_icon_color
                                                }),
                                        )
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.workspace_search_whole_word =
                                                    !this.workspace_search_whole_word;
                                                this.run_workspace_search(cx);
                                            }),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("workspace-search-regex-btn")
                                        .w(px(button_size))
                                        .h(px(button_size))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(button_radius))
                                        .cursor(CursorStyle::Arrow)
                                        .when(
                                            self.workspace_search_use_regex,
                                            |this: gpui::Stateful<gpui::Div>| {
                                                this.bg(button_active_bg)
                                            },
                                        )
                                        .hover(move |style| style.bg(button_hover_bg))
                                        .child(
                                            svg()
                                                .path(SEARCH_REGEX_ICON_SVG_PATH)
                                                .size(px(button_icon_size))
                                                .text_color(if self.workspace_search_use_regex {
                                                    button_active_icon_color
                                                } else {
                                                    button_icon_color
                                                }),
                                        )
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, _, _, cx| {
                                                cx.stop_propagation();
                                                this.workspace_search_use_regex =
                                                    !this.workspace_search_use_regex;
                                                this.run_workspace_search(cx);
                                            }),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("workspace-search-close-btn")
                                        .w(px(button_size))
                                        .h(px(button_size))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(button_radius))
                                        .cursor(CursorStyle::Arrow)
                                        .hover(move |style| style.bg(button_hover_bg))
                                        .child(
                                            svg()
                                                .path(SEARCH_CLOSE_ICON_SVG_PATH)
                                                .size(px(button_icon_size))
                                                .text_color(button_icon_color),
                                        )
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(|this, _, window, cx| {
                                                cx.stop_propagation();
                                                this.close_workspace_search(window, cx);
                                            }),
                                        ),
                                ),
                        ),
                )
                .into_any_element(),
        );

        if !trimmed_query.is_empty() && self.workspace_search_results.is_empty() {
            let message = if self.workspace_search_task.is_some() {
                "正在搜索..."
            } else {
                "无结果"
            };
            children.push(
                div()
                    .w_full()
                    .px(px(self.scaled(workspace_styles.ws_search_padding_px)))
                    .py(px(self.scaled(8.0)))
                    .text_size(px(
                        self.scaled(workspace_styles.ws_search_summary_font_size_px)
                    ))
                    .opacity(0.6)
                    .text_color(workspace_styles.ws_search_summary_text_color)
                    .child(message)
                    .into_any_element(),
            );
        } else if !trimmed_query.is_empty() {
            let total_matches: usize = self
                .workspace_search_results
                .iter()
                .map(|result| result.matches.len())
                .sum();
            let file_count = self.workspace_search_results.len();
            let summary_text = if self.workspace_search_task.is_some() {
                format!("正在搜索... 当前 {total_matches} 个结果，{file_count} 个文件")
            } else {
                format!("{total_matches} 个结果，{file_count} 个文件")
            };
            children.push(
                div()
                    .w_full()
                    .px(px(self.scaled(workspace_styles.ws_search_padding_px)))
                    .py(px(self.scaled(4.0)))
                    .text_size(px(
                        self.scaled(workspace_styles.ws_search_summary_font_size_px)
                    ))
                    .opacity(0.6)
                    .text_color(workspace_styles.ws_search_summary_text_color)
                    .child(summary_text)
                    .into_any_element(),
            );

            let mut remaining_rows = self.workspace_search_render_limit;
            let mut match_index = 0usize;
            'files: for result in &self.workspace_search_results {
                if remaining_rows == 0 {
                    break;
                }
                let file_name = result
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let match_count = result.matches.len();

                children.push(
                    div()
                        .w_full()
                        .h(px(
                            self.scaled(workspace_styles.ws_search_file_header_height_px)
                        ))
                        .px(px(self.scaled(workspace_styles.ws_search_padding_px)))
                        .flex()
                        .items_center()
                        .gap(px(self.scaled(4.0)))
                        .bg(workspace_styles.ws_search_file_header_bg)
                        .border_t(px(1.0))
                        .border_b(px(1.0))
                        .border_color(self.theme.border_primary)
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(self
                                    .scaled(workspace_styles.ws_search_file_header_font_size_px)))
                                .text_color(workspace_styles.ws_search_file_header_text_color)
                                .font_weight(FontWeight::BOLD)
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .child(file_name),
                        )
                        .child(
                            div()
                                .text_size(px(self.scaled(
                                    workspace_styles.ws_search_file_header_count_font_size_px,
                                )))
                                .text_color(workspace_styles.ws_search_file_header_count_text_color)
                                .opacity(0.4)
                                .child(format!("{match_count}")),
                        )
                        .into_any_element(),
                );
                remaining_rows = remaining_rows.saturating_sub(1);
                if remaining_rows == 0 {
                    break;
                }

                let hover_bg = workspace_styles.ws_search_match_hover_bg;
                let hl_bg = workspace_styles.ws_search_match_highlight_bg;
                let hl_color = workspace_styles.ws_search_match_highlight_text_color;
                for search_match in result.matches.iter().take(50) {
                    if remaining_rows == 0 {
                        break 'files;
                    }
                    let current_match_index = match_index;
                    match_index += 1;
                    let path = result.path.clone();
                    let path_for_click = path.clone();
                    let search_match_for_click = search_match.clone();
                    let line_num = search_match.line_number;
                    let line_text = search_match.line_text.clone();
                    let start = search_match.match_start;
                    let end = search_match.match_end;
                    let is_selected =
                        self.workspace_search_selected_match == Some(current_match_index);

                    let before = &line_text[..start.min(line_text.len())];
                    let matched = &line_text[start.min(line_text.len())..end.min(line_text.len())];
                    let after = &line_text[end.min(line_text.len())..];

                    children.push(
                        div()
                            .id(gpui::SharedString::from(format!(
                                "ws-match-{}-{}-{}",
                                result.path.display(),
                                line_num,
                                start
                            )))
                            .w_full()
                            .h(px(self.scaled(workspace_styles.ws_search_match_height_px)))
                            .px(px(
                                self.scaled(workspace_styles.ws_search_match_padding_x_px)
                            ))
                            .flex()
                            .items_center()
                            .gap(px(self.scaled(4.0)))
                            .cursor(CursorStyle::Arrow)
                            .when(is_selected, |this| this.bg(result_selected_bg))
                            .hover(move |style| style.bg(hover_bg))
                            .child(
                                div()
                                    .min_w(px(self.scaled(28.0)))
                                    .text_size(px(
                                        self.scaled(workspace_styles.ws_search_match_font_size_px)
                                    ))
                                    .text_color(workspace_styles.ws_search_match_line_number_color)
                                    .opacity(0.4)
                                    .child(format!("{line_num}")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .text_size(px(
                                        self.scaled(workspace_styles.ws_search_match_font_size_px)
                                    ))
                                    .text_color(workspace_styles.ws_search_match_text_color)
                                    .flex()
                                    .child(before.to_string())
                                    .child(
                                        div()
                                            .bg(hl_bg)
                                            .text_color(hl_color)
                                            .child(matched.to_string()),
                                    )
                                    .child(after.to_string()),
                            )
                            .on_click(cx.listener(move |this, event: &gpui::ClickEvent, _, cx| {
                                let is_double_click = event.click_count() >= 2;
                                cx.stop_propagation();
                                if is_double_click {
                                    this.preview_workspace_search_match_at(
                                        current_match_index,
                                        true,
                                        true,
                                        cx,
                                    );
                                } else {
                                    this.workspace_search_selected_match =
                                        Some(current_match_index);
                                    if this.switch_to_existing_editor_tab_for_path(
                                        &path_for_click,
                                        true,
                                        cx,
                                    ) {
                                        this.reveal_workspace_search_match(
                                            path_for_click.clone(),
                                            search_match_for_click.clone(),
                                            cx,
                                        );
                                    } else {
                                        this.show_workspace_search_match(
                                            path_for_click.clone(),
                                            search_match_for_click.clone(),
                                            false,
                                            false,
                                            cx,
                                        );
                                    }
                                }
                            }))
                            .into_any_element(),
                    );
                    remaining_rows = remaining_rows.saturating_sub(1);
                }
            }
        }

        div()
            .id("workspace-search-panel")
            .flex_1()
            .overflow_y_scroll()
            .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                this.maybe_extend_workspace_search_results(cx);
            }))
            .track_scroll(&self.workspace_search_scroll_handle)
            .children(children)
            .into_any_element()
    }
}

fn search_directory(
    dir: &std::path::Path,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    use_regex: bool,
    results: &mut Vec<WorkspaceSearchResult>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let is_dir = path.is_dir();
        if should_skip_path(&path, is_dir) {
            continue;
        }
        if is_dir {
            search_directory(&path, query, case_sensitive, whole_word, use_regex, results);
        } else if let Ok(bytes) = std::fs::read(&path) {
            let file_kind = classify_file_kind_for_bytes(&path, &bytes);
            if file_kind.supports_text_search() {
                let content = String::from_utf8_lossy(&bytes).into_owned();
                let match_ranges =
                    find_search_matches(&content, query, case_sensitive, whole_word, use_regex);
                if match_ranges.is_empty() {
                    continue;
                }

                let mut matches = Vec::new();
                for range in match_ranges {
                    let byte_offset = range.start;
                    let line_start = content[..byte_offset]
                        .rfind('\n')
                        .map(|position| position + 1)
                        .unwrap_or(0);
                    let line_end = content[byte_offset..]
                        .find('\n')
                        .map(|position| byte_offset + position)
                        .unwrap_or(content.len());
                    let line_number = content[..byte_offset].matches('\n').count() + 1;
                    let line_text = content[line_start..line_end].to_string();
                    let match_start_in_line = byte_offset - line_start;
                    let match_end_in_line = match_start_in_line + (range.end - range.start);

                    matches.push(WorkspaceSearchMatch {
                        line_number,
                        line_text,
                        match_start: match_start_in_line,
                        match_end: match_end_in_line,
                        byte_offset,
                        byte_end: range.end,
                    });
                }

                results.push(WorkspaceSearchResult { path, matches });
            }
        }
    }
}
