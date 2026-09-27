use std::ops::Range;

use gpui::{Context, Window};

use super::EditorView;
use crate::{
    buffer::Buffer,
    markdown::{self, BlockType, LineDecoration},
};

pub(crate) struct TableSource {
    range: Range<usize>,
    buffer_len: usize,
    edit_cursor: Option<usize>,
    format_pending: bool,
}

fn alignment_edits(
    text: &str,
    base: usize,
    protected_offsets: &[usize],
) -> Vec<(Range<usize>, String)> {
    let mut groups: Vec<Vec<(usize, &str, Vec<usize>)>> = vec![Vec::new()];
    let mut offset = base;
    for line in text.split('\n') {
        let pipes = markdown::table_pipe_offsets(line);
        let complete = pipes.len() >= 2
            && line[..pipes[0]].trim().is_empty()
            && line[pipes[pipes.len() - 1] + 1..].trim().is_empty();
        if complete {
            groups.last_mut().unwrap().push((offset, line, pipes));
        } else if !groups.last().unwrap().is_empty() {
            groups.push(Vec::new());
        }
        offset += line.len() + 1;
    }
    let mut edits = Vec::new();
    for rows in groups {
        let columns = rows
            .iter()
            .map(|(_, _, pipes)| pipes.len() - 1)
            .max()
            .unwrap_or(0);
        let mut widths = vec![0; columns];
        for (_, line, pipes) in &rows {
            for (column, pair) in pipes.windows(2).enumerate() {
                widths[column] = widths[column].max(crate::table_ops::cell_display_width(
                    line[pair[0] + 1..pair[1]].trim_matches(|ch| ch == ' ' || ch == '\t'),
                ));
            }
        }
        for (base, line, pipes) in rows {
            for (column, pair) in pipes.windows(2).enumerate() {
                let start = pair[0] + 1;
                let end = pair[1];
                if protected_offsets
                    .iter()
                    .any(|offset| base + start <= *offset && *offset <= base + end)
                {
                    continue;
                }
                let cell = &line[start..end];
                let content = cell.trim_matches(|ch| ch == ' ' || ch == '\t');
                let width = crate::table_ops::cell_display_width(content);
                let replacement = format!(" {content}{}", " ".repeat(widths[column] - width + 1));
                if cell != replacement {
                    edits.push((base + start..base + end, replacement));
                }
            }
        }
    }
    edits
}

fn offset_after_alignment(
    offset: usize,
    edits: &[(Range<usize>, String)],
    original: &str,
    base: usize,
) -> usize {
    let mut shift = 0isize;
    for (range, replacement) in edits {
        if offset < range.start {
            break;
        }
        let old = &original[range.start - base..range.end - base];
        if offset <= range.end {
            let content = old.trim_matches(|ch| ch == ' ' || ch == '\t');
            let content_start = old.find(content).unwrap_or(0);
            let relative = if offset - range.start <= content_start {
                1
            } else {
                (offset - range.start - content_start + 1).min(content.len() + 1)
            };
            return (range.start as isize + shift) as usize + relative;
        }
        shift += replacement.len() as isize - range.len() as isize;
    }
    (offset as isize + shift) as usize
}

fn snapshot_edit(before: &str, after: &str) -> (Range<usize>, Range<usize>) {
    let mut start = before
        .bytes()
        .zip(after.bytes())
        .take_while(|(left, right)| left == right)
        .count();
    while !before.is_char_boundary(start) || !after.is_char_boundary(start) {
        start -= 1;
    }
    let suffix = before[start..]
        .bytes()
        .rev()
        .zip(after[start..].bytes().rev())
        .take_while(|(left, right)| left == right)
        .count();
    let mut old_end = before.len() - suffix;
    let mut new_end = after.len() - suffix;
    while !before.is_char_boundary(old_end) || !after.is_char_boundary(new_end) {
        old_end += 1;
        new_end += 1;
    }
    (start..old_end, start..new_end)
}

impl TableSource {
    fn new(range: Range<usize>, buffer: &Buffer) -> Self {
        Self {
            range,
            buffer_len: buffer.len(),
            edit_cursor: None,
            format_pending: false,
        }
    }

    pub(super) fn valid_for(&self, buffer: &Buffer) -> bool {
        self.buffer_len == buffer.len()
            && self.range.start <= self.range.end
            && self.range.end <= buffer.len()
            && buffer.text().is_char_boundary(self.range.start)
            && buffer.text().is_char_boundary(self.range.end)
    }

    fn contains_cursor(&self, offset: usize) -> bool {
        self.range.start <= offset && offset <= self.range.end
    }

    pub(super) fn contains_line(&self, buffer: &Buffer, line: usize) -> bool {
        buffer.offset_to_line_col(self.range.start).0 <= line
            && line <= buffer.offset_to_line_col(self.range.end).0
    }

    #[cfg(test)]
    fn apply_edit(&mut self, replaced: &Range<usize>, inserted_len: usize) -> bool {
        let removed = replaced.end - replaced.start;
        self.range = crate::buffer::range_after_edit(&self.range, replaced, inserted_len);
        self.buffer_len = self.buffer_len - removed + inserted_len;
        true
    }

    #[cfg(test)]
    fn apply_history(&mut self, edits: &[crate::buffer::BufferEdit], undo: bool) -> bool {
        if undo {
            edits.iter().rev().all(|edit| {
                self.apply_edit(
                    &(edit.start..edit.start + edit.inserted_text.len()),
                    edit.deleted_text.len(),
                )
            })
        } else {
            edits.iter().all(|edit| {
                self.apply_edit(
                    &(edit.start..edit.start + edit.deleted_text.len()),
                    edit.inserted_text.len(),
                )
            })
        }
    }

    pub(super) fn decorations(&self, buffer: &Buffer) -> Vec<LineDecoration> {
        let mut masked = buffer.text().as_bytes().to_vec();
        for byte in &mut masked[self.range.clone()] {
            if *byte != b'\n' {
                *byte = b' ';
            }
        }
        let mut decorations = markdown::parse_decorations(std::str::from_utf8(&masked).unwrap());
        let start = buffer.offset_to_line_col(self.range.start).0;
        let end = buffer.offset_to_line_col(self.range.end).0;
        for decoration in decorations.iter_mut().take(end + 1).skip(start) {
            *decoration = LineDecoration::default();
            decoration.block_type = BlockType::CodeBlock;
            decoration.code_language = Some("markdown".to_string());
        }
        decorations
    }
}

impl EditorView {
    pub(super) fn restore_table_source_snapshot(&mut self, text: &str) -> bool {
        if self.table_source.is_none() {
            return false;
        }
        let (replaced, inserted) = snapshot_edit(self.buffer.text(), text);
        self.buffer.replace(replaced, &text[inserted]);
        self.pending_line_display_transfer = None;
        self.clear_active_tab_layout_cache();
        true
    }

    pub(super) fn format_table_source_after_edit(&mut self) {
        self.format_table_source_edit(true);
    }

    pub(super) fn format_table_source_before_ime_commit(&mut self) {
        self.format_table_source_edit(false);
    }

    fn format_table_source_edit(&mut self, record_history: bool) {
        self.reconcile_table_source_edits();
        if self.marked_range.is_some()
            || !self.extra_marked_ranges.is_empty()
            || self.ime_composition_snapshot.is_some()
        {
            return;
        }
        let Some(source) = self.table_source.as_mut() else {
            return;
        };
        if !source.format_pending {
            return;
        }
        source.format_pending = false;
        let range = source.range.clone();
        let original = self.buffer.text_for_range(range.clone()).to_string();
        let protected_offsets: Vec<_> = std::iter::once(&self.cursor)
            .chain(self.extra_cursors.iter())
            .map(|cursor| cursor.offset)
            .collect();
        let edits = alignment_edits(&original, range.start, &protected_offsets);
        if let Some(source) = self.table_source.as_mut() {
            source.format_pending = !protected_offsets.is_empty();
        }
        if edits.is_empty() {
            return;
        }
        if record_history && self.pending_history_transaction.is_none() {
            self.checkpoint();
        }
        for cursor in std::iter::once(&mut self.cursor).chain(self.extra_cursors.iter_mut()) {
            cursor.offset = offset_after_alignment(cursor.offset, &edits, &original, range.start);
            cursor.anchor = offset_after_alignment(cursor.anchor, &edits, &original, range.start);
        }
        for (range, replacement) in edits.iter().rev() {
            self.buffer.replace(range.clone(), replacement);
        }
        self.pending_line_display_transfer = None;
        self.clear_active_tab_layout_cache();
        self.reparse();
        if let Some(source) = self.table_source.as_mut() {
            source.format_pending = !protected_offsets.is_empty();
        }
        self.reset_cursor_visual_targeting();
    }

    pub(super) fn finish_table_source_history(&mut self) {
        if let Some(source) = self.table_source.as_mut() {
            source.format_pending = false;
        }
    }

    pub(super) fn show_table_source(
        &mut self,
        table_start_line: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.block_is_front_matter(table_start_line) || !self.begin_content_edit(cx) {
            return;
        }
        let Some((start, end, lines)) = self.table_lines_for_block(table_start_line) else {
            return;
        };
        let formatted = crate::table_ops::format_table_source(&lines);
        self.close_context_menu_immediately(cx);
        self.dismiss_line_format_toolbar(cx);
        self.dismiss_selection_toolbar(window, cx);
        if formatted != lines {
            self.replace_table_block(start, end, &formatted, cx);
        }
        let text_start = self.buffer.line_start(start);
        let text_end = self.buffer.line_end(start + formatted.len() - 1);
        self.table_source = Some(TableSource::new(text_start..text_end, &self.buffer));
        self.buffer.track_range(Some(text_start..text_end));
        self.pending_code_fence = None;
        self.table_boundary_caret = None;
        self.table_boundary_drag_origin = None;
        self.clear_extra_cursors();
        self.clear_marked_ranges();
        self.cursor.move_to(text_start, &self.buffer);
        self.reset_cursor_visual_targeting();
        self.clear_active_tab_layout_cache();
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
        self.request_editor_focus(cx);
        cx.notify();
    }

    pub(super) fn sync_table_source_view(&mut self) {
        self.format_table_source_after_edit();
        if self.table_source.as_ref().is_some_and(|source| {
            !source.valid_for(&self.buffer)
                || (!source.contains_cursor(self.cursor.offset)
                    && source.edit_cursor != Some(self.cursor.offset))
        }) {
            self.table_source = None;
            self.buffer.track_range(None);
            self.clear_active_tab_layout_cache();
            self.reparse();
        }
    }

    pub(super) fn reconcile_table_source_edits(&mut self) {
        if let Some(source) = &mut self.table_source {
            if let Some((range, changed)) = self.buffer.tracked_range_update() {
                source.range = range;
                source.buffer_len = self.buffer.len();
                if changed {
                    source.edit_cursor = Some(self.cursor.offset);
                    source.format_pending = true;
                }
            }
        } else {
            self.buffer.track_range(None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn align_buffer(buffer: &mut Buffer, range: Range<usize>, positions: &mut [usize]) {
        let original = buffer.text_for_range(range.clone()).to_string();
        let edits = alignment_edits(&original, range.start, &[]);
        for position in positions {
            *position = offset_after_alignment(*position, &edits, &original, range.start);
        }
        for (range, replacement) in edits.into_iter().rev() {
            buffer.replace(range, &replacement);
        }
    }

    #[test]
    fn source_tracking_covers_direct_paste_cut_and_empty_replacement() {
        let mut buffer = Buffer::from_text(TEXT);
        let original = source(&buffer).range;
        buffer.track_range(Some(original.clone()));
        let paste = "| long pasted cell | value |\n";
        buffer.insert(buffer.line_start(4), paste);
        let (range, changed) = buffer.tracked_range_update().unwrap();
        assert!(changed);
        assert_eq!(range, original.start..original.end + paste.len());
        buffer.delete(range.clone());
        let (empty, changed) = buffer.tracked_range_update().unwrap();
        assert!(changed);
        assert_eq!(empty, original.start..original.start);
        buffer.insert(empty.start, "| replacement | row |");
        let (range, _) = buffer.tracked_range_update().unwrap();
        assert_eq!(buffer.text_for_range(range), "| replacement | row |");
    }

    #[test]
    fn source_tracking_keeps_cross_boundary_edits_and_shifts_for_edits_above() {
        let mut buffer = Buffer::from_text(TEXT);
        let original = source(&buffer).range;
        buffer.track_range(Some(original.clone()));
        buffer.insert(0, "new\n");
        let (range, _) = buffer.tracked_range_update().unwrap();
        assert_eq!(range, original.start + 4..original.end + 4);
        buffer.replace(range.start - 1..range.start + 4, "pasted");
        let (changed, _) = buffer.tracked_range_update().unwrap();
        assert_eq!(changed.start, range.start - 1);
        assert_eq!(changed.end, range.end + 1);
    }

    #[test]
    fn live_source_alignment_preserves_caret_and_selection_inside_cells() {
        let text = "| a | second |\n| --- | --- |\n| much longer | tail |";
        let mut buffer = Buffer::from_text(text);
        let mut positions = [
            text.find("second").unwrap() + 3,
            text.find("tail").unwrap() + 2,
            0,
        ];
        let old_cells: Vec<_> = text
            .split('\n')
            .map(markdown::parse_table_cells_with_ranges)
            .collect();
        let first_relative = positions[0] - old_cells[0][1].1.start;
        let last_relative = positions[1] - buffer.line_start(2) - old_cells[2][1].1.start;
        let len = buffer.len();
        align_buffer(&mut buffer, 0..len, &mut positions);
        let rows: Vec<_> = buffer.text().split('\n').collect();
        let pipes: Vec<_> = rows
            .iter()
            .map(|row| markdown::table_pipe_offsets(row))
            .collect();
        assert!(pipes.iter().all(|row| row == &pipes[0]));
        let first = markdown::parse_table_cells_with_ranges(rows[0]);
        let last = markdown::parse_table_cells_with_ranges(rows[2]);
        assert_eq!(positions[0] - first[1].1.start, first_relative);
        assert_eq!(
            positions[1] - buffer.line_start(2) - last[1].1.start,
            last_relative
        );
        assert_eq!(positions[2], 0);
        assert!(alignment_edits(buffer.text(), 0, &[]).is_empty());
    }

    #[test]
    fn live_source_alignment_preserves_spaces_invalid_rows_and_escaped_pipes() {
        let text = "| a  | `x|y` |\n| --- | --- |\n| \u{4e2d}\u{6587}\\|z | b |\n\nincomplete text";
        let mut buffer = Buffer::from_text(text);
        let caret = text.find("a  ").unwrap() + 3;
        let mut positions = [caret];
        let len = buffer.len();
        align_buffer(&mut buffer, 0..len, &mut positions);
        assert_eq!(positions[0], caret - 2);
        assert!(buffer.text().starts_with("| a "));
        assert!(buffer.text().contains("`x|y`"));
        assert!(buffer.text().contains("\u{4e2d}\u{6587}\\|z"));
        assert!(buffer.text().ends_with("\n\nincomplete text"));
        assert_eq!(alignment_edits("", 0, &[]), Vec::new());
    }

    #[test]
    fn source_alignment_defers_the_active_cell_until_the_cursor_leaves() {
        let text = "| a   | b |\n| --- | --- |\n| longer | c |";
        let active_cell = text.find("a   ").unwrap() + 2;
        let deferred = alignment_edits(text, 0, &[active_cell]);
        assert!(
            deferred
                .iter()
                .all(|(range, _)| !range.contains(&active_cell))
        );
        assert!(
            alignment_edits(text, 0, &[])
                .iter()
                .any(|(range, _)| range.contains(&active_cell))
        );
    }

    #[test]
    fn live_source_alignment_removes_old_padding_without_growing() {
        let text = "|  a       | b  |\n| --- | --- |\n| long | c |";
        let mut buffer = Buffer::from_text(text);
        let mut positions = [text.find('a').unwrap()];
        let len = buffer.len();
        align_buffer(&mut buffer, 0..len, &mut positions);
        assert_eq!(&buffer.text()[positions[0]..positions[0] + 1], "a");
        assert!(buffer.text().starts_with("| a    | b"));
        let formatted = buffer.text().to_string();
        let len = buffer.len();
        align_buffer(&mut buffer, 0..len, &mut positions);
        assert_eq!(buffer.text(), formatted);
    }

    #[test]
    fn paste_and_live_alignment_undo_together_without_losing_source_range() {
        let text = "| a | b |\n| --- | --- |\n| c | d |";
        let mut buffer = Buffer::from_text(text);
        buffer.track_range(Some(0..buffer.len()));
        buffer.begin_edit_recording();
        let offset = text.find('b').unwrap();
        buffer.insert(offset, "pasted ");
        let mut positions = [offset + "pasted ".len()];
        let len = buffer.len();
        align_buffer(&mut buffer, 0..len, &mut positions);
        let formatted = buffer.text().to_string();
        let edits = buffer.finish_edit_recording();
        assert!(edits.len() > 1);
        assert!(buffer.apply_edits_inverse(&edits));
        assert_eq!(buffer.text(), text);
        assert_eq!(buffer.tracked_range_update().unwrap().0, 0..text.len());
        assert!(buffer.apply_edits_forward(&edits));
        assert_eq!(buffer.text(), formatted);
        assert_eq!(buffer.tracked_range_update().unwrap().0, 0..formatted.len());
    }

    #[test]
    fn source_snapshot_replacement_keeps_unicode_boundaries_and_range() {
        let before = "before\n| \u{4e2d}\u{6587} | row |\nafter";
        let after = "before\n| \u{4e2d}\u{56fd} pasted | row |\nafter";
        let mut buffer = Buffer::from_text(before);
        buffer.track_range(Some(buffer.line_start(1)..buffer.line_end(1)));
        let (old, new) = snapshot_edit(before, after);
        buffer.replace(old, &after[new]);
        assert_eq!(buffer.text(), after);
        assert_eq!(
            buffer.tracked_range_update().unwrap().0,
            buffer.line_start(1)..buffer.line_end(1)
        );
    }

    #[test]
    fn table_source_highlights_table_punctuation_and_inline_markdown() {
        let buffer = Buffer::from_text(TEXT);
        let decorations = source(&buffer).decorations(&buffer);
        for theme in [
            crate::theme::Theme::moeka_print_light(),
            crate::theme::Theme::moeka_print_dark(),
        ] {
            for line in [2, 3] {
                let content = buffer.line_content(line);
                let mut colors = vec![theme.text_primary; content.len()];
                let mut weights = vec![gpui::FontWeight::NORMAL; content.len()];
                let mut styles = vec![gpui::FontStyle::Normal; content.len()];
                super::super::helpers::apply_code_block_highlighting(
                    content,
                    decorations[line].code_language.as_deref().unwrap(),
                    &theme,
                    &mut colors,
                    &mut weights,
                    &mut styles,
                );
                assert_ne!(
                    colors[0], colors[1],
                    "table pipes need distinct syntax coloring"
                );
                if line == 2 {
                    assert_ne!(
                        weights[content.find('A').unwrap()],
                        gpui::FontWeight::NORMAL
                    );
                } else {
                    assert_ne!(colors[content.find('-').unwrap()], colors[1]);
                }
            }
        }
    }

    const TEXT: &str = "before\n\n| **A** | B |\n| --- | ---: |\n| C | D |\n\nafter\n\n| E | F |\n| --- | --- |\n| G | H |\n";

    fn source(buffer: &Buffer) -> TableSource {
        TableSource::new(buffer.line_start(2)..buffer.line_end(4), buffer)
    }

    #[test]
    fn source_formatting_aligns_delimiters_and_preserves_cell_content() {
        let lines: Vec<_> = [
            "| **A** | B | C |",
            "| --- | :---: | ---: |",
            "| X\\|Y | \u{4e2d}\u{6587} | Z<br>W |",
        ]
        .into_iter()
        .map(str::to_string)
        .collect();
        let formatted = crate::table_ops::format_table_source(&lines);
        let before = crate::table_ops::parse_table_block(&lines);
        let after = crate::table_ops::parse_table_block(&formatted);
        assert_eq!(before.header_cells, after.header_cells);
        assert_eq!(before.data_rows, after.data_rows);
        assert_eq!(before.aligns, after.aligns);
        let widths: Vec<_> = formatted
            .iter()
            .map(|line| {
                line.chars()
                    .map(|character| {
                        if matches!(character, '\u{4e2d}' | '\u{6587}') {
                            2
                        } else {
                            1
                        }
                    })
                    .sum::<usize>()
            })
            .collect();
        assert!(widths.iter().all(|width| *width == widths[0]));
        assert_eq!(crate::table_ops::format_table_source(&formatted), formatted);
    }

    #[test]
    fn source_formatting_keeps_container_prefixes_intact() {
        for prefix in ["> ", "  ", "- "] {
            let lines: Vec<_> = ["| A | B |", "| --- | --- |", "| C | D |"]
                .into_iter()
                .map(|line| format!("{prefix}{line}"))
                .collect();
            assert_eq!(crate::table_ops::format_table_source(&lines), lines);
        }
    }

    #[test]
    fn table_source_projection_is_local_and_does_not_change_the_document() {
        let buffer = Buffer::from_text(TEXT);
        let source = source(&buffer);
        let decorations = source.decorations(&buffer);
        for decoration in &decorations[2..5] {
            assert_eq!(decoration.block_type, BlockType::CodeBlock);
            assert!(decoration.table_info.is_none());
            assert!(decoration.syntax_ranges.is_empty());
        }
        assert!(decorations[8].table_info.is_some());
        assert_eq!(buffer.text(), TEXT);
        assert!(
            markdown::parse_decorations(buffer.text())[2]
                .table_info
                .is_some()
        );
        assert!(source.contains_cursor(source.range.start));
        assert!(source.contains_cursor(source.range.end));
        assert!(!source.contains_cursor(source.range.start - 1));
        assert!(!source.contains_cursor(source.range.end + 1));
        assert!(source.contains_line(&buffer, 2));
        assert!(source.contains_line(&buffer, 4));
        assert!(!source.contains_line(&buffer, 5));
    }

    #[test]
    fn source_range_tracks_unicode_paste_newlines_and_deletion() {
        let mut buffer = Buffer::from_text(TEXT);
        let mut source = source(&buffer);
        let insert = buffer.line_start(4);
        let text = "| \u{4e2d}\u{6587} | X |\n";
        assert!(source.apply_edit(&(insert..insert), text.len()));
        buffer.replace(insert..insert, text);
        assert!(source.valid_for(&buffer));
        assert_eq!(source.range.end, buffer.line_end(5));
        assert_eq!(
            source.decorations(&buffer)[5].block_type,
            BlockType::CodeBlock
        );
        let removal = insert..insert + text.len();
        assert!(source.apply_edit(&removal, 0));
        buffer.replace(removal, "");
        assert!(source.valid_for(&buffer));
        assert_eq!(buffer.text(), TEXT);
        let range = source.range.clone();
        assert!(source.apply_edit(&range, 0));
        assert!(source.range.is_empty());
    }

    #[test]
    fn table_source_range_follows_undo_and_redo_transactions() {
        let mut buffer = Buffer::from_text(TEXT);
        let mut source = source(&buffer);
        let initial_range = source.range.clone();
        let edits = vec![crate::buffer::BufferEdit {
            start: buffer.line_start(4),
            deleted_text: String::new(),
            inserted_text: "| extra | row |\n".to_string(),
        }];
        assert!(buffer.apply_edits_forward(&edits));
        assert!(source.apply_history(&edits, false));
        assert!(source.valid_for(&buffer));
        assert_eq!(source.range.end, buffer.line_end(5));
        assert!(buffer.apply_edits_inverse(&edits));
        assert!(source.apply_history(&edits, true));
        assert!(source.valid_for(&buffer));
        assert_eq!(source.range, initial_range);
        assert!(buffer.apply_edits_forward(&edits));
        assert!(source.apply_history(&edits, false));
        assert!(source.valid_for(&buffer));
    }

    #[test]
    fn source_edits_stay_literal_even_when_table_syntax_is_temporarily_invalid() {
        let mut buffer = Buffer::from_text(TEXT);
        let mut source = source(&buffer);
        let range = buffer.line_start(3)..buffer.line_end(3);
        assert!(source.apply_edit(&range, 3));
        buffer.replace(range, "```");
        let decorations = source.decorations(&buffer);
        assert_eq!(decorations[3].block_type, BlockType::CodeBlock);
        assert_eq!(decorations[6].block_type, BlockType::Normal);
        assert!(decorations[8].table_info.is_some());
        assert!(source.apply_edit(&(0..0), 1));
    }
}
