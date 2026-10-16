//! Source-owned front-matter groups. Presentation never rewrites YAML;
//! deletion plans are one exact source replacement, not serialization.
use std::ops::Range;

use gpui::Context;

use crate::buffer::Buffer;
use crate::front_matter::{FrontMatterLineInfo, FrontMatterValue};
use crate::markdown::LineDecoration;

use super::EditorView;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum GroupKind {
    Multiline,
    List,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ValueGroup {
    pub header_line: usize,
    /// Physical rows, including the header but never a front-matter delimiter.
    pub source_lines: Range<usize>,
    pub kind: GroupKind,
    pub items: Vec<ListItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ListItem {
    /// Absolute UTF-8 byte range of the scalar, excluding dash/comma and comments.
    pub source_range: Range<usize>,
    pub display_text: String,
    pub source_line: usize,
}

struct Header<'a> {
    info: &'a FrontMatterLineInfo,
    token: &'a str,
    indent: usize,
    token_range: Range<usize>,
}

/// Trust only the leading front-matter region already recognized by Markdown.
fn metadata_lines(buffer: &Buffer, decorations: &[LineDecoration]) -> Option<Range<usize>> {
    let table = decorations.first()?.table_info.as_ref()?;
    if !table.is_front_matter
        || table.table_start_line != 0
        || table.table_end_line < 2
        || table.table_end_line > buffer.line_count().min(decorations.len())
    {
        return None;
    }
    let opening = buffer.line_content(0).trim_start_matches('\u{feff}').trim();
    let closing = buffer.line_content(table.table_end_line - 1).trim_end();
    if !matches!((opening, closing), ("---", "---" | "...") | ("-", "-")) {
        return None;
    }
    let rows = 1..table.table_end_line - 1;
    if rows.clone().any(|line| {
        decorations[line].front_matter.is_none()
            || decorations[line].table_info.as_ref().is_none_or(|row| {
                !row.is_front_matter
                    || row.table_start_line != 0
                    || row.table_end_line != table.table_end_line
            })
    }) {
        return None;
    }
    Some(rows)
}

fn header<'a>(
    buffer: &'a Buffer,
    decorations: &'a [LineDecoration],
    line: usize,
) -> Option<Header<'a>> {
    let info = decorations.get(line)?.front_matter.as_ref()?;
    let key = info.key_range.as_ref()?;
    let source = buffer.line_content(line);
    source.get(key.clone())?;
    let token = source.get(info.value_range.clone())?;
    let offset = buffer.line_start(line);
    Some(Header {
        info,
        token,
        indent: key.start,
        token_range: offset + info.value_range.start..offset + info.value_range.end,
    })
}

fn indent(source: &str) -> usize {
    source.len() - source.trim_start_matches(' ').len()
}

fn sequence_row(source: &str) -> bool {
    source.starts_with('-') && source.as_bytes().get(1).is_none_or(u8::is_ascii_whitespace)
}

fn comment_or_blank(source: &str) -> bool {
    source.trim().is_empty() || source.trim_start().starts_with('#')
}

fn multiline_group(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    line: usize,
    limit: usize,
    header: &Header<'_>,
) -> Option<ValueGroup> {
    let literal = matches!(header.info.value, FrontMatterValue::BlockHeader);
    if !literal
        && (!matches!(header.info.value, FrontMatterValue::Text(_))
            || header.token.starts_with(['\'', '"', '&', '*', '!']))
    {
        return None;
    }
    let mut content_indent = if literal {
        header
            .token
            .bytes()
            .find(|byte| matches!(byte, b'1'..=b'9'))
            .map(|byte| header.indent + usize::from(byte - b'0'))
    } else {
        None
    };
    let mut end = line + 1;
    for row in line + 1..limit {
        let source = buffer.line_content(row).trim_end_matches('\r');
        let blank = source.trim().is_empty();
        let row_indent = indent(source);
        if literal && blank {
            end = row + 1;
            continue;
        }
        if !literal && comment_or_blank(source) {
            // Pending trivia belongs only if another body row follows.
            continue;
        }
        if row_indent <= header.indent || row_indent < *content_indent.get_or_insert(row_indent) {
            break;
        }
        let info = decorations[row].front_matter.as_ref()?;
        if info.key_range.is_some()
            || !info.display_prefix.is_empty()
            || !matches!(info.value, FrontMatterValue::Text(_))
        {
            return None;
        }
        end = row + 1;
    }
    if !literal && end == line + 1 {
        return None;
    }
    Some(ValueGroup {
        header_line: line,
        source_lines: line..end,
        kind: GroupKind::Multiline,
        items: Vec::new(),
    })
}

/// Decode only a scalar token. Never resolve aliases/tags/anchors, normalize
/// numbers/booleans/dates, or interpret Markdown inside a pill.
fn scalar_display(token: &str) -> Option<String> {
    if token.starts_with(['&', '*', '!', '|', '>', '[', '{']) {
        return None;
    }
    let value = serde_yaml::from_str::<serde_yaml::Value>(token).ok()?;
    match value {
        serde_yaml::Value::String(text) if token.starts_with(['\'', '"']) => Some(
            text.replace("\r\n", " ")
                .replace(['\n', '\r', '\u{85}', '\u{2028}', '\u{2029}'], " "),
        ),
        serde_yaml::Value::String(_)
        | serde_yaml::Value::Bool(_)
        | serde_yaml::Value::Number(_)
        | serde_yaml::Value::Null => Some(token.to_owned()),
        _ => None,
    }
}

struct FlowItems {
    items: Vec<ListItem>,
    commas: Vec<usize>,
    mapping: bool,
}

/// A deliberately single-physical-row flow sequence or mapping. Quotes are
/// entered only at node start (after a comma or colon), so an apostrophe in
/// O'Reilly is not a quoting delimiter. Multiline flow syntax and complex nodes
/// stay source-visible.
fn flow_items(header: &Header<'_>, line: usize) -> Option<FlowItems> {
    let token = header.token;
    let mapping = match (token.as_bytes().first(), token.as_bytes().last()) {
        (Some(b'['), Some(b']')) => false,
        (Some(b'{'), Some(b'}')) => true,
        _ => return None,
    };
    let bytes = token.as_bytes();
    let end = bytes.len() - 1;
    let mut quote = None;
    let mut node_start = true;
    let mut start = 1;
    let mut i = start;
    let mut pieces = Vec::new();
    let mut commas = Vec::new();
    while i < end {
        let byte = bytes[i];
        if let Some(delimiter) = quote {
            if delimiter == b'"' && byte == b'\\' {
                i += 2;
                continue;
            }
            if byte == delimiter {
                if delimiter == b'\'' && bytes.get(i + 1) == Some(&b'\'') {
                    i += 2;
                    continue;
                }
                quote = None;
            }
        } else {
            match byte {
                b'\'' | b'"' if node_start => {
                    quote = Some(byte);
                    node_start = false;
                }
                b',' => {
                    pieces.push(start..i);
                    commas.push(header.token_range.start + i);
                    start = i + 1;
                    node_start = true;
                }
                b'[' | b']' | b'{' | b'}' => return None,
                b'#' if node_start || bytes[i - 1].is_ascii_whitespace() => return None,
                b' ' | b'\t' => {}
                b'\r' | b'\n' => return None,
                b':' => node_start = true,
                _ => node_start = false,
            }
        }
        i += 1;
    }
    if quote.is_some() {
        return None;
    }
    pieces.push(start..end);
    let piece_count = pieces.len();
    let mut items = Vec::new();
    for (index, piece) in pieces.into_iter().enumerate() {
        let source = token.get(piece.clone())?;
        let text = source.trim_matches([' ', '\t']);
        if text.is_empty() {
            // Empty collections and a single trailing comma are valid. Missing
            // interior nodes ([a,,b] or [,a]) are not pills.
            if index + 1 == piece_count && (index == 0 || !items.is_empty()) {
                continue;
            }
            return None;
        }
        let local_start = piece.start + source.len() - source.trim_start_matches([' ', '\t']).len();
        let display_text = if mapping {
            mapping_entry_display(text)?
        } else {
            scalar_display(text)?
        };
        items.push(ListItem {
            source_range: header.token_range.start + local_start
                ..header.token_range.start + local_start + text.len(),
            display_text,
            source_line: line,
        });
    }
    // Reject malformed quoted/plain combinations that a delimiter-only scanner
    // cannot distinguish from scalar text.
    let scalar = |value: &serde_yaml::Value| {
        matches!(
            value,
            serde_yaml::Value::String(_)
                | serde_yaml::Value::Bool(_)
                | serde_yaml::Value::Number(_)
                | serde_yaml::Value::Null
        )
    };
    let valid = match serde_yaml::from_str::<serde_yaml::Value>(token).ok()? {
        serde_yaml::Value::Sequence(values) => {
            !mapping && values.len() == items.len() && values.iter().all(scalar)
        }
        serde_yaml::Value::Mapping(entries) => {
            mapping
                && entries.len() == items.len()
                && entries
                    .iter()
                    .all(|(key, value)| scalar(key) && scalar(value))
        }
        _ => false,
    };
    valid.then_some(FlowItems {
        items,
        commas,
        mapping,
    })
}

/// One `key: value` entry of a flow mapping, displayed as a single pill.
/// Keys and values must be scalars; anything else returns `None`.
fn mapping_entry_display(entry: &str) -> Option<String> {
    let key_end = if entry.starts_with(['\'', '"']) {
        quoted_end(entry)?
    } else {
        entry.find(':')?
    };
    let key = entry.get(..key_end)?.trim_end_matches([' ', '\t']);
    let rest = entry.get(key_end..)?.trim_start_matches([' ', '\t']);
    let value = rest.strip_prefix(':')?;
    if !value.is_empty() && !value.starts_with([' ', '\t']) {
        return None;
    }
    let value = value.trim_matches([' ', '\t']);
    if value.is_empty() {
        return None;
    }
    Some(format!(
        "{}: {}",
        scalar_display(key)?,
        scalar_display(value)?
    ))
}

fn quoted_end(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let quote = bytes[0];
    let mut i = 1;
    while i < bytes.len() {
        match (quote, bytes[i]) {
            (b'"', b'\\') => i += 2,
            (b'\'', b'\'') if bytes.get(i + 1) == Some(&b'\'') => i += 2,
            (_, byte) if byte == quote => return Some(i + 1),
            _ => i += 1,
        }
    }
    None
}

fn block_items(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    line: usize,
    limit: usize,
    header: &Header<'_>,
) -> Option<ValueGroup> {
    if !header.token.is_empty() {
        return None;
    }
    let mut items = Vec::new();
    let mut item_indent = None;
    let mut end = line + 1;
    for row in line + 1..limit {
        let source = buffer.line_content(row).trim_end_matches('\r');
        if comment_or_blank(source) {
            continue;
        }
        let row_indent = indent(source);
        let body = &source[row_indent..];
        if row_indent < header.indent || (row_indent == header.indent && !sequence_row(body)) {
            break;
        }
        if !sequence_row(body) || row_indent != *item_indent.get_or_insert(row_indent) {
            // Nested collections, scalar continuations and compact mappings
            // cannot be collapsed into a list of single scalar tokens.
            return None;
        }
        let info = decorations[row].front_matter.as_ref()?;
        if info.key_range.is_some() || info.display_prefix != "• " {
            return None;
        }
        let token = source.get(info.value_range.clone())?;
        let mut scalar_start = row_indent + 1;
        while source
            .as_bytes()
            .get(scalar_start)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
        {
            scalar_start += 1;
        }
        if scalar_start != info.value_range.start {
            return None;
        }
        let display_text = scalar_display(token)?;
        // Null items, including bare dashes, have source-preserving Text
        // metadata so empty and populated rows share an aligned value column.
        // Raw is reserved for unsupported/invalid nodes and escaped multiline
        // strings; do not turn invalid null source into polished pills.
        if matches!(info.value, FrontMatterValue::Raw) {
            match serde_yaml::from_str::<serde_yaml::Value>(token).ok()? {
                // The row lexer intentionally leaves escaped line breaks Raw.
                // These are still exact, single-source-row quoted scalar items.
                serde_yaml::Value::String(text)
                    if token.starts_with(['\'', '"'])
                        && text.contains(['\n', '\r', '\u{85}', '\u{2028}', '\u{2029}']) => {}
                _ => return None,
            }
        }
        let offset = buffer.line_start(row);
        items.push(ListItem {
            source_range: offset + info.value_range.start..offset + info.value_range.end,
            display_text,
            source_line: row,
        });
        end = row + 1;
    }
    if items.is_empty() {
        return None;
    }
    Some(ValueGroup {
        header_line: line,
        source_lines: line..end,
        kind: GroupKind::List,
        items,
    })
}

/// Comment rows and trailing comments remain source trivia, not list items.
/// A renderer collapsing owned list rows must keep that trivia source-visible;
/// `source_lines` is ownership/activation, not permission to discard comments.
pub(super) fn groups(buffer: &Buffer, decorations: &[LineDecoration]) -> Vec<ValueGroup> {
    let Some(rows) = metadata_lines(buffer, decorations) else {
        return Vec::new();
    };
    let mut result = Vec::new();
    let mut line = rows.start;
    while line < rows.end {
        let Some(header) = header(buffer, decorations, line) else {
            line += 1;
            continue;
        };
        let group = if header.token.starts_with(['[', '{']) {
            flow_items(&header, line).map(|flow| ValueGroup {
                header_line: line,
                source_lines: line..line + 1,
                kind: GroupKind::List,
                items: flow.items,
            })
        } else if header.token.is_empty() {
            block_items(buffer, decorations, line, rows.end, &header)
        } else {
            multiline_group(buffer, decorations, line, rows.end, &header)
        };
        if let Some(group) = group {
            line = group.source_lines.end;
            result.push(group);
        } else {
            line += 1;
        }
    }
    result
}

/// The renderer supplies carets, selections and IME ranges in source bytes.
/// Carets at EOL (including empty rows and either CRLF boundary) activate the
/// owning group; a selection ending at its start does not. Delimiters do not.
#[cfg(test)]
pub(super) fn group_is_active(
    buffer: &Buffer,
    group: &ValueGroup,
    active_ranges: &[Range<usize>],
) -> bool {
    let rows = &group.source_lines;
    if rows.is_empty() || rows.end > buffer.line_count() {
        return false;
    }
    let start = buffer.line_start(rows.start);
    let caret_end = buffer.line_end(rows.end - 1);
    let selection_end = buffer.line_start(rows.end);
    active_ranges.iter().any(|range| {
        if range.is_empty() {
            start <= range.start && range.start <= caret_end
        } else {
            range.start < selection_end && range.end > start
        }
    })
}

/// Preserve an item's trailing comment as a standalone physical row. Otherwise
/// remove that row including its original line ending, never the next sibling.
fn block_row_removal(buffer: &Buffer, item: &ListItem) -> Option<Range<usize>> {
    let line = item.source_line;
    let source = buffer.line_content(line).trim_end_matches('\r');
    let offset = buffer.line_start(line);
    let suffix = source.get(item.source_range.end.checked_sub(offset)?..)?;
    if suffix.trim_start().starts_with('#') {
        Some(offset + indent(source)..item.source_range.end)
    } else if suffix.trim().is_empty() {
        Some(offset..buffer.line_start(line + 1))
    } else {
        None
    }
}

pub(super) fn remove_list_item_edit(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    header_line: usize,
    item_index: usize,
) -> Option<(Range<usize>, String)> {
    let rows = metadata_lines(buffer, decorations)?;
    if !rows.contains(&header_line) {
        return None;
    }
    let header = header(buffer, decorations, header_line)?;
    if header.token.starts_with(['[', '{']) {
        let flow = flow_items(&header, header_line)?;
        let item = flow.items.get(item_index)?;
        if flow.items.len() == 1 {
            let empty = if flow.mapping { "{}" } else { "[]" };
            return Some((header.token_range, empty.into()));
        }
        let range = if let Some(next) = flow.items.get(item_index + 1) {
            // Consume the following comma/spacing, not the next scalar.
            item.source_range.start..next.source_range.start
        } else {
            let previous = flow.items.get(item_index.checked_sub(1)?)?;
            // Leave an optional trailing comma intact ([a,b,] -> [a,]).
            let comma = *flow.commas.get(item_index - 1)?;
            if comma < previous.source_range.end || comma >= item.source_range.start {
                return None;
            }
            previous.source_range.end..item.source_range.end
        };
        return Some((range, String::new()));
    }
    let group = block_items(buffer, decorations, header_line, rows.end, &header)?;
    let item = group.items.get(item_index)?;
    let removal = block_row_removal(buffer, item)?;
    if group.items.len() > 1 {
        return Some((removal, String::new()));
    }
    // Empty block sequences must become explicit [], never a null key. Span
    // both changes with one replacement and copy every intervening byte (header
    // comment, blank/comment rows, original CRLF) instead of serializing YAML.
    let start = header.token_range.start;
    let range = start..removal.end;
    let mut replacement = String::new();
    if buffer.text().as_bytes().get(start.checked_sub(1)?) == Some(&b':') {
        replacement.push(' ');
    }
    replacement.push_str("[]");
    if buffer.text().as_bytes().get(start) == Some(&b'#') {
        replacement.push(' ');
    }
    replacement.push_str(buffer.text().get(start..removal.start)?);
    Some((range, replacement))
}

impl EditorView {
    pub(super) fn remove_front_matter_list_item(
        &mut self,
        header_line: usize,
        item_index: usize,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab_is_read_only() {
            return;
        }
        let Some((range, replacement)) =
            remove_list_item_edit(&self.buffer, &self.decorations, header_line, item_index)
        else {
            return;
        };
        self.checkpoint();
        self.queue_non_markdown_line_display_transfer(&range, &replacement);
        self.buffer.replace(range.clone(), &replacement);
        super::remap_source_edit_positions(
            &mut self.cursor,
            &mut self.extra_cursors,
            &mut self.marked_range,
            &mut self.extra_marked_ranges,
            &mut self.wrap_boundary_affinity,
            &range,
            replacement.len(),
        );
        // Keep existing source positions, rather than focusing the clicked pill
        // or jumping into the remaining list. Clamps must be UTF-8 boundaries.
        for cursor in std::iter::once(&mut self.cursor).chain(self.extra_cursors.iter_mut()) {
            cursor.offset = self.buffer.clamp_offset(cursor.offset);
            cursor.anchor = self.buffer.clamp_offset(cursor.anchor);
        }
        for range in self
            .marked_range
            .iter_mut()
            .chain(self.extra_marked_ranges.iter_mut())
        {
            range.start = self.buffer.clamp_offset(range.start);
            range.end = self.buffer.clamp_offset(range.end);
        }
        if let Some(affinity) = self.wrap_boundary_affinity.as_mut() {
            affinity.offset = self.buffer.clamp_offset(affinity.offset);
        }
        self.reparse();
        self.pause_blinking(cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{GroupKind, ValueGroup, group_is_active, groups, remove_list_item_edit};
    use crate::buffer::Buffer;
    use crate::cursor::Cursor;
    use crate::markdown::parse_decorations;

    fn parsed(source: &str) -> (Buffer, Vec<ValueGroup>) {
        let buffer = Buffer::from_text(source);
        let found = groups(&buffer, &parse_decorations(source));
        (buffer, found)
    }

    fn removed(source: &str, line: usize, index: usize) -> String {
        let mut buffer = Buffer::from_text(source);
        let (range, replacement) =
            remove_list_item_edit(&buffer, &parse_decorations(source), line, index)
                .unwrap_or_else(|| panic!("no edit for item {index}: {source}"));
        assert_eq!(buffer.text(), source);
        assert!(source.is_char_boundary(range.start));
        assert!(source.is_char_boundary(range.end));
        buffer.replace(range, &replacement);
        buffer.text().to_owned()
    }

    #[test]
    fn multiline_ownership_literal_folded_explicit_indent_and_chomping() {
        for token in ["|", ">", "|-", ">+", "|2", ">2-", "|-2", "|2+"] {
            let source = format!(
                "---\nparent:\n  '日🙂': {token} # header\n\n    first\n    # literal body\n\n   # dedented comment\n  sibling: keep\nnext: keep\n...\nbody"
            );
            let (buffer, found) = parsed(&source);
            assert_eq!(found.len(), 1, "{source}");
            let group = &found[0];
            assert_eq!(group.header_line, 2);
            assert_eq!(group.source_lines, 2..7, "{source}");
            assert_eq!(group.kind, GroupKind::Multiline);
            assert!(group.items.is_empty());
            for line in group.source_lines.clone() {
                let eol = buffer.line_end(line);
                assert!(group_is_active(&buffer, group, &[eol..eol]));
            }
            let outside = buffer.line_start(7);
            assert!(!group_is_active(&buffer, group, &[outside..outside]));
        }
    }

    #[test]
    fn multiline_plain_body_includes_internal_blank_not_sibling_trivia() {
        let source = "---\nvalue: first\n  second\n\n  third\n# sibling comment\n\nnext: keep\n---";
        let (_, found) = parsed(source);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].source_lines, 1..5);
        assert_eq!(found[0].kind, GroupKind::Multiline);
        // A comment terminates a plain scalar; a later apparent continuation
        // is malformed YAML, not a second body row to hide.
        assert!(
            parsed("---\nvalue: first\n  second\n# comment\n  third\nnext: keep\n---")
                .1
                .is_empty()
        );
    }

    #[test]
    fn leading_front_matter_only_and_no_delimiter_ownership() {
        for (open, close) in [("---", "---"), ("---", "..."), ("-", "-")] {
            let source = format!(
                "\u{feff}{open}\nvalue: |-\n  text\nlist: [one, two]\n{close}\n---\nother: |-\n  body\n---"
            );
            let (buffer, found) = parsed(&source);
            assert_eq!(found.len(), 2);
            for group in &found {
                assert!(group.source_lines.start > 0);
                assert!(group.source_lines.end <= 4);
                for line in [0, 4, 6, 7] {
                    let offset = buffer.line_start(line);
                    assert!(!group_is_active(&buffer, group, &[offset..offset]));
                }
            }
        }
        for source in [
            "text\n---\nlist: [one]\n---",
            "---\nlist: [one]",
            "---\nnot a mapping\n---",
        ] {
            assert!(parsed(source).1.is_empty(), "{source}");
        }
    }

    #[test]
    fn nested_compact_and_indentless_lists_have_exact_owned_rows() {
        let source = "-\nsections:\n  - title: keep\n    tags: # header\n    - 日\n\n# between\n    - 本 # item\n    # sibling comment\n    sibling: keep\n  - other: untouched\n-\nbody";
        let (buffer, found) = parsed(source);
        assert_eq!(found.len(), 1);
        let group = &found[0];
        assert_eq!(group.header_line, 3);
        assert_eq!(group.source_lines, 3..8);
        assert_eq!(group.kind, GroupKind::List);
        assert_eq!(group.items.len(), 2);
        for (item, line, text) in [(&group.items[0], 4, "日"), (&group.items[1], 7, "本")] {
            assert_eq!(item.source_line, line);
            assert_eq!(buffer.text_for_range(item.source_range.clone()), text);
            assert_eq!(item.display_text, text);
        }
        assert_eq!(
            removed(source, 3, 0),
            "-\nsections:\n  - title: keep\n    tags: # header\n\n# between\n    - 本 # item\n    # sibling comment\n    sibling: keep\n  - other: untouched\n-\nbody"
        );
    }

    #[test]
    fn list_display_is_decoded_but_types_and_markdown_are_literal() {
        let source = "---\nitems:\n  - TRUE\n  - 2026-10-13\n  - 0x10\n  - null\n  - \"true\"\n  - 'it''s, 日🙂'\n  - \"a\\nb\\t🙂\"\n  - ==literal==\n  - O'Reilly\n---";
        let (buffer, found) = parsed(source);
        assert_eq!(found.len(), 1);
        let texts: Vec<_> = found[0]
            .items
            .iter()
            .map(|item| item.display_text.as_str())
            .collect();
        assert_eq!(
            texts,
            [
                "TRUE",
                "2026-10-13",
                "0x10",
                "null",
                "true",
                "it's, 日🙂",
                "a b\t🙂",
                "==literal==",
                "O'Reilly"
            ]
        );
        assert_eq!(
            buffer.text_for_range(found[0].items[5].source_range.clone()),
            "'it''s, 日🙂'"
        );
    }

    #[test]
    fn active_any_physical_row_carets_selections_and_ime_crlf() {
        let source = "---\r\nitems: # header\r\n  - 日🙂\r\n\r\n# between\r\n  - 本\r\nnext: keep\r\n---\r\nbody";
        let (buffer, found) = parsed(source);
        let group = &found[0];
        assert_eq!(group.source_lines, 1..6);
        for line in group.source_lines.clone() {
            for offset in [
                buffer.line_start(line),
                buffer.line_end(line) - 1,
                buffer.line_end(line),
            ] {
                assert!(
                    group_is_active(&buffer, group, &[offset..offset]),
                    "line {line}, {offset}"
                );
            }
        }
        let start = buffer.line_start(1);
        let end = buffer.line_start(6);
        assert!(!group_is_active(&buffer, group, &[0..start]));
        assert!(!group_is_active(&buffer, group, &[end..buffer.len()]));
        assert!(!group_is_active(&buffer, group, &[0..0, end..end]));
        assert!(group_is_active(
            &buffer,
            group,
            &[0..0, start - 1..start + 1]
        ));
        let scalar = group.items[1].source_range.clone();
        assert!(group_is_active(&buffer, group, &[scalar]));
        assert!(!group_is_active(&buffer, group, &[]));
    }

    #[test]
    fn block_duplicate_items_are_removed_by_index() {
        let source =
            "---\nitems:\n  - same # first\n  - same # clicked\n  - same # last\nnext: keep\n---";
        assert_eq!(
            removed(source, 1, 1),
            "---\nitems:\n  - same # first\n   # clicked\n  - same # last\nnext: keep\n---"
        );
        let source = "---\nitems:\n- same\n- same\n- other\nnext: keep\n---";
        assert_eq!(
            removed(source, 1, 1),
            "---\nitems:\n- same\n- other\nnext: keep\n---"
        );
    }

    #[test]
    fn last_block_item_becomes_empty_sequence_retaining_all_comments_and_crlf() {
        for eol in ["\n", "\r\n"] {
            let source = "---\nparent:\n  '日🙂': # header\n\n# before\n    - \"本🙂\"  # item\n  # after\n  sibling: keep\n...\nbody🙂\n".replace('\n', eol);
            let expected = "---\nparent:\n  '日🙂': [] # header\n\n# before\n      # item\n  # after\n  sibling: keep\n...\nbody🙂\n".replace('\n', eol);
            assert_eq!(removed(&source, 2, 0), expected);
            let metadata = expected
                .lines()
                .skip(1)
                .take_while(|row| *row != "...")
                .collect::<Vec<_>>()
                .join("\n");
            let value: serde_yaml::Value = serde_yaml::from_str(&metadata).unwrap();
            assert_eq!(
                value["parent"]["日🙂"],
                serde_yaml::Value::Sequence(Vec::new())
            );
        }
        assert_eq!(
            removed("---\nitems:\n  - one\nnext: keep\n---", 1, 0),
            "---\nitems: []\nnext: keep\n---"
        );
    }

    #[test]
    fn empty_and_null_block_scalars_have_exact_ranges_and_last_item_stays_a_list() {
        for (row, text) in [
            ("-", ""),
            ("- # item", ""),
            ("- null # item", "null"),
            ("- ~", "~"),
        ] {
            let source = format!("---\nitems:\n  {row}\n---");
            let (buffer, found) = parsed(&source);
            assert_eq!(found.len(), 1, "{source}");
            assert_eq!(found[0].items.len(), 1);
            assert_eq!(
                buffer.text_for_range(found[0].items[0].source_range.clone()),
                text
            );
            let result = removed(&source, 1, 0);
            assert!(result.contains("items: []\n"));
            assert_eq!(result.contains("# item"), source.contains("# item"));
            let metadata = result
                .lines()
                .skip(1)
                .take_while(|row| *row != "---")
                .collect::<Vec<_>>()
                .join("\n");
            let value: serde_yaml::Value = serde_yaml::from_str(&metadata).unwrap();
            assert_eq!(value["items"], serde_yaml::Value::Sequence(Vec::new()));
        }
    }

    #[test]
    fn empty_null_rows_with_extra_whitespace_keep_owned_group_focus_utf8_and_crlf() {
        for eol in ["\n", "\r\n"] {
            let source = [
                "---",
                "parent:",
                "  项目:",
                "      - 日🙂",
                "      -",
                "      -   ",
                "      -   # 空🙂",
                "      - null",
                "      - ~",
                "      - --literal",
                "  next: ok",
                "---",
            ]
            .join(eol);
            let (buffer, found) = parsed(&source);
            assert_eq!(found.len(), 1);
            let group = &found[0];
            assert_eq!(group.header_line, 2);
            assert_eq!(group.source_lines, 2..10);
            assert_eq!(group.items.len(), 7);
            for (item, token) in
                group
                    .items
                    .iter()
                    .zip(["日🙂", "", "", "", "null", "~", "--literal"])
            {
                assert_eq!(buffer.text_for_range(item.source_range.clone()), token);
                assert_eq!(item.display_text, token);
                assert!(buffer.text().is_char_boundary(item.source_range.start));
                assert!(buffer.text().is_char_boundary(item.source_range.end));
                let start = buffer.line_start(item.source_line);
                let end = buffer.line_end(item.source_line);
                for caret in [start, item.source_range.start, end - eol.len() + 1, end] {
                    assert!(group_is_active(&buffer, group, &[caret..caret]));
                }
                assert!(group_is_active(&buffer, group, &[start..end]));
            }
            assert!(!group_is_active(&buffer, group, &[0..0]));
            let sibling = buffer.line_start(10);
            assert!(!group_is_active(&buffer, group, &[sibling..sibling]));
            assert_eq!(buffer.text(), source);
        }
        // Multiple structural dashes form a nested collection, not scalar pills.
        for body in ["  -   -", "  -\n    - null", "  -\t  "] {
            let source = format!("---\nitems:\n{body}\n---");
            assert!(parsed(&source).1.is_empty(), "{source}");
        }
        // An incomplete sibling degrades alone; the valid list still renders.
        let (_, found) = parsed("---\nitems:\n  - null\nbroken: [\n---");
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].items.len(), 1);
        assert_eq!(found[0].items[0].display_text, "null");
    }

    #[test]
    fn flow_lists_handle_quoted_commas_escapes_duplicate_indices_and_trailing_comma() {
        let source = "---\n'日🙂': [\"a,b\", 'it''s, c', \"a\\\"b\", O'Reilly, \"a,b\",] # header\nnext: keep\n---";
        let (buffer, found) = parsed(source);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].source_lines, 1..2);
        let texts: Vec<_> = found[0]
            .items
            .iter()
            .map(|item| item.display_text.as_str())
            .collect();
        assert_eq!(texts, ["a,b", "it's, c", "a\"b", "O'Reilly", "a,b"]);
        assert_eq!(
            buffer.text_for_range(found[0].items[0].source_range.clone()),
            "\"a,b\""
        );
        assert_eq!(
            removed(source, 1, 0),
            "---\n'日🙂': ['it''s, c', \"a\\\"b\", O'Reilly, \"a,b\",] # header\nnext: keep\n---"
        );
        assert_eq!(
            removed(source, 1, 4),
            "---\n'日🙂': [\"a,b\", 'it''s, c', \"a\\\"b\", O'Reilly,] # header\nnext: keep\n---"
        );
        assert_eq!(
            removed("---\nitems: [a, a, b]\n---", 1, 1),
            "---\nitems: [a, b]\n---"
        );
        assert_eq!(
            removed("---\nitems: [ a , b , c ]\n---", 1, 1),
            "---\nitems: [ a , c ]\n---"
        );
    }

    #[test]
    fn last_flow_item_is_exact_empty_sequence_without_losing_suffix() {
        for token in ["[one]", "[ '日🙂' , ]", "[null]"] {
            let source =
                format!("---\r\n  'key🙂': {token}  # keep\r\n  sibling: true\r\n...\r\nbody🙂");
            assert_eq!(
                removed(&source, 1, 0),
                "---\r\n  'key🙂': []  # keep\r\n  sibling: true\r\n...\r\nbody🙂"
            );
        }
        let (buffer, found) = parsed("---\nitems: [] # keep\n---");
        assert_eq!(found.len(), 1);
        assert!(found[0].items.is_empty());
        assert!(remove_list_item_edit(&buffer, &parse_decorations(buffer.text()), 1, 0).is_none());
    }

    #[test]
    fn complex_malformed_and_node_properties_are_not_hidden_or_rewritten() {
        for token in [
            "[one, {a: b}]",
            "[one, [two]]",
            "[one, &anchor two]",
            "[one, !tag two]",
            "[one, *alias]",
            "[a,,b]",
            "[,a]",
            "[\"unterminated]",
            "[a: b]",
            "\n  - one\n  - {a: b}",
            "\n  - one\n  - key: value",
            "\n  - one\n  - - two",
            "\n  - one\n  - &anchor two",
            "\n  - one\n  - !tag two",
            "\n  - one\n  - *alias",
            "\n  - one\n  - |-\n    multiline",
            "\n  - one\n    continuation",
            "[\n  one,\n  two\n]",
        ] {
            let source = format!("---\nitems: {token}\nnext: keep\n---");
            let (buffer, found) = parsed(&source);
            assert!(
                !found.iter().any(|group| group.header_line == 1),
                "{source}"
            );
            assert!(
                remove_list_item_edit(&buffer, &parse_decorations(&source), 1, 0).is_none(),
                "{source}"
            );
            assert_eq!(buffer.text(), source);
        }
        for token in ["&anchor |-", "!tag >-", "|0", "|--", "\"unterminated"] {
            let source = format!("---\nvalue: {token}\n  body\nnext: keep\n---");
            assert!(parsed(&source).1.is_empty(), "{source}");
        }
    }

    #[test]
    fn invalid_indices_and_non_list_headers_are_no_ops() {
        for (source, line, index) in [
            ("---\nitems: [one]\n---", 1, 1),
            ("---\nitems:\n  - one\n---", 1, usize::MAX),
            ("---\nvalue: text\n---", 1, 0),
            ("---\nitems: [one]\n---", 0, 0),
            ("---\nitems: [one]\n---", 2, 0),
            ("---\nitems: [one]\n---", usize::MAX, 0),
        ] {
            let buffer = Buffer::from_text(source);
            assert!(
                remove_list_item_edit(&buffer, &parse_decorations(source), line, index).is_none()
            );
            assert_eq!(buffer.text(), source);
        }
    }

    #[test]
    fn deletion_is_one_recorded_edit_remaps_utf8_positions_and_does_not_focus_list() {
        let source = "---\r\nitems:\r\n  - 日🙂\r\n  - 本\r\n---\r\nbody🙂";
        let mut buffer = Buffer::from_text(source);
        let decorations = parse_decorations(source);
        let (range, replacement) = remove_list_item_edit(&buffer, &decorations, 1, 0).unwrap();
        let body = source.find("body").unwrap();
        let mut cursor = Cursor {
            offset: body,
            anchor: body,
        };
        let mut extra = [Cursor {
            offset: source.len(),
            anchor: source.len(),
        }];
        let mut marked = Some(body..source.len());
        let mut extra_marked = [body..body + 4];
        let mut affinity = None;
        buffer.begin_edit_recording();
        buffer.replace(range.clone(), &replacement);
        super::super::remap_source_edit_positions(
            &mut cursor,
            &mut extra,
            &mut marked,
            &mut extra_marked,
            &mut affinity,
            &range,
            replacement.len(),
        );
        let edits = buffer.finish_edit_recording();
        assert_eq!(edits.len(), 1);
        assert_eq!(&buffer.text()[cursor.offset..cursor.offset + 4], "body");
        assert_eq!(cursor.offset, cursor.anchor);
        assert_eq!(extra[0].offset, buffer.len());
        assert_eq!(marked, Some(cursor.offset..buffer.len()));
        assert_eq!(extra_marked, [cursor.offset..cursor.offset + 4]);
        let found = groups(&buffer, &parse_decorations(buffer.text()));
        assert!(!group_is_active(
            &buffer,
            &found[0],
            &[
                cursor.offset..cursor.offset,
                extra[0].offset..extra[0].offset
            ]
        ));
        assert!(buffer.apply_edits_inverse(&edits));
        assert_eq!(buffer.text(), source);
        assert!(buffer.apply_edits_forward(&edits));
        assert_eq!(buffer.text(), "---\r\nitems:\r\n  - 本\r\n---\r\nbody🙂");
    }

    #[test]
    fn read_only_is_a_caller_policy_pure_plan_does_not_mutate_source() {
        let source = "---\nitems: [\"日🙂\", false]\n---";
        let buffer = Buffer::from_text(source);
        // No editor/read-only state belongs in the pure planner. The handler's
        // guard precedes planning/checkpointing; a read-only caller may inspect
        // this plan without performing an edit or creating undo history.
        assert!(remove_list_item_edit(&buffer, &parse_decorations(source), 1, 0).is_some());
        assert_eq!(buffer.text(), source);
    }

    #[test]
    fn flow_mappings_render_entries_as_owned_pills_and_remove_exact_entries() {
        let source = "---\nitems: {a: 1, 'b c': \"x, y\", d: true,} # keep\nnext: keep\n---";
        let (buffer, found) = parsed(source);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].source_lines, 1..2);
        assert_eq!(found[0].kind, GroupKind::List);
        let texts: Vec<_> = found[0]
            .items
            .iter()
            .map(|item| item.display_text.as_str())
            .collect();
        assert_eq!(texts, ["a: 1", "b c: x, y", "d: true"]);
        assert_eq!(
            buffer.text_for_range(found[0].items[1].source_range.clone()),
            "'b c': \"x, y\""
        );
        assert_eq!(
            removed(source, 1, 1),
            "---\nitems: {a: 1, d: true,} # keep\nnext: keep\n---"
        );
        assert_eq!(
            removed("---\nitems: {a: 1}\n---", 1, 0),
            "---\nitems: {}\n---"
        );
    }

    #[test]
    fn flow_mappings_reject_nested_keyless_and_malformed_entries() {
        for token in [
            "{a: {b: 1}}",
            "{a}",
            "{a:}",
            "{a: b: c}",
            "{a: 1, a: 2}",
            "{[a]: 1}",
            "{a: *x}",
            "{a: &x 1}",
            "{a: 1, , b: 2}",
            "{a:1}",
        ] {
            let source = format!("---\nitems: {token}\nnext: keep\n---");
            let (buffer, found) = parsed(&source);
            assert!(
                !found.iter().any(|group| group.header_line == 1),
                "{source}"
            );
            assert!(
                remove_list_item_edit(&buffer, &parse_decorations(&source), 1, 0).is_none(),
                "{source}"
            );
            assert_eq!(buffer.text(), source);
        }
    }
}
