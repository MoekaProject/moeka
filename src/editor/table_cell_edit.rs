//! Source ownership for edits in rendered Markdown cells. Semantic table ranges
//! trim padding; editing must use the same real trailing whitespace as rendering.
use std::{borrow::Cow, ops::Range};

use gpui::Context;

use crate::{buffer::Buffer, cursor::Cursor, markdown::LineDecoration};

use super::{
    EditorView,
    table_cell_list_edit::{self as lists, Edit},
};

pub(super) fn cell_range_for_selection(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    rendered_markdown: bool,
    selection: &Range<usize>,
) -> Option<Range<usize>> {
    if !rendered_markdown
        || selection.start > selection.end
        || buffer.text().get(selection.clone()).is_none()
    {
        return None;
    }
    let (line, _) = buffer.offset_to_line_col(selection.start);
    let info = decorations.get(line)?.table_info.as_ref()?;
    if info.is_separator || info.is_front_matter {
        return None;
    }
    let line_start = buffer.line_start(line);
    let source = buffer.line_content(line);
    for col in 0..info.cell_ranges.len() {
        let (content, range) = super::table_cell_render::content_with_range(info, source, col);
        // Reject stale metadata rather than claiming unrelated source bytes.
        if source.get(range.clone()) != Some(content.as_str()) {
            continue;
        }
        let range = line_start + range.start..line_start + range.end;
        // Ranges own insertion at their end, including immediately BEFORE a
        // pipe, but never the pipe byte or any part of the following cell.
        if range.start <= selection.start && selection.end <= range.end {
            return Some(range);
        }
    }
    None
}

/// Remap a pre-insertion target after materializing a table boundary with one
/// source newline. Only a caret exactly on that boundary follows its visual
/// outside affinity; other ranges continue to refer to the same source text.
pub(super) fn native_range_after_boundary_insertion(
    range: Range<usize>,
    boundary: Option<(usize, usize)>,
) -> Range<usize> {
    let Some((insertion, outside)) = boundary else {
        return range;
    };
    if range.start == insertion && range.end == insertion {
        outside..outside
    } else if range.start >= insertion {
        range.start + 1..range.end + 1
    } else {
        range.start..range.end + usize::from(range.end > insertion)
    }
}

fn cell_newlines(text: &str) -> String {
    text.replace("\r\n", "\n").replace(['\r', '\n'], "<br>")
}

#[derive(Debug)]
struct NativeEdit {
    range: Range<usize>,
    text: String,
    selection: Range<usize>,
    primary: bool,
}

/// Resolve every target before mutating: two carets can share a source row.
/// Native selection offsets refer to the incoming UTF-16 text, not the expanded
/// `<br>` source, so translate both endpoints before applying the edit.
fn native_newline_edits(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    rendered_markdown: bool,
    targets: Vec<(Range<usize>, bool)>,
    text: &str,
    selected_utf16: Option<&Range<usize>>,
) -> Option<Vec<NativeEdit>> {
    if !text.contains(['\r', '\n']) {
        return None;
    }
    let selection = selected_utf16.map_or(text.len()..text.len(), |range| {
        EditorView::utf8_offset_in_text_for_utf16(text, range.start)
            ..EditorView::utf8_offset_in_text_for_utf16(text, range.end)
    });
    let mut handled = false;
    let edits = targets
        .into_iter()
        .map(|(range, primary)| {
            let in_cell =
                cell_range_for_selection(buffer, decorations, rendered_markdown, &range).is_some();
            handled |= in_cell;
            NativeEdit {
                range,
                text: if in_cell {
                    cell_newlines(text)
                } else {
                    text.to_owned()
                },
                selection: if in_cell {
                    cell_newlines(&text[..selection.start]).len()
                        ..cell_newlines(&text[..selection.end]).len()
                } else {
                    selection.clone()
                },
                primary,
            }
        })
        .collect();
    handled.then_some(edits)
}

fn apply_native_edits(
    buffer: &mut Buffer,
    cursor: &mut Cursor,
    extra_cursors: &mut Vec<Cursor>,
    marked_range: &mut Option<Range<usize>>,
    extra_marked_ranges: &mut Vec<Range<usize>>,
    mut edits: Vec<NativeEdit>,
    keep_marked: bool,
) {
    edits.sort_by_key(|edit| std::cmp::Reverse(edit.range.start));
    for edit in &edits {
        buffer.replace(edit.range.clone(), &edit.text);
    }
    edits.reverse();
    extra_cursors.clear();
    *marked_range = None;
    extra_marked_ranges.clear();
    let mut delta = 0isize;
    for edit in edits {
        let start = edit.range.start.saturating_add_signed(delta);
        let next_cursor = Cursor {
            anchor: buffer.clamp_offset(start + edit.selection.start),
            offset: buffer.clamp_offset(start + edit.selection.end),
        };
        let marked =
            (keep_marked && !edit.text.is_empty()).then_some(start..start + edit.text.len());
        if edit.primary {
            *cursor = next_cursor;
            *marked_range = marked;
        } else {
            extra_cursors.push(next_cursor);
            if let Some(marked) = marked {
                extra_marked_ranges.push(marked);
            }
        }
        delta += edit.text.len() as isize - edit.range.len() as isize;
    }
}

fn table_source_overlaps(
    buffer: &Buffer,
    rows: &[LineDecoration],
    range: &Range<usize>,
    protect_line_breaks: bool,
) -> bool {
    let first = buffer.offset_to_line_col(range.start.saturating_sub(1)).0;
    let last = buffer.offset_to_line_col(range.end.min(buffer.len())).0;
    (first..=last).any(|line| {
        let Some(info) = rows.get(line).and_then(|row| row.table_info.as_ref()) else {
            return false;
        };
        if info.is_front_matter {
            return false;
        }
        let start = buffer
            .line_start(line)
            .saturating_sub(usize::from(protect_line_breaks));
        let end = (buffer.line_end(line) + usize::from(protect_line_breaks)).min(buffer.len());
        if range.is_empty() {
            buffer.line_start(line) <= range.start && range.start <= buffer.line_end(line)
        } else {
            range.start < end && start < range.end
        }
    })
}

fn ordinary_delete_plan(buffer: &Buffer, cursor: Cursor, action: lists::DeleteAction) -> Edit {
    use lists::DeleteAction;
    let range = if cursor.has_selection() && action != DeleteAction::Line {
        cursor.selection_range()
    } else {
        let offset = cursor.offset;
        let line = buffer.offset_to_line_col(offset).0;
        match action {
            DeleteAction::Backward => buffer.prev_grapheme_boundary(offset)..offset,
            DeleteAction::Forward => offset..buffer.next_grapheme_boundary(offset),
            DeleteAction::Word { backwards } => {
                let mut probe = Cursor {
                    offset,
                    anchor: offset,
                };
                if backwards {
                    probe.move_word_left(buffer);
                } else {
                    probe.move_word_right(buffer);
                }
                offset.min(probe.offset)..offset.max(probe.offset)
            }
            DeleteAction::LineBoundary { to_start: true, .. } => buffer.line_start(line)..offset,
            DeleteAction::LineBoundary {
                to_start: false,
                kill,
            } => {
                let end = buffer.line_end(line);
                offset..if kill && offset == end && end < buffer.len() {
                    end + 1
                } else {
                    end
                }
            }
            DeleteAction::Line => {
                let start = buffer.line_start(line);
                let end = buffer.line_end(line);
                let end = end + usize::from(end < buffer.len());
                start.saturating_sub(usize::from(start == end && start > 0))..end
            }
        }
    };
    Edit::new(range, String::new())
}

/// Cell-local bytes can still expose a literal pipe (by deleting its escape or
/// code delimiter). Validate the combined row edit, not merely each range's
/// containment, so independent cursors cannot change the table's column owners.
fn row_deletions_preserve_cells(
    buffer: &Buffer,
    rows: &[LineDecoration],
    line: usize,
    plans: &[(Cursor, bool, Edit)],
) -> bool {
    let edits: Vec<_> = lists::deletion_ranges(plans)
        .into_iter()
        .map(|range| Edit::new(range, String::new()))
        .collect();
    row_edits_preserve_cells(buffer, rows, line, &edits)
}

/// Simulate replacements as well as deletions: even inserting inside a code
/// delimiter can expose a pipe. Boundary affinity keeps newly inserted bytes
/// in their owner, including an insertion at either end of an empty cell.
fn row_edits_preserve_cells(
    buffer: &Buffer,
    rows: &[LineDecoration],
    line: usize,
    edits: &[Edit],
) -> bool {
    let mut edits: Vec<_> = edits
        .iter()
        .filter(|edit| !edit.range.is_empty() || !edit.text.is_empty())
        .collect();
    edits.sort_by_key(|edit| (edit.range.start, edit.range.end));
    if edits
        .windows(2)
        .any(|pair| pair[0].range.end > pair[1].range.start)
    {
        return false;
    }
    let start = buffer.line_start(line);
    let source = buffer.line_content(line);
    let before = crate::markdown::parse_table_cells_with_ranges(source);
    let mut after = source.to_string();
    let remap = |offset: usize, right: bool| {
        let mut delta = 0isize;
        for edit in &edits {
            if offset < edit.range.start || (offset == edit.range.start && !right) {
                break;
            }
            if offset <= edit.range.end {
                return edit.range.start.saturating_add_signed(delta)
                    + if right { edit.text.len() } else { 0 };
            }
            delta += edit.text.len() as isize - edit.range.len() as isize;
        }
        offset.saturating_add_signed(delta)
    };
    let mut owners = Vec::new();
    for edit in &edits {
        let Some(cell) = cell_range_for_selection(buffer, rows, true, &edit.range) else {
            return false;
        };
        let Some(column) = before
            .iter()
            .position(|(_, r)| cell.start <= start + r.start && start + r.end <= cell.end)
        else {
            return false;
        };
        owners.push((column, cell));
    }
    for edit in edits.iter().rev() {
        let Some(local_start) = edit.range.start.checked_sub(start) else {
            return false;
        };
        let Some(local_end) = edit
            .range
            .end
            .checked_sub(start)
            .filter(|end| *end <= source.len())
        else {
            return false;
        };
        if edit.text.contains(['\r', '\n']) {
            return false;
        }
        after.replace_range(local_start..local_end, &edit.text);
    }
    let after = crate::markdown::parse_table_cells_with_ranges(&after);
    if before.len() != after.len() {
        return false;
    }
    before
        .iter()
        .zip(&after)
        .enumerate()
        .all(|(column, ((text, old), (next, new)))| {
            if let Some((_, owner)) = owners.iter().find(|(index, _)| *index == column) {
                remap(owner.start, false) <= start + new.start
                    && start + new.end <= remap(owner.end, true)
            } else {
                text == next
                    && start + new.start == remap(start + old.start, true)
                    && start + new.end == remap(start + old.end, true)
            }
        })
}

fn multiline_selection(buffer: &Buffer, cursor: Cursor) -> bool {
    cursor.has_selection()
        && buffer.offset_to_line_col(cursor.offset).0 != buffer.offset_to_line_col(cursor.anchor).0
}

/// Protect implicit structural edits, not explicit selections spanning blocks.
fn deletion_plan(
    buffer: &Buffer,
    rows: &[LineDecoration],
    rendered_markdown: bool,
    cursor: Cursor,
    action: lists::DeleteAction,
) -> (Edit, bool) {
    let selection = cursor.selection_range();
    if buffer.text().get(selection.clone()).is_none() {
        return (
            Edit::new(cursor.offset..cursor.offset, String::new()),
            rendered_markdown,
        );
    }
    if let Some(cell) = cell_range_for_selection(buffer, rows, rendered_markdown, &selection) {
        let decoration = lists::cell_decoration(buffer, rows, &cell);
        let local = Cursor {
            offset: cursor.offset - cell.start,
            anchor: cursor.anchor - cell.start,
        };
        let edit = lists::cell_delete(
            buffer.text_for_range(cell.clone()),
            &decoration,
            local,
            action,
        )
        .translated(cell.start);
        let line = buffer.offset_to_line_col(cell.start).0;
        let edit =
            if row_deletions_preserve_cells(buffer, rows, line, &[(cursor, true, edit.clone())]) {
                edit
            } else {
                Edit::new(cursor.offset..cursor.offset, String::new())
            };
        return (edit, true);
    }
    let mut edit = ordinary_delete_plan(buffer, cursor, action);
    let table = rendered_markdown
        && (table_source_overlaps(buffer, rows, &selection, false)
            || table_source_overlaps(
                buffer,
                rows,
                &edit.range,
                action != lists::DeleteAction::Line,
            ));
    if table && (action == lists::DeleteAction::Line || !multiline_selection(buffer, cursor)) {
        edit = Edit::new(cursor.offset..cursor.offset, String::new());
    } else if action == lists::DeleteAction::Line
        && super::ordered_list_near_line(rows, buffer.offset_to_line_col(cursor.offset).0)
    {
        edit.normalize = Some((false, 0));
    }
    (edit, table)
}

fn backspace_table_tail_target(
    buffer: &Buffer,
    rows: &[LineDecoration],
    cursor: Cursor,
) -> Option<usize> {
    let start = if cursor.has_selection() {
        cursor.selection_range().start
    } else {
        cursor.offset
    };
    let (line, _) = buffer.offset_to_line_col(start);
    if line == 0 {
        return None;
    }
    let line_start = buffer.line_start(line);
    let line_end = buffer.line_end(line);
    let current_line_blank = buffer
        .line_content(line)
        .trim_matches([' ', '\t', '\r'])
        .is_empty();
    if cursor.has_selection() {
        let selected_line_end = line_end + usize::from(line_end < buffer.len());
        if start != line_start || cursor.selection_range().end < selected_line_end {
            return None;
        }
    } else {
        if current_line_blank {
            if cursor.offset < line_start || cursor.offset > line_end {
                return None;
            }
        } else if cursor.offset != line_start {
            return None;
        }
    }
    let mut table_line = line;
    while table_line > 0 {
        table_line -= 1;
        if current_line_blank
            && buffer
                .line_content(table_line)
                .trim_matches([' ', '\t', '\r'])
                .is_empty()
        {
            continue;
        }
        break;
    }
    let info = rows.get(table_line)?.table_info.as_ref()?;
    if info.is_front_matter || info.is_separator || info.table_end_line != table_line + 1 {
        return None;
    }
    let tail = buffer.line_end(table_line);
    Some(tail - usize::from(buffer.line_content(table_line).ends_with('\r')))
}

struct BlankTableRowBackspace {
    range: Range<usize>,
    target_offset: usize,
    boundary: Option<super::TableBoundaryCaret>,
}

fn blank_table_row_backspace(
    buffer: &Buffer,
    rows: &[LineDecoration],
    cursor: Cursor,
) -> Option<BlankTableRowBackspace> {
    if cursor.has_selection() {
        return None;
    }
    let (line, _) = buffer.offset_to_line_col(cursor.offset);
    if line == 0 {
        return None;
    }
    let line_start = buffer.line_start(line);
    let content = buffer.line_content(line);
    if !content.trim_matches([' ', '\t', '\r']).is_empty()
        || cursor.offset < line_start
        || cursor.offset > buffer.line_end(line)
    {
        return None;
    }

    let previous_separator_len = if line_start >= 2
        && buffer.text().as_bytes().get(line_start - 2..line_start) == Some(b"\r\n")
    {
        2
    } else if buffer.text().as_bytes().get(line_start - 1) == Some(&b'\n') {
        1
    } else {
        return None;
    };
    let mut table_line = line;
    while table_line > 0 {
        table_line -= 1;
        if buffer
            .line_content(table_line)
            .trim_matches([' ', '\t', '\r'])
            .is_empty()
        {
            continue;
        }
        break;
    }
    let info = rows.get(table_line)?.table_info.as_ref()?;
    if info.is_front_matter || info.is_separator || info.table_end_line != table_line + 1 {
        return None;
    }

    let content_without_cr = content.strip_suffix('\r').unwrap_or(content);
    let range = line_start - previous_separator_len..line_start + content_without_cr.len();
    let retains_line_ending = line + 1 < buffer.line_count();
    let retained_cr = usize::from(retains_line_ending && content.ends_with('\r'));
    let next_line_blank = retains_line_ending
        && buffer
            .line_content(line + 1)
            .trim_matches([' ', '\t', '\r'])
            .is_empty();
    let previous_blank_remains = table_line + 1 < line;
    let table_is_adjacent = table_line + 1 == line;
    let target_offset = if next_line_blank {
        range.start + usize::from(retains_line_ending) + retained_cr
    } else if previous_blank_remains {
        buffer.line_start(line - 1)
    } else {
        range.start + retained_cr
    };
    Some(BlankTableRowBackspace {
        range: range.clone(),
        target_offset,
        boundary: (table_is_adjacent && !next_line_blank).then_some(super::TableBoundaryCaret {
            table_start_line: info.table_start_line,
            side: super::TableBoundarySide::Below,
        }),
    })
}

fn deletion_plans(
    buffer: &Buffer,
    rows: &[LineDecoration],
    rendered_markdown: bool,
    targets: Vec<(Cursor, bool)>,
    action: lists::DeleteAction,
) -> Vec<(Cursor, bool, Edit)> {
    let mut plans: Vec<_> = targets
        .into_iter()
        .map(|(cursor, primary)| {
            (
                cursor,
                primary,
                deletion_plan(buffer, rows, rendered_markdown, cursor, action).0,
            )
        })
        .collect();
    if rendered_markdown {
        let mut lines: Vec<_> = plans
            .iter()
            .filter(|(_, _, e)| !e.range.is_empty())
            .filter(|(cursor, _, _)| !multiline_selection(buffer, *cursor))
            .filter_map(|(_, _, edit)| {
                let line = buffer.offset_to_line_col(edit.range.start).0;
                rows.get(line)
                    .and_then(|r| r.table_info.as_ref())
                    .filter(|info| !info.is_front_matter)
                    .map(|_| line)
            })
            .collect();
        lines.sort_unstable();
        lines.dedup();
        for line in lines {
            let targets: Vec<_> = plans
                .iter()
                .filter(|(_, _, e)| {
                    !e.range.is_empty() && buffer.offset_to_line_col(e.range.start).0 == line
                })
                .filter(|(cursor, _, _)| !multiline_selection(buffer, *cursor))
                .cloned()
                .collect();
            if !row_deletions_preserve_cells(buffer, rows, line, &targets) {
                for (cursor, _, edit) in &mut plans {
                    if !edit.range.is_empty()
                        && buffer.offset_to_line_col(edit.range.start).0 == line
                        && !multiline_selection(buffer, *cursor)
                    {
                        *edit = Edit::new(cursor.offset..cursor.offset, String::new());
                    }
                }
            }
        }
    }
    plans
}

fn deleted_table_rows_as_blank_lines(
    buffer: &Buffer,
    rows: &[LineDecoration],
    deletion: &Range<usize>,
) -> String {
    let mut blank_lines = String::new();
    for (line, decoration) in rows.iter().enumerate() {
        let Some(info) = decoration.table_info.as_ref().filter(|info| {
            info.is_header && !info.is_front_matter && info.table_start_line == line
        }) else {
            continue;
        };
        let block_start = buffer.line_start(line);
        let end_line = info.table_end_line.min(buffer.line_count());
        let block_end = if end_line < buffer.line_count() {
            buffer.line_start(end_line)
        } else {
            buffer.len()
        };
        if deletion.start > block_start || deletion.end < block_end {
            continue;
        }
        for table_line in line..end_line {
            let line_end = buffer.line_end(table_line);
            if line_end < buffer.len() {
                let next_start = buffer.line_start(table_line + 1);
                let newline_start =
                    line_end - usize::from(buffer.line_content(table_line).ends_with('\r'));
                blank_lines.push_str(buffer.text().get(newline_start..next_start).unwrap_or("\n"));
            }
        }
    }
    blank_lines
}

fn native_angle_edit(
    buffer: &Buffer,
    rows: &[LineDecoration],
    rendered_markdown: bool,
    cursor: Cursor,
) -> Option<NativeEdit> {
    if cursor.has_selection() {
        return None;
    }
    let cell =
        cell_range_for_selection(buffer, rows, rendered_markdown, &cursor.selection_range())?;
    let decoration = lists::cell_decoration(buffer, rows, &cell);
    let edit = lists::cell_angle_input(
        buffer.text_for_range(cell.clone()),
        &decoration,
        cursor.offset - cell.start,
    )?
    .translated(cell.start);
    Some(NativeEdit {
        range: edit.range,
        text: edit.text,
        selection: edit.caret..edit.caret,
        primary: true,
    })
}

fn cell_indent_plan(
    buffer: &Buffer,
    rows: &[LineDecoration],
    cell: Range<usize>,
    selection: Range<usize>,
    forward: bool,
) -> Option<Vec<Edit>> {
    let decoration = lists::cell_decoration(buffer, rows, &cell);
    let mut plans = lists::cell_indent(
        buffer.text_for_range(cell.clone()),
        &decoration,
        selection.start - cell.start..selection.end - cell.start,
        forward,
    )?;
    // `cell` comes from content_with_range: if it starts immediately after
    // the pipe, there is no ASCII formatting space yet. The first inserted
    // space would become padding rather than indentation. Materialize padding
    // once, including before existing tab indentation, without consuming tabs.
    if forward
        && cell
            .start
            .checked_sub(1)
            .and_then(|at| buffer.text().as_bytes().get(at))
            == Some(&b'|')
        && decoration
            .table_cell_fragments
            .first()
            .is_some_and(|f| f.prefix_len > 0)
        && let Some(first) = plans
            .iter_mut()
            .find(|edit| edit.range == (0..0) && !edit.text.is_empty())
    {
        first.text.push(' ');
        first.caret += 1;
    }
    Some(
        plans
            .into_iter()
            .map(|edit| edit.translated(cell.start))
            .collect(),
    )
}

fn cell_enter_plan(
    buffer: &Buffer,
    rows: &[LineDecoration],
    rendered_markdown: bool,
    cursor: Cursor,
    soft: bool,
) -> Option<Edit> {
    let selection = cursor.selection_range();
    let cell = cell_range_for_selection(buffer, rows, rendered_markdown, &selection)?;
    let decoration = lists::cell_decoration(buffer, rows, &cell);
    Some(
        lists::cell_enter(
            buffer.text_for_range(cell.clone()),
            &decoration,
            selection.start - cell.start..selection.end - cell.start,
            cursor.offset - cell.start,
            soft,
        )
        .translated(cell.start),
    )
}

/// Ctrl-O is primary-only and deliberately does not continue list syntax.
/// None means protected, not an invitation to fall back to a physical newline.
fn open_line_plan(
    buffer: &Buffer,
    rows: &[LineDecoration],
    rendered_markdown: bool,
    cursor: Cursor,
) -> Option<Edit> {
    let selection = cursor.selection_range();
    buffer.text().get(selection.clone())?;
    let mut edit =
        if let Some(cell) = cell_range_for_selection(buffer, rows, rendered_markdown, &selection) {
            let decoration = lists::cell_decoration(buffer, rows, &cell);
            let local = Cursor {
                offset: cursor.offset - cell.start,
                anchor: cursor.anchor - cell.start,
            };
            let range = if cursor.has_selection() {
                lists::cell_delete(
                    buffer.text_for_range(cell.clone()),
                    &decoration,
                    local,
                    lists::DeleteAction::Forward,
                )
                .range
            } else {
                decoration
                    .display_replacements
                    .iter()
                    .find(|r| {
                        r.display_text == "\n"
                            && r.source_range.start < local.offset
                            && local.offset < r.source_range.end
                    })
                    .map_or(local.offset..local.offset, |r| r.source_range.clone())
            };
            let edit = Edit::new(range, "<br>".to_string()).translated(cell.start);
            let line = buffer.offset_to_line_col(cell.start).0;
            if !row_edits_preserve_cells(buffer, rows, line, std::slice::from_ref(&edit)) {
                return None;
            }
            edit
        } else {
            if rendered_markdown && table_source_overlaps(buffer, rows, &selection, false) {
                return None;
            }
            Edit::new(selection, "\n".to_string())
        };
    edit.caret = 0;
    Some(edit)
}

impl EditorView {
    pub(super) fn backspace_blank_table_row(&mut self) -> bool {
        let rendered = self.active_tab_is_markdown()
            && self.table_source.is_none()
            && self.table_boundary_caret.is_none();
        if !rendered {
            return false;
        }
        let Some(edit) = blank_table_row_backspace(&self.buffer, &self.decorations, self.cursor)
        else {
            return false;
        };
        self.checkpoint();
        self.queue_non_markdown_line_display_transfer(&edit.range, "");
        self.buffer.delete(edit.range);
        self.cursor.move_to(edit.target_offset, &self.buffer);
        self.table_boundary_caret = edit.boundary;
        self.mark_active_tab_dirty();
        self.clear_marked_ranges();
        self.reset_cursor_visual_targeting();
        self.reparse();
        self.scroll_to_cursor();
        true
    }

    pub(super) fn semantic_open_line_plan(&self) -> Option<Edit> {
        open_line_plan(
            &self.buffer,
            &self.decorations,
            self.active_tab_is_markdown()
                && self.table_source.is_none()
                && self.table_boundary_caret.is_none(),
            self.cursor,
        )
    }

    pub(super) fn table_cell_enter_plan(&self, cursor: Cursor, soft: bool) -> Option<Edit> {
        cell_enter_plan(
            &self.buffer,
            &self.decorations,
            self.active_tab_is_markdown()
                && self.table_source.is_none()
                && self.table_boundary_caret.is_none(),
            cursor,
            soft,
        )
    }

    pub(super) fn semantic_delete_touches_table(&self, action: lists::DeleteAction) -> bool {
        let rendered = self.active_tab_is_markdown()
            && self.table_source.is_none()
            && self.table_boundary_caret.is_none();
        rendered
            && self.all_cursors_snapshot().into_iter().any(|cursor| {
                deletion_plan(&self.buffer, &self.decorations, rendered, cursor, action).1
            })
    }

    pub(super) fn apply_semantic_delete(&mut self, action: lists::DeleteAction) -> bool {
        let plans = self.semantic_delete_plans(action);
        let caret_targets: Vec<_> = if action == lists::DeleteAction::Backward {
            plans
                .iter()
                .map(|(cursor, _, edit)| {
                    if edit.range.is_empty() {
                        None
                    } else {
                        backspace_table_tail_target(&self.buffer, &self.decorations, *cursor)
                    }
                })
                .collect()
        } else {
            Vec::new()
        };
        self.apply_semantic_delete_plans(plans, action, &caret_targets)
    }

    pub(super) fn semantic_delete_plans(
        &self,
        action: lists::DeleteAction,
    ) -> Vec<(Cursor, bool, Edit)> {
        let rendered = self.active_tab_is_markdown()
            && self.table_source.is_none()
            && self.table_boundary_caret.is_none();
        deletion_plans(
            &self.buffer,
            &self.decorations,
            rendered,
            self.cursor_states_with_primary_flag(),
            action,
        )
    }

    pub(super) fn apply_semantic_delete_plans(
        &mut self,
        plans: Vec<(Cursor, bool, Edit)>,
        action: lists::DeleteAction,
        caret_targets: &[Option<usize>],
    ) -> bool {
        let ranges = lists::deletion_ranges(&plans);
        let replacements: Vec<_> = ranges
            .iter()
            .map(|range| {
                (
                    range.clone(),
                    deleted_table_rows_as_blank_lines(&self.buffer, &self.decorations, range),
                )
            })
            .collect();
        let folded_anchors = (ranges.len() > 1).then(|| self.snapshot_folded_anchor_offsets());
        let fold_edits = replacements
            .iter()
            .map(|(range, text)| (range.clone(), text.len()))
            .collect::<Vec<_>>();
        if matches!(action, lists::DeleteAction::LineBoundary { kill: true, .. })
            && !ranges.is_empty()
        {
            self.kill_ring = Some(
                ranges
                    .iter()
                    .map(|r| self.buffer.text_for_range(r.clone()))
                    .collect::<Vec<_>>()
                    .join("\n"),
            );
        }
        if let [(range, text)] = replacements.as_slice() {
            self.queue_non_markdown_line_display_transfer(range, text);
        } else if !ranges.is_empty() {
            self.pending_line_display_transfer = None;
        }
        let mut resolved =
            lists::apply_deletions_with_replacements(&mut self.buffer, &plans, &replacements);
        for ((cursor, primary, _), target) in resolved.iter_mut().zip(caret_targets) {
            if let Some(target) = target {
                if let Some((next, _)) = super::fold::folded_delete_cursor_targets(
                    &self.buffer,
                    &[(*target, *primary)],
                    &fold_edits,
                )
                .into_iter()
                .next()
                {
                    *cursor = next;
                }
            }
        }
        if let Some(folded_anchors) = folded_anchors {
            self.remap_folded_anchor_offsets(folded_anchors, &fold_edits);
        }
        self.finish_table_aware_cursor_plans(resolved);
        !ranges.is_empty()
    }

    /// All target ownership and plans are resolved against the same source.
    /// Keep normalization in this transaction, never in the native text path:
    /// paste and IME newlines are soft breaks, not Enter actions.
    pub(super) fn apply_table_aware_cursor_plans(&mut self, plans: Vec<(Edit, bool)>) {
        let folded_anchors = self
            .pending_line_display_transfer
            .is_none()
            .then(|| self.snapshot_folded_anchor_offsets());
        let fold_edits = plans
            .iter()
            .map(|(edit, _)| (edit.range.clone(), edit.text.len()))
            .collect::<Vec<_>>();
        let Some(resolved) = lists::apply(&mut self.buffer, &plans) else {
            return;
        };
        if let Some(folded_anchors) = folded_anchors {
            self.remap_folded_anchor_offsets(folded_anchors, &fold_edits);
        }
        self.finish_table_aware_cursor_plans(resolved);
    }

    fn finish_table_aware_cursor_plans(
        &mut self,
        resolved: Vec<(Cursor, bool, Option<(bool, usize)>)>,
    ) {
        self.extra_cursors.clear();
        let mut normalizations = Vec::new();
        for (cursor, primary, normalize) in resolved {
            let index = self.extra_cursors.len();
            if primary {
                self.cursor = cursor;
            } else {
                self.extra_cursors.push(cursor);
            }
            if let Some((preserve, back)) = normalize {
                normalizations.push((primary, index, preserve, back));
            }
        }
        self.clear_marked_ranges();
        self.reset_cursor_visual_targeting();
        self.reparse();
        for (primary, index, preserve, back) in normalizations {
            let cursor = if primary {
                self.cursor
            } else {
                self.extra_cursors[index]
            };
            let offset = cursor.offset.saturating_sub(back);
            if let Some(cell) = self.table_edit_cell_range(&(offset..offset)) {
                let decoration = lists::cell_decoration(&self.buffer, &self.decorations, &cell);
                let edits = lists::ordered_edits(
                    self.buffer.text_for_range(cell.clone()),
                    &decoration,
                    offset - cell.start,
                    preserve,
                )
                .into_iter()
                .map(|edit| edit.translated(cell.start))
                .collect();
                self.apply_table_cell_prefix_edits(edits);
            } else {
                let edits = lists::ordinary_ordered_edits(
                    &self.buffer,
                    &self.decorations,
                    offset,
                    preserve,
                );
                self.apply_table_cell_prefix_edits(edits);
            }
            self.reparse();
        }
        self.dedup_cursors();
    }

    fn apply_table_cell_prefix_edits(&mut self, edits: Vec<Edit>) {
        let mut cursors = self.cursor_states_with_primary_flag();
        lists::apply_prefix_edits(&mut self.buffer, &mut cursors, edits);
        // Keep target indices stable until all per-caret normalizations finish.
        // Several carets may collapse onto one emptied fragment.
        self.extra_cursors.clear();
        for (cursor, primary) in cursors {
            if primary {
                self.cursor = cursor;
            } else {
                self.extra_cursors.push(cursor);
            }
        }
    }

    /// List Tab wins over cell navigation. Non-list single-cell Tab retains
    /// table navigation; mixed cell/non-cell multi-cursor Tab must not indent
    /// a physical table row or clear the extra carets via cell navigation.
    pub(super) fn indent_table_cell_lists(
        &mut self,
        forward: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let targets = self.cursor_states_with_primary_flag();
        let mut edits = Vec::new();
        let mut has_cell = false;
        let mut unsafe_selection = false;
        let mut all_lists = true;
        for (cursor, _) in &targets {
            let selection = cursor.selection_range();
            let Some(cell) = self.table_edit_cell_range(&selection) else {
                let cell_endpoint = self
                    .table_edit_cell_range(&(cursor.offset..cursor.offset))
                    .is_some()
                    || self
                        .table_edit_cell_range(&(cursor.anchor..cursor.anchor))
                        .is_some();
                has_cell |= cell_endpoint;
                unsafe_selection |= cell_endpoint;
                all_lists = false;
                continue;
            };
            has_cell = true;
            if let Some(plans) =
                cell_indent_plan(&self.buffer, &self.decorations, cell, selection, forward)
            {
                edits.extend(plans);
            } else {
                all_lists = false;
            }
        }
        if !has_cell {
            return false;
        }
        if !all_lists {
            return if self.has_extra_cursors() || unsafe_selection {
                self.pause_blinking(cx);
                true
            } else {
                false
            };
        }
        self.checkpoint();
        self.apply_table_cell_prefix_edits(edits);
        self.reparse();
        // Match list Tab's near-line renumbering, but keep every edit in its cell.
        for index in 0..targets.len() {
            let cursor = if index == 0 {
                self.cursor
            } else {
                self.extra_cursors[index - 1]
            };
            if let Some(cell) = self.table_edit_cell_range(&cursor.selection_range()) {
                let decoration = lists::cell_decoration(&self.buffer, &self.decorations, &cell);
                let edits = lists::ordered_edits(
                    self.buffer.text_for_range(cell.clone()),
                    &decoration,
                    cursor.offset - cell.start,
                    false,
                )
                .into_iter()
                .map(|edit| edit.translated(cell.start))
                .collect();
                self.apply_table_cell_prefix_edits(edits);
                self.reparse();
            }
        }
        self.dedup_cursors();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
        true
    }

    pub(super) fn table_edit_cell_range(&self, selection: &Range<usize>) -> Option<Range<usize>> {
        cell_range_for_selection(
            &self.buffer,
            &self.decorations,
            self.active_tab_is_markdown()
                && self.table_source.is_none()
                && self.table_boundary_caret.is_none(),
            selection,
        )
    }

    pub(super) fn table_cell_input_text<'a>(
        &self,
        range: &Range<usize>,
        text: &'a str,
    ) -> Cow<'a, str> {
        if text.contains(['\r', '\n']) && self.table_edit_cell_range(range).is_some() {
            Cow::Owned(cell_newlines(text))
        } else {
            Cow::Borrowed(text)
        }
    }

    /// Native insertText/setMarkedText can deliver a newline without dispatching
    /// either Enter action. Also localize an ordinary typed `<` before the shared
    /// native caller can apply physical-row pairing. Explicit ranges, selections,
    /// multi-cursor input and IME composition retain their existing input policy.
    pub(super) fn replace_table_cell_native_newlines(
        &mut self,
        primary_range: Option<Range<usize>>,
        text: &str,
        selected_utf16: Option<&Range<usize>>,
        keep_marked: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if !text.contains(['\r', '\n']) && text != "<" {
            return false;
        }
        let rendered = self.active_tab_is_markdown()
            && self.table_source.is_none()
            && self.table_boundary_caret.is_none();
        let edits = if text == "<"
            && primary_range.is_none()
            && selected_utf16.is_none()
            && !keep_marked
            && self.marked_range.is_none()
            && self.extra_marked_ranges.is_empty()
            && self.ime_composition_snapshot.is_none()
            && !self.has_extra_cursors()
        {
            native_angle_edit(&self.buffer, &self.decorations, rendered, self.cursor)
                .map(|edit| vec![edit])
        } else {
            native_newline_edits(
                &self.buffer,
                &self.decorations,
                rendered,
                self.collect_multi_cursor_ime_targets_desc(primary_range),
                text,
                selected_utf16,
            )
        };
        let Some(edits) = edits else {
            return false;
        };
        self.pending_code_fence = None;
        if edits.len() == 1 {
            self.queue_non_markdown_line_display_transfer(&edits[0].range, &edits[0].text);
        } else {
            // A single-edit layout transfer cannot represent multiple targets.
            self.pending_line_display_transfer = None;
        }
        apply_native_edits(
            &mut self.buffer,
            &mut self.cursor,
            &mut self.extra_cursors,
            &mut self.marked_range,
            &mut self.extra_marked_ranges,
            edits,
            keep_marked,
        );
        self.reset_cursor_visual_targeting();
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
        true
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn backspace_deleting_a_line_below_a_table_moves_to_table_tail() {
        use super::super::fold::folded_delete_cursor_targets;

        for newline in ["\n", "\r\n"] {
            let source =
                ["| A | B |", "| --- | --- |", "| x | y |", "remove", "keep"].join(newline);
            let buffer = Buffer::from_text(&source);
            let rows = markdown::parse_decorations(&source);
            let line = 3;
            let start = buffer.line_start(line);
            let end = buffer.line_end(line) + 1;
            let cursor = Cursor {
                anchor: start,
                offset: end,
            };
            let target = backspace_table_tail_target(&buffer, &rows, cursor).unwrap();
            let table_tail =
                buffer.line_end(2) - usize::from(buffer.line_content(2).ends_with('\r'));
            assert_eq!(target, table_tail);

            let plans = deletion_plans(
                &buffer,
                &rows,
                true,
                vec![(cursor, true)],
                lists::DeleteAction::Backward,
            );
            let ranges = lists::deletion_ranges(&plans);
            let edits: Vec<_> = ranges.iter().cloned().map(|range| (range, 0)).collect();
            let mut edited = Buffer::from_text(&source);
            let mut resolved = lists::apply_deletions(&mut edited, &plans);
            resolved[0].0 = folded_delete_cursor_targets(&edited, &[(target, true)], &edits)[0].0;
            assert_eq!(
                edited.text(),
                ["| A | B |", "| --- | --- |", "| x | y |", "keep"].join(newline)
            );
            assert_eq!(resolved[0].0.offset, table_tail);
        }
    }

    #[test]
    fn backspace_table_tail_target_requires_the_whole_adjacent_line() {
        let source = "| A | B |\n| --- | --- |\n| x | y |\nremove\nkeep";
        let buffer = Buffer::from_text(source);
        let rows = markdown::parse_decorations(source);
        let start = buffer.line_start(3);
        assert!(
            backspace_table_tail_target(
                &buffer,
                &rows,
                Cursor {
                    anchor: start,
                    offset: start + 2,
                },
            )
            .is_none()
        );
        assert!(
            backspace_table_tail_target(
                &buffer,
                &rows,
                Cursor {
                    anchor: buffer.line_start(4),
                    offset: buffer.line_end(4),
                },
            )
            .is_none()
        );
    }

    #[test]
    fn backspace_at_line_start_after_table_has_a_navigation_target() {
        let source = "| A | B |\n| --- | --- |\n| x | y |\nparagraph";
        let buffer = Buffer::from_text(source);
        let rows = markdown::parse_decorations(source);
        let cursor = Cursor {
            anchor: buffer.line_start(3),
            offset: buffer.line_start(3),
        };
        assert_eq!(
            backspace_table_tail_target(&buffer, &rows, cursor),
            Some(buffer.line_end(2))
        );
    }

    #[test]
    fn backspace_deletes_blank_rows_one_at_a_time_before_reaching_table() {
        for newline in ["\n", "\r\n"] {
            let source =
                ["| A | B |", "| --- | --- |", "| x | y |", " 	 ", "", "keep"].join(newline);
            let buffer = Buffer::from_text(&source);
            let rows = markdown::parse_decorations(&source);
            let blank_line = 4;
            let cursor_offset = buffer.line_start(blank_line);
            let mut edited = Buffer::from_text(&source);
            let first_plan = blank_table_row_backspace(
                &edited,
                &rows,
                Cursor {
                    anchor: cursor_offset,
                    offset: cursor_offset,
                },
            )
            .unwrap();
            assert!(first_plan.boundary.is_none());
            edited.delete(first_plan.range);
            assert_eq!(
                edited.text(),
                ["| A | B |", "| --- | --- |", "| x | y |", " \t ", "keep"].join(newline)
            );
            assert_eq!(first_plan.target_offset, edited.line_start(3));
            let rows = markdown::parse_decorations(edited.text());
            let second_plan = blank_table_row_backspace(
                &edited,
                &rows,
                Cursor {
                    anchor: first_plan.target_offset,
                    offset: first_plan.target_offset,
                },
            )
            .unwrap();
            assert_eq!(
                second_plan.boundary.unwrap().side,
                super::super::TableBoundarySide::Below
            );
            edited.delete(second_plan.range);
            assert_eq!(
                edited.text(),
                ["| A | B |", "| --- | --- |", "| x | y |", "keep"].join(newline)
            );
            let table_tail = edited.line_end(2);
            assert_eq!(second_plan.target_offset, table_tail);
        }
    }

    #[test]
    fn folded_blank_backspace_keeps_other_cursors_deleting_and_maps_heading_target() {
        use super::super::fold::{folded_delete_cursor_targets, folded_delete_overrides};

        let mut buffer = Buffer::from_text("prefix\n# A\nbody\n\n# B\nxyz");
        let rows = crate::markdown::parse_decorations(buffer.text());
        let blank = buffer.line_start(3);
        let mut plans = deletion_plans(
            &buffer,
            &rows,
            false,
            vec![
                (
                    Cursor {
                        anchor: blank,
                        offset: blank,
                    },
                    true,
                ),
                (
                    Cursor {
                        anchor: 1,
                        offset: 1,
                    },
                    false,
                ),
            ],
            lists::DeleteAction::Backward,
        );
        let targets =
            folded_delete_overrides(&buffer, &[2..3], &mut plans, lists::DeleteAction::Backward)
                .unwrap();
        let ranges = lists::deletion_ranges(&plans);
        let edits: Vec<_> = ranges.into_iter().map(|range| (range, 0)).collect();
        let mut resolved = lists::apply_deletions(&mut buffer, &plans);
        for ((cursor, primary, _), target) in resolved.iter_mut().zip(targets) {
            if let Some(target) = target {
                *cursor = folded_delete_cursor_targets(&buffer, &[(target, *primary)], &edits)[0].0;
            }
        }
        assert_eq!(buffer.text(), "refix\n# A\nbody\n# B\nxyz");
        assert_eq!(
            resolved[0].0,
            Cursor {
                anchor: buffer.line_end(1),
                offset: buffer.line_end(1)
            }
        );
        assert!(resolved[0].1);
        assert_eq!(
            resolved[1].0,
            Cursor {
                anchor: 0,
                offset: 0
            }
        );
        assert!(!resolved[1].1);
    }

    #[test]
    fn folded_boundary_navigation_does_not_swallow_other_cursor_deletions() {
        use super::super::fold::{folded_delete_cursor_targets, folded_delete_overrides};

        for action in [
            lists::DeleteAction::Backward,
            lists::DeleteAction::Word { backwards: true },
        ] {
            let mut buffer = Buffer::from_text("# A\nbody\n# B\nother word");
            let rows = crate::markdown::parse_decorations(buffer.text());
            let boundary = buffer.line_start(2);
            let last = buffer.len();
            let mut plans = deletion_plans(
                &buffer,
                &rows,
                false,
                vec![
                    (
                        Cursor {
                            anchor: boundary,
                            offset: boundary,
                        },
                        true,
                    ),
                    (
                        Cursor {
                            anchor: last,
                            offset: last,
                        },
                        false,
                    ),
                ],
                action,
            );
            let ordinary = plans[1].2.range.clone();
            let targets = folded_delete_overrides(&buffer, &[1..2], &mut plans, action).unwrap();
            assert_eq!(plans[1].2.range, ordinary);
            assert!(plans[0].2.range.is_empty());
            let edits: Vec<_> = lists::deletion_ranges(&plans)
                .into_iter()
                .map(|range| (range, 0))
                .collect();
            let mut resolved = lists::apply_deletions(&mut buffer, &plans);
            resolved[0].0 =
                folded_delete_cursor_targets(&buffer, &[(targets[0].unwrap(), true)], &edits)[0].0;
            assert_eq!(
                resolved[0].0,
                Cursor {
                    anchor: buffer.line_end(0),
                    offset: buffer.line_end(0)
                }
            );
            assert_eq!(buffer.line_content(1), "body");
            assert!(buffer.len() < last);
            assert_eq!(resolved[1].0.offset, buffer.len());
        }
    }

    use super::*;
    use crate::markdown;

    fn apply_indent_targets(buffer: &mut Buffer, cursors: &mut [(Cursor, bool)], forward: bool) {
        let rows = markdown::parse_decorations(buffer.text());
        let mut edits = Vec::new();
        for (cursor, _) in cursors.iter() {
            let selection = cursor.selection_range();
            let cell = cell_range_for_selection(buffer, &rows, true, &selection).unwrap();
            edits.extend(cell_indent_plan(buffer, &rows, cell, selection, forward).unwrap());
        }
        lists::apply_prefix_edits(buffer, cursors, edits);
    }

    #[test]
    fn compact_first_fragment_tab_shift_tab_roundtrip_keeps_depth_padding_and_tabs() {
        for (marker, step) in [
            ("- ", 2),
            ("* ", 2),
            ("+ ", 2),
            ("1. ", 3),
            ("- [ ] ", 2),
            ("- [x] ", 2),
        ] {
            for lead in ["", " ", "   ", " \t", "\t", "\t  "] {
                for (container, prefix, suffix) in [
                    ("", "|", "| neighbor |"),
                    ("", "| neighbor |", "|"),
                    ("  > ", "  > | neighbor |", "|"),
                ] {
                    let original = format!(
                        "{container}| A | B |\n{container}| --- | --- |\n{prefix}{lead}{marker}先🙂{suffix}"
                    );
                    let mut buffer = Buffer::from_text(&original);
                    let at = original.find("先🙂").unwrap() + "先🙂".len();
                    let mut cursors = [(
                        Cursor {
                            offset: at,
                            anchor: at,
                        },
                        true,
                    )];
                    let initial_cell = owner(&buffer, at..at).unwrap();
                    let original_content = buffer.text_for_range(initial_cell.clone()).to_string();
                    let initial_indent = original_content.len()
                        - original_content.trim_start_matches([' ', '\t']).len();
                    let needs_padding = original.as_bytes()[initial_cell.start - 1] == b'|';
                    let mut canonical = original.clone();
                    if needs_padding {
                        canonical.insert(initial_cell.start, ' ');
                    }
                    for depth in 1..=4 {
                        apply_indent_targets(&mut buffer, &mut cursors, true);
                        let cell = owner(&buffer, cursors[0].0.selection_range()).unwrap();
                        assert_eq!(
                            buffer.text_for_range(cell),
                            format!("{}{original_content}", " ".repeat(step * depth)),
                            "{original:?} at depth {depth}"
                        );
                    }
                    for depth in (0..4).rev() {
                        apply_indent_targets(&mut buffer, &mut cursors, false);
                        let cell = owner(&buffer, cursors[0].0.selection_range()).unwrap();
                        let content = buffer.text_for_range(cell);
                        assert_eq!(
                            content,
                            format!("{}{original_content}", " ".repeat(step * depth))
                        );
                        assert_eq!(
                            content.len() - content.trim_start_matches([' ', '\t']).len(),
                            initial_indent + step * depth
                        );
                    }
                    // Padding is normalized once; ShiftTab must retain it, just
                    // as it retains formatting padding present before the edit.
                    assert_eq!(buffer.text(), canonical);
                    assert_eq!(cursors[0].0.offset, at + usize::from(needs_padding));
                    assert_eq!(cursors[0].0.anchor, cursors[0].0.offset);
                    for _ in 0..3 {
                        apply_indent_targets(&mut buffer, &mut cursors, true);
                        apply_indent_targets(&mut buffer, &mut cursors, false);
                        assert_eq!(buffer.text(), canonical);
                    }
                }
            }
        }
    }

    #[test]
    fn compact_first_fragment_padding_is_added_once_for_shared_cursor_targets() {
        let original = "| A | B |\n| --- | --- |\n|- 先🙂| neighbor |";
        let mut buffer = Buffer::from_text(original);
        let start = original.find("先🙂").unwrap();
        let mut cursors = [
            (
                Cursor {
                    offset: start,
                    anchor: start,
                },
                true,
            ),
            (
                Cursor {
                    offset: start + "先🙂".len(),
                    anchor: start + "先🙂".len(),
                },
                false,
            ),
        ];
        apply_indent_targets(&mut buffer, &mut cursors, true);
        assert_eq!(buffer.text(), original.replacen("|- ", "|   - ", 1));
        assert_eq!(cursors[0].0.offset, start + 3);
        assert_eq!(cursors[1].0.offset, start + "先🙂".len() + 3);
        apply_indent_targets(&mut buffer, &mut cursors, false);
        assert_eq!(buffer.text(), original.replacen("|- ", "| - ", 1));
        assert_eq!(cursors[0].0.offset, start + 1);
    }

    #[test]
    fn compact_first_fragment_padding_does_not_leak_into_later_fragments() {
        for first in ["lead", "- first"] {
            let original = format!("| A | B |\n| --- | --- |\n|{first}<br>- second| neighbor |");
            let mut buffer = Buffer::from_text(&original);
            let at = original.find("second").unwrap() + "second".len();
            let mut cursors = [(
                Cursor {
                    offset: at,
                    anchor: at,
                },
                true,
            )];
            for _ in 0..4 {
                apply_indent_targets(&mut buffer, &mut cursors, true);
                assert_eq!(
                    buffer.text(),
                    original.replace("<br>- second", "<br>  - second")
                );
                apply_indent_targets(&mut buffer, &mut cursors, false);
                assert_eq!(buffer.text(), original);
            }
        }
        let original = "| A | B |\n| --- | --- |\n|- first<br>- second| neighbor |";
        let mut buffer = Buffer::from_text(original);
        let mut cursors = [(
            Cursor {
                offset: original.find("- first").unwrap(),
                anchor: original.find("second").unwrap() + "second".len(),
            },
            true,
        )];
        apply_indent_targets(&mut buffer, &mut cursors, true);
        assert_eq!(
            buffer.text(),
            original.replace("|- first<br>- second", "|   - first<br>  - second")
        );
        apply_indent_targets(&mut buffer, &mut cursors, false);
        assert_eq!(buffer.text(), original.replace("|- first", "| - first"));
    }

    fn deletion_actions() -> [lists::DeleteAction; 8] {
        use lists::DeleteAction::*;
        [
            Backward,
            Forward,
            Word { backwards: true },
            Word { backwards: false },
            LineBoundary {
                to_start: true,
                kill: false,
            },
            LineBoundary {
                to_start: false,
                kill: false,
            },
            LineBoundary {
                to_start: false,
                kill: true,
            },
            Line,
        ]
    }

    #[test]
    fn deletion_plans_at_every_cell_caret_never_consume_pipes_neighbors_or_rows() {
        let content = "先🙂<BR />- two words<br>`x<br>y` $a<br>b$ \\| tail  ";
        for closing in [false, true] {
            let (buffer, start) = fixture(content, closing);
            let before = buffer.text();
            let rows = markdown::parse_decorations(before);
            let cell = owner(&buffer, start..start).unwrap();
            for local in content
                .char_indices()
                .map(|(at, _)| at)
                .chain([content.len()])
            {
                for action in deletion_actions() {
                    let at = start + local;
                    let cursor = Cursor {
                        offset: at,
                        anchor: at,
                    };
                    let (edit, table) = deletion_plan(&buffer, &rows, true, cursor, action);
                    assert!(table);
                    assert!(
                        cell.start <= edit.range.start && edit.range.end <= cell.end,
                        "{action:?} at {local}: {:?}",
                        edit.range
                    );
                    let removed = edit.range.len();
                    let mut edited = Buffer::from_text(before);
                    lists::apply_deletions(&mut edited, &[(cursor, true, edit)]);
                    assert_eq!(&edited.text()[..cell.start], &before[..cell.start]);
                    assert_eq!(&edited.text()[cell.end - removed..], &before[cell.end..]);
                    assert_eq!(edited.line_count(), buffer.line_count());
                    let parsed = markdown::parse_decorations(edited.text());
                    assert_eq!(
                        parsed[2].table_info.as_ref().map(|info| info.cells.len()),
                        Some(2),
                        "{action:?} at {local}: {:?}",
                        edited.text()
                    );
                }
            }
        }
    }

    #[test]
    fn word_delete_and_forward_delete_plan_mixed_targets_before_mutation() {
        use lists::DeleteAction;
        let text = "outside alpha\n\n| A | B |\n| --- | --- |\n| - one<br>two words | 9. 先🙂 |\n\ntrailing omega";
        let buffer = Buffer::from_text(text);
        let rows = markdown::parse_decorations(text);
        let outside = "outside alpha".len();
        let inside = text.find("words").unwrap() + "words".len();
        let edge = text.find("- one").unwrap();
        let cursors = [outside, inside, edge].map(|offset| Cursor {
            offset,
            anchor: offset,
        });
        let plans: Vec<_> = cursors
            .iter()
            .enumerate()
            .map(|(i, c)| {
                (
                    *c,
                    i == 0,
                    deletion_plan(
                        &buffer,
                        &rows,
                        true,
                        *c,
                        DeleteAction::Word { backwards: true },
                    )
                    .0,
                )
            })
            .collect();
        let mut edited = Buffer::from_text(text);
        let resolved = lists::apply_deletions(&mut edited, &plans);
        assert_eq!(
            edited.text(),
            text.replacen("outside alpha", "outside ", 1)
                .replacen("two words", "two ", 1)
        );
        assert_eq!(resolved[0].0.offset, "outside ".len());
        assert!(edited.text()[..resolved[1].0.offset].ends_with("two "));
        assert_eq!(edited.text().as_bytes()[resolved[2].0.offset], b'-');

        let br = text.find("<br>").unwrap();
        let first_cell = owner(&buffer, edge..edge).unwrap();
        let cursors = [0, br, first_cell.end].map(|offset| Cursor {
            offset,
            anchor: offset,
        });
        let plans: Vec<_> = cursors
            .iter()
            .enumerate()
            .map(|(i, c)| {
                (
                    *c,
                    i == 0,
                    deletion_plan(&buffer, &rows, true, *c, DeleteAction::Forward).0,
                )
            })
            .collect();
        let mut edited = Buffer::from_text(text);
        let resolved = lists::apply_deletions(&mut edited, &plans);
        assert_eq!(edited.text(), text[1..].replacen("<br>", "", 1));
        assert_eq!(edited.text().as_bytes()[resolved[2].0.offset], b'|');
    }

    #[test]
    fn kill_and_delete_line_in_mixed_cells_do_not_use_physical_table_rows() {
        use lists::DeleteAction;
        let text = "outside\n\n| A | B |\n| --- | --- |\n| first<BR />second<br>last | untouched |\n\nafter";
        let buffer = Buffer::from_text(text);
        let rows = markdown::parse_decorations(text);
        let br = text.find("<BR />").unwrap();
        let cursors = ["outside".len(), br].map(|offset| Cursor {
            offset,
            anchor: offset,
        });
        let plans: Vec<_> = cursors
            .iter()
            .enumerate()
            .map(|(i, c)| {
                (
                    *c,
                    i == 0,
                    deletion_plan(
                        &buffer,
                        &rows,
                        true,
                        *c,
                        DeleteAction::LineBoundary {
                            to_start: false,
                            kill: true,
                        },
                    )
                    .0,
                )
            })
            .collect();
        let ranges = lists::deletion_ranges(&plans);
        assert_eq!(
            ranges
                .iter()
                .map(|r| buffer.text_for_range(r.clone()))
                .collect::<Vec<_>>(),
            ["\n", "<BR />"]
        );
        let mut edited = Buffer::from_text(text);
        lists::apply_deletions(&mut edited, &plans);
        assert_eq!(
            edited.text(),
            text.replacen("outside\n", "outside", 1)
                .replacen("<BR />", "", 1)
        );

        let second = text.find("second").unwrap();
        let cursors = [2, second + 1, second + 4].map(|offset| Cursor {
            offset,
            anchor: offset,
        });
        let plans: Vec<_> = cursors
            .iter()
            .enumerate()
            .map(|(i, c)| {
                (
                    *c,
                    i == 0,
                    deletion_plan(&buffer, &rows, true, *c, DeleteAction::Line).0,
                )
            })
            .collect();
        let mut edited = Buffer::from_text(text);
        lists::apply_deletions(&mut edited, &plans);
        assert_eq!(
            edited.text(),
            text.replacen("outside\n", "", 1)
                .replacen("second<br>", "", 1)
        );
        assert_eq!(
            markdown::parse_decorations(edited.text())[3]
                .table_info
                .as_ref()
                .unwrap()
                .cells
                .len(),
            2
        );
    }

    #[test]
    fn protected_selections_separator_rows_and_adjacent_boundaries_are_noops() {
        let text = "before\n| A | B |\n| --- | --- |\n| one<br>two | neighbor |\nafter";
        let buffer = Buffer::from_text(text);
        let rows = markdown::parse_decorations(text);
        let from = text.find("one").unwrap();
        let to = text.find("neighbor").unwrap() + 3;
        for cursor in [
            Cursor {
                offset: from,
                anchor: to,
            },
            Cursor {
                offset: to,
                anchor: from,
            },
            Cursor {
                offset: buffer.line_start(2) + 2,
                anchor: buffer.line_start(2) + 2,
            },
        ] {
            for action in deletion_actions() {
                let (edit, table) = deletion_plan(&buffer, &rows, true, cursor, action);
                assert!(table);
                assert!(edit.range.is_empty(), "{action:?}: {:?}", edit.range);
                let mut edited = Buffer::from_text(text);
                let resolved = lists::apply_deletions(&mut edited, &[(cursor, true, edit)]);
                assert_eq!(edited.text(), text);
                assert_eq!(resolved[0].0, cursor);
            }
        }
        for (at, action) in [
            (buffer.line_end(0), lists::DeleteAction::Forward),
            (
                buffer.line_start(4),
                lists::DeleteAction::Word { backwards: true },
            ),
        ] {
            assert!(
                deletion_plan(
                    &buffer,
                    &rows,
                    true,
                    Cursor {
                        offset: at,
                        anchor: at
                    },
                    action
                )
                .0
                .range
                .is_empty()
            );
        }
        // Deleting an entire outside line may safely expose a table header at
        // the beginning of a line; it must not be confused with joining rows.
        let edit = deletion_plan(
            &buffer,
            &rows,
            true,
            Cursor {
                offset: 2,
                anchor: 2,
            },
            lists::DeleteAction::Line,
        )
        .0;
        assert_eq!(edit.range, 0.."before\n".len());
    }

    #[test]
    fn explicit_block_selections_delete_one_or_more_tables_in_both_directions() {
        let text = "before\n| A | B |\n| --- | --- |\n| one | two |\nbetween\n| C | D |\n| --- | --- |\n| three | four |\nafter";
        for range in [
            0..text.len(),
            text.find("| A").unwrap()..text.find("between").unwrap(),
            text.find("one").unwrap()..text.find("four").unwrap() + 4,
        ] {
            for reversed in [false, true] {
                let cursor = Cursor {
                    anchor: if reversed { range.end } else { range.start },
                    offset: if reversed { range.start } else { range.end },
                };
                for action in deletion_actions()
                    .into_iter()
                    .filter(|action| *action != lists::DeleteAction::Line)
                {
                    let mut buffer = Buffer::from_text(text);
                    let rows = markdown::parse_decorations(text);
                    let plans = deletion_plans(&buffer, &rows, true, vec![(cursor, true)], action);
                    assert_eq!(
                        lists::deletion_ranges(&plans),
                        vec![range.clone()],
                        "{action:?}"
                    );
                    let resolved = lists::apply_deletions(&mut buffer, &plans);
                    assert_eq!(
                        buffer.text(),
                        format!("{}{}", &text[..range.start], &text[range.end..])
                    );
                    assert_eq!(
                        resolved[0].0,
                        Cursor {
                            offset: range.start,
                            anchor: range.start
                        }
                    );
                }
            }
        }
    }

    #[test]
    fn deleting_complete_table_blocks_leaves_blank_rows_and_preserves_newlines() {
        for newline in ["\n", "\r\n"] {
            let text = [
                "before",
                "| A | B |",
                "| --- | --- |",
                "| one | two |",
                "between",
                "| C | D |",
                "| --- | --- |",
                "| three | four |",
                "after",
            ]
            .join(newline);
            let start = text.find("| A |").unwrap();
            let end = text.find("between").unwrap();
            let buffer = Buffer::from_text(&text);
            let rows = markdown::parse_decorations(&text);
            let plans = deletion_plans(
                &buffer,
                &rows,
                true,
                vec![(
                    Cursor {
                        anchor: start,
                        offset: end,
                    },
                    true,
                )],
                lists::DeleteAction::Backward,
            );
            let replacements = lists::deletion_ranges(&plans)
                .into_iter()
                .map(|range| {
                    let text = deleted_table_rows_as_blank_lines(&buffer, &rows, &range);
                    (range, text)
                })
                .collect::<Vec<_>>();
            assert_eq!(replacements[0].1, newline.repeat(3));
            let mut edited = Buffer::from_text(&text);
            let resolved =
                lists::apply_deletions_with_replacements(&mut edited, &plans, &replacements);
            assert_eq!(
                edited.text(),
                [
                    "before",
                    "",
                    "",
                    "",
                    "between",
                    "| C | D |",
                    "| --- | --- |",
                    "| three | four |",
                    "after"
                ]
                .join(newline)
            );
            assert_eq!(resolved[0].0.offset, start);
            assert!(
                markdown::parse_decorations(edited.text())
                    .iter()
                    .all(|row| row.table_info.is_none()
                        || row
                            .table_info
                            .as_ref()
                            .is_some_and(|info| info.table_start_line == 5))
            );

            let all_range = 0..buffer.len();
            let all_plan = deletion_plans(
                &buffer,
                &rows,
                true,
                vec![(
                    Cursor {
                        anchor: 0,
                        offset: buffer.len(),
                    },
                    true,
                )],
                lists::DeleteAction::Forward,
            );
            let all_replacements = lists::deletion_ranges(&all_plan)
                .into_iter()
                .map(|range| {
                    let text = deleted_table_rows_as_blank_lines(&buffer, &rows, &range);
                    (range, text)
                })
                .collect::<Vec<_>>();
            assert_eq!(all_replacements[0].1, newline.repeat(6));
            let mut edited = Buffer::from_text(&text);
            lists::apply_deletions_with_replacements(&mut edited, &all_plan, &all_replacements);
            assert_eq!(edited.text(), newline.repeat(6));
            let _ = all_range;
        }
    }

    #[test]
    fn partial_table_selection_does_not_expand_to_the_whole_block() {
        let text = "| A | B |\n| --- | --- |\n| one | two |\nafter";
        let buffer = Buffer::from_text(text);
        let rows = markdown::parse_decorations(text);
        let start = text.find("one").unwrap();
        let range = start..start + 3;
        assert!(deleted_table_rows_as_blank_lines(&buffer, &rows, &range).is_empty());
    }

    #[test]
    fn deletion_keeps_source_mode_yaml_and_ordinary_word_policies() {
        let (buffer, start) = fixture("- text", true);
        let rows = markdown::parse_decorations(buffer.text());
        let pipe = buffer.line_end(2) - 1;
        let (edit, table) = deletion_plan(
            &buffer,
            &rows,
            false,
            Cursor {
                offset: pipe,
                anchor: pipe,
            },
            lists::DeleteAction::Forward,
        );
        assert!(!table);
        assert_eq!(edit.range, pipe..pipe + 1);
        assert!(start < pipe);
        let text = "---\ntitle: alpha beta\n---\n";
        let buffer = Buffer::from_text(text);
        let rows = markdown::parse_decorations(text);
        let at = buffer.line_end(1);
        let (edit, table) = deletion_plan(
            &buffer,
            &rows,
            true,
            Cursor {
                offset: at,
                anchor: at,
            },
            lists::DeleteAction::Word { backwards: true },
        );
        assert!(!table);
        assert_eq!(buffer.text_for_range(edit.range), "beta");
    }

    #[test]
    fn combined_deletion_plans_cannot_pair_code_delimiters_across_neighbor_cells() {
        let text = "outside\n\n| A | B |\n| --- | --- |\n| ```x | `z` |";
        let buffer = Buffer::from_text(text);
        let rows = markdown::parse_decorations(text);
        let left = text.find("```x").unwrap();
        let right = text.find('z').unwrap();
        let targets: Vec<_> = [0, left, right]
            .into_iter()
            .enumerate()
            .map(|(index, offset)| {
                (
                    Cursor {
                        offset,
                        anchor: offset,
                    },
                    index == 0,
                )
            })
            .collect();
        // Each cell-local edit is safe by itself, but together they create a
        // matched code span that would swallow the inter-cell pipe.
        for (cursor, _) in &targets[1..] {
            assert!(
                !deletion_plan(&buffer, &rows, true, *cursor, lists::DeleteAction::Forward)
                    .0
                    .range
                    .is_empty()
            );
        }
        let plans = deletion_plans(&buffer, &rows, true, targets, lists::DeleteAction::Forward);
        assert!(!plans[0].2.range.is_empty());
        assert!(plans[1].2.range.is_empty());
        assert!(plans[2].2.range.is_empty());
        let mut edited = Buffer::from_text(text);
        let resolved = lists::apply_deletions(&mut edited, &plans);
        assert_eq!(edited.text(), &text[1..]);
        assert_eq!(resolved[1].0.offset, left - 1);
        assert_eq!(resolved[2].0.offset, right - 1);
    }

    #[test]
    fn literal_pipes_cannot_be_exposed_by_deleting_an_escape_or_code_delimiter() {
        for content in [r"a\|b", "`a|b`"] {
            let (buffer, start) = fixture(content, true);
            let rows = markdown::parse_decorations(buffer.text());
            let at = start + if content.starts_with('`') { 0 } else { 1 };
            let cursor = Cursor {
                offset: at,
                anchor: at,
            };
            let plans = deletion_plans(
                &buffer,
                &rows,
                true,
                vec![(cursor, true)],
                lists::DeleteAction::Forward,
            );
            assert!(plans[0].2.range.is_empty(), "{content:?}");
            let mut edited = Buffer::from_text(buffer.text());
            lists::apply_deletions(&mut edited, &plans);
            assert_eq!(edited.text(), buffer.text());
        }
    }

    #[test]
    fn backspace_validates_escaped_pipes_code_delimiters_and_code_run_deletion() {
        for (content, local) in [
            (r"a\|b", 2),
            ("`a|b`", 1),
            ("`a|b`", 5),
            ("``a|b``", 1),
            ("``a|b``", 6),
        ] {
            let (mut buffer, start) = fixture(content, true);
            let before = buffer.text().to_string();
            let rows = markdown::parse_decorations(&before);
            let cursor = Cursor {
                offset: start + local,
                anchor: start + local,
            };
            let plans = deletion_plans(
                &buffer,
                &rows,
                true,
                vec![(cursor, true)],
                lists::DeleteAction::Backward,
            );
            assert!(plans[0].2.range.is_empty(), "{content:?} at {local}");
            let resolved = lists::apply_deletions(&mut buffer, &plans);
            assert_eq!(buffer.text(), before);
            assert_eq!(resolved[0].0, cursor);
        }
    }

    #[test]
    fn paired_backspace_cannot_join_code_runs_across_a_neighbor_pipe() {
        let text = "| A | B |\n| --- | --- |\n| `()`x | ``z`` |";
        let mut buffer = Buffer::from_text(text);
        let rows = markdown::parse_decorations(text);
        let at = text.find("()").unwrap() + 1;
        let cursor = Cursor {
            offset: at,
            anchor: at,
        };
        let cell = cell_range_for_selection(&buffer, &rows, true, &(at..at)).unwrap();
        assert_eq!(
            lists::paired_backspace_range(
                buffer.text_for_range(cell.clone()),
                Cursor {
                    offset: at - cell.start,
                    anchor: at - cell.start,
                }
            ),
            Some(at - cell.start - 1..at - cell.start + 1)
        );
        let plans = deletion_plans(
            &buffer,
            &rows,
            true,
            vec![(cursor, true)],
            lists::DeleteAction::Backward,
        );
        assert!(plans[0].2.range.is_empty());
        let resolved = lists::apply_deletions(&mut buffer, &plans);
        assert_eq!(buffer.text(), text);
        assert_eq!(resolved[0].0, cursor);
    }

    #[test]
    fn backspace_keeps_list_marker_pair_and_atomic_break_semantics_after_validation() {
        for (content, local, expected) in [
            ("- item", 2, "item"),
            ("  - item", 4, "  item"),
            ("3. item", 3, "item"),
            ("- [ ] item", 6, "item"),
            ("- [x] item", 6, "item"),
            ("()", 1, ""),
            ("[[]]", 2, ""),
            ("“”", "“".len(), ""),
            // Backticks are not ordinary editor auto-pairs.
            ("``", 1, "`"),
            ("first<BR />last", "first<BR />".len(), "firstlast"),
            ("first<BR />last", "first<BR".len(), "firstlast"),
            (r"literal\<br>", r"literal\<br>".len(), r"literal\<br"),
            ("`code<br>`", "`code<br>".len(), "`code<br`"),
            ("$a<br>$", "$a<br>".len(), "$a<br$"),
        ] {
            let (mut buffer, start) = fixture(content, true);
            let before = buffer.text().to_string();
            let rows = markdown::parse_decorations(&before);
            let cursor = Cursor {
                offset: start + local,
                anchor: start + local,
            };
            let plans = deletion_plans(
                &buffer,
                &rows,
                true,
                vec![(cursor, true)],
                lists::DeleteAction::Backward,
            );
            assert!(!plans[0].2.range.is_empty(), "{content:?}");
            let resolved = lists::apply_deletions(&mut buffer, &plans);
            assert_eq!(
                buffer.text(),
                format!("{}{expected}|", &before[..start]),
                "{content:?}"
            );
            assert_eq!(resolved[0].0.offset, plans[0].2.range.start);
            assert_eq!(resolved[0].0.anchor, resolved[0].0.offset);
        }
    }

    #[test]
    fn backspace_protected_selections_keep_both_endpoints_with_mixed_cursors() {
        let text = "outside\n\n| A | B |\n| --- | --- |\n| a\\|b | neighbor |\nafter";
        let buffer = Buffer::from_text(text);
        let rows = markdown::parse_decorations(text);
        let slash = text.find('\\').unwrap();
        let neighbor = text.find("neighbor").unwrap();
        let edge = text.find("after").unwrap();
        for range in [slash..slash + 1, slash..neighbor + 2] {
            for reversed in [false, true] {
                let selection = if reversed {
                    Cursor {
                        offset: range.start,
                        anchor: range.end,
                    }
                } else {
                    Cursor {
                        offset: range.end,
                        anchor: range.start,
                    }
                };
                let ordinary = Cursor {
                    offset: 1,
                    anchor: 1,
                };
                let adjacent = Cursor {
                    offset: edge,
                    anchor: edge,
                };
                let plans = deletion_plans(
                    &buffer,
                    &rows,
                    true,
                    vec![(selection, true), (ordinary, false), (adjacent, false)],
                    lists::DeleteAction::Backward,
                );
                assert!(plans[0].2.range.is_empty());
                assert!(!plans[1].2.range.is_empty());
                assert!(plans[2].2.range.is_empty());
                let mut edited = Buffer::from_text(text);
                let resolved = lists::apply_deletions(&mut edited, &plans);
                assert_eq!(edited.text(), &text[1..]);
                assert_eq!(
                    resolved[0].0,
                    Cursor {
                        offset: selection.offset - 1,
                        anchor: selection.anchor - 1,
                    }
                );
                assert_eq!(
                    resolved[2].0,
                    Cursor {
                        offset: edge - 1,
                        anchor: edge - 1
                    }
                );
            }
        }
    }

    #[test]
    fn backspace_overlapping_duplicate_selections_and_pairs_delete_the_union_once() {
        let (buffer, start) = fixture("one()🙂two<br>last", true);
        let rows = markdown::parse_decorations(buffer.text());
        let selection = Cursor {
            offset: start + 5 + "🙂".len(),
            anchor: start + 2,
        };
        let targets = vec![
            (selection, true),
            (
                Cursor {
                    offset: start + 2,
                    anchor: selection.offset,
                },
                false,
            ),
            (
                Cursor {
                    offset: start + 4,
                    anchor: start + 4,
                },
                false,
            ),
            (
                Cursor {
                    offset: selection.offset,
                    anchor: selection.offset,
                },
                false,
            ),
        ];
        let plans = deletion_plans(&buffer, &rows, true, targets, lists::DeleteAction::Backward);
        assert_eq!(
            lists::deletion_ranges(&plans),
            [start + 2..selection.offset]
        );
        let mut edited = Buffer::from_text(buffer.text());
        let resolved = lists::apply_deletions(&mut edited, &plans);
        assert_eq!(edited.text(), buffer.text().replace("e()🙂", ""));
        assert!(resolved.iter().all(|(c, _, _)| *c
            == Cursor {
                offset: start + 2,
                anchor: start + 2,
            }));
    }

    #[test]
    fn combined_backspace_cannot_pair_code_delimiters_across_cells() {
        let text = "outside\n\n| A | B |\n| --- | --- |\n| ```x | `z` |";
        let buffer = Buffer::from_text(text);
        let rows = markdown::parse_decorations(text);
        let left = text.find("```x").unwrap() + 1;
        let right = text.find('z').unwrap() + 1;
        let targets: Vec<_> = [1, left, right]
            .into_iter()
            .enumerate()
            .map(|(i, at)| {
                (
                    Cursor {
                        offset: at,
                        anchor: at,
                    },
                    i == 0,
                )
            })
            .collect();
        for (cursor, _) in &targets[1..] {
            assert!(
                !deletion_plan(&buffer, &rows, true, *cursor, lists::DeleteAction::Backward)
                    .0
                    .range
                    .is_empty()
            );
        }
        let plans = deletion_plans(&buffer, &rows, true, targets, lists::DeleteAction::Backward);
        assert!(!plans[0].2.range.is_empty());
        assert!(plans[1..].iter().all(|(_, _, e)| e.range.is_empty()));
        let mut edited = Buffer::from_text(text);
        let resolved = lists::apply_deletions(&mut edited, &plans);
        assert_eq!(edited.text(), &text[1..]);
        assert_eq!(resolved[1].0.offset, left - 1);
        assert_eq!(resolved[2].0.offset, right - 1);
    }

    #[test]
    fn open_line_last_fragment_and_lists_insert_only_a_break_without_advancing() {
        for content in [
            "",
            "last",
            "first<br>",
            "first<br>last  ",
            "- item",
            "- ",
            "3. item",
            "3. ",
            "- [ ] item",
            "- [x] ",
            "- [X] item",
        ] {
            for closing in [false, true] {
                // An empty pipe-less tail is the preceding cell's trailing
                // delimiter, not a second cell with editable source ownership.
                if content.is_empty() && !closing {
                    continue;
                }
                let (mut buffer, start) = fixture(content, closing);
                let at = start + content.len();
                let cursor = Cursor {
                    offset: at,
                    anchor: at,
                };
                let before = buffer.text().to_string();
                for count in 1..=3 {
                    let rows = markdown::parse_decorations(buffer.text());
                    let edit = open_line_plan(&buffer, &rows, true, cursor).unwrap();
                    assert_eq!(edit.text, "<br>");
                    assert_eq!(edit.caret, 0);
                    assert_eq!(edit.normalize, None);
                    let resolved = lists::apply(&mut buffer, &[(edit, true)]).unwrap();
                    // The handler passes only this plan, irrespective of extras.
                    assert_eq!(resolved.len(), 1);
                    assert_eq!(resolved[0].0, cursor);
                    assert!(resolved[0].1);
                    assert_eq!(
                        buffer.text(),
                        format!("{}{}{}", &before[..at], "<br>".repeat(count), &before[at..])
                    );
                    assert_eq!(buffer.line_count(), 3);
                    assert_eq!(
                        markdown::parse_decorations(buffer.text())[2]
                            .table_info
                            .as_ref()
                            .unwrap()
                            .cells
                            .len(),
                        2
                    );
                }
            }
        }
    }

    #[test]
    fn open_line_at_every_cell_caret_preserves_pipes_neighbors_and_physical_rows() {
        let content = "先🙂<BR />- two words<br>`x<br>y` $a<br>b$ \\| tail  ";
        for closing in [false, true] {
            let (buffer, start) = fixture(content, closing);
            let rows = markdown::parse_decorations(buffer.text());
            let cell = owner(&buffer, start..start).unwrap();
            for local in content
                .char_indices()
                .map(|(at, _)| at)
                .chain([content.len()])
            {
                let at = start + local;
                let cursor = Cursor {
                    offset: at,
                    anchor: at,
                };
                let Some(edit) = open_line_plan(&buffer, &rows, true, cursor) else {
                    continue;
                };
                assert_eq!(edit.text, "<br>");
                assert!(cell.start <= edit.range.start && edit.range.end <= cell.end);
                let delta = edit.text.len() as isize - edit.range.len() as isize;
                let expected_caret = edit.range.start;
                let mut edited = Buffer::from_text(buffer.text());
                let resolved = lists::apply(&mut edited, &[(edit, true)]).unwrap();
                assert_eq!(
                    resolved[0].0,
                    Cursor {
                        offset: expected_caret,
                        anchor: expected_caret
                    }
                );
                assert_eq!(&edited.text()[..cell.start], &buffer.text()[..cell.start]);
                assert_eq!(
                    &edited.text()[cell.end.saturating_add_signed(delta)..],
                    &buffer.text()[cell.end..]
                );
                assert_eq!(edited.line_count(), buffer.line_count());
                assert_eq!(
                    markdown::parse_decorations(edited.text())[2]
                        .table_info
                        .as_ref()
                        .unwrap()
                        .cells
                        .len(),
                    2,
                    "at {local}"
                );
            }
        }
    }

    #[test]
    fn open_line_selection_is_cell_local_atomic_and_keeps_caret_at_start() {
        let (buffer, start) = fixture("one<BR />two<br>last", true);
        for local in [1..3, 4..7, 2..11] {
            for reversed in [false, true] {
                let range = start + local.start..start + local.end;
                let cursor = if reversed {
                    Cursor {
                        offset: range.start,
                        anchor: range.end,
                    }
                } else {
                    Cursor {
                        offset: range.end,
                        anchor: range.start,
                    }
                };
                let rows = markdown::parse_decorations(buffer.text());
                let edit = open_line_plan(&buffer, &rows, true, cursor).unwrap();
                assert!(edit.range.start <= range.start && edit.range.end >= range.end);
                let mut edited = Buffer::from_text(buffer.text());
                let resolved = lists::apply(&mut edited, &[(edit.clone(), true)]).unwrap();
                assert_eq!(
                    edited.text(),
                    format!(
                        "{}<br>{}",
                        &buffer.text()[..edit.range.start],
                        &buffer.text()[edit.range.end..]
                    )
                );
                assert_eq!(
                    resolved[0].0,
                    Cursor {
                        offset: edit.range.start,
                        anchor: edit.range.start
                    }
                );
            }
        }
    }

    #[test]
    fn open_line_protects_cross_cell_selection_separator_and_pipe_escapes() {
        let text = "| A | B |\n| --- | --- |\n| a\\|b | neighbor |";
        let buffer = Buffer::from_text(text);
        let rows = markdown::parse_decorations(text);
        let slash = text.find('\\').unwrap();
        let neighbor = text.find("neighbor").unwrap();
        for range in [
            slash..slash + 1,
            slash..neighbor + 2,
            buffer.line_start(1) + 2..buffer.line_start(1) + 2,
        ] {
            for (offset, anchor) in [(range.start, range.end), (range.end, range.start)] {
                let cursor = Cursor { offset, anchor };
                assert!(open_line_plan(&buffer, &rows, true, cursor).is_none());
                // A refused plan never enters apply(), so anchors/extras survive.
                assert_eq!(buffer.text(), text);
            }
        }
        // Inserting between the two ticks must not break a code delimiter and
        // expose the pipe inside its formerly opaque code span.
        let (buffer, start) = fixture("``a|b``", true);
        let rows = markdown::parse_decorations(buffer.text());
        let cursor = Cursor {
            offset: start + 1,
            anchor: start + 1,
        };
        assert!(open_line_plan(&buffer, &rows, true, cursor).is_none());
    }

    #[test]
    fn open_line_uses_accepted_breaks_only_and_leaves_source_and_yaml_physical() {
        for (content, local, expected) in [
            ("first<BR />last", 8, "first<br>last"),
            (r"literal\<br>", 10, r"literal\<b<br>r>"),
            ("`code<br>tag`", 7, "`code<b<br>r>tag`"),
            ("$a<br>b$", 4, "$a<b<br>r>b$"),
        ] {
            let (mut buffer, start) = fixture(content, true);
            let before = buffer.text().to_string();
            let rows = markdown::parse_decorations(&before);
            let edit = open_line_plan(
                &buffer,
                &rows,
                true,
                Cursor {
                    offset: start + local,
                    anchor: start + local,
                },
            )
            .unwrap();
            lists::apply(&mut buffer, &[(edit, true)]).unwrap();
            assert_eq!(buffer.text(), format!("{}{expected}|", &before[..start]));
        }
        for (text, rendered, at) in [
            ("| A | B |\n| --- | --- |\n| one | two |", false, 26),
            ("---\ntitle: alpha beta\n---\n", true, 12),
            ("ordinary text", true, 5),
        ] {
            let mut buffer = Buffer::from_text(text);
            let rows = markdown::parse_decorations(text);
            let edit = open_line_plan(
                &buffer,
                &rows,
                rendered,
                Cursor {
                    offset: at,
                    anchor: at,
                },
            )
            .unwrap();
            assert_eq!(edit.text, "\n");
            let resolved = lists::apply(&mut buffer, &[(edit, true)]).unwrap();
            assert_eq!(buffer.text(), format!("{}\n{}", &text[..at], &text[at..]));
            assert_eq!(
                resolved[0].0,
                Cursor {
                    offset: at,
                    anchor: at
                }
            );
        }
    }

    #[test]
    fn native_angle_pairing_preserves_cell_ownership_and_caret_inside_pair() {
        for content in [
            "first<br>",
            "first<BR />  ",
            "word",
            r"literal\<br>",
            "`code<br>tag`",
            "$a<br>b$",
        ] {
            let (mut buffer, start) = fixture(content, true);
            let rows = markdown::parse_decorations(buffer.text());
            let at = start + content.len();
            let cursor = Cursor {
                offset: at,
                anchor: at,
            };
            let edit = native_angle_edit(&buffer, &rows, true, cursor).unwrap();
            let paired = !content.starts_with("first");
            assert_eq!(edit.text, if paired { "<>" } else { "<" }, "{content:?}");
            assert!(native_angle_edit(&buffer, &rows, false, cursor).is_none());
            assert!(
                native_angle_edit(
                    &buffer,
                    &rows,
                    true,
                    Cursor {
                        anchor: start,
                        ..cursor
                    }
                )
                .is_none()
            );
            let before = buffer.text().to_string();
            let mut current = cursor;
            apply_native_edits(
                &mut buffer,
                &mut current,
                &mut Vec::new(),
                &mut None,
                &mut Vec::new(),
                vec![edit],
                false,
            );
            assert_eq!(current.offset, at + 1);
            assert_eq!(&buffer.text()[..at], &before[..at]);
            assert!(buffer.text().ends_with(if paired { "<>|" } else { "<|" }));
        }
    }

    #[test]
    fn enter_actions_use_owned_fragments_and_never_mutate_neighbor_cells_or_rows() {
        for closing in [false, true] {
            for (source, expected) in [
                ("- 先🙂", "- 先🙂<br>- "),
                (
                    "lead<BR />  + [x] done",
                    "lead<BR />  + [x] done<br>  + [ ] ",
                ),
                ("lead<br>   9. ", "lead<br>9. "),
                ("lead<br>- ", "lead<br><br>"),
            ] {
                let (mut buffer, start) = fixture(source, closing);
                let before = buffer.text().to_string();
                let rows = markdown::parse_decorations(buffer.text());
                let caret = start + source.len();
                let plan = cell_enter_plan(
                    &buffer,
                    &rows,
                    true,
                    Cursor {
                        offset: caret,
                        anchor: caret,
                    },
                    false,
                )
                .unwrap();
                assert!(start <= plan.range.start && plan.range.end <= caret);
                let result = lists::apply(&mut buffer, &[(plan, true)]).unwrap();
                assert_eq!(
                    buffer.text(),
                    format!("{}{}{}", &before[..start], expected, &before[caret..])
                );
                assert_eq!(buffer.line_count(), 3);
                assert_eq!(
                    markdown::parse_decorations(buffer.text())[2]
                        .table_info
                        .as_ref()
                        .unwrap()
                        .cells
                        .len(),
                    2
                );
                assert_eq!(result[0].0.offset, start + expected.len());
            }
        }
    }

    #[test]
    fn action_ownership_rejects_source_yaml_separator_and_cross_cell_ranges() {
        let (buffer, start) = fixture("- text", true);
        let rows = markdown::parse_decorations(buffer.text());
        let cursor = Cursor {
            offset: start + 6,
            anchor: start + 6,
        };
        assert!(cell_enter_plan(&buffer, &rows, false, cursor, false).is_none());
        assert!(
            cell_enter_plan(
                &buffer,
                &rows,
                true,
                Cursor {
                    anchor: start - 3,
                    ..cursor
                },
                false
            )
            .is_none()
        );
        let separator = buffer.line_start(1) + 2;
        assert!(
            cell_enter_plan(
                &buffer,
                &rows,
                true,
                Cursor {
                    offset: separator,
                    anchor: separator
                },
                false
            )
            .is_none()
        );
        let buffer = Buffer::from_text("---\ntitle: - text\n---\n");
        let rows = markdown::parse_decorations(buffer.text());
        let caret = buffer.line_end(1);
        assert!(
            cell_enter_plan(
                &buffer,
                &rows,
                true,
                Cursor {
                    offset: caret,
                    anchor: caret
                },
                false
            )
            .is_none()
        );
    }

    #[test]
    fn action_batch_plans_multiple_cells_before_the_shared_row_changes() {
        let text = "| A | B |\n| --- | --- |\n| - 先🙂 | 9. é |";
        let mut buffer = Buffer::from_text(text);
        let rows = markdown::parse_decorations(text);
        let carets = [
            text.find("先🙂").unwrap() + "先🙂".len(),
            text.find("é").unwrap() + "é".len(),
        ];
        let plans = carets
            .iter()
            .enumerate()
            .map(|(index, offset)| {
                (
                    cell_enter_plan(
                        &buffer,
                        &rows,
                        true,
                        Cursor {
                            offset: *offset,
                            anchor: *offset,
                        },
                        false,
                    )
                    .unwrap(),
                    index == 0,
                )
            })
            .collect::<Vec<_>>();
        let cursors = lists::apply(&mut buffer, &plans).unwrap();
        assert_eq!(
            buffer.text(),
            "| A | B |\n| --- | --- |\n| - 先🙂<br>-  | 9. é<br>10.  |"
        );
        assert!(buffer.text()[..cursors[0].0.offset].ends_with("<br>- "));
        assert!(buffer.text()[..cursors[1].0.offset].ends_with("<br>10. "));
    }

    fn fixture(content: &str, closing_pipe: bool) -> (Buffer, usize) {
        let prefix = "| 隣🙂 | ";
        let text = format!(
            "| Neighbor | Input |\n| --- | --- |\n{prefix}{content}{}",
            if closing_pipe { "|" } else { "" },
        );
        let buffer = Buffer::from_text(&text);
        let start = buffer.line_start(2) + prefix.len();
        (buffer, start)
    }

    fn owner(buffer: &Buffer, selection: Range<usize>) -> Option<Range<usize>> {
        cell_range_for_selection(
            buffer,
            &markdown::parse_decorations(buffer.text()),
            true,
            &selection,
        )
    }

    fn soft_break(buffer: &mut Buffer, cursor: &mut Cursor) {
        let rows = markdown::parse_decorations(buffer.text());
        let plan = cell_enter_plan(buffer, &rows, true, *cursor, true).unwrap();
        *cursor = lists::apply(buffer, &[(plan, true)]).unwrap()[0].0;
        let parsed = markdown::parse_decorations(buffer.text());
        let info = parsed[2]
            .table_info
            .as_ref()
            .expect("Enter broke the table");
        assert_eq!(info.cells.len(), 2);
        assert_eq!(info.table_end_line, 3);
        assert_eq!(buffer.line_count(), 3);
    }

    #[test]
    fn last_fragment_end_and_real_trailing_padding_share_renderer_ownership() {
        for content in [
            "first<br>last",
            "first<br>last  \t",
            "first<br>",
            "first<br>  \t",
            "first<br>- ",
            "first<br>* ",
            "first<br>+ ",
            "first<br>12. ",
            "first<br>- [ ] ",
            "first<br>  - [x] \t",
            "先🙂<br>最後é  ",
            "a\\|b<br>last ",
        ] {
            for closing_pipe in [false, true] {
                let (buffer, start) = fixture(content, closing_pipe);
                let parsed = markdown::parse_decorations(buffer.text());
                let info = parsed[2].table_info.as_ref().unwrap();
                let (rendered, local_range) = super::super::table_cell_render::content_with_range(
                    info,
                    buffer.line_content(2),
                    1,
                );
                assert_eq!(rendered, content);
                let decoration = super::super::helpers::table_cell_decoration(
                    &parsed[2],
                    &rendered,
                    &local_range,
                );
                let last = decoration.table_cell_fragments.last().unwrap();
                assert_eq!(last.range.end, content.len());
                for offset in content
                    .char_indices()
                    .map(|(i, _)| i)
                    .chain([content.len()])
                {
                    assert_eq!(
                        owner(&buffer, start + offset..start + offset),
                        Some(start..start + content.len()),
                        "{content:?} at {offset}"
                    );
                }
            }
        }
    }

    #[test]
    fn repeated_soft_break_at_last_blank_or_completed_list_fragment_stays_in_cell() {
        for content in [
            "first<br>last",
            "first<br>",
            "first<br>  ",
            "first<br>- ",
            "first<br>1. ",
            "first<br>- [ ] ",
            "先🙂<br>末尾  ",
        ] {
            for closing_pipe in [false, true] {
                let (mut buffer, start) = fixture(content, closing_pipe);
                let mut cursor = Cursor {
                    offset: start + content.len(),
                    anchor: start + content.len(),
                };
                let original_row = buffer.line_content(2).to_owned();
                for count in 1..=4 {
                    soft_break(&mut buffer, &mut cursor);
                    let mut expected = original_row.clone();
                    expected.insert_str(
                        start - buffer.line_start(2) + content.len(),
                        &"<br>".repeat(count),
                    );
                    assert_eq!(buffer.line_content(2), expected);
                    assert_eq!(cursor.offset, start + content.len() + count * 4);
                    assert_eq!(cursor.anchor, cursor.offset);
                    assert!(owner(&buffer, cursor.selection_range()).is_some());
                }
            }
        }
    }

    #[test]
    fn pipe_is_an_insertion_boundary_not_selected_cell_content() {
        let (mut buffer, start) = fixture("first<br>last  \t", true);
        let pipe = buffer.line_end(2) - 1;
        let cell = owner(&buffer, pipe..pipe).unwrap();
        assert_eq!(cell, start..pipe);
        assert_eq!(buffer.text().as_bytes()[pipe], b'|');
        assert!(owner(&buffer, pipe..pipe + 1).is_none());
        assert!(owner(&buffer, pipe + 1..pipe + 1).is_none());
        assert!(owner(&buffer, start..pipe + 1).is_none());
        assert!(owner(&buffer, start - 1..pipe).is_none());
        let mut cursor = Cursor {
            anchor: pipe,
            offset: pipe,
        };
        soft_break(&mut buffer, &mut cursor);
        assert!(buffer.line_content(2).ends_with("last  \t<br>|"));
    }

    #[test]
    fn selections_in_both_directions_can_replace_trailing_spaces_and_fragments() {
        let content = "先🙂<br>last  \t";
        for reversed in [false, true] {
            for from in ["先🙂".len() + 4, content.len() - 2] {
                let (mut buffer, start) = fixture(content, true);
                let (anchor, offset) = (start + from, start + content.len());
                let mut cursor = if reversed {
                    Cursor {
                        anchor: offset,
                        offset: anchor,
                    }
                } else {
                    Cursor { anchor, offset }
                };
                soft_break(&mut buffer, &mut cursor);
                assert!(
                    buffer
                        .line_content(2)
                        .ends_with(&format!("{}<br>|", &content[..from]))
                );
                assert_eq!(cursor.offset, start + from + 4);
            }
        }
        let (mut buffer, start) = fixture(content, true);
        let mut cursor = Cursor {
            anchor: start,
            offset: start + content.len(),
        };
        soft_break(&mut buffer, &mut cursor);
        assert_eq!(buffer.line_content(2), "| 隣🙂 | <br>|");
    }

    #[test]
    fn pipe_less_final_cell_owns_eof_padding_but_not_the_next_physical_line() {
        let (mut buffer, start) = fixture("first<br>末尾  \t", false);
        let end = buffer.len();
        assert!(owner(&buffer, end..end).is_some());
        assert!(owner(&buffer, start..end).is_some());
        buffer.insert(end, "\n\nparagraph");
        assert!(owner(&buffer, end..end).is_some());
        assert!(owner(&buffer, start..end + 1).is_none());
        assert!(owner(&buffer, end + 1..end + 1).is_none());
        assert!(owner(&buffer, buffer.len()..buffer.len()).is_none());
    }

    #[test]
    fn other_cells_separators_source_modes_front_matter_and_invalid_ranges_are_rejected() {
        let (buffer, start) = fixture("先🙂<br>- ", true);
        let parsed = markdown::parse_decorations(buffer.text());
        let end = buffer.line_end(2) - 1;
        let first_cell = buffer.line_start(2) + "| ".len();
        assert!(cell_range_for_selection(&buffer, &parsed, true, &(first_cell..end)).is_none());
        let separator = buffer.line_start(1) + 2;
        assert!(
            cell_range_for_selection(&buffer, &parsed, true, &(separator..separator)).is_none()
        );
        // Rendered metadata is deliberately retained: the mode gate itself must
        // prevent both non-Markdown and source-table edits from taking ownership.
        assert!(cell_range_for_selection(&buffer, &parsed, false, &(end..end)).is_none());
        assert!(owner(&buffer, start + 1..start + 1).is_none()); // mid-UTF-8
        assert!(owner(&buffer, end..start).is_none());
        assert!(owner(&buffer, end..buffer.len() + 1).is_none());
        let mut stale = parsed.clone();
        stale[2].table_info.as_mut().unwrap().cells[1] = "unrelated".to_owned();
        assert!(cell_range_for_selection(&buffer, &stale, true, &(end..end)).is_none());

        let front = Buffer::from_text("---\ntitle: '先🙂<br>- '\nitems:\n  - value\n---");
        let parsed = markdown::parse_decorations(front.text());
        for line in 0..front.line_count() {
            for col in front
                .line_content(line)
                .char_indices()
                .map(|(i, _)| i)
                .chain([front.line_content(line).len()])
            {
                let offset = front.line_start(line) + col;
                assert!(
                    cell_range_for_selection(&front, &parsed, true, &(offset..offset)).is_none()
                );
            }
        }
    }

    #[test]
    fn native_and_pasted_newlines_remain_soft_even_inside_lists() {
        let (buffer, start) = fixture("lead<br>- [x] 先🙂", true);
        let rows = markdown::parse_decorations(buffer.text());
        let at = start + "lead<br>- [x] 先🙂".len();
        let targets = vec![(at..at, true)];
        let edits =
            native_newline_edits(&buffer, &rows, true, targets.clone(), "\n", None).unwrap();
        assert_eq!(edits[0].text, "<br>");
        let edits =
            native_newline_edits(&buffer, &rows, true, targets, "a\r\nb\n- pasted", None).unwrap();
        assert_eq!(edits[0].text, "a<br>b<br>- pasted");
        let enter = cell_enter_plan(
            &buffer,
            &rows,
            true,
            Cursor {
                offset: at,
                anchor: at,
            },
            false,
        )
        .unwrap();
        assert_eq!(enter.text, "<br>- [ ] ");
    }

    #[test]
    fn native_newline_commit_honors_explicit_replacement_and_clears_marking() {
        let (mut buffer, start) = fixture("first<br>- \t", true);
        let end = buffer.line_end(2) - 1;
        let parsed = markdown::parse_decorations(buffer.text());
        let edits = native_newline_edits(
            &buffer,
            &parsed,
            true,
            vec![(end - 1..end, true)],
            "\n",
            None,
        )
        .unwrap();
        let mut cursor = Cursor {
            anchor: start,
            offset: start,
        }; // not the replacement target
        let mut extras = Vec::new();
        let mut marked = Some(end - 1..end);
        let mut extra_marks = Vec::new();
        apply_native_edits(
            &mut buffer,
            &mut cursor,
            &mut extras,
            &mut marked,
            &mut extra_marks,
            edits,
            false,
        );
        assert!(buffer.line_content(2).ends_with("first<br>- <br>|"));
        assert_eq!(cursor.offset, end - 1 + 4);
        assert_eq!(cursor.anchor, cursor.offset);
        assert!(marked.is_none());
        assert_eq!(buffer.line_count(), 3);
    }

    #[test]
    fn native_ime_newlines_translate_utf16_selection_and_keep_the_mark_inside_cell() {
        let (mut buffer, start) = fixture("first<br>- ", true);
        let end = buffer.line_end(2) - 1;
        let parsed = markdown::parse_decorations(buffer.text());
        let incoming = "語🙂\r\n次\n";
        let edits = native_newline_edits(
            &buffer,
            &parsed,
            true,
            vec![(end..end, true)],
            incoming,
            Some(&(5..6)),
        )
        .unwrap();
        let mut cursor = Cursor {
            anchor: start,
            offset: start,
        };
        let mut extras = Vec::new();
        let mut marked = None;
        let mut extra_marks = Vec::new();
        apply_native_edits(
            &mut buffer,
            &mut cursor,
            &mut extras,
            &mut marked,
            &mut extra_marks,
            edits,
            true,
        );
        let expanded = "語🙂<br>次<br>";
        assert!(buffer.line_content(2).ends_with(&format!("{expanded}|")));
        assert_eq!(marked, Some(end..end + expanded.len()));
        assert_eq!(buffer.text_for_range(cursor.selection_range()), "次");
        assert_eq!(cursor.anchor, end + "語🙂<br>".len());
        assert_eq!(cursor.offset, end + "語🙂<br>次".len());
        assert!(owner(&buffer, marked.clone().unwrap()).is_some());

        let parsed = markdown::parse_decorations(buffer.text());
        let edits = native_newline_edits(
            &buffer,
            &parsed,
            true,
            vec![(marked.clone().unwrap(), true)],
            "確定\n",
            None,
        )
        .unwrap();
        apply_native_edits(
            &mut buffer,
            &mut cursor,
            &mut extras,
            &mut marked,
            &mut extra_marks,
            edits,
            false,
        );
        assert!(buffer.line_content(2).ends_with("確定<br>|"));
        assert!(marked.is_none());
        assert!(owner(&buffer, cursor.selection_range()).is_some());
    }

    #[test]
    fn native_multi_cursor_plans_ownership_before_same_row_edits_and_remaps_carets() {
        let (mut buffer, start) = fixture("first<br>- ", true);
        let primary = buffer.line_end(2) - 1;
        let neighbor = buffer.line_start(2) + "| 隣🙂".len();
        let parsed = markdown::parse_decorations(buffer.text());
        let edits = native_newline_edits(
            &buffer,
            &parsed,
            true,
            vec![(primary..primary, true), (neighbor..neighbor, false)],
            "\n",
            None,
        )
        .unwrap();
        let mut cursor = Cursor {
            anchor: primary,
            offset: primary,
        };
        let mut extras = Vec::new();
        let mut marked = None;
        let mut extra_marks = Vec::new();
        apply_native_edits(
            &mut buffer,
            &mut cursor,
            &mut extras,
            &mut marked,
            &mut extra_marks,
            edits,
            true,
        );
        assert_eq!(buffer.line_content(2), "| 隣🙂<br> | first<br>- <br>|");
        assert_eq!(cursor.offset, primary + 8);
        assert_eq!(extras[0].offset, neighbor + 4);
        assert_eq!(buffer.line_count(), 3);
        assert!(owner(&buffer, marked.unwrap()).is_some());
        assert!(owner(&buffer, extra_marks[0].clone()).is_some());
        assert!(owner(&buffer, start + 4..cursor.offset).is_some());
    }

    #[test]
    fn native_mixed_targets_convert_only_owned_cells_and_preserve_literal_outside_text() {
        let (mut buffer, _) = fixture("first<br>- ", true);
        let cell_end = buffer.line_end(2) - 1;
        buffer.insert(buffer.len(), "\n\noutside");
        let outside = buffer.len();
        let parsed = markdown::parse_decorations(buffer.text());
        let edits = native_newline_edits(
            &buffer,
            &parsed,
            true,
            vec![(outside..outside, false), (cell_end..cell_end, true)],
            "新\n",
            None,
        )
        .unwrap();
        let mut cursor = Cursor {
            anchor: cell_end,
            offset: cell_end,
        };
        let mut extras = Vec::new();
        let mut marked = None;
        let mut extra_marks = Vec::new();
        apply_native_edits(
            &mut buffer,
            &mut cursor,
            &mut extras,
            &mut marked,
            &mut extra_marks,
            edits,
            false,
        );
        assert!(buffer.line_content(2).ends_with("新<br>|"));
        assert!(buffer.text().ends_with("outside新\n"));
        assert_eq!(cursor.offset, cell_end + "新<br>".len());
        assert_eq!(extras[0].offset, outside + "新<br>".len() + "新\n".len());
    }

    fn apply_native_at_original_boundary(
        original: &str,
        side: super::super::TableBoundarySide,
        incoming: &str,
        selected_utf16: Option<Range<usize>>,
        keep_marked: bool,
    ) -> (Buffer, Cursor, Option<Range<usize>>) {
        let mut buffer = Buffer::from_text(original);
        let parsed = markdown::parse_decorations(original);
        let boundary = super::super::input_table::table_boundary_offsets(
            &buffer,
            &parsed,
            super::super::TableBoundaryCaret {
                table_start_line: 0,
                side,
            },
        )
        .unwrap();
        // Both callbacks receive native offsets for the ORIGINAL buffer, even
        // though boundary materialization happens before their replacement.
        let original_utf16 = buffer.utf16_offset_for_offset(boundary.0);
        let original_range = buffer.offset_for_utf16_offset(original_utf16)
            ..buffer.offset_for_utf16_offset(original_utf16);
        assert_eq!(original_range, boundary.0..boundary.0);
        buffer.insert(boundary.0, "\n");
        let remapped = native_range_after_boundary_insertion(original_range, Some(boundary));
        let remapped_utf16 = buffer.utf16_offset_for_offset(remapped.start)
            ..buffer.utf16_offset_for_offset(remapped.end);
        let target = buffer.offset_for_utf16_offset(remapped_utf16.start)
            ..buffer.offset_for_utf16_offset(remapped_utf16.end);
        assert_eq!(target, boundary.1..boundary.1);
        let parsed = markdown::parse_decorations(buffer.text());
        assert!(
            native_newline_edits(
                &buffer,
                &parsed,
                true,
                vec![(target.clone(), true)],
                incoming,
                selected_utf16.as_ref()
            )
            .is_none()
        );
        let relative = selected_utf16.map_or(incoming.len()..incoming.len(), |selection| {
            EditorView::utf8_offset_in_text_for_utf16(incoming, selection.start)
                ..EditorView::utf8_offset_in_text_for_utf16(incoming, selection.end)
        });
        let mut cursor = Cursor {
            anchor: target.start,
            offset: target.start,
        };
        let mut marked = None;
        // The interceptor declined the remapped outside target: apply the same
        // literal replacement/mark/selection as the callbacks' ordinary path.
        apply_native_edits(
            &mut buffer,
            &mut cursor,
            &mut Vec::new(),
            &mut marked,
            &mut Vec::new(),
            vec![NativeEdit {
                range: target,
                text: incoming.to_owned(),
                selection: relative,
                primary: true,
            }],
            keep_marked,
        );
        let mut expected = original.to_owned();
        expected.insert(boundary.0, '\n');
        expected.insert_str(boundary.1, incoming);
        assert_eq!(buffer.text(), expected);
        (buffer, cursor, marked)
    }

    #[test]
    fn native_committed_explicit_original_below_boundary_stays_outside_pipe_less_cell() {
        for content in ["first<br>last", "first<br>  \t", "先🙂<br>- [ ] "] {
            for closing_pipe in [false, true] {
                let (original, _) = fixture(content, closing_pipe);
                let row = original.line_content(2).to_owned();
                for following in ["", "\n\nfollowing paragraph"] {
                    let text = format!("{}{following}", original.text());
                    for incoming in ["\n", "outside", "外🙂\n次"] {
                        let (buffer, cursor, marked) = apply_native_at_original_boundary(
                            &text,
                            super::super::TableBoundarySide::Below,
                            incoming,
                            None,
                            false,
                        );
                        assert_eq!(buffer.line_content(2), row);
                        assert_eq!(cursor.offset, original.len() + 1 + incoming.len());
                        assert_eq!(cursor.anchor, cursor.offset);
                        assert!(marked.is_none());
                        let parsed = markdown::parse_decorations(buffer.text());
                        assert_eq!(parsed[2].table_info.as_ref().unwrap().cells.len(), 2);
                        assert_eq!(parsed[2].table_info.as_ref().unwrap().table_end_line, 3);
                    }
                }
            }
        }
    }

    #[test]
    fn native_marked_explicit_original_below_boundary_keeps_literal_newlines_and_utf16_selection() {
        for closing_pipe in [false, true] {
            let (original, _) = fixture("先🙂<br>- [ ] ", closing_pipe);
            for (incoming, selected) in [("\n", 1..1), ("語🙂\n次", 4..5)] {
                let (mut buffer, cursor, marked) = apply_native_at_original_boundary(
                    original.text(),
                    super::super::TableBoundarySide::Below,
                    incoming,
                    Some(selected.clone()),
                    true,
                );
                let start = original.len() + 1;
                assert_eq!(marked, Some(start..start + incoming.len()));
                assert_eq!(buffer.text_for_range(marked.clone().unwrap()), incoming);
                assert_eq!(buffer.line_content(2), original.line_content(2));
                assert_eq!(
                    cursor.anchor,
                    start + EditorView::utf8_offset_in_text_for_utf16(incoming, selected.start)
                );
                assert_eq!(
                    cursor.offset,
                    start + EditorView::utf8_offset_in_text_for_utf16(incoming, selected.end)
                );
                // A subsequent commit has no live boundary; its marked target
                // must stay outside without being shifted a second time.
                let range = native_range_after_boundary_insertion(marked.unwrap(), None);
                let parsed = markdown::parse_decorations(buffer.text());
                assert!(
                    native_newline_edits(
                        &buffer,
                        &parsed,
                        true,
                        vec![(range.clone(), true)],
                        "確定\n",
                        None
                    )
                    .is_none()
                );
                buffer.replace(range, "確定\n");
                assert_eq!(buffer.text(), format!("{}\n確定\n", original.text()));
            }
        }
    }

    #[test]
    fn native_original_above_boundary_uses_outside_line_instead_of_shifted_header() {
        let (original, _) = fixture("first<br>- ", false);
        for keep_marked in [false, true] {
            let (buffer, _, marked) = apply_native_at_original_boundary(
                original.text(),
                super::super::TableBoundarySide::Above,
                "外🙂\n",
                None,
                keep_marked,
            );
            assert_eq!(buffer.text(), format!("外🙂\n\n{}", original.text()));
            assert_eq!(marked, keep_marked.then_some(0.."外🙂\n".len()));
        }
    }

    #[test]
    fn native_boundary_remap_preserves_other_source_targets_and_is_identity_without_insertion() {
        for outside in [10, 11] {
            let boundary = Some((10, outside));
            assert_eq!(
                native_range_after_boundary_insertion(10..10, boundary),
                outside..outside
            );
            assert_eq!(native_range_after_boundary_insertion(2..5, boundary), 2..5);
            assert_eq!(
                native_range_after_boundary_insertion(8..10, boundary),
                8..10
            );
            assert_eq!(
                native_range_after_boundary_insertion(10..13, boundary),
                11..14
            );
            assert_eq!(
                native_range_after_boundary_insertion(11..11, boundary),
                12..12
            );
            assert_eq!(
                native_range_after_boundary_insertion(8..13, boundary),
                8..14
            );
        }
        let (buffer, _) = fixture("first<br>- ", false);
        let end = buffer.len();
        let range = native_range_after_boundary_insertion(end..end, None);
        assert_eq!(range, end..end);
        let parsed = markdown::parse_decorations(buffer.text());
        assert!(
            native_newline_edits(&buffer, &parsed, true, vec![(range, true)], "\n", None).is_some()
        );
    }

    #[test]
    fn native_source_mode_without_boundary_keeps_requested_target_and_literal_text() {
        for keep_marked in [false, true] {
            let (mut buffer, _) = fixture("先🙂<br>- ", false);
            let original = buffer.text().to_owned();
            let offset = buffer.len();
            let target = native_range_after_boundary_insertion(offset..offset, None);
            let parsed = markdown::parse_decorations(buffer.text());
            assert_eq!(target, offset..offset);
            assert!(
                native_newline_edits(
                    &buffer,
                    &parsed,
                    false,
                    vec![(target.clone(), true)],
                    "\n",
                    None
                )
                .is_none()
            );
            let mut cursor = Cursor {
                anchor: offset,
                offset,
            };
            let mut marked = None;
            apply_native_edits(
                &mut buffer,
                &mut cursor,
                &mut Vec::new(),
                &mut marked,
                &mut Vec::new(),
                vec![NativeEdit {
                    range: target,
                    text: "\n".to_owned(),
                    selection: 1..1,
                    primary: true,
                }],
                keep_marked,
            );
            assert_eq!(buffer.text(), format!("{original}\n"));
            assert_eq!(marked, keep_marked.then_some(offset..offset + 1));
            assert_eq!(cursor.offset, offset + 1);
        }
    }

    #[test]
    fn native_source_front_matter_and_pipe_crossing_callbacks_are_not_intercepted() {
        let (buffer, start) = fixture("first<br>- ", true);
        let end = buffer.line_end(2) - 1;
        let parsed = markdown::parse_decorations(buffer.text());
        assert!(
            native_newline_edits(&buffer, &parsed, false, vec![(end..end, true)], "\n", None)
                .is_none()
        );
        assert!(
            native_newline_edits(
                &buffer,
                &parsed,
                true,
                vec![(start..end + 1, true)],
                "\n",
                None
            )
            .is_none()
        );
        assert!(
            native_newline_edits(&buffer, &parsed, true, vec![(end..end, true)], "文字", None)
                .is_none()
        );
        let front = Buffer::from_text("---\ntitle: value\n---");
        let parsed = markdown::parse_decorations(front.text());
        let end = front.line_end(1);
        assert!(
            native_newline_edits(&front, &parsed, true, vec![(end..end, true)], "\n", None)
                .is_none()
        );
    }
}
