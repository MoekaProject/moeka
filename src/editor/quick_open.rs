use std::path::{Path, PathBuf};
use std::time::SystemTime;

use gpui::{AnyElement, Context, CursorStyle, MouseButton, Window, div, prelude::*, px};
use pinyin::ToPinyin;

use super::*;

impl EditorView {
    pub(crate) fn on_open_file_dialog(
        &mut self,
        _: &OpenFileDialog,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let title = self.t(crate::i18n::TextKey::OpenFile).to_string();
        cx.spawn(async move |this, cx| {
            let maybe_path = cx
                .background_executor()
                .spawn(async move { helpers::pick_markdown_file_blocking(&title) })
                .await;
            if let Some(path) = maybe_path {
                let _ = this.update(cx, |editor, cx| {
                    editor.open_path_in_permanent_tab(path, true, cx);
                });
            }
        })
        .detach();
    }

    fn quick_open_text_input_styles(&self) -> crate::style::TextInputStyles {
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

    pub(crate) fn on_quick_open(
        &mut self,
        _: &QuickOpen,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_quick_open(window, cx);
    }

    pub(crate) fn toggle_quick_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.quick_open_visible = !self.quick_open_visible;
        if !self.quick_open_visible {
            self.close_quick_open(window, cx);
            return;
        }

        if self.quick_open_editor.is_none() {
            let styles = self.quick_open_text_input_styles();
            let placeholder = self.t(crate::i18n::TextKey::QuickOpenPlaceholder).to_string();
            let editor =
                cx.new(move |cx| crate::text_input::TextInput::new(styles, &placeholder, cx));
            cx.subscribe(
                &editor,
                |this, _editor, event: &crate::text_input::TextInputEvent, cx| match event {
                    crate::text_input::TextInputEvent::Changed => {
                        let query = this
                            .quick_open_editor
                            .as_ref()
                            .map(|e| e.read(cx).text().to_string())
                            .unwrap_or_default();
                        this.quick_open_query = query;
                        this.run_quick_open_search(cx);
                    }
                    crate::text_input::TextInputEvent::Submit
                    | crate::text_input::TextInputEvent::SubmitBackward => {
                        this.confirm_quick_open(cx);
                    }
                    crate::text_input::TextInputEvent::Escape => {}
                    crate::text_input::TextInputEvent::MoveUp => {
                        this.move_quick_open_selection(-1, cx);
                    }
                    crate::text_input::TextInputEvent::MoveDown => {
                        this.move_quick_open_selection(1, cx);
                    }
                },
            )
            .detach();
            self.quick_open_editor = Some(editor);
        }

        self.quick_open_selected_index = 0;
        self.quick_open_render_limit = 100;
        self.run_quick_open_search(cx);

        if let Some(editor) = &self.quick_open_editor {
            editor.update(cx, |ti, cx| {
                ti.styles = self.quick_open_text_input_styles();
                ti.set_text(&self.quick_open_query, cx);
                ti.select_all(cx);
            });
            editor.focus_handle(cx).focus(window);
        }

        cx.notify();
    }

    pub(crate) fn close_quick_open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.quick_open_visible = false;
        self.quick_open_task = None;
        window.focus(&self.focus_handle(cx));
        cx.notify();
    }

    pub(crate) fn move_quick_open_selection(&mut self, delta: isize, cx: &mut Context<Self>) {
        if self.quick_open_results.is_empty() {
            self.quick_open_selected_index = 0;
            cx.notify();
            return;
        }

        let len = self.quick_open_results.len() as isize;
        let current = self.quick_open_selected_index as isize;
        self.quick_open_selected_index = (current + delta).clamp(0, len - 1) as usize;
        if self.quick_open_selected_index + 1 > self.quick_open_render_limit {
            self.quick_open_render_limit =
                (self.quick_open_selected_index + 100).min(self.quick_open_results.len());
        }
        self.quick_open_scroll_handle
            .scroll_to_item(self.quick_open_selected_index);
        cx.notify();
    }

    fn maybe_extend_quick_open_results(&mut self, cx: &mut Context<Self>) {
        let total = self.quick_open_results.len();
        if total <= self.quick_open_render_limit {
            return;
        }

        let bottom_item = self.quick_open_scroll_handle.bottom_item();
        if bottom_item + 12 >= self.quick_open_render_limit.saturating_sub(1) {
            self.quick_open_render_limit = (self.quick_open_render_limit + 100).min(total);
            cx.notify();
        }
    }

    pub(crate) fn confirm_quick_open(&mut self, cx: &mut Context<Self>) {
        let Some(result) = self
            .quick_open_results
            .get(self.quick_open_selected_index)
            .cloned()
        else {
            self.quick_open_visible = false;
            self.quick_open_task = None;
            cx.notify();
            return;
        };
        self.quick_open_visible = false;
        self.quick_open_task = None;
        cx.notify();
        self.open_sidebar_file(result.path, cx);
    }

    pub(crate) fn run_quick_open_search(&mut self, cx: &mut Context<Self>) {
        let root = self.sidebar_root.clone();
        if !root.exists() {
            self.quick_open_results.clear();
            self.quick_open_selected_index = 0;
            self.quick_open_task = None;
            self.quick_open_render_limit = 100;
            cx.notify();
            return;
        }

        let query = self.quick_open_query.clone();
        let task = cx.spawn(async move |this, cx| {
            let results = cx
                .background_executor()
                .spawn(async move { collect_quick_open_results(&root, &query) })
                .await;

            this.update(cx, |this, cx| {
                this.quick_open_results = results;
                this.quick_open_selected_index = 0;
                this.quick_open_task = None;
                this.quick_open_render_limit = 100;
                this.quick_open_scroll_handle.scroll_to_item(0);
                cx.notify();
            })
            .ok();
        });
        self.quick_open_task = Some(task);
    }

    pub(crate) fn render_quick_open_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let styles = &self.styles.quick_open;
        let bg = styles.quick_open_panel_bg;
        let border = styles.quick_open_panel_border_color;
        let hover_bg = styles.quick_open_result_hover_bg;
        let selected_bg = styles.quick_open_result_selected_bg;
        let text_primary = styles.quick_open_result_text_color;
        let text_secondary = styles.quick_open_result_secondary_text_color;
        let match_color = styles.quick_open_result_match_text_color;
        let backdrop = styles.quick_open_backdrop_bg;

        let panel_w = self.scaled(styles.quick_open_panel_width_px);
        let sidebar_w = self.sidebar_width();
        let right_panel_w = self.right_panel_width();
        let editor_w = (self.last_window_width - sidebar_w - right_panel_w).max(panel_w);
        let panel_left = sidebar_w + (editor_w - panel_w) / 2.0;
        let panel_top =
            (self.last_window_height / 5.0).max(self.scaled(styles.quick_open_panel_top_offset_px));
        let row_h = self.scaled(styles.quick_open_result_row_height_px);
        let path_font = self.scaled(styles.quick_open_result_path_font_size_px);
        let name_font = self.scaled(styles.quick_open_result_name_font_size_px);
        let max_results_h = self.scaled(styles.quick_open_results_max_height_px);
        let workspace_name = self.sidebar_root_name();
        let workspace_name_char_len = workspace_name.chars().count();

        let mut result_children: Vec<AnyElement> = Vec::new();
        if self.quick_open_results.is_empty() {
            let message = if self.quick_open_query.is_empty() {
                self.t(crate::i18n::TextKey::QuickOpenEmpty)
            } else if self.quick_open_task.is_some() {
                self.t(crate::i18n::TextKey::Searching)
            } else {
                self.t(crate::i18n::TextKey::QuickOpenNoResults)
            };
            result_children.push(
                div()
                    .w_full()
                    .h(px(self.scaled(styles.quick_open_empty_height_px)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(px(self.scaled(styles.quick_open_empty_font_size_px)))
                    .text_color(styles.quick_open_empty_text_color)
                    .child(message)
                    .into_any_element(),
            );
        } else {
            for (index, result) in self
                .quick_open_results
                .iter()
                .take(self.quick_open_render_limit)
                .enumerate()
            {
                let is_selected = index == self.quick_open_selected_index;
                let path = result.path.clone();
                let relative_path = path
                    .strip_prefix(&self.sidebar_root)
                    .unwrap_or(path.as_path())
                    .display()
                    .to_string();
                let relative_directory = Path::new(&relative_path)
                    .parent()
                    .filter(|parent| !parent.as_os_str().is_empty())
                    .map(|parent| parent.display().to_string());
                let file_name = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| relative_path.clone());
                let (directory_path, path_match_indices) =
                    if let Some(relative_directory) = relative_directory {
                        let prefix_offset = workspace_name_char_len + 1;
                        let relative_directory_char_len = relative_directory.chars().count();
                        (
                            format!("{workspace_name}/{relative_directory}"),
                            result
                                .path_match_indices
                                .iter()
                                .copied()
                                .filter(|index| *index < relative_directory_char_len)
                                .map(|index| index + prefix_offset)
                                .collect::<Vec<_>>(),
                        )
                    } else {
                        (workspace_name.clone(), Vec::new())
                    };
                let file_name_el = render_highlighted_text(
                    &file_name,
                    &result.file_name_match_indices,
                    text_primary,
                    match_color,
                    name_font,
                );
                let path_el = render_highlighted_text(
                    &directory_path,
                    &path_match_indices,
                    text_secondary,
                    match_color,
                    path_font,
                );

                result_children.push(
                    div()
                        .id(SharedString::from(format!("quick-open-{}", index)))
                        .w_full()
                        .h(px(row_h))
                        .px(px(self.scaled(styles.quick_open_result_padding_x_px)))
                        .flex()
                        .items_center()
                        .cursor(CursorStyle::Arrow)
                        .when(is_selected, |this| this.bg(selected_bg))
                        .hover(move |s| s.bg(hover_bg))
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(0.0))
                                .flex()
                                .flex_col()
                                .gap(px(self.scaled(styles.quick_open_result_gap_y_px)))
                                .child(file_name_el)
                                .child(path_el),
                        )
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                if let Some(pos) = this
                                    .quick_open_results
                                    .iter()
                                    .position(|candidate| candidate.path == path)
                                {
                                    this.quick_open_selected_index = pos;
                                }
                                this.confirm_quick_open(cx);
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
                            this.close_quick_open(window, cx);
                        }),
                    ),
            )
            .child(
                div()
                    .id("quick-open-overlay")
                    .absolute()
                    .top(px(panel_top))
                    .left(px(panel_left.max(0.0)))
                    .w(px(panel_w))
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
                            .when_some(self.quick_open_editor.clone(), |this, editor| {
                                this.child(editor)
                            }),
                    )
                    .child(
                        div()
                            .id("quick-open-results")
                            .w_full()
                            .flex_1()
                            .max_h(px(max_results_h))
                            .overflow_y_scroll()
                            .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                                this.maybe_extend_quick_open_results(cx);
                            }))
                            .track_scroll(&self.quick_open_scroll_handle)
                            .children(result_children),
                    ),
            )
            .into_any_element()
    }
}

fn collect_quick_open_results(root: &Path, query: &str) -> Vec<QuickOpenResult> {
    let mut paths = Vec::new();
    collect_openable_files(root, &mut paths);

    let mut results = Vec::new();
    for path in paths {
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        let relative_path = path
            .strip_prefix(root)
            .unwrap_or(path.as_path())
            .display()
            .to_string();

        let file_match = fuzzy_match(&file_name, query);
        let path_match = fuzzy_match(&relative_path, query);
        if query.is_empty() {
            results.push(QuickOpenResult {
                path,
                score: 0,
                file_name_match_indices: Vec::new(),
                path_match_indices: Vec::new(),
            });
            continue;
        }

        let (file_score, file_matches) = file_match.unwrap_or((0, Vec::new()));
        let (path_score, path_matches) = path_match.unwrap_or((0, Vec::new()));
        if file_score == 0 && path_score == 0 {
            continue;
        }

        results.push(QuickOpenResult {
            path,
            score: file_score * 100 + path_score,
            file_name_match_indices: file_matches,
            path_match_indices: path_matches,
        });
    }

    results.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| {
                a.path
                    .components()
                    .count()
                    .cmp(&b.path.components().count())
            })
            .then_with(|| a.path.cmp(&b.path))
    });
    results
}

pub(crate) fn collect_wiki_link_recent_files(
    root: &Path,
    limit: usize,
) -> Vec<WorkspaceFileCandidate> {
    let mut ranked = Vec::new();
    collect_recent_openable_files(root, root, limit.max(1), &mut ranked);
    ranked.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then_with(|| left.1.relative_path.cmp(&right.1.relative_path))
    });
    ranked
        .into_iter()
        .map(|(_, candidate)| candidate)
        .filter(|candidate| is_markdown_file(&candidate.path))
        .take(limit)
        .collect()
}

pub(crate) fn collect_wiki_link_search_results(
    root: &Path,
    query: &str,
    limit: usize,
) -> Vec<QuickOpenResult> {
    let mut results = collect_quick_open_results(root, query);
    results.retain(|result| is_markdown_file(&result.path));
    results.truncate(limit);
    results
}

fn collect_recent_openable_files(
    root: &Path,
    dir: &Path,
    limit: usize,
    ranked: &mut Vec<(SystemTime, WorkspaceFileCandidate)>,
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
            collect_recent_openable_files(root, &path, limit, ranked);
            continue;
        }
        if !is_markdown_file(&path) {
            continue;
        }

        let fallback = SystemTime::UNIX_EPOCH;
        let created = entry
            .metadata()
            .and_then(|meta| meta.created().or_else(|_| meta.modified()))
            .unwrap_or(fallback);
        let relative_path = path
            .strip_prefix(root)
            .unwrap_or(path.as_path())
            .to_string_lossy()
            .replace('\\', "/");
        let file_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| relative_path.clone());
        ranked.push((
            created,
            WorkspaceFileCandidate {
                path,
                relative_path,
                file_name,
            },
        ));
        ranked.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| left.1.relative_path.cmp(&right.1.relative_path))
        });
        if ranked.len() > limit {
            ranked.truncate(limit);
        }
    }
}

fn collect_openable_files(dir: &Path, results: &mut Vec<PathBuf>) {
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
            collect_openable_files(&path, results);
        } else {
            results.push(path);
        }
    }
}

fn fuzzy_match(text: &str, query: &str) -> Option<(i64, Vec<usize>)> {
    let direct_match = fuzzy_match_direct(text, query);
    let pinyin_match = fuzzy_match_via_pinyin(text, query);

    match (direct_match, pinyin_match) {
        (Some(direct), Some(pinyin)) => {
            if direct.0 >= pinyin.0 {
                Some(direct)
            } else {
                Some(pinyin)
            }
        }
        (Some(direct), None) => Some(direct),
        (None, Some(pinyin)) => Some(pinyin),
        (None, None) => None,
    }
}

fn fuzzy_match_direct(text: &str, query: &str) -> Option<(i64, Vec<usize>)> {
    if query.is_empty() {
        return Some((0, Vec::new()));
    }

    let text_chars: Vec<char> = text.chars().collect();
    let query_chars: Vec<char> = query.chars().collect();
    let source_indices: Vec<usize> = (0..text_chars.len()).collect();
    fuzzy_match_chars(&text_chars, &query_chars, &text_chars, &source_indices)
}

fn fuzzy_match_via_pinyin(text: &str, query: &str) -> Option<(i64, Vec<usize>)> {
    if !should_try_pinyin_match(text, query) {
        return None;
    }

    let normalized_query: Vec<char> = query
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .map(lowercase_char)
        .collect();
    if normalized_query.is_empty() {
        return None;
    }

    let text_chars: Vec<char> = text.chars().collect();
    let (normalized_text, source_indices) = build_pinyin_index(&text_chars);
    let (score, matched_indices) = fuzzy_match_chars(
        &normalized_text,
        &normalized_query,
        &text_chars,
        &source_indices,
    )?;
    Some((score - 25, matched_indices))
}

fn should_try_pinyin_match(text: &str, query: &str) -> bool {
    !query.is_empty()
        && query.chars().all(|ch| {
            ch.is_ascii_alphanumeric() || matches!(ch, '/' | '\\' | '-' | '_' | '.' | ' ')
        })
        && text.chars().any(is_cjk_character)
}

fn build_pinyin_index(text_chars: &[char]) -> (Vec<char>, Vec<usize>) {
    let mut normalized_text = Vec::new();
    let mut source_indices = Vec::new();

    for (source_index, ch) in text_chars.iter().copied().enumerate() {
        if let Some(pinyin) = ch.to_pinyin() {
            for normalized in pinyin.plain().chars().map(lowercase_char) {
                normalized_text.push(normalized);
                source_indices.push(source_index);
            }
            continue;
        }

        normalized_text.push(lowercase_char(ch));
        source_indices.push(source_index);
    }

    (normalized_text, source_indices)
}

fn fuzzy_match_chars(
    text_chars: &[char],
    query_chars: &[char],
    original_chars: &[char],
    source_indices: &[usize],
) -> Option<(i64, Vec<usize>)> {
    if query_chars.is_empty() {
        return Some((0, Vec::new()));
    }

    let mut query_index = 0usize;
    let mut matched_indices = Vec::new();
    let mut score = 0i64;
    let mut last_match: Option<usize> = None;

    for (index, ch) in text_chars.iter().enumerate() {
        if query_index >= query_chars.len() {
            break;
        }
        if chars_equal(*ch, query_chars[query_index]) {
            let source_index = source_indices.get(index).copied().unwrap_or(index);
            if matched_indices.last().copied() != Some(source_index) {
                matched_indices.push(source_index);
            }
            score += 10;
            if source_index == 0 {
                score += 30;
            }
            if matches!(last_match, Some(prev) if index == prev + 1) {
                score += 20;
            }
            if source_index > 0 {
                let prev = original_chars[source_index - 1];
                if is_match_boundary(prev) {
                    score += 15;
                }
            }
            last_match = Some(index);
            query_index += 1;
        }
    }

    if query_index == query_chars.len() {
        score += (200i64 - text_chars.len() as i64).max(0);
        Some((score, matched_indices))
    } else {
        None
    }
}

fn chars_equal(left: char, right: char) -> bool {
    if left.is_ascii() && right.is_ascii() {
        left.eq_ignore_ascii_case(&right)
    } else {
        left == right
    }
}

fn lowercase_char(ch: char) -> char {
    if ch.is_ascii() {
        ch.to_ascii_lowercase()
    } else {
        ch
    }
}

fn is_match_boundary(ch: char) -> bool {
    matches!(ch, '/' | '\\' | '-' | '_' | ' ' | '.')
}

fn is_cjk_character(ch: char) -> bool {
    let code = ch as u32;
    (0x3400..=0x4DBF).contains(&code)
        || (0x4E00..=0x9FFF).contains(&code)
        || (0xF900..=0xFAFF).contains(&code)
        || (0x20000..=0x2FA1F).contains(&code)
}

pub(crate) fn render_highlighted_text(
    text: &str,
    highlight_indices: &[usize],
    normal_color: gpui::Hsla,
    highlight_color: gpui::Hsla,
    font_size: f32,
) -> AnyElement {
    let chars: Vec<char> = text.chars().collect();
    if chars.is_empty() {
        return div().into_any_element();
    }

    let mut sorted_indices = highlight_indices.to_vec();
    sorted_indices.sort_unstable();
    sorted_indices.dedup();

    let mut parts: Vec<(String, bool)> = Vec::new();
    let mut current = String::new();
    let mut highlight_cursor = 0usize;
    let mut current_highlight = sorted_indices.first().copied() == Some(0);
    for (index, ch) in chars.iter().enumerate() {
        while highlight_cursor < sorted_indices.len() && sorted_indices[highlight_cursor] < index {
            highlight_cursor += 1;
        }
        let is_highlight = sorted_indices.get(highlight_cursor).copied() == Some(index);
        if index > 0 && is_highlight != current_highlight {
            parts.push((current, current_highlight));
            current = String::new();
            current_highlight = is_highlight;
        }
        current.push(*ch);
    }
    if !current.is_empty() {
        parts.push((current, current_highlight));
    }

    let mut row = div().flex().items_center().gap(px(0.0));
    for (segment, is_highlight) in parts {
        row = row.child(
            div()
                .text_size(px(font_size))
                .text_color(if is_highlight {
                    highlight_color
                } else {
                    normal_color
                })
                .child(segment),
        );
    }
    row.into_any_element()
}
