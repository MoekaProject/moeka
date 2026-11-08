use gpui::{Context, Window};

use crate::actions::*;
use crate::markdown::BlockType;

use super::*;

pub(super) fn code_unindent_width(content: &str, max_spaces: usize) -> usize {
    if content.starts_with('\t') {
        1
    } else {
        content
            .bytes()
            .take(max_spaces)
            .take_while(|byte| *byte == b' ')
            .count()
    }
}

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

fn selected_line_spans(buffer: &Buffer, cursors: &[Cursor], join: bool) -> Vec<(usize, usize)> {
    let mut spans: Vec<_> = cursors
        .iter()
        .filter_map(|cursor| {
            let range = cursor.selection_range();
            let start = buffer.offset_to_line_col(range.start).0;
            let last = range
                .end
                .saturating_sub(usize::from(range.end > range.start));
            let mut end = buffer.offset_to_line_col(last).0;
            if join && start == end {
                if end + 1 >= buffer.line_count() {
                    return None;
                }
                end += 1;
            }
            Some((start, end))
        })
        .collect();
    spans.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in spans {
        if let Some(previous) = merged.last_mut()
            && start <= previous.1
        {
            previous.1 = previous.1.max(end);
        } else {
            merged.push((start, end));
        }
    }
    merged
}

fn code_indentation_edits(
    buffer: &Buffer,
    cursors: &[Cursor],
    indent: Option<&str>,
    unindent_width: usize,
) -> Vec<table_cell_list_edit::Edit> {
    selected_line_spans(buffer, cursors, false)
        .into_iter()
        .flat_map(|(start, end)| start..=end)
        .filter_map(|line| {
            let start = buffer.line_start(line);
            if let Some(text) = indent {
                (!text.is_empty())
                    .then(|| table_cell_list_edit::Edit::new(start..start, text.to_string()))
            } else {
                let width = code_unindent_width(buffer.line_content(line), unindent_width);
                (width > 0)
                    .then(|| table_cell_list_edit::Edit::new(start..start + width, String::new()))
            }
        })
        .collect()
}

impl EditorView {
    pub(super) fn indent_code_selection(&mut self, forward: bool, cx: &mut Context<Self>) {
        let cursors = self.all_cursors_snapshot();
        let indent_text = self.current_indent_text();
        if forward && cursors.iter().all(|cursor| !cursor.has_selection()) {
            self.checkpoint();
            self.multi_cursor_insert(&indent_text);
            self.reparse();
        } else {
            let edits = code_indentation_edits(
                &self.buffer,
                &cursors,
                forward.then_some(indent_text.as_str()),
                self.current_unindent_width(),
            );
            if edits.is_empty() {
                self.pause_blinking(cx);
                return;
            }
            let fold_edits = edits
                .iter()
                .map(|edit| (edit.range.clone(), edit.text.len()))
                .collect::<Vec<_>>();
            let edited_lines = edits
                .iter()
                .map(|edit| self.buffer.offset_to_line_col(edit.range.start).0)
                .collect::<Vec<_>>();
            let anchors = self.snapshot_folded_anchor_offsets();
            let mut cursor_states = self.cursor_states_with_primary_flag();
            self.checkpoint();
            self.pending_line_display_transfer = None;
            self.invalidate_line_display_info_for_lines(&edited_lines);
            table_cell_list_edit::apply_prefix_edits(&mut self.buffer, &mut cursor_states, edits);
            self.restore_cursor_states(cursor_states);
            self.remap_folded_anchor_offsets(anchors, &fold_edits);
            self.reparse();
        }
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn target_lines_for_cursors(&self) -> Vec<usize> {
        selected_line_spans(&self.buffer, &self.all_cursors_snapshot(), false)
            .into_iter()
            .flat_map(|(start, end)| start..=end)
            .collect()
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

        let lines = self.target_lines_for_cursors();
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
        if self.has_extra_cursors() {
            self.unfold_cursor_regions();
            let spans = selected_line_spans(&self.buffer, &self.all_cursors_snapshot(), true);
            if spans.is_empty() {
                return;
            }
            let edits: Vec<_> = spans
                .into_iter()
                .map(|(start, end)| joined_lines_edit(&self.buffer, start, end))
                .collect();
            let fold_edits: Vec<_> = edits
                .iter()
                .flat_map(|edit| edit.boundaries.iter().cloned())
                .collect();
            let anchors = self.snapshot_folded_anchor_offsets();
            let mut cursors = self.cursor_states_with_primary_flag();
            self.checkpoint();
            self.pending_line_display_transfer = None;
            for edit in edits.into_iter().rev() {
                for (cursor, _) in &mut cursors {
                    edit.remap_cursor(cursor);
                }
                self.buffer.replace(edit.range, &edit.text);
            }
            self.remap_folded_anchor_offsets(anchors, &fold_edits);
            self.restore_cursor_states(cursors);
            self.mark_active_tab_dirty();
            self.reparse();
            self.scroll_to_cursor();
            self.pause_blinking(cx);
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

    #[test]
    fn code_indent_and_unindent_all_selected_lines_preserve_selection_and_undo() {
        let source = "α\r\nbeta\r\n γ\r\nend";
        for reverse in [false, true] {
            let mut buffer = Buffer::from_text(source);
            let end = buffer.line_start(3);
            let cursor = if reverse {
                Cursor {
                    anchor: end,
                    offset: 0,
                }
            } else {
                Cursor {
                    anchor: 0,
                    offset: end,
                }
            };
            let mut cursors = vec![(cursor, true)];
            let edits = code_indentation_edits(&buffer, &[cursor], Some("    "), 4);
            assert_eq!(edits.len(), 3);
            buffer.begin_edit_recording();
            table_cell_list_edit::apply_prefix_edits(&mut buffer, &mut cursors, edits);
            let undo = buffer.finish_edit_recording();
            assert_eq!(buffer.text(), "    α\r\n    beta\r\n     γ\r\nend");
            let selected = cursors[0].0;
            assert_eq!(selected.offset < selected.anchor, reverse);
            assert_eq!(selected.selection_range().start, 4);
            assert_eq!(selected.selection_range().end, buffer.line_start(3));
            assert!(buffer.apply_edits_inverse(&undo));
            assert_eq!(buffer.text(), source);
            assert!(buffer.apply_edits_forward(&undo));
            let edits = code_indentation_edits(&buffer, &[selected], None, 4);
            table_cell_list_edit::apply_prefix_edits(&mut buffer, &mut cursors, edits);
            assert_eq!(buffer.text(), source);
            assert_eq!(cursors[0].0, cursor);
        }
    }

    #[test]
    fn multiline_indent_invalidates_each_changed_rendered_row() {
        let mut buffer = Buffer::from_text("alpha\n  beta\nγ\nunchanged");
        let end = buffer.line_start(3);
        let cursor = Cursor {
            anchor: 0,
            offset: end,
        };
        let mut cursors = vec![(cursor, true)];
        let mut cache: LineDisplayCache = (0..buffer.line_count())
            .map(|line| {
                let mut info = super::super::tests::test_line_display_info(&[20.0], 0.0);
                info.display_text = buffer.line_content(line).to_string();
                Some(Box::new(info))
            })
            .collect();
        let old_cache = cache.clone();
        let edits = code_indentation_edits(&buffer, &[cursor], Some("  "), 2);
        let lines = edits
            .iter()
            .map(|edit| buffer.offset_to_line_col(edit.range.start).0)
            .collect::<Vec<_>>();
        super::super::invalidate_cached_line_entries(&mut cache, &lines);
        table_cell_list_edit::apply_prefix_edits(&mut buffer, &mut cursors, edits);
        assert_eq!(buffer.text(), "  alpha\n    beta\n  γ\nunchanged");
        for line in 0..3 {
            assert!(cache[line].is_none());
            assert!(!super::super::element::non_markdown_cached_line_can_reuse(
                old_cache[line].as_ref().unwrap(),
                None,
                buffer.line_content(line),
            ));
        }
        assert!(super::super::element::non_markdown_cached_line_can_reuse(
            cache[3].as_ref().unwrap(),
            None,
            buffer.line_content(3),
        ));
    }

    #[test]
    fn code_indentation_merges_overlaps_and_includes_unselected_carets() {
        let mut buffer = Buffer::from_text("\tfirst\n  second\nthird\nfourth");
        let first = Cursor {
            anchor: 0,
            offset: buffer.line_start(2),
        };
        let second = Cursor {
            anchor: buffer.line_start(1),
            offset: buffer.line_start(2) + 2,
        };
        let third = Cursor {
            anchor: buffer.line_start(3),
            offset: buffer.line_start(3),
        };
        let mut cursors = vec![(first, true), (second, false), (third, false)];
        let edits = code_indentation_edits(&buffer, &[first, second, third], None, 4);
        assert_eq!(edits.len(), 2);
        table_cell_list_edit::apply_prefix_edits(&mut buffer, &mut cursors, edits);
        assert_eq!(buffer.text(), "first\nsecond\nthird\nfourth");
        let edits = code_indentation_edits(
            &buffer,
            &cursors
                .iter()
                .map(|(cursor, _)| *cursor)
                .collect::<Vec<_>>(),
            Some("\t"),
            4,
        );
        assert_eq!(edits.len(), 4);
        table_cell_list_edit::apply_prefix_edits(&mut buffer, &mut cursors, edits);
        assert_eq!(buffer.text(), "\tfirst\n\tsecond\n\tthird\n\tfourth");
        assert_eq!(cursors.len(), 3);
    }

    #[test]
    fn code_unindent_removes_one_tab_or_the_configured_number_of_spaces() {
        assert_eq!(code_unindent_width("\t  text", 4), 1);
        assert_eq!(code_unindent_width("    text", 2), 2);
        assert_eq!(code_unindent_width("  text", 4), 2);
        assert_eq!(code_unindent_width("text", 4), 0);
    }

    #[test]
    fn multi_cursor_join_merges_overlapping_lines_and_maps_all_cursors() {
        let source = "one\r\n two\r\nthree\r\n four\r\nfive";
        let mut buffer = Buffer::from_text(source);
        let mut cursors = vec![
            (
                Cursor {
                    offset: 1,
                    anchor: 1,
                },
                true,
            ),
            (
                Cursor {
                    offset: 7,
                    anchor: 7,
                },
                false,
            ),
            (
                Cursor {
                    offset: 21,
                    anchor: 21,
                },
                false,
            ),
        ];
        let spans = selected_line_spans(
            &buffer,
            &cursors
                .iter()
                .map(|(cursor, _)| *cursor)
                .collect::<Vec<_>>(),
            true,
        );
        assert_eq!(spans, vec![(0, 2), (3, 4)]);
        let edits: Vec<_> = spans
            .into_iter()
            .map(|(start, end)| joined_lines_edit(&buffer, start, end))
            .collect();
        buffer.begin_edit_recording();
        for edit in edits.into_iter().rev() {
            for (cursor, _) in &mut cursors {
                edit.remap_cursor(cursor);
            }
            buffer.replace(edit.range, &edit.text);
        }
        assert_eq!(buffer.text(), "one two three\r\n four five");
        assert_eq!(cursors[0].0.offset, 1);
        assert!(
            cursors
                .iter()
                .all(|(cursor, _)| buffer.text().is_char_boundary(cursor.offset))
        );
        let history = buffer.finish_edit_recording();
        assert!(buffer.apply_edits_inverse(&history));
        assert_eq!(buffer.text(), source);
    }

    #[test]
    fn multi_cursor_comment_targets_deduplicate_shared_lines() {
        let buffer = Buffer::from_text("a\nb\nc\nd");
        let cursors = [
            Cursor {
                anchor: 0,
                offset: 4,
            },
            Cursor {
                anchor: 2,
                offset: 2,
            },
            Cursor {
                anchor: 6,
                offset: 6,
            },
        ];
        assert_eq!(
            selected_line_spans(&buffer, &cursors, false),
            vec![(0, 1), (3, 3)]
        );
    }

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
