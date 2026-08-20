//! In-place (working-tree vs HEAD) diff overlay.
//!
//! This computes a per-buffer-line diff of the active editable file against its
//! HEAD version and stores it on the tab as [`InlineDiffState`]. Unlike the
//! dedicated read-only diff tab (see `info_panel.rs`), the result is layered on
//! top of the live buffer so the editor can draw gutter indicators, inline
//! highlights, and expandable read-only "phantom" rows for removed lines while
//! the added/modified lines stay fully editable.

use std::ops::Range;
use std::time::Duration;

use gpui::{Context, Pixels, Point, point, px};
use similar::{Algorithm, DiffOp, TextDiff};

use super::info_panel::{build_inline_change_text_and_highlights, git_show_file_at_revision};
use super::*;

/// Accumulates the rows of one contiguous run of changed diff ops into a hunk.
struct HunkAcc {
    base_start: usize,
    base_end: usize,
    buffer_start: usize,
    buffer_end: usize,
    removed_lines: Vec<InlineDiffRemovedLine>,
    added: Vec<(usize, Vec<Range<usize>>)>,
}

impl HunkAcc {
    fn new(base_start: usize, buffer_start: usize) -> Self {
        Self {
            base_start,
            base_end: base_start,
            buffer_start,
            buffer_end: buffer_start,
            removed_lines: Vec::new(),
            added: Vec::new(),
        }
    }

    fn finish(self) -> InlineDiffHunk {
        let added_count = self.buffer_end.saturating_sub(self.buffer_start);
        let mut added_inline_spans = vec![Vec::new(); added_count];
        for (buffer_line, spans) in self.added {
            if let Some(slot) = buffer_line
                .checked_sub(self.buffer_start)
                .and_then(|index| added_inline_spans.get_mut(index))
            {
                *slot = spans;
            }
        }

        let has_added = self.buffer_end > self.buffer_start;
        let has_removed = !self.removed_lines.is_empty();
        let kind = match (has_added, has_removed) {
            (true, true) => InlineDiffHunkKind::Modified,
            (false, true) => InlineDiffHunkKind::Removed,
            // A hunk always has at least one changed side; treat the degenerate
            // case as an addition so it still renders an indicator.
            _ => InlineDiffHunkKind::Added,
        };

        InlineDiffHunk {
            kind,
            buffer_start: self.buffer_start,
            buffer_end: self.buffer_end,
            base_start: self.base_start,
            base_end: self.base_end,
            removed_lines: self.removed_lines,
            added_inline_spans,
            expanded: false,
        }
    }
}

/// Computes the inline diff state of `target_text` against `base_text`.
///
/// Runs purely on data, so it is safe to call from a background thread.
fn compute_inline_diff_state(
    base_text: &str,
    target_text: &str,
    file_kind: &FileKind,
) -> InlineDiffState {
    let base_decorations = super::parse_decorations_for_file_kind(base_text, file_kind);
    let hunks = compute_inline_diff_hunks(base_text, target_text, &base_decorations);
    InlineDiffState {
        base_text: base_text.to_string(),
        hunks,
    }
}

fn compute_inline_diff_hunks(
    base_text: &str,
    target_text: &str,
    base_decorations: &[crate::markdown::LineDecoration],
) -> Vec<InlineDiffHunk> {
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .diff_lines(base_text, target_text);

    let mut hunks: Vec<InlineDiffHunk> = Vec::new();
    let mut acc: Option<HunkAcc> = None;

    for op in diff.ops() {
        let (old_range, new_range, is_equal) = match *op {
            DiffOp::Equal {
                old_index,
                new_index,
                len,
            } => (old_index..old_index + len, new_index..new_index + len, true),
            DiffOp::Delete {
                old_index,
                old_len,
                new_index,
            } => (old_index..old_index + old_len, new_index..new_index, false),
            DiffOp::Insert {
                old_index,
                new_index,
                new_len,
            } => (old_index..old_index, new_index..new_index + new_len, false),
            DiffOp::Replace {
                old_index,
                old_len,
                new_index,
                new_len,
            } => (
                old_index..old_index + old_len,
                new_index..new_index + new_len,
                false,
            ),
        };

        if is_equal {
            if let Some(finished) = acc.take() {
                hunks.push(finished.finish());
            }
            continue;
        }

        let entry = acc.get_or_insert_with(|| HunkAcc::new(old_range.start, new_range.start));
        entry.base_end = old_range.end;
        entry.buffer_end = new_range.end;

        for change in diff.iter_inline_changes(op) {
            let (text, spans) = build_inline_change_text_and_highlights(&change);
            match change.tag() {
                similar::ChangeTag::Delete => {
                    let mut decoration = change
                        .old_index()
                        .and_then(|index| base_decorations.get(index).cloned())
                        .unwrap_or_default();
                    if decoration
                        .table_info
                        .as_ref()
                        .is_some_and(|table_info| !table_info.is_front_matter)
                    {
                        decoration.block_type = crate::markdown::BlockType::CodeBlock;
                        decoration.code_language = Some("table".to_string());
                    }
                    entry.removed_lines.push(InlineDiffRemovedLine {
                        text,
                        removed_spans: spans,
                        decoration,
                    });
                }
                similar::ChangeTag::Insert => {
                    if let Some(index) = change.new_index() {
                        entry.added.push((index, spans));
                    }
                }
                similar::ChangeTag::Equal => {}
            }
        }
    }

    if let Some(finished) = acc.take() {
        hunks.push(finished.finish());
    }

    hunks
}

impl EditorView {
    /// The inline diff state for the active editor tab, if any.
    pub(crate) fn active_inline_diff(&self) -> Option<&InlineDiffState> {
        self.active_tab_id
            .as_ref()
            .and_then(|id| self.tabs.iter().find(|tab| &tab.id == id))
            .and_then(|tab| tab.inline_diff.as_ref())
    }

    /// Whether `line_idx` is an added or modified buffer line of an **expanded**
    /// inline-diff hunk (used to paint the green changed-line background). While
    /// a hunk is collapsed only its gutter bar shows, not the line background.
    pub(crate) fn inline_diff_line_is_changed(&self, line_idx: usize) -> bool {
        let Some(diff) = self.active_inline_diff() else {
            return false;
        };
        diff.hunks.iter().any(|hunk| {
            hunk.expanded
                && matches!(
                    hunk.kind,
                    InlineDiffHunkKind::Added | InlineDiffHunkKind::Modified
                )
                && line_idx >= hunk.buffer_start
                && line_idx < hunk.buffer_end
        })
    }

    /// The inline added/changed character spans for a buffer line, if it belongs
    /// to an **expanded** added or modified hunk.
    pub(crate) fn inline_diff_added_spans_for_line(
        &self,
        line_idx: usize,
    ) -> Option<Vec<Range<usize>>> {
        let diff = self.active_inline_diff()?;
        for hunk in &diff.hunks {
            if hunk.expanded
                && matches!(
                    hunk.kind,
                    InlineDiffHunkKind::Added | InlineDiffHunkKind::Modified
                )
                && line_idx >= hunk.buffer_start
                && line_idx < hunk.buffer_end
            {
                return hunk
                    .added_inline_spans
                    .get(line_idx - hunk.buffer_start)
                    .cloned();
            }
        }
        None
    }

    /// Number of read-only phantom (removed) rows that render directly above
    /// `line` because an expanded hunk's removed content is anchored there.
    pub(crate) fn inline_diff_phantom_rows_above(&self, line: usize) -> usize {
        let Some(diff) = self.active_inline_diff() else {
            return 0;
        };
        let line_count = self.buffer.line_count();
        diff.hunks
            .iter()
            .filter(|hunk| {
                hunk.expanded
                    && !hunk.removed_lines.is_empty()
                    && hunk.buffer_start == line
                    && hunk.buffer_start < line_count
            })
            .map(|hunk| hunk.removed_lines.len())
            .sum()
    }

    /// Number of phantom rows rendered below `line` for a trailing deletion
    /// (removed content past the end of the buffer).
    pub(crate) fn inline_diff_phantom_rows_below(&self, line: usize) -> usize {
        let Some(diff) = self.active_inline_diff() else {
            return 0;
        };
        let line_count = self.buffer.line_count();
        if line_count == 0 || line != line_count - 1 {
            return 0;
        }
        diff.hunks
            .iter()
            .filter(|hunk| {
                hunk.expanded && !hunk.removed_lines.is_empty() && hunk.buffer_start >= line_count
            })
            .map(|hunk| hunk.removed_lines.len())
            .sum()
    }

    /// Whether `line` anchors any phantom rows (above or below).
    pub(crate) fn inline_diff_is_phantom_anchor(&self, line: usize) -> bool {
        self.inline_diff_phantom_rows_above(line) > 0
            || self.inline_diff_phantom_rows_below(line) > 0
    }

    /// Vertical space reserved above `line` for its expanded removed rows. Uses
    /// the measured height once shaped, falling back to a per-block estimate
    /// (one visual line each) before the first measurement. Returns 0 when the
    /// line no longer anchors phantom rows, so any stale cache entry left behind
    /// after a recompute is harmless.
    pub(crate) fn inline_diff_phantom_reserved_above(&self, line: usize) -> f32 {
        let estimate = self.estimate_inline_diff_phantom_height(line, false);
        if estimate == 0.0 {
            return 0.0;
        }
        self.inline_diff_phantom_height_above
            .get(&line)
            .copied()
            .unwrap_or(estimate)
    }

    /// Vertical space reserved below `line` for a trailing deletion's removed
    /// rows (see [`inline_diff_phantom_reserved_above`]).
    pub(crate) fn inline_diff_phantom_reserved_below(&self, line: usize) -> f32 {
        let estimate = self.estimate_inline_diff_phantom_height(line, true);
        if estimate == 0.0 {
            return 0.0;
        }
        self.inline_diff_phantom_height_below
            .get(&line)
            .copied()
            .unwrap_or(estimate)
    }

    fn estimate_inline_diff_phantom_height(&self, line: usize, trailing: bool) -> f32 {
        let Some(diff) = self.active_inline_diff() else {
            return 0.0;
        };
        let line_count = self.buffer.line_count();
        if trailing && (line_count == 0 || line != line_count - 1) {
            return 0.0;
        }
        let mut height = 0.0;
        for hunk in &diff.hunks {
            if !hunk.expanded || hunk.removed_lines.is_empty() {
                continue;
            }
            let matches = if trailing {
                hunk.buffer_start >= line_count
            } else {
                hunk.buffer_start == line && hunk.buffer_start < line_count
            };
            if matches {
                for removed in &hunk.removed_lines {
                    height += self.line_height_for_block(&removed.decoration.block_type);
                }
            }
        }
        height
    }

    /// The removed lines to render above `line`, in display order. Each entry is
    /// `(hunk_base_start, row_in_hunk, removed_line)` so the renderer can map
    /// rows back to their hunk for read-only selection.
    pub(crate) fn inline_diff_phantom_removed_above(
        &self,
        line: usize,
    ) -> Vec<(usize, usize, InlineDiffRemovedLine)> {
        self.collect_inline_diff_phantom_rows(line, false)
    }

    /// The removed lines to render below `line` (trailing deletion).
    pub(crate) fn inline_diff_phantom_removed_below(
        &self,
        line: usize,
    ) -> Vec<(usize, usize, InlineDiffRemovedLine)> {
        self.collect_inline_diff_phantom_rows(line, true)
    }

    fn collect_inline_diff_phantom_rows(
        &self,
        line: usize,
        trailing: bool,
    ) -> Vec<(usize, usize, InlineDiffRemovedLine)> {
        let Some(diff) = self.active_inline_diff() else {
            return Vec::new();
        };
        let line_count = self.buffer.line_count();
        if trailing && (line_count == 0 || line != line_count - 1) {
            return Vec::new();
        }
        let mut rows = Vec::new();
        for hunk in &diff.hunks {
            if !hunk.expanded || hunk.removed_lines.is_empty() {
                continue;
            }
            let matches = if trailing {
                hunk.buffer_start >= line_count
            } else {
                hunk.buffer_start == line && hunk.buffer_start < line_count
            };
            if matches {
                for (row, removed) in hunk.removed_lines.iter().enumerate() {
                    rows.push((hunk.base_start, row, removed.clone()));
                }
            }
        }
        rows
    }

    /// The text of the current read-only phantom (removed-row) selection, if
    /// any. Character-level within a row; rows are joined by newlines.
    pub(crate) fn phantom_selection_text(&self) -> Option<String> {
        let selection = self.phantom_selection?;
        if selection.is_collapsed() {
            return None;
        }
        let diff = self.active_inline_diff()?;
        let hunk = diff
            .hunks
            .iter()
            .find(|hunk| hunk.base_start == selection.hunk_base_start)?;
        if hunk.removed_lines.is_empty() {
            return None;
        }
        let (start_row, start_offset, end_row, end_offset) = selection.ordered();
        let last_row = hunk.removed_lines.len() - 1;
        let start_row = start_row.min(last_row);
        let end_row = end_row.min(last_row);
        let clamp = |row: usize, offset: usize| -> usize {
            let text = &hunk.removed_lines[row].text;
            let mut off = offset.min(text.len());
            while off > 0 && !text.is_char_boundary(off) {
                off -= 1;
            }
            off
        };
        let start_offset = clamp(start_row, start_offset);
        let end_offset = clamp(end_row, end_offset);
        if start_row == end_row {
            return Some(hunk.removed_lines[start_row].text[start_offset..end_offset].to_string());
        }
        let mut parts: Vec<String> = Vec::new();
        parts.push(hunk.removed_lines[start_row].text[start_offset..].to_string());
        for row in start_row + 1..end_row {
            parts.push(hunk.removed_lines[row].text.clone());
        }
        parts.push(hunk.removed_lines[end_row].text[..end_offset].to_string());
        Some(parts.join("\n"))
    }

    /// Clears any read-only phantom-row selection. Returns whether it changed.
    pub(crate) fn clear_phantom_selection(&mut self) -> bool {
        self.phantom_selecting = false;
        if self.phantom_selection.is_some() {
            self.phantom_selection = None;
            true
        } else {
            false
        }
    }

    /// Maps a screen position to a phantom removed-row offset
    /// `(hunk_base_start, row, byte_offset)` when it falls within a rendered row.
    pub(crate) fn phantom_offset_at_position(
        &self,
        position: Point<Pixels>,
    ) -> Option<(usize, usize, usize)> {
        let hit = self
            .phantom_row_hits
            .iter()
            .find(|hit| hit.bounds.contains(&position))?;
        let local_x = (f32::from(position.x) - hit.content_x).max(0.0);
        let local_y = (f32::from(position.y) - hit.row_top).max(0.0);
        let index = match hit
            .wrapped
            .closest_index_for_position(point(px(local_x), px(local_y)), px(hit.line_height))
        {
            Ok(index) | Err(index) => index,
        };
        // `index` is a display byte offset; map it back to the raw-text offset so
        // a click on a folded row lands on the right caret position.
        let buffer_offset = if index < hit.display_to_buffer.len() {
            hit.display_to_buffer[index]
        } else {
            hit.text_len
        }
        .min(hit.text_len);
        Some((hit.hunk_base_start, hit.row, buffer_offset))
    }

    /// Toggles whether a hunk is expanded to show its removed rows in place.
    pub(crate) fn toggle_inline_diff_hunk_expanded(
        &mut self,
        hunk_index: usize,
        cx: &mut Context<Self>,
    ) {
        let Some(tab_index) = self.active_tab_index() else {
            return;
        };
        let Some(diff) = self.tabs[tab_index].inline_diff.as_mut() else {
            return;
        };
        let Some(hunk) = diff.hunks.get_mut(hunk_index) else {
            return;
        };
        hunk.expanded = !hunk.expanded;
        self.phantom_selection = None;
        self.phantom_selecting = false;
        // Keep the measured phantom-height cache: re-expanding a hunk whose
        // removed lines are unchanged then reuses the correct height instantly
        // (the `reserved_*` getters ignore stale entries for collapsed hunks).
        self.clear_line_display_info();
        self.invalidate_line_geometry_cache();
        self.layout_dirty = true;
        cx.notify();
    }

    /// Reverts a single hunk to its HEAD version by replacing its buffer lines
    /// with the corresponding base lines. The edit goes through the normal
    /// checkpoint/reparse path so it is undoable and refreshes the diff.
    pub(crate) fn revert_inline_diff_hunk(&mut self, hunk_index: usize, cx: &mut Context<Self>) {
        if self.active_tab_is_read_only() {
            return;
        }
        let Some(tab_index) = self.active_tab_index() else {
            return;
        };
        let (replacement, buffer_start, buffer_end) = {
            let Some(diff) = self.tabs[tab_index].inline_diff.as_ref() else {
                return;
            };
            let Some(hunk) = diff.hunks.get(hunk_index) else {
                return;
            };
            let base_lines: Vec<&str> = diff.base_text.split('\n').collect();
            let base_start = hunk.base_start.min(base_lines.len());
            let base_end = hunk.base_end.max(base_start).min(base_lines.len());
            let replacement: Vec<String> = base_lines[base_start..base_end]
                .iter()
                .map(|line| line.to_string())
                .collect();
            (replacement, hunk.buffer_start, hunk.buffer_end)
        };

        let mut lines: Vec<String> = self
            .buffer
            .text()
            .split('\n')
            .map(|line| line.to_string())
            .collect();
        let buffer_start = buffer_start.min(lines.len());
        let buffer_end = buffer_end.max(buffer_start).min(lines.len());
        lines.splice(buffer_start..buffer_end, replacement);
        let new_text = lines.join("\n");
        let new_cursor = lines[..buffer_start]
            .iter()
            .map(|line| line.len() + 1)
            .sum::<usize>()
            .min(new_text.len());

        self.checkpoint();
        let old_len = self.buffer.len();
        self.buffer.replace(0..old_len, &new_text);
        self.cursor.offset = new_cursor;
        self.cursor.anchor = new_cursor;
        self.extra_cursors.clear();
        self.reset_cursor_visual_targeting();
        self.mark_active_tab_dirty();
        self.reparse();
        self.pause_blinking(cx);
    }

    /// Requests an inline-diff recompute on the next render. Set `refetch_base`
    /// when the HEAD baseline may have changed (tab switch, file open, git
    /// working-tree change) rather than only the buffer.
    pub(crate) fn mark_inline_diff_dirty(&mut self, refetch_base: bool) {
        self.inline_diff_dirty = true;
        if refetch_base {
            self.inline_diff_base_dirty = true;
        }
    }

    /// Consumes a pending inline-diff recompute request. Called from `render`,
    /// which provides the `Context` needed to spawn the background work.
    pub(crate) fn process_pending_inline_diff(&mut self, cx: &mut Context<Self>) {
        if !self.inline_diff_dirty {
            return;
        }
        self.inline_diff_dirty = false;
        let refetch_base = std::mem::take(&mut self.inline_diff_base_dirty);
        self.refresh_active_inline_diff(refetch_base, cx);
    }

    fn refresh_active_inline_diff(&mut self, refetch_base: bool, cx: &mut Context<Self>) {
        let Some(index) = self.active_tab_index() else {
            return;
        };
        let tab = &self.tabs[index];
        let supported = tab.kind == TabKind::Editor
            && tab.path.is_some()
            && tab
                .file_kind
                .as_ref()
                .is_some_and(FileKind::supports_text_buffer);
        if !supported {
            if self.tabs[index].inline_diff.is_some() {
                self.tabs[index].inline_diff = None;
                self.invalidate_line_geometry_cache();
                self.layout_dirty = true;
                cx.notify();
            }
            return;
        }

        let Some(path) = tab.path.clone() else {
            return;
        };
        let Some(file_kind) = tab.file_kind.clone() else {
            return;
        };
        let tab_id = tab.id.clone();
        let target_text = self.buffer.text().to_string();
        let need_base = refetch_base || tab.inline_diff.is_none();
        let cached_base = if need_base {
            None
        } else {
            tab.inline_diff.as_ref().map(|diff| diff.base_text.clone())
        };
        let expanded_keys: Vec<usize> = tab
            .inline_diff
            .as_ref()
            .map(|diff| {
                diff.hunks
                    .iter()
                    .filter(|hunk| hunk.expanded)
                    .map(|hunk| hunk.base_start)
                    .collect()
            })
            .unwrap_or_default();

        self.inline_diff_epoch = self.inline_diff_epoch.wrapping_add(1);
        let epoch = self.inline_diff_epoch;

        cx.spawn(async move |this, cx| {
            // Debounce: collapse rapid edits into a single recompute.
            cx.background_executor()
                .timer(Duration::from_millis(120))
                .await;
            if this
                .read_with(cx, |this, _| this.inline_diff_epoch != epoch)
                .unwrap_or(true)
            {
                return;
            }

            let computed = cx
                .background_executor()
                .spawn(async move {
                    let base_text = match cached_base {
                        Some(text) => text,
                        None => {
                            let context = git_repo_context_for_file(&path)?;
                            git_show_file_at_revision(
                                &context.repo_root,
                                "HEAD",
                                &context.relative_path,
                            )
                        }
                    };
                    Some(compute_inline_diff_state(
                        &base_text,
                        &target_text,
                        &file_kind,
                    ))
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                if this.inline_diff_epoch != epoch {
                    return;
                }
                let Some(tab) = this.all_tabs_mut().find(|tab| tab.id == tab_id) else {
                    return;
                };
                match computed {
                    Some(mut state) => {
                        if !expanded_keys.is_empty() {
                            for hunk in &mut state.hunks {
                                if expanded_keys.contains(&hunk.base_start) {
                                    hunk.expanded = true;
                                }
                            }
                        }
                        tab.inline_diff = Some(state);
                    }
                    None => {
                        tab.inline_diff = None;
                    }
                }
                // Keep the measured phantom-height cache across recomputes: the
                // removed lines of an unchanged hunk shape to the same height, so
                // reusing it avoids a one-frame estimate flash (vertical jitter)
                // while editing the added/modified lines. Stale entries for lines
                // that no longer anchor phantoms are ignored by the getters.
                this.refresh_generated_block_previews();
                this.phantom_selection = None;
                this.phantom_selecting = false;
                this.invalidate_line_geometry_cache();
                this.layout_dirty = true;
                cx.notify();
            });
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::markdown::BlockPreviewKind;

    #[test]
    fn removed_rows_keep_generated_preview_decorations() {
        let base = "```mermaid\ngraph TD\nA-->B\n```\n";
        let state = compute_inline_diff_state(base, "", &FileKind::Markdown);
        let removed_lines = state
            .hunks
            .first()
            .map(|hunk| hunk.removed_lines.as_slice())
            .unwrap_or(&[]);

        assert_eq!(removed_lines.len(), 4);
        let preview = removed_lines[0]
            .decoration
            .block_preview
            .as_ref()
            .expect("removed Mermaid preview decoration");
        assert_eq!(preview.kind, BlockPreviewKind::Mermaid);
        assert!(preview.is_anchor);
        assert!(
            removed_lines[1]
                .decoration
                .block_preview
                .as_ref()
                .is_some_and(|preview| !preview.is_anchor)
        );
    }

    #[test]
    fn removed_table_rows_match_diff_editor_source_rendering() {
        let base = "| A | B |\n| --- | --- |\n| 1 | 2 |\n";
        let state = compute_inline_diff_state(base, "", &FileKind::Markdown);
        let removed_lines = state
            .hunks
            .first()
            .map(|hunk| hunk.removed_lines.as_slice())
            .unwrap_or(&[]);

        assert!(removed_lines.iter().all(|line| {
            line.decoration.block_type == crate::markdown::BlockType::CodeBlock
                && line.decoration.code_language.as_deref() == Some("table")
        }));
    }
}
