use std::ops::Range;

use crate::{buffer::Buffer, cursor::Cursor};

use super::*;

struct MultiCursorImeEdit {
    start: usize,
    selection_start: usize,
    selection_end: usize,
    marked_range: Option<Range<usize>>,
    is_primary: bool,
    delta: isize,
}

pub(crate) fn paired_delimiter(input: &str) -> Option<(&'static str, &'static str)> {
    match input {
        "[" => Some(("[", "]")),
        "{" => Some(("{", "}")),
        "(" => Some(("(", ")")),
        "<" => Some(("<", ">")),
        "\"" => Some(("\"", "\"")),
        "'" => Some(("'", "'")),
        "`" => Some(("`", "`")),
        "【" => Some(("【", "】")),
        "｛" => Some(("｛", "｝")),
        "（" => Some(("（", "）")),
        "《" => Some(("《", "》")),
        "〈" => Some(("〈", "〉")),
        "「" => Some(("「", "」")),
        "『" => Some(("『", "』")),
        "“" => Some(("“", "”")),
        "‘" => Some(("‘", "’")),
        _ => None,
    }
}

pub(crate) fn paired_delimiter_at(
    text: &str,
    cursor_offset: usize,
    input: &str,
) -> Option<(&'static str, &'static str)> {
    let previous = text[..cursor_offset].chars().next_back();
    let next = text[cursor_offset..].chars().next();
    let at_line_start = text[..cursor_offset]
        .rsplit_once('\n')
        .map(|(_, line)| line.trim().is_empty())
        .unwrap_or_else(|| text[..cursor_offset].trim().is_empty());
    let previous_is_escape = previous == Some('\\');
    let next_is_word = next.is_some_and(|ch| ch.is_alphanumeric() || ch == '_');
    let previous_is_word = previous.is_some_and(|ch| ch.is_alphanumeric() || ch == '_');

    match input {
        "`" => {
            if previous_is_escape {
                None
            } else {
                Some(("`", "`"))
            }
        }
        "*" => {
            if previous_is_escape || next == Some('*') {
                None
            } else {
                Some(("*", "*"))
            }
        }
        "_" => {
            if previous_is_escape || previous_is_word || next_is_word || next == Some('_') {
                None
            } else {
                Some(("_", "_"))
            }
        }
        "~" => {
            if previous_is_escape || (previous == Some('~') && next == Some('~')) {
                None
            } else {
                Some(("~", "~"))
            }
        }
        "+" => {
            if previous_is_escape || (previous == Some('+') && next == Some('+')) {
                None
            } else {
                Some(("+", "+"))
            }
        }
        "=" => {
            if previous_is_escape || (previous == Some('=') && next == Some('=')) {
                None
            } else {
                Some(("=", "="))
            }
        }
        "<" => {
            if previous_is_escape || at_line_start || next_is_word {
                None
            } else {
                Some(("<", ">"))
            }
        }
        "'" | "\"" => {
            if previous_is_escape || previous_is_word || next_is_word {
                None
            } else {
                paired_delimiter(input)
            }
        }
        "[" | "{" | "(" => {
            // Don't auto-close brackets right before a word/number — the user is
            // almost always wrapping what follows (e.g. typing "(" before "abc"
            // wants "(abc)", not "()abc"). Closing is still inserted at line/word
            // boundaries.
            if next_is_word {
                None
            } else {
                paired_delimiter(input)
            }
        }
        _ => paired_delimiter(input),
    }
}

pub(crate) fn code_block_autocomplete_range(
    text: &str,
    cursor_offset: usize,
    new_text: &str,
) -> Option<Range<usize>> {
    if new_text != "`" || cursor_offset < 2 {
        return None;
    }

    if text.get(cursor_offset - 2..cursor_offset) != Some("``") {
        return None;
    }

    if cursor_offset >= 3 && text.get(cursor_offset - 3..cursor_offset - 2) == Some("`") {
        return None;
    }

    Some(cursor_offset - 2..cursor_offset)
}

fn is_auto_closing_delimiter(input: &str) -> bool {
    matches!(
        input,
        "]" | "}"
            | ")"
            | ">"
            | "\""
            | "'"
            | "`"
            | "】"
            | "｝"
            | "）"
            | "》"
            | "〉"
            | "」"
            | "』"
            | "”"
            | "’"
    )
}

fn auto_paired_delimiter_matches(open: char, close: char) -> bool {
    matches!(
        (open, close),
        ('[', ']')
            | ('{', '}')
            | ('(', ')')
            | ('<', '>')
            | ('"', '"')
            | ('\'', '\'')
            | ('`', '`')
            | ('【', '】')
            | ('｛', '｝')
            | ('（', '）')
            | ('《', '》')
            | ('〈', '〉')
            | ('「', '」')
            | ('『', '』')
            | ('“', '”')
            | ('‘', '’')
    )
}

fn clamp_ime_offset(buffer: &Buffer, offset: usize, cumulative_delta: isize) -> usize {
    buffer.clamp_grapheme_offset(apply_offset_delta(offset, cumulative_delta))
}

impl EditorView {
    /// Whether `offset` sits inside code (a fenced/indented code block, the body
    /// of a non-markdown file, or inline code). Markdown emphasis auto-pairing is
    /// suppressed there since `* _ ~ + =` are literal in code.
    pub(crate) fn offset_in_code_context(&self, offset: usize) -> bool {
        let (line, col) = self.buffer.offset_to_line_col(offset);
        let Some(decoration) = self.decorations.get(line) else {
            return false;
        };
        if matches!(
            decoration.block_type,
            BlockType::CodeBlock | BlockType::CodeBlockFence
        ) {
            return true;
        }
        decoration.inline_spans.iter().any(|span| {
            span.kind == crate::markdown::InlineKind::InlineCode
                && span.range.start <= col
                && col <= span.range.end
        })
    }

    pub(crate) fn should_skip_auto_closing_delimiter(&self, new_text: &str) -> bool {
        if !is_auto_closing_delimiter(new_text) {
            return false;
        }

        self.buffer
            .text()
            .get(self.cursor.offset..self.cursor.offset + new_text.len())
            == Some(new_text)
    }

    pub(crate) fn auto_paired_backspace_range(&self) -> Option<Range<usize>> {
        if self.cursor.has_selection() {
            return None;
        }

        let text = self.buffer.text();
        let offset = self.cursor.offset;

        if offset >= 2
            && text.get(offset - 2..offset) == Some("[[")
            && text.get(offset..offset + 2) == Some("]]")
        {
            return Some(offset - 2..offset + 2);
        }

        let (prev_start, prev_char) = text[..offset].char_indices().next_back()?;
        let next_char = text[offset..].chars().next()?;
        let next_end = offset + next_char.len_utf8();
        if auto_paired_delimiter_matches(prev_char, next_char) {
            Some(prev_start..next_end)
        } else {
            None
        }
    }

    pub(crate) fn begin_ime_composition(&mut self) {
        if self.ime_composition_snapshot.is_none() {
            self.ime_composition_snapshot = Some(self.current_snapshot());
        }
    }

    pub(crate) fn collect_multi_cursor_ime_targets_desc(
        &self,
        primary_override_range: Option<Range<usize>>,
    ) -> Vec<(Range<usize>, bool)> {
        let primary_range = primary_override_range
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.cursor.selection_range());

        let mut all = Vec::with_capacity(1 + self.extra_cursors.len());
        all.push((
            primary_range.start.min(primary_range.end)..primary_range.end.max(primary_range.start),
            true,
        ));

        for (index, cursor) in self.extra_cursors.iter().enumerate() {
            let range = self
                .extra_marked_ranges
                .get(index)
                .cloned()
                .unwrap_or_else(|| {
                    cursor.offset.min(cursor.anchor)..cursor.offset.max(cursor.anchor)
                });
            all.push((
                range.start.min(range.end)..range.end.max(range.start),
                false,
            ));
        }

        all.sort_by(|left, right| {
            right
                .0
                .start
                .cmp(&left.0.start)
                .then_with(|| right.0.end.cmp(&left.0.end))
        });
        all
    }

    pub(crate) fn apply_multi_cursor_ime_replace(
        &mut self,
        primary_override_range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<&Range<usize>>,
        keep_marked_ranges: bool,
    ) -> bool {
        let selection_relative = new_selected_range_utf16
            .map(|range_utf16| {
                (
                    Self::utf8_offset_in_text_for_utf16(new_text, range_utf16.start),
                    Self::utf8_offset_in_text_for_utf16(new_text, range_utf16.end),
                )
            })
            .unwrap_or((new_text.len(), new_text.len()));

        let mut range_spans_multiple_lines = false;
        let mut edits = Vec::with_capacity(1 + self.extra_cursors.len());

        for (range, is_primary) in
            self.collect_multi_cursor_ime_targets_desc(primary_override_range)
        {
            let (start_line, _) = self.buffer.offset_to_line_col(range.start);
            let (end_line, _) = self.buffer.offset_to_line_col(range.end);
            range_spans_multiple_lines |= start_line != end_line;

            let before_len = self.buffer.len();
            self.buffer.replace(range.clone(), new_text);
            let after_len = self.buffer.len();

            let mark_start = range.start;
            let mark_end = mark_start + new_text.len();
            edits.push(MultiCursorImeEdit {
                start: range.start,
                selection_start: mark_start + selection_relative.0,
                selection_end: mark_start + selection_relative.1,
                marked_range: if keep_marked_ranges && !new_text.is_empty() {
                    Some(mark_start..mark_end)
                } else {
                    None
                },
                is_primary,
                delta: after_len as isize - before_len as isize,
            });
        }

        edits.sort_by_key(|edit| edit.start);
        self.clear_marked_ranges();
        self.extra_cursors.clear();

        let mut cumulative_delta = 0isize;
        for edit in edits {
            let anchor = clamp_ime_offset(&self.buffer, edit.selection_start, cumulative_delta);
            let offset = clamp_ime_offset(&self.buffer, edit.selection_end, cumulative_delta);
            let marked_range = edit.marked_range.map(|range| {
                let start = clamp_ime_offset(&self.buffer, range.start, cumulative_delta);
                let end = clamp_ime_offset(&self.buffer, range.end, cumulative_delta);
                start..end
            });

            if edit.is_primary {
                self.cursor.anchor = anchor;
                self.cursor.offset = offset;
                self.marked_range = marked_range;
            } else {
                self.extra_cursors.push(Cursor { offset, anchor });
                if let Some(marked_range) = marked_range {
                    self.extra_marked_ranges.push(marked_range);
                }
            }

            cumulative_delta += edit.delta;
        }

        range_spans_multiple_lines
    }

    pub(crate) fn finish_ime_composition(&mut self) {
        let Some(snapshot) = self.ime_composition_snapshot.take() else {
            return;
        };

        let changed = snapshot.text != self.buffer.text()
            || snapshot.cursor_offset != self.cursor.offset
            || snapshot.cursor_anchor != self.cursor.anchor
            || snapshot.extra_cursors
                != self
                    .extra_cursors
                    .iter()
                    .map(|cursor| (cursor.offset, cursor.anchor))
                    .collect::<Vec<_>>();
        if changed {
            self.push_undo_snapshot(snapshot);
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::buffer::Buffer;

    use super::{clamp_ime_offset, code_block_autocomplete_range, paired_delimiter_at};

    #[test]
    fn third_backtick_triggers_code_block_autocomplete() {
        assert_eq!(code_block_autocomplete_range("``", 2, "`"), Some(0..2));
    }

    #[test]
    fn fourth_backtick_does_not_retrigger_code_block_autocomplete() {
        assert_eq!(code_block_autocomplete_range("```", 3, "`"), None);
    }

    #[test]
    fn paired_delimiter_at_handles_markdown_backticks_and_emphasis() {
        assert_eq!(paired_delimiter_at("", 0, "`"), Some(("`", "`")));
        assert_eq!(paired_delimiter_at("\\", 1, "`"), None);
        assert_eq!(paired_delimiter_at("foo", 3, "_"), None);
        assert_eq!(paired_delimiter_at("foo ", 4, "_"), Some(("_", "_")));
        assert_eq!(paired_delimiter_at("*", 1, "*"), Some(("*", "*")));
    }

    #[test]
    fn paired_delimiter_at_avoids_commonmark_autolink_and_html_conflicts() {
        assert_eq!(paired_delimiter_at("", 0, "<"), None);
        assert_eq!(paired_delimiter_at("see ", 4, "<"), Some(("<", ">")));
        assert_eq!(paired_delimiter_at("see ", 4, "'"), Some(("'", "'")));
        assert_eq!(paired_delimiter_at("word", 4, "'"), None);
    }

    #[test]
    fn paired_delimiter_at_covers_markdown_extension_delimiters() {
        assert_eq!(paired_delimiter_at("", 0, "("), Some(("(", ")")));
        assert_eq!(paired_delimiter_at("", 0, "{"), Some(("{", "}")));
        assert_eq!(paired_delimiter_at("", 0, "~"), Some(("~", "~")));
        assert_eq!(paired_delimiter_at("~", 1, "~"), Some(("~", "~")));
        assert_eq!(paired_delimiter_at("~~", 1, "~"), None);
        assert_eq!(paired_delimiter_at("", 0, "+"), Some(("+", "+")));
        assert_eq!(paired_delimiter_at("+", 1, "+"), Some(("+", "+")));
        assert_eq!(paired_delimiter_at("++", 1, "+"), None);
        assert_eq!(paired_delimiter_at("", 0, "="), Some(("=", "=")));
        assert_eq!(paired_delimiter_at("=", 1, "="), Some(("=", "=")));
        assert_eq!(paired_delimiter_at("==", 1, "="), None);
        assert_eq!(paired_delimiter_at("", 0, "【"), Some(("【", "】")));
    }

    #[test]
    fn clamp_ime_offset_snaps_into_grapheme_boundary() {
        let buffer = Buffer::from_text("a\u{0301}b");
        let combining_boundary = buffer
            .text()
            .char_indices()
            .nth(1)
            .map(|(idx, _)| idx)
            .expect("combining mark boundary");

        assert_eq!(clamp_ime_offset(&buffer, combining_boundary, 0), 0);
    }

    #[test]
    fn clamp_ime_offset_saturates_negative_delta() {
        let buffer = Buffer::from_text("abc");

        assert_eq!(clamp_ime_offset(&buffer, 1, -10), 0);
    }
}
