//! Remove empty YAML continuation rows atomically rather than erasing their
//! indentation, list marker and quotes one source character at a time.
use std::ops::Range;

use gpui::Context;

use crate::buffer::Buffer;
use crate::markdown::LineDecoration;

use super::EditorView;
use super::front_matter_groups::{self, GroupKind};

#[derive(Debug, PartialEq, Eq)]
pub(super) struct BackspaceEdit {
    pub range: Range<usize>,
    pub replacement: String,
    /// Absolute UTF-8 offset after applying the replacement.
    pub cursor: usize,
}

pub(super) fn backspace_edit(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    selection: Range<usize>,
) -> Option<BackspaceEdit> {
    if !selection.is_empty()
        || selection.start > buffer.len()
        || !buffer.text().is_char_boundary(selection.start)
    {
        return None;
    }
    let line = buffer.offset_to_line_col(selection.start).0;
    let source = buffer.line_content(line).trim_end_matches('\r');
    let column = selection.start - buffer.line_start(line);
    if let Some(info) = decorations
        .get(line)
        .and_then(|row| row.front_matter.as_ref())
        && let Some(key) = info.key_range.as_ref()
        && column == key.end + 2
        && source.get(key.end..column) == Some(": ")
    {
        return Some(BackspaceEdit {
            range: buffer.line_start(line) + key.end..selection.start,
            replacement: String::new(),
            cursor: buffer.line_start(line) + key.end,
        });
    }
    let groups = front_matter_groups::groups(buffer, decorations);
    let group = groups.iter().find(|group| {
        if line <= group.header_line {
            return false;
        }
        if group.source_lines.contains(&line) {
            return true;
        }
        // Presentation excludes trailing plain-scalar trivia. Enter-created
        // indented blank rows still belong to the preceding continuation, but
        // a dedented blank or intervening sibling/comment must not be claimed.
        let required = (group.header_line + 1..group.source_lines.end)
            .map(|row| buffer.line_content(row).trim_end_matches('\r'))
            .find(|row| !row.trim().is_empty())
            .map(|row| row.len() - row.trim_start_matches(' ').len());
        group.kind == GroupKind::Multiline
            && line >= group.source_lines.end
            && decorations
                .get(line)
                .is_some_and(|row| row.front_matter.is_some())
            && required.is_some_and(|indent| indent > 0 && source.len() >= indent)
            && (group.source_lines.end..=line).all(|row| {
                buffer
                    .line_content(row)
                    .trim_end_matches('\r')
                    .trim_matches(' ')
                    .is_empty()
            })
    })?;
    let (range, replacement) = match group.kind {
        GroupKind::Multiline if source.trim_matches(' ').is_empty() => (
            buffer.line_start(line)..buffer.line_start(line + 1),
            String::new(),
        ),
        GroupKind::List => {
            let index = group
                .items
                .iter()
                .position(|item| item.source_line == line)?;
            let item = &group.items[index];
            let token = buffer.text_for_range(item.source_range.clone());
            // An explicit null or whitespace-containing string is still source
            // content. Inline comments must not disappear with an empty item.
            if !matches!(token, "" | "\"\"" | "''")
                || !source[item.source_range.end - buffer.line_start(line)..]
                    .trim()
                    .is_empty()
                || !source.trim_start_matches(' ').starts_with('-')
            {
                return None;
            }
            front_matter_groups::remove_list_item_edit(
                buffer,
                decorations,
                group.header_line,
                index,
            )?
        }
        _ => return None,
    };
    let mut updated = Buffer::from_text(buffer.text());
    updated.replace(range.clone(), &replacement);
    let previous = updated.line_content(line - 1).trim_end_matches('\r');
    Some(BackspaceEdit {
        range,
        replacement,
        cursor: updated.line_start(line - 1) + previous.len(),
    })
}

impl EditorView {
    pub(super) fn backspace_empty_front_matter_row(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.active_tab_is_markdown() || self.has_extra_cursors() {
            return false;
        }
        let Some(edit) = backspace_edit(
            &self.buffer,
            &self.decorations,
            self.cursor.selection_range(),
        ) else {
            return false;
        };
        self.capture_typing_scroll_guard();
        self.checkpoint();
        self.queue_non_markdown_line_display_transfer(&edit.range, &edit.replacement);
        self.buffer.replace(edit.range.clone(), &edit.replacement);
        super::remap_source_edit_positions(
            &mut self.cursor,
            &mut self.extra_cursors,
            &mut self.marked_range,
            &mut self.extra_marked_ranges,
            &mut self.wrap_boundary_affinity,
            &edit.range,
            edit.replacement.len(),
        );
        self.cursor.move_to(edit.cursor, &self.buffer);
        self.reset_cursor_visual_targeting();
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
        cx.notify();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cursor::Cursor;
    use crate::markdown::parse_decorations;

    #[test]
    fn front_matter_backspace_deletes_colon_space_in_one_step() {
        for key in ["title", "\u{5206}\u{7c7b}", "\"quoted key\""] {
            let source = format!("---\n{key}: value\n---");
            let mut buffer = Buffer::from_text(&source);
            let column = buffer.line_start(1) + key.len() + 2;
            let edit =
                backspace_edit(&buffer, &parse_decorations(&source), column..column).unwrap();
            buffer.replace(edit.range, &edit.replacement);
            assert_eq!(buffer.text(), format!("---\n{key}value\n---"));
            assert_eq!(edit.cursor, 4 + key.len());
        }
    }

    fn erased(source: &str, line: usize, column: usize) -> (Buffer, usize) {
        let mut buffer = Buffer::from_text(source);
        let offset = buffer.line_start(line) + column;
        let edit = backspace_edit(&buffer, &parse_decorations(source), offset..offset)
            .unwrap_or_else(|| panic!("no empty-row edit: {source:?}"));
        assert!(source.is_char_boundary(edit.range.start));
        assert!(source.is_char_boundary(edit.range.end));
        buffer.replace(edit.range, &edit.replacement);
        assert!(buffer.text().is_char_boundary(edit.cursor));
        (buffer, edit.cursor)
    }

    #[test]
    fn front_matter_backspace_empty_multiline_row_skips_all_indentation() {
        for column in 0..=4 {
            let (buffer, cursor) = erased(
                "---\ntext: |-\n    第一行🙂\n    \n    next\nsibling: keep\n---\nBody",
                3,
                column,
            );
            assert_eq!(
                buffer.text(),
                "---\ntext: |-\n    第一行🙂\n    next\nsibling: keep\n---\nBody"
            );
            assert_eq!(cursor, buffer.line_end(2));
        }
    }

    #[test]
    fn front_matter_backspace_empty_first_body_returns_to_header() {
        let (buffer, cursor) = erased("---\ntext: |2- # header\n  \nsibling: keep\n---", 2, 2);
        assert_eq!(buffer.text(), "---\ntext: |2- # header\nsibling: keep\n---");
        assert_eq!(cursor, buffer.line_end(1));
    }

    #[test]
    fn front_matter_backspace_empty_list_item_skips_marker_quotes_and_spacing() {
        for row in ["  -", "  - ", "  -     ", "  - \"\"", "  - ''"] {
            let source =
                format!("---\nlist:\n  - \"first🙂\"\n{row}\n  - next\nsibling: keep\n---");
            let (buffer, cursor) = erased(&source, 3, row.len());
            assert_eq!(
                buffer.text(),
                "---\nlist:\n  - \"first🙂\"\n  - next\nsibling: keep\n---"
            );
            assert_eq!(cursor, buffer.line_end(2));
        }
    }

    #[test]
    fn front_matter_backspace_last_empty_list_item_retains_list_type_and_comments() {
        let (buffer, cursor) = erased(
            "---\nlist: # header\n  # keep\n  - \nsibling: keep\n---",
            3,
            4,
        );
        assert_eq!(
            buffer.text(),
            "---\nlist: [] # header\n  # keep\nsibling: keep\n---"
        );
        assert_eq!(cursor, buffer.line_end(2));
        let (buffer, cursor) = erased("---\nlist:\n  - \n---", 2, 4);
        assert_eq!(buffer.text(), "---\nlist: []\n---");
        assert_eq!(cursor, buffer.line_end(1));
    }

    #[test]
    fn front_matter_backspace_empty_row_preserves_crlf_and_source_byte_positions() {
        for source in [
            "---\r\ntext: |-\r\n  🙂\r\n  \r\nsibling: keep\r\n---",
            "---\r\nlist:\r\n  - 🙂\r\n  - \r\nsibling: keep\r\n---",
        ] {
            let (buffer, cursor) = erased(source, 3, source.lines().nth(3).unwrap().len());
            assert_eq!(buffer.line_count(), 5);
            assert!(buffer.text().contains("🙂\r\nsibling: keep"));
            assert_eq!(
                cursor,
                buffer.line_start(2) + buffer.line_content(2).trim_end_matches('\r').len()
            );
            assert_eq!(
                buffer.text().matches('\r').count(),
                buffer.text().matches('\n').count()
            );
        }
    }

    #[test]
    fn front_matter_backspace_does_not_remove_values_comments_selections_or_delimiters() {
        for row in ["  - null", "  - \" \"", "  - value", "  - # keep", "  - {}"] {
            let source = format!("---\nlist:\n{row}\n---");
            let buffer = Buffer::from_text(&source);
            let offset = buffer.line_start(2) + row.len();
            assert!(
                backspace_edit(&buffer, &parse_decorations(&source), offset..offset).is_none(),
                "{row}"
            );
        }
        let source = "---\ntext: |-\n  \n---";
        let buffer = Buffer::from_text(source);
        let decorations = parse_decorations(source);
        for range in [
            0..0,
            buffer.line_end(1)..buffer.line_end(1),
            buffer.line_start(2)..buffer.line_end(2),
            buffer.line_end(3)..buffer.line_end(3),
        ] {
            assert!(backspace_edit(&buffer, &decorations, range).is_none());
        }
        let buffer = Buffer::from_text("outside\n  ");
        assert!(
            backspace_edit(
                &buffer,
                &parse_decorations(buffer.text()),
                buffer.len()..buffer.len()
            )
            .is_none()
        );
    }

    #[test]
    fn front_matter_backspace_after_enter_removes_trailing_plain_continuation() {
        for eol in ["\n", "\r\n"] {
            for indent in [2, 4, 6] {
                let source = [
                    "---".to_owned(),
                    "value: first".into(),
                    format!("{}second🙂", " ".repeat(indent)),
                    "next: ok".into(),
                    "---".into(),
                ]
                .join(eol);
                let mut buffer = Buffer::from_text(&source);
                let before =
                    buffer.line_start(2) + buffer.line_content(2).trim_end_matches('\r').len();
                let enter = super::super::front_matter_enter::enter_edit(
                    &buffer,
                    &parse_decorations(&source),
                    before..before,
                )
                .unwrap();
                let caret = enter.range.start + enter.cursor;
                buffer.replace(enter.range, &enter.replacement);
                let edit = backspace_edit(&buffer, &parse_decorations(buffer.text()), caret..caret)
                    .unwrap();
                buffer.replace(edit.range, &edit.replacement);
                assert_eq!(buffer.text(), source);
                assert_eq!(edit.cursor, before);
            }
        }
        for source in [
            "---\nvalue: first\n    second\n# keep\n    \nnext: ok\n---",
            "---\nvalue: first\n    second\nnext: ok\n    \n---",
            "---\nvalue: first\n    second\n\nnext: ok\n---",
        ] {
            let buffer = Buffer::from_text(source);
            let row = source
                .lines()
                .position(|row| row.trim().is_empty())
                .unwrap();
            let caret = buffer.line_end(row);
            assert!(backspace_edit(&buffer, &parse_decorations(source), caret..caret).is_none());
        }
    }

    #[test]
    fn front_matter_backspace_empty_row_is_one_undo_edit_with_ime_remapping() {
        let source = "---\nlist:\n  - first\n  - \nsibling: keep\n---";
        let mut buffer = Buffer::from_text(source);
        let offset = buffer.line_end(3);
        let edit = backspace_edit(&buffer, &parse_decorations(source), offset..offset).unwrap();
        let mut cursor = Cursor {
            offset,
            anchor: offset,
        };
        let sibling = buffer.line_start(4);
        let mut extra = [Cursor {
            offset: sibling,
            anchor: sibling,
        }];
        let mut marked = Some(sibling..sibling + 7);
        buffer.begin_edit_recording();
        buffer.replace(edit.range.clone(), &edit.replacement);
        super::super::remap_source_edit_positions(
            &mut cursor,
            &mut extra,
            &mut marked,
            &mut [],
            &mut None,
            &edit.range,
            edit.replacement.len(),
        );
        cursor.move_to(edit.cursor, &buffer);
        assert_eq!(cursor.offset, buffer.line_end(2));
        assert_eq!(extra[0].offset, buffer.line_start(3));
        assert_eq!(
            marked.unwrap(),
            buffer.line_start(3)..buffer.line_start(3) + 7
        );
        let edits = buffer.finish_edit_recording();
        assert_eq!(edits.len(), 1);
        assert!(buffer.apply_edits_inverse(&edits));
        assert_eq!(buffer.text(), source);
    }
}
