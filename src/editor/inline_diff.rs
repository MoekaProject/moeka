//! In-place (working-tree vs HEAD) diff overlay.
//!
//! This computes a per-buffer-line diff of the active editable file against its
//! HEAD version and stores it on the tab as [`InlineDiffState`]. Unlike the
//! dedicated read-only diff tab (see `info_panel.rs`), the result is layered on
//! top of the live buffer so the editor can draw gutter indicators, inline
//! highlights, and expandable read-only "phantom" rows for removed lines while
//! the added/modified lines stay fully editable.

use std::borrow::Cow;
use std::ops::Range;
use std::time::Duration;

use gpui::{Context, Pixels, Point, point, px};
use similar::{Algorithm, DiffOp, TextDiff};

use super::info_panel::build_inline_change_text_and_highlights;
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

/// Project existing diff rows without changing their stored HEAD decorations.
pub(super) fn phantom_row_decoration(
    decoration: &crate::markdown::LineDecoration,
    source_mode: bool,
) -> Cow<'_, crate::markdown::LineDecoration> {
    if source_mode {
        let mut projected = decoration.clone();
        helpers::source_markdown_decoration(&mut projected);
        Cow::Owned(projected)
    } else {
        Cow::Borrowed(decoration)
    }
}

/// Geometry needs only the role, not a clone of the preview's source strings.
fn phantom_row_block_type(decoration: &LineDecoration, source_mode: bool) -> &BlockType {
    if source_mode
        && (matches!(
            decoration.block_type,
            BlockType::CodeBlock | BlockType::CodeBlockFence | BlockType::Table
        ) || decoration.block_preview.is_some()
            || decoration.front_matter.is_some())
    {
        &BlockType::Normal
    } else {
        &decoration.block_type
    }
}

fn phantom_reserved_height(
    has_rows: bool,
    measured: Option<f32>,
    estimate: impl FnOnce() -> f32,
) -> f32 {
    if has_rows {
        measured.unwrap_or_else(estimate)
    } else {
        // Measurements survive hunk collapse/recompute, but no longer reserve
        // space until this line actually anchors expanded removed rows again.
        0.0
    }
}

impl EditorView {
    /// Clear presentation-dependent measurements before a document-mode reflow.
    pub(crate) fn clear_inline_diff_phantom_layout_cache(&mut self) {
        self.inline_diff_phantom_height_above.clear();
        self.inline_diff_phantom_height_below.clear();
        self.phantom_row_hits.clear();
        self.invalidate_line_geometry_cache();
        self.layout_dirty = true;
    }

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
        phantom_reserved_height(
            self.inline_diff_phantom_rows_above(line) > 0,
            self.inline_diff_phantom_height_above.get(&line).copied(),
            || self.estimate_inline_diff_phantom_height(line, false),
        )
    }

    /// Vertical space reserved below `line` for a trailing deletion's removed
    /// rows (see [`inline_diff_phantom_reserved_above`]).
    pub(crate) fn inline_diff_phantom_reserved_below(&self, line: usize) -> f32 {
        phantom_reserved_height(
            self.inline_diff_phantom_rows_below(line) > 0,
            self.inline_diff_phantom_height_below.get(&line).copied(),
            || self.estimate_inline_diff_phantom_height(line, true),
        )
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
                    height += self.line_height_for_block(phantom_row_block_type(
                        &removed.decoration,
                        self.source_mode,
                    ));
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
        if !self.begin_content_edit(cx) {
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
        if !self.inline_diff_dirty
            || self.current_file_path.as_ref().is_some_and(|path| {
                file_watch::operation_suppressed("Refresh inline Git diff", path)
            })
        {
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

            let read_path = path.clone();
            let computed =
                crate::platform::fs_task::run("Refresh inline Git diff", path.clone(), move || {
                    let base_text = match cached_base {
                        Some(text) => text,
                        None => {
                            let Some(parent) = read_path.parent() else {
                                return Ok(None);
                            };
                            let Some(root) =
                                git_repo_root_result(parent).map_err(std::io::Error::other)?
                            else {
                                return Ok(None);
                            };
                            let relative = read_path
                                .strip_prefix(&root)
                                .map_err(std::io::Error::other)?
                                .to_string_lossy()
                                .replace('\\', "/");
                            let object = format!("HEAD:{relative}");
                            match run_git_raw(&root, &["show", &object]) {
                                Ok(text) => text,
                                Err(error)
                                    if error.contains("does not exist in 'HEAD'")
                                        || error.contains("exists on disk, but not in 'HEAD'")
                                        || error.contains("invalid object name 'HEAD'") =>
                                {
                                    String::new()
                                }
                                Err(error) => return Err(std::io::Error::other(error)),
                            }
                        }
                    };
                    crate::platform::fs_task::check_deadline()?;
                    Ok(Some(compute_inline_diff_state(
                        &base_text,
                        &target_text,
                        &file_kind,
                    )))
                })
                .await;

            let _ = this.update(cx, |this, cx| {
                if this.current_file_path.as_ref() != Some(&path) {
                    return;
                }
                let computed = match computed {
                    Ok(computed) => computed,
                    Err(error) => {
                        // A filesystem failure applies even if another edit has
                        // superseded this computation while its read was stuck.
                        this.inline_diff_dirty = true;
                        this.inline_diff_base_dirty |= need_base;
                        this.notify_fs_error("Refresh inline Git diff", &path, &error, cx);
                        return;
                    }
                };
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
    fn phantom_reserved_height_prefers_measurements_without_estimating() {
        for measured in [0.0, 23.0, 147.5] {
            assert_eq!(
                phantom_reserved_height(true, Some(measured), || panic!(
                    "measured rows must not be estimated"
                )),
                measured
            );
            assert_eq!(
                phantom_reserved_height(false, Some(measured), || panic!(
                    "collapsed or removed anchors must not be estimated"
                )),
                0.0
            );
        }
        let calls = std::cell::Cell::new(0);
        assert_eq!(
            phantom_reserved_height(true, None, || {
                calls.set(calls.get() + 1);
                42.0
            }),
            42.0
        );
        assert_eq!(calls.get(), 1);
        assert_eq!(
            phantom_reserved_height(false, None, || panic!("non-anchors have no estimate")),
            0.0
        );
        // A preserved measurement is reused after the hunk is re-expanded.
        assert_eq!(
            phantom_reserved_height(true, Some(147.5), || panic!(
                "re-expanded rows retain their measurement"
            )),
            147.5
        );
    }

    #[test]
    fn phantom_block_role_matches_projection_without_copying_preview_payloads() {
        for source in [
            "# Heading **bold**\n\n- [x] task\n\n> quote\n",
            "---\ntitle: Metadata\n---\n",
            "```rust\nlet x = 1;\n```\n",
            "| A | B |\n| --- | --- |\n| 1 | 2 |\n",
            "$$\nx^2\n$$\n",
            "\\[\nx^2\n\\]\n",
            "```mermaid\ngraph TD\nA-->B\n```\n",
            "![alt](image.png)\n",
        ] {
            for row in crate::markdown::parse_decorations(source) {
                for source_mode in [false, true] {
                    let role = phantom_row_block_type(&row, source_mode);
                    assert_eq!(*role, phantom_row_decoration(&row, source_mode).block_type);
                    if !source_mode || *role != BlockType::Normal {
                        assert!(std::ptr::eq(role, &row.block_type));
                    }
                }
            }
        }
    }

    #[test]
    fn source_phantom_projection_is_raw_and_reversible_for_existing_rows() {
        for base in [
            "---\ntitle: '**YAML**'\nitems: [one, two]\n---\n",
            "# Heading 中🙂\n\n- [x] **task**\n\n[link](target.md) and `code`\n",
            "| A | B |\n| --- | --- |\n| ~~value~~ | a<br>b |\n",
            "```rust\nlet title = \"中🙂\";\n\n```\n",
            "~~~python\nreturn \"中🙂\"\n~~~\n",
            "```mermaid\ngraph TD\nA[中🙂]-->B\n```\n",
            "$$\n\\frac{x^2}{2}\n$$\n\n![alt](image.png)\n",
            "\\[\n\\frac{x^2}{2}\n\\]\n",
            "$$\\alpha + x$$\n",
        ] {
            let state = compute_inline_diff_state(base, "", &FileKind::Markdown);
            assert!(!state.hunks.is_empty());
            for removed in state.hunks.iter().flat_map(|hunk| &hunk.removed_lines) {
                let original = removed.clone();
                for source_mode in [false, true, false] {
                    let decoration = phantom_row_decoration(&removed.decoration, source_mode);
                    if source_mode {
                        if matches!(
                            removed.decoration.block_type,
                            BlockType::Heading(_) | BlockType::TaskList(_)
                        ) {
                            assert_eq!(decoration.block_type, removed.decoration.block_type);
                            assert_eq!(decoration.inline_spans, removed.decoration.inline_spans);
                        }
                        assert!(decoration.table_info.is_none());
                        assert!(decoration.block_preview.is_none());
                        assert!(decoration.image_path.is_none());
                        let mut shared_projection = removed.decoration.clone();
                        helpers::source_markdown_decoration(&mut shared_projection);
                        assert_eq!(decoration.as_ref(), &shared_projection);
                        if removed
                            .decoration
                            .block_preview
                            .as_ref()
                            .is_some_and(|preview| preview.kind == BlockPreviewKind::Math)
                        {
                            assert_eq!(
                                decoration.code_language.as_deref(),
                                Some(helpers::SOURCE_MATH_HIGHLIGHT_LANGUAGE)
                            );
                        } else if removed.decoration.code_language.as_deref() == Some("table") {
                            assert_eq!(decoration.code_language.as_deref(), Some("markdown"));
                        } else if let Some(language) = removed.decoration.code_language.as_ref() {
                            assert_eq!(decoration.code_language.as_ref(), Some(language));
                        }
                        let mut active_ranges = Vec::new();
                        helpers::expand_source_syntax(true, removed.text.len(), &mut active_ranges);
                        let (display, display_to_buffer, buffer_to_display, collapsed) =
                            helpers::build_display_text(
                                &removed.text,
                                &decoration,
                                &active_ranges,
                                false,
                                None,
                                1,
                                3,
                                3,
                                2,
                                0,
                                0.7,
                                0.7,
                                0.7,
                            );
                        assert_eq!(display, removed.text);
                        assert_eq!(
                            display_to_buffer,
                            (0..removed.text.len()).collect::<Vec<_>>()
                        );
                        assert_eq!(
                            buffer_to_display,
                            (0..=removed.text.len()).collect::<Vec<_>>()
                        );
                        assert!(!collapsed);
                    } else {
                        assert!(std::ptr::eq(decoration.as_ref(), &removed.decoration));
                        assert_eq!(decoration.as_ref(), &original.decoration);
                    }
                }
                assert_eq!(removed.text, original.text);
                assert_eq!(removed.removed_spans, original.removed_spans);
                assert_eq!(removed.decoration, original.decoration);
            }
            assert_eq!(state.base_text, base);
        }
    }

    #[test]
    fn source_phantom_geometry_preserves_markdown_roles_and_baselines() {
        let body = gpui::font("Test Body");
        let heading = gpui::font("Test Heading");
        let mono = gpui::font("Test Mono");
        let mut decoration = LineDecoration::default();
        decoration.block_type = BlockType::Heading(1);

        for source_mode in [false, true, false] {
            let projected = phantom_row_decoration(&decoration, source_mode);
            assert_eq!(projected.block_type, BlockType::Heading(1));
            assert_eq!(
                helpers::line_baseline_font(&projected, false, &body, &heading, &mono),
                heading.clone().bold(),
            );
        }
        for source in ["```rust\nlet x = 1;\n```", "$$\nx^2\n$$"] {
            for row in crate::markdown::parse_decorations(source) {
                let projected = phantom_row_decoration(&row, true);
                assert_eq!(projected.block_type, BlockType::Normal);
                assert_eq!(
                    helpers::line_baseline_font(&projected, false, &body, &heading, &mono),
                    body,
                );
            }
        }
    }

    #[test]
    fn source_phantom_table_and_mermaid_syntax_reaches_text_runs() {
        let base = "| **bold** | `code` |\n| :--- | ---: |\n| 中🙂 | value |\n\n```mermaid\nsequenceDiagram\nAlice->>Bob: Hello 中🙂\n```\n";
        let state = compute_inline_diff_state(base, "", &FileKind::Markdown);
        let theme = crate::theme::Theme::one_dark();
        let font = gpui::font("Test Body");
        for removed in state.hunks.iter().flat_map(|hunk| &hunk.removed_lines) {
            let decoration = phantom_row_decoration(&removed.decoration, true);
            let line = &removed.text;
            let mut active = Vec::new();
            helpers::expand_source_syntax(true, line.len(), &mut active);
            let (display, d2b, b2d, collapsed) = helpers::build_display_text(
                line,
                &decoration,
                &active,
                false,
                None,
                0,
                3,
                3,
                2,
                0,
                0.7,
                0.7,
                0.7,
            );
            assert_eq!(&display, line);
            assert_eq!(d2b, (0..line.len()).collect::<Vec<_>>());
            assert_eq!(b2d, (0..=line.len()).collect::<Vec<_>>());
            assert!(!collapsed);
            assert!(decoration.table_info.is_none());
            assert!(decoration.block_preview.is_none());
            let runs = helpers::build_text_runs_for_display(
                &display,
                line,
                &d2b,
                &decoration,
                &active,
                false,
                collapsed,
                &font,
                &font,
                &font,
                &font,
                &font,
                &font,
                &font,
                3,
                16.0,
                &theme,
                theme.git_status_added,
                theme.text_danger,
                theme.link,
                gpui::FontWeight::NORMAL,
                theme.list_marker,
                gpui::FontWeight::BOLD,
                theme.syntax_marker,
                None,
                false,
            );
            let color_at = |offset: usize| {
                let mut end = 0;
                runs.iter()
                    .find_map(|run| {
                        end += run.len;
                        (offset < end).then_some(run.color)
                    })
                    .expect("highlighted byte has a native text run")
            };
            for (token, color) in [
                ("bold", theme.code_palette.number),
                ("code", theme.code_palette.string),
                (":---", theme.code_palette.keyword),
                ("sequenceDiagram", theme.code_palette.keyword),
                ("->>", theme.code_palette.operator),
                ("Hello 中🙂", theme.code_palette.string),
            ] {
                if let Some(offset) = line.find(token) {
                    assert_eq!(
                        color_at(offset),
                        gpui::rgb(color).into(),
                        "{line:?}: {token}"
                    );
                }
            }
            if line.starts_with('|') {
                assert_eq!(decoration.code_language.as_deref(), Some("markdown"));
                assert_eq!(color_at(0), gpui::rgb(theme.code_palette.operator).into());
            } else if line == "```" || line == "```mermaid" {
                assert_eq!(color_at(0), theme.syntax_marker);
            }
        }
    }

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
