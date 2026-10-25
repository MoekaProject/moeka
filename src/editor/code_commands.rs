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

fn line_comment_edits(
    line: &str,
    style: CommentStyle,
    uncomment: bool,
) -> Vec<table_cell_list_edit::Edit> {
    use table_cell_list_edit::Edit;

    let leading_len = line.len() - line.trim_start_matches([' ', '\t']).len();
    let end = line.len() - usize::from(line.ends_with('\r'));
    if !uncomment {
        let mut edits = vec![Edit::new(
            leading_len..leading_len,
            format!("{} ", style.prefix),
        )];
        if let Some(suffix) = style.suffix {
            edits.push(Edit::new(end..end, format!(" {suffix}")));
        }
        return edits;
    }
    let body = &line[leading_len..end];
    let Some(rest) = body.strip_prefix(style.prefix) else {
        return Vec::new();
    };
    let prefix_end = leading_len + style.prefix.len() + usize::from(rest.starts_with(' '));
    let mut edits = vec![Edit::new(leading_len..prefix_end, String::new())];
    if let Some(suffix) = style.suffix {
        let trimmed = line[..end].trim_end();
        if let Some(before_suffix) = trimmed.strip_suffix(suffix) {
            let suffix_start = before_suffix.len() - usize::from(before_suffix.ends_with(' '));
            edits.push(Edit::new(
                suffix_start.max(prefix_end)..trimmed.len(),
                String::new(),
            ));
        }
    }
    edits
}

fn apply_line_comment_edits(
    buffer: &mut Buffer,
    cursors: &mut [(Cursor, bool)],
    edits: Vec<table_cell_list_edit::Edit>,
) {
    let suffix_len = |offset| {
        edits
            .iter()
            .find_map(|edit| {
                (edit.range.is_empty() && edit.range.start == offset && edit.text.starts_with(' '))
                    .then_some(edit.text.len())
            })
            .unwrap_or(0)
    };
    let biases: Vec<_> = cursors
        .iter()
        .map(|(cursor, _)| (suffix_len(cursor.offset), suffix_len(cursor.anchor)))
        .collect();
    table_cell_list_edit::apply_prefix_edits(buffer, cursors, edits);
    for ((cursor, _), (offset_bias, anchor_bias)) in cursors.iter_mut().zip(biases) {
        cursor.offset -= offset_bias;
        cursor.anchor -= anchor_bias;
    }
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

struct JoinedLinesEdit {
    range: Range<usize>,
    text: String,
    boundaries: Vec<(Range<usize>, usize)>,
}

impl JoinedLinesEdit {
    fn remap_cursor(&self, cursor: &mut Cursor) {
        for (range, inserted_len) in self.boundaries.iter().rev() {
            cursor.offset = offset_after_source_replacement(cursor.offset, range, *inserted_len);
            cursor.anchor = offset_after_source_replacement(cursor.anchor, range, *inserted_len);
        }
    }
}

fn joined_lines_edit(buffer: &Buffer, start: usize, end: usize) -> JoinedLinesEdit {
    let range = buffer.line_start(start)..buffer.line_end(end);
    let mut text = String::with_capacity(range.len());
    let mut boundaries = Vec::with_capacity(end.saturating_sub(start));
    for line in start..=end {
        let raw = buffer.line_content(line);
        let content = if line < end {
            raw.strip_suffix('\r').unwrap_or(raw)
        } else {
            raw
        };
        if line == start {
            text.push_str(content);
        } else {
            let trimmed = content.trim_start_matches([' ', '\t']);
            let separator = join_separator(&text, trimmed);
            let previous_end = buffer.line_end(line - 1)
                - usize::from(buffer.line_content(line - 1).ends_with('\r'));
            boundaries.push((
                previous_end..buffer.line_start(line) + content.len() - trimmed.len(),
                separator.len(),
            ));
            text.push_str(separator);
            text.push_str(trimmed);
        }
    }
    JoinedLinesEdit {
        range,
        text,
        boundaries,
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
        let edits: Vec<_> = actionable
            .into_iter()
            .flat_map(|(line, style)| {
                let start = self.buffer.line_start(line);
                line_comment_edits(self.buffer.line_content(line), style, should_uncomment)
                    .into_iter()
                    .map(move |edit| edit.translated(start))
            })
            .collect();
        let fold_edits: Vec<_> = edits
            .iter()
            .map(|edit| (edit.range.clone(), edit.text.len()))
            .collect();
        let anchors = self.snapshot_folded_anchor_offsets();
        let mut cursors = self.cursor_states_with_primary_flag();
        self.checkpoint();
        self.pending_line_display_transfer = None;
        apply_line_comment_edits(&mut self.buffer, &mut cursors, edits);
        self.restore_cursor_states(cursors);
        self.remap_folded_anchor_offsets(anchors, &fold_edits);
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
            self.finish_fold_change(cx);
        }
    }

    pub(crate) fn toggle_current_fold(&mut self, cx: &mut Context<Self>) {
        if let Some(line) = self.folded_line_containing_cursor() {
            self.folded_sections.remove(&line);
            self.finish_fold_change(cx);
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
            self.finish_fold_change(cx);
        }
    }

    pub(crate) fn unfold_all_regions(&mut self, cx: &mut Context<Self>) {
        if self.folded_sections.is_empty() {
            return;
        }
        self.folded_sections.clear();
        self.finish_fold_change(cx);
    }

    pub(crate) fn join_lines(&mut self, cx: &mut Context<Self>) {
        if !self.begin_content_edit(cx) {
            return;
        }
        self.skip_folded_anchor_for_line_break();

        let (start_line, mut end_line) = self.selected_line_span();
        if start_line == end_line {
            if start_line + 1 >= self.buffer.line_count() {
                return;
            }
            end_line = start_line + 1;
        }

        let edit = joined_lines_edit(&self.buffer, start_line, end_line);
        self.queue_non_markdown_line_display_transfer(&edit.range, &edit.text);
        self.checkpoint();
        for cursor in std::iter::once(&mut self.cursor).chain(&mut self.extra_cursors) {
            edit.remap_cursor(cursor);
        }
        self.buffer.replace(edit.range, &edit.text);
        for cursor in std::iter::once(&mut self.cursor).chain(&mut self.extra_cursors) {
            cursor.offset = self.buffer.clamp_grapheme_offset(cursor.offset);
            cursor.anchor = self.buffer.clamp_grapheme_offset(cursor.anchor);
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

    fn commented_line(line: &str, style: CommentStyle, uncomment: bool) -> String {
        let mut buffer = Buffer::from_text(line);
        apply_line_comment_edits(
            &mut buffer,
            &mut [],
            line_comment_edits(line, style, uncomment),
        );
        buffer.text().to_string()
    }

    #[test]
    fn line_comment_style_roundtrips() {
        let style = CommentStyle::line("//");
        let commented = commented_line("  let value = 1;", style, false);
        assert_eq!(commented, "  // let value = 1;");
        assert_eq!(commented_line(&commented, style, true), "  let value = 1;");
    }

    #[test]
    fn block_comment_style_roundtrips() {
        let style = CommentStyle::block("<!--", "-->");
        let commented = commented_line("  title", style, false);
        assert_eq!(commented, "  <!-- title -->");
        assert_eq!(commented_line(&commented, style, true), "  title");
    }

    #[test]
    fn comment_edits_preserve_crlf_and_map_all_unicode_carets() {
        for style in [CommentStyle::line("//"), CommentStyle::block("<!--", "-->")] {
            let source = "  \u{1f642}\r\nnext";
            let mut buffer = Buffer::from_text(source);
            let mut cursors = vec![
                (
                    Cursor {
                        anchor: 2,
                        offset: 6,
                    },
                    true,
                ),
                (
                    Cursor {
                        anchor: buffer.len(),
                        offset: buffer.len(),
                    },
                    false,
                ),
            ];
            for uncomment in [false, true] {
                let edits = line_comment_edits(buffer.line_content(0), style, uncomment);
                apply_line_comment_edits(&mut buffer, &mut cursors, edits);
                assert_eq!(
                    buffer.text_for_range(cursors[0].0.selection_range()),
                    "\u{1f642}"
                );
                assert_eq!(cursors[1].0.offset, buffer.len());
                assert!(buffer.line_content(0).ends_with('\r'));
            }
            assert_eq!(buffer.text(), source);
            assert_eq!(
                cursors[0].0,
                Cursor {
                    anchor: 2,
                    offset: 6
                }
            );
        }
        assert_eq!(
            commented_line("<!-- -->", CommentStyle::block("<!--", "-->"), true),
            ""
        );
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

    #[test]
    fn joined_lines_edit_preserves_spacing_and_line_endings() {
        for (source, expected) in [
            ("one\n  two\nthree", "one two three"),
            ("one\n \n  three", "one three"),
            ("one\n  two  \nthree", "one two  three"),
            ("one\r\n  two\r\nthree", "one two three"),
            ("one\r\n  two\r\n", "one two\r\n"),
        ] {
            let mut buffer = Buffer::from_text(source);
            let end = if source.ends_with('\n') {
                buffer.line_count() - 2
            } else {
                buffer.line_count() - 1
            };
            let edit = joined_lines_edit(&buffer, 0, end);
            buffer.replace(edit.range, &edit.text);
            assert_eq!(buffer.text(), expected);
        }
    }

    #[test]
    fn joined_lines_edit_remaps_selection_and_extra_carets() {
        let mut buffer = Buffer::from_text("a\r\n  \u{1f642}\r\nz");
        let mut cursor = Cursor {
            anchor: 5,
            offset: 9,
        };
        let mut after = Cursor {
            anchor: buffer.len(),
            offset: buffer.len(),
        };
        let edit = joined_lines_edit(&buffer, 0, 1);
        edit.remap_cursor(&mut cursor);
        edit.remap_cursor(&mut after);
        buffer.replace(edit.range, &edit.text);
        assert_eq!(buffer.text(), "a \u{1f642}\r\nz");
        assert_eq!(buffer.text_for_range(cursor.selection_range()), "\u{1f642}");
        assert_eq!(after.offset, buffer.len());
        assert_eq!(after.anchor, after.offset);
    }

    #[test]
    fn joined_lines_edit_handles_large_selections() {
        let source = "word\n".repeat(4096);
        let buffer = Buffer::from_text(&source);
        let edit = joined_lines_edit(&buffer, 0, 4095);
        assert_eq!(edit.text, vec!["word"; 4096].join(" "));
    }
}
