//! Pure source edit plans. Cell callers supply only renderer-accepted fragments;
//! no plan can consume a pipe or mistake an opaque/escaped `<br>` for a line.
use std::ops::Range;

use crate::{
    buffer::Buffer,
    cursor::Cursor,
    markdown::{self, BlockType, LineDecoration},
};

use super::{helpers, task_prefix_len};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Edit {
    pub range: Range<usize>,
    pub text: String,
    /// Byte offset relative to the replacement start, after this edit.
    pub caret: usize,
    /// Preserve the current number, and bytes to look back from the caret.
    /// Empty-item exit normalizes the cleared fragment, not the new blank one.
    pub normalize: Option<(bool, usize)>,
}

impl Edit {
    pub fn new(range: Range<usize>, text: String) -> Self {
        let caret = text.len();
        Self {
            range,
            text,
            caret,
            normalize: None,
        }
    }

    pub fn translated(mut self, start: usize) -> Self {
        self.range = self.range.start + start..self.range.end + start;
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DeleteAction {
    Backward,
    Forward,
    Word { backwards: bool },
    LineBoundary { to_start: bool, kill: bool },
    Line,
}

fn fragment_index(decoration: &LineDecoration, offset: usize) -> Option<usize> {
    decoration
        .table_cell_fragments
        .iter()
        .position(|fragment| fragment.range.start <= offset && offset <= fragment.range.end)
}

/// Accepted breaks, not raw tag spellings, delimit the editable logical lines.
/// Every returned range is local to the supplied cell.
pub(super) fn cell_delete(
    source: &str,
    decoration: &LineDecoration,
    cursor: Cursor,
    action: DeleteAction,
) -> Edit {
    if action == DeleteAction::Backward {
        let buffer = Buffer::from_text(source);
        return cell_backspace(
            &buffer,
            0..source.len(),
            decoration,
            cursor,
            paired_backspace_range(source, cursor),
        );
    }
    let selection = cursor.selection_range();
    let breaks: Vec<_> = decoration
        .display_replacements
        .iter()
        .filter(|r| r.display_text == "\n")
        .map(|r| &r.source_range)
        .collect();
    let range = if action == DeleteAction::Line {
        let locate = |offset| {
            fragment_index(decoration, offset).or_else(|| {
                breaks
                    .iter()
                    .find(|r| r.start < offset && offset < r.end)
                    .and_then(|r| fragment_index(decoration, r.start))
            })
        };
        let Some(first) = locate(selection.start) else {
            return Edit::new(cursor.offset..cursor.offset, String::new());
        };
        let end_probe = selection
            .end
            .saturating_sub(usize::from(!selection.is_empty()));
        let last = locate(end_probe).unwrap_or(first);
        let fragments = &decoration.table_cell_fragments;
        if let Some(next) = fragments.get(last + 1) {
            fragments[first].range.start..next.range.start
        } else if first > 0 {
            fragments[first - 1].range.end..fragments[last].range.end
        } else {
            0..source.len()
        }
    } else if !selection.is_empty() {
        // A native/raw source selection may cut into a painted break. Expand
        // only accepted breaks, never escaped tags or code/math literals.
        let mut range = selection;
        for r in &breaks {
            if range.start < r.end && r.start < range.end {
                range.start = range.start.min(r.start);
                range.end = range.end.max(r.end);
            }
        }
        range
    } else if let Some(index) = fragment_index(decoration, cursor.offset) {
        let fragments = &decoration.table_cell_fragments;
        let fragment = &fragments[index].range;
        match action {
            DeleteAction::LineBoundary { to_start: true, .. } => fragment.start..cursor.offset,
            DeleteAction::LineBoundary {
                to_start: false,
                kill,
            } => {
                let end = if kill && cursor.offset == fragment.end {
                    fragments
                        .get(index + 1)
                        .map_or(fragment.end, |f| f.range.start)
                } else {
                    fragment.end
                };
                cursor.offset..end
            }
            DeleteAction::Forward if cursor.offset == fragment.end => {
                cursor.offset
                    ..fragments
                        .get(index + 1)
                        .map_or(cursor.offset, |f| f.range.start)
            }
            DeleteAction::Forward => {
                let buffer = Buffer::from_text(&source[fragment.clone()]);
                cursor.offset
                    ..fragment.start + buffer.next_grapheme_boundary(cursor.offset - fragment.start)
            }
            DeleteAction::Word { backwards } => {
                if backwards && cursor.offset == fragment.start {
                    fragments
                        .get(index.wrapping_sub(1))
                        .map_or(cursor.offset, |f| f.range.end)..cursor.offset
                } else if !backwards && cursor.offset == fragment.end {
                    cursor.offset
                        ..fragments
                            .get(index + 1)
                            .map_or(cursor.offset, |f| f.range.start)
                } else {
                    let buffer = Buffer::from_text(&source[fragment.clone()]);
                    let local = cursor.offset - fragment.start;
                    let mut probe = Cursor {
                        offset: local,
                        anchor: local,
                    };
                    if backwards {
                        probe.move_word_left(&buffer);
                    } else {
                        probe.move_word_right(&buffer);
                    }
                    fragment.start + local.min(probe.offset)
                        ..fragment.start + local.max(probe.offset)
                }
            }
            DeleteAction::Backward | DeleteAction::Line => unreachable!(),
        }
    } else if let Some(r) = breaks
        .iter()
        .find(|r| r.start < cursor.offset && cursor.offset < r.end)
    {
        (*r).clone()
    } else {
        cursor.offset..cursor.offset
    };
    let removes_break = breaks
        .iter()
        .any(|r| range.start < r.end && r.start < range.end);
    let touches_ordered = decoration.table_cell_fragments.iter().any(|f| {
        matches!(f.block_type, BlockType::OrderedList)
            && range.start < f.range.start + f.prefix_len
            && f.range.start < range.end
    });
    let mut edit = Edit::new(range, String::new());
    edit.normalize = (removes_break || touches_ordered).then_some((
        !removes_break && action == DeleteAction::Forward && !cursor.has_selection(),
        0,
    ));
    edit
}

/// Merge overlapping destructive plans before writing. No-op/protected targets
/// retain their selection, and every cursor is remapped through the same union.
pub(super) fn deletion_ranges(plans: &[(Cursor, bool, Edit)]) -> Vec<Range<usize>> {
    let mut ranges: Vec<_> = plans
        .iter()
        .map(|(_, _, e)| e.range.clone())
        .filter(|r| !r.is_empty())
        .collect();
    ranges.sort_by_key(|r| (r.start, r.end));
    let mut merged: Vec<Range<usize>> = Vec::new();
    for range in ranges {
        if let Some(last) = merged.last_mut().filter(|last| range.start <= last.end) {
            last.end = last.end.max(range.end);
        } else {
            merged.push(range);
        }
    }
    merged
}

#[cfg(test)]
pub(super) fn apply_deletions(
    buffer: &mut Buffer,
    plans: &[(Cursor, bool, Edit)],
) -> Vec<(Cursor, bool, Option<(bool, usize)>)> {
    let ranges = deletion_ranges(plans);
    let replacements: Vec<_> = ranges
        .into_iter()
        .map(|range| (range, String::new()))
        .collect();
    apply_deletions_with_replacements(buffer, plans, &replacements)
}

pub(super) fn apply_deletions_with_replacements(
    buffer: &mut Buffer,
    plans: &[(Cursor, bool, Edit)],
    replacements: &[(Range<usize>, String)],
) -> Vec<(Cursor, bool, Option<(bool, usize)>)> {
    for (range, text) in replacements.iter().rev() {
        buffer.replace(range.clone(), text);
    }
    let remap = |offset: usize| {
        let mut delta = 0isize;
        for (range, text) in replacements {
            if offset >= range.end {
                delta += text.len() as isize - range.len() as isize;
            } else if offset >= range.start {
                return buffer.clamp_offset(
                    (range.start as isize + delta).max(0) as usize
                        + (offset - range.start).min(text.len()),
                );
            } else {
                break;
            }
        }
        buffer.clamp_offset(offset.saturating_add_signed(delta))
    };
    plans
        .iter()
        .map(|(cursor, primary, edit)| {
            let cursor = if edit.range.is_empty() {
                Cursor {
                    offset: remap(cursor.offset),
                    anchor: remap(cursor.anchor),
                }
            } else {
                let offset = remap(edit.range.start);
                Cursor {
                    offset,
                    anchor: offset,
                }
            };
            (cursor, *primary, edit.normalize)
        })
        .collect()
}

/// Use ordinary pairing policy, but with no preceding pipe, previous fragment,
/// or following cell leaking into the line-start/adjacent-character context.
pub(super) fn cell_angle_input(
    source: &str,
    decoration: &LineDecoration,
    offset: usize,
) -> Option<Edit> {
    let fragment = &decoration
        .table_cell_fragments
        .get(fragment_index(decoration, offset)?)?
        .range;
    let local = offset - fragment.start;
    let pair = super::input_ime::paired_delimiter_at(&source[fragment.clone()], local, "<");
    let mut edit = Edit::new(
        offset..offset,
        if pair.is_some() { "<>" } else { "<" }.to_string(),
    );
    edit.caret = 1;
    Some(edit)
}

fn leading(source: &str) -> usize {
    source.len() - source.trim_start_matches([' ', '\t']).len()
}

fn list_prefix(source: &str, kind: &BlockType) -> usize {
    match kind {
        BlockType::TaskList(_) => task_prefix_len(source),
        BlockType::UnorderedList if ["- ", "* ", "+ "].iter().any(|p| source.starts_with(p)) => 2,
        BlockType::OrderedList => markdown::ordered_list_prefix_len(source),
        _ => 0,
    }
}

fn indent_step(kind: &BlockType, source: &str) -> Option<usize> {
    match kind {
        BlockType::OrderedList => Some(3),
        BlockType::TaskList(_) => {
            let (_, body) = markdown::split_continuation_prefix(source);
            Some(if markdown::ordered_list_prefix_len(body) > 0 {
                3
            } else {
                2
            })
        }
        BlockType::UnorderedList => Some(2),
        _ => None,
    }
}

/// The same marker/outdent policy as ordinary Enter. `range` and `selection`
/// are in one coordinate space; the caller chooses a physical or semantic line.
pub(super) fn enter(
    source: &str,
    range: Range<usize>,
    kind: &BlockType,
    selection: Range<usize>,
    caret: usize,
    newline: &str,
) -> Edit {
    let line = &source[range.clone()];
    let quote = markdown::blockquote_prefix_len(line);
    let (whitespace, trimmed) = markdown::split_continuation_prefix(line);
    let indent = whitespace.len();
    let prefix = list_prefix(trimmed, kind);
    if prefix > 0 && caret >= range.start + indent + prefix {
        if line[indent + prefix..].trim().is_empty() {
            let mut edit = if indent > quote {
                let step = indent_step(kind, line).unwrap().min(indent - quote);
                Edit::new(
                    range,
                    format!(
                        "{}{}{}",
                        &line[..quote],
                        " ".repeat(indent - quote - step),
                        &line[indent..]
                    ),
                )
            } else {
                Edit::new(range, format!("{}{newline}", &line[..quote]))
            };
            edit.normalize = (matches!(kind, BlockType::OrderedList)
                || (matches!(kind, BlockType::TaskList(_))
                    && markdown::ordered_list_prefix_len(trimmed) > 0))
                .then_some((false, if indent > quote { 0 } else { newline.len() }));
            return edit;
        }
        let marker = match kind {
            BlockType::TaskList(_) => {
                super::task_list_continuation(trimmed)
                    .expect("task fragment has a valid prefix")
                    .0
            }
            BlockType::OrderedList => {
                // Read digits, not `prefix - 2`: accepted prefixes can use `)`
                // or multiple spaces/tabs. Continuations use the ordinary `. `.
                let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
                let number = trimmed[..digits].parse::<u32>().unwrap_or(1);
                format!("{}. ", number.saturating_add(1))
            }
            _ => trimmed[..prefix].to_string(),
        };
        let mut edit = Edit::new(selection, format!("{newline}{whitespace}{marker}"));
        if matches!(kind, BlockType::OrderedList)
            || (matches!(kind, BlockType::TaskList(_))
                && markdown::ordered_list_prefix_len(trimmed) > 0)
        {
            edit.normalize = Some((true, 0));
        }
        return edit;
    }
    if matches!(kind, BlockType::Blockquote) && quote > 0 && caret >= range.start + quote {
        return if line[quote..].trim().is_empty() {
            Edit::new(range, newline.to_string())
        } else {
            Edit::new(selection, format!("{newline}{}", &line[..quote]))
        };
    }
    Edit::new(selection, newline.to_string())
}

pub(super) fn cell_enter(
    source: &str,
    decoration: &LineDecoration,
    selection: Range<usize>,
    caret: usize,
    soft: bool,
) -> Edit {
    if !soft {
        if let Some(fragment) = decoration.table_cell_fragments.iter().find(|fragment| {
            fragment.range.start <= caret
                && caret <= fragment.range.end
                && fragment.range.start <= selection.start
                && selection.end <= fragment.range.end
        }) {
            // Quotes/headings/fences are deliberately not block constructs in
            // cells. Only semantic list prefixes participate in continuation.
            if fragment.prefix_len > 0 {
                return enter(
                    source,
                    fragment.range.clone(),
                    &fragment.block_type,
                    selection,
                    caret,
                    "<br>",
                );
            }
        }
    }
    Edit::new(selection, "<br>".to_string())
}

pub(super) fn cell_backspace(
    buffer: &Buffer,
    cell: Range<usize>,
    decoration: &LineDecoration,
    cursor: Cursor,
    paired: Option<Range<usize>>,
) -> Edit {
    let selection = cursor.selection_range();
    if !selection.is_empty() {
        return cell_delete(
            buffer.text_for_range(cell.clone()),
            decoration,
            Cursor {
                offset: cursor.offset - cell.start,
                anchor: cursor.anchor - cell.start,
            },
            DeleteAction::Forward,
        )
        .translated(cell.start);
    }
    let local = cursor.offset - cell.start;
    let source = buffer.text_for_range(cell.clone());
    if let Some(fragment) = decoration.table_cell_fragments.iter().find(|fragment| {
        fragment.prefix_len > 0 && local == fragment.range.start + fragment.prefix_len
    }) {
        let line = &source[fragment.range.clone()];
        let indent = leading(line);
        let trimmed = &line[indent..];
        let prefix = list_prefix(trimmed, &fragment.block_type);
        if prefix > 0 {
            let start = cell.start + fragment.range.start + indent;
            let mut edit = Edit::new(start..start + prefix, String::new());
            edit.normalize = Some((false, 0));
            return edit;
        }
    }
    if let Some(range) = paired.filter(|r| cell.start <= r.start && r.end <= cell.end) {
        return Edit::new(range, String::new());
    }
    if let Some(replacement) = decoration.display_replacements.iter().find(|r| {
        r.display_text == "\n" && r.source_range.start < local && local <= r.source_range.end
    }) {
        let mut edit = Edit::new(
            cell.start + replacement.source_range.start..cell.start + replacement.source_range.end,
            String::new(),
        );
        edit.normalize = Some((false, 0));
        return edit;
    }
    let start = buffer.prev_grapheme_boundary(cursor.offset).max(cell.start);
    let mut edit = Edit::new(start..cursor.offset, String::new());
    edit.normalize = decoration
        .table_cell_fragments
        .iter()
        .any(|fragment| {
            matches!(fragment.block_type, BlockType::OrderedList)
                && fragment.range.start <= start - cell.start
                && start - cell.start <= fragment.range.start + fragment.prefix_len
        })
        .then_some((true, 0));
    edit
}

/// Tab affects each selected fragment once, not the physical table row. A
/// selection ending at the next fragment's start does not indent that fragment.
pub(super) fn cell_indent(
    source: &str,
    decoration: &LineDecoration,
    selection: Range<usize>,
    forward: bool,
) -> Option<Vec<Edit>> {
    let fragments: Vec<_> = decoration
        .table_cell_fragments
        .iter()
        .filter(|fragment| {
            if selection.is_empty() {
                fragment.range.start <= selection.start && selection.start <= fragment.range.end
            } else {
                fragment.range.start < selection.end && selection.start <= fragment.range.end
            }
        })
        .collect();
    if !fragments
        .iter()
        .any(|f| indent_step(&f.block_type, &source[f.range.clone()]).is_some())
    {
        return None;
    }
    let mut width = 2;
    Some(
        fragments
            .into_iter()
            .map(|fragment| {
                width = indent_step(&fragment.block_type, &source[fragment.range.clone()])
                    .unwrap_or(width);
                let start = fragment.range.start;
                if forward {
                    Edit::new(start..start, " ".repeat(width))
                } else {
                    let strip = source[start..fragment.range.end]
                        .bytes()
                        .take(width)
                        .take_while(|b| *b == b' ')
                        .count();
                    Edit::new(start..start + strip, String::new())
                }
            })
            .collect(),
    )
}

/// Ordinary near-line numbering, restricted to accepted fragments of one cell.
pub(super) fn ordered_edits(
    source: &str,
    decoration: &LineDecoration,
    caret: usize,
    preserve: bool,
) -> Vec<Edit> {
    let fragments = &decoration.table_cell_fragments;
    let Some(current) = fragments
        .iter()
        .position(|f| f.range.start <= caret && caret <= f.range.end)
    else {
        return Vec::new();
    };
    let info = |index: usize| {
        let f = fragments.get(index)?;
        let line = &source[f.range.clone()];
        let indent = leading(line);
        if !matches!(
            f.block_type,
            BlockType::OrderedList | BlockType::TaskList(_)
        ) || markdown::ordered_list_prefix_len(&line[indent..]) == 0
        {
            return None;
        }
        let digits = line[indent..]
            .bytes()
            .take_while(u8::is_ascii_digit)
            .count();
        let number = line[indent..indent + digits].parse::<u32>().ok()?;
        Some((
            f.range.start + indent..f.range.start + indent + digits,
            indent,
            number,
        ))
    };
    let target = if info(current).is_some() {
        current
    } else {
        current + 1
    };
    let Some((_, indent, number)) = info(target) else {
        return Vec::new();
    };
    let (start, mut next) = if preserve {
        (target + 1, number.saturating_add(1))
    } else {
        let mut previous = None;
        for index in (0..target).rev() {
            let line = &source[fragments[index].range.clone()];
            if line.trim().is_empty() || leading(line) < indent {
                break;
            }
            if leading(line) > indent {
                continue;
            }
            previous = info(index).map(|(_, _, n)| n);
            break;
        }
        (target, previous.map_or(1, |n| n.saturating_add(1)))
    };
    let mut edits = Vec::new();
    for index in start..fragments.len() {
        let line = &source[fragments[index].range.clone()];
        if leading(line) < indent {
            break;
        }
        if leading(line) > indent {
            continue;
        }
        let Some((range, _, number)) = info(index) else {
            break;
        };
        if number != next {
            edits.push(Edit::new(range, next.to_string()));
        }
        next = next.saturating_add(1);
    }
    edits
}

pub(super) fn ordinary_ordered_edits(
    buffer: &Buffer,
    rows: &[LineDecoration],
    caret: usize,
    preserve: bool,
) -> Vec<Edit> {
    let mut decoration = LineDecoration::default();
    decoration.table_cell_fragments = (0..buffer.line_count())
        .map(|line| markdown::TableCellFragment {
            range: buffer.line_start(line)..buffer.line_end(line),
            block_type: rows
                .get(line)
                .map(|d| d.block_type.clone())
                .unwrap_or(BlockType::Normal),
            prefix_len: 0,
        })
        .collect();
    ordered_edits(buffer.text(), &decoration, caret, preserve)
}

pub(super) fn cell_decoration(
    buffer: &Buffer,
    rows: &[LineDecoration],
    cell: &Range<usize>,
) -> LineDecoration {
    let (line, _) = buffer.offset_to_line_col(cell.start);
    let start = buffer.line_start(line);
    helpers::table_cell_decoration(
        &rows[line],
        buffer.text_for_range(cell.clone()),
        &(cell.start - start..cell.end - start),
    )
}

pub(super) fn paired_backspace_range(text: &str, cursor: Cursor) -> Option<Range<usize>> {
    if cursor.has_selection() {
        return None;
    }
    let offset = cursor.offset;
    if text.get(offset.saturating_sub(2)..offset) == Some("[[")
        && text.get(offset..offset + 2) == Some("]]")
    {
        return Some(offset - 2..offset + 2);
    }
    let (start, open) = text.get(..offset)?.char_indices().next_back()?;
    let next = text.get(offset..)?.chars().next()?;
    let mut encoded = [0; 4];
    let (_, close) = super::input_ime::paired_delimiter(open.encode_utf8(&mut encoded))?;
    (close.chars().next() == Some(next)).then_some(start..offset + next.len_utf8())
}

pub(super) fn apply_prefix_edits(
    buffer: &mut Buffer,
    cursors: &mut [(Cursor, bool)],
    mut edits: Vec<Edit>,
) {
    edits.sort_by_key(|e| (e.range.start, e.range.end));
    edits.dedup_by(|a, b| a.range == b.range && a.text == b.text);
    for edit in edits.into_iter().rev() {
        buffer.replace(edit.range.clone(), &edit.text);
        for (cursor, _) in cursors.iter_mut() {
            let adjust = |offset: usize| {
                if offset < edit.range.start {
                    offset
                } else if offset >= edit.range.end {
                    offset - edit.range.len() + edit.text.len()
                } else {
                    edit.range.start + (offset - edit.range.start).min(edit.text.len())
                }
            };
            cursor.offset = adjust(cursor.offset);
            cursor.anchor = adjust(cursor.anchor);
        }
    }
}

/// Resolve all source plans before mutation. Identical empty-item edits from
/// several carets coalesce; incompatible overlapping selections are a safe no-op.
pub(super) fn apply(
    buffer: &mut Buffer,
    plans: &[(Edit, bool)],
) -> Option<Vec<(Cursor, bool, Option<(bool, usize)>)>> {
    let mut unique: Vec<&Edit> = plans.iter().map(|(edit, _)| edit).collect();
    unique.sort_by_key(|e| (e.range.start, e.range.end));
    unique.dedup_by(|a, b| a.range == b.range && a.text == b.text);
    if unique.windows(2).any(|pair| {
        pair[0].range.end > pair[1].range.start
            || (pair[0].range.start == pair[1].range.start && pair[0].text != pair[1].text)
    }) {
        return None;
    }
    for edit in unique.iter().rev() {
        buffer.replace(edit.range.clone(), &edit.text);
    }
    Some(
        plans
            .iter()
            .map(|(plan, primary)| {
                let delta: isize = unique
                    .iter()
                    .filter(|edit| edit.range.start < plan.range.start)
                    .map(|edit| edit.text.len() as isize - edit.range.len() as isize)
                    .sum();
                let offset =
                    buffer.clamp_offset(plan.range.start.saturating_add_signed(delta) + plan.caret);
                (
                    Cursor {
                        offset,
                        anchor: offset,
                    },
                    *primary,
                    plan.normalize,
                )
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decoration(source: &str) -> LineDecoration {
        let rows = markdown::parse_decorations("| A | B |\n| --- | --- |\n| left | right |");
        helpers::table_cell_decoration(&rows[2], source, &(0..source.len()))
    }

    fn run(source: &str, at: usize, soft: bool) -> (String, usize) {
        let plan = cell_enter(source, &decoration(source), at..at, at, soft);
        let mut buffer = Buffer::from_text(source);
        let cursors = apply(&mut buffer, &[(plan, true)]).unwrap();
        (buffer.text().to_string(), cursors[0].0.offset)
    }

    #[test]
    fn deletion_boundaries_are_semantic_and_breaks_are_atomic() {
        for tag in ["<br>", "<BR/>", "<br />", "<Br\t/>"] {
            let source = format!("先🙂{tag}two words<br>last");
            let d = decoration(&source);
            let left = "先🙂".len();
            let right = left + tag.len();
            for action in [
                DeleteAction::Forward,
                DeleteAction::Word { backwards: false },
                DeleteAction::LineBoundary {
                    to_start: false,
                    kill: true,
                },
            ] {
                assert_eq!(
                    cell_delete(
                        &source,
                        &d,
                        Cursor {
                            offset: left,
                            anchor: left
                        },
                        action
                    )
                    .range,
                    left..right
                );
            }
            assert_eq!(
                cell_delete(
                    &source,
                    &d,
                    Cursor {
                        offset: right,
                        anchor: right
                    },
                    DeleteAction::Word { backwards: true }
                )
                .range,
                left..right
            );
            for at in [left + 1, right - 1] {
                assert_eq!(
                    cell_delete(
                        &source,
                        &d,
                        Cursor {
                            offset: at,
                            anchor: at
                        },
                        DeleteAction::Forward
                    )
                    .range,
                    left..right
                );
            }
            let at = right + "two ".len();
            assert_eq!(
                cell_delete(
                    &source,
                    &d,
                    Cursor {
                        offset: at,
                        anchor: at
                    },
                    DeleteAction::LineBoundary {
                        to_start: true,
                        kill: false
                    }
                )
                .range,
                right..at
            );
            assert_eq!(
                cell_delete(
                    &source,
                    &d,
                    Cursor {
                        offset: at,
                        anchor: at
                    },
                    DeleteAction::LineBoundary {
                        to_start: false,
                        kill: false
                    }
                )
                .range,
                at..right + "two words".len()
            );
            assert_eq!(
                cell_delete(
                    &source,
                    &d,
                    Cursor {
                        offset: left,
                        anchor: left
                    },
                    DeleteAction::LineBoundary {
                        to_start: false,
                        kill: false
                    }
                )
                .range,
                left..left
            );
            let selected = Cursor {
                offset: left + 2,
                anchor: left + 1,
            };
            assert_eq!(
                cell_delete(&source, &d, selected, DeleteAction::Forward).range,
                left..right
            );
        }
    }

    #[test]
    fn word_deletion_matches_ordinary_policy_inside_each_fragment() {
        for text in [
            "alpha beta",
            "先🙂 é ending",
            "- [x] todo next",
            "x(a) end",
            r"code\<br> literal",
            "`code<br>tag` word",
            "$a<br>b$ word",
        ] {
            let source = format!("before<br>{text}<br>after");
            let d = decoration(&source);
            let start = "before<br>".len();
            let buffer = Buffer::from_text(text);
            for at in text.char_indices().map(|(at, _)| at).filter(|at| *at > 0) {
                for backwards in [false, true] {
                    let cursor = Cursor {
                        offset: at,
                        anchor: at,
                    };
                    let mut ordinary = Buffer::from_text(buffer.text());
                    let mut probe = cursor;
                    if backwards {
                        probe.delete_word_left(&mut ordinary);
                    } else {
                        probe.delete_word_right(&mut ordinary);
                    }
                    let plan = cell_delete(
                        &source,
                        &d,
                        Cursor {
                            offset: start + at,
                            anchor: start + at,
                        },
                        DeleteAction::Word { backwards },
                    );
                    let mut edited = Buffer::from_text(&source);
                    edited.delete(plan.range);
                    assert_eq!(
                        edited.text(),
                        format!("before<br>{}<br>after", ordinary.text()),
                        "{text:?} at {at}, backwards={backwards}"
                    );
                }
            }
        }
    }

    #[test]
    fn delete_line_removes_only_selected_fragments_and_one_accepted_break() {
        let source = "first<BR />second<br>last";
        let d = decoration(source);
        for (at, range) in [(2, 0..11), (13, 11..21), (23, 17..25)] {
            assert_eq!(
                cell_delete(
                    source,
                    &d,
                    Cursor {
                        offset: at,
                        anchor: at
                    },
                    DeleteAction::Line
                )
                .range,
                range
            );
        }
        let next = d.table_cell_fragments[1].range.start;
        assert_eq!(
            cell_delete(
                source,
                &d,
                Cursor {
                    offset: 0,
                    anchor: next
                },
                DeleteAction::Line
            )
            .range,
            0..next
        );
        let last = d.table_cell_fragments[2].range.start;
        assert_eq!(
            cell_delete(
                source,
                &d,
                Cursor {
                    offset: 0,
                    anchor: last
                },
                DeleteAction::Line
            )
            .range,
            0..last
        );
        assert_eq!(
            cell_delete(
                "only",
                &decoration("only"),
                Cursor {
                    offset: 2,
                    anchor: 2
                },
                DeleteAction::Line
            )
            .range,
            0..4
        );
        for source in [r"a\<br>b", "`a<br>b`", "$a<br>b$", "<code>a<br>b</code>"] {
            assert_eq!(
                cell_delete(
                    source,
                    &decoration(source),
                    Cursor {
                        offset: 1,
                        anchor: 1
                    },
                    DeleteAction::Line
                )
                .range,
                0..source.len()
            );
        }
    }

    #[test]
    fn cell_edges_are_noops_for_directional_deletes() {
        let source = "first<br>last  ";
        let d = decoration(source);
        for action in [
            DeleteAction::Forward,
            DeleteAction::Word { backwards: false },
            DeleteAction::LineBoundary {
                to_start: false,
                kill: true,
            },
        ] {
            assert!(
                cell_delete(
                    source,
                    &d,
                    Cursor {
                        offset: source.len(),
                        anchor: source.len()
                    },
                    action
                )
                .range
                .is_empty()
            );
        }
        for action in [
            DeleteAction::Word { backwards: true },
            DeleteAction::LineBoundary {
                to_start: true,
                kill: false,
            },
        ] {
            assert!(
                cell_delete(
                    source,
                    &d,
                    Cursor {
                        offset: 0,
                        anchor: 0
                    },
                    action
                )
                .range
                .is_empty()
            );
        }
    }

    #[test]
    fn overlapping_deletions_merge_and_noop_targets_retain_reversed_selections() {
        let mut buffer = Buffer::from_text("0123456789");
        let plans = vec![
            (
                Cursor {
                    offset: 3,
                    anchor: 3,
                },
                true,
                Edit::new(2..6, String::new()),
            ),
            (
                Cursor {
                    offset: 5,
                    anchor: 5,
                },
                false,
                Edit::new(4..8, String::new()),
            ),
            (
                Cursor {
                    offset: 9,
                    anchor: 1,
                },
                false,
                Edit::new(9..9, String::new()),
            ),
        ];
        assert_eq!(deletion_ranges(&plans), [2..8]);
        let resolved = apply_deletions(&mut buffer, &plans);
        assert_eq!(buffer.text(), "0189");
        assert_eq!(resolved[0].0.offset, 2);
        assert_eq!(resolved[1].0.offset, 2);
        assert_eq!(
            resolved[2].0,
            Cursor {
                offset: 3,
                anchor: 1
            }
        );
    }

    #[test]
    fn cutting_an_extra_selection_preserves_the_unselected_primary_caret() {
        let mut buffer = Buffer::from_text("alpha beta gamma");
        let plans = vec![
            (
                Cursor {
                    offset: 2,
                    anchor: 2,
                },
                true,
                Edit::new(2..2, String::new()),
            ),
            (
                Cursor {
                    offset: 10,
                    anchor: 6,
                },
                false,
                Edit::new(6..10, String::new()),
            ),
        ];
        let resolved = apply_deletions(&mut buffer, &plans);
        assert_eq!(buffer.text(), "alpha  gamma");
        assert_eq!(
            resolved[0].0,
            Cursor {
                offset: 2,
                anchor: 2
            }
        );
        assert!(resolved[0].1);
        assert_eq!(
            resolved[1].0,
            Cursor {
                offset: 6,
                anchor: 6
            }
        );
    }

    #[test]
    fn cutting_disjoint_lines_keeps_each_primary_and_secondary_caret() {
        let mut buffer = Buffer::from_text("one\ntwo\nthree\nfour");
        let plans = vec![
            (
                Cursor {
                    offset: 1,
                    anchor: 1,
                },
                true,
                Edit::new(0..4, String::new()),
            ),
            (
                Cursor {
                    offset: 9,
                    anchor: 9,
                },
                false,
                Edit::new(8..14, String::new()),
            ),
        ];
        let resolved = apply_deletions(&mut buffer, &plans);
        assert_eq!(buffer.text(), "two\nfour");
        assert_eq!(
            resolved[0].0,
            Cursor {
                offset: 0,
                anchor: 0
            }
        );
        assert!(resolved[0].1);
        assert_eq!(
            resolved[1].0,
            Cursor {
                offset: 4,
                anchor: 4
            }
        );
    }

    #[test]
    fn angle_pairing_sees_only_the_current_semantic_line() {
        for (source, at, text) in [
            ("", 0, "<"),
            ("  ", 2, "<"),
            ("first<br>", 9, "<"),
            ("first<BR />  ", 12, "<"),
            ("word", 4, "<>"),
            (r"word\", 5, "<"),
            ("first<br>word", 9, "<"),
            ("first<br>- ", 11, "<>"),
            (r"first\<br>", 10, "<>"),
            ("`code<br>tag`", 12, "<>"),
            ("$a<br>b$", 8, "<>"),
        ] {
            let edit = cell_angle_input(source, &decoration(source), at).unwrap();
            assert_eq!(edit.text, text, "{source:?} at {at}");
            assert_eq!(edit.caret, 1);
            assert_eq!(edit.range, at..at);
        }
    }

    #[test]
    fn enter_continues_all_bullets_and_resets_tasks() {
        for marker in [
            "- ", "* ", "+ ", "- [ ] ", "- [x] ", "- [X] ", "* [x] ", "* [X] ", "+ [ ] ", "+ [X] ",
        ] {
            let source = format!("lead<BR />  {marker}先🙂");
            let next = if marker.contains('[') {
                format!("{}[ ] ", &marker[..2])
            } else {
                marker.to_string()
            };
            let expected = format!("{source}<br>  {next}");
            assert_eq!(
                run(&source, source.len(), false),
                (expected.clone(), expected.len())
            );
        }
        assert_eq!(run("9) item", 7, false).0, "9) item<br>10. ");
        assert_eq!(run("9. \titem", 8, false).0, "9. \titem<br>10. ");
    }

    #[test]
    fn uppercase_tasks_share_ordinary_continuation_backspace_and_checked_rendering() {
        for bullet in ["-", "*", "+"] {
            let source = format!("{bullet} [X] done");
            let d = decoration(&source);
            assert_eq!(
                d.table_cell_fragments[0].block_type,
                BlockType::TaskList(true)
            );
            assert_eq!(d.table_cell_fragments[0].prefix_len, 6);
            let expected = format!("{source}<br>{bullet} [ ] ");
            assert_eq!(run(&source, source.len(), false).0, expected);
            let ordinary = enter(
                &source,
                0..source.len(),
                &BlockType::TaskList(true),
                source.len()..source.len(),
                source.len(),
                "\n",
            );
            assert_eq!(ordinary.text, format!("\n{bullet} [ ] "));
            let edit = cell_delete(
                &source,
                &d,
                Cursor {
                    offset: 6,
                    anchor: 6,
                },
                DeleteAction::Backward,
            );
            assert_eq!(edit.range, 0..6);
            assert_eq!(run(&format!("{bullet} [X] "), 6, false).0, "<br>");
        }
    }

    #[test]
    fn ordered_tasks_continue_and_normalize_like_ordered_lists() {
        for (source, next) in [
            ("1. [x] done", "2. [ ] "),
            ("9) [X] done", "10) [ ] "),
            ("9.\t[ ] done", "10.\t[ ] "),
        ] {
            let d = decoration(source);
            assert_eq!(
                d.table_cell_fragments[0].block_type,
                BlockType::TaskList(source.contains("[x]") || source.contains("[X]"))
            );
            let ordinary = enter(
                source,
                0..source.len(),
                &d.table_cell_fragments[0].block_type,
                source.len()..source.len(),
                source.len(),
                "\n",
            );
            assert_eq!(ordinary.text, format!("\n{next}"));
            assert!(ordinary.normalize.is_some());
            assert_eq!(
                run(source, source.len(), false).0,
                format!("{source}<br>{next}")
            );
        }

        let source = "1. [x] first<br>4. [ ] second<br>7. ordinary";
        let d = decoration(source);
        let edits = ordered_edits(source, &d, "1. [x] first".len(), true);
        let mut updated = source.to_string();
        for edit in edits.into_iter().rev() {
            updated.replace_range(edit.range, &edit.text);
        }
        assert_eq!(updated, "1. [x] first<br>2. [ ] second<br>3. ordinary");
    }

    #[test]
    fn empty_items_outdent_then_exit_without_touching_other_fragments() {
        for (source, expected) in [
            ("lead<br>  - ", "lead<br>- "),
            ("lead<br> - [x] \t", "lead<br>- [x] \t"),
            ("lead<br>   12. ", "lead<br>12. "),
            ("lead<br>- ", "lead<br><br>"),
            ("lead<br>- [ ] ", "lead<br><br>"),
            ("lead<br>1. ", "lead<br><br>"),
        ] {
            assert_eq!(
                run(source, source.len(), false),
                (expected.to_string(), expected.len())
            );
        }
    }

    #[test]
    fn soft_break_and_non_list_blocks_never_continue() {
        for source in [
            "- item",
            "- [x] done",
            "1. item",
            "> quote",
            "# title",
            "```rust",
            "---",
            "- [X] done",
        ] {
            assert_eq!(run(source, source.len(), true).0, format!("{source}<br>"));
        }
        for source in ["> quote", "# title", "```rust", "---"] {
            assert_eq!(run(source, source.len(), false).0, format!("{source}<br>"));
        }
    }

    #[test]
    fn selection_split_and_literal_tags_use_semantic_fragments() {
        let source = "- before after<br>+ next";
        let plan = cell_enter(source, &decoration(source), 8..14, 14, false);
        assert_eq!(plan, Edit::new(8..14, "<br>- ".to_string()));
        let plan = cell_enter(
            source,
            &decoration(source),
            8..source.len(),
            source.len(),
            false,
        );
        assert_eq!(plan.text, "<br>");
        for source in [
            r"plain\<br>- text",
            "`a<br>- text`",
            "$a<br>- text$",
            "<code>a<br>- text</code>",
            "[[a<br>- text]]",
        ] {
            assert_eq!(decoration(source).table_cell_fragments.len(), 1, "{source}");
            assert_eq!(run(source, source.len(), false).0, format!("{source}<br>"));
        }
        assert_eq!(run("- text", 1, false).0, "-<br> text");
    }

    #[test]
    fn backspace_removes_prefix_before_join_and_keeps_indentation() {
        for marker in ["- ", "* ", "+ ", "12. ", "- [ ] ", "- [x] "] {
            let source = format!("lead<BR />  {marker}先🙂");
            let buffer = Buffer::from_text(&source);
            let offset = "lead<BR />  ".len() + marker.len();
            let plan = cell_backspace(
                &buffer,
                0..source.len(),
                &decoration(&source),
                Cursor {
                    offset,
                    anchor: offset,
                },
                None,
            );
            assert_eq!(plan.range, "lead<BR />  ".len()..offset);
            let mut buffer = buffer;
            apply(&mut buffer, &[(plan, true)]).unwrap();
            assert_eq!(buffer.text(), "lead<BR />  先🙂");
        }
        let source = "lead<BR />text";
        let buffer = Buffer::from_text(source);
        let offset = 10;
        let plan = cell_backspace(
            &buffer,
            0..source.len(),
            &decoration(source),
            Cursor {
                offset,
                anchor: offset,
            },
            None,
        );
        assert_eq!(plan.range, 4..10);
        let plan = cell_backspace(
            &buffer,
            0..source.len(),
            &decoration(source),
            Cursor {
                offset: 0,
                anchor: 0,
            },
            None,
        );
        assert_eq!(plan.range, 0..0);
    }

    #[test]
    fn tab_uses_list_kind_steps_and_excludes_next_fragment_endpoint() {
        let source = "- first<br>12. next<br>  - [ ] last";
        let d = decoration(source);
        let edits =
            cell_indent(source, &d, 0..d.table_cell_fragments[1].range.start, true).unwrap();
        assert_eq!(edits, vec![Edit::new(0..0, "  ".to_string())]);
        let edits = cell_indent(source, &d, 0..source.len(), true).unwrap();
        assert_eq!(
            edits.iter().map(|e| e.text.len()).collect::<Vec<_>>(),
            [2, 3, 2]
        );
        let edits =
            cell_indent(source, &d, d.table_cell_fragments[2].range.clone(), false).unwrap();
        assert_eq!(edits[0].range.len(), 2);
        assert!(cell_indent("plain", &decoration("plain"), 0..0, true).is_none());
    }

    #[test]
    fn ordered_numbering_is_cell_local_and_skips_nested_items() {
        let source =
            "9. first<br>10. <br>   - child<br>10. old<br>11. tail<br>plain<br>99. separate";
        let d = decoration(source);
        let edits = ordered_edits(source, &d, "9. first<br>10. ".len(), true);
        assert_eq!(
            edits.iter().map(|e| e.text.as_str()).collect::<Vec<_>>(),
            ["11", "12"]
        );
        let source = "9. first<br>plain<br>9. next";
        assert_eq!(
            ordered_edits(source, &decoration(source), 14, false)[0].text,
            "1"
        );
    }

    #[test]
    fn paired_backspace_is_cell_local_for_primary_and_extra_carets() {
        for (source, at, expected) in [
            ("()", 1, 0..2),
            ("[[]]", 2, 0..4),
            ("「」", 3, 0..6),
            ("‘’", 3, 0..6),
        ] {
            let buffer = Buffer::from_text(source);
            let cursor = Cursor {
                offset: at,
                anchor: at,
            };
            let paired = paired_backspace_range(source, cursor);
            assert_eq!(paired, Some(expected.clone()));
            assert_eq!(
                cell_backspace(
                    &buffer,
                    0..source.len(),
                    &decoration(source),
                    cursor,
                    paired
                )
                .range,
                expected
            );
        }
        let source = "left|()|right";
        let buffer = Buffer::from_text(source);
        let cursor = Cursor {
            offset: 6,
            anchor: 6,
        };
        let plan = cell_backspace(&buffer, 5..7, &decoration("()"), cursor, Some(4..8));
        assert_eq!(plan.range, 5..6);
    }

    #[test]
    fn prefix_edits_deduplicate_shared_fragments_and_preserve_reversed_selections() {
        let source = "lead<br>  - é🙂<br>12. x";
        let d = decoration(source);
        let start = d.table_cell_fragments[1].range.start;
        let mut edits = cell_indent(source, &d, start..source.len(), true).unwrap();
        edits.extend(edits.clone());
        let mut buffer = Buffer::from_text(source);
        let mut cursors = [
            (
                Cursor {
                    offset: start + 4,
                    anchor: source.len(),
                },
                true,
            ),
            (
                Cursor {
                    offset: source.len(),
                    anchor: source.len(),
                },
                false,
            ),
        ];
        apply_prefix_edits(&mut buffer, &mut cursors, edits);
        assert_eq!(buffer.text(), "lead<br>    - é🙂<br>   12. x");
        assert_eq!(cursors[0].0.offset, start + 6);
        assert_eq!(cursors[0].0.anchor, buffer.len());
        assert_eq!(cursors[1].0.offset, buffer.len());
        let edits = cell_indent(
            buffer.text(),
            &decoration(buffer.text()),
            start..buffer.len(),
            false,
        )
        .unwrap();
        apply_prefix_edits(&mut buffer, &mut cursors, edits);
        assert_eq!(buffer.text(), source);
        assert_eq!(cursors[0].0.offset, start + 4);
        assert_eq!(cursors[0].0.anchor, source.len());
    }

    #[test]
    fn overlapping_action_plans_leave_source_and_carets_untouched() {
        let mut buffer = Buffer::from_text("先🙂abcdef");
        let before = buffer.text().to_string();
        let start = "先🙂".len();
        assert!(
            apply(
                &mut buffer,
                &[
                    (Edit::new(start..start + 4, String::new()), true),
                    (Edit::new(start + 2..start + 5, "<br>".to_string()), false)
                ]
            )
            .is_none()
        );
        assert_eq!(buffer.text(), before);
    }

    #[test]
    fn normalization_tracks_original_empty_item_and_digit_width_changes() {
        let source = "9. first<br>10. <br>10. next";
        let d = decoration(source);
        let at = d.table_cell_fragments[1].range.end;
        let exit = cell_enter(source, &d, at..at, at, false);
        assert_eq!(exit.normalize, Some((false, 4)));
        let source = "8. first<br>9. new<br>9. old<br>10. tail";
        let d = decoration(source);
        let edits = ordered_edits(source, &d, d.table_cell_fragments[1].range.end, true);
        let mut buffer = Buffer::from_text(source);
        let mut cursors = [(
            Cursor {
                offset: source.len(),
                anchor: source.len(),
            },
            true,
        )];
        apply_prefix_edits(&mut buffer, &mut cursors, edits);
        assert_eq!(buffer.text(), "8. first<br>9. new<br>10. old<br>11. tail");
        assert_eq!(cursors[0].0.offset, buffer.len());
    }

    #[test]
    fn batch_plans_coalesce_same_empty_item_and_remap_utf8_carets() {
        let source = "- <br>+ 先🙂";
        let d = decoration(source);
        let first = cell_enter(source, &d, 2..2, 2, false);
        let last = cell_enter(source, &d, source.len()..source.len(), source.len(), false);
        let mut buffer = Buffer::from_text(source);
        let cursors = apply(
            &mut buffer,
            &[(first.clone(), true), (last, false), (first, false)],
        )
        .unwrap();
        assert_eq!(buffer.text(), "<br><br>+ 先🙂<br>+ ");
        assert_eq!(cursors[0].0.offset, 4);
        assert_eq!(cursors[1].0.offset, buffer.len());
        assert_eq!(cursors[2].0.offset, 4);
    }
}
