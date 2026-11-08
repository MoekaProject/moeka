use std::ops::Range;
use std::time::{Duration, Instant};

pub(super) fn preview_reveal_progress(
    started: Option<Instant>,
    duration: Duration,
    now: Instant,
) -> f32 {
    let Some(started) = started.filter(|_| !duration.is_zero()) else {
        return 1.0;
    };
    crate::ui::motion::ease_out(
        (now.saturating_duration_since(started).as_secs_f32() / duration.as_secs_f32())
            .clamp(0.0, 1.0),
    )
}

const MAX_LCS_CELLS: usize = 1_000_000;
const MIN_MODIFICATION_SIMILARITY: f32 = 0.25;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TextLineDiff {
    pub(crate) hunks: Vec<LineDiffHunk>,
    current_line_count: usize,
    unchanged_comparison: Option<usize>,
}

#[derive(Clone, Debug)]
pub(crate) struct LineDiffComparison {
    pub(crate) saved_text: String,
    current_text: String,
    pub(crate) hunk: LineDiffHunk,
    pub(crate) cursor_entered: bool,
}

impl LineDiffComparison {
    pub(crate) fn new(saved: &str, current: &str, hunk: LineDiffHunk) -> Self {
        Self {
            saved_text: saved.to_owned(),
            current_text: current.to_owned(),
            hunk,
            cursor_entered: false,
        }
    }

    pub(crate) fn update(&mut self, current: &str, diff: &mut TextLineDiff) -> usize {
        if self.current_text != current {
            let old = &self.current_text;
            let mut prefix = old
                .bytes()
                .zip(current.bytes())
                .take_while(|(a, b)| a == b)
                .count();
            while !old.is_char_boundary(prefix) || !current.is_char_boundary(prefix) {
                prefix -= 1;
            }
            let mut suffix = old[prefix..]
                .bytes()
                .rev()
                .zip(current[prefix..].bytes().rev())
                .take_while(|(a, b)| a == b)
                .count();
            while !old.is_char_boundary(old.len() - suffix)
                || !current.is_char_boundary(current.len() - suffix)
            {
                suffix -= 1;
            }
            let old_lines = line_slices(old);
            let start = line_start_offset(old, &old_lines, self.hunk.current_lines.start);
            let end = if self.hunk.current_lines.is_empty() {
                start
            } else {
                let last = self.hunk.current_lines.end - 1;
                line_start_offset(old, &old_lines, last) + old_lines[last].len()
            };
            let mapped = crate::buffer::range_after_edit(
                &(start..end),
                &(prefix..old.len() - suffix),
                current.len() - prefix - suffix,
            );
            let start_line = current[..mapped.start]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count();
            let end_line = current[..mapped.end]
                .bytes()
                .filter(|byte| *byte == b'\n')
                .count();
            self.hunk.current_lines = if current.is_empty()
                || (mapped.is_empty() && self.hunk.current_lines.is_empty())
            {
                start_line..start_line
            } else {
                start_line..end_line + 1
            };
            self.current_text = current.to_owned();
        }
        let current_lines = line_slices(current);
        let saved_lines = line_slices(&self.saved_text);
        let saved = saved_lines[self.hunk.saved_lines.clone()].join("\n");
        let current = current_lines[self.hunk.current_lines.clone()].join("\n");
        self.hunk.kind = if self.hunk.current_lines.is_empty() {
            LineDiffKind::Deleted
        } else {
            LineDiffKind::Modified
        };
        (self.hunk.saved_text_range, self.hunk.current_text_range) =
            changed_text_ranges(&saved, &current);
        diff.hunks.retain(|hunk| {
            !(hunk.saved_lines.start < self.hunk.saved_lines.end
                && self.hunk.saved_lines.start < hunk.saved_lines.end)
                && !(hunk.current_lines.start < self.hunk.current_lines.end
                    && self.hunk.current_lines.start < hunk.current_lines.end)
        });
        let index = diff.hunks.len();
        diff.hunks.push(self.hunk.clone());
        diff.current_line_count = current_lines.len();
        diff.unchanged_comparison = (saved == current).then_some(index);
        index
    }

    pub(crate) fn apply_recorded_edits(
        &mut self,
        edits: &[crate::buffer::BufferEdit],
        undo: bool,
        current: &str,
    ) {
        if self.current_text == current {
            return;
        }
        let lines = line_slices(&self.current_text);
        let start = line_start_offset(&self.current_text, &lines, self.hunk.current_lines.start);
        let end = if self.hunk.current_lines.is_empty() {
            start
        } else {
            let last = self.hunk.current_lines.end - 1;
            line_start_offset(&self.current_text, &lines, last) + lines[last].len()
        };
        let mut range = start..end;
        let mut text = self.current_text.clone();
        let mut apply = |edit: &crate::buffer::BufferEdit| {
            let (deleted, inserted) = if undo {
                (&edit.inserted_text, &edit.deleted_text)
            } else {
                (&edit.deleted_text, &edit.inserted_text)
            };
            let replaced = edit.start..edit.start + deleted.len();
            if text.get(replaced.clone()) != Some(deleted.as_str()) {
                return false;
            }
            range = crate::buffer::range_after_edit(&range, &replaced, inserted.len());
            text.replace_range(replaced, inserted);
            true
        };
        // A refresh may already have consumed part of an edit batch. In that case
        // update() remaps from its last accepted snapshot instead.
        let applied = if undo {
            edits.iter().rev().all(&mut apply)
        } else {
            edits.iter().all(&mut apply)
        };
        if !applied || text != current {
            return;
        }
        let start_line = text[..range.start]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count();
        let end_line = text[..range.end]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count();
        self.hunk.current_lines =
            if text.is_empty() || (range.is_empty() && self.hunk.current_lines.is_empty()) {
                start_line..start_line
            } else {
                start_line..end_line + 1
            };
        self.current_text = text;
    }

    pub(crate) fn contains_line(&self, line: usize) -> bool {
        if self.hunk.current_lines.is_empty() {
            line == self.hunk.current_lines.start
        } else {
            self.hunk.current_lines.contains(&line)
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct LineDiffSelection {
    pub(crate) hunk_index: usize,
    pub(crate) anchor: usize,
    pub(crate) head: usize,
    pub(crate) affinity: Option<super::WrapBoundaryAffinityKind>,
    pub(crate) goal_x: Option<f32>,
}

impl LineDiffSelection {
    pub(crate) fn range(self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }

    pub(crate) fn move_head(&mut self, text: &str, forward: bool, extend: bool) {
        use unicode_segmentation::UnicodeSegmentation;

        self.affinity = None;
        self.goal_x = None;
        if !extend && self.anchor != self.head {
            self.head = if forward {
                self.anchor.max(self.head)
            } else {
                self.anchor.min(self.head)
            };
            self.anchor = self.head;
            return;
        }

        let boundaries = text
            .grapheme_indices(true)
            .map(|(index, _)| index)
            .chain(std::iter::once(text.len()))
            .collect::<Vec<_>>();
        let head = self.head.min(text.len());
        self.head = if forward {
            boundaries
                .iter()
                .copied()
                .find(|boundary| *boundary > head)
                .unwrap_or(text.len())
        } else {
            boundaries
                .iter()
                .copied()
                .take_while(|boundary| *boundary < head)
                .last()
                .unwrap_or(0)
        };
        if !extend {
            self.anchor = self.head;
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct LineDiffHunk {
    pub(crate) saved_lines: Range<usize>,
    pub(crate) current_lines: Range<usize>,
    pub(crate) kind: LineDiffKind,
    pub(crate) expanded: bool,
    pub(crate) saved_text_range: Range<usize>,
    pub(crate) current_text_range: Range<usize>,
}

impl TextLineDiff {
    pub(crate) fn from_saved_text(saved: Option<&str>, current: &str) -> Self {
        saved.map_or_else(Self::default, |saved| Self::between(saved, current))
    }

    pub(crate) fn between(saved: &str, current: &str) -> Self {
        let saved_lines = line_slices(saved);
        let current_lines = line_slices(current);
        let mut prefix = 0;
        while prefix < saved_lines.len().min(current_lines.len())
            && lines_equal(saved_lines[prefix], current_lines[prefix])
        {
            prefix += 1;
        }

        let mut suffix = 0;
        while suffix < saved_lines.len().saturating_sub(prefix)
            && suffix < current_lines.len().saturating_sub(prefix)
            && lines_equal(
                saved_lines[saved_lines.len() - suffix - 1],
                current_lines[current_lines.len() - suffix - 1],
            )
        {
            suffix += 1;
        }

        let saved_end = saved_lines.len() - suffix;
        let current_end = current_lines.len() - suffix;
        let old_len = saved_end - prefix;
        let new_len = current_end - prefix;
        if old_len == 0 && new_len == 0 {
            return Self::default();
        }

        let cell_count = (old_len + 1).checked_mul(new_len + 1);
        let Some(cell_count) = cell_count.filter(|count| *count <= MAX_LCS_CELLS) else {
            let mut hunks = Vec::new();
            append_atomic_changes(
                &mut hunks,
                &saved_lines,
                &current_lines,
                prefix..saved_end,
                prefix..current_end,
                true,
            );
            Self::merge_deleted_hunks(&mut hunks, &saved_lines);
            return Self {
                hunks,
                current_line_count: current_lines.len(),
                unchanged_comparison: None,
            };
        };
        let width = new_len + 1;
        let mut lcs = vec![0u32; cell_count];
        for old in (0..old_len).rev() {
            for new in (0..new_len).rev() {
                let cell = old * width + new;
                lcs[cell] = if lines_equal(saved_lines[prefix + old], current_lines[prefix + new]) {
                    lcs[(old + 1) * width + new + 1] + 1
                } else {
                    lcs[(old + 1) * width + new].max(lcs[old * width + new + 1])
                };
            }
        }

        let (mut old, mut new) = (0, 0);
        let (mut hunk_old, mut hunk_new) = (None, None);
        let mut hunks = Vec::new();
        while old < old_len || new < new_len {
            let equal = old < old_len
                && new < new_len
                && lines_equal(saved_lines[prefix + old], current_lines[prefix + new]);
            if equal {
                if let (Some(old_start), Some(new_start)) = (hunk_old.take(), hunk_new.take()) {
                    append_atomic_changes(
                        &mut hunks,
                        &saved_lines,
                        &current_lines,
                        prefix + old_start..prefix + old,
                        prefix + new_start..prefix + new,
                        true,
                    );
                }
                old += 1;
                new += 1;
                continue;
            }

            hunk_old.get_or_insert(old);
            hunk_new.get_or_insert(new);
            let can_delete = old < old_len;
            let can_insert = new < new_len;
            if can_delete
                && (!can_insert || lcs[(old + 1) * width + new] >= lcs[old * width + new + 1])
            {
                old += 1;
            } else {
                new += 1;
            }
        }
        if let (Some(old_start), Some(new_start)) = (hunk_old, hunk_new) {
            append_atomic_changes(
                &mut hunks,
                &saved_lines,
                &current_lines,
                prefix + old_start..saved_end,
                prefix + new_start..current_end,
                true,
            );
        }
        Self::merge_deleted_hunks(&mut hunks, &saved_lines);
        Self {
            hunks,
            current_line_count: current_lines.len(),
            unchanged_comparison: None,
        }
    }

    pub(crate) fn markers(&self) -> Vec<LineDiffMarker> {
        let mut markers = Vec::new();
        for (hunk_index, hunk) in self.hunks.iter().enumerate() {
            if self.unchanged_comparison == Some(hunk_index) {
                continue;
            }
            markers.push(LineDiffMarker {
                line: hunk.current_lines.start,
                hunk_index,
                kind: hunk.kind,
                at_end: hunk.kind == LineDiffKind::Deleted
                    && hunk.current_lines.start == self.current_line_count,
                expanded: hunk.expanded,
                deleted_ordinal: 0,
                deleted_count: 0,
            });
        }
        let mut deleted_counts = std::collections::HashMap::new();
        for marker in &markers {
            if marker.kind == LineDiffKind::Deleted {
                *deleted_counts.entry(marker.line).or_insert(0usize) += 1;
            }
        }
        let mut deleted_ordinals = std::collections::HashMap::new();
        for marker in &mut markers {
            if marker.kind == LineDiffKind::Deleted {
                let ordinal = deleted_ordinals.entry(marker.line).or_insert(0usize);
                marker.deleted_ordinal = *ordinal;
                marker.deleted_count = deleted_counts[&marker.line];
                *ordinal += 1;
            }
        }
        markers
    }

    pub(super) fn stripe_groups(&self) -> Vec<usize> {
        let mut groups: Vec<_> = (0..self.hunks.len()).collect();
        let mut order: Vec<_> = (0..self.hunks.len()).collect();
        order.sort_unstable_by_key(|&index| {
            let hunk = &self.hunks[index];
            (hunk.current_lines.start, hunk.saved_lines.start)
        });
        for pair in order.windows(2) {
            let (previous, current) = (pair[0], pair[1]);
            let a = &self.hunks[previous];
            let b = &self.hunks[current];
            if self.unchanged_comparison == Some(previous)
                || self.unchanged_comparison == Some(current)
                || a.kind != b.kind
            {
                continue;
            }
            let adjacent = match a.kind {
                LineDiffKind::Added => {
                    a.current_lines.end == b.current_lines.start && a.saved_lines == b.saved_lines
                }
                LineDiffKind::Deleted => {
                    a.saved_lines.end == b.saved_lines.start && a.current_lines == b.current_lines
                }
                LineDiffKind::Modified => false,
            };
            if adjacent {
                groups[current] = groups[previous];
            }
        }
        groups
    }

    pub(crate) fn toggle_expanded(&mut self, hunk_index: usize) -> Option<bool> {
        if self.hunks.get(hunk_index)?.kind == LineDiffKind::Added {
            return None;
        }
        let expanded = !self.hunks.get(hunk_index)?.expanded;
        for (index, hunk) in self.hunks.iter_mut().enumerate() {
            hunk.expanded = expanded && index == hunk_index;
        }
        Some(expanded)
    }

    pub(crate) fn current_comparison_text(&self, current: &str, hunk_index: usize) -> String {
        let Some(hunk) = self.hunks.get(hunk_index) else {
            return String::new();
        };
        line_slices(current)[hunk.current_lines.clone()].join("\n")
    }

    #[cfg(test)]
    pub(crate) fn expanded_saved_line_count(&self, line: usize, trailing: bool) -> usize {
        self.preview_hunk_index(line, trailing)
            .map_or(0, |index| self.hunks[index].saved_lines.len())
    }

    pub(crate) fn preview_hunk_index(&self, line: usize, trailing: bool) -> Option<usize> {
        let matches_location = |hunk: &LineDiffHunk| {
            hunk.kind != LineDiffKind::Added
                && if trailing {
                    hunk.current_lines.start >= self.current_line_count
                } else {
                    hunk.current_lines.start == line
                        && hunk.current_lines.start < self.current_line_count
                }
        };
        self.hunks
            .iter()
            .position(|hunk| hunk.expanded && matches_location(hunk))
    }

    pub(crate) fn expanded_saved_preview_lines(
        &self,
        saved: &str,
        hunk_index: usize,
    ) -> Vec<LineDiffPreviewLine> {
        let Some(hunk) = self.hunks.get(hunk_index) else {
            return Vec::new();
        };
        if !hunk.expanded || hunk.kind == LineDiffKind::Added {
            return Vec::new();
        }
        let saved_lines = line_slices(saved);
        let Some(sources) = saved_lines.get(hunk.saved_lines.clone()) else {
            return Vec::new();
        };
        let decorations = crate::markdown::parse_decorations(saved);
        sources
            .iter()
            .enumerate()
            .map(|(offset, source)| {
                let source_line = hunk.saved_lines.start + offset;
                let text = source.strip_suffix('\r').unwrap_or(source).to_string();
                let mut decoration = decorations.get(source_line).cloned().unwrap_or_default();
                if decoration
                    .block_contexts
                    .contains(&crate::markdown::BlockContext::Blockquote)
                    && decoration.callout_kind.is_none()
                {
                    for previous in decorations[..source_line].iter().rev() {
                        if !previous
                            .block_contexts
                            .contains(&crate::markdown::BlockContext::Blockquote)
                        {
                            break;
                        }
                        if previous.callout_kind.is_some() {
                            decoration.callout_kind = previous.callout_kind;
                            break;
                        }
                    }
                }
                LineDiffPreviewLine {
                    hunk_index,
                    kind: LineDiffKind::Deleted,
                    changes: vec![InlineDiffChange {
                        saved_range: if hunk.kind == LineDiffKind::Deleted {
                            visible_text_range(&text)
                        } else {
                            hunk.saved_text_range.clone()
                        },
                        current_range: hunk.current_text_range.clone(),
                        kind: LineDiffKind::Deleted,
                    }],
                    text,
                    decoration,
                }
            })
            .collect()
    }

    fn merge_deleted_hunks(hunks: &mut Vec<LineDiffHunk>, saved_lines: &[&str]) {
        let mut merged: Vec<LineDiffHunk> = Vec::with_capacity(hunks.len());
        for hunk in hunks.drain(..) {
            if let Some(previous) = merged.last_mut()
                && previous.kind == LineDiffKind::Deleted
                && hunk.kind == LineDiffKind::Deleted
                && previous.saved_lines.end == hunk.saved_lines.start
                && previous.current_lines == hunk.current_lines
            {
                let previous_len = if previous.saved_lines.len() == 1 {
                    saved_lines[previous.saved_lines.start].len()
                } else {
                    previous.saved_text_range.end
                };
                previous.saved_lines.end = hunk.saved_lines.end;
                previous.saved_text_range =
                    0..previous_len + 1 + saved_lines[hunk.saved_lines.start].len();
            } else {
                merged.push(hunk);
            }
        }
        *hunks = merged;
    }

    pub(crate) fn current_byte_range(
        &self,
        current: &str,
        hunk_index: usize,
    ) -> Option<Range<usize>> {
        let hunk = self.hunks.get(hunk_index)?;
        let lines = line_slices(current);
        let mut start = line_start_offset(current, &lines, hunk.current_lines.start);
        let end = line_start_offset(current, &lines, hunk.current_lines.end);
        if hunk.kind == LineDiffKind::Added
            && hunk.current_lines.end == lines.len()
            && start > 0
            && let Some(separator) = current[..start].rfind('\n')
        {
            start = separator;
            if start > 0 && current.as_bytes()[start - 1] == b'\r' {
                start -= 1;
            }
        }
        Some(start..end)
    }

    pub(crate) fn replacement_for_hunk(
        &self,
        saved: &str,
        current: &str,
        hunk_index: usize,
    ) -> Option<String> {
        let hunk = self.hunks.get(hunk_index)?;
        let saved_lines = line_slices(saved);
        let current_lines = line_slices(current);
        let mut replacement = concatenate_lines(saved, hunk.saved_lines.clone());
        let current_start = line_start_offset(current, &current_lines, hunk.current_lines.start);
        if hunk.saved_lines.start > 0
            && current_start > 0
            && current.as_bytes().get(current_start - 1) != Some(&b'\n')
        {
            replacement.insert_str(
                0,
                line_ending_after(&saved_lines, hunk.saved_lines.start - 1),
            );
        }
        if !hunk.saved_lines.is_empty()
            && hunk.saved_lines.end < saved_lines.len()
            && !replacement.ends_with('\n')
        {
            let ending = line_ending_after(&saved_lines, hunk.saved_lines.end - 1);
            if ending == "\r\n" && replacement.ends_with('\r') {
                replacement.push('\n');
            } else {
                replacement.push_str(ending);
            }
        }
        Some(replacement)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct LineDiffPreviewLine {
    pub(crate) hunk_index: usize,
    pub(crate) text: String,
    pub(crate) kind: LineDiffKind,
    pub(crate) changes: Vec<InlineDiffChange>,
    pub(crate) decoration: crate::markdown::LineDecoration,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct InlineDiffChange {
    pub(crate) saved_range: Range<usize>,
    pub(crate) current_range: Range<usize>,
    pub(crate) kind: LineDiffKind,
}

pub(crate) fn inline_text_changes(saved: &str, current: &str) -> Vec<InlineDiffChange> {
    use unicode_segmentation::UnicodeSegmentation;

    let saved = saved.strip_suffix('\r').unwrap_or(saved);
    let current = current.strip_suffix('\r').unwrap_or(current);
    let old: Vec<_> = saved.grapheme_indices(true).collect();
    let new: Vec<_> = current.grapheme_indices(true).collect();
    let mut prefix = 0;
    while prefix < old.len().min(new.len()) && old[prefix].1 == new[prefix].1 {
        prefix += 1;
    }
    let mut old_end = old.len();
    let mut new_end = new.len();
    while old_end > prefix && new_end > prefix && old[old_end - 1].1 == new[new_end - 1].1 {
        old_end -= 1;
        new_end -= 1;
    }
    let old_len = old_end - prefix;
    let new_len = new_end - prefix;
    let mut matches = Vec::new();
    if (old_len + 1)
        .checked_mul(new_len + 1)
        .is_some_and(|cells| cells <= MAX_LCS_CELLS)
    {
        matches = token_matches(&old[prefix..old_end], &new[prefix..new_end])
            .into_iter()
            .map(|(i, j)| (prefix + i, prefix + j))
            .collect();
    } else {
        // Coarse word anchors bound long-line work without matching isolated
        // characters in a changed word against the unchanged text that follows.
        let old_start_byte = old.get(prefix).map_or(saved.len(), |item| item.0);
        let new_start_byte = new.get(prefix).map_or(current.len(), |item| item.0);
        let old_end_byte = old.get(old_end).map_or(saved.len(), |item| item.0);
        let new_end_byte = new.get(new_end).map_or(current.len(), |item| item.0);
        let old_words: Vec<_> = saved[old_start_byte..old_end_byte]
            .split_word_bound_indices()
            .collect();
        let new_words: Vec<_> = current[new_start_byte..new_end_byte]
            .split_word_bound_indices()
            .collect();
        let anchors = token_matches(&old_words, &new_words);
        for (old_word, new_word) in anchors {
            let old_byte = old_start_byte + old_words[old_word].0;
            let new_byte = new_start_byte + new_words[new_word].0;
            let old_index = old.partition_point(|item| item.0 < old_byte);
            let new_index = new.partition_point(|item| item.0 < new_byte);
            for offset in 0..old_words[old_word].1.graphemes(true).count() {
                matches.push((old_index + offset, new_index + offset));
            }
        }
    }
    let mut changes = Vec::new();
    let (mut i, mut j) = (prefix, prefix);
    for (old_match, new_match) in matches.into_iter().chain([(old_end, new_end)]) {
        let saved_range = old.get(i).map_or(saved.len(), |item| item.0)
            ..old.get(old_match).map_or(saved.len(), |item| item.0);
        let current_range = new.get(j).map_or(current.len(), |item| item.0)
            ..new.get(new_match).map_or(current.len(), |item| item.0);
        if !saved_range.is_empty() || !current_range.is_empty() {
            let kind = if saved_range.is_empty() {
                LineDiffKind::Added
            } else if current_range.is_empty() {
                LineDiffKind::Deleted
            } else {
                LineDiffKind::Modified
            };
            changes.push(InlineDiffChange {
                saved_range,
                current_range,
                kind,
            });
        }
        i = old_match + 1;
        j = new_match + 1;
    }
    changes
}

fn token_matches(old: &[(usize, &str)], new: &[(usize, &str)]) -> Vec<(usize, usize)> {
    let mut matches = Vec::new();
    if let Some(cells) = (old.len() + 1)
        .checked_mul(new.len() + 1)
        .filter(|cells| *cells <= MAX_LCS_CELLS)
    {
        let width = new.len() + 1;
        let mut lcs = vec![0u32; cells];
        for i in (0..old.len()).rev() {
            for j in (0..new.len()).rev() {
                lcs[i * width + j] = if old[i].1 == new[j].1 {
                    1 + lcs[(i + 1) * width + j + 1]
                } else {
                    lcs[(i + 1) * width + j].max(lcs[i * width + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < old.len() && j < new.len() {
            if old[i].1 == new[j].1 {
                matches.push((i, j));
                i += 1;
                j += 1;
            } else if lcs[(i + 1) * width + j] >= lcs[i * width + j + 1] {
                i += 1;
            } else {
                j += 1;
            }
        }
    } else {
        let mut positions = std::collections::HashMap::<&str, Vec<usize>>::new();
        for (j, (_, text)) in new.iter().enumerate() {
            positions.entry(text).or_default().push(j);
        }
        let mut next = 0;
        for (i, (_, text)) in old.iter().enumerate() {
            if let Some(indices) = positions.get(text)
                && let Some(&j) = indices.get(indices.partition_point(|j| *j < next))
            {
                matches.push((i, j));
                next = j + 1;
            }
        }
    }
    matches
}

fn append_atomic_changes(
    hunks: &mut Vec<LineDiffHunk>,
    saved_lines: &[&str],
    current_lines: &[&str],
    saved: Range<usize>,
    current: Range<usize>,
    pair_replacements: bool,
) {
    let old_len = saved.len();
    let new_len = current.len();
    if old_len == 0 || new_len == 0 || !pair_replacements {
        append_unpaired_changes(hunks, saved_lines, current_lines, saved, current);
        return;
    }

    let cell_count = (old_len + 1).checked_mul(new_len + 1);
    let Some(cell_count) = cell_count.filter(|count| *count <= MAX_LCS_CELLS) else {
        append_large_atomic_changes(hunks, saved_lines, current_lines, saved, current);
        return;
    };

    let width = new_len + 1;
    let mut costs = vec![0.0f32; cell_count];
    let mut steps = vec![0u8; cell_count];
    for old in (0..old_len).rev() {
        costs[old * width + new_len] = costs[(old + 1) * width + new_len] + 1.0;
        steps[old * width + new_len] = 1;
    }
    for new in (0..new_len).rev() {
        costs[old_len * width + new] = costs[old_len * width + new + 1] + 1.0;
        steps[old_len * width + new] = 2;
    }

    let force_single_replacement = old_len == 1 && new_len == 1;
    for old in (0..old_len).rev() {
        for new in (0..new_len).rev() {
            let cell = old * width + new;
            let delete_cost = 1.0 + costs[(old + 1) * width + new];
            let insert_cost = 1.0 + costs[old * width + new + 1];
            let mut best_cost = delete_cost;
            let mut best_step = 1;
            if insert_cost < best_cost {
                best_cost = insert_cost;
                best_step = 2;
            }

            let similarity = if force_single_replacement {
                1.0
            } else {
                line_similarity(
                    saved_lines[saved.start + old],
                    current_lines[current.start + new],
                )
            };
            if similarity >= MIN_MODIFICATION_SIMILARITY {
                let replace_cost = 2.0 * (1.0 - similarity) + costs[(old + 1) * width + new + 1];
                if replace_cost <= best_cost {
                    best_cost = replace_cost;
                    best_step = 3;
                }
            }
            costs[cell] = best_cost;
            steps[cell] = best_step;
        }
    }

    let (mut old, mut new) = (0, 0);
    while old < old_len || new < new_len {
        match steps[old * width + new] {
            1 => {
                let saved_line = saved.start + old;
                let boundary = current.start + new;
                hunks.push(LineDiffHunk {
                    saved_lines: saved_line..saved_line + 1,
                    current_lines: boundary..boundary,
                    kind: LineDiffKind::Deleted,
                    expanded: false,
                    saved_text_range: visible_text_range(saved_lines[saved_line]),
                    current_text_range: 0..0,
                });
                old += 1;
            }
            2 => {
                let current_line = current.start + new;
                let boundary = saved.start + old;
                hunks.push(LineDiffHunk {
                    saved_lines: boundary..boundary,
                    current_lines: current_line..current_line + 1,
                    kind: LineDiffKind::Added,
                    expanded: false,
                    saved_text_range: 0..0,
                    current_text_range: visible_text_range(current_lines[current_line]),
                });
                new += 1;
            }
            3 => {
                let saved_line = saved.start + old;
                let current_line = current.start + new;
                let (saved_text_range, current_text_range) =
                    changed_text_ranges(saved_lines[saved_line], current_lines[current_line]);
                hunks.push(LineDiffHunk {
                    saved_lines: saved_line..saved_line + 1,
                    current_lines: current_line..current_line + 1,
                    kind: LineDiffKind::Modified,
                    expanded: false,
                    saved_text_range,
                    current_text_range,
                });
                old += 1;
                new += 1;
            }
            _ => break,
        }
    }
}

fn append_unpaired_changes(
    hunks: &mut Vec<LineDiffHunk>,
    saved_lines: &[&str],
    current_lines: &[&str],
    saved: Range<usize>,
    current: Range<usize>,
) {
    for line in saved.clone() {
        hunks.push(LineDiffHunk {
            saved_lines: line..line + 1,
            current_lines: current.start..current.start,
            kind: LineDiffKind::Deleted,
            expanded: false,
            saved_text_range: visible_text_range(saved_lines[line]),
            current_text_range: 0..0,
        });
    }
    for line in current.clone() {
        hunks.push(LineDiffHunk {
            saved_lines: saved.end..saved.end,
            current_lines: line..line + 1,
            kind: LineDiffKind::Added,
            expanded: false,
            saved_text_range: 0..0,
            current_text_range: visible_text_range(current_lines[line]),
        });
    }
}

fn append_large_atomic_changes(
    hunks: &mut Vec<LineDiffHunk>,
    saved_lines: &[&str],
    current_lines: &[&str],
    saved: Range<usize>,
    current: Range<usize>,
) {
    let mut old = saved.start;
    let mut new = current.start;
    while old < saved.end || new < current.end {
        let matched = old < saved.end
            && new < current.end
            && (saved.len() == 1 && current.len() == 1
                || line_similarity(saved_lines[old], current_lines[new])
                    >= MIN_MODIFICATION_SIMILARITY);
        if matched {
            let (saved_text_range, current_text_range) =
                changed_text_ranges(saved_lines[old], current_lines[new]);
            hunks.push(LineDiffHunk {
                saved_lines: old..old + 1,
                current_lines: new..new + 1,
                kind: LineDiffKind::Modified,
                expanded: false,
                saved_text_range,
                current_text_range,
            });
            old += 1;
            new += 1;
        } else if old < saved.end {
            hunks.push(LineDiffHunk {
                saved_lines: old..old + 1,
                current_lines: new..new,
                kind: LineDiffKind::Deleted,
                expanded: false,
                saved_text_range: visible_text_range(saved_lines[old]),
                current_text_range: 0..0,
            });
            old += 1;
        } else if new < current.end {
            hunks.push(LineDiffHunk {
                saved_lines: old..old,
                current_lines: new..new + 1,
                kind: LineDiffKind::Added,
                expanded: false,
                saved_text_range: 0..0,
                current_text_range: visible_text_range(current_lines[new]),
            });
            new += 1;
        }
    }
}

fn line_similarity(saved: &str, current: &str) -> f32 {
    let saved = saved.strip_suffix('\r').unwrap_or(saved);
    let current = current.strip_suffix('\r').unwrap_or(current);
    let saved = saved.as_bytes();
    let current = current.as_bytes();
    let saved_len = saved.len();
    let current_len = current.len();
    let max_chars = saved_len.max(current_len);
    if max_chars == 0 {
        return 1.0;
    }
    let min_bytes = saved_len.min(current_len);
    let mut prefix = 0;
    for (saved_byte, current_byte) in saved.iter().zip(current) {
        if saved_byte != current_byte {
            break;
        }
        prefix += 1;
    }
    let mut suffix = 0;
    for (saved_byte, current_byte) in saved.iter().rev().zip(current.iter().rev()) {
        if suffix >= min_bytes - prefix || saved_byte != current_byte {
            break;
        }
        suffix += 1;
    }
    (prefix + suffix) as f32 / max_chars as f32
}

fn visible_text_range(line: &str) -> Range<usize> {
    0..line.strip_suffix('\r').unwrap_or(line).len()
}

fn changed_text_ranges(saved: &str, current: &str) -> (Range<usize>, Range<usize>) {
    let saved = saved.strip_suffix('\r').unwrap_or(saved);
    let current = current.strip_suffix('\r').unwrap_or(current);
    let saved_chars: Vec<_> = saved.char_indices().collect();
    let current_chars: Vec<_> = current.char_indices().collect();
    let mut prefix = 0;
    while prefix < saved_chars.len().min(current_chars.len())
        && saved_chars[prefix].1 == current_chars[prefix].1
    {
        prefix += 1;
    }
    let mut suffix = 0;
    while suffix < saved_chars.len().saturating_sub(prefix)
        && suffix < current_chars.len().saturating_sub(prefix)
        && saved_chars[saved_chars.len() - suffix - 1].1
            == current_chars[current_chars.len() - suffix - 1].1
    {
        suffix += 1;
    }
    let saved_start = saved_chars
        .get(prefix)
        .map_or(saved.len(), |(offset, _)| *offset);
    let current_start = current_chars
        .get(prefix)
        .map_or(current.len(), |(offset, _)| *offset);
    let saved_end = saved_chars
        .get(saved_chars.len().saturating_sub(suffix))
        .map_or(saved.len(), |(offset, _)| *offset);
    let current_end = current_chars
        .get(current_chars.len().saturating_sub(suffix))
        .map_or(current.len(), |(offset, _)| *offset);
    (saved_start..saved_end, current_start..current_end)
}

fn line_slices(text: &str) -> Vec<&str> {
    if text.is_empty() {
        Vec::new()
    } else {
        text.split('\n').collect()
    }
}

fn lines_equal(saved: &str, current: &str) -> bool {
    saved.strip_suffix('\r').unwrap_or(saved) == current.strip_suffix('\r').unwrap_or(current)
}

fn concatenate_lines(text: &str, range: Range<usize>) -> String {
    line_slices(text)
        .into_iter()
        .skip(range.start)
        .take(range.len())
        .collect::<Vec<_>>()
        .join("\n")
}

fn line_start_offset(text: &str, lines: &[&str], line: usize) -> usize {
    if line >= lines.len() {
        return text.len();
    }
    lines.iter().take(line).map(|line| line.len() + 1).sum()
}

fn line_ending_after(lines: &[&str], line: usize) -> &'static str {
    if lines.get(line).is_some_and(|line| line.ends_with('\r')) {
        "\r\n"
    } else {
        "\n"
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LineDiffKind {
    Added,
    Modified,
    Deleted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LineDiffMarker {
    pub(crate) line: usize,
    pub(crate) hunk_index: usize,
    pub(crate) kind: LineDiffKind,
    pub(crate) at_end: bool,
    pub(crate) expanded: bool,
    pub(crate) deleted_ordinal: usize,
    pub(crate) deleted_count: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stripe_groups_merge_only_adjacent_additions_and_deletions() {
        let added = TextLineDiff::between("start\nend", "start\nfirst\nsecond\nend");
        assert_eq!(added.stripe_groups(), vec![0, 0]);
        assert_eq!(added.hunks.len(), 2);
        let deleted = TextLineDiff::between("start\nfirst\nsecond\nend", "start\nend");
        assert_eq!(deleted.stripe_groups(), vec![0]);
        assert_eq!(deleted.hunks.len(), 1);
        let modified = TextLineDiff::between(
            "start\nold first\nold second\nend",
            "start\nnew first\nnew second\nend",
        );
        assert_eq!(modified.stripe_groups(), vec![0, 1]);
        let separated = TextLineDiff::between("a\nb\nc", "a\nfirst\nb\nsecond\nc");
        assert_eq!(separated.stripe_groups(), vec![0, 1]);
        let mut deleted = deleted;
        deleted.toggle_expanded(0);
        assert_eq!(deleted.stripe_groups(), vec![0]);
        assert_eq!(deleted.preview_hunk_index(1, false), Some(0));
        let reordered = TextLineDiff {
            hunks: added.hunks.into_iter().rev().collect(),
            ..added
        };
        assert_eq!(reordered.stripe_groups(), vec![1, 1]);
    }

    #[test]
    fn line_diff_comparison_remaps_unicode_split_join_and_empty_documents() {
        let saved = "before\nold 字\nlast";
        let original = "before\nnew 字\nlast";
        let diff = TextLineDiff::between(saved, original);
        let mut hunk = diff.hunks[0].clone();
        hunk.expanded = true;
        let mut comparison = LineDiffComparison::new(saved, original, hunk);
        for (current, expected_lines) in [
            ("prefix\nbefore\nnew 字\nlast", 2..3),
            ("prefix\nbefore\nnew\n 字\nlast", 2..4),
            ("prefix\nbefore\nnew 字\nlast", 2..3),
            ("prefix\nbefore\nold 字\nlast", 2..3),
        ] {
            let mut diff = TextLineDiff::between(saved, current);
            let index = comparison.update(current, &mut diff);
            assert_eq!(diff.hunks[index].current_lines, expected_lines);
            assert!(diff.hunks[index].expanded);
            assert_eq!(
                diff.expanded_saved_preview_lines(saved, index)[0].text,
                "old 字"
            );
            if current.contains("old 字") {
                assert!(
                    diff.markers()
                        .iter()
                        .all(|marker| marker.hunk_index != index)
                );
                assert!(!diff.markers().is_empty(), "unrelated insertion remains");
            }
        }
        let saved = "old";
        let diff = TextLineDiff::between(saved, "new");
        let mut hunk = diff.hunks[0].clone();
        hunk.expanded = true;
        let mut comparison = LineDiffComparison::new(saved, "new", hunk);
        for current in ["", "old", "new", ""] {
            let mut diff = TextLineDiff::between(saved, current);
            let index = comparison.update(current, &mut diff);
            assert!(diff.hunks[index].expanded);
            assert_eq!(diff.current_comparison_text(current, index), current);
            assert_eq!(diff.markers().is_empty(), current == saved);
        }
    }

    #[test]
    fn inline_diff_keeps_separate_replacements_and_unchanged_gaps() {
        let saved = "old middle red ending";
        let current = "new middle cyan ending!";
        let changes = inline_text_changes(saved, current);
        assert_eq!(changes.len(), 3);
        assert_eq!(
            (
                &saved[changes[0].saved_range.clone()],
                &current[changes[0].current_range.clone()],
                changes[0].kind
            ),
            ("old", "new", LineDiffKind::Modified)
        );
        assert_eq!(
            (
                &saved[changes[1].saved_range.clone()],
                &current[changes[1].current_range.clone()],
                changes[1].kind
            ),
            ("red", "cyan", LineDiffKind::Modified)
        );
        assert_eq!(
            (&current[changes[2].current_range.clone()], changes[2].kind),
            ("!", LineDiffKind::Added)
        );
    }

    #[test]
    fn inline_diff_distinguishes_insertions_deletions_and_grapheme_replacements() {
        let saved = "abc REMOVE keep old tail";
        let current = "abc keep new tail ADD";
        let changes = inline_text_changes(saved, current);
        assert!(
            changes
                .iter()
                .any(|change| change.kind == LineDiffKind::Deleted)
        );
        assert!(
            changes
                .iter()
                .any(|change| change.kind == LineDiffKind::Modified)
        );
        assert!(
            changes
                .iter()
                .any(|change| change.kind == LineDiffKind::Added)
        );
        let saved = "字 e\u{301} unchanged 👩‍💻 end";
        let current = "界 a\u{301} unchanged 👨‍💻 end";
        let changes = inline_text_changes(saved, current);
        assert_eq!(changes.len(), 3);
        for change in changes {
            assert!(!saved[change.saved_range].contains("unchanged"));
            assert!(!current[change.current_range].contains("unchanged"));
        }
        assert!(inline_text_changes(saved, saved).is_empty());
        assert_eq!(inline_text_changes("", "new")[0].kind, LineDiffKind::Added);
        assert_eq!(
            inline_text_changes("old", "")[0].kind,
            LineDiffKind::Deleted
        );
    }

    #[test]
    fn inline_diff_bounds_large_line_work_and_preserves_matching_anchors() {
        let saved = format!("old {} end", "unchanged ".repeat(400));
        let current = format!("new {} tail", "unchanged ".repeat(400));
        let changes = inline_text_changes(&saved, &current);
        assert!(changes.len() >= 2);
        assert!(changes.iter().all(
            |change| !saved[change.saved_range.clone()].contains("unchanged")
                && !current[change.current_range.clone()].contains("unchanged")
        ));
    }

    #[test]
    fn preview_reveal_starts_empty_and_finishes_at_full_height() {
        let started = Instant::now();
        let duration = Duration::from_millis(180);
        assert_eq!(
            preview_reveal_progress(Some(started), duration, started),
            0.0
        );
        let middle = preview_reveal_progress(Some(started), duration, started + duration / 2);
        assert!(middle > 0.0 && middle < 1.0);
        assert_eq!(
            preview_reveal_progress(Some(started), duration, started + duration),
            1.0
        );
        assert_eq!(
            preview_reveal_progress(Some(started), Duration::ZERO, started),
            1.0
        );
        assert_eq!(preview_reveal_progress(None, duration, started), 1.0);
    }

    #[test]
    fn added_hunks_never_expand_or_dismiss_an_existing_preview() {
        let mut diff = TextLineDiff::between("old line", "new line\nadded");
        let changed = diff
            .hunks
            .iter()
            .position(|hunk| hunk.kind == LineDiffKind::Modified)
            .unwrap();
        let added = diff
            .hunks
            .iter()
            .position(|hunk| hunk.kind == LineDiffKind::Added)
            .unwrap();
        assert_eq!(diff.toggle_expanded(changed), Some(true));
        assert_eq!(diff.toggle_expanded(added), None);
        assert!(diff.hunks[changed].expanded);
        assert!(!diff.hunks[added].expanded);
    }

    #[test]
    fn line_diff_selection_moves_by_grapheme_and_collapses_before_moving() {
        let text = "aé👩‍💻z";
        let mut selection = LineDiffSelection {
            hunk_index: 3,
            anchor: 1,
            head: 1,
            ..Default::default()
        };

        selection.move_head(text, true, false);
        assert_eq!(selection.head, "aé".len());
        selection.move_head(text, true, false);
        assert_eq!(selection.head, "aé👩‍💻".len());
        selection.anchor = 1;
        selection.head = text.len();
        selection.move_head(text, false, false);
        assert_eq!(selection.anchor, 1);
        assert_eq!(selection.head, 1);
    }

    #[test]
    fn computes_added_modified_and_deleted_hunks() {
        let diff = TextLineDiff::between("one\ntwo\nthree", "one\nchanged\nthree\nfour");

        assert_eq!(
            diff.hunks,
            vec![
                LineDiffHunk {
                    saved_lines: 1..2,
                    current_lines: 1..2,
                    kind: LineDiffKind::Modified,
                    expanded: false,
                    saved_text_range: 0..3,
                    current_text_range: 0..7,
                },
                LineDiffHunk {
                    saved_lines: 3..3,
                    current_lines: 3..4,
                    kind: LineDiffKind::Added,
                    expanded: false,
                    saved_text_range: 0..0,
                    current_text_range: 0..4,
                },
            ]
        );
        assert_eq!(
            diff.markers(),
            vec![
                LineDiffMarker {
                    line: 1,
                    hunk_index: 0,
                    kind: LineDiffKind::Modified,
                    at_end: false,
                    expanded: false,
                    deleted_ordinal: 0,
                    deleted_count: 0,
                },
                LineDiffMarker {
                    line: 3,
                    hunk_index: 1,
                    kind: LineDiffKind::Added,
                    at_end: false,
                    expanded: false,
                    deleted_ordinal: 0,
                    deleted_count: 0,
                },
            ]
        );
    }

    #[test]
    fn never_saved_text_has_no_diff_baseline() {
        assert_eq!(
            TextLineDiff::from_saved_text(None, "draft"),
            TextLineDiff::default()
        );
        assert_eq!(
            TextLineDiff::from_saved_text(Some(""), "draft").hunks.len(),
            1
        );
    }

    #[test]
    fn represents_deleted_lines_at_the_current_boundary() {
        let diff = TextLineDiff::between("one\ntwo\nthree", "one\nthree");

        assert_eq!(
            diff.markers(),
            vec![LineDiffMarker {
                line: 1,
                hunk_index: 0,
                kind: LineDiffKind::Deleted,
                at_end: false,
                expanded: false,
                deleted_ordinal: 0,
                deleted_count: 1,
            }]
        );
    }

    #[test]
    fn deleted_groups_preserve_crlf_trailing_restore_and_live_comparison_identity() {
        let saved = "before\r\nfirst\r\n\r\nlast";
        let current = "before";
        let mut diff = TextLineDiff::between(saved, current);
        assert_eq!(diff.hunks.len(), 1);
        assert_eq!(diff.hunks[0].saved_lines, 1..4);
        diff.toggle_expanded(0);
        assert_eq!(diff.expanded_saved_line_count(0, true), 3);
        assert_eq!(
            diff.expanded_saved_preview_lines(saved, 0)
                .iter()
                .map(|line| line.text.as_str())
                .collect::<Vec<_>>(),
            vec!["first", "", "last"]
        );
        assert_eq!(
            diff.replacement_for_hunk(saved, current, 0).unwrap(),
            "\r\nfirst\r\n\r\nlast"
        );
        let mut comparison = LineDiffComparison::new(saved, current, diff.hunks[0].clone());
        let changed = "before\r\nfirst\r\nedited\r\nlast";
        let mut diff = TextLineDiff::between(saved, changed);
        let index = comparison.update(changed, &mut diff);
        assert_eq!(diff.hunks.len(), 1);
        assert_eq!(diff.expanded_saved_preview_lines(saved, index).len(), 3);
    }

    #[test]
    fn adjacent_deleted_lines_share_one_complete_preview_and_restore() {
        let mut diff = TextLineDiff::between("one\nold one\nold two\nend", "one\nend");
        assert_eq!(diff.preview_hunk_index(1, false), None);
        assert!(diff.markers().iter().all(|marker| !marker.expanded));
        diff.toggle_expanded(0);
        let markers = diff.markers();

        assert_eq!(markers.len(), 1);
        assert_eq!(
            (markers[0].deleted_ordinal, markers[0].deleted_count),
            (0, 1)
        );
        assert!(markers[0].expanded);
        assert_eq!(diff.preview_hunk_index(1, false), Some(0));
        assert_eq!(diff.expanded_saved_line_count(1, false), 2);
        assert_eq!(
            diff.replacement_for_hunk("one\nold one\nold two\nend", "one\nend", 0)
                .unwrap(),
            "old one\nold two\n"
        );

        assert_eq!(
            diff.expanded_saved_preview_lines("one\nold one\nold two\nend", 0)
                .into_iter()
                .map(|line| (line.text, line.kind))
                .collect::<Vec<_>>(),
            vec![
                ("old one".into(), LineDiffKind::Deleted),
                ("old two".into(), LineDiffKind::Deleted)
            ]
        );
    }

    #[test]
    fn consecutive_modified_lines_keep_distinct_hunk_identity() {
        let diff = TextLineDiff::between(
            "one\nold first\nold second\nend",
            "one\nnew first\nnew second\nend",
        );

        assert_eq!(diff.hunks.len(), 2);
        assert_eq!(diff.hunks[0].kind, LineDiffKind::Modified);
        assert_eq!(diff.hunks[1].kind, LineDiffKind::Modified);
        assert_eq!(diff.hunks[0].current_lines, 1..2);
        assert_eq!(diff.hunks[1].current_lines, 2..3);
        assert_eq!(diff.preview_hunk_index(2, false), None);
        assert_eq!(diff.hunks[0].saved_text_range, 0..3);
        assert_eq!(diff.hunks[0].current_text_range, 0..3);
    }

    #[test]
    fn aligns_modified_lines_around_unrelated_insertions() {
        let diff = TextLineDiff::between(
            "start\nlet x = 1\nreturn value\nend",
            "start\nunrelated intro\nlet x = 2\nreturn value\nend",
        );

        assert_eq!(
            diff.hunks
                .iter()
                .map(|hunk| (
                    hunk.kind,
                    hunk.saved_lines.clone(),
                    hunk.current_lines.clone()
                ))
                .collect::<Vec<_>>(),
            vec![
                (LineDiffKind::Added, 1..1, 1..2),
                (LineDiffKind::Modified, 1..2, 2..3),
            ]
        );
    }

    #[test]
    fn does_not_pair_unrelated_multiline_changes_as_modifications() {
        let diff = TextLineDiff::between(
            "start\nold alpha\nobsolete beta\nend",
            "start\nnew gamma\ninserted delta\nend",
        );

        assert_eq!(
            diff.hunks.iter().map(|hunk| hunk.kind).collect::<Vec<_>>(),
            vec![
                LineDiffKind::Deleted,
                LineDiffKind::Added,
                LineDiffKind::Added,
            ]
        );
    }

    #[test]
    fn changed_text_ranges_preserve_utf8_boundaries() {
        assert_eq!(
            changed_text_ranges("标题旧内容", "标题新内容"),
            (6..9, 6..9)
        );
        assert_eq!(changed_text_ranges("prefix", "prefix added"), (6..6, 6..12));
        assert_eq!(
            changed_text_ranges("prefix removed", "prefix"),
            (6..14, 6..6)
        );
    }

    #[test]
    fn expanding_a_hunk_exposes_its_saved_lines() {
        let mut diff = TextLineDiff::between("one\nold\nthree", "one\nnew\nthree");

        assert_eq!(diff.expanded_saved_line_count(1, false), 0);
        assert_eq!(diff.toggle_expanded(0), Some(true));
        assert_eq!(diff.expanded_saved_line_count(1, false), 1);
        assert_eq!(
            diff.expanded_saved_preview_lines("one\nold\nthree", 0)
                .into_iter()
                .map(|line| (line.text, line.kind))
                .collect::<Vec<_>>(),
            vec![("old".into(), LineDiffKind::Deleted)]
        );
        assert_eq!(diff.toggle_expanded(0), Some(false));
        assert_eq!(diff.expanded_saved_line_count(1, false), 0);
    }

    #[test]
    fn saved_preview_uses_document_level_markdown_context() {
        let saved = "before\n```rust\nold_value();\n```\nafter";
        let mut diff = TextLineDiff::between(saved, "before\n```rust\nnew_value();\n```\nafter");
        diff.toggle_expanded(0);

        let preview = diff.expanded_saved_preview_lines(saved, 0);
        assert_eq!(preview.len(), 1);
        assert_eq!(
            preview[0].decoration.block_type,
            crate::markdown::BlockType::CodeBlock
        );
        assert_eq!(preview[0].decoration.code_language.as_deref(), Some("rust"));
    }

    #[test]
    fn saved_preview_keeps_list_and_task_markdown_context() {
        let saved = "- [ ] old task\n- old item";
        let mut diff = TextLineDiff::between(saved, "- [ ] new task\n- new item");
        let decorations = crate::markdown::parse_decorations(saved);
        assert_eq!(
            decorations[0].block_type,
            crate::markdown::BlockType::TaskList(false)
        );
        assert_eq!(
            decorations[1].block_type,
            crate::markdown::BlockType::UnorderedList
        );

        diff.toggle_expanded(0);
        assert_eq!(
            diff.expanded_saved_preview_lines(saved, 0)[0]
                .decoration
                .block_type,
            crate::markdown::BlockType::TaskList(false)
        );
        diff.toggle_expanded(1);
        assert_eq!(
            diff.expanded_saved_preview_lines(saved, 1)[0]
                .decoration
                .block_type,
            crate::markdown::BlockType::UnorderedList
        );
    }

    #[test]
    fn nested_task_preview_preserves_the_collapsed_parent_indent() {
        let saved = "- [ ] parent\n  - [ ] child";
        let mut diff = TextLineDiff::between(saved, "- [ ] parent\n  - [ ] changed");
        diff.toggle_expanded(0);
        let preview = diff.expanded_saved_preview_lines(saved, 0);
        assert_eq!(preview.len(), 1);
        assert_eq!(
            preview[0].decoration.block_type,
            crate::markdown::BlockType::TaskList(false)
        );

        let (display, _, _, prefix_collapsed) = crate::editor::helpers::build_display_text(
            &preview[0].text,
            &preview[0].decoration,
            &[],
            false,
            None,
            0,
            3,
            3,
            2,
            0,
            1.0,
            1.0,
            1.0,
        );

        assert!(prefix_collapsed);
        assert_eq!(display, "      child");
    }

    #[test]
    fn saved_preview_keeps_blockquote_and_callout_context() {
        let saved = "> [!NOTE]\n> old";
        let mut diff = TextLineDiff::between(saved, "> [!NOTE]\n> new");
        diff.toggle_expanded(0);

        let preview = diff.expanded_saved_preview_lines(saved, 0);
        assert_eq!(preview.len(), 1);
        assert!(
            preview[0]
                .decoration
                .block_contexts
                .contains(&crate::markdown::BlockContext::Blockquote)
        );
        assert_eq!(
            preview[0].decoration.callout_kind,
            Some(crate::markdown::CalloutKind::Note)
        );
    }

    #[test]
    fn extra_saved_rows_are_classified_as_deletions() {
        let mut diff = TextLineDiff::between("one\nold\nremoved\nend", "one\nnew\nend");
        diff.toggle_expanded(0);

        assert_eq!(
            diff.expanded_saved_preview_lines("one\nold\nremoved\nend", 0)
                .into_iter()
                .map(|line| (line.text, line.kind))
                .collect::<Vec<_>>(),
            vec![
                ("old".into(), LineDiffKind::Deleted),
                ("removed".into(), LineDiffKind::Deleted)
            ]
        );
        assert_eq!(diff.toggle_expanded(1), None);
    }

    #[test]
    fn expanded_trailing_deletions_render_after_the_last_current_line() {
        let mut diff = TextLineDiff::between("one\ntwo", "one");

        assert_eq!(diff.toggle_expanded(0), Some(true));
        assert_eq!(diff.expanded_saved_line_count(0, true), 1);
        assert_eq!(
            diff.expanded_saved_preview_lines("one\ntwo", 0)
                .into_iter()
                .map(|line| (line.text, line.kind))
                .collect::<Vec<_>>(),
            vec![("two".into(), LineDiffKind::Deleted)]
        );
    }

    #[test]
    fn deletion_at_end_uses_the_last_line_as_its_anchor() {
        let diff = TextLineDiff::between("one\ntwo", "one");

        assert_eq!(
            diff.markers(),
            vec![LineDiffMarker {
                line: 1,
                hunk_index: 0,
                kind: LineDiffKind::Deleted,
                at_end: true,
                expanded: false,
                deleted_ordinal: 0,
                deleted_count: 1,
            }]
        );
    }

    #[test]
    fn preserves_trailing_newlines_in_line_ranges() {
        let diff = TextLineDiff::between("one\n", "one");

        assert_eq!(diff.hunks.len(), 1);
        assert_eq!(diff.current_byte_range("one", 0), Some(3..3));
        assert_eq!(
            diff.replacement_for_hunk("one\n", "one", 0).as_deref(),
            Some("\n")
        );
    }

    #[test]
    fn replacing_each_hunk_restores_the_saved_text_exactly() {
        for (saved, current) in [
            ("one\nold\nthree", "one\nnew\nthree"),
            ("one\nthree", "one\ntwo\nthree"),
            ("one\ntwo\nthree", "one\nthree"),
            ("one\n", "one"),
            ("a\r\nb", "a\r\nchanged"),
            ("one\r\ntwo", "one"),
            ("one", "one\nnew"),
            ("a\r\nb", "a\r\nb\r\nnew"),
        ] {
            let diff = TextLineDiff::between(saved, current);
            let hunk_index = 0;
            let range = diff.current_byte_range(current, hunk_index).unwrap();
            let replacement = diff
                .replacement_for_hunk(saved, current, hunk_index)
                .unwrap();
            let mut restored = current.to_string();
            restored.replace_range(range, &replacement);
            assert_eq!(restored, saved);
        }
    }

    #[test]
    fn bounds_work_for_large_unrelated_changes() {
        let saved = (0..1500)
            .map(|line| format!("old {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let current = (0..1500)
            .map(|line| format!("new {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        let diff = TextLineDiff::between(&saved, &current);

        assert_eq!(diff.hunks.len(), 1500);
        assert!(diff.hunks.iter().all(|hunk| {
            hunk.saved_lines.len() == 1
                && hunk.current_lines.len() == 1
                && hunk.kind == LineDiffKind::Modified
        }));
    }
}
