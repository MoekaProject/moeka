use std::ops::Range;

use gpui::Context;

use super::*;

/// Row zero remains the source anchor. This is semantic ownership, not the
/// presentation hide range: the opener and summary retain geometry, while the
/// closer leaves the layout when unfocused (`front_matter_line_is_hidden`).
pub(super) fn front_matter_fold_range(
    decorations: &[LineDecoration],
    line: usize,
) -> Option<Range<usize>> {
    let table = decorations.get(line)?.table_info.as_ref()?;
    (table.is_front_matter
        && line == table.table_start_line
        && table.table_end_line > line + 1
        && table.table_end_line <= decorations.len())
    .then_some(line + 1..table.table_end_line)
}

/// Empty/comment-only metadata is valid but the Markdown detector deliberately
/// ignores it. Give it the same editor-only geometry as populated metadata.
pub(super) fn decorate_empty_front_matter(buffer: &Buffer, decorations: &mut [LineDecoration]) {
    if decorations
        .first()
        .is_none_or(|d| d.table_info.as_ref().is_some_and(|t| t.is_front_matter))
        || buffer.line_content(0).trim_start_matches('\u{feff}').trim() != "---"
    {
        return;
    }
    let Some(end) = (1..buffer.line_count())
        .find(|&line| matches!(buffer.line_content(line).trim_end(), "---" | "..."))
    else {
        return;
    };
    if end >= decorations.len()
        || !(1..end).all(|line| {
            let source = buffer.line_content(line).trim();
            source.is_empty() || source.starts_with('#') || source == "{}"
        })
    {
        return;
    }
    let metadata = &buffer.text()[buffer.line_start(1)..buffer.line_start(end)];
    let empty = match serde_yaml::from_str::<serde_yaml::Value>(metadata) {
        Ok(serde_yaml::Value::Null) => true,
        Ok(serde_yaml::Value::Mapping(mapping)) => mapping.is_empty(),
        _ => false,
    };
    if !empty {
        return;
    }
    for (line, decoration) in decorations.iter_mut().enumerate().take(end + 1) {
        *decoration = LineDecoration::default();
        decoration.block_type = BlockType::Table;
        decoration.table_info = Some(crate::markdown::TableLineInfo {
            cells: Vec::new(),
            cell_ranges: Vec::new(),
            column_alignments: Vec::new(),
            is_header: false,
            is_separator: line == 0 || line == end,
            is_front_matter: true,
            table_start_line: 0,
            table_end_line: end + 1,
            column_widths: vec![3, 0],
        });
    }
}

pub(super) fn front_matter_entry_count(buffer: &Buffer, decorations: &[LineDecoration]) -> usize {
    let Some(range) = front_matter_fold_range(decorations, 0) else {
        return 0;
    };
    let start = buffer.line_start(range.start);
    let end = buffer.line_start(range.end - 1);
    if let Ok(serde_yaml::Value::Mapping(mapping)) =
        serde_yaml::from_str(&buffer.text()[start..end])
    {
        return mapping.len();
    }
    // Incomplete YAML and duplicate keys can still have editable metadata.
    // Its lexical ranges exclude scalar bodies and nested flow contents.
    let keys = range.start..range.end.saturating_sub(1);
    let keys: Vec<_> = keys
        .filter_map(|line| {
            let key = decorations
                .get(line)?
                .front_matter
                .as_ref()?
                .key_range
                .as_ref()?;
            buffer.line_content(line)[..key.start]
                .chars()
                .all(char::is_whitespace)
                .then_some(key.start)
        })
        .collect();
    if let Some(root_indent) = keys.iter().min() {
        return keys.iter().filter(|indent| *indent == root_indent).count();
    }
    0
}

pub(super) fn front_matter_fold_summary_line(range: &Range<usize>) -> usize {
    // Empty metadata has no body row, so its fallback summary stays on the opener.
    if range.end > range.start + 1 {
        range.start
    } else {
        range.start - 1
    }
}

/// Only interior rows collapse. The summary row and both delimiters keep their
/// measured geometry, so the summary is always visible and source mapping holds.
pub(super) fn front_matter_fold_hidden_rows(range: &Range<usize>) -> Range<usize> {
    let summary = front_matter_fold_summary_line(range);
    summary + 1..(range.end - 1).max(summary + 1)
}

pub(super) fn front_matter_fold_line_is_hidden(range: &Range<usize>, line: usize) -> bool {
    front_matter_fold_hidden_rows(range).contains(&line)
}

pub(super) fn front_matter_is_closing_delimiter_line(
    decorations: &[LineDecoration],
    line: usize,
) -> bool {
    let Some(block) = decorations
        .first()
        .and_then(|d| d.table_info.as_ref())
        .filter(|table| table.is_front_matter)
    else {
        return false;
    };
    line > block.table_start_line
        && line + 1 == block.table_end_line
        && decorations
            .get(line)
            .and_then(|d| d.table_info.as_ref())
            .is_some_and(|table| table.is_front_matter && table.is_separator)
}

/// The control never sits on a delimiter: it marks the summary row when collapsed,
/// otherwise the first visible property row.
pub(super) fn front_matter_fold_control_line(
    decorations: &[LineDecoration],
    plans: &[helpers::FrontMatterGroupPlan],
    collapsed: bool,
) -> Option<usize> {
    let range = front_matter_fold_range(decorations, 0)?;
    if collapsed || range.end == 2 {
        return Some(front_matter_fold_summary_line(&range));
    }
    let visible = |line: &usize| !plans.iter().any(|plan| plan.hides_line(*line));
    // A multiline property's label may live on its first body row.
    (range.start..range.end - 1)
        .filter(visible)
        .find(|&line| {
            decorations[line]
                .front_matter
                .as_ref()
                .is_some_and(|info| info.key_range.is_some())
                || plans.iter().any(|plan| plan.label_line() == line)
        })
        .or_else(|| (range.start..range.end - 1).find(visible))
}

/// The opener keeps its source fold range for keyboard folding, but the visible
/// control lives on the summary or first property row instead.
pub(super) fn front_matter_opener_hides_fold_control(
    decorations: &[LineDecoration],
    line: usize,
    control_line: Option<usize>,
) -> bool {
    control_line != Some(line) && front_matter_fold_range(decorations, line).is_some()
}

pub(super) fn front_matter_hidden_navigation_offset(
    buffer: &Buffer,
    previous: usize,
    current: usize,
    hidden: impl Fn(usize) -> bool,
) -> Option<usize> {
    let line = buffer.offset_to_line_col(current).0;
    if current == previous || !hidden(line) {
        return None;
    }
    let target = if current < previous {
        (0..line)
            .rev()
            .find(|&line| !hidden(line))
            .map(|line| buffer.line_end(line))
    } else {
        (line + 1..buffer.line_count())
            .find(|&line| !hidden(line))
            .map(|line| buffer.line_start(line))
    };
    Some(target.unwrap_or(previous))
}

fn front_matter_fold_crossing_offset(
    buffer: &Buffer,
    hidden_lines: &Range<usize>,
    previous: usize,
    current: usize,
) -> Option<usize> {
    if !hidden_lines.contains(&buffer.offset_to_line_col(current).0) {
        return None;
    }
    Some(if current < previous {
        buffer.line_end(hidden_lines.start - 1)
    } else {
        buffer.line_start(hidden_lines.end)
    })
}

/// Caret movement past a folded block. A block that ends the document has no line
/// to enter, so downward movement stays on the opener instead of reaching EOF.
pub(super) fn front_matter_fold_navigation_offset(
    buffer: &Buffer,
    hidden_lines: &Range<usize>,
    previous: usize,
    current: usize,
) -> Option<usize> {
    let target = front_matter_fold_crossing_offset(buffer, hidden_lines, previous, current)?;
    Some(
        if current >= previous && hidden_lines.end >= buffer.line_count() {
            buffer.line_end(hidden_lines.start - 1)
        } else {
            target
        },
    )
}

/// Reconciliation is not navigation: nonempty selections own source ranges,
/// even when an endpoint (including EOF) is in folded metadata.
fn reconcile_front_matter_caret(
    buffer: &Buffer,
    hidden: &Range<usize>,
    cursor: &mut Cursor,
) -> bool {
    if cursor.has_selection() || !hidden.contains(&buffer.offset_to_line_col(cursor.offset).0) {
        return false;
    }
    cursor.move_to(buffer.line_end(hidden.start - 1), buffer);
    true
}

pub(super) fn skip_front_matter_selection_head(
    buffer: &Buffer,
    hidden: &Range<usize>,
    previous: usize,
    cursor: &mut Cursor,
) -> bool {
    if cursor.offset == previous {
        return false;
    }
    let Some(offset) = front_matter_fold_crossing_offset(buffer, hidden, previous, cursor.offset)
    else {
        return false;
    };
    cursor.select_to(offset, buffer);
    true
}

/// Movement and Shift movement share direction-aware targets, not anchor semantics.
pub(super) fn skip_front_matter_navigation_head(
    buffer: &Buffer,
    collapsed: Option<&Range<usize>>,
    hidden: impl Fn(usize) -> bool,
    previous: usize,
    cursor: &mut Cursor,
    selecting: bool,
) -> bool {
    if selecting && let Some(range) = collapsed {
        return skip_front_matter_selection_head(buffer, range, previous, cursor);
    }
    if cursor.offset == previous {
        return false;
    }
    let offset = if let Some(range) = collapsed {
        front_matter_fold_navigation_offset(buffer, range, previous, cursor.offset)
    } else {
        front_matter_hidden_navigation_offset(buffer, previous, cursor.offset, hidden)
    };
    let Some(offset) = offset else {
        return false;
    };
    if selecting {
        cursor.select_to(offset, buffer);
    } else {
        cursor.move_to(offset, buffer);
    }
    true
}

const CODE_FOLD_TAB_WIDTH: usize = 4;

#[derive(Clone, Copy, PartialEq, Eq)]
enum RegionMarker {
    Start,
    End,
}

fn leading_indent_columns(line: &str) -> usize {
    let mut columns = 0usize;
    for ch in line.chars() {
        match ch {
            ' ' => columns += 1,
            '\t' => columns += CODE_FOLD_TAB_WIDTH - (columns % CODE_FOLD_TAB_WIDTH),
            _ => break,
        }
    }
    columns
}

fn marker_keyword_matches(text: &str, keyword: &str) -> bool {
    let Some(rest) = text.strip_prefix(keyword) else {
        return false;
    };
    match rest.chars().next() {
        None => true,
        Some(ch) => ch.is_whitespace() || matches!(ch, ':' | '-' | '/' | '*'),
    }
}

fn classify_region_marker(trimmed: &str) -> Option<RegionMarker> {
    let normalized = trimmed.trim_start_matches(|ch: char| {
        ch.is_whitespace() || matches!(ch, '/' | '#' | '*' | ';' | '<' | '!' | '-')
    });
    let lower = normalized.to_ascii_lowercase();
    if marker_keyword_matches(&lower, "endregion") || marker_keyword_matches(&lower, "#endregion") {
        Some(RegionMarker::End)
    } else if marker_keyword_matches(&lower, "region") || marker_keyword_matches(&lower, "#region")
    {
        Some(RegionMarker::Start)
    } else {
        None
    }
}

fn region_fold_range_for_line<'a, F>(
    line_count: usize,
    line: usize,
    line_content: F,
) -> Option<Range<usize>>
where
    F: Fn(usize) -> Option<&'a str>,
{
    if line + 1 >= line_count {
        return None;
    }
    if classify_region_marker(line_content(line)?.trim()) != Some(RegionMarker::Start) {
        return None;
    }

    let mut depth = 1usize;
    for candidate_line in line + 1..line_count {
        let Some(content) = line_content(candidate_line) else {
            break;
        };
        match classify_region_marker(content.trim()) {
            Some(RegionMarker::Start) => depth += 1,
            Some(RegionMarker::End) => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return (candidate_line + 1 > line + 1).then_some(line + 1..candidate_line + 1);
                }
            }
            None => {}
        }
    }

    None
}

fn markup_tag_delta(line: &str, tag: &str) -> isize {
    let mut delta = 0isize;
    let mut index = 0usize;

    while let Some(relative) = line[index..].find('<') {
        let start = index + relative;
        let rest = &line[start + 1..];
        if rest.starts_with("!--") {
            if let Some(comment_end) = line[start + 4..].find("-->") {
                index = start + 4 + comment_end + 3;
            } else {
                break;
            }
            continue;
        }
        if rest.starts_with('!') || rest.starts_with('?') {
            index = start + 2;
            continue;
        }

        let closing = rest.starts_with('/');
        let name_start = start + 1 + usize::from(closing);
        let name_rest = &line[name_start..];
        let name_len = name_rest
            .find(|ch: char| !ch.is_ascii_alphanumeric() && !matches!(ch, ':' | '_' | '-'))
            .unwrap_or(name_rest.len());
        if name_len == 0 {
            index = name_start + 1;
            continue;
        }

        let name = &name_rest[..name_len];
        let tag_matches = name.eq_ignore_ascii_case(tag);
        let token_end = line[name_start + name_len..]
            .find('>')
            .map(|offset| name_start + name_len + offset + 1)
            .unwrap_or(line.len());
        let token = &line[start..token_end];
        let self_closing = !closing && token.trim_end().ends_with("/>");

        if tag_matches {
            if closing {
                delta -= 1;
            } else if !self_closing {
                delta += 1;
            }
        }

        index = token_end.max(start + 1);
    }

    delta
}

fn opening_markup_tag_name(trimmed: &str) -> Option<String> {
    let trimmed = trimmed.trim_start();
    if !trimmed.starts_with('<')
        || trimmed.starts_with("</")
        || trimmed.starts_with("<!--")
        || trimmed.starts_with("<!")
        || trimmed.starts_with("<?")
    {
        return None;
    }

    let rest = &trimmed[1..];
    let name_len = rest
        .find(|ch: char| !ch.is_ascii_alphanumeric() && !matches!(ch, ':' | '_' | '-'))
        .unwrap_or(rest.len());
    if name_len == 0 {
        return None;
    }

    let tag = &rest[..name_len];
    (markup_tag_delta(trimmed, tag) > 0).then(|| tag.to_ascii_lowercase())
}

fn markup_fold_range_for_line<'a, F>(
    line_count: usize,
    line: usize,
    line_content: F,
) -> Option<Range<usize>>
where
    F: Fn(usize) -> Option<&'a str>,
{
    if line + 1 >= line_count {
        return None;
    }

    let tag = opening_markup_tag_name(line_content(line)?.trim_end())?;
    let mut depth = markup_tag_delta(line_content(line)?, &tag);
    for candidate_line in line + 1..line_count {
        let Some(content) = line_content(candidate_line) else {
            break;
        };
        depth += markup_tag_delta(content, &tag);
        if depth <= 0 {
            return (candidate_line + 1 > line + 1).then_some(line + 1..candidate_line + 1);
        }
    }

    None
}

fn brace_fold_range_for_line_impl<'a, F>(
    line_count: usize,
    line: usize,
    trimmed: &str,
    line_content: F,
) -> Option<Range<usize>>
where
    F: Fn(usize) -> Option<&'a str>,
{
    let (open, close) = if trimmed.ends_with('{') {
        ('{', '}')
    } else if trimmed.ends_with('[') {
        ('[', ']')
    } else if trimmed.ends_with('(') {
        ('(', ')')
    } else {
        return None;
    };

    let mut depth = 1usize;
    for candidate_line in line + 1..line_count {
        let Some(candidate) = line_content(candidate_line) else {
            break;
        };
        for ch in candidate.chars() {
            if ch == open {
                depth += 1;
            } else if ch == close {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return (candidate_line + 1 > line + 1).then_some(line + 1..candidate_line + 1);
                }
            }
        }
    }

    None
}

fn indent_fold_range_for_line<'a, F>(
    line_count: usize,
    line: usize,
    line_content: F,
) -> Option<Range<usize>>
where
    F: Fn(usize) -> Option<&'a str>,
{
    if line + 1 >= line_count {
        return None;
    }

    let indent = leading_indent_columns(line_content(line)?);
    let mut next_line = line + 1;
    while line_content(next_line)?.trim().is_empty() {
        next_line += 1;
        if next_line >= line_count {
            return None;
        }
    }
    if leading_indent_columns(line_content(next_line)?) <= indent {
        return None;
    }

    let end = (next_line..line_count)
        .find(|&idx| {
            line_content(idx).is_none_or(|candidate| {
                !candidate.trim().is_empty() && leading_indent_columns(candidate) <= indent
            })
        })
        .unwrap_or(line_count);

    (end > line + 1).then_some(line + 1..end)
}

fn generic_code_fold_range_for_line_impl<'a, F>(
    line_count: usize,
    line: usize,
    line_content: F,
) -> Option<Range<usize>>
where
    F: Fn(usize) -> &'a str + std::marker::Copy,
{
    bounded_code_fold_range_for_line(line_count, line, |idx| Some(line_content(idx)))
}

/// `None` is a structural boundary, not an empty row. This lets Source reuse
/// code folding without scanning for the end of a code block on every row.
fn bounded_code_fold_range_for_line<'a, F>(
    line_count: usize,
    line: usize,
    line_content: F,
) -> Option<Range<usize>>
where
    F: Fn(usize) -> Option<&'a str> + std::marker::Copy,
{
    if line + 1 >= line_count {
        return None;
    }

    let content = line_content(line)?;
    let trimmed = content.trim_end();
    if trimmed.trim().is_empty() {
        return None;
    }

    region_fold_range_for_line(line_count, line, line_content)
        .or_else(|| brace_fold_range_for_line_impl(line_count, line, trimmed, line_content))
        .or_else(|| markup_fold_range_for_line(line_count, line, line_content))
        .or_else(|| indent_fold_range_for_line(line_count, line, line_content))
}

/// Source folds hide complete source rows, never a rich preview or metadata
/// summary. Heading boundaries are cached by the outline collector; only a
/// candidate block opener or a code-fold candidate scans its own body.
fn source_fold_range_for_line(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    outline: &outline::SourceOutline,
    line: usize,
) -> Option<Range<usize>> {
    let headings = &outline.headings;
    let math_ranges = &outline.math_ranges;
    let line_count = buffer.line_count();
    let decoration = decorations.get(line)?;
    if let Some(range) = front_matter_fold_range(decorations, line) {
        return Some(range);
    }
    if let Ok(index) = headings.binary_search_by_key(&line, |heading| heading.line) {
        let level = headings[index].level;
        let end = headings[index + 1..]
            .iter()
            .find(|heading| heading.level <= level)
            .map_or(line_count, |heading| heading.line);
        return (end > line + 1).then_some(line + 1..end);
    }
    let math_index = math_ranges.partition_point(|range| range.end <= line);
    if let Some(range) = math_ranges.get(math_index)
        && range.contains(&line)
    {
        return (range.start == line && range.end > line + 1).then_some(line + 1..range.end);
    }
    if let Some(preview) = decoration.block_preview.as_ref() {
        if preview.start_line == line {
            let end = preview.end_line.saturating_add(1).min(line_count);
            return (end > line + 1).then_some(line + 1..end);
        }
        // Math is literal in Source but is not a programming-language block.
        if preview.kind == crate::markdown::BlockPreviewKind::Math {
            return None;
        }
    }
    if let Ok(index) = outline
        .code_ranges
        .binary_search_by_key(&line, |range| range.start)
    {
        let end = outline.code_ranges[index].end;
        return (end > line + 1).then_some(line + 1..end);
    }
    match decoration.block_type {
        // Closing fences can have quote/list syntax ranges too; those do not
        // make them openers. The cached source spans own that distinction.
        BlockType::CodeBlockFence => None,
        BlockType::CodeBlock => bounded_code_fold_range_for_line(line_count, line, |row| {
            decorations
                .get(row)
                .filter(|decoration| decoration.block_type == BlockType::CodeBlock)
                .map(|_| buffer.line_content(row))
        }),
        BlockType::UnorderedList | BlockType::OrderedList | BlockType::TaskList(_) => {
            indent_fold_range_for_line(line_count, line, |row| Some(buffer.line_content(row)))
        }
        BlockType::Blockquote => {
            let end = (line + 1..line_count)
                .find(|&row| !buffer.line_content(row).trim_start().starts_with('>'))
                .unwrap_or(line_count);
            (end > line + 1).then_some(line + 1..end)
        }
        _ => None,
    }
}

/// Search needs source ownership (including the anchor), not just hidden rows.
/// Keep this separate from the presentation fold range used by ordinary layout.
fn search_fold_source_range(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    anchor: usize,
    headings: Option<&[(usize, u8)]>,
    fallback: impl FnOnce() -> Option<Range<usize>>,
) -> Option<Range<usize>> {
    let decoration = decorations.get(anchor)?;
    if let Some(range) = front_matter_fold_range(decorations, anchor) {
        return Some(anchor..range.end);
    }
    if let Some(preview) = decoration.block_preview.as_ref()
        && preview.start_line == anchor
    {
        return Some(anchor..preview.end_line.saturating_add(1).min(buffer.line_count()));
    }
    let heading_level = if let Some(headings) = headings {
        headings
            .binary_search_by_key(&anchor, |&(line, _)| line)
            .ok()
            .map(|index| headings[index].1)
    } else if let BlockType::Heading(level) = decoration.block_type {
        Some(level)
    } else {
        None
    };
    if let Some(level) = heading_level {
        let end = if let Some(headings) = headings {
            headings[headings.partition_point(|&(line, _)| line <= anchor)..]
                .iter()
                .find(|&&(_, next_level)| next_level <= level)
                .map_or(buffer.line_count(), |&(line, _)| line)
        } else {
            fallback().map_or(anchor + 1, |range| range.end)
        };
        return Some(anchor..end);
    }
    if headings.is_some() && matches!(decoration.block_type, BlockType::Heading(_)) {
        // A locally parsed fake heading inside a long math block is not a
        // structural heading merely because its opener was outside that chunk.
        return None;
    }
    if decoration.block_type == BlockType::Blockquote {
        // Lazy decorations can stop halfway through a quote. Explicit quoted
        // source rows remain its container even when their chunks are unparsed.
        let end = (anchor + 1..buffer.line_count())
            .find(|&line| !buffer.line_content(line).trim_start().starts_with('>'))
            .unwrap_or(buffer.line_count());
        return Some(anchor..end);
    }
    fallback().map(|range| anchor..range.end)
}

fn search_fold_anchors_to_unfold(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    anchors: &[usize],
    lines: &Range<usize>,
    headings: Option<&[(usize, u8)]>,
    mut fold_range: impl FnMut(usize) -> Option<Range<usize>>,
) -> Vec<usize> {
    if lines.is_empty() {
        return Vec::new();
    }
    anchors
        .iter()
        .copied()
        .filter(|&anchor| anchor < lines.end)
        .filter(|&anchor| {
            search_fold_source_range(buffer, decorations, anchor, headings, || fold_range(anchor))
                .is_some_and(|range| range.start < lines.end && lines.start < range.end)
        })
        .collect()
}

fn source_fold_structural_content(content: &str) -> Option<(&str, usize)> {
    let leading = content.bytes().take_while(|byte| *byte == b' ').count();
    if leading > 3 {
        return None;
    }
    let mut content = &content[leading..];
    let mut quote_depth = 0;
    while let Some(rest) = content.strip_prefix('>') {
        quote_depth += 1;
        content = rest.strip_prefix([' ', '\t']).unwrap_or(rest);
        let leading = content.bytes().take_while(|byte| *byte == b' ').count();
        if leading > 3 {
            return None;
        }
        content = &content[leading..];
    }
    Some((content, quote_depth))
}

/// Strip only the containers that owned the opener. Additional `>` characters
/// inside a fence are literal code, not permission to close a different fence.
fn source_fold_quote_content(mut content: &str, quote_depth: usize) -> Option<&str> {
    for _ in 0..quote_depth {
        let leading = content.bytes().take_while(|byte| *byte == b' ').count();
        if leading > 3 {
            return None;
        }
        content = content[leading..].strip_prefix('>')?;
        content = content.strip_prefix([' ', '\t']).unwrap_or(content);
    }
    Some(content)
}

#[derive(Clone, Copy)]
struct SourceCodeFence {
    marker: u8,
    run: usize,
    indent: usize,
    quote_depth: usize,
    start: usize,
}

fn search_fold_list_prefix_len(content: &str) -> usize {
    if content
        .as_bytes()
        .first()
        .is_some_and(|byte| matches!(byte, b'-' | b'*' | b'+'))
        && content
            .as_bytes()
            .get(1)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
    {
        content.len() - content[1..].trim_start_matches([' ', '\t']).len()
    } else {
        crate::markdown::ordered_list_prefix_len(content)
    }
}

fn search_fold_code_fence(content: &str) -> Option<(u8, usize)> {
    let marker = *content.as_bytes().first()?;
    if !matches!(marker, b'`' | b'~') {
        return None;
    }
    let run = content.bytes().take_while(|byte| *byte == marker).count();
    (run >= 3).then_some((marker, run))
}

/// The outline cache cannot supply these boundaries unchanged: it tracks code
/// fences, but YAML/math can contain both fake headings and fake code fences.
/// Scan source structure only; never decorate the anchor-to-match interval or
/// publish context-filtered headings into the shared incremental outline cache.
pub(super) fn search_fold_heading_boundaries(
    buffer: &Buffer,
    decorations: &[LineDecoration],
) -> Vec<(usize, u8)> {
    source_fold_structure(buffer, decorations).0
}

/// A cached Source index also needs closed math spans: a viewport fragment can
/// end before the closer, and its local parser metadata is not a block boundary.
pub(super) fn source_fold_structure(
    buffer: &Buffer,
    decorations: &[LineDecoration],
) -> (Vec<(usize, u8)>, Vec<Range<usize>>, Vec<Range<usize>>) {
    let mut headings = Vec::new();
    let mut math_ranges = Vec::new();
    let mut code_ranges = Vec::new();
    // Match the editor parser's pending four-tilde policy, not just CommonMark.
    let mut pending_tilde = None;
    for line in 0..buffer.line_count() {
        if buffer
            .line_content(line)
            .trim_start_matches([' ', '\t'])
            .trim_end()
            == "~~~~"
        {
            pending_tilde = if pending_tilde.is_none() {
                Some(line)
            } else {
                None
            };
        }
    }
    let mut code_fence: Option<SourceCodeFence> = None;
    let mut math: Option<(&str, usize, usize)> = None;
    let mut previous_text_line = None;
    let mut preview_end = 0;
    let first_line = front_matter_fold_range(decorations, 0).map_or_else(
        || {
            // Line zero need not have been visited yet. Resolve only a closed
            // leading metadata block, never the gap to an offscreen heading.
            let Some(range) = leading_front_matter_line_range(buffer) else {
                return 0;
            };
            if range.end < 2 {
                return 0;
            }
            let closer = buffer.line_content(range.end - 1).trim_end();
            let compact = buffer.line_content(0).trim_start_matches('\u{feff}').trim() == "-";
            if !(if compact {
                closer == "-"
            } else {
                matches!(closer, "---" | "...")
            }) {
                return 0;
            }
            let end = if range.end < buffer.line_count() {
                buffer.line_start(range.end)
            } else {
                buffer.len()
            };
            let mut leading = crate::markdown::parse_decorations(&buffer.text()[..end]);
            decorate_empty_front_matter(buffer, &mut leading);
            front_matter_fold_range(&leading, 0).map_or(0, |range| range.end)
        },
        |range| range.end,
    );
    for line in first_line..buffer.line_count() {
        if line < preview_end {
            continue;
        }
        let source = buffer.line_content(line);
        let structural = source_fold_structural_content(source);
        let content = if pending_tilde == Some(line) {
            Some("text")
        } else {
            structural.map(|(content, _)| content)
        };
        if code_fence.is_none()
            && math.is_none()
            && let Some(preview) = decorations
                .get(line)
                .and_then(|row| row.block_preview.as_ref())
            && preview.start_line == line
        {
            preview_end = preview.end_line.saturating_add(1);
            if preview.kind == crate::markdown::BlockPreviewKind::Math {
                math_ranges.push(line..preview_end);
            } else {
                code_ranges.push(line..preview_end);
            }
            previous_text_line = None;
            continue;
        }
        if let Some(fence) = code_fence {
            let body = source_fold_quote_content(source, fence.quote_depth);
            let container_ended = body.is_none_or(|body| {
                !body.trim().is_empty()
                    && body.len() - body.trim_start_matches(' ').len() < fence.indent
            });
            if container_ended {
                code_ranges.push(fence.start..line);
                code_fence = None;
            } else {
                let closer = body
                    .filter(|_| pending_tilde != Some(line))
                    .and_then(|body| {
                        let content = body.get(fence.indent..)?;
                        let leading = content.bytes().take_while(|byte| *byte == b' ').count();
                        (leading <= 3).then_some(&content[leading..])
                    });
                if let Some(content) = closer
                    && search_fold_code_fence(content)
                        .is_some_and(|(next, count)| next == fence.marker && count >= fence.run)
                    && content
                        .trim_end_matches([' ', '\t', '\r'])
                        .bytes()
                        .all(|byte| byte == fence.marker)
                {
                    code_ranges.push(fence.start..line + 1);
                    code_fence = None;
                }
                previous_text_line = None;
                continue;
            }
        }
        if let Some((closer, heading_count, start)) = math {
            if source.trim() == closer {
                headings.truncate(heading_count);
                math_ranges.push(start..line + 1);
                math = None;
                previous_text_line = None;
                continue;
            }
            // Remember candidate headings until a closer proves this really is
            // math. An unmatched opener must not swallow subsequent sections.
        }
        if let Some(content) = content {
            let list_prefix = search_fold_list_prefix_len(content);
            let fence_content = &content[list_prefix..];
            if let Some((marker, run)) = search_fold_code_fence(fence_content)
                && (marker != b'`' || !fence_content[run..].contains('`'))
            {
                let quote_depth = structural.map_or(0, |(_, depth)| depth);
                let body = source_fold_quote_content(source, quote_depth).unwrap_or(source);
                let indent = if list_prefix == 0 {
                    0
                } else {
                    body.len() - body.trim_start_matches(' ').len() + list_prefix
                };
                code_fence = Some(SourceCodeFence {
                    marker,
                    run,
                    indent,
                    quote_depth,
                    start: line,
                });
                previous_text_line = None;
                continue;
            }
        }
        if math.is_none() {
            if matches!(source.trim(), "$$" | "\\[") {
                math = Some((
                    if source.trim() == "$$" { "$$" } else { "\\]" },
                    headings.len(),
                    line,
                ));
                previous_text_line = None;
                continue;
            }
        }
        let Some(content) = content else {
            previous_text_line = None;
            continue;
        };
        let list_prefix = search_fold_list_prefix_len(content);
        let heading_content = &content[list_prefix..];
        let marker_len = heading_content
            .bytes()
            .take_while(|byte| *byte == b'#')
            .count();
        if (1..=6).contains(&marker_len)
            && heading_content
                .as_bytes()
                .get(marker_len)
                .is_none_or(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
        {
            let level = marker_len as u8;
            if list_prefix == 0
                || crate::markdown::parse_decorations_fragment(source)
                    .first()
                    .is_some_and(|row| row.block_type == BlockType::Heading(level))
            {
                headings.push((line, level));
            }
            previous_text_line = None;
            continue;
        }
        let trimmed = content.trim();
        let setext_level = if !trimmed.is_empty() && trimmed.bytes().all(|byte| byte == b'=') {
            Some(1)
        } else if !trimmed.is_empty() && trimmed.bytes().all(|byte| byte == b'-') {
            Some(2)
        } else {
            None
        };
        if let Some(level) = setext_level {
            if let Some(previous) = previous_text_line.take() {
                // Validate only two rows, never an arbitrarily long paragraph.
                let candidate = format!("{}\n{}", buffer.line_content(previous), source);
                if crate::markdown::parse_decorations_fragment(&candidate)
                    .first()
                    .is_some_and(|row| row.block_type == BlockType::Heading(level))
                {
                    // Decorations expose each content row of a multiline
                    // setext heading as a possible fold anchor.
                    headings.extend((previous..line).map(|row| (row, level)));
                }
            }
            continue;
        }
        let list_item = content
            .as_bytes()
            .first()
            .is_some_and(|byte| matches!(byte, b'-' | b'*' | b'+'))
            && content
                .as_bytes()
                .get(1)
                .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
            || crate::markdown::ordered_list_prefix_len(content) > 0;
        if trimmed.is_empty() || list_item {
            previous_text_line = None;
        } else {
            previous_text_line.get_or_insert(line);
        }
    }
    if let Some(fence) = code_fence {
        code_ranges.push(fence.start..buffer.line_count());
    }
    (headings, math_ranges, code_ranges)
}

impl EditorView {
    /// Keep the RefCell guard alive for the entire metadata read. A mapped Ref
    /// cannot also borrow the fallback slice from `self` outside that RefCell.
    pub(super) fn with_canonical_markdown_decorations<T>(
        &self,
        read: impl FnOnce(&[LineDecoration]) -> T,
    ) -> T {
        let cache = self.source_syntax_context.borrow();
        read(
            cache
                .markdown_decorations
                .as_deref()
                .unwrap_or(&self.decorations),
        )
    }

    /// Resolve every container before removing any fold. Only folded anchors
    /// need Markdown decoration; intervening headings use a lexical source scan.
    pub(super) fn unfold_search_match(&mut self, lines: Range<usize>) {
        if lines.is_empty() || self.folded_sections.is_empty() {
            return;
        }
        let anchors: Vec<_> = self
            .folded_sections
            .iter()
            .copied()
            .filter(|&anchor| anchor < lines.end && anchor < self.buffer.line_count())
            .collect();
        if anchors.is_empty() {
            return;
        }
        for &anchor in &anchors {
            self.ensure_lazy_markdown_decorations(anchor..anchor + 1);
        }
        // A structural edit can leave the worker snapshot pending. Resolve it
        // on explicit search/outline navigation, never from a gutter/layout read.
        let pending_source = if self.source_mode
            && self.active_tab_uses_markdown_editor()
            && self
                .markdown_outline_index
                .borrow()
                .as_ref()
                .is_none_or(|index| index.source.is_none())
        {
            Some(self.with_canonical_markdown_decorations(|decorations| {
                outline::source_outline_data(
                    &self.buffer,
                    decorations,
                    self.uses_lazy_markdown_decorations(),
                )
            }))
        } else {
            None
        };
        let headings = if self.source_mode && self.active_tab_uses_markdown_editor() {
            let index = self.markdown_outline_index.borrow();
            pending_source
                .as_ref()
                .or_else(|| index.as_ref()?.source.as_ref())
                .map(|source| {
                    source
                        .headings
                        .iter()
                        .map(|heading| (heading.line, heading.level))
                        .collect()
                })
        } else if self.active_tab_uses_markdown_editor() && self.uses_lazy_markdown_decorations() {
            // Metadata is document-leading and can contain fence-like scalars.
            // Resolve only its anchor, rather than parsing intervening chunks.
            self.ensure_lazy_markdown_decorations(0..1);
            Some(self.with_canonical_markdown_decorations(|decorations| {
                search_fold_heading_boundaries(&self.buffer, decorations)
            }))
        } else {
            None
        };
        let unfold = self.with_canonical_markdown_decorations(|decorations| {
            search_fold_anchors_to_unfold(
                &self.buffer,
                decorations,
                &anchors,
                &lines,
                headings.as_deref(),
                |anchor| {
                    if let Some(source) = pending_source.as_ref() {
                        source_fold_range_for_line(&self.buffer, decorations, source, anchor)
                    } else {
                        self.fold_range_for_line(anchor)
                    }
                },
            )
        });
        for anchor in unfold {
            self.folded_sections.remove(&anchor);
        }
    }

    pub(crate) fn fold_range_for_line(&self, line: usize) -> Option<std::ops::Range<usize>> {
        if self.source_mode && self.active_tab_uses_markdown_editor() {
            return self.with_canonical_markdown_decorations(|decorations| {
                let index = self.markdown_outline_index.borrow();
                source_fold_range_for_line(
                    &self.buffer,
                    decorations,
                    index.as_ref()?.source.as_ref()?,
                    line,
                )
            });
        }
        if self.active_tab_is_markdown()
            && let Some(range) = front_matter_fold_range(&self.decorations, line)
        {
            return Some(range);
        }
        let decoration = self.decorations.get(line)?;
        let line_count = self.buffer.line_count();
        match &decoration.block_type {
            BlockType::Heading(level) => {
                let lv = *level;
                let start = line + 1;
                let end = (start..line_count)
                    .find(|&l| {
                        matches!(
                            self.decorations.get(l).map(|d| &d.block_type),
                            Some(BlockType::Heading(h)) if *h <= lv
                        )
                    })
                    .unwrap_or(line_count);
                if end > start { Some(start..end) } else { None }
            }
            BlockType::CodeBlockFence => {
                if line > 0
                    && matches!(
                        self.decorations.get(line - 1).map(|d| &d.block_type),
                        Some(BlockType::CodeBlock | BlockType::CodeBlockFence)
                    )
                {
                    return None;
                }
                let start = line + 1;
                let end = (start..line_count)
                    .find(|&l| {
                        matches!(
                            self.decorations.get(l).map(|d| &d.block_type),
                            Some(BlockType::CodeBlockFence)
                        )
                    })
                    .map(|l| l + 1)
                    .unwrap_or(line_count);
                if end > start { Some(start..end) } else { None }
            }
            BlockType::UnorderedList | BlockType::OrderedList | BlockType::TaskList(_) => {
                let content = self.buffer.line_content(line);
                let indent = content.len() - content.trim_start_matches([' ', '\t']).len();
                if line + 1 >= line_count {
                    return None;
                }
                let next_content = self.buffer.line_content(line + 1);
                let next_indent =
                    next_content.len() - next_content.trim_start_matches([' ', '\t']).len();
                if next_indent <= indent {
                    return None;
                }
                let start = line + 1;
                let end = (start..line_count)
                    .find(|&l| {
                        let lc = self.buffer.line_content(l);
                        let li = lc.len() - lc.trim_start_matches([' ', '\t']).len();
                        !lc.trim().is_empty() && li <= indent
                    })
                    .unwrap_or(line_count);
                if end > start { Some(start..end) } else { None }
            }
            BlockType::Blockquote => {
                let start = line + 1;
                let end = (start..line_count)
                    .find(|&l| {
                        !matches!(
                            self.decorations.get(l).map(|d| &d.block_type),
                            Some(BlockType::Blockquote)
                        )
                    })
                    .unwrap_or(line_count);
                if end > start { Some(start..end) } else { None }
            }
            _ => {
                if self.active_tab_is_non_markdown_text() {
                    self.generic_code_fold_range_for_line(line)
                } else {
                    None
                }
            }
        }
    }

    fn generic_code_fold_range_for_line(&self, line: usize) -> Option<std::ops::Range<usize>> {
        generic_code_fold_range_for_line_impl(self.buffer.line_count(), line, |idx| {
            self.buffer.line_content(idx)
        })
    }

    pub(super) fn collapsed_front_matter_range(&self) -> Option<Range<usize>> {
        if !self.active_tab_is_markdown() || !self.folded_sections.contains(&0) {
            return None;
        }
        front_matter_fold_range(&self.decorations, 0)
    }

    pub(super) fn front_matter_has_summary(&self) -> bool {
        self.collapsed_front_matter_range().is_some()
            || (front_matter_fold_range(&self.decorations, 0).is_some_and(|range| range.end == 2)
                && !self.front_matter_delimiters_are_visible())
    }

    pub(super) fn front_matter_summary_line(&self) -> Option<usize> {
        self.front_matter_has_summary().then(|| {
            front_matter_fold_summary_line(&front_matter_fold_range(&self.decorations, 0).unwrap())
        })
    }

    pub(super) fn front_matter_fold_summary(&self, line: usize) -> Option<String> {
        (self.front_matter_summary_line() == Some(line)).then(|| {
            // The summary row's existing shaping cache also caches the YAML count.
            // Reparse and fold/layout changes invalidate it; no parallel state is needed.
            if !self.front_matter_group_geometry_dirty
                && let Some(info) = self
                    .line_display_info
                    .get(line)
                    .and_then(|info| info.as_ref())
                && info.display_to_buffer.len() == info.display_text.len() + 1
                && let Some(count) = info
                    .display_text
                    .split(|ch: char| !ch.is_ascii_digit())
                    .find(|part| !part.is_empty())
                    .and_then(|part| part.parse().ok())
                && info.display_text
                    == crate::i18n::front_matter_fold_summary(self.ui_language, count)
            {
                return info.display_text.clone();
            }
            crate::i18n::front_matter_fold_summary(
                self.ui_language,
                front_matter_entry_count(&self.buffer, &self.decorations),
            )
        })
    }

    pub(super) fn front_matter_line_is_hidden(&self, line: usize) -> bool {
        self.collapsed_front_matter_range()
            .is_some_and(|range| front_matter_fold_line_is_hidden(&range, line))
            || (front_matter_is_closing_delimiter_line(&self.decorations, line)
                && !self.front_matter_delimiters_are_visible())
    }

    pub(super) fn reconcile_front_matter_fold_cursor(&mut self) -> bool {
        let Some(range) = self.collapsed_front_matter_range() else {
            return false;
        };
        let start = self.buffer.line_start(range.start);
        let end = self.buffer.line_start(range.end);
        let hidden = |offset| range.contains(&self.buffer.offset_to_line_col(offset).0);
        let marked_hidden = self
            .marked_ranges()
            .any(|marked| hidden(marked.start) || (marked.start < end && marked.end > start));
        let composing_hidden = marked_hidden
            || (self.ime_composition_snapshot.is_some()
                && std::iter::once(&self.cursor)
                    .chain(&self.extra_cursors)
                    .any(|cursor| hidden(cursor.offset) || hidden(cursor.anchor)));
        if composing_hidden {
            // Keep a tombstone before clearing source ownership. Cocoa may send
            // a range-less commit/update after the caret has been relocated.
            self.fold_ime_cancellation.arm();
            self.clear_marked_ranges();
            self.finish_ime_composition();
        }
        let mut moved = reconcile_front_matter_caret(&self.buffer, &range, &mut self.cursor);
        for cursor in &mut self.extra_cursors {
            moved |= reconcile_front_matter_caret(&self.buffer, &range, cursor);
        }
        if moved || composing_hidden {
            self.reset_cursor_visual_targeting();
            self.is_selecting = false;
            self.selection_drag_position = None;
            self.selection_drag_scroll_task = None;
            self.last_mouse_down_target = None;
        }
        moved || composing_hidden
    }

    pub(crate) fn toggle_fold(&mut self, line: usize, cx: &mut Context<Self>) {
        self.ensure_lazy_markdown_decorations(line..line.saturating_add(1));
        let is_front_matter = self.active_tab_is_markdown()
            && front_matter_fold_range(&self.decorations, line).is_some();
        let mut cursor_moved = false;
        if self.folded_sections.contains(&line) {
            self.folded_sections.remove(&line);
        } else if let Some(range) = self.fold_range_for_line(line) {
            self.folded_sections.insert(line);
            let cursor_line = self.cursor_line();
            if is_front_matter {
                cursor_moved = self.reconcile_front_matter_fold_cursor();
            } else if range.contains(&cursor_line) {
                let new_offset = self.buffer.line_end(line);
                self.cursor.move_to(new_offset, &self.buffer);
                cursor_moved = true;
            }
        }
        self.clear_line_display_info();
        if is_front_matter {
            self.layout_dirty = true;
        }
        if cursor_moved {
            self.scroll_to_cursor();
        } else {
            cx.notify();
        }
        self.pause_blinking(cx);
    }

    pub(crate) fn toggle_generated_preview_fold(&mut self, line: usize, cx: &mut Context<Self>) {
        let Some(preview) = self
            .decorations
            .get(line)
            .and_then(|decoration| decoration.block_preview.as_ref())
            .cloned()
            .filter(|preview| preview.is_anchor)
        else {
            self.toggle_fold(line, cx);
            return;
        };

        let selection_ranges = std::iter::once(self.cursor.selection_range())
            .chain(
                self.extra_cursors
                    .iter()
                    .map(|cursor| cursor.selection_range()),
            )
            .collect::<Vec<_>>();
        let preview_is_active =
            helpers::block_preview_is_active(&preview, self.cursor_line(), &selection_ranges);

        if self
            .preview_overlay_state()
            .is_some_and(|overlay| overlay.line_idx == preview.start_line)
        {
            self.close_preview_overlay();
        }

        if self.folded_sections.contains(&preview.start_line) {
            self.folded_sections.remove(&preview.start_line);
        } else if let Some(range) = self.fold_range_for_line(preview.start_line) {
            self.folded_sections.insert(preview.start_line);
            if range.contains(&self.cursor_line()) || preview_is_active {
                let new_offset = self.buffer.line_end(preview.start_line);
                self.cursor.move_to(new_offset, &self.buffer);
                self.scroll_to_cursor();
            }
        }

        self.clear_line_display_info();
        cx.notify();
        self.pause_blinking(cx);
    }

    pub(crate) fn toggle_image_fold(&mut self, line: usize, cx: &mut Context<Self>) {
        if !self.folded_image_previews.remove(&line) {
            self.folded_image_previews.insert(line);
        }
        self.clear_line_display_info();
        cx.notify();
    }

    pub(crate) fn pick_image_for_line(&self, line: usize, cx: &mut Context<Self>) {
        if self.active_tab_is_read_only() {
            return;
        }
        cx.spawn(async move |this, cx| {
            #[cfg(target_os = "macos")]
            let maybe_path = match this.update(cx, |_, cx| {
                cx.prompt_for_paths(gpui::PathPromptOptions {
                    files: true,
                    directories: false,
                    multiple: false,
                    prompt: None,
                })
            }) {
                Ok(receiver) => receiver
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .flatten()
                    .and_then(|mut paths| paths.pop())
                    .filter(|path| super::is_image_path(path)),
                Err(_) => None,
            };
            #[cfg(not(target_os = "macos"))]
            let maybe_path = cx
                .background_executor()
                .spawn(async move { helpers::pick_image_file_blocking() })
                .await;
            if let Some(path) = maybe_path {
                this.update(cx, |editor, cx| {
                    editor.replace_image_url_on_line(line, &path, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    pub(crate) fn replace_image_url_on_line(
        &mut self,
        line: usize,
        new_path: &PathBuf,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab_is_read_only() {
            return;
        }
        let line_content = self.buffer.line_content(line).to_string();
        let line_start = self.buffer.line_start(line);
        if let Some(bracket_pos) = line_content.find("](") {
            let url_start = bracket_pos + 2;
            if let Some(close_pos) = line_content[url_start..].find(')') {
                let url_end = url_start + close_pos;
                let old_url = line_content[url_start..url_end].to_string();
                let new_url = new_path.to_string_lossy().to_string();
                self.checkpoint();
                self.buffer
                    .replace(line_start + url_start..line_start + url_end, &new_url);
                self.retire_image_cache_entry(&old_url);
                self.reparse();
                self.pause_blinking(cx);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::helpers;
    use super::{
        decorate_empty_front_matter, front_matter_entry_count, front_matter_fold_control_line,
        front_matter_fold_line_is_hidden, front_matter_fold_range, front_matter_fold_summary_line,
        generic_code_fold_range_for_line_impl,
    };
    use crate::{buffer::Buffer, cursor::Cursor, markdown::parse_decorations};

    fn lazy_search_heading_unfold(
        buffer: &Buffer,
        anchors: &[usize],
        lines: std::ops::Range<usize>,
    ) -> (
        Vec<usize>,
        Vec<super::LineDecoration>,
        super::LazyMarkdownDecorations,
    ) {
        let (mut decorations, mut lazy) =
            super::super::new_lazy_markdown_decorations(buffer.line_count());
        super::super::parse_lazy_markdown_range(buffer, &mut decorations, &mut lazy, lines.clone());
        for &anchor in anchors.iter().filter(|&&anchor| anchor < lines.end) {
            super::super::parse_lazy_markdown_range(
                buffer,
                &mut decorations,
                &mut lazy,
                anchor..anchor + 1,
            );
        }
        super::super::parse_lazy_markdown_range(buffer, &mut decorations, &mut lazy, 0..1);
        let headings = super::search_fold_heading_boundaries(buffer, &decorations);
        let unfold = super::search_fold_anchors_to_unfold(
            buffer,
            &decorations,
            anchors,
            &lines,
            Some(&headings),
            |_| panic!("lazy heading ownership must not use decoration-only fold bounds"),
        );
        (unfold, decorations, lazy)
    }

    #[test]
    fn search_unfold_lazy_peer_does_not_open_preceding_section_or_parse_the_gap() {
        let mut rows = vec![""; 4096];
        rows[0] = "# A";
        rows[2048] = "# B";
        rows[3072] = "target 中🌸";
        let buffer = Buffer::from_text(&rows.join("\n"));
        let (unfold, decorations, lazy) = lazy_search_heading_unfold(&buffer, &[0], 3072..3073);
        assert!(unfold.is_empty(), "a hit in B must leave A folded");
        assert_eq!(decorations[2048].block_type, super::BlockType::Normal);
        assert!(!lazy.is_parsed(2048 / super::super::LARGE_MARKDOWN_CHUNK_LINES));
        assert_eq!(
            lazy.parsed_chunks.iter().filter(|&&parsed| parsed).count(),
            2
        );
    }

    #[test]
    fn search_unfold_lazy_long_math_does_not_turn_an_interior_heading_into_a_peer() {
        for (opener, closer) in [("$$", "$$"), ("\\[", "\\]")] {
            let mut rows = vec![""; 8192];
            rows[0] = "# A";
            rows[1024] = opener;
            rows[2048] = "# Fake math heading";
            rows[3072] = "target inside math";
            rows[4096] = closer;
            rows[5120] = "# B";
            rows[6144] = "target in B";
            let buffer = Buffer::from_text(&rows.join("\n"));
            for (line, expected) in [(3072, vec![0]), (6144, vec![])] {
                let (unfold, _, lazy) = lazy_search_heading_unfold(&buffer, &[0], line..line + 1);
                assert_eq!(unfold, expected, "{opener}: target {line}");
                for unparsed in [1024, 2048, 4096, 5120] {
                    assert!(!lazy.is_parsed(unparsed / super::super::LARGE_MARKDOWN_CHUNK_LINES));
                }
            }
        }
    }

    #[test]
    fn search_unfold_lazy_nested_match_opens_only_real_ancestors_and_its_own_anchor() {
        let mut rows = vec![""; 4096];
        rows[0] = "# Root";
        rows[512] = "## Previous";
        rows[1024] = "## Current";
        rows[2048] = "### Child";
        rows[3072] = "target";
        let buffer = Buffer::from_text(&rows.join("\n"));
        let anchors = [0, 512, 1024, 2048];
        for lines in [3072..3073, 2048..2049] {
            let (unfold, _, _) = lazy_search_heading_unfold(&buffer, &anchors, lines);
            assert_eq!(unfold, [0, 1024, 2048]);
        }
    }

    #[test]
    fn search_unfold_heading_boundary_is_half_open_and_crossline_hits_open_both_containers() {
        let buffer = Buffer::from_text("# A\nbody\n# B\nbody\n# C");
        let decorations = parse_decorations(buffer.text());
        let headings = super::search_fold_heading_boundaries(&buffer, &decorations);
        let unfold = |lines| {
            super::search_fold_anchors_to_unfold(
                &buffer,
                &decorations,
                &[0, 2, 4],
                &lines,
                Some(&headings),
                |_| panic!("heading source bounds are independent of presentation folds"),
            )
        };
        assert_eq!(unfold(2..3), [2]);
        assert_eq!(unfold(1..3), [0, 2]);
        assert_eq!(
            unfold(4..5),
            [4],
            "even an EOF heading anchor is a match container"
        );
        assert!(unfold(2..2).is_empty());
    }

    #[test]
    fn search_unfold_lazy_heading_scan_preserves_code_math_and_setext_contexts() {
        for body in [
            "```rust\n# fake\nTitle\n===\n```",
            "~~~~lang\n# fake\n~~~\n# still code\n~~~~",
            "> ```\n> # fake\n> ```",
            "$$\n# fake\nTitle\n===\n$$",
            "$$x\n# fake\n$$",
            "\\[\n# fake\nTitle\n===\n\\]",
            "\\[\n```\n# fake\n```\n\\]",
            "\\[\n```\n# fake\n\\]",
            "$$\n```\n# fake\n$$",
            "- ```\n  # fake\n  ```",
            "- ```\n  # fake\n# Outside the list",
            "- # List heading\n  body",
            "1. ## List heading\n   body",
            "```\n$$\n# fake\n```",
            "Heading\n=\n\nbody",
            "Heading\n-\n\nbody",
            "one\ntwo\n===\n\nbody",
            "#\n\nbody",
            "$$\n\n# Unclosed math does not hide headings",
        ] {
            let source = format!("# A\n\n{body}\n\n# B\n\n目标");
            let buffer = Buffer::from_text(&source);
            let (unparsed, _) = super::super::new_lazy_markdown_decorations(buffer.line_count());
            let expected: Vec<_> = parse_decorations(&source)
                .iter()
                .enumerate()
                .filter_map(|(line, row)| match row.block_type {
                    super::BlockType::Heading(level) => Some((line, level)),
                    _ => None,
                })
                .collect();
            assert_eq!(
                super::search_fold_heading_boundaries(&buffer, &unparsed),
                expected,
                "{body}"
            );
        }
    }

    #[test]
    fn search_unfold_front_matter_fence_like_scalars_do_not_poison_heading_boundaries() {
        let source = "---\nnote: |\n  ```\n  # scalar, not a heading\n# YAML comment\n---\n\n# A\n\nbody\n\n# B\n\ntarget";
        let buffer = Buffer::from_text(source);
        let decorations = parse_decorations(source);
        assert!(front_matter_fold_range(&decorations, 0).is_some());
        let expected = vec![(7, 1), (11, 1)];
        assert_eq!(
            super::search_fold_heading_boundaries(&buffer, &decorations),
            expected
        );
        let outline = super::MarkdownOutlineIndex::rebuild(&buffer);
        assert!(
            !outline.headings.iter().any(|heading| heading.line == 11),
            "the ordinary outline index demonstrably retains a fence opened inside metadata"
        );
        let (unfold, _, _) = lazy_search_heading_unfold(&buffer, &[7], 13..14);
        assert!(unfold.is_empty());
    }

    #[test]
    fn search_unfold_preview_and_front_matter_use_source_ownership_without_fallback() {
        for source in [
            "```mermaid\ngraph TD\nA --> B\n```\n\noutside",
            "$$\nx + 中\n$$\n\noutside",
            "\\[\nx + y\n\\]\n\noutside",
            "---\ntitle: 中🌸\n---\n\noutside",
            "---\n# comment only\n---\n\noutside",
        ] {
            let buffer = Buffer::from_text(source);
            let mut decorations = parse_decorations(source);
            decorate_empty_front_matter(&buffer, &mut decorations);
            let end = decorations[0]
                .block_preview
                .as_ref()
                .map(|preview| preview.end_line + 1)
                .or_else(|| front_matter_fold_range(&decorations, 0).map(|range| range.end))
                .unwrap();
            for line in 0..end {
                let unfold = super::search_fold_anchors_to_unfold(
                    &buffer,
                    &decorations,
                    &[0],
                    &(line..line + 1),
                    None,
                    |_| {
                        panic!(
                            "source container metadata must win over decoration fence heuristics"
                        )
                    },
                );
                assert_eq!(unfold, [0], "{source}: line {line}");
            }
            assert!(
                super::search_fold_anchors_to_unfold(
                    &buffer,
                    &decorations,
                    &[0],
                    &(end..end + 1),
                    None,
                    |_| panic!("metadata determines ownership"),
                )
                .is_empty()
            );
        }
    }

    #[test]
    fn search_unfold_fallback_folds_include_anchor_but_exclude_following_siblings() {
        for (source, end) in [
            ("```rust\nlet hit = 1;\n```\n\noutside", 3),
            ("- parent\n  - hit\n- sibling", 2),
        ] {
            let buffer = Buffer::from_text(source);
            let decorations = parse_decorations(source);
            for line in 0..end {
                assert_eq!(
                    super::search_fold_anchors_to_unfold(
                        &buffer,
                        &decorations,
                        &[0],
                        &(line..line + 1),
                        None,
                        |_| Some(1..end),
                    ),
                    [0]
                );
            }
            assert!(
                super::search_fold_anchors_to_unfold(
                    &buffer,
                    &decorations,
                    &[0],
                    &(end..end + 1),
                    None,
                    |_| Some(1..end),
                )
                .is_empty()
            );
        }
    }

    #[test]
    fn search_unfold_lazy_quote_rows_and_generic_code_containers() {
        let buffer = Buffer::from_text("> first\n> target\n> last\noutside");
        let mut decorations = super::super::new_lazy_markdown_decorations(buffer.line_count()).0;
        decorations[0].block_type = super::BlockType::Blockquote;
        assert_eq!(
            super::search_fold_anchors_to_unfold(
                &buffer,
                &decorations,
                &[0],
                &(2..3),
                None,
                |_| Some(1..2),
            ),
            [0],
            "an unparsed quote tail must not truncate ownership"
        );
        assert!(
            super::search_fold_anchors_to_unfold(
                &buffer,
                &decorations,
                &[0],
                &(3..4),
                None,
                |_| Some(1..2),
            )
            .is_empty()
        );

        let buffer =
            Buffer::from_text("fn a() {\n  if x {\n    hit();\n  }\n}\nfn b() {\n  other();\n}");
        let decorations = super::super::new_lazy_markdown_decorations(buffer.line_count()).0;
        assert_eq!(
            super::search_fold_anchors_to_unfold(
                &buffer,
                &decorations,
                &[0, 1, 5],
                &(2..3),
                None,
                |line| generic_code_fold_range_for_line_impl(buffer.line_count(), line, |row| {
                    buffer.line_content(row)
                }),
            ),
            [0, 1]
        );
    }

    #[test]
    fn search_unfold_heading_boundaries_are_recomputed_after_source_changes() {
        for (peer, expected) in [("# B", vec![]), ("## Child", vec![0])] {
            let source = format!("# A\n\n{peer}\n\ntarget");
            let buffer = Buffer::from_text(&source);
            let (unfold, _, _) = lazy_search_heading_unfold(&buffer, &[0], 4..5);
            assert_eq!(unfold, expected);
        }
    }

    #[test]
    fn front_matter_navigation_word_right_escapes_summary_before_reconciliation() {
        for source in [
            "---\ntitle: 中🌸\n---\nBody first\nBody second",
            "---\r\ntitle: 中🌸\r\n---\r\nBody first\r\nBody second",
        ] {
            let buffer = Buffer::from_text(source);
            let folded = front_matter_fold_range(&parse_decorations(source), 0).unwrap();
            let body = buffer.line_start(folded.end);
            for start in [0, buffer.line_end(0)] {
                let mut cursor = Cursor {
                    anchor: start,
                    offset: start,
                };
                for _ in 0..3 {
                    let previous = cursor.offset;
                    cursor.move_word_right(&buffer);
                    super::skip_front_matter_navigation_head(
                        &buffer,
                        Some(&folded),
                        |_| false,
                        previous,
                        &mut cursor,
                        false,
                    );
                    assert!(!super::reconcile_front_matter_caret(
                        &buffer,
                        &folded,
                        &mut cursor
                    ));
                    assert!(!cursor.has_selection());
                    if cursor.offset >= body {
                        break;
                    }
                }
                assert_eq!((cursor.anchor, cursor.offset), (body, body));
                let previous = cursor.offset;
                cursor.move_word_right(&buffer);
                assert!(!super::skip_front_matter_navigation_head(
                    &buffer,
                    Some(&folded),
                    |_| false,
                    previous,
                    &mut cursor,
                    false,
                ));
                assert!(
                    cursor.offset > body,
                    "repeated Option+Right must continue in the body"
                );
                assert!(!cursor.has_selection());
            }
            let mut cursor = Cursor {
                anchor: body,
                offset: body,
            };
            cursor.move_word_left(&buffer);
            assert!(super::skip_front_matter_navigation_head(
                &buffer,
                Some(&folded),
                |_| false,
                body,
                &mut cursor,
                false,
            ));
            assert_eq!(
                (cursor.anchor, cursor.offset),
                (buffer.line_end(0), buffer.line_end(0))
            );
            assert_eq!(buffer.text(), source);
        }
    }

    #[test]
    fn front_matter_navigation_multi_cursor_moves_skip_before_reconciliation_without_selections() {
        let source = "---\ntitle: 中🌸\ntags: [one, two]\n---\nBody first\nBody second";
        let buffer = Buffer::from_text(source);
        let folded = front_matter_fold_range(&parse_decorations(source), 0).unwrap();
        let opener = buffer.line_end(0);
        let body = buffer.line_start(folded.end);
        let second_body = buffer.line_start(folded.end + 1);
        let operations: [(&str, fn(&mut Cursor, &Buffer), bool); 6] = [
            ("left", Cursor::move_left, false),
            ("right", Cursor::move_right, true),
            ("up", Cursor::move_up, false),
            ("down", Cursor::move_down, true),
            ("word-left", Cursor::move_word_left, false),
            ("word-right", Cursor::move_word_right, true),
        ];
        for (name, movement, forward) in operations {
            let primary = if forward { opener } else { body };
            let mut cursors = [
                Cursor {
                    anchor: primary,
                    offset: primary,
                },
                Cursor {
                    anchor: second_body + 5,
                    offset: second_body + 5,
                },
                // An ordinary movement must also collapse an existing selection.
                Cursor {
                    anchor: second_body,
                    offset: second_body + 4,
                },
            ];
            let previous = cursors;
            for cursor in &mut cursors {
                movement(cursor, &buffer);
            }
            let raw = cursors;
            for (cursor, previous) in cursors.iter_mut().zip(previous) {
                super::skip_front_matter_navigation_head(
                    &buffer,
                    Some(&folded),
                    |_| false,
                    previous.offset,
                    cursor,
                    false,
                );
                assert!(
                    !cursor.has_selection(),
                    "{name} created a phantom selection"
                );
                assert!(
                    !super::reconcile_front_matter_caret(&buffer, &folded, cursor),
                    "{name} skipped too late"
                );
            }
            let target = if forward { body } else { opener };
            assert_eq!(
                (cursors[0].anchor, cursors[0].offset),
                (target, target),
                "{name}"
            );
            assert_eq!(
                &cursors[1..],
                &raw[1..],
                "{name} changed unrelated body cursors"
            );
        }
        assert_eq!(buffer.text(), source);
    }

    #[test]
    fn front_matter_navigation_reading_mode_moves_and_selections_skip_hidden_rows() {
        let buffer = Buffer::from_text(
            "---\ntags:\n  - alpha\n  - 二🌸\ntitle: Moeka\n---\nBody first\nBody second",
        );
        let hidden_rows = [0, 2, 3, 5];
        let operations: [(
            fn(&mut Cursor, &Buffer),
            fn(&mut Cursor, &Buffer),
            usize,
            usize,
        ); 6] = [
            (
                Cursor::move_left,
                Cursor::select_left,
                buffer.line_start(6),
                buffer.line_end(4),
            ),
            (
                Cursor::move_right,
                Cursor::select_right,
                buffer.line_end(4),
                buffer.line_start(6),
            ),
            (
                Cursor::move_up,
                Cursor::select_up,
                buffer.line_start(4),
                buffer.line_end(1),
            ),
            (
                Cursor::move_down,
                Cursor::select_down,
                buffer.line_start(1),
                buffer.line_start(4),
            ),
            (
                Cursor::move_word_left,
                Cursor::select_word_left,
                buffer.line_start(6),
                buffer.line_end(4),
            ),
            (
                Cursor::move_word_right,
                Cursor::select_word_right,
                buffer.line_end(1),
                buffer.line_start(4),
            ),
        ];
        for (movement, selection, start, target) in operations {
            for selecting in [false, true] {
                let mut cursors = [
                    Cursor {
                        anchor: start,
                        offset: start,
                    },
                    Cursor {
                        anchor: buffer.line_start(7),
                        offset: buffer.line_end(7),
                    },
                ];
                let previous = cursors;
                for cursor in &mut cursors {
                    if selecting {
                        selection(cursor, &buffer);
                    } else {
                        movement(cursor, &buffer);
                    }
                }
                let unrelated = cursors[1];
                for (cursor, previous) in cursors.iter_mut().zip(previous) {
                    super::skip_front_matter_navigation_head(
                        &buffer,
                        None,
                        |line| hidden_rows.contains(&line),
                        previous.offset,
                        cursor,
                        selecting,
                    );
                    assert!(!hidden_rows.contains(&buffer.offset_to_line_col(cursor.offset).0));
                    if selecting {
                        assert_eq!(cursor.anchor, previous.anchor);
                    } else {
                        assert!(!cursor.has_selection());
                    }
                }
                assert_eq!(cursors[0].offset, target);
                assert_eq!(cursors[0].anchor, if selecting { start } else { target });
                assert_eq!(cursors[1], unrelated);
            }
        }
        // Left at the first visible property remains an empty caret, not a
        // selection of the hidden opening delimiter.
        let start = buffer.line_start(1);
        let mut cursor = Cursor {
            anchor: start,
            offset: start,
        };
        cursor.move_left(&buffer);
        super::skip_front_matter_navigation_head(
            &buffer,
            None,
            |line| hidden_rows.contains(&line),
            start,
            &mut cursor,
            false,
        );
        assert_eq!((cursor.anchor, cursor.offset), (start, start));
    }

    #[test]
    fn front_matter_navigation_explicit_selection_mode_preserves_anchors_and_hidden_eof() {
        let source = "---\ntitle: 中🌸\n---";
        let buffer = Buffer::from_text(source);
        let folded = front_matter_fold_range(&parse_decorations(source), 0).unwrap();
        let opener = buffer.line_end(0);
        for selecting in [false, true] {
            let mut cursor = Cursor {
                anchor: opener,
                offset: opener,
            };
            if selecting {
                cursor.select_word_right(&buffer);
            } else {
                cursor.move_word_right(&buffer);
            }
            assert!(super::skip_front_matter_navigation_head(
                &buffer,
                Some(&folded),
                |_| false,
                opener,
                &mut cursor,
                selecting,
            ));
            assert_eq!(cursor.anchor, opener);
            assert_eq!(cursor.offset, if selecting { buffer.len() } else { opener });
            assert_eq!(cursor.has_selection(), selecting);
            assert!(!super::reconcile_front_matter_caret(
                &buffer,
                &folded,
                &mut cursor
            ));
        }
        // Contracting a reversed selection can reach its anchor temporarily;
        // selection intent must not be inferred from has_selection().
        let buffer = Buffer::from_text("---\ntitle: 中🌸\n---\nBody");
        let folded = front_matter_fold_range(&parse_decorations(buffer.text()), 0).unwrap();
        let body = buffer.line_start(folded.end);
        let hidden_anchor = buffer.line_start(folded.start);
        let mut cursor = Cursor {
            anchor: hidden_anchor,
            offset: body,
        };
        cursor.select_to(hidden_anchor, &buffer);
        assert!(!cursor.has_selection());
        assert!(super::skip_front_matter_navigation_head(
            &buffer,
            Some(&folded),
            |_| false,
            body,
            &mut cursor,
            true,
        ));
        assert_eq!(
            (cursor.anchor, cursor.offset),
            (hidden_anchor, buffer.line_end(0))
        );
        // No movement/reconciliation must rewrite existing source selections.
        let snapshot = cursor;
        assert!(!super::skip_front_matter_navigation_head(
            &buffer,
            Some(&folded),
            |_| false,
            cursor.offset,
            &mut cursor,
            true,
        ));
        assert_eq!(cursor, snapshot);
    }

    #[test]
    fn front_matter_reading_horizontal_and_shift_navigation_never_stop_on_hidden_delimiters() {
        let buffer = Buffer::from_text("---\ntitle: value\n---\nBody");
        let hidden = |line| line == 0 || line == 2;
        let start = buffer.line_start(1);
        let end = buffer.line_end(1);
        let body = buffer.line_start(3);
        assert_eq!(
            super::front_matter_hidden_navigation_offset(&buffer, start, 3, hidden),
            Some(start)
        );
        assert_eq!(
            super::front_matter_hidden_navigation_offset(
                &buffer,
                end,
                buffer.line_start(2),
                hidden
            ),
            Some(body)
        );
        assert_eq!(
            super::front_matter_hidden_navigation_offset(&buffer, body, buffer.line_end(2), hidden),
            Some(end)
        );
        assert_eq!(
            super::front_matter_hidden_navigation_offset(&buffer, body, body, hidden),
            None
        );
        let eof_buffer = Buffer::from_text("---\ntitle: value\n---");
        let previous = eof_buffer.line_end(1);
        assert_eq!(
            super::front_matter_hidden_navigation_offset(
                &eof_buffer,
                previous,
                eof_buffer.len(),
                hidden
            ),
            Some(previous)
        );
    }

    #[test]
    fn front_matter_fold_control_tracks_first_visible_property_but_keeps_anchor_zero() {
        for source in [
            "---\ntitle: Moeka\ntags: [one, two]\n---\nBody",
            "---\nbody: |\n  first\n  second\ntitle: Moeka\n---\nBody",
            "---\n# metadata\ntitle: Moeka\n---\nBody",
        ] {
            let buffer = Buffer::from_text(source);
            let decorations = crate::markdown::parse_decorations(source);
            let plans = helpers::front_matter_group_plan(&buffer, &decorations, &[], true);
            let expected = if source.contains("body:") || source.contains("# metadata") {
                2
            } else {
                1
            };
            assert_eq!(
                front_matter_fold_control_line(&decorations, &plans, false),
                Some(expected)
            );
            // Collapsed metadata hosts the summary on the first body row.
            assert_eq!(
                front_matter_fold_control_line(&decorations, &[], true),
                Some(1)
            );
            assert!(front_matter_fold_range(&decorations, 0).is_some());
            assert!(front_matter_fold_range(&decorations, expected).is_none());
            assert_eq!(buffer.text(), source);
        }
        let buffer = Buffer::from_text("---\n---");
        let mut decorations = crate::markdown::parse_decorations(buffer.text());
        decorate_empty_front_matter(&buffer, &mut decorations);
        assert_eq!(
            front_matter_fold_control_line(&decorations, &[], false),
            Some(0)
        );
        assert!(!super::front_matter_opener_hides_fold_control(
            &decorations,
            0,
            Some(0)
        ));
    }

    #[test]
    fn front_matter_opener_draws_no_fold_control_when_body_hosts_it() {
        let source = "---\ntitle: Moeka\n---\nBody";
        let decorations = parse_decorations(source);
        let control = front_matter_fold_control_line(&decorations, &[], false);
        assert_eq!(control, Some(1));
        assert!(super::front_matter_opener_hides_fold_control(
            &decorations,
            0,
            control
        ));
        assert!(!super::front_matter_opener_hides_fold_control(
            &decorations,
            1,
            control
        ));
    }

    #[test]
    fn front_matter_collapsed_geometry_hides_only_interior_rows() {
        let source = "---\ntitle: Moeka\ntags: [one, two]\nbody: x\n---\nBody";
        let decorations = parse_decorations(source);
        let range = front_matter_fold_range(&decorations, 0).unwrap();
        // Row 0 (opener) and the closer keep geometry; only interior rows after the summary hide.
        assert_eq!(front_matter_fold_summary_line(&range), 1);
        assert!(!front_matter_fold_line_is_hidden(&range, 0));
        assert!(!front_matter_fold_line_is_hidden(&range, 1));
        assert!(front_matter_fold_line_is_hidden(&range, 2));
        assert!(front_matter_fold_line_is_hidden(&range, 3));
        assert!(!front_matter_fold_line_is_hidden(&range, 4));
        // Source ownership still includes the summary row and closer.
        assert_eq!(range, 1..5);

        let empty = Buffer::from_text("---\n---\nBody");
        let mut decorations = parse_decorations(empty.text());
        decorate_empty_front_matter(&empty, &mut decorations);
        let range = front_matter_fold_range(&decorations, 0).unwrap();
        // Empty metadata has no body row, so the fallback summary uses the opener.
        assert_eq!(front_matter_fold_summary_line(&range), 0);
        assert!(!front_matter_fold_line_is_hidden(&range, 0));
        assert!(!front_matter_fold_line_is_hidden(&range, 1));
    }

    #[test]
    fn front_matter_closing_delimiter_is_detected_for_hiding() {
        let decorations = parse_decorations("---\ntitle: Moeka\n---\nBody");
        let detected: Vec<_> = (0..4)
            .map(|line| super::front_matter_is_closing_delimiter_line(&decorations, line))
            .collect();
        assert_eq!(detected, [false, false, true, false]);
        let empty_buffer = Buffer::from_text("---\n---\nBody");
        let mut empty = parse_decorations(empty_buffer.text());
        decorate_empty_front_matter(&empty_buffer, &mut empty);
        assert!(super::front_matter_is_closing_delimiter_line(&empty, 1));
        assert!(!super::front_matter_is_closing_delimiter_line(&empty, 0));
    }

    #[test]
    fn front_matter_collapsed_navigation_and_reconcile_own_summary_and_closer() {
        let source = "---\ntitle: Moeka\ntags: [one, two]\nbody: x\n---\nBody";
        let buffer = Buffer::from_text(source);
        let decorations = parse_decorations(source);
        let folded = front_matter_fold_range(&decorations, 0).unwrap();
        // Entering from the opener skips the summary and closer as one source block.
        assert_eq!(
            super::front_matter_fold_navigation_offset(
                &buffer,
                &folded,
                buffer.line_end(0),
                buffer.line_start(1),
            ),
            Some(buffer.line_start(5))
        );
        // Leaving upward from after the closer lands on the opener's end.
        assert_eq!(
            super::front_matter_fold_navigation_offset(
                &buffer,
                &folded,
                buffer.line_start(5),
                buffer.line_end(4),
            ),
            Some(buffer.line_end(0))
        );
        // A caret on the summary row or the closer reconciles to the opener's end.
        for offset in [buffer.line_start(1), buffer.line_end(4)] {
            let mut cursor = Cursor {
                anchor: offset,
                offset,
            };
            assert!(super::reconcile_front_matter_caret(
                &buffer,
                &folded,
                &mut cursor
            ));
            assert_eq!(cursor.offset, buffer.line_end(0));
        }
    }

    #[test]
    fn front_matter_fold_includes_closer_not_body_and_counts_only_root_entries() {
        for source in [
            "---\ntitle: Moeka\nobject:\n  child: true\n  nested:\n    deep: 2\ntags:\n  - name: first\n    extra: 1\n  - second\nbody: |-\n  key: literal\n  more\n---\nBody",
            "---\r\ntitle: Moeka\r\nobject: {nested: true}\r\ntags: [a, b]\r\nbody: >+\r\n  prose\r\n...\r\nBody",
            "-\ntitle: Moeka\nobject: {}\ntags: []\nbody: null\n-\nBody",
        ] {
            let buffer = Buffer::from_text(source);
            let decorations = parse_decorations(source);
            let range = front_matter_fold_range(&decorations, 0).unwrap();
            assert_eq!(range.start, 1);
            assert!(range.contains(&(range.end - 1)));
            assert!(!range.contains(&0));
            assert_eq!(buffer.line_content(range.end), "Body");
            assert_eq!(front_matter_entry_count(&buffer, &decorations), 4);
            assert_eq!(front_matter_fold_range(&decorations, range.end - 1), None);
            assert_eq!(front_matter_fold_range(&decorations, 1), None);
            assert_eq!(buffer.text(), source);
        }
    }

    #[test]
    fn front_matter_fold_handles_empty_flow_duplicate_and_missing_metadata() {
        for (source, count, foldable) in [
            ("---\n---", 0, true),
            ("---\n# comment\n\n---\nBody", 0, true),
            ("---\n{}\n---", 0, true),
            ("---\n{one: 1, two: {nested: 2}}\n---", 2, true),
            ("---\n? [a, b]\n: complex\ntitle: scalar\n---", 2, true),
            ("---\none: 1\none: 2\n---", 2, true),
            ("---\nbody: |\n  fake: key\n---", 1, true),
            ("---\nordinary paragraph\n---", 0, false),
            ("---\ntitle: missing closer", 0, false),
            ("Body\n---\ntitle: not metadata\n---", 0, false),
        ] {
            let buffer = Buffer::from_text(source);
            let mut decorations = parse_decorations(source);
            decorate_empty_front_matter(&buffer, &mut decorations);
            assert_eq!(
                front_matter_fold_range(&decorations, 0).is_some(),
                foldable,
                "{source}"
            );
            assert_eq!(
                front_matter_entry_count(&buffer, &decorations),
                count,
                "{source}"
            );
            assert_eq!(buffer.text(), source);
        }
    }

    #[test]
    fn front_matter_fold_navigation_skips_hidden_source_in_both_directions_and_at_eof() {
        for source in [
            "---\ntitle: 中🌸\n---\nBody",
            "---\ntitle: 中🌸\n---",
            "---\n---",
        ] {
            let buffer = Buffer::from_text(source);
            let mut decorations = parse_decorations(source);
            decorate_empty_front_matter(&buffer, &mut decorations);
            let range = front_matter_fold_range(&decorations, 0).unwrap();
            let anchor = buffer.line_end(0);
            let first_hidden = buffer.line_start(range.start);
            let outside = if range.end < buffer.line_count() {
                buffer.line_start(range.end)
            } else {
                anchor
            };
            assert_eq!(
                super::front_matter_fold_navigation_offset(&buffer, &range, anchor, first_hidden),
                Some(outside)
            );
            let last_hidden = buffer.line_end(range.end - 1);
            assert_eq!(
                super::front_matter_fold_navigation_offset(
                    &buffer,
                    &range,
                    buffer.len() + 1,
                    last_hidden
                ),
                Some(anchor)
            );
            assert_eq!(
                super::front_matter_fold_navigation_offset(&buffer, &range, first_hidden, anchor),
                None
            );
            assert_eq!(buffer.text(), source);
        }
    }

    #[test]
    fn front_matter_fold_reconciliation_preserves_source_selections_at_eof_and_inside() {
        for source in [
            "---\ntitle: 中🌸\n---",
            "---\r\ntitle: 中🌸\r\n---\r\nBody",
            "---\n---",
        ] {
            let buffer = Buffer::from_text(source);
            let mut decorations = parse_decorations(source);
            decorate_empty_front_matter(&buffer, &mut decorations);
            let hidden = front_matter_fold_range(&decorations, 0).unwrap();
            let mut all = Cursor::new();
            all.select_all(&buffer);
            for _ in 0..3 {
                assert!(!super::reconcile_front_matter_caret(
                    &buffer, &hidden, &mut all
                ));
                assert_eq!(all.selection_range(), 0..buffer.len());
            }
            // Folding with a partial or reversed source selection must not
            // replace it, even if both endpoints belong to hidden rows.
            let start = buffer.line_start(hidden.start);
            let end = buffer.line_end(hidden.end - 1);
            for (anchor, offset) in [(0, end), (end, 0), (start, end), (end, start)] {
                let mut cursor = Cursor { anchor, offset };
                if cursor.has_selection() {
                    assert!(!super::reconcile_front_matter_caret(
                        &buffer,
                        &hidden,
                        &mut cursor
                    ));
                    assert_eq!((cursor.anchor, cursor.offset), (anchor, offset));
                }
            }
            assert_eq!(buffer.text(), source);
        }
    }

    #[test]
    fn front_matter_fold_reconciliation_relocates_only_hidden_empty_carets() {
        let source = "---\ntitle: 中🌸\n---\nBody";
        let buffer = Buffer::from_text(source);
        let hidden = front_matter_fold_range(&parse_decorations(source), 0).unwrap();
        let body = buffer.line_start(hidden.end);
        let mut cursors = [
            Cursor {
                anchor: 0,
                offset: buffer.len(),
            },
            Cursor {
                anchor: 4,
                offset: 4,
            },
            Cursor {
                anchor: body,
                offset: body,
            },
            Cursor {
                anchor: 4,
                offset: buffer.line_end(1),
            },
        ];
        for cursor in &mut cursors {
            super::reconcile_front_matter_caret(&buffer, &hidden, cursor);
        }
        assert_eq!(cursors[0].selection_range(), 0..buffer.len());
        assert_eq!((cursors[1].anchor, cursors[1].offset), (3, 3));
        assert_eq!((cursors[2].anchor, cursors[2].offset), (body, body));
        assert_eq!(cursors[3].selection_range(), 4..buffer.line_end(1));
    }

    #[test]
    fn front_matter_fold_shift_navigation_skips_source_without_changing_anchor() {
        for source in [
            "---\ntitle: 中🌸\n---\nBody",
            "---\r\ntitle: 中🌸\r\n---\r\nBody",
        ] {
            let buffer = Buffer::from_text(source);
            let hidden = front_matter_fold_range(&parse_decorations(source), 0).unwrap();
            let opener = buffer.line_end(0);
            let body = buffer.line_start(hidden.end);
            for (origin, forward, target) in [(body, false, opener), (opener, true, body)] {
                let mut cursor = Cursor {
                    anchor: origin,
                    offset: origin,
                };
                if forward {
                    cursor.select_right(&buffer);
                } else {
                    cursor.select_left(&buffer);
                }
                assert!(super::skip_front_matter_selection_head(
                    &buffer,
                    &hidden,
                    origin,
                    &mut cursor
                ));
                assert_eq!((cursor.anchor, cursor.offset), (origin, target));
                assert!(!super::reconcile_front_matter_caret(
                    &buffer,
                    &hidden,
                    &mut cursor
                ));
                // Shift navigation back contracts the selection, not its anchor.
                let previous = cursor.offset;
                if forward {
                    cursor.select_left(&buffer);
                } else {
                    cursor.select_right(&buffer);
                }
                super::skip_front_matter_selection_head(&buffer, &hidden, previous, &mut cursor);
                assert_eq!((cursor.anchor, cursor.offset), (origin, origin));
            }
        }
        let source = "---\ntitle: 中🌸\n---";
        let buffer = Buffer::from_text(source);
        let hidden = front_matter_fold_range(&parse_decorations(source), 0).unwrap();
        let mut all = Cursor::new();
        all.select_all(&buffer);
        assert!(!super::skip_front_matter_selection_head(
            &buffer,
            &hidden,
            buffer.len(),
            &mut all
        ));
        assert_eq!(all.selection_range(), 0..buffer.len());
        let opener = buffer.line_end(0);
        let mut cursor = Cursor {
            anchor: opener,
            offset: opener,
        };
        cursor.select_right(&buffer);
        assert!(super::skip_front_matter_selection_head(
            &buffer,
            &hidden,
            opener,
            &mut cursor
        ));
        assert_eq!((cursor.anchor, cursor.offset), (opener, buffer.len()));
        assert!(!super::reconcile_front_matter_caret(
            &buffer,
            &hidden,
            &mut cursor
        ));
        // Repeated Shift+Right at EOF retains the selected hidden source.
        cursor.select_right(&buffer);
        assert!(!super::skip_front_matter_selection_head(
            &buffer,
            &hidden,
            buffer.len(),
            &mut cursor
        ));
        assert_eq!(cursor.selection_range(), opener..buffer.len());
    }

    fn source_folds(
        text: &str,
    ) -> (
        Buffer,
        Vec<super::LineDecoration>,
        super::outline::SourceOutline,
    ) {
        let buffer = Buffer::from_text(text);
        let mut decorations = parse_decorations(text);
        decorate_empty_front_matter(&buffer, &mut decorations);
        let headings = super::outline::source_outline_data(&buffer, &decorations, false);
        (buffer, decorations, headings)
    }

    #[test]
    fn source_fold_ranges_use_canonical_blocks_not_the_literal_projection() {
        let text = "---\ntitle: Note\n---\n# A\n```rust\nfn a() {\n    run();\n}\n```\n$$\nx + y\n$$\n```mermaid\ngraph TD\n A --> B\n```\n## Child\nbody\n# B\nlast";
        let (buffer, decorations, headings) = source_folds(text);
        let fold = |line| super::source_fold_range_for_line(&buffer, &decorations, &headings, line);
        assert_eq!(fold(0), Some(1..3));
        assert_eq!(fold(3), Some(4..18));
        assert_eq!(fold(4), Some(5..9));
        assert_eq!(fold(5), Some(6..8));
        assert_eq!(fold(8), None);
        assert_eq!(fold(9), Some(10..12));
        assert_eq!(fold(11), None);
        assert_eq!(fold(12), Some(13..16));
        assert_eq!(fold(15), None);
        assert_eq!(fold(16), Some(17..18));
        assert_eq!(fold(18), Some(19..20));

        let mut presentation = decorations.clone();
        for decoration in &mut presentation {
            helpers::source_markdown_decoration(decoration);
        }
        for line in [4, 9, 12] {
            assert_eq!(presentation[line].block_type, super::BlockType::Normal);
            assert!(fold(line).is_some());
        }
    }

    #[test]
    fn source_fold_ranges_handle_adjacent_unclosed_and_empty_fences() {
        for (text, expected) in [
            (
                "```rust\na\n```\n```python\nb\n```",
                vec![(0, Some(1..3)), (2, None), (3, Some(4..6)), (5, None)],
            ),
            (
                "~~~~\na\n~~~~\n~~~\nb",
                vec![(0, Some(1..3)), (2, None), (3, Some(4..5))],
            ),
            ("```\n```", vec![(0, Some(1..2)), (1, None)]),
            ("```", vec![(0, None)]),
        ] {
            let (buffer, decorations, headings) = source_folds(text);
            for (line, range) in expected {
                assert_eq!(
                    super::source_fold_range_for_line(&buffer, &decorations, &headings, line),
                    range,
                    "{text:?}, line {line}"
                );
            }
        }
    }

    #[test]
    fn source_fenced_folds_resolve_quote_and_list_openers_not_closing_prefixes() {
        for (text, opener, closer, end) in [
            ("> ```\n> code\n> ```\n# Next", 0, 2, 3),
            ("- ```\n  code\n  ```\n\n# Next", 0, 2, 3),
            ("```\na\n```\n```\nb\n```", 3, 5, 6),
        ] {
            let (buffer, decorations, outline) = source_folds(text);
            assert_eq!(
                super::source_fold_range_for_line(&buffer, &decorations, &outline, opener),
                Some(opener + 1..end),
                "{text:?}"
            );
            assert_eq!(
                super::source_fold_range_for_line(&buffer, &decorations, &outline, closer),
                None,
                "{text:?}"
            );
        }
    }

    #[test]
    fn source_fence_closers_keep_the_openers_quote_container_eager_and_lazy() {
        for text in [
            "```md\n> ```\n# literal\n```\n# Real",
            "> ```md\n> > ```\n> # literal\n> ```\n# Real",
            ">> ```md\n>>> ```\n>> # literal\n>> ```\n# Real",
            "> - ```md\n>   > ```\n>   # literal\n>   ```\n# Real",
        ] {
            let buffer = Buffer::from_text(text);
            let canonical = parse_decorations(text);
            let unparsed = super::super::new_lazy_markdown_decorations(buffer.line_count()).0;
            for (lazy, decorations) in [(false, &canonical), (true, &unparsed)] {
                let outline = super::outline::source_outline_data(&buffer, decorations, lazy);
                assert_eq!(outline.code_ranges, vec![0..4], "lazy={lazy}: {text:?}");
                assert_eq!(
                    outline
                        .headings
                        .iter()
                        .map(|heading| (heading.line, heading.text.as_ref()))
                        .collect::<Vec<_>>(),
                    vec![(4, "Real")],
                    "lazy={lazy}: {text:?}"
                );
                assert_eq!(
                    super::source_fold_range_for_line(&buffer, decorations, &outline, 0),
                    Some(1..4)
                );
                assert_eq!(
                    super::source_fold_range_for_line(&buffer, decorations, &outline, 1),
                    None
                );
                assert_eq!(
                    super::source_fold_range_for_line(&buffer, decorations, &outline, 3),
                    None
                );
            }
        }
    }

    #[test]
    fn source_quote_fence_stops_when_its_opener_container_ends() {
        let text = "> ```md\n> # literal\n# Real";
        let buffer = Buffer::from_text(text);
        for lazy in [false, true] {
            let decorations = if lazy {
                vec![super::LineDecoration::default(); buffer.line_count()]
            } else {
                parse_decorations(text)
            };
            let outline = super::outline::source_outline_data(&buffer, &decorations, lazy);
            assert_eq!(outline.code_ranges, vec![0..2]);
            assert_eq!(
                outline
                    .headings
                    .iter()
                    .map(|heading| heading.line)
                    .collect::<Vec<_>>(),
                vec![2]
            );
        }
    }

    #[test]
    fn source_code_folds_cannot_escape_their_block_or_fold_markdown_prose() {
        let text = "```python\ndef f():\n    pass\n```\nprose {\n  indented prose\n}\n\n```js\nfunction broken() {\n  call();\n```\n}";
        let (buffer, decorations, headings) = source_folds(text);
        let fold = |line| super::source_fold_range_for_line(&buffer, &decorations, &headings, line);
        assert_eq!(fold(1), Some(2..3));
        assert_eq!(fold(4), None);
        assert_eq!(fold(9), Some(10..11));
        assert_ne!(fold(9), Some(10..13));
    }

    #[test]
    fn source_lazy_heading_folds_and_unfold_use_offscreen_source_boundaries() {
        let mut rows = vec![""; 4096];
        rows[0] = "# A";
        rows[1024] = "```md";
        rows[1025] = "# Literal code heading";
        rows[1030] = "```";
        rows[2048] = "# B";
        rows[3072] = "target";
        let buffer = Buffer::from_text(&rows.join("\n"));
        let (mut decorations, mut lazy) =
            super::super::new_lazy_markdown_decorations(buffer.line_count());
        super::super::parse_lazy_markdown_range(&buffer, &mut decorations, &mut lazy, 0..1);
        let headings = super::outline::source_outline_data(&buffer, &decorations, true);
        assert_eq!(
            super::source_fold_range_for_line(&buffer, &decorations, &headings, 0),
            Some(1..2048)
        );
        assert_eq!(
            super::source_fold_range_for_line(&buffer, &decorations, &headings, 2048),
            Some(2049..4096)
        );
        assert_eq!(decorations[2048].block_type, super::BlockType::Normal);
        assert!(!lazy.is_parsed(2048 / super::super::LARGE_MARKDOWN_CHUNK_LINES));
        let boundaries = super::search_fold_heading_boundaries(&buffer, &decorations);
        assert_eq!(
            super::search_fold_anchors_to_unfold(
                &buffer,
                &decorations,
                &[0, 2048],
                &(3072..3073),
                Some(&boundaries),
                |line| {
                    super::source_fold_range_for_line(&buffer, &decorations, &headings, line)
                }
            ),
            vec![2048]
        );
    }

    #[test]
    fn source_lazy_math_fold_uses_the_closed_span_not_a_local_fragment() {
        for (opener, closer) in [("$$", "$$"), ("\\[", "\\]")] {
            let mut rows = vec![""; 8192];
            rows[0] = "# A";
            rows[1024] = opener;
            rows[2048] = "# Literal math heading";
            rows[3072] = "target";
            rows[4096] = closer;
            rows[5120] = "# B";
            let buffer = Buffer::from_text(&rows.join("\n"));
            let decorations = super::super::new_lazy_markdown_decorations(buffer.line_count()).0;
            let outline = super::outline::source_outline_data(&buffer, &decorations, true);
            assert_eq!(outline.math_ranges, vec![1024..4097]);
            assert_eq!(
                super::source_fold_range_for_line(&buffer, &decorations, &outline, 1024),
                Some(1025..4097)
            );
            assert_eq!(
                super::source_fold_range_for_line(&buffer, &decorations, &outline, 2048),
                None
            );
            assert_eq!(
                super::source_fold_range_for_line(&buffer, &decorations, &outline, 4096),
                None
            );
            assert_eq!(
                super::search_fold_anchors_to_unfold(
                    &buffer,
                    &decorations,
                    &[0, 1024],
                    &(3072..3073),
                    None,
                    |line| {
                        super::source_fold_range_for_line(&buffer, &decorations, &outline, line)
                    }
                ),
                vec![0, 1024]
            );
        }
    }

    #[test]
    fn source_search_unfold_includes_front_matter_math_and_diagram_closers() {
        let text = "---\ntitle: Note\n---\n$$\nx\n$$\n```mermaid\ngraph TD\n```";
        let (buffer, decorations, headings) = source_folds(text);
        for (line, anchor) in [(2, 0), (5, 3), (8, 6)] {
            assert_eq!(
                super::search_fold_anchors_to_unfold(
                    &buffer,
                    &decorations,
                    &[0, 3, 6],
                    &(line..line + 1),
                    None,
                    |line| {
                        super::source_fold_range_for_line(&buffer, &decorations, &headings, line)
                    }
                ),
                vec![anchor]
            );
        }
    }

    #[test]
    fn generic_fold_supports_brace_blocks() {
        let lines = [
            "fn main() {",
            "    if ready {",
            "        work();",
            "    }",
            "}",
        ];

        let range = generic_code_fold_range_for_line_impl(lines.len(), 0, |idx| lines[idx]);

        assert_eq!(range, Some(1..5));
    }

    #[test]
    fn generic_fold_supports_indentation_blocks() {
        let lines = [
            "def run():",
            "    step_one()",
            "    if ready:",
            "        step_two()",
            "    finish()",
            "done()",
        ];

        let range = generic_code_fold_range_for_line_impl(lines.len(), 0, |idx| lines[idx]);

        assert_eq!(range, Some(1..5));
    }

    #[test]
    fn generic_fold_supports_region_markers() {
        let lines = ["// #region Setup", "const port = 3000;", "// #endregion"];

        let range = generic_code_fold_range_for_line_impl(lines.len(), 0, |idx| lines[idx]);

        assert_eq!(range, Some(1..3));
    }

    #[test]
    fn generic_fold_supports_markup_tags() {
        let lines = ["<section>", "  <div>", "    body", "  </div>", "</section>"];

        let range = generic_code_fold_range_for_line_impl(lines.len(), 0, |idx| lines[idx]);

        assert_eq!(range, Some(1..5));
    }
}
