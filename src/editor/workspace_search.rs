use std::path::PathBuf;

use gpui::{AnyElement, Context, CursorStyle, FontWeight, MouseButton, div, prelude::*, px, svg};

use super::*;

impl EditorView {
    pub(crate) fn on_find_in_files(
        &mut self,
        _: &FindInFiles,
        _: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_workspace_search(cx);
    }

    pub(crate) fn toggle_workspace_search(&mut self, cx: &mut Context<Self>) {
        self.workspace_search_visible = !self.workspace_search_visible;
        if self.workspace_search_visible {
            self.sidebar_visible = true;
            if self.workspace_search_editor.is_none() {
                let styles = self.styles.text_input.clone();
                let editor =
                    cx.new(|cx| crate::text_input::TextInput::new(styles, "搜索文件...", cx));
                cx.subscribe(
                    &editor,
                    |this, _editor, event: &crate::text_input::TextInputEvent, cx| {
                        match event {
                            crate::text_input::TextInputEvent::Changed => {
                                let query = this
                                    .workspace_search_editor
                                    .as_ref()
                                    .map(|e| e.read(cx).text().to_string())
                                    .unwrap_or_default();
                                this.workspace_search_query = query;
                                this.run_workspace_search(cx);
                            }
                            crate::text_input::TextInputEvent::Submit => {
                                // Navigate to first result
                                if let Some(first) = this.workspace_search_results.first() {
                                    if let Some(m) = first.matches.first() {
                                        let path = first.path.clone();
                                        let offset = m.byte_offset;
                                        this.open_file_at_offset(path, offset, cx);
                                    }
                                }
                            }
                            crate::text_input::TextInputEvent::Escape => {
                                this.workspace_search_visible = false;
                                cx.notify();
                            }
                            _ => {}
                        }
                    },
                )
                .detach();
                self.workspace_search_editor = Some(editor);
            }
        }
        cx.notify();
    }

    pub(crate) fn run_workspace_search(&mut self, cx: &mut Context<Self>) {
        self.workspace_search_results.clear();
        let query = self.workspace_search_query.clone();
        if query.is_empty() {
            self.workspace_search_task = None;
            cx.notify();
            return;
        }

        let root = self.sidebar_root.clone();
        if !root.exists() {
            cx.notify();
            return;
        }

        // Run search in background
        let task = cx.spawn(async move |this, cx| {
            let results = cx
                .background_executor()
                .spawn(async move {
                    let mut results = Vec::new();
                    search_directory(&root, &query, &mut results);
                    results
                })
                .await;

            this.update(cx, |this, cx| {
                this.workspace_search_results = results;
                this.workspace_search_task = None;
                cx.notify();
            })
            .ok();
        });
        self.workspace_search_task = Some(task);
    }

    pub(crate) fn open_file_at_offset(
        &mut self,
        path: PathBuf,
        byte_offset: usize,
        cx: &mut Context<Self>,
    ) {
        // Open the file (reuse existing tab or create new)
        self.open_sidebar_file(path, cx);
        // Move cursor to offset
        let offset = byte_offset.min(self.buffer.len());
        self.cursor.offset = offset;
        self.cursor.anchor = offset;
        self.scroll_to_cursor();
        cx.notify();
    }

    pub(crate) fn render_workspace_search(&self, cx: &mut Context<Self>) -> AnyElement {
        let wss = &self.styles.workspace_search;
        let ss = &self.styles.sidebar;

        let mut children: Vec<AnyElement> = Vec::new();

        // Search input
        children.push(
            div()
                .w_full()
                .px(px(self.scaled(wss.ws_search_padding_px)))
                .py(px(self.scaled(6.0)))
                .when_some(self.workspace_search_editor.clone(), |this, editor| {
                    this.child(
                        div()
                            .w_full()
                            .h(px(self.scaled(self.styles.text_input.text_input_height_px)))
                            .child(editor),
                    )
                })
                .into_any_element(),
        );

        // Results
        if self.workspace_search_query.is_empty() {
            children.push(
                div()
                    .w_full()
                    .px(px(self.scaled(wss.ws_search_padding_px)))
                    .py(px(self.scaled(8.0)))
                    .text_size(px(self.scaled(wss.ws_search_summary_font_size_px)))
                    .text_color(wss.ws_search_summary_text_color)
                    .child("输入关键字搜索工作区文件")
                    .into_any_element(),
            );
        } else if self.workspace_search_results.is_empty() && self.workspace_search_task.is_none() {
            children.push(
                div()
                    .w_full()
                    .px(px(self.scaled(wss.ws_search_padding_px)))
                    .py(px(self.scaled(8.0)))
                    .text_size(px(self.scaled(wss.ws_search_summary_font_size_px)))
                    .text_color(wss.ws_search_summary_text_color)
                    .child("无结果")
                    .into_any_element(),
            );
        } else {
            // Summary
            let total_matches: usize = self
                .workspace_search_results
                .iter()
                .map(|r| r.matches.len())
                .sum();
            let file_count = self.workspace_search_results.len();
            children.push(
                div()
                    .w_full()
                    .px(px(self.scaled(wss.ws_search_padding_px)))
                    .py(px(self.scaled(4.0)))
                    .text_size(px(self.scaled(wss.ws_search_summary_font_size_px)))
                    .text_color(wss.ws_search_summary_text_color)
                    .child(format!("{total_matches} 个结果，{file_count} 个文件"))
                    .into_any_element(),
            );

            // File groups
            for result in &self.workspace_search_results {
                let file_name = result
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let match_count = result.matches.len();

                children.push(
                    div()
                        .w_full()
                        .h(px(self.scaled(wss.ws_search_file_header_height_px)))
                        .px(px(self.scaled(wss.ws_search_padding_px)))
                        .flex()
                        .items_center()
                        .gap(px(self.scaled(4.0)))
                        .bg(wss.ws_search_file_header_bg)
                        .child(
                            div()
                                .flex_1()
                                .text_size(px(self.scaled(wss.ws_search_file_header_font_size_px)))
                                .text_color(wss.ws_search_file_header_text_color)
                                .font_weight(FontWeight::MEDIUM)
                                .overflow_hidden()
                                .text_ellipsis()
                                .whitespace_nowrap()
                                .child(file_name),
                        )
                        .child(
                            div()
                                .text_size(px(
                                    self.scaled(wss.ws_search_file_header_count_font_size_px)
                                ))
                                .text_color(wss.ws_search_file_header_count_text_color)
                                .child(format!("{match_count}")),
                        )
                        .into_any_element(),
                );

                // Match lines (limit to first 50 per file)
                let hover_bg = wss.ws_search_match_hover_bg;
                let hl_bg = wss.ws_search_match_highlight_bg;
                let hl_color = wss.ws_search_match_highlight_text_color;
                for m in result.matches.iter().take(50) {
                    let path = result.path.clone();
                    let offset = m.byte_offset;
                    let line_num = m.line_number;
                    let line_text = m.line_text.clone();
                    let ms = m.match_start;
                    let me = m.match_end;

                    let before = &line_text[..ms.min(line_text.len())];
                    let matched = &line_text[ms.min(line_text.len())..me.min(line_text.len())];
                    let after = &line_text[me.min(line_text.len())..];

                    children.push(
                        div()
                            .id(gpui::SharedString::from(format!(
                                "ws-match-{}-{}-{}",
                                result.path.display(),
                                line_num,
                                ms
                            )))
                            .w_full()
                            .h(px(self.scaled(wss.ws_search_match_height_px)))
                            .px(px(self.scaled(wss.ws_search_match_padding_x_px)))
                            .flex()
                            .items_center()
                            .gap(px(self.scaled(4.0)))
                            .cursor(CursorStyle::PointingHand)
                            .hover(move |s| s.bg(hover_bg))
                            .child(
                                div()
                                    .min_w(px(self.scaled(28.0)))
                                    .text_size(px(self.scaled(wss.ws_search_match_font_size_px)))
                                    .text_color(wss.ws_search_match_line_number_color)
                                    .child(format!("{line_num}")),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .text_size(px(self.scaled(wss.ws_search_match_font_size_px)))
                                    .text_color(wss.ws_search_match_text_color)
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
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    cx.stop_propagation();
                                    this.open_file_at_offset(path.clone(), offset, cx);
                                }),
                            )
                            .into_any_element(),
                    );
                }
            }
        }

        // Back to file tree button
        let back_hover_bg = ss.sidebar_header_hover_bg;
        let header = div()
            .w_full()
            .h(px(self.scaled(ss.sidebar_header_height_px)))
            .px(px(self.scaled(ss.sidebar_header_padding_right_px)))
            .pl(px(self.scaled(ss.sidebar_header_padding_left_px)))
            .flex()
            .items_center()
            .gap(px(self.scaled(ss.sidebar_header_gap_px)))
            .flex_none()
            .hover(move |s| s.bg(back_hover_bg))
            .child(
                div()
                    .text_size(px(self.scaled(ss.sidebar_header_font_size_px)))
                    .text_color(ss.sidebar_header_text_color)
                    .child("← 搜索"),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.workspace_search_visible = false;
                    cx.notify();
                }),
            );

        div()
            .id("workspace-search-panel")
            .flex_1()
            .overflow_y_scroll()
            .track_scroll(&self.workspace_search_scroll_handle)
            .child(header)
            .children(children)
            .into_any_element()
    }
}

// ── Background search helper ────────────────────────────────────────

fn search_directory(dir: &std::path::Path, query: &str, results: &mut Vec<WorkspaceSearchResult>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    let needle = query.to_lowercase();

    for entry in entries.flatten() {
        let path = entry.path();
        if path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.'))
        {
            continue;
        }
        if path.is_dir() {
            search_directory(&path, query, results);
        } else if is_markdown_file(&path) {
            if let Ok(content) = std::fs::read_to_string(&path) {
                let lower = content.to_lowercase();
                let mut matches = Vec::new();
                let mut search_start = 0;

                while let Some(pos) = lower[search_start..].find(&needle) {
                    let byte_offset = search_start + pos;
                    // Find line number and line text
                    let line_start = content[..byte_offset]
                        .rfind('\n')
                        .map(|p| p + 1)
                        .unwrap_or(0);
                    let line_end = content[byte_offset..]
                        .find('\n')
                        .map(|p| byte_offset + p)
                        .unwrap_or(content.len());
                    let line_number = content[..byte_offset].matches('\n').count() + 1;
                    let line_text = content[line_start..line_end].to_string();
                    let match_start_in_line = byte_offset - line_start;
                    let match_end_in_line = match_start_in_line + needle.len();

                    matches.push(WorkspaceSearchMatch {
                        line_number,
                        line_text,
                        match_start: match_start_in_line,
                        match_end: match_end_in_line,
                        byte_offset,
                    });

                    search_start = byte_offset + needle.len();
                }

                if !matches.is_empty() {
                    results.push(WorkspaceSearchResult { path, matches });
                }
            }
        }
    }
}
