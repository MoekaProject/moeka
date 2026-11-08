use gpui::{Context, KeyDownEvent, Window};

use crate::actions::*;
use crate::markdown::{self, BlockType};

use super::profiler::{RuntimeProfilePaths, RuntimeProfiler};
use super::table_cell_list_edit::DeleteAction;
use super::*;

fn page_down_row_fits(remaining_height: f32, step_distance: f32, first_step: bool) -> bool {
    first_step || step_distance <= remaining_height
}

fn next_unselected_occurrence(
    buffer: &Buffer,
    needle: &str,
    after: usize,
    selections: &[Range<usize>],
) -> Option<Range<usize>> {
    if needle.is_empty() {
        return None;
    }
    let text = buffer.text();
    let mut selected: Vec<_> = selections
        .iter()
        .filter(|range| !range.is_empty())
        .cloned()
        .collect();
    selected.sort_by_key(|range| (range.start, range.end));
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in selected {
        if let Some(previous) = merged.last_mut()
            && range.start <= previous.end
        {
            previous.end = previous.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    let candidates = || {
        text.match_indices(needle).filter_map(|(start, _)| {
            let end = start + needle.len();
            let first_possible = merged.partition_point(|range| range.end <= start);
            (buffer.clamp_grapheme_offset(start) == start
                && buffer.clamp_grapheme_offset(end) == end
                && merged
                    .get(first_possible)
                    .is_none_or(|range| range.start >= end))
            .then_some(start..end)
        })
    };
    candidates()
        .find(|range| range.start >= after)
        .or_else(|| candidates().find(|range| range.start < after))
}

fn select_next_occurrence_for_cursors(
    buffer: &Buffer,
    needle: &str,
    cursors: &mut Vec<Cursor>,
) -> Option<Range<usize>> {
    for cursor in cursors.iter_mut() {
        let range = cursor.selection_range();
        if !range.is_empty() && buffer.text_for_range(range.clone()) == needle {
            cursor.anchor = range.start;
            cursor.offset = range.end;
        }
    }
    let mut seen = std::collections::HashSet::new();
    cursors.retain(|cursor| seen.insert((cursor.offset, cursor.anchor)));

    let primary = cursors.first()?;
    let selections: Vec<_> = cursors.iter().map(Cursor::selection_range).collect();
    let range =
        next_unselected_occurrence(buffer, needle, primary.selection_range().end, &selections)?;
    cursors.insert(
        0,
        Cursor {
            offset: range.end,
            anchor: range.start,
        },
    );
    Some(range)
}

#[cfg(test)]
#[test]
fn repeated_occurrence_selection_wraps_once_and_leaves_carets_on_the_right() {
    let buffer = Buffer::from_text("word word word word");
    let mut cursors = vec![Cursor {
        offset: 10,
        anchor: 14,
    }];
    for expected in [15..19, 0..4, 5..9] {
        let previous_count = cursors.len();
        assert_eq!(
            select_next_occurrence_for_cursors(&buffer, "word", &mut cursors),
            Some(expected.clone())
        );
        assert_eq!(cursors.len(), previous_count + 1);
        assert_eq!(cursors[0].selection_range(), expected);
        assert!(cursors.iter().all(|cursor| cursor.offset > cursor.anchor));
    }
    let before = cursors.clone();
    for _ in 0..3 {
        assert_eq!(
            select_next_occurrence_for_cursors(&buffer, "word", &mut cursors),
            None
        );
        assert_eq!(cursors, before);
    }
    cursors.push(Cursor {
        offset: 5,
        anchor: 9,
    });
    assert_eq!(
        select_next_occurrence_for_cursors(&buffer, "word", &mut cursors),
        None
    );
    assert_eq!(cursors, before);
}

#[cfg(test)]
#[test]
fn next_occurrence_wraps_past_selected_matches_and_respects_graphemes() {
    let buffer = Buffer::from_text("word word word word");
    let selected = vec![10..14, 15..19, 0..4];
    assert_eq!(
        next_unselected_occurrence(&buffer, "word", 4, &selected),
        Some(5..9)
    );
    assert_eq!(
        next_unselected_occurrence(&buffer, "word", 19, &selected),
        Some(5..9)
    );
    let all = vec![0..4, 5..9, 10..14, 15..19];
    assert_eq!(next_unselected_occurrence(&buffer, "word", 19, &all), None);
    let unicode = Buffer::from_text("e\u{301} e é");
    assert_eq!(
        next_unselected_occurrence(&unicode, "e", 0, &[]),
        Some(4..5)
    );
}

fn selected_logical_lines(buffer: &Buffer, cursors: &[(Cursor, bool)]) -> Vec<(Cursor, bool)> {
    let mut ranges: Vec<(Range<usize>, bool)> = cursors
        .iter()
        .map(|(cursor, primary)| {
            let selection = cursor.selection_range();
            let start = selection.start.min(buffer.len());
            let end = selection.end.min(buffer.len());
            let first = buffer.offset_to_line_col(start).0;
            let last = buffer
                .offset_to_line_col(end.saturating_sub(usize::from(end > start)))
                .0;
            let line_end = buffer.line_end(last);
            (
                buffer.line_start(first)..(line_end + usize::from(line_end < buffer.len())),
                *primary,
            )
        })
        .collect();
    ranges.sort_by_key(|(range, _)| (range.start, range.end));
    let mut merged: Vec<(Range<usize>, bool)> = Vec::new();
    for (range, primary) in ranges {
        if let Some((previous, was_primary)) = merged.last_mut()
            && (range.start < previous.end || range == *previous)
        {
            previous.end = previous.end.max(range.end);
            *was_primary |= primary;
        } else {
            merged.push((range, primary));
        }
    }
    merged
        .into_iter()
        .map(|(range, primary)| {
            (
                Cursor {
                    anchor: range.start,
                    offset: range.end,
                },
                primary,
            )
        })
        .collect()
}

#[derive(Clone, Copy)]
enum MultiLineInsertion {
    Above,
    Below,
    Duplicate,
}

fn copy_lines_at_cursors(
    buffer: &mut Buffer,
    cursors: &[(Cursor, bool)],
    upward: bool,
) -> Vec<(Cursor, bool)> {
    let groups = selected_logical_lines(buffer, cursors);
    let edits: Vec<_> = groups
        .iter()
        .map(|(cursor, _)| {
            let range = cursor.selection_range();
            let end = if range.end > range.start
                && buffer.text().as_bytes().get(range.end - 1) == Some(&b'\n')
            {
                range.end - 1
            } else {
                range.end
            };
            let text = buffer.text_for_range(range.start..end);
            if upward {
                (range.start, format!("{text}\n"))
            } else {
                (end, format!("\n{text}"))
            }
        })
        .collect();
    let resolved = cursors
        .iter()
        .map(|(cursor, primary)| {
            let start = cursor.selection_range().start;
            let group = groups
                .iter()
                .position(|(selected, _)| {
                    let range = selected.selection_range();
                    range.start <= start && (start < range.end || range.is_empty())
                })
                .expect("every cursor belongs to its selected line group");
            let shift: usize = edits[..group].iter().map(|(_, text)| text.len()).sum();
            let shift = shift + if upward { 0 } else { edits[group].1.len() };
            (
                Cursor {
                    anchor: cursor.anchor + shift,
                    offset: cursor.offset + shift,
                },
                *primary,
            )
        })
        .collect();
    for (offset, text) in edits.into_iter().rev() {
        buffer.insert(offset, &text);
    }
    resolved
}

fn multi_line_insertion_plans(
    buffer: &Buffer,
    cursors: &[(Cursor, bool)],
    action: MultiLineInsertion,
) -> Vec<(table_cell_list_edit::Edit, bool)> {
    cursors
        .iter()
        .map(|(cursor, primary)| {
            let line = buffer.offset_to_line_col(cursor.offset.min(buffer.len())).0;
            let start = buffer.line_start(line);
            let end = buffer.line_end(line);
            let (at, text, caret) = match action {
                MultiLineInsertion::Above => (start, "\n".to_string(), 0),
                MultiLineInsertion::Below => (end, "\n".to_string(), 1),
                MultiLineInsertion::Duplicate => {
                    let text = format!("\n{}", buffer.text_for_range(start..end));
                    let caret = text.len();
                    (end, text, caret)
                }
            };
            let mut edit = table_cell_list_edit::Edit::new(at..at, text);
            edit.caret = caret;
            (edit, *primary)
        })
        .collect()
}

fn transpose_plan(buffer: &Buffer, cursor: Cursor) -> Option<table_cell_list_edit::Edit> {
    if cursor.has_selection() {
        return None;
    }
    let offset = cursor.offset;
    let (line, _) = buffer.offset_to_line_col(offset);
    let line_start = buffer.line_start(line);
    let line_end = buffer.line_end(line);
    let pivot = if offset >= line_end {
        buffer.prev_grapheme_boundary(offset.min(line_end))
    } else {
        offset
    };
    let left_start = buffer.prev_grapheme_boundary(pivot);
    let right_end = buffer.next_grapheme_boundary(pivot);
    if left_start >= pivot || pivot >= right_end || left_start < line_start || right_end > line_end
    {
        return None;
    }
    let left = buffer.text_for_range(left_start..pivot);
    let right = buffer.text_for_range(pivot..right_end);
    Some(table_cell_list_edit::Edit::new(
        left_start..right_end,
        format!("{right}{left}"),
    ))
}

#[cfg(test)]
#[test]
fn multi_cursor_transpose_is_atomic_and_grapheme_aware() {
    let source = "a🙂b\nxéy";
    let mut buffer = Buffer::from_text(source);
    let cursors = [
        (
            Cursor {
                offset: 1,
                anchor: 1,
            },
            true,
        ),
        (
            Cursor {
                offset: source.find('é').unwrap(),
                anchor: source.find('é').unwrap(),
            },
            false,
        ),
    ];
    let plans: Vec<_> = cursors
        .iter()
        .map(|(cursor, primary)| (transpose_plan(&buffer, *cursor).unwrap(), *primary))
        .collect();
    buffer.begin_edit_recording();
    let resolved = table_cell_list_edit::apply(&mut buffer, &plans).unwrap();
    assert_eq!(buffer.text(), "🙂ab\néxy");
    assert_eq!(resolved.len(), 2);
    let edits = buffer.finish_edit_recording();
    assert!(buffer.apply_edits_inverse(&edits));
    assert_eq!(buffer.text(), source);

    let overlapping = [
        (
            transpose_plan(
                &buffer,
                Cursor {
                    offset: 1,
                    anchor: 1,
                },
            )
            .unwrap(),
            true,
        ),
        (
            transpose_plan(
                &buffer,
                Cursor {
                    offset: 5,
                    anchor: 5,
                },
            )
            .unwrap(),
            false,
        ),
    ];
    assert!(table_cell_list_edit::apply(&mut buffer, &overlapping).is_none());
    assert_eq!(buffer.text(), source);
}

#[cfg(test)]
#[test]
fn multi_cursor_line_selection_handles_unicode_overlap_and_end_of_file() {
    let buffer = Buffer::from_text("中🙂\r\nsecond\nthird");
    let second = buffer.line_start(1);
    let third = buffer.line_start(2);
    let cursors = [
        (
            Cursor {
                anchor: 0,
                offset: second,
            },
            false,
        ),
        (
            Cursor {
                anchor: second + 3,
                offset: second,
            },
            true,
        ),
        (
            Cursor {
                anchor: third + 2,
                offset: third + 2,
            },
            false,
        ),
        (
            Cursor {
                anchor: second + 1,
                offset: second + 1,
            },
            false,
        ),
    ];
    let selected = selected_logical_lines(&buffer, &cursors);
    assert_eq!(selected.len(), 3);
    assert_eq!(selected[0].0.selection_range(), 0..second);
    assert_eq!(selected[1].0.selection_range(), second..third);
    assert!(selected[1].1);
    assert_eq!(selected[2].0.selection_range(), third..buffer.len());
}

#[cfg(test)]
#[test]
fn multi_cursor_line_insertions_deduplicate_shared_lines_and_remap_carets() {
    let source = "中🙂\nsecond\nthird";
    for (action, expected) in [
        (MultiLineInsertion::Above, "\n中🙂\n\nsecond\nthird"),
        (MultiLineInsertion::Below, "中🙂\n\nsecond\n\nthird"),
        (
            MultiLineInsertion::Duplicate,
            "中🙂\n中🙂\nsecond\nsecond\nthird",
        ),
    ] {
        let mut buffer = Buffer::from_text(source);
        let first = buffer.line_start(0);
        let second = buffer.line_start(1);
        let cursors = [
            (
                Cursor {
                    offset: first,
                    anchor: first,
                },
                true,
            ),
            (
                Cursor {
                    offset: second,
                    anchor: second,
                },
                false,
            ),
            (
                Cursor {
                    offset: second + 1,
                    anchor: second + 1,
                },
                false,
            ),
        ];
        let plans = multi_line_insertion_plans(&buffer, &cursors, action);
        buffer.begin_edit_recording();
        let resolved = table_cell_list_edit::apply(&mut buffer, &plans).unwrap();
        assert_eq!(buffer.text(), expected);
        assert!(resolved.iter().any(|(_, primary, _)| *primary));
        assert!(
            resolved
                .iter()
                .all(|(cursor, _, _)| buffer.text().is_char_boundary(cursor.offset))
        );
        assert_eq!(buffer.finish_edit_recording().len(), 2);
    }
}

#[cfg(test)]
#[test]
fn multi_cursor_copy_lines_preserves_reverse_selections_and_deduplicates() {
    for (upward, expected) in [
        (true, "one\none\n中🙂\n中🙂\nlast"),
        (false, "one\none\n中🙂\n中🙂\nlast"),
    ] {
        let mut buffer = Buffer::from_text("one\n中🙂\nlast");
        let second = buffer.line_start(1);
        let second_end = buffer.line_end(1);
        let cursors = [
            (
                Cursor {
                    offset: 1,
                    anchor: 1,
                },
                true,
            ),
            (
                Cursor {
                    offset: second,
                    anchor: second_end,
                },
                false,
            ),
            (
                Cursor {
                    offset: second,
                    anchor: second,
                },
                false,
            ),
        ];
        let resolved = copy_lines_at_cursors(&mut buffer, &cursors, upward);
        assert_eq!(buffer.text(), expected);
        assert!(resolved[0].1);
        assert_eq!(&buffer.text()[resolved[1].0.selection_range()], "中🙂");
        assert!(resolved[1].0.offset < resolved[1].0.anchor);
        assert_eq!(resolved[2].0.offset, resolved[1].0.offset);
    }
}

fn adjacent_line_swap_edit(buffer: &Buffer, first: usize) -> Option<(Range<usize>, String)> {
    if first.checked_add(1)? >= buffer.line_count() {
        return None;
    }
    let upper = buffer.line_content(first);
    let lower = buffer.line_content(first + 1);
    let upper_text = upper.strip_suffix('\r').unwrap_or(upper);
    let lower_text = lower.strip_suffix('\r').unwrap_or(lower);
    let separator = if upper.ends_with('\r') { "\r\n" } else { "\n" };
    let suffix = if lower.ends_with('\r') { "\r" } else { "" };
    Some((
        buffer.line_start(first)..buffer.line_end(first + 1),
        format!("{lower_text}{separator}{upper_text}{suffix}"),
    ))
}

struct MultiLineMove {
    first: usize,
    last: usize,
    upward: bool,
    range: Range<usize>,
    replacement: String,
}

impl MultiLineMove {
    fn remap_line(&self, line: usize) -> usize {
        if line < self.first || line > self.last {
            line
        } else if self.upward {
            if line == self.first {
                self.last
            } else {
                line - 1
            }
        } else if line == self.last {
            self.first
        } else {
            line + 1
        }
    }
}

fn multi_line_moves(
    buffer: &Buffer,
    cursors: &[(Cursor, bool)],
    upward: bool,
) -> Vec<MultiLineMove> {
    let mut spans: Vec<_> = selected_logical_lines(buffer, cursors)
        .iter()
        .map(|(cursor, _)| {
            let range = cursor.selection_range();
            let first = buffer.offset_to_line_col(range.start).0;
            let last = buffer
                .offset_to_line_col(range.end.saturating_sub(usize::from(!range.is_empty())))
                .0;
            (first, last)
        })
        .collect();
    spans.sort_unstable();
    let mut groups: Vec<(usize, usize)> = Vec::new();
    for (first, last) in spans {
        if let Some(previous) = groups.last_mut()
            && first <= previous.1 + 1
        {
            previous.1 = previous.1.max(last);
        } else {
            groups.push((first, last));
        }
    }
    groups
        .into_iter()
        .filter_map(|(first, last)| {
            let (start, end) = if upward {
                (first.checked_sub(1)?, last)
            } else {
                let end = last.checked_add(1)?;
                (first, (end < buffer.line_count()).then_some(end)?)
            };
            let mut order: Vec<_> = (start..=end).collect();
            if upward {
                order.rotate_left(1);
            } else {
                order.rotate_right(1);
            }
            let mut replacement = String::new();
            for (position, source_line) in order.into_iter().enumerate() {
                let content = buffer.line_content(source_line);
                replacement.push_str(content.strip_suffix('\r').unwrap_or(content));
                let old_content = buffer.line_content(start + position);
                if position < end - start {
                    replacement.push_str(if old_content.ends_with('\r') {
                        "\r\n"
                    } else {
                        "\n"
                    });
                } else if old_content.ends_with('\r') {
                    replacement.push('\r');
                }
            }
            Some(MultiLineMove {
                first: start,
                last: end,
                upward,
                range: buffer.line_start(start)..buffer.line_end(end),
                replacement,
            })
        })
        .collect()
}

fn remap_moved_line(moves: &[MultiLineMove], line: usize) -> usize {
    moves
        .iter()
        .find(|movement| movement.first <= line && line <= movement.last)
        .map_or(line, |movement| movement.remap_line(line))
}

fn moved_cursor_endpoints(
    buffer: &Buffer,
    cursor: Cursor,
    moves: &[MultiLineMove],
) -> ((usize, usize, bool), (usize, usize, bool)) {
    let selection = cursor.selection_range();
    let endpoint = |offset| {
        let (line, col) = buffer.offset_to_line_col(offset);
        let after_line = !selection.is_empty() && offset == selection.end && col == 0 && line > 0;
        if after_line {
            (remap_moved_line(moves, line - 1), 0, true)
        } else {
            (remap_moved_line(moves, line), col, false)
        }
    };
    (endpoint(cursor.offset), endpoint(cursor.anchor))
}

fn moved_endpoint_offset(buffer: &Buffer, (line, col, after_line): (usize, usize, bool)) -> usize {
    if after_line {
        if line + 1 < buffer.line_count() {
            buffer.line_start(line + 1)
        } else {
            buffer.len()
        }
    } else {
        buffer.clamp_grapheme_offset(buffer.line_col_to_offset(line, col))
    }
}

#[cfg(test)]
#[test]
fn multi_cursor_line_moves_preserve_selection_direction_crlf_and_undo() {
    let source = "a\r\nbb\r\n中\r\nd\r\ne";
    for (upward, expected) in [
        (true, "bb\r\na\r\nd\r\n中\r\ne"),
        (false, "a\r\n中\r\nbb\r\ne\r\nd"),
    ] {
        let mut buffer = Buffer::from_text(source);
        let cursors = [
            (
                Cursor {
                    offset: buffer.line_start(1),
                    anchor: buffer.line_start(2),
                },
                true,
            ),
            (
                Cursor {
                    offset: buffer.line_start(3),
                    anchor: buffer.line_start(3),
                },
                false,
            ),
        ];
        let moves = multi_line_moves(&buffer, &cursors, upward);
        let mapped: Vec<_> = cursors
            .iter()
            .map(|(cursor, _)| moved_cursor_endpoints(&buffer, *cursor, &moves))
            .collect();
        buffer.begin_edit_recording();
        for movement in moves.iter().rev() {
            buffer.replace(movement.range.clone(), &movement.replacement);
        }
        assert_eq!(buffer.text(), expected);
        let first = Cursor {
            offset: moved_endpoint_offset(&buffer, mapped[0].0),
            anchor: moved_endpoint_offset(&buffer, mapped[0].1),
        };
        assert_eq!(buffer.text_for_range(first.selection_range()), "bb\r\n");
        assert!(first.offset < first.anchor);
        let second = moved_endpoint_offset(&buffer, mapped[1].0);
        assert_eq!(
            buffer
                .line_content(buffer.offset_to_line_col(second).0)
                .trim_end_matches('\r'),
            "d"
        );
        let edits = buffer.finish_edit_recording();
        assert!(buffer.apply_edits_inverse(&edits));
        assert_eq!(buffer.text(), source);
    }
}

#[cfg(test)]
#[test]
fn single_multiline_selection_moves_and_copies_whole_block() {
    let source = "head\r\none\r\n二\r\ntail";
    for upward in [true, false] {
        let mut buffer = Buffer::from_text(source);
        let cursor = Cursor {
            offset: buffer.line_start(1),
            anchor: buffer.line_start(3),
        };
        let cursors = [(cursor, true)];
        let moves = multi_line_moves(&buffer, &cursors, upward);
        let mapped = moved_cursor_endpoints(&buffer, cursor, &moves);
        buffer.begin_edit_recording();
        for movement in moves.iter().rev() {
            buffer.replace(movement.range.clone(), &movement.replacement);
        }
        assert_eq!(
            buffer.text(),
            if upward {
                "one\r\n二\r\nhead\r\ntail"
            } else {
                "head\r\ntail\r\none\r\n二"
            }
        );
        let selected = Cursor {
            offset: moved_endpoint_offset(&buffer, mapped.0),
            anchor: moved_endpoint_offset(&buffer, mapped.1),
        };
        assert!(selected.offset < selected.anchor);
        assert!(
            buffer
                .text_for_range(selected.selection_range())
                .starts_with("one\r\n二")
        );
        let edits = buffer.finish_edit_recording();
        assert!(buffer.apply_edits_inverse(&edits));
        assert_eq!(buffer.text(), source);

        let resolved = copy_lines_at_cursors(&mut buffer, &cursors, upward);
        assert_eq!(buffer.text(), "head\r\none\r\n二\r\none\r\n二\r\ntail");
        assert!(resolved[0].0.offset < resolved[0].0.anchor);
        assert_eq!(
            buffer.text_for_range(resolved[0].0.selection_range()),
            "one\r\n二\r\n"
        );
    }
}

#[cfg(test)]
#[test]
fn multi_cursor_line_moves_merge_adjacent_targets_and_map_fold_lines() {
    let buffer = Buffer::from_text("a\nb\nc\nd\ne\n");
    let cursors = [
        (
            Cursor {
                offset: buffer.line_start(1),
                anchor: buffer.line_start(1),
            },
            true,
        ),
        (
            Cursor {
                offset: buffer.line_start(2),
                anchor: buffer.line_start(2),
            },
            false,
        ),
        (
            Cursor {
                offset: buffer.line_start(2),
                anchor: buffer.line_start(2),
            },
            false,
        ),
        (
            Cursor {
                offset: buffer.len(),
                anchor: buffer.len(),
            },
            false,
        ),
    ];
    let moves = multi_line_moves(&buffer, &cursors, true);
    assert_eq!(moves.len(), 2);
    assert_eq!(moves[0].replacement, "b\nc\na");
    assert_eq!(remap_moved_line(&moves, 0), 2);
    assert_eq!(remap_moved_line(&moves, 1), 0);
    assert_eq!(remap_moved_line(&moves, 2), 1);
    assert_eq!(remap_moved_line(&moves, 4), 5);
    assert_eq!(remap_moved_line(&moves, 5), 4);
    let down = multi_line_moves(&buffer, &cursors, false);
    assert_eq!(down.len(), 1);
    assert_eq!(down[0].replacement, "d\nb\nc");
}

#[cfg(test)]
#[test]
fn multi_cursor_line_moves_keep_each_caret_on_its_line_for_all_target_sets() {
    for source in ["a\nb\nc\nd\ne", "a\r\nb\r\nc\r\nd\r\ne", "a\nb\nc\nd\ne\n"] {
        for upward in [false, true] {
            for mask in 1usize..(1 << Buffer::from_text(source).line_count()) {
                let mut buffer = Buffer::from_text(source);
                let cursors: Vec<_> = (0..buffer.line_count())
                    .filter(|line| mask & (1 << line) != 0)
                    .map(|line| {
                        let at = buffer.line_start(line);
                        (
                            Cursor {
                                offset: at,
                                anchor: at,
                            },
                            line == mask.trailing_zeros() as usize,
                        )
                    })
                    .collect();
                let original_lines: Vec<_> = cursors
                    .iter()
                    .map(|(cursor, _)| {
                        buffer
                            .line_content(buffer.offset_to_line_col(cursor.offset).0)
                            .trim_end_matches('\r')
                            .to_string()
                    })
                    .collect();
                let moves = multi_line_moves(&buffer, &cursors, upward);
                let endpoints: Vec<_> = cursors
                    .iter()
                    .map(|(cursor, _)| moved_cursor_endpoints(&buffer, *cursor, &moves))
                    .collect();
                for movement in moves.iter().rev() {
                    buffer.replace(movement.range.clone(), &movement.replacement);
                }
                for ((offset, _), original) in endpoints.into_iter().zip(original_lines) {
                    let mapped = moved_endpoint_offset(&buffer, offset);
                    let line = buffer.offset_to_line_col(mapped).0;
                    assert_eq!(
                        buffer.line_content(line).trim_end_matches('\r'),
                        original,
                        "source={source:?}, upward={upward}, mask={mask}"
                    );
                }
            }
        }
    }
}

#[cfg(test)]
#[test]
fn adjacent_line_swap_preserves_eof_newlines_unicode_and_crlf() {
    for (source, expected) in [
        ("a\nb", "b\na"),
        ("a\nb\nc", "b\na\nc"),
        ("a\nb\n", "b\na\n"),
        ("\ntext", "text\n"),
        ("a\r\nb", "b\r\na"),
        ("a\r\nb\r\nc", "b\r\na\r\nc"),
        ("\u{4e2d}\n\u{1f642}", "\u{1f642}\n\u{4e2d}"),
    ] {
        let mut buffer = Buffer::from_text(source);
        let old_line_count = buffer.line_count();
        buffer.begin_edit_recording();
        let (range, replacement) = adjacent_line_swap_edit(&buffer, 0).unwrap();
        buffer.replace(range, &replacement);
        let edits = buffer.finish_edit_recording();
        assert_eq!(buffer.text(), expected);
        assert_eq!(buffer.line_count(), old_line_count);
        assert_eq!(edits.len(), 1);
        assert!(buffer.apply_edits_inverse(&edits));
        assert_eq!(buffer.text(), source);
    }
}

#[cfg(test)]
#[test]
fn page_down_budgets_actual_rows_instead_of_uniform_steps() {
    let steps = |heights: &[f32], viewport: f32| {
        let mut remaining = viewport;
        let mut steps = 0;
        for &height in heights {
            if !page_down_row_fits(remaining, height, steps == 0) {
                break;
            }
            remaining -= height;
            steps += 1;
        }
        steps
    };
    assert_eq!(steps(&[24.0, 60.0, 24.0, 24.0], 100.0), 2);
    assert_eq!(steps(&[24.0; 5], 100.0), 4);
    assert_eq!(steps(&[120.0, 24.0], 100.0), 1);
    assert_eq!(steps(&[24.0; 5], 0.0), 1);
}

#[cfg(test)]
#[test]
fn page_down_counts_document_gaps_and_tall_sibling_cells() {
    let steps = |tops: &[f32], viewport: f32| {
        let mut remaining = viewport;
        let mut steps = 0;
        for pair in tops.windows(2) {
            let distance = pair[1] - pair[0];
            if !page_down_row_fits(remaining, distance, steps == 0) {
                break;
            }
            remaining -= distance;
            steps += 1;
        }
        steps
    };
    assert_eq!(steps(&[0.0, 24.0, 84.0, 108.0], 100.0), 2);
    assert_eq!(steps(&[0.0, 24.0, 144.0, 168.0], 100.0), 1);
    assert_eq!(steps(&[0.0, 144.0, 168.0], 100.0), 1);
}

impl EditorView {
    // ── Action handlers ─────────────────────────────────────────────

    fn finish_horizontal_cursor_move(&mut self, cx: &mut Context<Self>) {
        self.table_handles.selection = None;
        self.table_cell_selection = None;
        self.pending_scroll_to_cursor = false;
        self.pending_center_cursor_in_view = false;
        self.pause_blinking(cx);
    }

    fn apply_history_snapshot(
        &mut self,
        cx: &mut Context<Self>,
        take_snapshot: impl FnOnce(
            &mut history_store::TabHistory,
            &EditorSnapshot,
        ) -> Option<EditorSnapshot>,
    ) {
        self.commit_history_transaction();
        let current = self.current_snapshot();
        let Some(index) = self.active_tab_index() else {
            return;
        };
        let Some(snapshot) = take_snapshot(&mut self.tabs[index].history, &current) else {
            return;
        };

        self.restore_snapshot(snapshot);
        self.finish_table_source_history();
        if self.search_visible {
            self.run_search(cx);
        }
        self.reset_typing_checkpoint_grouping();
        let snapshot = self.current_snapshot();
        self.tabs[index].set_snapshot(snapshot);
        self.sync_active_tab_dirty_state();
        session_store::persist_session(self);
        self.pause_blinking(cx);
    }

    fn restore_history_view(&mut self, view: history_store::SnapshotViewState) {
        self.cursor.offset = view.cursor_offset.min(self.buffer.len());
        self.cursor.anchor = view.cursor_anchor.min(self.buffer.len());
        self.extra_cursors = view
            .extra_cursors
            .into_iter()
            .map(|(offset, anchor)| Cursor {
                offset: offset.min(self.buffer.len()),
                anchor: anchor.min(self.buffer.len()),
            })
            .collect();
        self.scroll_offset = self
            .pending_typing_scroll_guard
            .map_or(view.scroll_offset, |guard| guard.scroll_offset);
    }

    fn apply_history_transaction(&mut self, undo: bool, cx: &mut Context<Self>) -> bool {
        self.commit_history_transaction();
        let Some(index) = self.active_tab_index() else {
            return false;
        };
        let transaction = if undo {
            self.tabs[index].history.take_undo_transaction()
        } else {
            self.tabs[index].history.take_redo_transaction()
        };
        let Some((edits, view)) = transaction else {
            return false;
        };
        let folded_anchors = self.snapshot_folded_anchor_offsets();
        let applied = if undo {
            self.buffer.apply_edits_inverse(&edits)
        } else {
            self.buffer.apply_edits_forward(&edits)
        };
        debug_assert!(applied, "history transaction out of sync");
        if !applied {
            return false;
        }
        if let Some(comparison) = &mut self.line_diff_comparison {
            comparison.apply_recorded_edits(&edits, undo, self.buffer.text());
        }
        self.remap_folded_anchor_offsets_after_history(folded_anchors, &edits, undo);
        self.restore_history_view(view);
        self.clear_marked_ranges();
        self.reset_cursor_visual_targeting();
        self.ime_composition_snapshot = None;
        self.dedup_cursors();
        self.reparse();
        self.finish_table_source_history();
        self.scroll_to_cursor();
        if self.search_visible {
            self.run_search(cx);
        }
        self.reset_typing_checkpoint_grouping();
        self.sync_active_tab_dirty_state();
        self.pause_blinking(cx);
        true
    }

    pub(crate) fn on_undo(&mut self, _: &Undo, _: &mut Window, cx: &mut Context<Self>) {
        self.pending_typing_scroll_guard = None;
        if !self.begin_content_edit(cx) {
            return;
        }
        if !self.apply_history_transaction(true, cx) {
            self.apply_history_snapshot(cx, |history, current| history.take_undo(current));
        }
    }

    pub(crate) fn on_escape(&mut self, _: &Escape, window: &mut Window, cx: &mut Context<Self>) {
        if crate::text_input::dismiss_input_context_menu(window, cx) {
            return;
        }
        if self.line_diff_selection.take().is_some() {
            self.line_diff_selection_dragging = false;
            cx.notify();
            return;
        }
        if self.cancel_table_handle_press(cx) {
            return;
        }
        let cleared_border = self.table_handles.selection.take().is_some();
        let cleared_cells = self.table_cell_selection.take().is_some();
        if cleared_border || cleared_cells {
            cx.notify();
            return;
        }
        if self.cancel_line_drag(cx) {
            return;
        }
        if self.preview_overlay.is_some() {
            self.close_preview_overlay();
            cx.notify();
            return;
        }
        if self.settings_font_picker.is_some() {
            self.close_font_picker(cx);
            return;
        }
        if self.quick_open_visible {
            self.close_quick_open(window, cx);
            return;
        }
        if self.command_palette_visible {
            self.close_command_palette(window, cx);
            return;
        }
        if self.language_picker_visible {
            self.close_language_picker(window, cx);
            return;
        }
        if self.workspace_search_visible {
            self.close_workspace_search(window, cx);
            return;
        }
        if self.dialog.is_some() {
            self.dismiss_dialog(cx);
            return;
        }
        if self.search_visible {
            self.dismiss_search(window, cx);
            return;
        }
        if self.sidebar_edit.is_some() {
            self.cancel_sidebar_edit(cx);
            return;
        }
        if self.context_menu.is_some() {
            self.close_context_menu(cx);
            return;
        }
        if self.slash_menu_start.is_some() {
            self.dismiss_slash_menu(cx);
            return;
        }
        if self.table_boundary_caret.take().is_some() {
            self.table_boundary_drag_origin = None;
            cx.notify();
            return;
        }
        if self.selected_handle.take().is_some() || self.table_drag.take().is_some() {
            cx.notify();
            return;
        }
        if self.has_extra_cursors() {
            self.clear_extra_cursors();
            cx.notify();
            return;
        }
        if self.focused_sidebar_path.is_some() {
            self.focused_sidebar_path = None;
            cx.notify();
            return;
        }
        // Last resort, and macOS-only: elsewhere fullscreen is toggled with F11.
        #[cfg(target_os = "macos")]
        if window.is_fullscreen() {
            window.toggle_fullscreen();
        }
    }

    pub(crate) fn on_toggle_fullscreen(
        &mut self,
        _: &ToggleFullscreen,
        window: &mut Window,
        _: &mut Context<Self>,
    ) {
        window.toggle_fullscreen();
    }

    pub(crate) fn on_preview_overlay_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.preview_overlay.is_none() {
            return;
        }

        if matches!(event.keystroke.key.as_ref(), "escape") {
            window.prevent_default();
            cx.stop_propagation();
            self.close_preview_overlay();
            cx.notify();
        }
    }

    pub(crate) fn on_redo(&mut self, _: &Redo, _: &mut Window, cx: &mut Context<Self>) {
        self.pending_typing_scroll_guard = None;
        if !self.begin_content_edit(cx) {
            return;
        }
        if !self.apply_history_transaction(false, cx) {
            self.apply_history_snapshot(cx, |history, current| history.take_redo(current));
        }
    }

    pub(crate) fn on_toggle_runtime_profile(
        &mut self,
        _: &ToggleRuntimeProfile,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(profiler) = self.runtime_profiler.take() {
            let diagnostics = self.runtime_profile_diagnostics();
            match profiler.stop_with_diagnostics("manual-stop", Some(diagnostics)) {
                Ok(paths) => self.show_runtime_profile_paths(
                    self.message(crate::i18n::MessageKey::ProfileStopped),
                    paths,
                    cx,
                ),
                Err(error) => self.show_runtime_profile_error(
                    self.message(crate::i18n::MessageKey::ProfileStopFailed),
                    format!("{error:#}"),
                    cx,
                ),
            }
            return;
        }

        match RuntimeProfiler::start_manual() {
            Ok(profiler) => {
                let output_dir = profiler.output_dir().to_path_buf();
                let stop_shortcut = crate::editor::shortcut::display_shortcut_from_keystroke(
                    "secondary-alt-ctrl-p",
                );
                let snapshot_shortcut = crate::editor::shortcut::display_shortcut_from_keystroke(
                    "secondary-shift-alt-ctrl-p",
                );
                self.runtime_profiler = Some(profiler);
                self.show_dialog(
                    Dialog {
                        title: self
                            .message(crate::i18n::MessageKey::ProfileStarted)
                            .to_string(),
                        message: format!(
                            "{}:\n{}\n\n{}",
                            self.message(crate::i18n::MessageKey::ProfileOutput),
                            output_dir.display(),
                            crate::i18n::MessageKey::ProfileInstructions.format(
                                self.ui_language,
                                &[("stop", &stop_shortcut), ("snapshot", &snapshot_shortcut)]
                            ),
                        ),
                        save_label: None,
                        confirm_label: self.t(TextKey::Acknowledge).to_string(),
                        cancel_label: self.t(TextKey::Close).to_string(),
                        is_danger: false,
                        action: DialogAction::DismissOnly,
                    },
                    cx,
                );
            }
            Err(error) => self.show_runtime_profile_error(
                self.message(crate::i18n::MessageKey::ProfileStartFailed),
                format!("{error:#}"),
                cx,
            ),
        }
    }

    pub(crate) fn on_write_runtime_profile_snapshot(
        &mut self,
        _: &WriteRuntimeProfileSnapshot,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.runtime_profiler.is_none() {
            let shortcut =
                crate::editor::shortcut::display_shortcut_from_keystroke("secondary-alt-ctrl-p");
            self.show_runtime_profile_error(
                self.message(crate::i18n::MessageKey::ProfileNotRunning),
                crate::i18n::MessageKey::ProfileStartInstruction
                    .format(self.ui_language, &[("shortcut", &shortcut)]),
                cx,
            );
            return;
        }

        let diagnostics = self.runtime_profile_diagnostics();
        let profiler = self
            .runtime_profiler
            .as_mut()
            .expect("runtime profiler checked above");
        match profiler.write_snapshot_with_diagnostics("manual-snapshot", Some(diagnostics)) {
            Ok(paths) => self.show_runtime_profile_paths(
                self.message(crate::i18n::MessageKey::ProfileSnapshotWritten),
                paths,
                cx,
            ),
            Err(error) => self.show_runtime_profile_error(
                self.message(crate::i18n::MessageKey::ProfileSnapshotFailed),
                format!("{error:#}"),
                cx,
            ),
        }
    }

    fn show_runtime_profile_paths(
        &mut self,
        title: &str,
        paths: RuntimeProfilePaths,
        cx: &mut Context<Self>,
    ) {
        self.show_dialog(
            Dialog {
                title: title.to_string(),
                message: paths.message(self.ui_language),
                save_label: None,
                confirm_label: self.t(TextKey::Acknowledge).to_string(),
                cancel_label: self.t(TextKey::Close).to_string(),
                is_danger: false,
                action: DialogAction::DismissOnly,
            },
            cx,
        );
    }

    fn show_runtime_profile_error(&mut self, title: &str, message: String, cx: &mut Context<Self>) {
        self.show_dialog(
            Dialog {
                title: title.to_string(),
                message,
                save_label: None,
                confirm_label: self.t(TextKey::Acknowledge).to_string(),
                cancel_label: self.t(TextKey::Close).to_string(),
                is_danger: false,
                action: DialogAction::DismissOnly,
            },
            cx,
        );
    }

    pub(crate) fn on_backspace(
        &mut self,
        _: &Backspace,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_content_edit(cx) {
            return;
        }
        if self.handle_folded_boundary_delete(DeleteAction::Backward) {
            self.pause_blinking(cx);
            return;
        }
        if self.source_mode {
            self.capture_typing_scroll_guard();
            self.pending_code_fence = None;
            self.checkpoint();
            if self.active_file_kind().is_some_and(FileKind::is_markdown) {
                self.apply_source_action(source_edit::SourceAction::Backspace);
            } else if self.has_extra_cursors() {
                self.multi_cursor_backspace();
            } else if let Some(range) = self.auto_paired_backspace_range() {
                self.queue_non_markdown_line_display_transfer(&range, "");
                self.buffer.delete(range.clone());
                self.cursor.move_to(range.start, &self.buffer);
            } else {
                let range = if self.cursor.has_selection() {
                    self.cursor.selection_range()
                } else {
                    self.buffer.prev_grapheme_boundary(self.cursor.offset)..self.cursor.offset
                };
                self.queue_non_markdown_line_display_transfer(&range, "");
                self.cursor.backspace(&mut self.buffer);
            }
            if !self.active_file_kind().is_some_and(FileKind::is_markdown) {
                self.reparse();
            }
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }
        if self.delete_table_cell_selection(cx) {
            return;
        }
        if self.delete_table_border_selection(cx) {
            return;
        }
        if let Some(boundary) = self.table_boundary_caret {
            if boundary.side == TableBoundarySide::Below {
                self.table_boundary_caret = None;
                self.table_boundary_drag_origin = None;
                self.table_delete_table(boundary.table_start_line, cx);
                self.pause_blinking(cx);
                return;
            }
            self.pause_blinking(cx);
            cx.notify();
            return;
        }
        if !self.has_extra_cursors() && self.backspace_blank_table_row() {
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }
        if self.semantic_delete_touches_table(DeleteAction::Backward) {
            self.capture_typing_scroll_guard();
            self.checkpoint();
            self.apply_semantic_delete(DeleteAction::Backward);
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            if let Some(start) = self.slash_menu_start {
                if self.cursor.offset <= start {
                    self.dismiss_slash_menu(cx);
                } else {
                    self.update_slash_filter(cx);
                }
            } else if self.wiki_link_start.is_some() {
                self.sync_wiki_link_menu(cx);
            } else {
                self.maybe_open_wiki_link_menu_after_edit(cx);
            }
            return;
        }
        if self.backspace_empty_front_matter_row(cx) {
            return;
        }
        self.capture_typing_scroll_guard();
        self.checkpoint();
        let should_normalize_ordered_lists = self.active_tab_is_markdown();
        let cursor_line_before = self.cursor_line();
        let cursor_had_inline_headroom = self.primary_cursor_has_inline_edit_headroom();
        let selection = self.cursor.selection_range();
        let selection_spans_multiple_lines = if self.cursor.has_selection() {
            let (start_line, _) = self.buffer.offset_to_line_col(selection.start);
            let (end_line, _) = self.buffer.offset_to_line_col(selection.end);
            start_line != end_line
        } else {
            false
        };
        let selection_line_span = if self.cursor.has_selection() {
            let (start_line, _) = self.buffer.offset_to_line_col(selection.start);
            let end_probe = selection
                .end
                .saturating_sub(usize::from(selection.end > selection.start));
            let (end_line, _) = self
                .buffer
                .offset_to_line_col(end_probe.min(self.buffer.len()));
            Some((start_line, end_line))
        } else {
            None
        };
        let touched_ordered_prefix = if should_normalize_ordered_lists {
            if self.cursor.has_selection() {
                self.ordered_list_prefix_touched_by_range(&selection)
            } else {
                self.cursor
                    .offset
                    .checked_sub(1)
                    .and_then(|offset| self.ordered_list_prefix_touched_by_range(&(offset..offset)))
            }
        } else {
            None
        };
        let removes_line_break = if self.cursor.has_selection() {
            let selection =
                self.buffer.clamp_offset(selection.start)..self.buffer.clamp_offset(selection.end);
            self.buffer.text_for_range(selection).contains('\n')
        } else {
            self.cursor.offset > 0
                && self.buffer.text().as_bytes().get(self.cursor.offset - 1) == Some(&b'\n')
        };

        // Multi-cursor backspace
        if self.has_extra_cursors() {
            self.multi_cursor_backspace();
            self.reparse();
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }

        if !self.cursor.has_selection() {
            let (line, col) = self.buffer.offset_to_line_col(self.cursor.offset);
            let line_start = self.buffer.line_start(line);
            let line_content = self.buffer.line_content(line).to_string();
            let leading_ws =
                line_content.len() - line_content.trim_start_matches([' ', '\t']).len();
            let trimmed = &line_content[leading_ws..];
            let prefix_len = match self.decorations.get(line).map(|d| d.block_type.clone()) {
                Some(BlockType::UnorderedList) => {
                    if trimmed.starts_with("- ")
                        || trimmed.starts_with("* ")
                        || trimmed.starts_with("+ ")
                    {
                        2
                    } else {
                        0
                    }
                }
                Some(BlockType::OrderedList) => markdown::ordered_list_prefix_len(trimmed),
                Some(BlockType::TaskList(_)) => task_prefix_len(trimmed),
                Some(BlockType::Blockquote) => {
                    if trimmed.starts_with("> ") {
                        2
                    } else if trimmed.starts_with('>') {
                        1
                    } else {
                        0
                    }
                }
                _ => 0,
            };
            if prefix_len > 0 && col == leading_ws + prefix_len {
                let del_start = line_start + leading_ws;
                let del_end = line_start + leading_ws + prefix_len;
                self.queue_non_markdown_line_display_transfer(&(del_start..del_end), "");
                self.buffer.replace(del_start..del_end, "");
                self.cursor.move_to(del_start, &self.buffer);
                self.reparse();
                if should_normalize_ordered_lists {
                    self.normalize_all_ordered_lists();
                    self.reparse();
                }
                if !cursor_had_inline_headroom || self.cursor_line() != cursor_line_before {
                    self.scroll_to_cursor();
                }
                self.pause_blinking(cx);
                return;
            }

            if let Some(delete_range) = self.auto_paired_backspace_range() {
                self.queue_non_markdown_line_display_transfer(&delete_range, "");
                self.buffer.delete(delete_range.clone());
                self.cursor.move_to(delete_range.start, &self.buffer);
                self.reparse();
                if !cursor_had_inline_headroom || self.cursor_line() != cursor_line_before {
                    self.scroll_to_cursor();
                }
                self.pause_blinking(cx);
                if self.wiki_link_start.is_some() {
                    self.sync_wiki_link_menu(cx);
                }
                return;
            }
        }
        let delete_range = if self.cursor.has_selection() {
            Some(self.cursor.selection_range())
        } else if self.cursor.offset > 0 {
            let prev = self.buffer.prev_grapheme_boundary(self.cursor.offset);
            Some(prev..self.cursor.offset)
        } else {
            None
        };
        if let Some(delete_range) = delete_range.as_ref() {
            self.queue_non_markdown_line_display_transfer(delete_range, "");
        }
        self.cursor.backspace(&mut self.buffer);
        self.reparse();
        if should_normalize_ordered_lists
            && let Some((start_line, _)) = selection_line_span
            && (touched_ordered_prefix.is_some() || removes_line_break)
        {
            self.normalize_ordered_list_near_line(start_line, false);
            self.reparse();
        } else if should_normalize_ordered_lists && let Some(line) = touched_ordered_prefix {
            self.normalize_ordered_list_near_line(line, true);
            self.reparse();
        } else if should_normalize_ordered_lists && removes_line_break {
            self.normalize_ordered_list_near_line(self.cursor_line(), false);
            self.reparse();
        }
        if !cursor_had_inline_headroom
            || (!removes_line_break
                && !selection_spans_multiple_lines
                && self.cursor_line() != cursor_line_before)
        {
            self.scroll_to_cursor();
        }
        self.pause_blinking(cx);

        if let Some(start) = self.slash_menu_start {
            if self.cursor.offset <= start {
                self.dismiss_slash_menu(cx);
            } else {
                self.update_slash_filter(cx);
            }
        } else if self.wiki_link_start.is_some() {
            self.sync_wiki_link_menu(cx);
        } else {
            self.maybe_open_wiki_link_menu_after_edit(cx);
        }
    }

    pub(crate) fn on_delete(&mut self, _: &Delete, _window: &mut Window, cx: &mut Context<Self>) {
        if self.focused_sidebar_path.is_some() {
            self.sidebar_delete_focused(cx);
            return;
        }
        if !self.begin_content_edit(cx) {
            return;
        }
        if self.handle_folded_boundary_delete(DeleteAction::Forward) {
            self.pause_blinking(cx);
            return;
        }
        if self.source_mode {
            self.capture_typing_scroll_guard();
            self.pending_code_fence = None;
            self.checkpoint();
            if self.active_file_kind().is_some_and(FileKind::is_markdown) {
                self.apply_source_action(source_edit::SourceAction::Delete);
            } else if self.has_extra_cursors() {
                self.multi_cursor_delete();
            } else {
                let range = if self.cursor.has_selection() {
                    self.cursor.selection_range()
                } else {
                    self.cursor.offset..self.buffer.next_grapheme_boundary(self.cursor.offset)
                };
                self.queue_non_markdown_line_display_transfer(&range, "");
                self.cursor.delete(&mut self.buffer);
            }
            if !self.active_file_kind().is_some_and(FileKind::is_markdown) {
                self.reparse();
            }
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }
        if self.delete_table_cell_selection(cx) {
            return;
        }
        if self.delete_table_border_selection(cx) {
            return;
        }
        if let Some(boundary) = self.table_boundary_caret {
            if boundary.side == TableBoundarySide::Below {
                self.table_boundary_caret = None;
                self.table_boundary_drag_origin = None;
                self.table_delete_table(boundary.table_start_line, cx);
                self.pause_blinking(cx);
                return;
            }
            self.pause_blinking(cx);
            cx.notify();
            return;
        }
        if self.semantic_delete_touches_table(DeleteAction::Forward) {
            self.capture_typing_scroll_guard();
            self.checkpoint();
            self.apply_semantic_delete(DeleteAction::Forward);
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }
        self.capture_typing_scroll_guard();
        self.checkpoint();
        let should_normalize_ordered_lists = self.active_tab_is_markdown();
        let cursor_line_before = self.cursor_line();
        let cursor_had_inline_headroom = self.primary_cursor_has_inline_edit_headroom();
        let selection = self.cursor.selection_range();
        let selection_spans_multiple_lines = if self.cursor.has_selection() {
            let (start_line, _) = self.buffer.offset_to_line_col(selection.start);
            let (end_line, _) = self.buffer.offset_to_line_col(selection.end);
            start_line != end_line
        } else {
            false
        };
        let selection_line_span = if self.cursor.has_selection() {
            let (start_line, _) = self.buffer.offset_to_line_col(selection.start);
            let end_probe = selection
                .end
                .saturating_sub(usize::from(selection.end > selection.start));
            let (end_line, _) = self
                .buffer
                .offset_to_line_col(end_probe.min(self.buffer.len()));
            Some((start_line, end_line))
        } else {
            None
        };
        let touched_ordered_prefix = if should_normalize_ordered_lists {
            if self.cursor.has_selection() {
                self.ordered_list_prefix_touched_by_range(&selection)
            } else {
                self.ordered_list_prefix_touched_by_range(&(self.cursor.offset..self.cursor.offset))
            }
        } else {
            None
        };
        let removes_line_break = if self.cursor.has_selection() {
            let selection =
                self.buffer.clamp_offset(selection.start)..self.buffer.clamp_offset(selection.end);
            self.buffer.text_for_range(selection).contains('\n')
        } else {
            self.buffer.text().as_bytes().get(self.cursor.offset) == Some(&b'\n')
        };
        if self.has_extra_cursors() {
            self.multi_cursor_delete();
            self.reparse();
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }
        let delete_range = if self.cursor.has_selection() {
            Some(self.cursor.selection_range())
        } else if self.cursor.offset < self.buffer.len() {
            let next = self.buffer.next_grapheme_boundary(self.cursor.offset);
            Some(self.cursor.offset..next)
        } else {
            None
        };
        if let Some(delete_range) = delete_range.as_ref() {
            self.queue_non_markdown_line_display_transfer(delete_range, "");
        }
        self.cursor.delete(&mut self.buffer);
        self.reparse();
        if should_normalize_ordered_lists
            && let Some((start_line, _)) = selection_line_span
            && (touched_ordered_prefix.is_some() || removes_line_break)
        {
            self.normalize_ordered_list_near_line(start_line, false);
            self.reparse();
        } else if should_normalize_ordered_lists && let Some(line) = touched_ordered_prefix {
            self.normalize_ordered_list_near_line(line, true);
            self.reparse();
        } else if should_normalize_ordered_lists && removes_line_break {
            self.normalize_ordered_list_near_line(self.cursor_line(), false);
            self.reparse();
        }
        if !cursor_had_inline_headroom
            || (!removes_line_break
                && !selection_spans_multiple_lines
                && self.cursor_line() != cursor_line_before)
        {
            self.scroll_to_cursor();
        }
        self.pause_blinking(cx);
        if self.wiki_link_start.is_some() {
            self.sync_wiki_link_menu(cx);
        } else {
            self.maybe_open_wiki_link_menu_after_edit(cx);
        }
    }

    pub(crate) fn on_left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.move_line_diff_selection_horizontal(false, false, cx) {
            return;
        }
        if self.focused_sidebar_path.is_some() {
            self.sidebar_nav_left(cx);
            return;
        }
        let had_selection = self.cursor.has_selection();
        self.reset_cursor_visual_targeting();
        if self.has_extra_cursors() {
            self.multi_cursor_move_left();
            self.finish_horizontal_cursor_move(cx);
            return;
        }
        let prev_offset = self.cursor.offset;
        if !had_selection && self.move_table_cursor_horizontal(false) {
            if self.table_boundary_caret.is_none() && self.cursor.offset != prev_offset {
                self.maybe_auto_select_link_url(prev_offset, false, cx);
            }
            self.finish_horizontal_cursor_move(cx);
            return;
        }
        self.table_boundary_caret = None;
        let (line, _col) = self.buffer.offset_to_line_col(self.cursor.offset);
        let cell_ranges = self
            .decorations
            .get(line)
            .and_then(|d| d.table_info.as_ref())
            .filter(|ti| !ti.is_separator)
            .map(|ti| ti.cell_ranges.clone());
        let previous = [self.cursor];
        self.cursor.move_left(&self.buffer);
        if let Some(cell_ranges) = cell_ranges {
            let (_, new_col) = self.buffer.offset_to_line_col(self.cursor.offset);
            let mut jumped = false;
            for i in 0..cell_ranges.len().saturating_sub(1) {
                let gap_start = cell_ranges[i].end.saturating_add(1);
                let gap_end = cell_ranges[i + 1].start;
                if new_col >= gap_start && new_col < gap_end {
                    let line_start = self.buffer.line_start(line);
                    self.cursor
                        .move_to(line_start + cell_ranges[i].end, &self.buffer);
                    jumped = true;
                    break;
                }
            }
            if !jumped && !cell_ranges.is_empty() && new_col < cell_ranges[0].start {
                let line_start = self.buffer.line_start(line);
                self.cursor
                    .move_to(line_start + cell_ranges[0].start, &self.buffer);
            }
        }
        self.skip_folded_caret_navigation(&previous);
        if !had_selection
            && self.cursor.offset != prev_offset
            && self.is_wrap_boundary_offset(self.cursor.offset)
        {
            self.set_wrap_boundary_affinity(
                self.cursor.offset,
                WrapBoundaryAffinityKind::Downstream,
            );
        }
        self.maybe_auto_select_link_url(prev_offset, false, cx);
        self.finish_horizontal_cursor_move(cx);
        if self.wiki_link_start.is_some() {
            self.sync_wiki_link_menu(cx);
        }
    }

    pub(crate) fn on_right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.move_line_diff_selection_horizontal(true, false, cx) {
            return;
        }
        if self.focused_sidebar_path.is_some() {
            self.sidebar_nav_right(cx);
            return;
        }
        let had_selection = self.cursor.has_selection();
        self.reset_cursor_visual_targeting();
        if self.has_extra_cursors() {
            self.multi_cursor_move_right();
            self.finish_horizontal_cursor_move(cx);
            return;
        }
        let prev_offset = self.cursor.offset;
        if !had_selection && self.move_table_cursor_horizontal(true) {
            if self.table_boundary_caret.is_none() && self.cursor.offset != prev_offset {
                self.maybe_auto_select_link_url(prev_offset, false, cx);
            }
            self.finish_horizontal_cursor_move(cx);
            return;
        }
        self.table_boundary_caret = None;
        let (line, _col) = self.buffer.offset_to_line_col(self.cursor.offset);
        let cell_ranges = self
            .decorations
            .get(line)
            .and_then(|d| d.table_info.as_ref())
            .filter(|ti| !ti.is_separator)
            .map(|ti| ti.cell_ranges.clone());
        let previous = [self.cursor];
        self.cursor.move_right(&self.buffer);
        if let Some(cell_ranges) = cell_ranges {
            let (_, new_col) = self.buffer.offset_to_line_col(self.cursor.offset);
            for i in 0..cell_ranges.len().saturating_sub(1) {
                let gap_start = cell_ranges[i].end.saturating_add(1);
                let gap_end = cell_ranges[i + 1].start;
                if new_col >= gap_start && new_col < gap_end {
                    let line_start = self.buffer.line_start(line);
                    self.cursor
                        .move_to(line_start + cell_ranges[i + 1].start, &self.buffer);
                    break;
                }
            }
        }
        self.skip_folded_caret_navigation(&previous);
        self.maybe_auto_select_link_url(prev_offset, false, cx);
        self.finish_horizontal_cursor_move(cx);
        if self.wiki_link_start.is_some() {
            self.sync_wiki_link_menu(cx);
        }
    }

    pub(crate) fn on_up(&mut self, _: &Up, window: &mut Window, cx: &mut Context<Self>) {
        if self.move_line_diff_selection_vertical(false, false, cx) {
            return;
        }
        self.table_handles.selection = None;
        self.table_cell_selection = None;
        if self.focused_sidebar_path.is_some() {
            self.sidebar_nav_up(cx);
            return;
        }
        if self.wiki_link_start.is_some() {
            if self.menu_selected_index > 0 {
                self.menu_selected_index -= 1;
            }
            cx.notify();
            return;
        }
        if self.slash_menu_start.is_some() {
            if self.context_menu.is_none() {
                self.dismiss_slash_menu(cx);
            } else {
                if self.menu_selected_index > 0 {
                    self.menu_selected_index -= 1;
                }
                cx.notify();
                return;
            }
        }
        if self.has_extra_cursors() {
            self.reset_cursor_visual_targeting();
            self.multi_cursor_move_up();
            self.navigate_scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }
        if self.table_cursor_cell().is_some() {
            if self.vertical_move_goal_x.is_none() {
                self.vertical_move_goal_x =
                    self.table_visual_x_for_offset(window, self.cursor.offset);
            }
            let goal_x = self.vertical_move_goal_x.unwrap_or_default();
            if let Some(target) =
                self.table_vertical_target(window, self.cursor.offset, false, goal_x)
            {
                let offset = self.apply_table_cursor_target(target);
                self.cursor.move_to(offset, &self.buffer);
                self.navigate_scroll_to_cursor();
                self.pause_blinking(cx);
                return;
            }
        }
        if self.move_table_boundary_caret(false) {
            self.navigate_scroll_to_cursor();
            self.pause_blinking(cx);
            cx.notify();
            return;
        }
        let previous = [self.cursor];
        let prev_offset = self.cursor.offset;
        if self.vertical_move_goal_x.is_none() {
            self.vertical_move_goal_x = self.cursor_visual_x(self.cursor.offset);
        }
        let goal_x = self.vertical_move_goal_x.unwrap_or_default();
        let previous_row = self.cursor_visual_position(prev_offset).map(|(row, _)| row);
        if let Some(offset) = self.offset_for_visual_up(self.cursor.offset) {
            self.cursor.move_to(offset, &self.buffer);
        } else {
            self.cursor.move_up(&self.buffer);
        }
        self.snap_cursor_to_content();
        self.skip_table_separator_line(false);
        self.update_wrap_boundary_affinity_for_vertical_move(
            prev_offset,
            previous_row,
            false,
            goal_x,
        );
        self.skip_folded_caret_navigation(&previous);
        self.maybe_auto_select_link_url(prev_offset, false, cx);
        self.navigate_scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_down(&mut self, _: &Down, window: &mut Window, cx: &mut Context<Self>) {
        if self.move_line_diff_selection_vertical(true, false, cx) {
            return;
        }
        self.table_handles.selection = None;
        self.table_cell_selection = None;
        if self.focused_sidebar_path.is_some() {
            self.sidebar_nav_down(cx);
            return;
        }
        if self.wiki_link_start.is_some() {
            let max_index = self.wiki_link_result_count().saturating_sub(1);
            if self.menu_selected_index < max_index {
                self.menu_selected_index += 1;
            }
            cx.notify();
            return;
        }
        if self.slash_menu_start.is_some() {
            if self.context_menu.is_none() {
                self.dismiss_slash_menu(cx);
            } else {
                let max_index = self.slash_menu_matched_actions().len().saturating_sub(1);
                if self.menu_selected_index < max_index {
                    self.menu_selected_index += 1;
                }
                cx.notify();
                return;
            }
        }
        if self.has_extra_cursors() {
            self.reset_cursor_visual_targeting();
            self.multi_cursor_move_down();
            self.navigate_scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }
        if self.table_cursor_cell().is_some() {
            if self.vertical_move_goal_x.is_none() {
                self.vertical_move_goal_x =
                    self.table_visual_x_for_offset(window, self.cursor.offset);
            }
            let goal_x = self.vertical_move_goal_x.unwrap_or_default();
            if let Some(target) =
                self.table_vertical_target(window, self.cursor.offset, true, goal_x)
            {
                let offset = self.apply_table_cursor_target(target);
                self.cursor.move_to(offset, &self.buffer);
                self.navigate_scroll_to_cursor();
                self.pause_blinking(cx);
                return;
            }
        }
        if self.move_table_boundary_caret(true) {
            self.navigate_scroll_to_cursor();
            self.pause_blinking(cx);
            cx.notify();
            return;
        }
        let previous = [self.cursor];
        let prev_offset = self.cursor.offset;
        if self.vertical_move_goal_x.is_none() {
            self.vertical_move_goal_x = self.cursor_visual_x(self.cursor.offset);
        }
        let goal_x = self.vertical_move_goal_x.unwrap_or_default();
        let previous_row = self.cursor_visual_position(prev_offset).map(|(row, _)| row);
        if let Some(offset) = self.offset_for_visual_down(self.cursor.offset) {
            self.cursor.move_to(offset, &self.buffer);
        } else {
            self.cursor.move_down(&self.buffer);
        }
        self.snap_cursor_to_content();
        self.skip_table_separator_line(true);
        self.update_wrap_boundary_affinity_for_vertical_move(
            prev_offset,
            previous_row,
            true,
            goal_x,
        );
        self.skip_folded_caret_navigation(&previous);
        self.maybe_auto_select_link_url(prev_offset, false, cx);
        self.navigate_scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_select_left(
        &mut self,
        _: &SelectLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.move_line_diff_selection_horizontal(false, true, cx) {
            return;
        }
        let previous = [self.cursor];
        self.table_boundary_caret = None;
        self.reset_cursor_visual_targeting();
        let in_table = self.table_cell_for_offset(self.cursor.offset).is_some();
        if self.has_extra_cursors() {
            self.multi_cursor_select_left();
        } else if let Some(target) = self.table_visible_horizontal_target(self.cursor.offset, false)
        {
            self.cursor.select_to(target, &self.buffer);
        } else if let Some(target) = self.table_selection_exit_offset(false) {
            self.cursor.select_to(target, &self.buffer);
        } else if !in_table {
            self.cursor.select_left(&self.buffer);
        }
        self.skip_folded_selection_navigation(&previous);
        self.dedup_cursors();
        self.finish_horizontal_cursor_move(cx);
    }

    pub(crate) fn on_select_right(
        &mut self,
        _: &SelectRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.move_line_diff_selection_horizontal(true, true, cx) {
            return;
        }
        let previous = [self.cursor];
        self.table_boundary_caret = None;
        self.reset_cursor_visual_targeting();
        let in_table = self.table_cell_for_offset(self.cursor.offset).is_some();
        if self.has_extra_cursors() {
            self.multi_cursor_select_right();
        } else if let Some(target) = self.table_visible_horizontal_target(self.cursor.offset, true)
        {
            self.cursor.select_to(target, &self.buffer);
        } else if let Some(target) = self.table_selection_exit_offset(true) {
            self.cursor.select_to(target, &self.buffer);
        } else if !in_table {
            self.cursor.select_right(&self.buffer);
        }
        self.skip_folded_selection_navigation(&previous);
        self.dedup_cursors();
        self.finish_horizontal_cursor_move(cx);
    }

    pub(crate) fn on_select_up(
        &mut self,
        _: &SelectUp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.move_line_diff_selection_vertical(false, true, cx) {
            return;
        }
        let previous = [self.cursor];
        let previous_row = self
            .cursor_visual_position(self.cursor.offset)
            .map(|(row, _)| row);
        self.table_boundary_caret = None;
        if self.has_extra_cursors() {
            self.reset_cursor_visual_targeting();
            self.multi_cursor_select_up();
        } else {
            let in_table = self.table_cell_for_offset(self.cursor.offset).is_some();
            if self.vertical_move_goal_x.is_none() {
                self.vertical_move_goal_x = self
                    .table_visual_x_for_offset(window, self.cursor.offset)
                    .or_else(|| self.cursor_visual_x(self.cursor.offset));
            }
            let goal_x = self.vertical_move_goal_x.unwrap_or_default();
            if let Some(target) =
                self.table_vertical_target(window, self.cursor.offset, false, goal_x)
            {
                let offset = self.apply_table_cursor_target(target);
                self.cursor.select_to(offset, &self.buffer);
            } else if !in_table && let Some(offset) = self.offset_for_visual_up(self.cursor.offset)
            {
                self.cursor.select_to(offset, &self.buffer);
            } else if let Some(offset) = self.table_selection_exit_offset(false) {
                self.cursor.select_to(offset, &self.buffer);
                self.clear_wrap_boundary_affinity();
            } else if !in_table {
                self.cursor.select_up(&self.buffer);
            }
            if !in_table {
                self.update_wrap_boundary_affinity_for_vertical_move(
                    previous[0].offset,
                    previous_row,
                    false,
                    goal_x,
                );
            }
        }
        self.skip_folded_selection_navigation(&previous);
        self.dedup_cursors();
        self.navigate_scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_select_down(
        &mut self,
        _: &SelectDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.move_line_diff_selection_vertical(true, true, cx) {
            return;
        }
        let previous = [self.cursor];
        let previous_row = self
            .cursor_visual_position(self.cursor.offset)
            .map(|(row, _)| row);
        self.table_boundary_caret = None;
        if self.has_extra_cursors() {
            self.reset_cursor_visual_targeting();
            self.multi_cursor_select_down();
        } else {
            let in_table = self.table_cell_for_offset(self.cursor.offset).is_some();
            if self.vertical_move_goal_x.is_none() {
                self.vertical_move_goal_x = self
                    .table_visual_x_for_offset(window, self.cursor.offset)
                    .or_else(|| self.cursor_visual_x(self.cursor.offset));
            }
            let goal_x = self.vertical_move_goal_x.unwrap_or_default();
            if let Some(target) =
                self.table_vertical_target(window, self.cursor.offset, true, goal_x)
            {
                let offset = self.apply_table_cursor_target(target);
                self.cursor.select_to(offset, &self.buffer);
            } else if !in_table
                && self
                    .collapsed_front_matter_range()
                    .is_some_and(|hidden| hidden.end >= self.buffer.line_count())
            {
                self.cursor.select_to(self.buffer.len(), &self.buffer);
            } else if !in_table
                && let Some(offset) = self.offset_for_visual_down(self.cursor.offset)
            {
                self.cursor.select_to(offset, &self.buffer);
            } else if let Some(offset) = self.table_selection_exit_offset(true) {
                self.cursor.select_to(offset, &self.buffer);
                self.clear_wrap_boundary_affinity();
            } else if !in_table {
                self.cursor.select_down(&self.buffer);
            }
            if !in_table {
                self.update_wrap_boundary_affinity_for_vertical_move(
                    previous[0].offset,
                    previous_row,
                    true,
                    goal_x,
                );
            }
        }
        self.skip_folded_selection_navigation(&previous);
        self.dedup_cursors();
        self.navigate_scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        if self.line_diff_selection.is_some() {
            self.move_line_diff_selection_document_edge(false, false, cx);
            self.move_line_diff_selection_document_edge(true, true, cx);
            return;
        }
        self.table_boundary_caret = None;
        self.cursor.select_all(&self.buffer);
        self.unfold_cursor_regions();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        if self.move_line_diff_selection_line_edge(false, false, cx) {
            return;
        }
        if self.table_boundary_caret.is_some() {
            self.pause_blinking(cx);
            return;
        }
        if self.move_table_cursors_to_cell_edge(true, false) {
            self.finish_horizontal_cursor_move(cx);
            return;
        }
        self.table_boundary_caret = None;
        self.reset_cursor_visual_targeting();
        if self.has_extra_cursors() {
            self.multi_cursor_move_to_line_start();
        } else {
            self.cursor.move_to_line_start(&self.buffer);
        }
        self.finish_horizontal_cursor_move(cx);
    }

    pub(crate) fn on_end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        if self.move_line_diff_selection_line_edge(true, false, cx) {
            return;
        }
        if self.table_boundary_caret.is_some() {
            self.pause_blinking(cx);
            return;
        }
        if self.move_table_cursors_to_cell_edge(false, false) {
            self.finish_horizontal_cursor_move(cx);
            return;
        }
        self.table_boundary_caret = None;
        self.reset_cursor_visual_targeting();
        if self.has_extra_cursors() {
            self.multi_cursor_move_to_line_end();
        } else {
            self.cursor.move_to_line_end(&self.buffer);
        }
        self.finish_horizontal_cursor_move(cx);
    }

    pub(crate) fn on_select_home(
        &mut self,
        _: &SelectHome,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.move_line_diff_selection_line_edge(false, true, cx) {
            return;
        }
        if self.table_boundary_caret.is_some() {
            self.pause_blinking(cx);
            return;
        }
        if self.move_table_cursors_to_cell_edge(true, true) {
            self.finish_horizontal_cursor_move(cx);
            return;
        }
        self.table_boundary_caret = None;
        if self.has_extra_cursors() {
            self.multi_cursor_select_to_line_start();
        } else {
            self.cursor.select_to_line_start(&self.buffer);
        }
        self.finish_horizontal_cursor_move(cx);
    }

    pub(crate) fn on_select_end(&mut self, _: &SelectEnd, _: &mut Window, cx: &mut Context<Self>) {
        if self.move_line_diff_selection_line_edge(true, true, cx) {
            return;
        }
        if self.table_boundary_caret.is_some() {
            self.pause_blinking(cx);
            return;
        }
        if self.move_table_cursors_to_cell_edge(false, true) {
            self.finish_horizontal_cursor_move(cx);
            return;
        }
        self.table_boundary_caret = None;
        if self.has_extra_cursors() {
            self.multi_cursor_select_to_line_end();
        } else {
            self.cursor.select_to_line_end(&self.buffer);
        }
        self.finish_horizontal_cursor_move(cx);
    }

    /// Enter on an empty nested list item steps out one level instead of
    /// dropping the marker outright. Reports whether the outdent applied.
    fn outdent_empty_list_item(
        &mut self,
        line_start: usize,
        line_end: usize,
        line_content: &str,
        prefix_len: usize,
        body_indent: usize,
        indent_step: usize,
    ) -> bool {
        if body_indent == 0 {
            return false;
        }
        let step = indent_step.clamp(1, body_indent);
        let outdented = format!(
            "{}{}{}",
            &line_content[..prefix_len],
            " ".repeat(body_indent - step),
            &line_content[prefix_len + body_indent..]
        );
        self.queue_non_markdown_line_display_transfer(&(line_start..line_end), &outdented);
        self.buffer.replace(line_start..line_end, &outdented);
        self.cursor
            .move_to(line_start + outdented.len(), &self.buffer);
        true
    }

    fn insert_table_cell_break(&mut self, soft: bool, cx: &mut Context<Self>) -> bool {
        if self.has_extra_cursors() {
            return false;
        }
        let Some(plan) = self.table_cell_enter_plan(self.cursor, soft) else {
            return false;
        };

        self.pending_code_fence = None;
        self.checkpoint();
        self.queue_non_markdown_line_display_transfer(&plan.range, &plan.text);
        self.apply_table_aware_cursor_plans(vec![(plan, true)]);
        self.scroll_to_cursor();
        self.pause_blinking(cx);
        true
    }

    pub(crate) fn on_enter(&mut self, _: &Enter, _: &mut Window, cx: &mut Context<Self>) {
        if self.focused_sidebar_path.is_some() {
            if self.sidebar_edit.is_some() {
                self.commit_sidebar_edit(cx);
            } else {
                self.sidebar_confirm_focused(cx);
            }
            return;
        }
        if !self.source_mode && self.table_boundary_caret.is_some() {
            if !self.begin_content_edit(cx) {
                return;
            }
            if self.insert_front_matter_break(cx) {
                return;
            }
            if self.insert_line_at_table_boundary(cx) {
                return;
            }
        }
        if !self.begin_content_edit(cx) {
            return;
        }
        if !self.folded_sections.is_empty() {
            self.checkpoint();
            if self.skip_folded_anchor_for_line_break() {
                self.pending_code_fence = None;
                self.multi_cursor_insert("\n");
                self.reparse();
                self.scroll_to_cursor();
                self.pause_blinking(cx);
                return;
            }
        }
        if self.source_mode {
            if self.sidebar_edit.is_some() {
                self.commit_sidebar_edit(cx);
                return;
            }
            self.pending_code_fence = None;
            self.checkpoint();
            if self.active_file_kind().is_some_and(FileKind::is_markdown) {
                self.apply_source_action(source_edit::SourceAction::Enter);
            } else {
                self.multi_cursor_insert("\n");
                self.reparse();
            }
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }
        if self.wiki_link_start.is_some() {
            self.confirm_wiki_link_selection(cx);
            return;
        }
        if self.slash_menu_start.is_some() {
            if self.context_menu.is_some() {
                self.execute_slash_menu_selected(cx);
                return;
            }
            self.dismiss_slash_menu(cx);
        }
        if self.sidebar_edit.is_some() {
            self.commit_sidebar_edit(cx);
            return;
        }
        if self.insert_front_matter_break(cx) {
            return;
        }
        if self.insert_table_cell_break(false, cx) {
            return;
        }
        let pending_fence = self
            .pending_code_fence
            .take()
            .is_some_and(|(line, _)| line == self.cursor_line());
        self.checkpoint();
        let cursor_line_before = self.cursor_line();
        let cursor_had_inline_headroom = self.primary_cursor_has_inline_edit_headroom();

        if self.has_extra_cursors() {
            self.multi_cursor_enter();
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }

        let cursor_line = self.cursor_line();
        let line_start = self.buffer.line_start(cursor_line);
        let line_end = self.buffer.line_end(cursor_line);
        let line_content = self.buffer.line_content(cursor_line).to_string();
        let block_type = self
            .decorations
            .get(cursor_line)
            .map(|d| d.block_type.clone())
            .unwrap_or(BlockType::Normal);

        // Blockquote lines continue with their own `> ` prefix, and any list
        // nested inside one is measured from after that prefix.
        let quote_prefix_len = markdown::blockquote_prefix_len(&line_content);
        let quote_prefix = &line_content[..quote_prefix_len];
        let (ws_str, trimmed) = markdown::split_continuation_prefix(&line_content);
        let body_indent = ws_str.len() - quote_prefix_len;
        let leading_ws = quote_prefix_len + body_indent;

        match block_type {
            BlockType::UnorderedList => {
                let marker_len = if trimmed.starts_with("- ")
                    || trimmed.starts_with("* ")
                    || trimmed.starts_with("+ ")
                {
                    2
                } else {
                    0
                };
                if marker_len > 0 && self.cursor.offset >= line_start + leading_ws + marker_len {
                    let text_after_marker = &line_content[leading_ws + marker_len..];
                    if text_after_marker.trim().is_empty() {
                        if !self.outdent_empty_list_item(
                            line_start,
                            line_end,
                            &line_content,
                            quote_prefix_len,
                            body_indent,
                            2,
                        ) {
                            let cleared = quote_prefix.to_string();
                            self.queue_non_markdown_line_display_transfer(
                                &(line_start..line_end),
                                &cleared,
                            );
                            self.buffer.replace(line_start..line_end, &cleared);
                            self.cursor
                                .move_to(line_start + cleared.len(), &self.buffer);
                            self.cursor.insert(&mut self.buffer, "\n");
                        }
                    } else {
                        let marker = &trimmed[..marker_len];
                        let continuation = format!("\n{}{}", ws_str, marker);
                        let insert_range = self.cursor.selection_range();
                        self.queue_non_markdown_line_display_transfer(&insert_range, &continuation);
                        self.cursor.insert(&mut self.buffer, &continuation);
                    }
                    self.reparse();
                    if !cursor_had_inline_headroom {
                        self.scroll_to_cursor();
                    }
                    self.pause_blinking(cx);
                    return;
                }
            }
            BlockType::TaskList(_) => {
                let prefix_len = task_prefix_len(trimmed);
                if prefix_len > 0
                    && self.cursor.offset >= line_start + leading_ws + prefix_len
                    && let Some((marker, ordered_number)) = task_list_continuation(trimmed)
                {
                    let text_after_marker = &line_content[leading_ws + prefix_len..];
                    let empty = text_after_marker.trim().is_empty();
                    if empty {
                        if !self.outdent_empty_list_item(
                            line_start,
                            line_end,
                            &line_content,
                            quote_prefix_len,
                            body_indent,
                            if ordered_number.is_some() { 3 } else { 2 },
                        ) {
                            let cleared = quote_prefix.to_string();
                            self.queue_non_markdown_line_display_transfer(
                                &(line_start..line_end),
                                &cleared,
                            );
                            self.buffer.replace(line_start..line_end, &cleared);
                            self.cursor
                                .move_to(line_start + cleared.len(), &self.buffer);
                            self.cursor.insert(&mut self.buffer, "\n");
                        }
                    } else {
                        let continuation = format!("\n{}{}", ws_str, marker);
                        let insert_range = self.cursor.selection_range();
                        self.queue_non_markdown_line_display_transfer(&insert_range, &continuation);
                        self.cursor.insert(&mut self.buffer, &continuation);
                    }
                    self.reparse();
                    if let Some(number) = ordered_number {
                        if empty {
                            self.normalize_ordered_list_near_line(cursor_line, false);
                        } else {
                            self.renumber_ordered_list_after(
                                cursor_line + 2,
                                number.saturating_add(2),
                                leading_ws,
                            );
                        }
                        self.reparse();
                    }
                    if !cursor_had_inline_headroom {
                        self.scroll_to_cursor();
                    }
                    self.pause_blinking(cx);
                    return;
                }
            }
            BlockType::OrderedList => {
                let prefix_len = markdown::ordered_list_prefix_len(trimmed);
                if prefix_len > 0 && self.cursor.offset >= line_start + leading_ws + prefix_len {
                    let number: u32 = trimmed[..prefix_len.saturating_sub(2)].parse().unwrap_or(1);
                    let text_after_marker = &line_content[leading_ws + prefix_len..];
                    if text_after_marker.trim().is_empty() {
                        if !self.outdent_empty_list_item(
                            line_start,
                            line_end,
                            &line_content,
                            quote_prefix_len,
                            body_indent,
                            3,
                        ) {
                            let cleared = quote_prefix.to_string();
                            self.queue_non_markdown_line_display_transfer(
                                &(line_start..line_end),
                                &cleared,
                            );
                            self.buffer.replace(line_start..line_end, &cleared);
                            self.cursor
                                .move_to(line_start + cleared.len(), &self.buffer);
                            self.cursor.insert(&mut self.buffer, "\n");
                        }
                        self.reparse();
                        self.normalize_ordered_list_near_line(cursor_line, false);
                    } else {
                        let continuation = format!("\n{}{}. ", ws_str, number + 1);
                        let insert_range = self.cursor.selection_range();
                        self.queue_non_markdown_line_display_transfer(&insert_range, &continuation);
                        self.cursor.insert(&mut self.buffer, &continuation);
                        self.reparse();
                        self.renumber_ordered_list_after(cursor_line + 2, number + 2, leading_ws);
                        self.reparse();
                    }
                    if !cursor_had_inline_headroom {
                        self.scroll_to_cursor();
                    }
                    self.pause_blinking(cx);
                    return;
                }
            }
            BlockType::Blockquote if quote_prefix_len > 0 => {
                if self.cursor.offset >= line_start + quote_prefix_len {
                    // Enter on an empty quote line leaves the quote instead of
                    // adding another empty `>` row.
                    if line_content[quote_prefix_len..].trim().is_empty() {
                        self.queue_non_markdown_line_display_transfer(&(line_start..line_end), "");
                        self.buffer.replace(line_start..line_end, "");
                        self.cursor.move_to(line_start, &self.buffer);
                        self.cursor.insert(&mut self.buffer, "\n");
                    } else {
                        let continuation = format!("\n{}", quote_prefix);
                        let insert_range = self.cursor.selection_range();
                        self.queue_non_markdown_line_display_transfer(&insert_range, &continuation);
                        self.cursor.insert(&mut self.buffer, &continuation);
                    }
                    self.reparse();
                    if !cursor_had_inline_headroom {
                        self.scroll_to_cursor();
                    }
                    self.pause_blinking(cx);
                    return;
                }
            }
            _ => {}
        }

        if self.active_tab_is_markdown()
            && self.table_source.is_none()
            && self.cursor.offset == line_end
            && let Some(closer) = super::input_ime::code_fence_enter_closer(
                self.buffer.text(),
                cursor_line,
                pending_fence,
            )
        {
            let continuation = format!("\n\n{closer}");
            let insert_range = self.cursor.selection_range();
            let body_offset = insert_range.start + 1;
            self.queue_non_markdown_line_display_transfer(&insert_range, &continuation);
            self.cursor.insert(&mut self.buffer, &continuation);
            self.cursor.move_to(body_offset, &self.buffer);
            self.reparse();
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }

        let insert_range = self.cursor.selection_range();
        self.queue_non_markdown_line_display_transfer(&insert_range, "\n");
        self.cursor.insert(&mut self.buffer, "\n");
        self.reparse();
        if !cursor_had_inline_headroom && self.cursor_line() != cursor_line_before {
            self.scroll_to_cursor();
        }
        self.pause_blinking(cx);
    }

    pub(crate) fn on_insert_soft_break(
        &mut self,
        _: &InsertSoftBreak,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_content_edit(cx) {
            return;
        }
        if !self.folded_sections.is_empty() {
            self.checkpoint();
            if self.skip_folded_anchor_for_line_break() {
                self.pending_code_fence = None;
                self.multi_cursor_insert("\n");
                self.reparse();
                self.scroll_to_cursor();
                self.pause_blinking(cx);
                return;
            }
        }
        if self.source_mode {
            self.pending_code_fence = None;
            self.checkpoint();
            self.multi_cursor_insert("\n");
            self.reparse();
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }
        if self.insert_front_matter_break(cx) {
            return;
        }
        if self.table_boundary_caret.is_some() {
            let _ = self.insert_line_at_table_boundary(cx);
            return;
        }
        if self.insert_table_cell_break(true, cx) {
            return;
        }
        self.pending_code_fence = None;
        self.checkpoint();
        self.multi_cursor_insert("\n");
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_tab(&mut self, _: &Tab, _: &mut Window, cx: &mut Context<Self>) {
        if !self.begin_content_edit(cx) {
            return;
        }
        if self.active_tab_is_non_markdown_text() {
            self.indent_code_selection(true, cx);
            return;
        }
        if self.table_boundary_caret.is_some() {
            let _ = self.move_table_cursor_horizontal(true);
            self.pause_blinking(cx);
            cx.notify();
            return;
        }
        if self.indent_table_cell_lists(true, cx) {
            return;
        }
        if self.table_move_to_adjacent_cell(true, cx) {
            return;
        }
        self.checkpoint();
        let (line_indices, has_selection, has_list_line) =
            self.collect_line_indices_for_block_indent();
        let indent_text = self.current_indent_text();

        if has_list_line {
            self.indent_selected_list_lines();
        } else if has_selection {
            self.indent_line_indices(&line_indices, &indent_text);
        } else {
            if self.has_extra_cursors() {
                self.multi_cursor_insert(&indent_text);
                self.reparse();
                self.scroll_to_cursor();
            } else {
                let insert_range = self.cursor.selection_range();
                self.queue_non_markdown_line_display_transfer(&insert_range, &indent_text);
                self.cursor.insert(&mut self.buffer, &indent_text);
                self.reparse();
                self.scroll_to_cursor();
            }
        }
        self.pause_blinking(cx);
    }

    pub(crate) fn on_back_tab(&mut self, _: &BackTab, _: &mut Window, cx: &mut Context<Self>) {
        if !self.begin_content_edit(cx) {
            return;
        }
        if self.active_tab_is_non_markdown_text() {
            self.indent_code_selection(false, cx);
            return;
        }
        if self.table_boundary_caret.is_some() {
            let _ = self.move_table_cursor_horizontal(false);
            self.pause_blinking(cx);
            cx.notify();
            return;
        }
        if self.indent_table_cell_lists(false, cx) {
            return;
        }
        if self.table_move_to_adjacent_cell(false, cx) {
            return;
        }
        self.checkpoint();
        let (line_indices, has_selection, has_list_line) =
            self.collect_line_indices_for_block_indent();
        let unindent_width = self.current_unindent_width();

        if has_list_line {
            self.unindent_selected_list_lines();
        } else if has_selection || self.active_tab_is_non_markdown_text() {
            self.unindent_line_indices(&line_indices, unindent_width);
        }
        self.pause_blinking(cx);
    }

    pub(super) fn copy_line_diff_selection_to_clipboard(&self, cx: &mut Context<Self>) -> bool {
        if let Some(selection) = self.line_diff_selection {
            if let Some(line) = self
                .line_diff_selection_layouts
                .iter()
                .find(|line| line.hunk_index == selection.hunk_index)
                && let Some(text) = line.source_text.get(selection.range())
                && !text.is_empty()
            {
                cx.write_to_clipboard(gpui::ClipboardItem::new_string(text.to_string()));
            }
            return true;
        }
        false
    }

    pub(crate) fn on_copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if self.copy_line_diff_selection_to_clipboard(cx) {
            return;
        }
        if let Some(text) = self.copy_table_cell_selection() {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            return;
        }
        if let Some(text) = self.copy_table_border_selection() {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(text));
            return;
        }
        if let Some(range) = self.table_boundary_block_range() {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                self.buffer.text_for_range(range).to_string(),
            ));
            return;
        }
        self.copy_selection_to_clipboard(cx);
    }

    pub(crate) fn on_cut(&mut self, _: &Cut, _: &mut Window, cx: &mut Context<Self>) {
        if self.line_diff_selection.is_some() {
            return;
        }
        if self.table_cell_selection.is_some() {
            self.cut_table_cell_selection(cx);
            return;
        }
        if self.table_handles.selection.is_some() {
            self.cut_table_border_selection(cx);
            return;
        }
        if let Some(boundary) = self.table_boundary_caret
            && let Some(range) = self.table_boundary_block_range()
            && self.begin_content_edit(cx)
        {
            cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                self.buffer.text_for_range(range).to_string(),
            ));
            self.table_boundary_caret = None;
            self.table_boundary_drag_origin = None;
            self.table_delete_table(boundary.table_start_line, cx);
            self.pause_blinking(cx);
            return;
        }
        self.cut_selection_to_clipboard(cx);
    }

    pub(crate) fn on_paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        self.paste_from_clipboard(cx);
    }

    pub(crate) fn on_paste_plain_text(
        &mut self,
        _: &PastePlainText,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_content_edit(cx) {
            return;
        }
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            if self.table_cell_selection.is_some() {
                self.paste_table_cell_selection(&text, true, cx);
                return;
            }
            if self.table_handles.selection.is_some() {
                self.paste_table_border_selection(&text, cx);
                return;
            }
            if self.table_boundary_caret.is_some() && !self.insert_line_at_table_boundary(cx) {
                return;
            }
            self.insert_pasted_text(&text, cx);
        }
    }

    pub(crate) fn on_show_character_palette(
        &mut self,
        _: &ShowCharacterPalette,
        window: &mut Window,
        _cx: &mut Context<Self>,
    ) {
        window.show_character_palette();
    }

    pub(crate) fn on_move_word_left(
        &mut self,
        _: &MoveWordLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.move_line_diff_selection_word(false, false, cx) {
            return;
        }
        if self.table_boundary_caret.is_some() {
            let _ = self.move_table_cursor_horizontal(false);
            self.finish_horizontal_cursor_move(cx);
            return;
        }
        if self.has_extra_cursors() {
            self.multi_cursor_move_word_left();
        } else {
            let previous = [self.cursor];
            let current = self.cursor.offset;
            let mut probe = self.cursor;
            probe.move_word_left(&self.buffer);
            if let Some(target) = self.table_visible_word_target(current, probe.offset, false) {
                self.cursor.move_to(target, &self.buffer);
            } else if self.table_cell_for_offset(current).is_none() {
                self.cursor = probe;
            }
            self.skip_folded_caret_navigation(&previous);
        }
        self.finish_horizontal_cursor_move(cx);
    }

    pub(crate) fn on_move_word_right(
        &mut self,
        _: &MoveWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.move_line_diff_selection_word(true, false, cx) {
            return;
        }
        if self.table_boundary_caret.is_some() {
            let _ = self.move_table_cursor_horizontal(true);
            self.finish_horizontal_cursor_move(cx);
            return;
        }
        if self.has_extra_cursors() {
            self.multi_cursor_move_word_right();
        } else {
            let previous = [self.cursor];
            let current = self.cursor.offset;
            let mut probe = self.cursor;
            probe.move_word_right(&self.buffer);
            if let Some(target) = self.table_visible_word_target(current, probe.offset, true) {
                self.cursor.move_to(target, &self.buffer);
            } else if self.table_cell_for_offset(current).is_none() {
                self.cursor = probe;
            }
            self.skip_folded_caret_navigation(&previous);
        }
        self.finish_horizontal_cursor_move(cx);
    }

    pub(crate) fn on_select_word_left(
        &mut self,
        _: &SelectWordLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.move_line_diff_selection_word(false, true, cx) {
            return;
        }
        let previous = self.all_cursors_snapshot();
        self.table_boundary_caret = None;
        self.reset_cursor_visual_targeting();
        let current = self.cursor.offset;
        let mut probe = self.cursor;
        probe.select_word_left(&self.buffer);
        if let Some(target) = self.table_visible_word_target(current, probe.offset, false) {
            self.cursor.select_to(target, &self.buffer);
        } else if self.table_cell_for_offset(current).is_none() {
            self.cursor = probe;
        }
        for cursor in &mut self.extra_cursors {
            cursor.select_word_left(&self.buffer);
        }
        self.skip_folded_selection_navigation(&previous);
        self.dedup_cursors();
        self.finish_horizontal_cursor_move(cx);
    }

    pub(crate) fn on_select_word_right(
        &mut self,
        _: &SelectWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.move_line_diff_selection_word(true, true, cx) {
            return;
        }
        let previous = self.all_cursors_snapshot();
        self.table_boundary_caret = None;
        self.reset_cursor_visual_targeting();
        let current = self.cursor.offset;
        let mut probe = self.cursor;
        probe.select_word_right(&self.buffer);
        if let Some(target) = self.table_visible_word_target(current, probe.offset, true) {
            self.cursor.select_to(target, &self.buffer);
        } else if self.table_cell_for_offset(current).is_none() {
            self.cursor = probe;
        }
        for cursor in &mut self.extra_cursors {
            cursor.select_word_right(&self.buffer);
        }
        self.skip_folded_selection_navigation(&previous);
        self.dedup_cursors();
        self.finish_horizontal_cursor_move(cx);
    }

    pub(crate) fn on_move_to_document_start(
        &mut self,
        _: &MoveToDocumentStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.move_line_diff_selection_document_edge(false, false, cx) {
            return;
        }
        let previous = self.all_cursors_snapshot();
        self.table_boundary_caret = None;
        self.cursor.move_to(0, &self.buffer);
        for cursor in &mut self.extra_cursors {
            cursor.move_to(0, &self.buffer);
        }
        self.skip_folded_caret_navigation(&previous);
        self.dedup_cursors();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_move_to_document_end(
        &mut self,
        _: &MoveToDocumentEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.move_line_diff_selection_document_edge(true, false, cx) {
            return;
        }
        let previous = self.all_cursors_snapshot();
        self.table_boundary_caret = None;
        let end = self.buffer.len();
        self.cursor.move_to(end, &self.buffer);
        for cursor in &mut self.extra_cursors {
            cursor.move_to(end, &self.buffer);
        }
        self.skip_folded_caret_navigation(&previous);
        self.dedup_cursors();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_select_to_document_start(
        &mut self,
        _: &SelectToDocumentStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.move_line_diff_selection_document_edge(false, true, cx) {
            return;
        }
        let previous = self.all_cursors_snapshot();
        self.table_boundary_caret = None;
        self.cursor.select_to(0, &self.buffer);
        for cursor in &mut self.extra_cursors {
            cursor.select_to(0, &self.buffer);
        }
        self.skip_folded_selection_navigation(&previous);
        self.dedup_cursors();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_select_to_document_end(
        &mut self,
        _: &SelectToDocumentEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.move_line_diff_selection_document_edge(true, true, cx) {
            return;
        }
        let previous = self.all_cursors_snapshot();
        self.table_boundary_caret = None;
        let end = self.buffer.len();
        self.cursor.select_to(end, &self.buffer);
        for cursor in &mut self.extra_cursors {
            cursor.select_to(end, &self.buffer);
        }
        self.skip_folded_selection_navigation(&previous);
        self.dedup_cursors();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn delete_to_line_boundary(&mut self, to_start: bool, cx: &mut Context<Self>) {
        self.delete_to_line_boundary_impl(to_start, false, cx);
    }

    fn delete_to_line_boundary_impl(&mut self, to_start: bool, kill: bool, cx: &mut Context<Self>) {
        if !self.begin_content_edit(cx) {
            return;
        }
        if self.handle_folded_boundary_delete(DeleteAction::LineBoundary { to_start, kill }) {
            self.pause_blinking(cx);
            return;
        }
        if self.table_boundary_caret.is_some() {
            self.pause_blinking(cx);
            return;
        }
        self.checkpoint();
        self.apply_semantic_delete(DeleteAction::LineBoundary { to_start, kill });
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_delete_to_line_start(
        &mut self,
        _: &DeleteToLineStart,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.delete_to_line_boundary(true, cx);
    }

    pub(crate) fn on_delete_to_line_end(
        &mut self,
        _: &DeleteToLineEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.delete_to_line_boundary(false, cx);
    }

    pub(crate) fn on_kill_to_line_end(
        &mut self,
        _: &KillToLineEnd,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.delete_to_line_boundary_impl(false, true, cx);
    }

    pub(crate) fn on_yank(&mut self, _: &Yank, _: &mut Window, cx: &mut Context<Self>) {
        if !self.begin_content_edit(cx) {
            return;
        }
        let Some(text) = self.kill_ring.clone() else {
            return;
        };
        if text.is_empty() {
            return;
        }
        if self.table_boundary_caret.is_some() && !self.insert_line_at_table_boundary(cx) {
            return;
        }
        self.insert_pasted_text(&text, cx);
    }

    /// Inserts a line break after the cursor without moving it (emacs ctrl-o).
    pub(crate) fn on_open_line(&mut self, _: &OpenLine, _: &mut Window, cx: &mut Context<Self>) {
        if !self.begin_content_edit(cx) {
            return;
        }
        if self.has_extra_cursors() {
            if self.table_boundary_caret.is_some() {
                self.pause_blinking(cx);
                return;
            }
            self.unfold_cursor_regions();
            let plans: Option<Vec<_>> = self
                .cursor_states_with_primary_flag()
                .into_iter()
                .map(|(cursor, primary)| {
                    self.semantic_open_line_plan_for_cursor(cursor)
                        .map(|plan| (plan, primary))
                })
                .collect();
            if let Some(plans) = plans {
                self.checkpoint();
                self.pending_line_display_transfer = None;
                if self.apply_table_aware_cursor_plans(plans) {
                    self.mark_active_tab_dirty();
                    self.scroll_to_cursor();
                }
            }
            self.pause_blinking(cx);
            return;
        }
        self.skip_folded_anchor_for_line_break();
        if self.table_boundary_caret.is_some() {
            let _ = self.insert_line_at_table_boundary(cx);
            return;
        }
        let Some(plan) = self.semantic_open_line_plan() else {
            self.pause_blinking(cx);
            return;
        };
        self.checkpoint();
        self.queue_non_markdown_line_display_transfer(&plan.range, &plan.text);
        self.apply_table_aware_cursor_plans(vec![(plan, true)]);
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    /// Swaps the graphemes around the cursor and advances it (emacs ctrl-t).
    pub(crate) fn on_transpose_chars(
        &mut self,
        _: &TransposeChars,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_content_edit(cx) {
            return;
        }
        if self.table_boundary_caret.is_some() || self.table_cursor_cell().is_some() {
            self.pause_blinking(cx);
            return;
        }
        if self.has_extra_cursors() {
            let cursors = self.cursor_states_with_primary_flag();
            if self.active_tab_is_markdown()
                && self.table_source.is_none()
                && cursors.iter().any(|(cursor, _)| {
                    let line = self.buffer.offset_to_line_col(cursor.offset).0;
                    self.decorations
                        .get(line)
                        .is_some_and(|dec| dec.table_info.is_some())
                })
            {
                self.pause_blinking(cx);
                return;
            }
            let plans: Option<Vec<_>> = cursors
                .into_iter()
                .map(|(cursor, primary)| transpose_plan(&self.buffer, cursor).map(|p| (p, primary)))
                .collect();
            if let Some(plans) = plans {
                self.checkpoint();
                self.pending_line_display_transfer = None;
                if self.apply_table_aware_cursor_plans(plans) {
                    self.mark_active_tab_dirty();
                    self.scroll_to_cursor();
                }
            }
            self.pause_blinking(cx);
            return;
        }
        self.clear_extra_cursors();
        if self.cursor.has_selection() {
            return;
        }
        let offset = self.cursor.offset;
        let (line, _) = self.buffer.offset_to_line_col(offset);
        let line_start = self.buffer.line_start(line);
        let line_end = self.buffer.line_end(line);
        // At line end, transpose the last two graphemes instead.
        let pivot = if offset >= line_end {
            self.buffer.prev_grapheme_boundary(offset.min(line_end))
        } else {
            offset
        };
        let left_start = self.buffer.prev_grapheme_boundary(pivot);
        let right_end = self.buffer.next_grapheme_boundary(pivot);
        if left_start >= pivot
            || pivot >= right_end
            || left_start < line_start
            || right_end > line_end
        {
            return;
        }
        self.checkpoint();
        let left = self.buffer.text_for_range(left_start..pivot).to_string();
        let right = self.buffer.text_for_range(pivot..right_end).to_string();
        let swapped = format!("{right}{left}");
        self.buffer.replace(left_start..right_end, &swapped);
        self.cursor.move_to(right_end, &self.buffer);
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    /// Moves the cursor down one viewport page (emacs/macOS ctrl-v).
    pub(crate) fn on_move_page_down(
        &mut self,
        _: &MovePageDown,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.line_diff_selection.is_some() {
            let mut remaining = self.editor_visible_height();
            while remaining > 0.0 {
                let Some(selection) = self.line_diff_selection else {
                    break;
                };
                let Some(layout) = self
                    .line_diff_selection_layouts
                    .iter()
                    .find(|layout| layout.hunk_index == selection.hunk_index)
                else {
                    break;
                };
                remaining -= layout.line_height.max(1.0);
                self.move_line_diff_selection_vertical(true, false, cx);
                if self.line_diff_selection == Some(selection) {
                    break;
                }
            }
            return;
        }
        let line_height = (self.base_font_size()
            * self.styles.editor.editor_paragraph_line_height_factor)
            .max(1.0);
        let use_editor_geometry = self.focused_sidebar_path.is_none()
            && self.wiki_link_start.is_none()
            && self.slash_menu_start.is_none()
            && !self.has_extra_cursors();
        let mut remaining_height = self.editor_visible_height();
        if !use_editor_geometry {
            let steps = ((remaining_height / line_height).floor() as usize).max(1);
            for _ in 0..steps {
                self.on_down(&Down, window, cx);
            }
            return;
        }
        let goal_x = self
            .vertical_move_goal_x
            .or_else(|| self.table_visual_x_for_offset(window, self.cursor.offset))
            .or_else(|| self.cursor_visual_x(self.cursor.offset))
            .unwrap_or_default();
        self.vertical_move_goal_x = Some(goal_x);
        let mut first_step = true;
        loop {
            let before = self.cursor.offset;
            let before_affinity = self.wrap_boundary_affinity_for_offset(before);
            let before_bounds =
                self.document_row_bounds_for_offset(window, before, before_affinity);
            let target = self
                .table_vertical_target(window, before, true, goal_x)
                .or_else(|| {
                    self.offset_for_visual_down(before).map(|offset| {
                        (
                            offset,
                            self.wrap_boundary_affinity_for_goal_x(offset, goal_x),
                        )
                    })
                });
            let fallback_height = before_bounds
                .map(|(top, bottom)| bottom - top)
                .unwrap_or(line_height)
                .max(1.0);
            let distance = target
                .and_then(|(offset, affinity)| {
                    let (top, _) = self.document_row_bounds_for_offset(window, offset, affinity)?;
                    Some((top - before_bounds?.0).max(1.0))
                })
                .unwrap_or(fallback_height);
            if !page_down_row_fits(remaining_height, distance, first_step) {
                break;
            }
            self.on_down(&Down, window, cx);
            if self.cursor.offset == before
                && self.wrap_boundary_affinity_for_offset(before) == before_affinity
            {
                break;
            }
            let actual_distance = self
                .document_row_bounds_for_offset(
                    window,
                    self.cursor.offset,
                    self.wrap_boundary_affinity_for_offset(self.cursor.offset),
                )
                .zip(before_bounds)
                .map(|((top, _), (before_top, _))| (top - before_top).max(1.0))
                .unwrap_or(distance);
            remaining_height -= actual_distance;
            first_step = false;
        }
    }

    pub(crate) fn on_delete_word_left(
        &mut self,
        _: &DeleteWordLeft,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_content_edit(cx) {
            return;
        }
        if self.handle_folded_boundary_delete(DeleteAction::Word { backwards: true }) {
            self.pause_blinking(cx);
            return;
        }
        if self.table_boundary_caret.is_some() {
            self.pause_blinking(cx);
            return;
        }
        self.checkpoint();
        let cursor_line_before = self.cursor_line();
        let cursor_had_inline_headroom = self.primary_cursor_has_inline_edit_headroom();
        self.multi_cursor_delete_word_left();
        self.reparse();
        if !cursor_had_inline_headroom || self.cursor_line() != cursor_line_before {
            self.scroll_to_cursor();
        }
        self.pause_blinking(cx);
    }

    pub(crate) fn on_delete_word_right(
        &mut self,
        _: &DeleteWordRight,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_content_edit(cx) {
            return;
        }
        if self.handle_folded_boundary_delete(DeleteAction::Word { backwards: false }) {
            self.pause_blinking(cx);
            return;
        }
        if self.table_boundary_caret.is_some() {
            self.pause_blinking(cx);
            return;
        }
        self.checkpoint();
        let cursor_line_before = self.cursor_line();
        let cursor_had_inline_headroom = self.primary_cursor_has_inline_edit_headroom();
        self.multi_cursor_delete_word_right();
        self.reparse();
        if !cursor_had_inline_headroom || self.cursor_line() != cursor_line_before {
            self.scroll_to_cursor();
        }
        self.pause_blinking(cx);
    }

    // ── Format actions ──────────────────────────────────────────────

    fn begin_markdown_format_edit(&mut self, cx: &mut Context<Self>) -> bool {
        self.active_tab_is_markdown()
            && self.begin_content_edit(cx)
            && (self.table_boundary_caret.is_none() || self.insert_line_at_table_boundary(cx))
    }

    fn begin_markdown_block_format_edit(&mut self, cx: &mut Context<Self>) -> bool {
        (self.table_boundary_caret.is_some() || self.table_block_format_allowed(false))
            && self.begin_markdown_format_edit(cx)
    }

    pub(crate) fn on_toggle_bold(
        &mut self,
        _: &ToggleBold,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_markdown_format_edit(cx) {
            return;
        }
        self.checkpoint();
        self.apply_wrap("**", "**");
        self.pause_blinking(cx);
    }

    pub(crate) fn on_toggle_italic(
        &mut self,
        _: &ToggleItalic,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_markdown_format_edit(cx) {
            return;
        }
        self.checkpoint();
        self.apply_wrap("*", "*");
        self.pause_blinking(cx);
    }

    pub(crate) fn on_toggle_underline(
        &mut self,
        _: &ToggleUnderline,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_markdown_format_edit(cx) {
            return;
        }
        self.checkpoint();
        self.apply_wrap("++", "++");
        self.pause_blinking(cx);
    }

    pub(crate) fn on_toggle_strikethrough(
        &mut self,
        _: &ToggleStrikethrough,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_markdown_format_edit(cx) {
            return;
        }
        self.checkpoint();
        self.apply_wrap("~~", "~~");
        self.pause_blinking(cx);
    }

    pub(crate) fn on_toggle_inline_code(
        &mut self,
        _: &ToggleInlineCode,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_markdown_format_edit(cx) {
            return;
        }
        self.checkpoint();
        self.apply_wrap("`", "`");
        self.pause_blinking(cx);
    }

    pub(crate) fn on_toggle_link(
        &mut self,
        _: &ToggleLink,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_markdown_format_edit(cx) {
            return;
        }
        self.execute_format_menu_action(FormatMenuAction::Link, cx);
    }

    pub(crate) fn on_toggle_image(
        &mut self,
        _: &ToggleImage,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_markdown_format_edit(cx) {
            return;
        }
        self.checkpoint();
        if self.app_settings.link_syntax == "wikilink" {
            self.insert_wiki_image();
        } else {
            self.insert_link_or_image("![", "](image.png)", "alt");
        }
        self.pause_blinking(cx);
    }

    pub(crate) fn on_toggle_quote(
        &mut self,
        _: &ToggleQuote,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_markdown_block_format_edit(cx) {
            return;
        }
        self.checkpoint();
        self.toggle_current_line_prefix("> ");
        self.pause_blinking(cx);
    }

    pub(crate) fn on_toggle_todo(
        &mut self,
        _: &ToggleTodo,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_markdown_format_edit(cx) {
            return;
        }
        self.checkpoint();
        self.toggle_current_line_prefix("- [ ] ");
        self.pause_blinking(cx);
    }

    pub(crate) fn on_toggle_unordered_list(
        &mut self,
        _: &ToggleUnorderedList,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_markdown_format_edit(cx) {
            return;
        }
        self.checkpoint();
        self.toggle_current_line_prefix("- ");
        self.pause_blinking(cx);
    }

    pub(crate) fn on_toggle_ordered_list(
        &mut self,
        _: &ToggleOrderedList,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_markdown_format_edit(cx) {
            return;
        }
        self.checkpoint();
        self.toggle_current_line_prefix("1. ");
        self.normalize_ordered_lists_near_all_cursors(true);
        self.reparse();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_insert_table(
        &mut self,
        _: &InsertTable,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab_is_markdown() {
            self.execute_format_menu_action(FormatMenuAction::Table, cx);
        }
    }

    pub(crate) fn on_insert_code_block(
        &mut self,
        _: &InsertCodeBlock,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_markdown_block_format_edit(cx) {
            return;
        }
        self.checkpoint();
        self.wrap_current_lines_as_code_block();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_insert_horizontal_rule(
        &mut self,
        _: &InsertHorizontalRule,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_markdown_block_format_edit(cx) {
            return;
        }
        self.checkpoint();
        self.toggle_horizontal_rule_at_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_insert_heading1(
        &mut self,
        _: &InsertHeading1,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_markdown_block_format_edit(cx) {
            return;
        }
        self.checkpoint();
        self.apply_heading(1);
        self.pause_blinking(cx);
    }
    pub(crate) fn on_insert_heading2(
        &mut self,
        _: &InsertHeading2,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_markdown_block_format_edit(cx) {
            return;
        }
        self.checkpoint();
        self.apply_heading(2);
        self.pause_blinking(cx);
    }
    pub(crate) fn on_insert_heading3(
        &mut self,
        _: &InsertHeading3,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_markdown_block_format_edit(cx) {
            return;
        }
        self.checkpoint();
        self.apply_heading(3);
        self.pause_blinking(cx);
    }
    pub(crate) fn on_insert_heading4(
        &mut self,
        _: &InsertHeading4,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_markdown_block_format_edit(cx) {
            return;
        }
        self.checkpoint();
        self.apply_heading(4);
        self.pause_blinking(cx);
    }
    pub(crate) fn on_insert_heading5(
        &mut self,
        _: &InsertHeading5,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_markdown_block_format_edit(cx) {
            return;
        }
        self.checkpoint();
        self.apply_heading(5);
        self.pause_blinking(cx);
    }
    pub(crate) fn on_insert_heading6(
        &mut self,
        _: &InsertHeading6,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_markdown_block_format_edit(cx) {
            return;
        }
        self.checkpoint();
        self.apply_heading(6);
        self.pause_blinking(cx);
    }

    pub(crate) fn on_set_paragraph(
        &mut self,
        _: &SetParagraph,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.begin_markdown_block_format_edit(cx) {
            return;
        }
        if let Some(level) = self
            .current_heading_action()
            .and_then(|action| match action {
                FormatMenuAction::Heading1 => Some(1),
                FormatMenuAction::Heading2 => Some(2),
                FormatMenuAction::Heading3 => Some(3),
                FormatMenuAction::Heading4 => Some(4),
                FormatMenuAction::Heading5 => Some(5),
                FormatMenuAction::Heading6 => Some(6),
                _ => None,
            })
        {
            self.checkpoint();
            self.apply_heading(level);
            self.pause_blinking(cx);
        }
    }

    pub(crate) fn on_zoom_in(&mut self, _: &ZoomIn, _: &mut Window, cx: &mut Context<Self>) {
        self.set_ui_scale(self.ui_scale + self.styles.editor.editor_scale_step, cx);
        self.app_settings.save();
    }
    pub(crate) fn on_zoom_out(&mut self, _: &ZoomOut, _: &mut Window, cx: &mut Context<Self>) {
        self.set_ui_scale(self.ui_scale - self.styles.editor.editor_scale_step, cx);
        self.app_settings.save();
    }
    pub(crate) fn on_zoom_reset(&mut self, _: &ZoomReset, _: &mut Window, cx: &mut Context<Self>) {
        self.set_ui_scale(1.0, cx);
        self.app_settings.save();
    }

    pub(crate) fn on_toggle_sidebar(
        &mut self,
        _: &ToggleSidebar,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspaces.is_empty() {
            self.pick_workspace_folder(cx);
        } else {
            self.toggle_sidebar_visibility(cx);
        }
    }

    pub(crate) fn on_toggle_terminal_panel(
        &mut self,
        _: &ToggleTerminalPanel,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let had_terminals = self.has_terminal_tabs();
        self.toggle_terminal_panel(cx);
        if had_terminals {
            window.focus(&self.focus_handle(cx));
        }
    }

    pub(crate) fn on_open_settings(
        &mut self,
        _: &OpenSettings,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_settings_tab(cx);
    }

    pub(crate) fn on_toggle_outline(
        &mut self,
        _: &ToggleOutline,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_outline_visibility(cx);
    }

    pub(crate) fn on_toggle_info_panel(
        &mut self,
        _: &ToggleInfoPanel,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_info_panel(cx);
    }

    pub(crate) fn on_toggle_theme(
        &mut self,
        _: &ToggleTheme,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_theme(cx);
    }

    pub(crate) fn on_toggle_theme_debugger(
        &mut self,
        _: &ToggleThemeDebugger,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_theme_debugger_visibility(cx);
    }

    pub(crate) fn on_toggle_editor_left_align(
        &mut self,
        _: &ToggleEditorLeftAlign,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.toggle_editor_alignment(cx);
    }

    pub(crate) fn on_toggle_word_wrap(
        &mut self,
        _: &ToggleWordWrap,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.editor_uses_code_gutter() {
            self.toggle_editor_alignment(cx);
        }
    }

    pub(crate) fn on_toggle_search_case_sensitive(
        &mut self,
        _: &ToggleSearchCaseSensitive,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace_search_visible {
            self.workspace_search_case_sensitive = !self.workspace_search_case_sensitive;
            self.run_workspace_search(cx);
        } else if self.search_visible {
            self.search_case_sensitive = !self.search_case_sensitive;
            self.run_search(cx);
        }
    }

    pub(crate) fn on_toggle_search_whole_word(
        &mut self,
        _: &ToggleSearchWholeWord,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace_search_visible {
            self.workspace_search_whole_word = !self.workspace_search_whole_word;
            self.run_workspace_search(cx);
        } else if self.search_visible {
            self.search_whole_word = !self.search_whole_word;
            self.run_search(cx);
        }
    }

    pub(crate) fn on_toggle_search_regex(
        &mut self,
        _: &ToggleSearchRegex,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.workspace_search_visible {
            self.workspace_search_use_regex = !self.workspace_search_use_regex;
            self.run_workspace_search(cx);
        } else if self.search_visible {
            self.search_use_regex = !self.search_use_regex;
            self.run_search(cx);
        }
    }

    pub(crate) fn on_select_line(
        &mut self,
        _: &SelectLine,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar_edit.is_some() {
            return;
        }
        self.unfold_cursor_regions();
        if self.has_extra_cursors() || self.cursor.has_selection() {
            let selections =
                selected_logical_lines(&self.buffer, &self.cursor_states_with_primary_flag());
            self.restore_cursor_states(selections);
            self.unfold_cursor_regions();
            self.pause_blinking(cx);
        } else {
            self.select_logical_line_at_offset(self.cursor.offset, cx);
        }
    }

    pub(crate) fn select_logical_line_at_offset(&mut self, offset: usize, cx: &mut Context<Self>) {
        let (line, _) = self
            .buffer
            .offset_to_line_col(offset.min(self.buffer.len()));
        let start = self.buffer.line_start(line);
        let end = self.buffer.line_end(line);
        let select_end = if end < self.buffer.len() {
            end + 1
        } else {
            end
        };
        self.cursor.anchor = start;
        self.cursor.offset = select_end;
        self.pause_blinking(cx);
    }

    pub(crate) fn on_insert_line_below(
        &mut self,
        _: &InsertLineBelow,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar_edit.is_some() {
            return;
        }
        if !self.begin_content_edit(cx) {
            return;
        }
        if self.has_extra_cursors() {
            self.insert_lines_at_all_cursors(MultiLineInsertion::Below, cx);
            return;
        }
        let after_fold = self.skip_folded_anchor_for_line_break();
        self.checkpoint();
        let (line, _) = self.buffer.offset_to_line_col(self.cursor.offset);
        let end = self.buffer.line_end(line);
        let insert_at = if after_fold { self.cursor.offset } else { end };
        self.queue_non_markdown_line_display_transfer(&(insert_at..insert_at), "\n");
        self.buffer.insert(insert_at, "\n");
        self.cursor.offset = insert_at + 1;
        self.cursor.anchor = self.cursor.offset;
        self.mark_active_tab_dirty();
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_insert_line_above(
        &mut self,
        _: &InsertLineAbove,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar_edit.is_some() {
            return;
        }
        if !self.begin_content_edit(cx) {
            return;
        }
        if self.has_extra_cursors() {
            self.insert_lines_at_all_cursors(MultiLineInsertion::Above, cx);
            return;
        }
        self.checkpoint();
        let (line, _) = self.buffer.offset_to_line_col(self.cursor.offset);
        let start = self.buffer.line_start(line);
        self.queue_non_markdown_line_display_transfer(&(start..start), "\n");
        self.buffer.insert(start, "\n");
        self.cursor.offset = start;
        self.cursor.anchor = self.cursor.offset;
        self.mark_active_tab_dirty();
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_duplicate_line(
        &mut self,
        _: &DuplicateLine,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar_edit.is_some() {
            return;
        }
        if !self.begin_content_edit(cx) {
            return;
        }
        if self.cursor.has_selection() || self.extra_cursors.iter().any(Cursor::has_selection) {
            self.copy_selected_lines(false, cx);
            return;
        }
        if self.has_extra_cursors() {
            self.insert_lines_at_all_cursors(MultiLineInsertion::Duplicate, cx);
            return;
        }
        self.skip_folded_anchor_for_line_break();
        self.checkpoint();
        let (line, _) = self.buffer.offset_to_line_col(self.cursor.offset);
        let start = self.buffer.line_start(line);
        let end = self.buffer.line_end(line);
        let line_text = self.buffer.text()[start..end].to_string();
        let insertion = format!("\n{}", line_text);
        self.queue_non_markdown_line_display_transfer(&(end..end), &insertion);
        self.buffer.insert(end, &insertion);
        self.cursor.offset = end + 1 + line_text.len();
        self.cursor.anchor = self.cursor.offset;
        self.mark_active_tab_dirty();
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn insert_lines_at_all_cursors(&mut self, action: MultiLineInsertion, cx: &mut Context<Self>) {
        self.unfold_cursor_regions();
        let plans = multi_line_insertion_plans(
            &self.buffer,
            &self.cursor_states_with_primary_flag(),
            action,
        );
        self.checkpoint();
        self.apply_table_aware_cursor_plans(plans);
        self.mark_active_tab_dirty();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn copy_selected_lines(&mut self, upward: bool, cx: &mut Context<Self>) {
        if self.sidebar_edit.is_some() || !self.begin_content_edit(cx) {
            return;
        }
        if self.has_extra_cursors() || self.cursor.has_selection() {
            self.unfold_cursor_regions();
            let cursors = self.cursor_states_with_primary_flag();
            let groups = selected_logical_lines(&self.buffer, &cursors);
            let folded_anchors = self.snapshot_folded_anchor_offsets();
            let fold_edits = groups
                .iter()
                .map(|(cursor, _)| {
                    let range = cursor.selection_range();
                    let has_newline = range.end > range.start
                        && self.buffer.text().as_bytes().get(range.end - 1) == Some(&b'\n');
                    let end = range.end - usize::from(has_newline);
                    let insert_at = if upward { range.start } else { end };
                    (insert_at..insert_at, end - range.start + 1)
                })
                .collect::<Vec<_>>();
            self.checkpoint();
            let resolved = copy_lines_at_cursors(&mut self.buffer, &cursors, upward);
            self.remap_folded_anchor_offsets(folded_anchors, &fold_edits);
            self.restore_cursor_states(resolved);
            self.mark_active_tab_dirty();
            self.reparse();
            self.scroll_to_cursor();
            self.pause_blinking(cx);
            return;
        }
        self.checkpoint();
        let selection = self.cursor.selection_range();
        let start_line = self.buffer.offset_to_line_col(selection.start).0;
        let mut end_line = self.buffer.offset_to_line_col(selection.end).0;
        if selection.start != selection.end
            && selection.end == self.buffer.line_start(end_line)
            && end_line > start_line
        {
            end_line -= 1;
        }
        let start = self.buffer.line_start(start_line);
        let end = self.buffer.line_end(end_line);
        let text = self.buffer.text()[start..end].to_string();
        let inserted_len = text.len() + 1;
        if upward {
            let insertion = format!("{text}\n");
            self.queue_non_markdown_line_display_transfer(&(start..start), &insertion);
            self.buffer.insert(start, &insertion);
            self.cursor.offset = self.cursor.offset.min(self.buffer.len());
            self.cursor.anchor = self.cursor.anchor.min(self.buffer.len());
        } else {
            let insertion = format!("\n{text}");
            self.queue_non_markdown_line_display_transfer(&(end..end), &insertion);
            self.buffer.insert(end, &insertion);
            self.cursor.offset = (self.cursor.offset + inserted_len).min(self.buffer.len());
            self.cursor.anchor = (self.cursor.anchor + inserted_len).min(self.buffer.len());
        }
        self.extra_cursors.clear();
        self.mark_active_tab_dirty();
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_copy_line_up(
        &mut self,
        _: &CopyLineUp,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.copy_selected_lines(true, cx);
    }

    pub(crate) fn on_copy_line_down(
        &mut self,
        _: &CopyLineDown,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.copy_selected_lines(false, cx);
    }

    pub(crate) fn on_delete_line(
        &mut self,
        _: &DeleteLine,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar_edit.is_some() {
            return;
        }
        if !self.begin_content_edit(cx) {
            return;
        }
        if self.table_boundary_caret.is_some() {
            self.pause_blinking(cx);
            return;
        }
        self.checkpoint();
        if self.apply_semantic_delete(DeleteAction::Line) {
            self.mark_active_tab_dirty();
        }
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn move_lines_at_all_cursors(&mut self, upward: bool, cx: &mut Context<Self>) {
        self.unfold_cursor_regions();
        let cursors = self.cursor_states_with_primary_flag();
        let moves = multi_line_moves(&self.buffer, &cursors, upward);
        if moves.is_empty() {
            return;
        }
        let endpoints: Vec<_> = cursors
            .iter()
            .map(|(cursor, primary)| {
                (
                    moved_cursor_endpoints(&self.buffer, *cursor, &moves),
                    *primary,
                )
            })
            .collect();
        let folded_ends: Vec<_> = self
            .folded_section_ends
            .iter()
            .map(|(&anchor, &end)| {
                let (line, col) = self.buffer.offset_to_line_col(end);
                (
                    remap_moved_line(&moves, anchor),
                    remap_moved_line(&moves, line),
                    col,
                )
            })
            .collect();
        self.checkpoint();
        self.pending_line_display_transfer = None;
        for movement in moves.iter().rev() {
            self.buffer
                .replace(movement.range.clone(), &movement.replacement);
        }
        self.folded_sections = self
            .folded_sections
            .iter()
            .map(|line| remap_moved_line(&moves, *line))
            .collect();
        self.folded_image_previews = self
            .folded_image_previews
            .iter()
            .map(|line| remap_moved_line(&moves, *line))
            .collect();
        self.folded_section_ends = folded_ends
            .into_iter()
            .map(|(anchor, line, col)| (anchor, self.buffer.line_col_to_offset(line, col)))
            .collect();
        self.restore_cursor_states(
            endpoints
                .into_iter()
                .map(|((offset, anchor), primary)| {
                    (
                        Cursor {
                            offset: moved_endpoint_offset(&self.buffer, offset),
                            anchor: moved_endpoint_offset(&self.buffer, anchor),
                        },
                        primary,
                    )
                })
                .collect(),
        );
        self.table_boundary_caret = None;
        self.clear_marked_ranges();
        self.reset_cursor_visual_targeting();
        self.mark_active_tab_dirty();
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_move_line_up(
        &mut self,
        _: &MoveLineUp,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar_edit.is_some() {
            return;
        }
        if !self.begin_content_edit(cx) {
            return;
        }
        if self.has_extra_cursors() || self.cursor.has_selection() {
            self.move_lines_at_all_cursors(true, cx);
            return;
        }
        let (line, col) = self.buffer.offset_to_line_col(self.cursor.offset);
        if line == 0 {
            return;
        }
        let Some((range, replacement)) = adjacent_line_swap_edit(&self.buffer, line - 1) else {
            return;
        };
        self.checkpoint();
        self.queue_non_markdown_line_display_transfer(&range, &replacement);
        if let Some(transfer) = self.pending_line_display_transfer.as_mut() {
            transfer.fold_line_swap = Some(line - 1);
        }
        self.buffer.replace(range, &replacement);
        let new_offset = self
            .buffer
            .clamp_grapheme_offset(self.buffer.line_col_to_offset(line - 1, col));
        self.cursor.offset = new_offset;
        self.cursor.anchor = self.cursor.offset;
        self.mark_active_tab_dirty();
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    pub(crate) fn on_move_line_down(
        &mut self,
        _: &MoveLineDown,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.sidebar_edit.is_some() {
            return;
        }
        if !self.begin_content_edit(cx) {
            return;
        }
        if self.has_extra_cursors() || self.cursor.has_selection() {
            self.move_lines_at_all_cursors(false, cx);
            return;
        }
        let (line, col) = self.buffer.offset_to_line_col(self.cursor.offset);
        let line_count = self.buffer.line_count();
        if line >= line_count.saturating_sub(1) {
            return;
        }
        let Some((range, replacement)) = adjacent_line_swap_edit(&self.buffer, line) else {
            return;
        };
        self.checkpoint();
        self.queue_non_markdown_line_display_transfer(&range, &replacement);
        if let Some(transfer) = self.pending_line_display_transfer.as_mut() {
            transfer.fold_line_swap = Some(line);
        }
        self.buffer.replace(range, &replacement);
        let new_offset = self
            .buffer
            .clamp_grapheme_offset(self.buffer.line_col_to_offset(line + 1, col));
        self.cursor.offset = new_offset.min(self.buffer.len());
        self.cursor.anchor = self.cursor.offset;
        self.mark_active_tab_dirty();
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
    }

    fn format_markdown_tables_for_save(&self, text: &str) -> String {
        // `text` is the current buffer text (see `normalize_text_for_save`), so
        // `self.decorations` is already in sync with it. Reusing the cached
        // decorations avoids a full-document `parse_decorations` on every save,
        // which is a multi-hundred-millisecond UI freeze on large files.
        let decorations = &self.decorations;
        let mut lines = text
            .split('\n')
            .map(|line| line.to_string())
            .collect::<Vec<_>>();
        let mut table_ranges = Vec::new();

        for (line_index, decoration) in decorations.iter().enumerate() {
            let Some(table_info) = decoration.table_info.as_ref() else {
                continue;
            };
            if table_info.is_front_matter
                || !table_info.is_header
                || table_info.table_start_line != line_index
            {
                continue;
            }
            table_ranges.push((table_info.table_start_line, table_info.table_end_line));
        }

        table_ranges.sort_unstable();
        table_ranges.dedup();

        for (start_line, end_line) in table_ranges.into_iter().rev() {
            if start_line >= end_line || end_line > lines.len() {
                continue;
            }
            let formatted = crate::table_ops::serialize_table(
                &crate::table_ops::parse_table_block(&lines[start_line..end_line]),
            );
            lines.splice(start_line..end_line, formatted);
        }

        lines.join("\n")
    }

    fn normalize_text_for_save(&mut self) -> String {
        let original_text = self.buffer.text().to_string();
        let mut text = if self.active_tab_is_markdown() {
            self.format_markdown_tables_for_save(&original_text)
        } else {
            original_text.clone()
        };
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }

        if text != original_text {
            let old_len = self.buffer.len();
            self.buffer.replace(0..old_len, &text);
            self.cursor.offset = self.cursor.offset.min(self.buffer.len());
            self.cursor.anchor = self.cursor.anchor.min(self.buffer.len());
            for cursor in &mut self.extra_cursors {
                cursor.offset = cursor.offset.min(self.buffer.len());
                cursor.anchor = cursor.anchor.min(self.buffer.len());
            }
            self.reparse();
        }
        text
    }

    pub(super) fn persist_saved_tab_history(
        &mut self,
        tab_id: &str,
        path: &std::path::Path,
        snapshot: &EditorSnapshot,
        cx: &mut Context<Self>,
    ) {
        let Some(index) = self.tabs.iter().position(|tab| tab.id == tab_id) else {
            return;
        };
        let history = &mut self.tabs[index].history;
        let journal_path = history
            .journal_path
            .get_or_insert_with(|| history_store::default_journal_path(tab_id, Some(path)))
            .clone();
        history
            .source_identity
            .get_or_insert_with(|| history_store::default_source_identity(tab_id, Some(path)));
        let mut history = history.clone();
        let tab_id = tab_id.to_string();
        let path = path.to_path_buf();
        let snapshot = snapshot.clone();
        let save = file_ops::DocumentSave::new(journal_path.clone());
        let token = save.clone();
        cx.spawn(async move |this, cx| {
            let result = crate::platform::fs_task::run(
                "save undo history",
                journal_path.clone(),
                move || {
                    save.write_with(&[], |_, _| {
                        history_store::persist_tab_history(
                            &tab_id,
                            Some(&path),
                            &mut history,
                            &snapshot,
                        )
                        .map(|_| ())
                    })
                },
            )
            .await;
            token.observe_result(&result);
            if token.is_latest()
                && let Err(error) = result
            {
                this.update(cx, |editor, cx| {
                    editor.notify_error(
                        format!(
                            "{} ({}): {error}",
                            editor.t(crate::i18n::TextKey::FileOperationFailed),
                            journal_path.display()
                        ),
                        cx,
                    );
                })
                .ok();
            }
        })
        .detach();
    }

    pub(super) fn notify_document_save_failed(
        &mut self,
        tab_id: &str,
        path: &std::path::Path,
        error: &std::io::Error,
        cx: &mut Context<Self>,
    ) {
        if let Some(tab) = self.all_tabs_mut().find(|tab| tab.id == tab_id) {
            tab.is_dirty = true;
        }
        self.persist_recovery_for_tab(tab_id);
        self.notify_error(
            format!(
                "{} ({}): {error}",
                self.t(crate::i18n::TextKey::SaveFailed),
                path.display()
            ),
            cx,
        );
    }

    pub(super) fn finish_text_save(
        &mut self,
        tab_id: &str,
        path: &std::path::Path,
        saved_snapshot: EditorSnapshot,
        result: std::io::Result<()>,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = result {
            self.notify_document_save_failed(tab_id, path, &error, cx);
            return;
        }
        let Some(owner) = self.pane_id_for_tab(tab_id) else {
            return;
        };
        self.with_pane(owner, |editor| {
            let Some(index) = editor.tabs.iter().position(|tab| tab.id == tab_id) else {
                return;
            };
            if editor.tabs[index].path.as_deref() != Some(path) {
                return;
            }
            editor.tabs[index].history.saved_text = Some(saved_snapshot.text.clone());
            editor.refresh_line_diff();
            let current_text = if editor.active_tab_id.as_deref() == Some(tab_id) {
                editor.buffer.text()
            } else {
                &editor.tabs[index].snapshot.text
            };
            if current_text != saved_snapshot.text {
                editor.tabs[index].recovery.source_content_hash =
                    Some(file_ops::content_hash(saved_snapshot.text.as_bytes()));
                return;
            }
            editor.tabs[index].is_dirty = false;
            // A save acknowledges bytes, not an older cursor or scroll position.
            editor.tabs[index].set_snapshot_text(saved_snapshot.text.clone());
            editor.clear_temp_file_for_tab(tab_id);
            editor.persist_saved_tab_history(tab_id, path, &saved_snapshot, cx);
        });
        session_store::persist_session(self);
        if path == crate::editor::config_paths::settings_file_path() {
            self.apply_saved_config_file(cx);
        }
        cx.notify();
    }

    pub(crate) fn on_save(&mut self, _: &Save, _: &mut Window, cx: &mut Context<Self>) {
        if !self.active_tab_supports_persistent_save() {
            return;
        }
        let Some(index) = self.active_tab_index() else {
            return;
        };
        self.commit_history_transaction();
        let before_normalize = self.buffer.text().to_string();
        if self.active_tab_is_markdown() || !self.buffer.text().ends_with('\n') {
            self.checkpoint();
        }
        let text = self.normalize_text_for_save();
        if text == before_normalize {
            self.pending_history_transaction = None;
            self.buffer.finish_edit_recording();
        } else {
            self.commit_history_transaction();
        }
        let tab_id = self.tabs[index].id.clone();
        let saved_snapshot = self.current_snapshot();
        if let Some(path) = self.tabs[index].path.clone() {
            let save = file_ops::DocumentSave::new(path.clone());
            let token = save.clone();
            cx.spawn(async move |this, cx| {
                let write_text = text.clone();
                let result = crate::platform::fs_task::run_write("save", path.clone(), move || {
                    save.write(write_text.as_bytes())
                })
                .await;
                token.observe_result(&result);
                if !token.is_latest() || matches!(result, Ok(false)) {
                    return;
                }
                this.update(cx, |editor, cx| {
                    editor.finish_text_save(&tab_id, &path, saved_snapshot, result.map(|_| ()), cx);
                })
                .ok();
            })
            .detach();
        } else {
            // No file path — open native save dialog in background
            let default_name = format!("{}.md", self.tabs[index].title);
            #[cfg(not(target_os = "macos"))]
            let title = self.t(crate::i18n::TextKey::SaveFileDialog).to_string();
            cx.spawn(async move |this, cx| {
                #[cfg(target_os = "macos")]
                let maybe_path = {
                    let receiver = this.update(cx, |_, cx| {
                        cx.prompt_for_new_path(
                            &std::env::home_dir().unwrap_or_default(),
                            Some(&default_name),
                        )
                    });
                    match receiver {
                        Ok(receiver) => receiver.await.ok().and_then(Result::ok).flatten(),
                        Err(_) => None,
                    }
                };
                #[cfg(not(target_os = "macos"))]
                let maybe_path = cx
                    .background_executor()
                    .spawn(async move { helpers::save_file_blocking(&default_name, &title) })
                    .await;
                let Some(path) = maybe_path else {
                    return;
                };
                let save = this.update(cx, |_, _| file_ops::DocumentSave::new(path.clone()));
                let Ok(save) = save else { return };
                let token = save.clone();
                let result = crate::platform::fs_task::run_write("save", path.clone(), move || {
                    save.write(text.as_bytes())
                })
                .await;
                token.observe_result(&result);
                if !token.is_latest() || matches!(result, Ok(false)) {
                    return;
                }
                if let Err(error) = result {
                    this.update(cx, |editor, cx| {
                        editor.notify_document_save_failed(&tab_id, &path, &error, cx);
                    })
                    .ok();
                    return;
                }
                this.update(cx, |editor, cx| {
                    let Some(owner) = editor.pane_id_for_tab(&tab_id) else {
                        return;
                    };
                    editor.with_pane(owner, |editor| {
                        let Some(idx) = editor.tabs.iter().position(|tab| tab.id == tab_id) else {
                            return;
                        };
                        if editor.tabs[idx].path.is_some() {
                            return;
                        }
                        editor.tabs[idx].path = Some(path.clone());
                        editor.tabs[idx].title = EditorView::title_for_path(&path);
                        if editor.active_tab_id.as_deref() == Some(&tab_id) {
                            editor.current_file_path = Some(path.clone());
                        }
                    });
                    editor.finish_text_save(&tab_id, &path, saved_snapshot, Ok(()), cx);
                })
                .ok();
            })
            .detach();
        }
    }

    pub(crate) fn on_save_as(&mut self, _: &SaveAs, _: &mut Window, cx: &mut Context<Self>) {
        if !self.active_tab_supports_persistent_save() {
            return;
        }
        let Some(index) = self.active_tab_index() else {
            return;
        };
        let tab_id = self.tabs[index].id.clone();
        let original_path = self.tabs[index].path.clone();
        let contents = self.normalize_text_for_save();
        let default_name = self.tabs[index]
            .path
            .as_deref()
            .and_then(std::path::Path::file_name)
            .and_then(|name| name.to_str())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{}.md", self.tabs[index].title));
        let snapshot = self.current_snapshot();
        #[cfg(target_os = "macos")]
        let dialog_directory = self.tabs[index]
            .path
            .as_deref()
            .and_then(std::path::Path::parent)
            .map(std::path::Path::to_path_buf)
            .unwrap_or_else(|| std::env::home_dir().unwrap_or_default());
        #[cfg(not(target_os = "macos"))]
        let title = self.t(crate::i18n::TextKey::SaveFileDialog).to_string();

        cx.spawn(async move |this, cx| {
            #[cfg(target_os = "macos")]
            let maybe_path = match this.update(cx, |_, cx| {
                cx.prompt_for_new_path(&dialog_directory, Some(&default_name))
            }) {
                Ok(receiver) => receiver.await.ok().and_then(Result::ok).flatten(),
                Err(_) => None,
            };
            #[cfg(not(target_os = "macos"))]
            let maybe_path = cx
                .background_executor()
                .spawn(async move { helpers::save_file_blocking(&default_name, &title) })
                .await;
            let Some(path) = maybe_path else {
                return;
            };
            let contents_for_kind = contents.clone();
            let save = this.update(cx, |_, _| file_ops::DocumentSave::new(path.clone()));
            let Ok(save) = save else { return };
            let token = save.clone();
            let result = crate::platform::fs_task::run_write("save as", path.clone(), move || {
                save.write(contents.as_bytes())
            })
            .await;
            token.observe_result(&result);
            if !token.is_latest() || matches!(result, Ok(false)) {
                return;
            }
            if let Err(error) = result {
                this.update(cx, |editor, cx| {
                    editor.notify_document_save_failed(&tab_id, &path, &error, cx);
                })
                .ok();
                return;
            }

            this.update(cx, |editor, cx| {
                let Some(owner) = editor.pane_id_for_tab(&tab_id) else {
                    return;
                };
                editor.with_pane(owner, |editor| {
                    let Some(index) = editor.tabs.iter().position(|tab| tab.id == tab_id) else {
                        return;
                    };
                    if editor.tabs[index].path != original_path {
                        return;
                    }
                    let unchanged = if editor.active_tab_id.as_deref() == Some(&tab_id) {
                        editor.buffer.text() == contents_for_kind
                    } else {
                        editor.tabs[index].snapshot.text == contents_for_kind
                    };
                    editor.tabs[index].path = Some(path.clone());
                    editor.tabs[index].title = EditorView::title_for_path(&path);
                    editor.tabs[index].file_kind = Some(classify_file_kind_for_bytes(
                        &path,
                        contents_for_kind.as_bytes(),
                    ));
                    editor.tabs[index].history.saved_text = Some(contents_for_kind.clone());
                    editor.refresh_line_diff();
                    if unchanged {
                        editor.tabs[index].is_dirty = false;
                        editor.tabs[index].set_snapshot(snapshot.clone());
                        editor.clear_temp_file_for_tab(&tab_id);
                    }
                    editor.tabs[index].history.source_identity = Some(
                        history_store::default_source_identity(&tab_id, Some(path.as_path())),
                    );
                    editor.tabs[index].history.journal_path = Some(
                        history_store::default_journal_path(&tab_id, Some(path.as_path())),
                    );
                    if unchanged {
                        editor.persist_saved_tab_history(&tab_id, &path, &snapshot, cx);
                    }
                    if editor.active_tab_index() == Some(index) {
                        editor.current_file_path = Some(path);
                    }
                    editor.invalidate_sidebar_items_cache();
                    session_store::persist_session(editor);
                    cx.notify();
                });
            })
            .ok();
        })
        .detach();
    }

    pub(crate) fn on_new_tab(&mut self, _: &NewTab, _: &mut Window, cx: &mut Context<Self>) {
        self.create_scratch_tab(cx);
    }

    pub(crate) fn on_new_document(
        &mut self,
        _: &NewDocument,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.create_scratch_tab(cx);
    }

    fn sidebar_action_parent(&self) -> std::path::PathBuf {
        let target = self
            .focused_sidebar_path
            .clone()
            .unwrap_or_else(|| self.sidebar_root.clone());
        if self.sidebar_cached_path_is_dir(&target) == Some(true) {
            target
        } else {
            target
                .parent()
                .map(std::path::Path::to_path_buf)
                .unwrap_or_else(|| self.sidebar_root.clone())
        }
    }

    pub(crate) fn on_sidebar_create_file(
        &mut self,
        _: &SidebarCreateFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let parent = self.sidebar_action_parent();
        self.start_create_file(parent, window, cx);
    }

    pub(crate) fn on_sidebar_create_folder(
        &mut self,
        _: &SidebarCreateFolder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let parent = self.sidebar_action_parent();
        self.start_create_dir(parent, window, cx);
    }

    pub(crate) fn on_sidebar_rename(
        &mut self,
        _: &SidebarRename,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(path) = self
            .focused_sidebar_path
            .clone()
            .filter(|path| path != &self.sidebar_root)
        {
            self.start_rename(path, window, cx);
        }
    }

    pub(crate) fn on_sidebar_delete(
        &mut self,
        _: &SidebarDelete,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self
            .focused_sidebar_path
            .as_ref()
            .is_some_and(|path| path != &self.sidebar_root)
        {
            self.sidebar_delete_focused(cx);
        }
    }

    pub(crate) fn on_close_active_tab(
        &mut self,
        _: &CloseActiveTab,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tabs.is_empty() {
            window.remove_window();
        } else {
            self.close_active_tab(cx);
        }
    }

    pub(crate) fn on_reopen_closed_tab(
        &mut self,
        _: &ReopenClosedTab,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reopen_closed_tab(cx);
    }

    pub(crate) fn on_activate_previous_tab(
        &mut self,
        _: &ActivatePreviousTab,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate_previous_tab(cx);
    }

    pub(crate) fn on_activate_next_tab(
        &mut self,
        _: &ActivateNextTab,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.activate_next_tab(cx);
    }

    // ── Multi-cursor actions ────────────────────────────────────────

    pub(crate) fn on_add_cursor_above(
        &mut self,
        _: &AddCursorAbove,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.line_diff_selection.is_some() {
            return;
        }
        let cursors = self.all_cursors_snapshot();
        let mut added = false;
        for cursor in cursors {
            let (line, col) = self.buffer.offset_to_line_col(cursor.offset);
            if line == 0 {
                continue;
            }
            let new_offset = self.buffer.line_col_to_offset(line - 1, col);
            self.add_cursor_at(new_offset, new_offset);
            added = true;
        }
        if added {
            self.pause_blinking(cx);
        }
    }

    pub(crate) fn on_add_cursor_below(
        &mut self,
        _: &AddCursorBelow,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.line_diff_selection.is_some() {
            return;
        }
        let cursors = self.all_cursors_snapshot();
        let mut added = false;
        for cursor in cursors {
            let (line, col) = self.buffer.offset_to_line_col(cursor.offset);
            if line + 1 >= self.buffer.line_count() {
                continue;
            }
            let new_offset = self.buffer.line_col_to_offset(line + 1, col);
            self.add_cursor_at(new_offset, new_offset);
            added = true;
        }
        if added {
            self.pause_blinking(cx);
        }
    }

    pub(crate) fn on_select_next_occurrence(
        &mut self,
        _: &SelectNextOccurrence,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.line_diff_selection.is_some() {
            return;
        }
        let previous_cursor_offset = self.cursor.offset;
        let selected_word = !self.cursor.has_selection();
        let search_text = if self.cursor.has_selection() {
            let range = self.cursor.selection_range();
            self.buffer.text_for_range(range.clone()).to_string()
        } else {
            // Select the word under cursor first
            let offset = self.cursor.offset;
            self.select_word_at_offset(offset);
            if !self.cursor.has_selection() {
                return;
            }
            let range = self.cursor.selection_range();
            self.buffer.text_for_range(range.clone()).to_string()
        };
        if search_text.is_empty() {
            return;
        }

        let mut cursors = self.all_cursors_snapshot();
        let added = select_next_occurrence_for_cursors(&self.buffer, &search_text, &mut cursors);
        self.restore_cursor_states(
            cursors
                .into_iter()
                .enumerate()
                .map(|(index, cursor)| (cursor, index == 0))
                .collect(),
        );
        if let Some(range) = added.or_else(|| selected_word.then(|| self.cursor.selection_range()))
        {
            self.reset_cursor_visual_targeting();
            self.occurrence_reveal = Some(super::search::OccurrenceReveal {
                tab_id: self.active_tab_id.clone(),
                range: range.clone(),
            });
            self.prepare_source_range_for_navigation(&range);
            self.capture_typing_scroll_guard();
            if let Some(guard) = self.pending_typing_scroll_guard.as_mut() {
                guard.cursor_offset = previous_cursor_offset;
            }
            self.scroll_to_cursor();
        }
        self.pause_blinking(cx);
    }

    pub(crate) fn on_select_all_occurrences(
        &mut self,
        _: &SelectAllOccurrences,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.line_diff_selection.is_some() {
            return;
        }
        let search_text = if self.cursor.has_selection() {
            let range = self.cursor.selection_range();
            self.buffer.text_for_range(range.clone()).to_string()
        } else {
            let offset = self.cursor.offset;
            self.select_word_at_offset(offset);
            if !self.cursor.has_selection() {
                return;
            }
            let range = self.cursor.selection_range();
            self.buffer.text_for_range(range.clone()).to_string()
        };
        if search_text.is_empty() {
            return;
        }

        let matches: Vec<_> = self
            .buffer
            .text()
            .match_indices(&search_text)
            .filter_map(|(start, _)| {
                let end = start + search_text.len();
                (self.buffer.clamp_grapheme_offset(start) == start
                    && self.buffer.clamp_grapheme_offset(end) == end)
                    .then_some(start..end)
            })
            .collect();
        if matches.is_empty() {
            return;
        }
        let mut first = true;
        self.extra_cursors.clear();
        for range in matches {
            if first {
                self.cursor.anchor = range.start;
                self.cursor.offset = range.end;
                first = false;
            } else {
                self.extra_cursors.push(Cursor {
                    offset: range.end,
                    anchor: range.start,
                });
            }
        }
        self.pause_blinking(cx);
    }
}
