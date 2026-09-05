use gpui::{Context, Window};

use crate::actions::*;
use crate::markdown::BlockType;

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct CommentStyle {
    prefix: &'static str,
    suffix: Option<&'static str>,
}

impl CommentStyle {
    const fn line(prefix: &'static str) -> Self {
        Self {
            prefix,
            suffix: None,
        }
    }

    const fn block(prefix: &'static str, suffix: &'static str) -> Self {
        Self {
            prefix,
            suffix: Some(suffix),
        }
    }
}

fn normalize_language(language: &str) -> String {
    language
        .trim()
        .trim_matches('`')
        .split(|ch: char| {
            ch.is_whitespace() || matches!(ch, ',' | '{' | '}' | '[' | ']' | '(' | ')')
        })
        .next()
        .unwrap_or_default()
        .trim_start_matches('.')
        .to_ascii_lowercase()
}

fn comment_style_for_language(language: &str) -> Option<CommentStyle> {
    match normalize_language(language).as_str() {
        "javascript" | "js" | "typescript" | "ts" | "tsx" | "jsx" | "rust" | "rs" | "go"
        | "swift" | "kotlin" | "kt" | "java" | "c" | "cpp" | "c++" | "cxx" | "cc" | "c#" | "cs"
        | "csharp" | "dart" | "scala" | "php" | "objective-c" | "objc" => {
            Some(CommentStyle::line("//"))
        }
        "python" | "py" | "ruby" | "rb" | "bash" | "sh" | "shell" | "zsh" | "fish" | "yaml"
        | "yml" | "toml" | "dockerfile" | "makefile" | "make" | "perl" | "r" | "julia"
        | "elixir" | "nim" => Some(CommentStyle::line("#")),
        "sql" | "lua" | "haskell" | "hs" | "ada" => Some(CommentStyle::line("--")),
        "clojure" | "lisp" | "scheme" | "racket" | "ini" | "conf" | "cfg" => {
            Some(CommentStyle::line(";"))
        }
        "erlang" | "erl" | "prolog" => Some(CommentStyle::line("%")),
        "html" | "xml" | "vue" | "svelte" | "markdown" | "md" => {
            Some(CommentStyle::block("<!--", "-->"))
        }
        "css" | "scss" | "sass" | "less" => Some(CommentStyle::block("/*", "*/")),
        _ => None,
    }
}

fn line_is_commented(line: &str, style: CommentStyle) -> bool {
    let body = line.trim_start_matches([' ', '\t']);
    if let Some(suffix) = style.suffix {
        body.starts_with(style.prefix) && body.trim_end().ends_with(suffix)
    } else {
        body.starts_with(style.prefix)
    }
}

fn comment_line(line: &str, style: CommentStyle) -> String {
    let leading_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    let (leading, body) = line.split_at(leading_len);
    match style.suffix {
        Some(suffix) => format!("{}{} {} {}", leading, style.prefix, body, suffix),
        None => format!("{}{} {}", leading, style.prefix, body),
    }
}

fn uncomment_line(line: &str, style: CommentStyle) -> String {
    let leading_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    let (leading, body) = line.split_at(leading_len);
    let mut body = body;

    if let Some(rest) = body.strip_prefix(style.prefix) {
        body = rest.strip_prefix(' ').unwrap_or(rest);
    }

    if let Some(suffix) = style.suffix {
        let trimmed_end_len = body.trim_end().len();
        if let Some(before_suffix) = body[..trimmed_end_len].strip_suffix(suffix) {
            body = before_suffix.strip_suffix(' ').unwrap_or(before_suffix);
        }
    }

    format!("{}{}", leading, body)
}

fn join_separator(left: &str, right: &str) -> &'static str {
    if left.is_empty()
        || right.is_empty()
        || left.ends_with(char::is_whitespace)
        || right.starts_with(char::is_whitespace)
    {
        ""
    } else {
        " "
    }
}

impl EditorView {
    fn target_lines_for_primary_selection(&self) -> Vec<usize> {
        let (start, end) = self.selected_line_span();
        (start..=end).collect()
    }

    fn comment_style_for_line(&self, line: usize) -> Option<CommentStyle> {
        if self.active_tab_is_non_markdown_text() {
            return match self.active_file_kind() {
                Some(FileKind::Code { language }) => comment_style_for_language(language),
                Some(FileKind::Text) => None,
                _ => None,
            };
        }

        if self.active_tab_is_markdown() {
            let decoration = self.decorations.get(line)?;
            return match decoration.block_type {
                BlockType::CodeBlock => decoration
                    .code_language
                    .as_deref()
                    .and_then(comment_style_for_language),
                BlockType::CodeBlockFence => None,
                _ => Some(CommentStyle::block("<!--", "-->")),
            };
        }

        None
    }

    pub(crate) fn toggle_line_comment(&mut self, cx: &mut Context<Self>) {
        if !self.begin_content_edit(cx) {
            return;
        }

        let lines = self.target_lines_for_primary_selection();
        let actionable: Vec<(usize, CommentStyle)> = lines
            .into_iter()
            .filter_map(|line| {
                let content = self.buffer.line_content(line);
                (!content.trim().is_empty())
                    .then(|| self.comment_style_for_line(line).map(|style| (line, style)))
                    .flatten()
            })
            .collect();

        if actionable.is_empty() {
            return;
        }

        let should_uncomment = actionable
            .iter()
            .all(|(line, style)| line_is_commented(self.buffer.line_content(*line), *style));

        self.checkpoint();
        for (line, style) in actionable.into_iter().rev() {
            let line_start = self.buffer.line_start(line);
            let line_end = self.buffer.line_end(line);
            let current = self.buffer.line_content(line).to_string();
            let replacement = if should_uncomment {
                uncomment_line(&current, style)
            } else {
                comment_line(&current, style)
            };
            self.buffer.replace(line_start..line_end, &replacement);
        }

        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn fold_line_near_cursor(&self) -> Option<usize> {
        let cursor_line = self.cursor_line();
        if self.fold_range_for_line(cursor_line).is_some() {
            return Some(cursor_line);
        }
        (0..=cursor_line)
            .rev()
            .find(|line| self.fold_range_for_line(*line).is_some())
    }

    fn folded_line_containing_cursor(&self) -> Option<usize> {
        let cursor_line = self.cursor_line();
        self.folded_sections.iter().copied().find(|fold_line| {
            *fold_line == cursor_line
                || self
                    .fold_range_for_line(*fold_line)
                    .is_some_and(|range| range.contains(&cursor_line))
        })
    }

    pub(crate) fn fold_current_region(&mut self, cx: &mut Context<Self>) {
        let Some(line) = self.fold_line_near_cursor() else {
            return;
        };
        if !self.folded_sections.contains(&line) {
            self.toggle_fold(line, cx);
        }
    }

    pub(crate) fn unfold_current_region(&mut self, cx: &mut Context<Self>) {
        let Some(line) = self.folded_line_containing_cursor() else {
            return;
        };
        if self.folded_sections.remove(&line) {
            self.clear_line_display_info();
            cx.notify();
            self.pause_blinking(cx);
        }
    }

    pub(crate) fn toggle_current_fold(&mut self, cx: &mut Context<Self>) {
        if let Some(line) = self.folded_line_containing_cursor() {
            self.folded_sections.remove(&line);
            self.clear_line_display_info();
            cx.notify();
            self.pause_blinking(cx);
        } else if let Some(line) = self.fold_line_near_cursor() {
            self.toggle_fold(line, cx);
        }
    }

    pub(crate) fn fold_all_regions(&mut self, cx: &mut Context<Self>) {
        let mut changed = false;
        for line in 0..self.buffer.line_count() {
            if self.fold_range_for_line(line).is_some() {
                changed |= self.folded_sections.insert(line);
            }
        }
        if changed {
            self.clear_line_display_info();
            cx.notify();
            self.pause_blinking(cx);
        }
    }

    pub(crate) fn unfold_all_regions(&mut self, cx: &mut Context<Self>) {
        if self.folded_sections.is_empty() {
            return;
        }
        self.folded_sections.clear();
        self.clear_line_display_info();
        cx.notify();
        self.pause_blinking(cx);
    }

    pub(crate) fn join_lines(&mut self, cx: &mut Context<Self>) {
        if !self.begin_content_edit(cx) {
            return;
        }

        let (start_line, mut end_line) = self.selected_line_span();
        if start_line == end_line {
            if start_line + 1 >= self.buffer.line_count() {
                return;
            }
            end_line = start_line + 1;
        }

        self.checkpoint();
        for line in (start_line..end_line).rev() {
            let left_end = self.buffer.line_end(line);
            let next_start = self.buffer.line_start(line + 1);
            let next_line = self.buffer.line_content(line + 1).to_string();
            let next_trimmed = next_line.trim_start_matches([' ', '\t']);
            let leading = next_line.len() - next_trimmed.len();
            let left_line = self.buffer.line_content(line).to_string();
            let separator = join_separator(&left_line, next_trimmed);
            self.buffer
                .replace(left_end..next_start + leading, separator);
        }

        self.cursor.offset = self.cursor.offset.min(self.buffer.len());
        self.cursor.anchor = self.cursor.anchor.min(self.buffer.len());
        for cursor in &mut self.extra_cursors {
            cursor.offset = cursor.offset.min(self.buffer.len());
            cursor.anchor = cursor.anchor.min(self.buffer.len());
        }
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_toggle_line_comment(
        &mut self,
        _: &ToggleLineComment,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_line_comment(cx);
    }

    pub(crate) fn on_fold(&mut self, _: &Fold, _: &mut Window, cx: &mut Context<Self>) {
        self.fold_current_region(cx);
    }

    pub(crate) fn on_unfold(&mut self, _: &Unfold, _: &mut Window, cx: &mut Context<Self>) {
        self.unfold_current_region(cx);
    }

    pub(crate) fn on_toggle_fold(
        &mut self,
        _: &ToggleFold,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_current_fold(cx);
    }

    pub(crate) fn on_fold_all(&mut self, _: &FoldAll, _: &mut Window, cx: &mut Context<Self>) {
        self.fold_all_regions(cx);
    }

    pub(crate) fn on_unfold_all(&mut self, _: &UnfoldAll, _: &mut Window, cx: &mut Context<Self>) {
        self.unfold_all_regions(cx);
    }

    pub(crate) fn on_join_lines(&mut self, _: &JoinLines, _: &mut Window, cx: &mut Context<Self>) {
        self.join_lines(cx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_comment_style_roundtrips() {
        let style = CommentStyle::line("//");
        let commented = comment_line("  let value = 1;", style);
        assert_eq!(commented, "  // let value = 1;");
        assert_eq!(uncomment_line(&commented, style), "  let value = 1;");
    }

    #[test]
    fn block_comment_style_roundtrips() {
        let style = CommentStyle::block("<!--", "-->");
        let commented = comment_line("  title", style);
        assert_eq!(commented, "  <!-- title -->");
        assert_eq!(uncomment_line(&commented, style), "  title");
    }

    #[test]
    fn detects_common_comment_styles() {
        assert_eq!(
            comment_style_for_language("rust"),
            Some(CommentStyle::line("//"))
        );
        assert_eq!(
            comment_style_for_language("python"),
            Some(CommentStyle::line("#"))
        );
        assert_eq!(
            comment_style_for_language("sql"),
            Some(CommentStyle::line("--"))
        );
        assert_eq!(
            comment_style_for_language("html"),
            Some(CommentStyle::block("<!--", "-->"))
        );
    }

    #[test]
    fn join_separator_adds_space_between_words() {
        assert_eq!(join_separator("hello", "world"), " ");
        assert_eq!(join_separator("hello ", "world"), "");
        assert_eq!(join_separator("hello", " world"), "");
    }
}
