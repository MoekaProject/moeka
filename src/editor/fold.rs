use gpui::Context;

use super::*;

impl EditorView {
    pub(crate) fn fold_range_for_line(&self, line: usize) -> Option<std::ops::Range<usize>> {
        let decoration = self.decorations.get(line)?;
        let line_count = self.buffer.line_count();
        match &decoration.block_type {
            BlockType::Heading(level) => {
                let lv = *level;
                let start = line + 1;
                let end = (start..line_count)
                    .find(|&l| {
                        matches!(
                            self.decorations.get(l).map(|d| &d.block_type),
                            Some(BlockType::Heading(h)) if *h <= lv
                        )
                    })
                    .unwrap_or(line_count);
                if end > start { Some(start..end) } else { None }
            }
            BlockType::CodeBlockFence => {
                if line > 0
                    && matches!(
                        self.decorations.get(line - 1).map(|d| &d.block_type),
                        Some(BlockType::CodeBlock | BlockType::CodeBlockFence)
                    )
                {
                    return None;
                }
                let start = line + 1;
                let end = (start..line_count)
                    .find(|&l| {
                        matches!(
                            self.decorations.get(l).map(|d| &d.block_type),
                            Some(BlockType::CodeBlockFence)
                        )
                    })
                    .map(|l| l + 1)
                    .unwrap_or(line_count);
                if end > start { Some(start..end) } else { None }
            }
            BlockType::UnorderedList | BlockType::OrderedList | BlockType::TaskList(_) => {
                let content = self.buffer.line_content(line);
                let indent = content.len() - content.trim_start_matches([' ', '\t']).len();
                if line + 1 >= line_count {
                    return None;
                }
                let next_content = self.buffer.line_content(line + 1);
                let next_indent =
                    next_content.len() - next_content.trim_start_matches([' ', '\t']).len();
                if next_indent <= indent {
                    return None;
                }
                let start = line + 1;
                let end = (start..line_count)
                    .find(|&l| {
                        let lc = self.buffer.line_content(l);
                        let li = lc.len() - lc.trim_start_matches([' ', '\t']).len();
                        !lc.trim().is_empty() && li <= indent
                    })
                    .unwrap_or(line_count);
                if end > start { Some(start..end) } else { None }
            }
            _ => {
                if self.active_tab_is_non_markdown_text() {
                    self.generic_code_fold_range_for_line(line)
                } else {
                    None
                }
            }
        }
    }

    fn generic_code_fold_range_for_line(&self, line: usize) -> Option<std::ops::Range<usize>> {
        let line_count = self.buffer.line_count();
        if line + 1 >= line_count {
            return None;
        }

        let content = self.buffer.line_content(line);
        let trimmed = content.trim_end();
        if trimmed.trim().is_empty() {
            return None;
        }

        if let Some(range) = self.brace_fold_range_for_line(line, trimmed) {
            return Some(range);
        }

        let indent = content.len() - content.trim_start_matches([' ', '\t']).len();
        let next_line = (line + 1..line_count).find(|&idx| {
            let next = self.buffer.line_content(idx);
            !next.trim().is_empty()
        })?;
        let next_content = self.buffer.line_content(next_line);
        let next_indent = next_content.len() - next_content.trim_start_matches([' ', '\t']).len();
        if next_indent <= indent {
            return None;
        }

        let end = (next_line..line_count)
            .find(|&idx| {
                let candidate = self.buffer.line_content(idx);
                if candidate.trim().is_empty() {
                    return false;
                }
                let candidate_indent =
                    candidate.len() - candidate.trim_start_matches([' ', '\t']).len();
                candidate_indent <= indent
            })
            .unwrap_or(line_count);

        (end > line + 1).then_some(line + 1..end)
    }

    fn brace_fold_range_for_line(
        &self,
        line: usize,
        trimmed: &str,
    ) -> Option<std::ops::Range<usize>> {
        let (open, close) = if trimmed.ends_with('{') {
            ('{', '}')
        } else if trimmed.ends_with('[') {
            ('[', ']')
        } else if trimmed.ends_with('(') {
            ('(', ')')
        } else {
            return None;
        };

        let mut depth = 1usize;
        let line_count = self.buffer.line_count();
        for candidate_line in line + 1..line_count {
            let candidate = self.buffer.line_content(candidate_line);
            for ch in candidate.chars() {
                if ch == open {
                    depth += 1;
                } else if ch == close {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return (candidate_line + 1 > line + 1)
                            .then_some(line + 1..candidate_line + 1);
                    }
                }
            }
        }

        None
    }

    pub(crate) fn toggle_fold(&mut self, line: usize, cx: &mut Context<Self>) {
        if self.folded_sections.contains(&line) {
            self.folded_sections.remove(&line);
        } else if let Some(range) = self.fold_range_for_line(line) {
            self.folded_sections.insert(line);
            let cursor_line = self.cursor_line();
            if range.contains(&cursor_line) {
                let new_offset = self.buffer.line_end(line);
                self.cursor.move_to(new_offset, &self.buffer);
            }
        }
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn toggle_image_fold(&mut self, line: usize, cx: &mut Context<Self>) {
        if !self.folded_image_previews.remove(&line) {
            self.folded_image_previews.insert(line);
        }
        cx.notify();
    }

    pub(crate) fn pick_image_for_line(&self, line: usize, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let maybe_path = cx
                .background_executor()
                .spawn(async move { helpers::pick_image_file_blocking() })
                .await;
            if let Some(path) = maybe_path {
                this.update(cx, |editor, cx| {
                    editor.replace_image_url_on_line(line, &path, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    pub(crate) fn replace_image_url_on_line(
        &mut self,
        line: usize,
        new_path: &PathBuf,
        cx: &mut Context<Self>,
    ) {
        let line_content = self.buffer.line_content(line).to_string();
        let line_start = self.buffer.line_start(line);
        if let Some(bracket_pos) = line_content.find("](") {
            let url_start = bracket_pos + 2;
            if let Some(close_pos) = line_content[url_start..].find(')') {
                let url_end = url_start + close_pos;
                let old_url = line_content[url_start..url_end].to_string();
                let new_url = new_path.to_string_lossy().to_string();
                self.checkpoint();
                self.buffer
                    .replace(line_start + url_start..line_start + url_end, &new_url);
                self.image_cache.remove(&old_url);
                self.reparse();
                self.pause_blinking(cx);
            }
        }
    }
}
