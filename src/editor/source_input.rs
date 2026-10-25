//! Shared routing for Source typing and editing actions. Every caret is planned
//! against the same immutable source, then all edits are applied atomically.
use super::{
    EditorView,
    input_ime::SourceContextCache,
    source_edit::{self, SourceAction, SourceCursorEdit},
};
use crate::{buffer::Buffer, cursor::Cursor};

pub(super) fn normalize_targets(
    mut cursors: Vec<(Cursor, bool)>,
    buffer: &Buffer,
) -> Vec<(Cursor, bool)> {
    for (cursor, _) in &mut cursors {
        cursor.offset = buffer.clamp_grapheme_offset(cursor.offset);
        cursor.anchor = buffer.clamp_grapheme_offset(cursor.anchor);
    }
    merge_targets(cursors)
}

/// Native UTF-16 replacements can intentionally address a combining character
/// inside a grapheme. Only enforce UTF-8 boundaries, not caret navigation rules.
pub(super) fn normalize_native_targets(
    mut cursors: Vec<(Cursor, bool)>,
    buffer: &Buffer,
) -> Vec<(Cursor, bool)> {
    for (cursor, _) in &mut cursors {
        cursor.offset = buffer.clamp_offset(cursor.offset);
        cursor.anchor = buffer.clamp_offset(cursor.anchor);
    }
    merge_targets(cursors)
}

fn merge_targets(mut cursors: Vec<(Cursor, bool)>) -> Vec<(Cursor, bool)> {
    cursors.sort_by_key(|(cursor, _)| {
        let range = cursor.selection_range();
        (range.start, range.end)
    });
    let mut result: Vec<(Cursor, bool)> = Vec::with_capacity(cursors.len());
    for (cursor, primary) in cursors {
        if let Some((last, last_primary)) = result.last_mut() {
            let left = last.selection_range();
            let right = cursor.selection_range();
            if left.start == right.start
                || right.start < left.end
                || (right.is_empty() && right.start == left.end)
            {
                let reversed = if primary {
                    cursor.offset < cursor.anchor
                } else {
                    last.offset < last.anchor
                };
                let start = left.start.min(right.start);
                let end = left.end.max(right.end);
                *last = Cursor {
                    offset: if reversed { start } else { end },
                    anchor: if reversed { end } else { start },
                };
                *last_primary |= primary;
                continue;
            }
        }
        result.push((cursor, primary));
    }
    result
}

fn literal_plan(buffer: &Buffer, cursor: Cursor, action: SourceAction<'_>) -> SourceCursorEdit {
    let mut range = cursor.selection_range();
    let text = match action {
        SourceAction::Insert(text) => text,
        SourceAction::Enter => "\n",
        SourceAction::Backspace => {
            if range.is_empty() {
                range.start = buffer.prev_grapheme_boundary(cursor.offset);
            }
            ""
        }
        SourceAction::Delete => {
            if range.is_empty() {
                range.end = buffer.next_grapheme_boundary(cursor.offset);
            }
            ""
        }
    };
    let offset = range.start + text.len();
    SourceCursorEdit {
        range,
        text: text.into(),
        cursor: Cursor {
            offset,
            anchor: offset,
        },
    }
}

fn plans(
    buffer: &Buffer,
    cursors: Vec<(Cursor, bool)>,
    action: SourceAction<'_>,
    cache: &mut SourceContextCache,
) -> Vec<(SourceCursorEdit, bool)> {
    let cursors = normalize_targets(cursors, buffer);
    let assisted: Vec<_> = cursors
        .iter()
        .map(|(cursor, primary)| (source_edit::plan(buffer, *cursor, action, cache), *primary))
        .collect();
    if source_edit::validate(buffer, &assisted) {
        return assisted;
    }
    // Two independent caret actions can expand onto the same syntax. Prefer a
    // safe literal edit over dropping input or applying partially overlapping
    // transformations; overlapping selections were already merged above.
    cursors
        .into_iter()
        .map(|(cursor, primary)| (literal_plan(buffer, cursor, action), primary))
        .collect()
}

fn literal_insert_plans(
    buffer: &Buffer,
    cursors: Vec<(Cursor, bool)>,
    text: &str,
) -> Vec<(SourceCursorEdit, bool)> {
    normalize_targets(cursors, buffer)
        .into_iter()
        .map(|(cursor, primary)| {
            (
                literal_plan(buffer, cursor, SourceAction::Insert(text)),
                primary,
            )
        })
        .collect()
}

fn segmented_insert_plans(
    buffer: &Buffer,
    cursors: Vec<(Cursor, bool)>,
    segments: &[String],
) -> Vec<(SourceCursorEdit, bool)> {
    if cursors.len() != segments.len() {
        return Vec::new();
    }
    let normalized = normalize_targets(cursors.clone(), buffer);
    let mut originals: Vec<_> = cursors
        .into_iter()
        .enumerate()
        .map(|(index, (cursor, _))| {
            let cursor = Cursor {
                offset: buffer.clamp_grapheme_offset(cursor.offset),
                anchor: buffer.clamp_grapheme_offset(cursor.anchor),
            };
            (cursor.selection_range(), index)
        })
        .collect();
    originals.sort_by_key(|(range, _)| (range.start, range.end));
    let mut originals = originals.into_iter().peekable();
    normalized
        .into_iter()
        .map(|(cursor, primary)| {
            let union = cursor.selection_range();
            let mut indices = Vec::new();
            while originals
                .peek()
                .is_some_and(|(range, _)| union.start <= range.start && range.end <= union.end)
            {
                indices.push(originals.next().unwrap().1);
            }
            indices.sort_unstable();
            // Overlap removes the one-to-one mapping. Keep identical segments
            // once, or join just this cluster's segments in clipboard order.
            // Independent selections retain their own segment unchanged.
            let first = indices
                .first()
                .map(|index| segments[*index].as_str())
                .unwrap_or("");
            let text = if indices.iter().all(|index| segments[*index] == first) {
                first.to_string()
            } else {
                indices
                    .iter()
                    .map(|index| segments[*index].as_str())
                    .collect::<Vec<_>>()
                    .join("\n")
            };
            (
                literal_plan(buffer, cursor, SourceAction::Insert(&text)),
                primary,
            )
        })
        .collect()
}

impl EditorView {
    pub(super) fn apply_source_paste_segments(&mut self, segments: &[String]) {
        let plans = segmented_insert_plans(
            &self.buffer,
            self.cursor_states_with_primary_flag(),
            segments,
        );
        self.apply_source_plans(plans);
    }

    /// Literal callers own reparsing and undo boundaries; never assist paste,
    /// soft breaks or native replacement callbacks, even for one character.
    pub(super) fn apply_source_literal_insert(&mut self, text: &str) {
        let plans =
            literal_insert_plans(&self.buffer, self.cursor_states_with_primary_flag(), text);
        self.apply_source_plans(plans);
    }

    pub(super) fn apply_source_action(&mut self, action: SourceAction<'_>) -> bool {
        // Keep the parsed table range anchored through the edit so formatting
        // can be included before this action's single reparse.
        let alignment_range = self.markdown_table_source_range_at_cursor();
        if let Some(range) = &alignment_range {
            self.buffer.track_range(Some(range.clone()));
        }
        let cursors = self.cursor_states_with_primary_flag();
        let plans = plans(
            &self.buffer,
            cursors,
            action,
            self.source_syntax_context.get_mut(),
        );
        let changed = self.apply_source_plans(plans);
        if changed {
            if alignment_range.is_some() {
                let range = self.buffer.tracked_range_update().map(|(range, _)| range);
                self.buffer.track_range(None);
                if let Some(range) = range {
                    self.align_table_source_range(range, true);
                }
            }
            self.reparse_with_source_context(true);
        } else if alignment_range.is_some() {
            self.buffer.track_range(None);
        }
        changed
    }

    fn apply_source_plans(&mut self, plans: Vec<(SourceCursorEdit, bool)>) -> bool {
        if !source_edit::validate(&self.buffer, &plans) {
            return false;
        }
        let changes: Vec<_> = plans
            .iter()
            .filter(|(edit, _)| {
                self.buffer.text().get(edit.range.clone()) != Some(edit.text.as_str())
            })
            .map(|(edit, _)| edit)
            .collect();
        let changed = !changes.is_empty();
        let identical_change = changes.first().is_some_and(|edit| {
            changes
                .iter()
                .all(|other| other.range == edit.range && other.text == edit.text)
        });
        let folded_anchors =
            (changed && !identical_change).then(|| self.snapshot_folded_anchor_offsets());
        let fold_edits = changes
            .iter()
            .map(|edit| (edit.range.clone(), edit.text.len()))
            .collect::<Vec<_>>();
        if let Some(edit) = changes.first() {
            if identical_change {
                self.queue_non_markdown_line_display_transfer(&edit.range, &edit.text);
            } else {
                let start = changes
                    .iter()
                    .map(|edit| edit.range.start)
                    .min()
                    .unwrap_or(edit.range.start);
                let line = self.buffer.offset_to_line_col(start).0;
                self.source_syntax_context.get_mut().invalidate_from(line);
                self.pending_line_display_transfer = None;
            }
        }
        let Some(cursors) = source_edit::apply(&mut self.buffer, plans) else {
            return false;
        };
        if let Some(folded_anchors) = folded_anchors {
            self.remap_folded_anchor_offsets(folded_anchors, &fold_edits);
        }
        self.restore_cursor_states(cursors);
        self.clear_marked_ranges();
        self.reset_cursor_visual_targeting();
        changed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caret(offset: usize) -> Cursor {
        Cursor {
            offset,
            anchor: offset,
        }
    }

    fn apply(
        buffer: &mut Buffer,
        cursors: Vec<(Cursor, bool)>,
        action: SourceAction<'_>,
    ) -> Vec<(Cursor, bool)> {
        let mut cache = SourceContextCache::default();
        let plans = plans(buffer, cursors, action, &mut cache);
        assert!(source_edit::validate(buffer, &plans));
        if let Some(line) = plans
            .iter()
            .map(|(edit, _)| buffer.offset_to_line_col(edit.range.start).0)
            .min()
        {
            cache.invalidate_from(line);
        }
        source_edit::apply(buffer, plans).unwrap()
    }

    #[test]
    fn source_literal_inserts_merge_overlapping_targets_without_assistance() {
        for text in ["\n", "\t", "(", "[", "*", "$", "`", "**pasted**", ""] {
            let mut buffer = Buffer::from_text("abcdefgh");
            let plans = literal_insert_plans(
                &buffer,
                vec![
                    (
                        Cursor {
                            offset: 1,
                            anchor: 5,
                        },
                        true,
                    ),
                    (
                        Cursor {
                            offset: 6,
                            anchor: 3,
                        },
                        false,
                    ),
                ],
                text,
            );
            let cursors = source_edit::apply(&mut buffer, plans).unwrap();
            assert_eq!(buffer.text(), format!("a{text}gh"));
            assert_eq!(cursors, vec![(caret(1 + text.len()), true)]);
        }
    }

    #[test]
    fn source_literal_inserts_preserve_independent_unicode_targets_and_primary() {
        let mut buffer = Buffer::from_text("文🙂字");
        let plans = literal_insert_plans(
            &buffer,
            vec![
                (caret("文🙂".len()), true),
                (caret("文".len()), false),
                (caret("文🙂".len()), false),
            ],
            "(",
        );
        let cursors = source_edit::apply(&mut buffer, plans).unwrap();
        assert_eq!(buffer.text(), "文(🙂(字");
        assert_eq!(
            cursors,
            vec![(caret("文(".len()), false), (caret("文(🙂(".len()), true)]
        );
    }

    #[test]
    fn source_normalized_targets_merge_transitive_overlaps_but_not_adjacent_selections() {
        let buffer = Buffer::from_text("abcdefghij");
        let cursors = normalize_targets(
            vec![
                (
                    Cursor {
                        offset: 2,
                        anchor: 4,
                    },
                    true,
                ),
                (
                    Cursor {
                        offset: 6,
                        anchor: 3,
                    },
                    false,
                ),
                (
                    Cursor {
                        offset: 1,
                        anchor: 3,
                    },
                    false,
                ),
                (
                    Cursor {
                        offset: 8,
                        anchor: 6,
                    },
                    false,
                ),
                (caret(6), false),
            ],
            &buffer,
        );
        assert_eq!(
            cursors,
            vec![
                (
                    Cursor {
                        offset: 1,
                        anchor: 6
                    },
                    true
                ),
                (
                    Cursor {
                        offset: 8,
                        anchor: 6
                    },
                    false
                ),
            ]
        );
    }

    #[test]
    fn source_segmented_paste_merges_overlaps_without_deleting_unselected_text() {
        for (segments, expected) in [
            (["X", "X"], "aXgh"),
            (["X", "Y"], "aX\nYgh"),
            (["(", "("], "a(gh"),
        ] {
            let mut buffer = Buffer::from_text("abcdefgh");
            let segments: Vec<_> = segments.into_iter().map(String::from).collect();
            let plans = segmented_insert_plans(
                &buffer,
                vec![
                    (
                        Cursor {
                            offset: 1,
                            anchor: 5,
                        },
                        true,
                    ),
                    (
                        Cursor {
                            offset: 6,
                            anchor: 3,
                        },
                        false,
                    ),
                ],
                &segments,
            );
            let cursors = source_edit::apply(&mut buffer, plans).unwrap();
            assert_eq!(buffer.text(), expected);
            assert_eq!(cursors, vec![(caret(expected.len() - 2), true)]);
        }
    }

    #[test]
    fn source_segmented_paste_only_joins_segments_in_the_overlapping_cluster() {
        let mut buffer = Buffer::from_text("abcdefghij");
        let plans = segmented_insert_plans(
            &buffer,
            vec![
                (
                    Cursor {
                        offset: 1,
                        anchor: 5,
                    },
                    true,
                ),
                (
                    Cursor {
                        offset: 6,
                        anchor: 3,
                    },
                    false,
                ),
                (caret(9), false),
            ],
            &["X".into(), "Y".into(), "Z".into()],
        );
        let cursors = source_edit::apply(&mut buffer, plans).unwrap();
        assert_eq!(buffer.text(), "aX\nYghiZj");
        assert_eq!(cursors, vec![(caret(4), true), (caret(8), false)]);
    }

    #[test]
    fn source_segmented_paste_keeps_original_segment_to_cursor_assignment() {
        let mut buffer = Buffer::from_text("文🙂字");
        let plans = segmented_insert_plans(
            &buffer,
            vec![(caret("文🙂".len()), true), (caret("文".len()), false)],
            &["*".into(), "(".into()],
        );
        let cursors = source_edit::apply(&mut buffer, plans).unwrap();
        assert_eq!(buffer.text(), "文(🙂*字");
        assert_eq!(
            cursors,
            vec![(caret("文(".len()), false), (caret("文(🙂*".len()), true)]
        );
    }

    #[test]
    fn source_native_targets_do_not_expand_combining_character_ranges() {
        let buffer = Buffer::from_text("a\u{0301}bc");
        assert_eq!(
            normalize_native_targets(
                vec![
                    (
                        Cursor {
                            offset: 3,
                            anchor: 1
                        },
                        true
                    ),
                    (caret(5), false)
                ],
                &buffer,
            ),
            vec![
                (
                    Cursor {
                        offset: 3,
                        anchor: 1
                    },
                    true
                ),
                (caret(5), false)
            ],
        );
    }

    #[test]
    fn source_batch_typing_uses_each_carets_context() {
        let mut buffer = Buffer::from_text("text \n```rust\nvalue \n```\n");
        let ordinary = "text ".len();
        let code = "text \n```rust\nvalue ".len();
        let cursors = apply(
            &mut buffer,
            vec![(caret(code), true), (caret(ordinary), false)],
            SourceAction::Insert("*"),
        );
        assert_eq!(buffer.text(), "text **\n```rust\nvalue *\n```\n");
        assert_eq!(cursors[0], (caret(ordinary + 1), false));
        assert_eq!(cursors[1], (caret(code + 3), true));
    }

    #[test]
    fn source_batch_enter_continues_ordered_tasks_without_renumbering_neighbors() {
        let text = "1. [x] one\n-  [X] two\n99. unchanged";
        let mut buffer = Buffer::from_text(text);
        let first = "1. [x] one".len();
        let second = "1. [x] one\n-  [X] two".len();
        let cursors = apply(
            &mut buffer,
            vec![(caret(first), true), (caret(second), false)],
            SourceAction::Enter,
        );
        assert_eq!(
            buffer.text(),
            "1. [x] one\n2. [ ] \n-  [X] two\n-  [ ] \n99. unchanged"
        );
        assert_eq!(cursors[0], (caret("1. [x] one\n2. [ ] ".len()), true));
        assert_eq!(
            cursors[1].0.offset,
            "1. [x] one\n2. [ ] \n-  [X] two\n-  [ ] ".len()
        );
    }

    #[test]
    fn source_batch_merges_overlapping_selections_before_literal_wrapping() {
        let mut buffer = Buffer::from_text("abcdef");
        let cursors = apply(
            &mut buffer,
            vec![
                (
                    Cursor {
                        offset: 1,
                        anchor: 4,
                    },
                    true,
                ),
                (
                    Cursor {
                        offset: 6,
                        anchor: 3,
                    },
                    false,
                ),
                (caret(4), false),
            ],
            SourceAction::Insert("("),
        );
        assert_eq!(buffer.text(), "a(bcdef)");
        assert_eq!(cursors.len(), 1);
        assert!(cursors[0].1);
        assert_eq!(
            cursors[0].0,
            Cursor {
                offset: 2,
                anchor: 7
            }
        );
    }

    #[test]
    fn source_batch_delete_and_backspace_are_atomic_and_preserve_unicode() {
        for action in [SourceAction::Backspace, SourceAction::Delete] {
            let mut buffer = Buffer::from_text("( )\n文「」🙂");
            let cursors = apply(
                &mut buffer,
                vec![(caret(1), true), (caret("( )\n文「".len()), false)],
                action,
            );
            assert!(buffer.text().ends_with("文🙂"));
            assert_eq!(cursors.len(), 2);
            assert!(
                cursors
                    .iter()
                    .all(|(cursor, _)| buffer.text().is_char_boundary(cursor.offset))
            );
        }
    }

    #[test]
    fn source_batch_conflicting_structural_plans_fall_back_without_losing_input() {
        let mut buffer = Buffer::from_text("- ");
        let cursors = apply(
            &mut buffer,
            vec![(caret(1), true), (caret(2), false)],
            SourceAction::Enter,
        );
        assert_eq!(buffer.text(), "-\n \n");
        assert_eq!(cursors.len(), 2);
    }

    #[test]
    fn source_batch_is_one_recorded_transaction_and_pasted_strings_stay_literal() {
        let mut buffer = Buffer::from_text("文\n字");
        buffer.begin_edit_recording();
        apply(
            &mut buffer,
            vec![(caret(0), true), (caret("文\n".len()), false)],
            SourceAction::Insert("**pasted**"),
        );
        let edits = buffer.finish_edit_recording();
        assert_eq!(edits.len(), 2);
        assert_eq!(buffer.text(), "**pasted**文\n**pasted**字");
        assert!(buffer.apply_edits_inverse(&edits));
        assert_eq!(buffer.text(), "文\n字");
        assert!(buffer.apply_edits_forward(&edits));
        assert_eq!(buffer.text(), "**pasted**文\n**pasted**字");
    }
}
