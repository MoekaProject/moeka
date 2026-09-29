use std::ops::Range;

use gpui::Context;

use super::*;

const CODE_FOLD_TAB_WIDTH: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RegionMarker {
    Start,
    End,
}

fn leading_indent_columns(line: &str) -> usize {
    let mut columns = 0usize;
    for ch in line.chars() {
        match ch {
            ' ' => columns += 1,
            '\t' => columns += CODE_FOLD_TAB_WIDTH - (columns % CODE_FOLD_TAB_WIDTH),
            _ => break,
        }
    }
    columns
}

fn marker_keyword_matches(text: &str, keyword: &str) -> bool {
    let Some(rest) = text.strip_prefix(keyword) else {
        return false;
    };
    match rest.chars().next() {
        None => true,
        Some(ch) => ch.is_whitespace() || matches!(ch, ':' | '-' | '/' | '*'),
    }
}

fn classify_region_marker(trimmed: &str) -> Option<RegionMarker> {
    let normalized = trimmed.trim_start_matches(|ch: char| {
        ch.is_whitespace() || matches!(ch, '/' | '#' | '*' | ';' | '<' | '!' | '-')
    });
    let lower = normalized.to_ascii_lowercase();
    if marker_keyword_matches(&lower, "endregion") || marker_keyword_matches(&lower, "#endregion") {
        Some(RegionMarker::End)
    } else if marker_keyword_matches(&lower, "region") || marker_keyword_matches(&lower, "#region")
    {
        Some(RegionMarker::Start)
    } else {
        None
    }
}

fn region_fold_range_for_line<'a, F>(
    line_count: usize,
    line: usize,
    line_content: F,
) -> Option<Range<usize>>
where
    F: Fn(usize) -> &'a str,
{
    if line + 1 >= line_count {
        return None;
    }
    if classify_region_marker(line_content(line).trim()) != Some(RegionMarker::Start) {
        return None;
    }

    let mut depth = 1usize;
    for candidate_line in line + 1..line_count {
        match classify_region_marker(line_content(candidate_line).trim()) {
            Some(RegionMarker::Start) => depth += 1,
            Some(RegionMarker::End) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return (candidate_line + 1 > line + 1).then_some(line + 1..candidate_line + 1);
                }
            }
            None => {}
        }
    }

    None
}

fn markup_tag_delta(line: &str, tag: &str) -> isize {
    let mut delta = 0isize;
    let mut index = 0usize;

    while let Some(relative) = line[index..].find('<') {
        let start = index + relative;
        let rest = &line[start + 1..];
        if rest.starts_with("!--") {
            if let Some(comment_end) = line[start + 4..].find("-->") {
                index = start + 4 + comment_end + 3;
            } else {
                break;
            }
            continue;
        }
        if rest.starts_with('!') || rest.starts_with('?') {
            index = start + 2;
            continue;
        }

        let closing = rest.starts_with('/');
        let name_start = start + 1 + usize::from(closing);
        let name_rest = &line[name_start..];
        let name_len = name_rest
            .find(|ch: char| !ch.is_ascii_alphanumeric() && !matches!(ch, ':' | '_' | '-'))
            .unwrap_or(name_rest.len());
        if name_len == 0 {
            index = name_start + 1;
            continue;
        }

        let name = &name_rest[..name_len];
        let tag_matches = name.eq_ignore_ascii_case(tag);
        let token_end = line[name_start + name_len..]
            .find('>')
            .map(|offset| name_start + name_len + offset + 1)
            .unwrap_or(line.len());
        let token = &line[start..token_end];
        let self_closing = !closing && token.trim_end().ends_with("/>");

        if tag_matches {
            if closing {
                delta -= 1;
            } else if !self_closing {
                delta += 1;
            }
        }

        index = token_end.max(start + 1);
    }

    delta
}

fn opening_markup_tag_name(trimmed: &str) -> Option<String> {
    let trimmed = trimmed.trim_start();
    if !trimmed.starts_with('<')
        || trimmed.starts_with("</")
        || trimmed.starts_with("<!--")
        || trimmed.starts_with("<!")
        || trimmed.starts_with("<?")
    {
        return None;
    }

    let rest = &trimmed[1..];
    let name_len = rest
        .find(|ch: char| !ch.is_ascii_alphanumeric() && !matches!(ch, ':' | '_' | '-'))
        .unwrap_or(rest.len());
    if name_len == 0 {
        return None;
    }

    let tag = &rest[..name_len];
    (markup_tag_delta(trimmed, tag) > 0).then(|| tag.to_ascii_lowercase())
}

fn markup_fold_range_for_line<'a, F>(
    line_count: usize,
    line: usize,
    line_content: F,
) -> Option<Range<usize>>
where
    F: Fn(usize) -> &'a str,
{
    if line + 1 >= line_count {
        return None;
    }

    let tag = opening_markup_tag_name(line_content(line).trim_end())?;
    let mut depth = markup_tag_delta(line_content(line), &tag);
    for candidate_line in line + 1..line_count {
        depth += markup_tag_delta(line_content(candidate_line), &tag);
        if depth <= 0 {
            return (candidate_line + 1 > line + 1).then_some(line + 1..candidate_line + 1);
        }
    }

    None
}

fn brace_fold_range_for_line_impl<'a, F>(
    line_count: usize,
    line: usize,
    trimmed: &str,
    line_content: F,
) -> Option<Range<usize>>
where
    F: Fn(usize) -> &'a str,
{
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
    for candidate_line in line + 1..line_count {
        let candidate = line_content(candidate_line);
        for ch in candidate.chars() {
            if ch == open {
                depth += 1;
            } else if ch == close {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return (candidate_line + 1 > line + 1).then_some(line + 1..candidate_line + 1);
                }
            }
        }
    }

    None
}

fn indent_fold_range_for_line<'a, F>(
    line_count: usize,
    line: usize,
    line_content: F,
) -> Option<Range<usize>>
where
    F: Fn(usize) -> &'a str,
{
    if line + 1 >= line_count {
        return None;
    }

    let content = line_content(line);
    let indent = leading_indent_columns(content);
    let next_line = (line + 1..line_count).find(|&idx| !line_content(idx).trim().is_empty())?;
    let next_indent = leading_indent_columns(line_content(next_line));
    if next_indent <= indent {
        return None;
    }

    let end = (next_line..line_count)
        .find(|&idx| {
            let candidate = line_content(idx);
            !candidate.trim().is_empty() && leading_indent_columns(candidate) <= indent
        })
        .unwrap_or(line_count);

    (end > line + 1).then_some(line + 1..end)
}

fn generic_code_fold_range_for_line_impl<'a, F>(
    line_count: usize,
    line: usize,
    line_content: F,
) -> Option<Range<usize>>
where
    F: Fn(usize) -> &'a str,
    F: std::marker::Copy,
{
    if line + 1 >= line_count {
        return None;
    }

    let content = line_content(line);
    let trimmed = content.trim_end();
    if trimmed.trim().is_empty() {
        return None;
    }

    region_fold_range_for_line(line_count, line, line_content)
        .or_else(|| brace_fold_range_for_line_impl(line_count, line, trimmed, line_content))
        .or_else(|| markup_fold_range_for_line(line_count, line, line_content))
        .or_else(|| indent_fold_range_for_line(line_count, line, line_content))
}

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
            BlockType::Blockquote => {
                let start = line + 1;
                let end = (start..line_count)
                    .find(|&l| {
                        !matches!(
                            self.decorations.get(l).map(|d| &d.block_type),
                            Some(BlockType::Blockquote)
                        )
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
        generic_code_fold_range_for_line_impl(self.buffer.line_count(), line, |idx| {
            self.buffer.line_content(idx)
        })
    }

    pub(crate) fn toggle_fold(&mut self, line: usize, cx: &mut Context<Self>) {
        let mut cursor_moved = false;
        if self.folded_sections.contains(&line) {
            self.folded_sections.remove(&line);
        } else if let Some(range) = self.fold_range_for_line(line) {
            self.folded_sections.insert(line);
            let cursor_line = self.cursor_line();
            if range.contains(&cursor_line) {
                let new_offset = self.buffer.line_end(line);
                self.cursor.move_to(new_offset, &self.buffer);
                cursor_moved = true;
            }
        }
        self.clear_line_display_info();
        if cursor_moved {
            self.scroll_to_cursor();
        } else {
            cx.notify();
        }
        self.pause_blinking(cx);
    }

    pub(crate) fn toggle_generated_preview_fold(&mut self, line: usize, cx: &mut Context<Self>) {
        let Some(preview) = self
            .decorations
            .get(line)
            .and_then(|decoration| decoration.block_preview.as_ref())
            .cloned()
            .filter(|preview| preview.is_anchor)
        else {
            self.toggle_fold(line, cx);
            return;
        };

        let selection_ranges = std::iter::once(self.cursor.selection_range())
            .chain(
                self.extra_cursors
                    .iter()
                    .map(|cursor| cursor.selection_range()),
            )
            .collect::<Vec<_>>();
        let preview_is_active =
            helpers::block_preview_is_active(&preview, self.cursor_line(), &selection_ranges);

        if self
            .preview_overlay_state()
            .is_some_and(|overlay| overlay.line_idx == preview.start_line)
        {
            self.close_preview_overlay();
        }

        if self.folded_sections.contains(&preview.start_line) {
            self.folded_sections.remove(&preview.start_line);
        } else if let Some(range) = self.fold_range_for_line(preview.start_line) {
            self.folded_sections.insert(preview.start_line);
            if range.contains(&self.cursor_line()) || preview_is_active {
                let new_offset = self.buffer.line_end(preview.start_line);
                self.cursor.move_to(new_offset, &self.buffer);
                self.scroll_to_cursor();
            }
        }

        self.clear_line_display_info();
        cx.notify();
        self.pause_blinking(cx);
    }

    pub(crate) fn toggle_image_fold(&mut self, line: usize, cx: &mut Context<Self>) {
        if !self.folded_image_previews.remove(&line) {
            self.folded_image_previews.insert(line);
        }
        self.clear_line_display_info();
        cx.notify();
    }

    pub(crate) fn pick_image_for_line(&self, line: usize, cx: &mut Context<Self>) {
        if self.active_tab_is_read_only() {
            return;
        }
        cx.spawn(async move |this, cx| {
            #[cfg(target_os = "macos")]
            let maybe_path = match this.update(cx, |_, cx| {
                cx.prompt_for_paths(gpui::PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: false,
                    prompt: None,
                })
            }) {
                Ok(receiver) => receiver
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .flatten()
                    .and_then(|mut paths| paths.pop())
                    .filter(|path| super::is_image_path(path)),
                Err(_) => None,
            };
            #[cfg(not(target_os = "macos"))]
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
        if self.active_tab_is_read_only() {
            return;
        }
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
                self.retire_image_cache_entry(&old_url);
                self.reparse();
                self.pause_blinking(cx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::generic_code_fold_range_for_line_impl;

    #[test]
    fn generic_fold_supports_brace_blocks() {
        let lines = [
            "fn main() {",
            "    if ready {",
            "        work();",
            "    }",
            "}",
        ];

        let range = generic_code_fold_range_for_line_impl(lines.len(), 0, |idx| lines[idx]);

        assert_eq!(range, Some(1..5));
    }

    #[test]
    fn generic_fold_supports_indentation_blocks() {
        let lines = [
            "def run():",
            "    step_one()",
            "    if ready:",
            "        step_two()",
            "    finish()",
            "done()",
        ];

        let range = generic_code_fold_range_for_line_impl(lines.len(), 0, |idx| lines[idx]);

        assert_eq!(range, Some(1..5));
    }

    #[test]
    fn generic_fold_supports_region_markers() {
        let lines = ["// #region Setup", "const port = 3000;", "// #endregion"];

        let range = generic_code_fold_range_for_line_impl(lines.len(), 0, |idx| lines[idx]);

        assert_eq!(range, Some(1..3));
    }

    #[test]
    fn generic_fold_supports_markup_tags() {
        let lines = ["<section>", "  <div>", "    body", "  </div>", "</section>"];

        let range = generic_code_fold_range_for_line_impl(lines.len(), 0, |idx| lines[idx]);

        assert_eq!(range, Some(1..5));
    }
}
