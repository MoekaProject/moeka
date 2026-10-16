//! Source-coordinate policies shared by rendered-cell keyboard interactions.
use std::ops::Range;

use crate::{buffer::Buffer, cursor::Cursor, markdown::LineDecoration};

use super::{helpers, table_cell_edit::cell_range_for_selection};

pub(super) fn adjacent_cell_break(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    rendered_markdown: bool,
    offset: usize,
    backwards: bool,
) -> Option<Range<usize>> {
    let adjacent = if backwards {
        buffer.text().get(..offset)?.ends_with('>')
    } else {
        buffer.text().get(offset..)?.starts_with('<')
    };
    if !adjacent {
        return None;
    }
    let cell = cell_range_for_selection(buffer, decorations, rendered_markdown, &(offset..offset))?;
    let (line, _) = buffer.offset_to_line_col(offset);
    let line_start = buffer.line_start(line);
    let decoration = helpers::table_cell_decoration(
        decorations.get(line)?,
        buffer.text_for_range(cell.clone()),
        &(cell.start - line_start..cell.end - line_start),
    );
    // The renderer owns break acceptance, including escaping and opaque spans.
    decoration
        .display_replacements
        .iter()
        .find_map(|replacement| {
            let range = cell.start + replacement.source_range.start
                ..cell.start + replacement.source_range.end;
            (replacement.display_text == "\n"
                && range.start >= cell.start
                && range.end <= cell.end
                && if backwards {
                    range.end == offset
                } else {
                    range.start == offset
                })
            .then_some(range)
        })
}

pub(super) fn cell_horizontal_target(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    rendered_markdown: bool,
    offset: usize,
    forward: bool,
) -> Option<usize> {
    let cell = cell_range_for_selection(buffer, decorations, rendered_markdown, &(offset..offset))?;
    if (forward && offset == cell.end) || (!forward && offset == cell.start) {
        return None;
    }
    if let Some(range) =
        adjacent_cell_break(buffer, decorations, rendered_markdown, offset, !forward)
    {
        return Some(if forward { range.end } else { range.start });
    }
    Some(if forward {
        buffer.next_grapheme_boundary(offset).min(cell.end)
    } else {
        buffer.prev_grapheme_boundary(offset).max(cell.start)
    })
}

pub(super) fn cell_edge_target(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    rendered_markdown: bool,
    offset: usize,
    start: bool,
) -> Option<usize> {
    let cell = cell_range_for_selection(buffer, decorations, rendered_markdown, &(offset..offset))?;
    Some(if start { cell.start } else { cell.end })
}

pub(super) fn move_cursor_to_edge(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    rendered_markdown: bool,
    cursor: &mut Cursor,
    start: bool,
    select: bool,
) {
    let target = cell_edge_target(buffer, decorations, rendered_markdown, cursor.offset, start)
        .unwrap_or_else(|| {
            let (line, _) = buffer.offset_to_line_col(cursor.offset);
            if start {
                buffer.line_start(line)
            } else {
                buffer.line_end(line)
            }
        });
    if select {
        cursor.select_to(target, buffer);
    } else {
        cursor.move_to(target, buffer);
    }
}

pub(super) fn block_format_allowed(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    rendered_markdown: bool,
    selection: &Range<usize>,
    cell_local_list: bool,
) -> bool {
    if !rendered_markdown {
        return true;
    }
    if buffer.text().get(selection.clone()).is_none() {
        return false;
    }
    let (first, _) = buffer.offset_to_line_col(selection.start);
    let last_offset = selection
        .end
        .saturating_sub(usize::from(!selection.is_empty()));
    let (last, _) = buffer.offset_to_line_col(last_offset);
    let touches_table = (first..=last).any(|line| {
        decorations
            .get(line)
            .and_then(|row| row.table_info.as_ref())
            .is_some_and(|info| !info.is_front_matter)
    });
    if !touches_table {
        return true;
    }
    if !cell_local_list {
        return false;
    }
    let Some(cell) = cell_range_for_selection(buffer, decorations, true, selection) else {
        return false;
    };
    let line_start = buffer.line_start(first);
    let decoration = helpers::table_cell_decoration(
        &decorations[first],
        buffer.text_for_range(cell.clone()),
        &(cell.start - line_start..cell.end - line_start),
    );
    decoration.table_cell_fragments.iter().any(|fragment| {
        cell.start + fragment.range.start <= selection.start
            && selection.end <= cell.start + fragment.range.end
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::parse_decorations;

    fn fixture(content: &str) -> (Buffer, Vec<LineDecoration>, usize) {
        let text = format!("before\n\n| A | B |\n| --- | --- |\n| other | {content} |\n\nafter");
        let buffer = Buffer::from_text(&text);
        let decorations = parse_decorations(&text);
        let info = decorations[4].table_info.as_ref().unwrap();
        let start = buffer.line_start(4) + info.cell_ranges[1].start;
        (buffer, decorations, start)
    }

    #[test]
    fn accepted_breaks_share_atomic_movement_and_deletion_ranges() {
        for tag in ["<br>", "<BR/>", "<br />", "<Br   />", "<br\t/>"] {
            let content = format!("先🙂{tag}後");
            let (buffer, decorations, start) = fixture(&content);
            let left = start + "先🙂".len();
            let right = left + tag.len();
            assert_eq!(
                cell_horizontal_target(&buffer, &decorations, true, left, true),
                Some(right),
                "{tag}"
            );
            assert_eq!(
                cell_horizontal_target(&buffer, &decorations, true, right, false),
                Some(left),
                "{tag}"
            );
            for (offset, backwards) in [(left, false), (right, true)] {
                let range =
                    adjacent_cell_break(&buffer, &decorations, true, offset, backwards).unwrap();
                assert_eq!(range, left..right);
                let mut edited = Buffer::from_text(buffer.text());
                edited.delete(range);
                assert_eq!(edited.text(), buffer.text().replace(tag, ""));
            }
        }
    }

    #[test]
    fn literal_breaks_keep_grapheme_movement_and_single_character_deletion() {
        for content in [
            r"a\<br>b",
            "`a<br>b`",
            "[[a<br>b]]",
            "$a<br>b$",
            "^a<br>b^",
            "~a<br>b~",
            "<code>a<br>b</code>",
            "lead <pre>a<br>b</pre>",
            "lead <script>a<br>b</script>",
            "lead <style>a<br>b</style>",
        ] {
            let (buffer, decorations, start) = fixture(content);
            let left = start + content.find("<br>").unwrap();
            let right = left + 4;
            assert_eq!(
                adjacent_cell_break(&buffer, &decorations, true, left, false),
                None,
                "{content}"
            );
            assert_eq!(
                adjacent_cell_break(&buffer, &decorations, true, right, true),
                None,
                "{content}"
            );
            assert_eq!(
                cell_horizontal_target(&buffer, &decorations, true, left, true),
                Some(left + 1),
                "{content}"
            );
            assert_eq!(
                cell_horizontal_target(&buffer, &decorations, true, right, false),
                Some(right - 1),
                "{content}"
            );
            for (offset, backwards, deleted) in [(left, false, left), (right, true, right - 1)] {
                let mut edited = Buffer::from_text(buffer.text());
                let mut cursor = Cursor {
                    offset,
                    anchor: offset,
                };
                if backwards {
                    cursor.backspace(&mut edited);
                } else {
                    cursor.delete(&mut edited);
                }
                let mut expected = buffer.text().to_string();
                expected.remove(deleted);
                assert_eq!(edited.text(), expected, "{content}");
            }
        }
    }

    #[test]
    fn only_accepted_breaks_are_atomic_in_a_mixed_cell() {
        let content = r"`a<br>b`<BR />c\<br>d<br>e";
        let (buffer, decorations, start) = fixture(content);
        for (local, tag) in [
            (content.find("<BR />").unwrap(), "<BR />"),
            (content.rfind("<br>").unwrap(), "<br>"),
        ] {
            assert_eq!(
                adjacent_cell_break(&buffer, &decorations, true, start + local, false),
                Some(start + local..start + local + tag.len())
            );
        }
        assert_eq!(
            adjacent_cell_break(
                &buffer,
                &decorations,
                true,
                start + content.find("<br>").unwrap(),
                false
            ),
            None
        );
    }

    #[test]
    fn source_mode_and_non_table_text_do_not_gain_atomic_breaks_or_cell_edges() {
        let (buffer, decorations, start) = fixture("a<br>b");
        assert_eq!(
            adjacent_cell_break(&buffer, &decorations, false, start + 1, false),
            None
        );
        assert_eq!(
            cell_horizontal_target(&buffer, &decorations, false, start + 1, true),
            None
        );
        assert_eq!(
            cell_edge_target(&buffer, &decorations, false, start + 1, true),
            None
        );
        let plain = Buffer::from_text("a<br>b");
        assert_eq!(
            adjacent_cell_break(&plain, &parse_decorations(plain.text()), true, 1, false),
            None
        );
    }

    #[test]
    fn home_end_and_selection_share_cell_edges_and_preserve_each_anchor() {
        let (buffer, decorations, start) = fixture("東京🙂<br>last  ");
        let range = cell_range_for_selection(&buffer, &decorations, true, &(start..start)).unwrap();
        for to_start in [false, true] {
            let target = if to_start { range.start } else { range.end };
            for anchor in [start, start + "東京🙂<br>".len(), range.end] {
                let original = Cursor {
                    offset: start + "東京".len(),
                    anchor,
                };
                let mut selected = original;
                move_cursor_to_edge(&buffer, &decorations, true, &mut selected, to_start, true);
                assert_eq!(
                    selected,
                    Cursor {
                        offset: target,
                        anchor
                    }
                );
                let mut moved = original;
                move_cursor_to_edge(&buffer, &decorations, true, &mut moved, to_start, false);
                assert_eq!(
                    moved,
                    Cursor {
                        offset: target,
                        anchor: target
                    }
                );
            }
        }
    }

    #[test]
    fn mixed_primary_and_extra_cursors_use_their_own_cell_or_line_edges() {
        let (buffer, decorations, start) = fixture("beta");
        let first =
            buffer.line_start(4) + decorations[4].table_info.as_ref().unwrap().cell_ranges[0].start;
        let cursors = [
            Cursor {
                offset: start + 2,
                anchor: start + 1,
            },
            Cursor {
                offset: first + 2,
                anchor: first,
            },
            Cursor {
                offset: 3,
                anchor: 2,
            },
        ];
        for to_start in [false, true] {
            for select in [false, true] {
                let mut actual = cursors;
                for cursor in &mut actual {
                    move_cursor_to_edge(&buffer, &decorations, true, cursor, to_start, select);
                }
                for (index, original) in cursors.iter().enumerate() {
                    let target =
                        cell_edge_target(&buffer, &decorations, true, original.offset, to_start)
                            .unwrap_or(if to_start { 0 } else { "before".len() });
                    assert_eq!(actual[index].offset, target);
                    assert_eq!(
                        actual[index].anchor,
                        if select { original.anchor } else { target }
                    );
                }
            }
        }
    }

    #[test]
    fn source_mode_home_selection_uses_the_physical_line() {
        let (buffer, decorations, start) = fixture("beta");
        let mut cursor = Cursor {
            offset: start + 2,
            anchor: start + 1,
        };
        move_cursor_to_edge(&buffer, &decorations, false, &mut cursor, true, true);
        assert_eq!(cursor.offset, buffer.line_start(4));
        assert_eq!(cursor.anchor, start + 1);
    }

    #[test]
    fn block_format_policy_rejects_row_transforms_but_keeps_local_lists() {
        let (buffer, decorations, start) = fixture("first<br>- [ ] second");
        for selection in [start..start, start..start + 5, start + 10..start + 14] {
            assert!(!block_format_allowed(
                &buffer,
                &decorations,
                true,
                &selection,
                false
            ));
            assert!(block_format_allowed(
                &buffer,
                &decorations,
                true,
                &selection,
                true
            ));
        }
        for selection in [
            start..start + 10,
            buffer.line_start(4)..start + 2,
            0..start + 2,
        ] {
            assert!(!block_format_allowed(
                &buffer,
                &decorations,
                true,
                &selection,
                false
            ));
            assert!(!block_format_allowed(
                &buffer,
                &decorations,
                true,
                &selection,
                true
            ));
        }
        for line in [2, 3, 4] {
            let offset = buffer.line_start(line);
            assert!(!block_format_allowed(
                &buffer,
                &decorations,
                true,
                &(offset..offset),
                false
            ));
        }
        assert!(block_format_allowed(
            &buffer,
            &decorations,
            true,
            &(0..0),
            false
        ));
        assert!(block_format_allowed(
            &buffer,
            &decorations,
            true,
            &(0..buffer.line_start(2)),
            false
        ));
        assert!(block_format_allowed(
            &buffer,
            &decorations,
            false,
            &(start..start),
            false
        ));
    }

    #[test]
    fn front_matter_keeps_ordinary_edge_and_format_policies() {
        let buffer = Buffer::from_text("---\ntitle: a<br>b\n---\nbody");
        let decorations = parse_decorations(buffer.text());
        let offset = buffer.line_start(1) + "title: a".len();
        assert_eq!(
            adjacent_cell_break(&buffer, &decorations, true, offset, false),
            None
        );
        assert_eq!(
            cell_edge_target(&buffer, &decorations, true, offset, true),
            None
        );
        assert!(block_format_allowed(
            &buffer,
            &decorations,
            true,
            &(offset..offset),
            false
        ));
        let mut cursor = Cursor {
            offset,
            anchor: offset + 1,
        };
        move_cursor_to_edge(&buffer, &decorations, true, &mut cursor, true, true);
        assert_eq!(cursor.offset, buffer.line_start(1));
        assert_eq!(cursor.anchor, offset + 1);
    }
}
