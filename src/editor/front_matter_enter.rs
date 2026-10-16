//! YAML-aware Enter plans. All offsets are source UTF-8 bytes; presentation
//! columns and property names never determine continuation indentation.
use std::ops::Range;

use gpui::Context;

use crate::buffer::Buffer;
use crate::front_matter::{FrontMatterValue, decoded_char_ranges};
use crate::markdown::{self, LineDecoration};

use super::front_matter_groups::{self, GroupKind, ListItem, ValueGroup};
use super::{Cursor, EditorView};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct EnterEdit {
    pub range: Range<usize>,
    pub replacement: String,
    /// Relative to `range.start`, not necessarily the end of the replacement.
    pub cursor: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CursorEnterEdit {
    pub cursor_index: usize,
    pub edit: EnterEdit,
}

pub(super) fn rows(buffer: &Buffer, decorations: &[LineDecoration]) -> Option<Range<usize>> {
    let table = decorations.first()?.table_info.as_ref()?;
    if !table.is_front_matter
        || table.table_start_line != 0
        || table.table_end_line < 2
        || table.table_end_line > buffer.line_count().min(decorations.len())
    {
        return None;
    }
    let opening = buffer.line_content(0).trim_start_matches('\u{feff}').trim();
    let closing = buffer.line_content(table.table_end_line - 1).trim();
    if !matches!((opening, closing), ("---", "---" | "...") | ("-", "-")) {
        return None;
    }
    Some(1..table.table_end_line - 1)
}

fn source(buffer: &Buffer, line: usize) -> &str {
    buffer.line_content(line).trim_end_matches('\r')
}

fn indent(text: &str) -> usize {
    text.len() - text.trim_start_matches(' ').len()
}

fn content_end(buffer: &Buffer, line: usize) -> usize {
    buffer.line_start(line) + source(buffer, line).len()
}

fn eol(buffer: &Buffer, line: usize) -> &'static str {
    if buffer.line_content(line).ends_with('\r') {
        "\r\n"
    } else {
        "\n"
    }
}

fn insertion(offset: usize, text: String) -> EnterEdit {
    EnterEdit {
        range: offset..offset,
        cursor: text.len(),
        replacement: text,
    }
}

/// Also includes delimiter rows: these must never become Markdown `<br>` cells.
pub(super) fn touches_front_matter(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    selection: &Range<usize>,
) -> bool {
    let Some(rows) = rows(buffer, decorations) else {
        return false;
    };
    let end = buffer.line_end(rows.end);
    selection.start <= end && (selection.is_empty() || selection.end > 0)
}

fn block_indent(buffer: &Buffer, decorations: &[LineDecoration], header: usize) -> Option<usize> {
    let info = decorations.get(header)?.front_matter.as_ref()?;
    let parent = info.key_range.as_ref()?.start;
    let token = source(buffer, header).get(info.value_range.clone())?;
    if let Some(extra) = token.bytes().find(|byte| matches!(byte, b'1'..=b'9')) {
        return Some(parent + usize::from(extra - b'0'));
    }
    let rows = rows(buffer, decorations)?;
    for line in header + 1..rows.end {
        let text = source(buffer, line);
        if text.trim().is_empty() {
            continue;
        }
        return Some(if indent(text) > parent {
            indent(text)
        } else {
            parent + 2
        });
    }
    Some(parent + 2)
}

/// Empty bodies can lack a display group. Lexical header/body metadata still
/// owns their blank rows, including rows with less than the required indent.
pub(super) fn block_owner(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    line: usize,
) -> Option<usize> {
    let rows = rows(buffer, decorations)?;
    for header in (rows.start..=line).rev() {
        let info = decorations.get(header)?.front_matter.as_ref()?;
        if info.key_range.is_none() || !matches!(info.value, FrontMatterValue::BlockHeader) {
            continue;
        }
        let required = block_indent(buffer, decorations, header)?;
        if (header + 1..=line).all(|row| {
            let text = source(buffer, row);
            text.trim().is_empty()
                || (indent(text) >= required
                    && decorations[row].front_matter.as_ref().is_some_and(|info| {
                        info.key_range.is_none() && info.display_prefix.is_empty()
                    }))
        }) {
            return Some(header);
        }
    }
    None
}

fn body_edit(buffer: &Buffer, selection: Range<usize>, required: usize) -> Option<EnterEdit> {
    let first = buffer.offset_to_line_col(selection.start).0;
    let last = buffer.offset_to_line_col(selection.end).0;
    let first_start = buffer.line_start(first);
    let last_start = buffer.line_start(last);
    let first_text = source(buffer, first);
    let last_text = source(buffer, last);
    // Splitting in structural whitespace must pad BOTH sides. Simply inserting
    // a newline at column zero would dedent the original body on the right.
    let start = selection.start.min(content_end(buffer, first));
    let end = selection.end.min(content_end(buffer, last));
    let structural_prefix = first_text
        .get(..start - first_start)?
        .bytes()
        .all(|byte| byte == b' ');
    let left_padding = " ".repeat(if structural_prefix {
        required.saturating_sub(start - first_start)
    } else {
        0
    });
    let right_start = (last_start + required.min(indent(last_text))).max(end);
    let replacement = format!(
        "{left_padding}{}{}",
        eol(buffer, first),
        " ".repeat(required)
    );
    let edit = EnterEdit {
        range: start..right_start,
        cursor: replacement.len(),
        replacement,
    };
    Some(edit)
}

fn quoted(text: &str, quote: Option<u8>) -> Option<String> {
    if quote == Some(b'\'') && !text.contains(['\n', '\r', '\u{85}', '\u{2028}', '\u{2029}']) {
        Some(format!("'{}'", text.replace('\'', "''")))
    } else {
        Some(
            serde_json::to_string(text)
                .ok()?
                .replace('\u{85}', "\\u0085")
                .replace('\u{2028}', "\\u2028")
                .replace('\u{2029}', "\\u2029"),
        )
    }
}

/// Decode atomically: a caret in `\u1234`, `\\` or `''` cannot bisect that
/// escape and leave two malformed scalar tokens.
fn fragment(token: &str, offset: usize, left: bool) -> Option<String> {
    if token.starts_with(['\'', '"']) {
        let decoded = serde_yaml::from_str::<String>(token).ok()?;
        let spans = decoded_char_ranges(token, &decoded)?;
        let boundary = spans.iter().take_while(|span| span.end <= offset).count();
        let text: String = if left {
            decoded.chars().take(boundary).collect()
        } else {
            decoded.chars().skip(boundary).collect()
        };
        quoted(&text, token.as_bytes().first().copied())
    } else {
        let text = if left {
            token.get(..offset)?
        } else {
            token.get(offset..)?
        }
        .trim();
        if text.is_empty() {
            return Some(String::new()); // A bare dash is a valid YAML null item.
        }
        match serde_yaml::from_str::<serde_yaml::Value>(text) {
            Ok(serde_yaml::Value::String(decoded)) if decoded == text => Some(text.to_owned()),
            _ => quoted(text, None),
        }
    }
}

fn item_at(group: &ValueGroup, line: usize) -> Option<&ListItem> {
    group.items.iter().find(|item| item.source_line == line)
}

fn list_edit(buffer: &Buffer, group: &ValueGroup, selection: Range<usize>) -> Option<EnterEdit> {
    let first_line = buffer.offset_to_line_col(selection.start).0;
    let last_line = buffer.offset_to_line_col(selection.end).0;
    let first = item_at(group, first_line)?;
    let last = item_at(group, last_line)?;
    let first_indent = indent(source(buffer, first_line));
    if first_indent != indent(source(buffer, last_line)) {
        return None;
    }
    let marker = format!("{}- ", " ".repeat(first_indent));
    // A flow item has no source dash; never add a block marker inside `[...]`.
    if !source(buffer, first_line)
        .get(first_indent..)?
        .starts_with('-')
    {
        return None;
    }
    let first_token = buffer.text_for_range(first.source_range.clone());
    let last_token = buffer.text_for_range(last.source_range.clone());
    let start = selection
        .start
        .clamp(first.source_range.start, first.source_range.end);
    let end = selection
        .end
        .clamp(last.source_range.start, last.source_range.end);
    let at_quoted_end =
        first_token.starts_with(['\'', '"']) && start >= first.source_range.end.saturating_sub(1);
    if selection.is_empty() && (start == first.source_range.end || at_quoted_end) {
        // Keep closing quotes and trailing comments on the existing item.
        return Some(insertion(
            content_end(buffer, first_line),
            format!("{}{marker}", eol(buffer, first_line)),
        ));
    }
    // A selection wholly in an inline comment is not scalar content.
    if !selection.is_empty() && selection.start > first.source_range.end {
        return None;
    }
    let left = fragment(first_token, start - first.source_range.start, true)?;
    let right = fragment(last_token, end - last.source_range.start, false)?;
    let suffix = if first_line == last_line {
        buffer.text_for_range(first.source_range.end..content_end(buffer, first_line))
    } else {
        ""
    };
    let prefix = format!("{left}{suffix}{}{marker}", eol(buffer, first_line));
    let cursor = prefix.len() + usize::from(right.starts_with(['\'', '"']));
    Some(EnterEdit {
        range: first.source_range.start..if first_line == last_line {
            content_end(buffer, last_line)
        } else {
            last.source_range.end
        },
        replacement: format!("{prefix}{right}"),
        cursor,
    })
}

fn plan_unchecked(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    selection: Range<usize>,
) -> Option<EnterEdit> {
    let rows = rows(buffer, decorations)?;
    let first_line = buffer.offset_to_line_col(selection.start).0;
    let last_line = buffer.offset_to_line_col(selection.end).0;
    if !rows.contains(&first_line) || !rows.contains(&last_line) {
        // Delimiter EOL is safe, but do not split a delimiter or select across it.
        return (selection.is_empty() && selection.start >= content_end(buffer, first_line)).then(
            || {
                insertion(
                    content_end(buffer, first_line),
                    eol(buffer, first_line).to_owned(),
                )
            },
        );
    }
    let groups = front_matter_groups::groups(buffer, decorations);
    if let Some(header) = block_owner(buffer, decorations, first_line) {
        if first_line == header {
            let info = decorations[header].front_matter.as_ref()?;
            if selection.is_empty()
                && selection.start >= buffer.line_start(header) + info.value_range.start
            {
                return Some(insertion(
                    content_end(buffer, header),
                    format!(
                        "{}{}",
                        eol(buffer, header),
                        " ".repeat(block_indent(buffer, decorations, header)?)
                    ),
                ));
            }
        } else if block_owner(buffer, decorations, last_line) == Some(header) {
            return body_edit(
                buffer,
                selection,
                block_indent(buffer, decorations, header)?,
            );
        }
        return None;
    }
    if let Some(group) = groups.iter().find(|group| {
        group.source_lines.contains(&first_line) && group.source_lines.contains(&last_line)
    }) {
        if group.kind == GroupKind::Multiline {
            let header = decorations[group.header_line].front_matter.as_ref()?;
            let required = (group.header_line + 1..group.source_lines.end)
                .find(|line| !source(buffer, *line).trim().is_empty())
                .map(|line| indent(source(buffer, line)))
                .unwrap_or(header.key_range.as_ref()?.start + 2);
            // Header keys and comments are structural, not text to split.
            if first_line == group.header_line
                && selection.start < buffer.line_start(first_line) + header.value_range.start
            {
                return None;
            }
            if source(buffer, first_line).trim_start().starts_with('#')
                && selection.start < content_end(buffer, first_line)
            {
                return None;
            }
            let info = decorations[first_line].front_matter.as_ref()?;
            if selection.is_empty()
                && selection.start >= buffer.line_start(first_line) + info.value_range.end
            {
                return Some(insertion(
                    content_end(buffer, first_line),
                    format!("{}{}", eol(buffer, first_line), " ".repeat(required)),
                ));
            }
            let last_start = buffer.line_start(last_line);
            if source(buffer, last_line)
                .get(selection.end - last_start..)?
                .trim_start()
                .starts_with('#')
            {
                return None; // Do not turn scalar text into a YAML comment.
            }
            return body_edit(buffer, selection, required);
        }
        if first_line != group.header_line {
            return list_edit(buffer, group, selection);
        }
    }
    // Editor-only empty metadata has table ownership but no per-row metadata.
    // Keep scalar/group handling above authoritative; only lexical blanks and
    // comments can use this safe EOL insertion without a value descriptor.
    let text = source(buffer, first_line);
    if first_line == last_line
        && selection.is_empty()
        && selection.start >= content_end(buffer, first_line)
        && (text.trim().is_empty() || text.trim_start().starts_with('#'))
    {
        return Some(insertion(
            content_end(buffer, first_line),
            format!("{}{}", eol(buffer, first_line), " ".repeat(indent(text))),
        ));
    }
    let info = decorations[first_line].front_matter.as_ref()?;
    if first_line == last_line && selection.is_empty() {
        if let Some(key) = &info.key_range {
            let text = source(buffer, first_line);
            let token = text.get(info.value_range.clone())?;
            let value_start = if token.is_empty() {
                key.end + text.get(key.end..info.value_range.start)?.find(':')? + 1
            } else {
                info.value_range.start
            };
            if selection.start >= buffer.line_start(first_line) + value_start {
                let group = groups
                    .iter()
                    .find(|group| group.header_line == first_line && group.kind == GroupKind::List);
                if token.is_empty()
                    && (group.is_some()
                        || super::front_matter::property_type(buffer, decorations, first_line)
                            == Some(super::front_matter::FrontMatterType::Empty))
                {
                    let required = group
                        .and_then(|group| group.items.first())
                        .map(|item| indent(source(buffer, item.source_line)))
                        .unwrap_or(key.start + 2);
                    return Some(insertion(
                        content_end(buffer, first_line),
                        format!("{}{}- ", eol(buffer, first_line), " ".repeat(required)),
                    ));
                }
                if group.is_some_and(|group| group.items.is_empty()) && token.starts_with('[') {
                    let suffix = text.get(info.value_range.end..)?;
                    let replacement = format!(
                        "{suffix}{}{}- ",
                        eol(buffer, first_line),
                        " ".repeat(key.start + 2)
                    );
                    return Some(EnterEdit {
                        range: buffer.line_start(first_line) + info.value_range.start
                            ..content_end(buffer, first_line),
                        cursor: replacement.len(),
                        replacement,
                    });
                }
            }
        }
        // Other YAML properties/comments support safe EOL source breaks, not
        // HTML breaks. Complex mid-token edits remain explicitly source-owned.
        if selection.start >= content_end(buffer, first_line) {
            return Some(insertion(
                content_end(buffer, first_line),
                format!(
                    "{}{}",
                    eol(buffer, first_line),
                    " ".repeat(indent(source(buffer, first_line)))
                ),
            ));
        }
    }
    None
}

fn valid_metadata_after_edit(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    edit: &EnterEdit,
) -> bool {
    let Some(rows) = rows(buffer, decorations) else {
        return false;
    };
    let start = buffer.line_start(rows.start);
    let end = buffer.line_start(rows.end);
    // Delimiter EOL edits are outside the YAML body.
    if edit.range.start < start || edit.range.start >= end {
        return true;
    }
    let mut yaml = buffer.text_for_range(start..end).to_owned();
    yaml.replace_range(
        edit.range.start - start..edit.range.end - start,
        &edit.replacement,
    );
    matches!(
        serde_yaml::from_str::<serde_yaml::Value>(&yaml),
        Ok(serde_yaml::Value::Mapping(_) | serde_yaml::Value::Null)
    )
}

/// One exact source replacement. `None` means unsupported/unsafe, not permission
/// to route front matter through the Markdown table Enter handler.
pub(super) fn enter_edit(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    selection: Range<usize>,
) -> Option<EnterEdit> {
    if selection.start > selection.end
        || selection.end > buffer.len()
        || !buffer.text().is_char_boundary(selection.start)
        || !buffer.text().is_char_boundary(selection.end)
        || !touches_front_matter(buffer, decorations, &selection)
    {
        return None;
    }
    let edit = plan_unchecked(buffer, decorations, selection)?;
    valid_metadata_after_edit(buffer, decorations, &edit).then_some(edit)
}

/// Pure atomic batch planning, descending in source order. Separate body/list
/// rows support multiple cursors; overlapping scalar rewrites are rejected
/// rather than silently applying a partially valid batch.
pub(super) fn enter_edits(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    selections: &[Range<usize>],
) -> Option<Vec<CursorEnterEdit>> {
    let mut targets: Vec<_> = selections.iter().cloned().enumerate().collect();
    targets.sort_by_key(|(_, range)| std::cmp::Reverse(range.start));
    let mut scratch = Buffer::from_text(buffer.text());
    let mut parsed = decorations.to_vec();
    let mut ceiling = buffer.len();
    let mut edits = Vec::with_capacity(targets.len());
    for (cursor_index, selection) in targets {
        let edit = enter_edit(&scratch, &parsed, selection)?;
        if edit.range.end > ceiling {
            return None;
        }
        ceiling = edit.range.start;
        scratch.replace(edit.range.clone(), &edit.replacement);
        parsed = markdown::parse_decorations(scratch.text());
        super::fold::decorate_empty_front_matter(&scratch, &mut parsed);
        edits.push(CursorEnterEdit { cursor_index, edit });
    }
    Some(edits)
}

impl EditorView {
    /// Both regular and soft Enter share YAML semantics. Return true even for
    /// protected syntax so front matter never falls through to `<br>`/Markdown.
    pub(super) fn insert_front_matter_break(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.active_tab_is_markdown() {
            return false;
        }
        let selections: Vec<_> = self
            .all_cursors_snapshot()
            .iter()
            .map(Cursor::selection_range)
            .collect();
        if !selections
            .iter()
            .any(|selection| touches_front_matter(&self.buffer, &self.decorations, selection))
        {
            return false;
        }
        let Some(edits) = enter_edits(&self.buffer, &self.decorations, &selections) else {
            return true;
        };
        self.pending_code_fence = None;
        self.checkpoint();
        for CursorEnterEdit { cursor_index, edit } in edits {
            self.queue_non_markdown_line_display_transfer(&edit.range, &edit.replacement);
            self.buffer.replace(edit.range.clone(), &edit.replacement);
            super::remap_source_edit_positions(
                &mut self.cursor,
                &mut self.extra_cursors,
                &mut self.marked_range,
                &mut self.extra_marked_ranges,
                &mut self.wrap_boundary_affinity,
                &edit.range,
                edit.replacement.len(),
            );
            let cursor = if cursor_index == 0 {
                &mut self.cursor
            } else {
                &mut self.extra_cursors[cursor_index - 1]
            };
            cursor.offset = edit.range.start + edit.cursor;
            cursor.anchor = cursor.offset;
        }
        for range in self
            .marked_range
            .iter_mut()
            .chain(self.extra_marked_ranges.iter_mut())
        {
            range.start = self.buffer.clamp_offset(range.start);
            range.end = self.buffer.clamp_offset(range.end);
        }
        self.reset_cursor_visual_targeting();
        self.dedup_cursors();
        self.reparse();
        self.scroll_to_cursor();
        self.pause_blinking(cx);
        cx.notify();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decorations(buffer: &Buffer) -> Vec<LineDecoration> {
        let mut decorations = markdown::parse_decorations(buffer.text());
        super::super::fold::decorate_empty_front_matter(buffer, &mut decorations);
        decorations
    }

    fn apply(marked: &str) -> (String, usize) {
        let start = marked.find('¦').unwrap();
        let source = marked.replacen('¦', "", 1);
        let end = source.find('¦').unwrap_or(start);
        let source = source.replacen('¦', "", 1);
        let mut buffer = Buffer::from_text(&source);
        let decorations = decorations(&buffer);
        let edit = enter_edit(&buffer, &decorations, start..end)
            .unwrap_or_else(|| panic!("no plan: {marked:?}"));
        let cursor = edit.range.start + edit.cursor;
        buffer.begin_edit_recording();
        buffer.replace(edit.range, &edit.replacement);
        let edits = buffer.finish_edit_recording();
        assert_eq!(edits.len(), 1);
        let result = buffer.text().to_owned();
        assert!(result.is_char_boundary(cursor));
        let parsed = self::decorations(&buffer);
        let rows = rows(&buffer, &parsed).unwrap();
        assert!(
            matches!(
                serde_yaml::from_str::<serde_yaml::Value>(
                    buffer
                        .text_for_range(buffer.line_start(rows.start)..buffer.line_start(rows.end))
                ),
                Ok(serde_yaml::Value::Mapping(_) | serde_yaml::Value::Null)
            ),
            "{result:?}"
        );
        assert!(buffer.apply_edits_inverse(&edits));
        assert_eq!(buffer.text(), source);
        assert!(buffer.apply_edits_forward(&edits));
        assert_eq!(buffer.text(), result);
        (result, cursor)
    }

    #[test]
    fn empty_and_comment_only_metadata_eol_breaks_without_row_metadata() {
        for (marked, expected) in [
            ("---\n¦\n---\nBody", "---\n\n\n---\nBody"),
            ("---\n  ¦\n---\nBody", "---\n  \n  \n---\nBody"),
            (
                "---\n# 日🙂 comment¦\n---\nBody",
                "---\n# 日🙂 comment\n\n---\nBody",
            ),
            (
                "---\n  # comment¦\n...\nBody",
                "---\n  # comment\n  \n...\nBody",
            ),
            ("---\r\n¦\r\n---\r\nBody", "---\r\n\r\n\r\n---\r\nBody"),
            ("---\r\n\r¦\n---\r\nBody", "---\r\n\r\n\r\n---\r\nBody"),
            (
                "\u{feff}---\r\n  # 日🙂 comment¦\r\n...\r\nBody",
                "\u{feff}---\r\n  # 日🙂 comment\r\n  \r\n...\r\nBody",
            ),
            ("---\n{}\n¦\n---\nBody", "---\n{}\n\n\n---\nBody"),
            (
                "---\n{}\n# comment¦\n---\nBody",
                "---\n{}\n# comment\n\n---\nBody",
            ),
        ] {
            let offset = marked.find('¦').unwrap();
            let source = marked.replace('¦', "");
            let buffer = Buffer::from_text(&source);
            let parsed = decorations(&buffer);
            let line = buffer.offset_to_line_col(offset).0;
            assert!(parsed[line].front_matter.is_none(), "{marked:?}");
            assert!(touches_front_matter(&buffer, &parsed, &(offset..offset)));
            let (result, cursor) = apply(marked);
            assert_eq!(result, expected, "{marked:?}");
            let buffer = Buffer::from_text(&result);
            assert_eq!(cursor, content_end(&buffer, line + 1));
        }
    }

    #[test]
    fn empty_metadata_breaks_require_front_matter_ownership_and_safe_eol() {
        for marked in [
            "---\n# com¦ment\n---\nBody",
            "---\n¦  \n---\nBody",
            "---\n# ¦comment¦\n---\nBody",
            "---\n¦\n---¦\nBody",
            "---\n\n---\nBody¦",
        ] {
            let start = marked.find('¦').unwrap();
            let source = marked.replacen('¦', "", 1);
            let end = source.find('¦').unwrap_or(start);
            let source = source.replacen('¦', "", 1);
            let buffer = Buffer::from_text(&source);
            assert!(enter_edit(&buffer, &decorations(&buffer), start..end).is_none());
        }
        for source in ["---\n\n---\nBody", "| A |\n| --- |\n| # comment |"] {
            let buffer = Buffer::from_text(source);
            let parsed = markdown::parse_decorations(source);
            let line = if source.starts_with('|') { 2 } else { 1 };
            let offset = content_end(&buffer, line);
            assert!(!touches_front_matter(&buffer, &parsed, &(offset..offset)));
            assert!(enter_edit(&buffer, &parsed, offset..offset).is_none());
        }
        let buffer = Buffer::from_text("---\nflag: true\n---\nBody");
        let mut parsed = decorations(&buffer);
        parsed[1].front_matter = None;
        let offset = content_end(&buffer, 1);
        assert!(touches_front_matter(&buffer, &parsed, &(offset..offset)));
        assert!(enter_edit(&buffer, &parsed, offset..offset).is_none());
    }

    #[test]
    fn empty_metadata_validation_accepts_null_or_mapping_but_not_other_yaml() {
        let buffer = Buffer::from_text("---\n\n---\nBody");
        let parsed = decorations(&buffer);
        for (yaml, valid) in [
            ("\n", true),
            ("# comment\n", true),
            ("{}\n", true),
            ("title: example\n", true),
            ("null\n", true),
            ("[]\n", false),
            ("plain scalar\n", false),
            ("title: [\n", false),
        ] {
            let edit = insertion(buffer.line_start(1), yaml.to_owned());
            assert_eq!(
                valid_metadata_after_edit(&buffer, &parsed, &edit),
                valid,
                "{yaml:?}"
            );
        }
    }

    #[test]
    fn empty_metadata_multicursor_breaks_are_grouped_and_remap_ime_and_undo() {
        let source = "---\r\n  \r\n  # 日🙂 comment\r\n\r\n---\r\nBody";
        let mut buffer = Buffer::from_text(source);
        let selections = vec![
            content_end(&buffer, 1)..content_end(&buffer, 1),
            content_end(&buffer, 2)..content_end(&buffer, 2),
            content_end(&buffer, 3)..content_end(&buffer, 3),
        ];
        let parsed = decorations(&buffer);

        let body = source.find("Body").unwrap();
        assert!(enter_edits(&buffer, &parsed, &[selections[0].clone(), body..body]).is_none());
        assert_eq!(buffer.text(), source);
        let plans = enter_edits(&buffer, &parsed, &selections).unwrap();
        let mut cursor = Cursor {
            offset: selections[0].start,
            anchor: selections[0].end,
        };
        let mut extra: Vec<_> = selections[1..]
            .iter()
            .map(|selection| Cursor {
                offset: selection.start,
                anchor: selection.end,
            })
            .collect();
        let mut marked = Some(source.find("Body").unwrap()..source.len());
        let comment = source.find('日').unwrap();
        let mut extra_marked = vec![comment..comment + "日🙂".len()];
        let mut affinity = None;
        buffer.begin_edit_recording();
        for CursorEnterEdit { cursor_index, edit } in plans {
            buffer.replace(edit.range.clone(), &edit.replacement);
            super::super::remap_source_edit_positions(
                &mut cursor,
                &mut extra,
                &mut marked,
                &mut extra_marked,
                &mut affinity,
                &edit.range,
                edit.replacement.len(),
            );
            let target = if cursor_index == 0 {
                &mut cursor
            } else {
                &mut extra[cursor_index - 1]
            };
            target.offset = edit.range.start + edit.cursor;
            target.anchor = target.offset;
        }
        let edits = buffer.finish_edit_recording();
        assert_eq!(edits.len(), 3);
        let expected = "---\r\n  \r\n  \r\n  # 日🙂 comment\r\n  \r\n\r\n\r\n---\r\nBody";
        assert_eq!(buffer.text(), expected);
        for (target, line) in [&cursor, &extra[0], &extra[1]].into_iter().zip([2, 4, 6]) {
            assert_eq!(target.offset, content_end(&buffer, line));
            assert_eq!(target.anchor, target.offset);
        }
        assert_eq!(buffer.text_for_range(marked.unwrap()), "Body");
        assert_eq!(buffer.text_for_range(extra_marked[0].clone()), "日🙂");
        assert!(buffer.apply_edits_inverse(&edits));
        assert_eq!(buffer.text(), source);
        assert!(buffer.apply_edits_forward(&edits));
        assert_eq!(buffer.text(), expected);
    }

    #[test]
    fn block_scalar_root_nested_inferred_explicit_utf8_and_crlf() {
        for (marked, expected) in [
            (
                "---\nbody: |-\n  日¦🙂本\n---",
                "---\nbody: |-\n  日\n  🙂本\n---",
            ),
            (
                "---\nparent:\n  body: >3-\n     日🙂¦\n  sibling: yes\n---",
                "---\nparent:\n  body: >3-\n     日🙂\n     \n  sibling: yes\n---",
            ),
            (
                "---\r\nbody: |+\r\n    日¦🙂\r\n---\r\n",
                "---\r\nbody: |+\r\n    日\r\n    🙂\r\n---\r\n",
            ),
            (
                "---\nbody: |2-\n    more¦ text\n---",
                "---\nbody: |2-\n    more\n   text\n---",
            ),
        ] {
            assert_eq!(apply(marked).0, expected);
        }
    }

    #[test]
    fn body_indent_is_repaired_before_content_and_on_blank_rows() {
        for (marked, expected) in [
            (
                "---\nbody: |-\n¦  text\n---",
                "---\nbody: |-\n  \n  text\n---",
            ),
            (
                "---\nbody: |-\n ¦ text\n---",
                "---\nbody: |-\n  \n  text\n---",
            ),
            (
                "---\nbody: |4-\n¦\n    text\n---",
                "---\nbody: |4-\n    \n    \n    text\n---",
            ),
            (
                "---\nbody: |-\n  a¦bc\n  de¦fg\nnext: ok\n---",
                "---\nbody: |-\n  a\n  fg\nnext: ok\n---",
            ),
        ] {
            assert_eq!(apply(marked).0, expected);
        }
    }

    #[test]
    fn headers_create_indented_bodies_and_keep_comments_and_siblings() {
        for (marked, expected) in [
            (
                "---\nbody: |¦- # keep\nnext: ok\n---",
                "---\nbody: |- # keep\n  \nnext: ok\n---",
            ),
            (
                "---\nparent:\n  body: |-2¦\n---",
                "---\nparent:\n  body: |-2\n    \n---",
            ),
            (
                "---\nitems:¦ # keep\nnext: ok\n---",
                "---\nitems: # keep\n  - \nnext: ok\n---",
            ),
            (
                "---\nitems:¦\n- existing\n---",
                "---\nitems:\n- \n- existing\n---",
            ),
            (
                "---\r\nparent:\r\n  items: [ ]¦  # keep\r\n  next: ok\r\n---",
                "---\r\nparent:\r\n  items:   # keep\r\n    - \r\n  next: ok\r\n---",
            ),
        ] {
            assert_eq!(apply(marked).0, expected);
        }
    }

    #[test]
    fn lists_keep_source_indent_empty_null_items_and_close_quotes_before_marker() {
        for (marked, expected) in [
            (
                "---\nitems:\n  - '日🙂'¦ # keep\nnext: ok\n---",
                "---\nitems:\n  - '日🙂' # keep\n  - \nnext: ok\n---",
            ),
            (
                "---\nitems:\n  - '日🙂¦' # keep\n---",
                "---\nitems:\n  - '日🙂' # keep\n  - \n---",
            ),
            (
                "---\nparent:\n  items:\n      - ¦\n  next: ok\n---",
                "---\nparent:\n  items:\n      - \n      - \n  next: ok\n---",
            ),
            (
                "---\r\nitems:\r\n- plain¦\r\n---",
                "---\r\nitems:\r\n- plain\r\n- \r\n---",
            ),
            ("---\nitems:\n  - ¦\n---", "---\nitems:\n  - \n  - \n---"),
        ] {
            assert_eq!(apply(marked).0, expected);
        }
    }

    #[test]
    fn enter_empty_null_list_rows_stay_aligned_owned_and_prefixed_before_and_after_input() {
        // Cover the reported two-space row, indentless YAML, and nonstandard
        // source indent without deriving it from the aligned display column.
        for eol in ["\n", "\r\n"] {
            for source_indent in [0, 2, 6] {
                let prefix = format!("{}- ", " ".repeat(source_indent));
                let marked = [
                    "---".to_owned(),
                    "long_property_项目:".into(),
                    format!("{prefix}日🙂¦"),
                    "next: keep".into(),
                    "---".into(),
                ]
                .join(eol);
                let (result, cursor) = apply(&marked);
                let buffer = Buffer::from_text(&result);
                assert_eq!(buffer.line_content(3).trim_end_matches('\r'), prefix);
                assert_eq!(buffer.offset_to_line_col(cursor), (3, prefix.len()));
                assert_list_value_columns(&buffer, cursor, 1..4);
                let parsed = decorations(&buffer);
                let info = parsed[3].front_matter.as_ref().unwrap();
                assert_eq!(info.value_range, prefix.len()..prefix.len());
                assert_eq!(info.display_prefix, "• ");
                assert_eq!(info.indent_columns, 0);
                assert_eq!(info.value, FrontMatterValue::Text(String::new()));
            }
        }
        for eol in ["\n", "\r\n"] {
            for row in [
                "-",
                "- ",
                "-   ",
                "-    ",
                "- null",
                "- ~",
                "- NULL",
                "- --日🙂",
            ] {
                let marked = [
                    "---".to_owned(),
                    "parent:".into(),
                    "  项目:".into(),
                    "      - 日🙂".into(),
                    format!("      {row}¦"),
                    "  untouched: yes".into(),
                    "---".into(),
                ]
                .join(eol);
                let original = marked.replace('¦', "");
                let (result, cursor) = apply(&marked);
                assert_eq!(
                    result,
                    original.replacen(
                        &format!("      {row}{eol}"),
                        &format!("      {row}{eol}      - {eol}"),
                        1,
                    ),
                );
                let mut buffer = Buffer::from_text(&result);
                let parsed = decorations(&buffer);
                let info = parsed[5].front_matter.as_ref().unwrap();
                assert_eq!(buffer.offset_to_line_col(cursor), (5, 8));
                assert_eq!(info.value_range, 8..8);
                assert_eq!(info.value, FrontMatterValue::Text(String::new()));
                assert_eq!(info.display_prefix, "• ");
                assert_eq!(info.indent_columns, 2);
                assert_list_value_columns(&buffer, cursor, 2..6);

                // Enter again while still empty must remain in the same owned
                // sequence, not fall through to an unprefixed ordinary YAML row.
                let edit = enter_edit(&buffer, &parsed, cursor..cursor).unwrap();
                assert_eq!(edit.replacement, format!("{eol}      - "));
                let next_cursor = edit.range.start + edit.cursor;
                buffer.replace(edit.range, &edit.replacement);
                assert_list_value_columns(&buffer, next_cursor, 2..7);

                for input in ["日🙂", "--literal", "null", "~"] {
                    let mut typed = Buffer::from_text(buffer.text());
                    typed.insert(next_cursor, input);
                    let typed_cursor = next_cursor + input.len();
                    assert_list_value_columns(&typed, typed_cursor, 2..7);
                    let typed_parsed = decorations(&typed);
                    let info = typed_parsed[6].front_matter.as_ref().unwrap();
                    assert_eq!(info.value_range, 8..8 + input.len());
                    assert_eq!(info.display_prefix, "• ");
                    assert_eq!(info.indent_columns, 2);
                    assert_eq!(info.value, FrontMatterValue::Text(input.into()));
                    assert_eq!(
                        typed.line_content(6).trim_end_matches('\r'),
                        format!("      - {input}")
                    );
                }
            }
        }
    }

    fn assert_list_value_columns(buffer: &Buffer, caret: usize, owned: Range<usize>) {
        let parsed = decorations(buffer);
        let found = front_matter_groups::groups(buffer, &parsed);
        assert_eq!(found.len(), 1, "{:?}", buffer.text());
        let group = &found[0];
        assert_eq!(group.source_lines, owned);
        assert_eq!(group.items.len(), owned.len() - 1);
        assert!(front_matter_groups::group_is_active(
            buffer,
            group,
            &[caret..caret]
        ));
        let plans =
            super::super::helpers::front_matter_group_plan(buffer, &parsed, &[caret..caret], true);
        assert_eq!(plans.len(), 1);
        assert!(plans[0].active);
        for line in owned.start + 1..owned.end {
            let text = buffer.line_content(line);
            let mut decoration = parsed[line].clone();
            super::super::helpers::apply_front_matter_group_display(
                &mut decoration,
                line,
                Some(&plans[0]),
                text,
            );
            let info = decoration.front_matter.as_ref().unwrap();
            let (display, d2b, b2d, collapsed) = super::super::helpers::build_display_text(
                text,
                &decoration,
                &[0..text.len()],
                true,
                None,
                0,
                3,
                0,
                0,
                1,
                1.0,
                1.0,
                1.0,
            );
            let column =
                decoration.table_info.as_ref().unwrap().column_widths[0] + 2 + info.indent_columns;
            assert_eq!(
                display[..b2d[info.value_range.start]].chars().count(),
                column,
                "{text:?} => {display:?}"
            );
            assert_eq!(display.find('-'), Some(column - 2), "{display:?}");
            assert!(!collapsed);
            assert!(d2b.iter().all(|offset| text.is_char_boundary(*offset)));
            assert!(b2d.iter().all(|offset| display.is_char_boundary(*offset)));
        }
    }

    #[test]
    fn list_mid_token_splits_are_yaml_safe_including_atomic_escapes() {
        for (marked, expected) in [
            (
                "---\nitems:\n  - '日¦🙂' # keep\n---",
                "---\nitems:\n  - '日' # keep\n  - '🙂'\n---",
            ),
            (
                "---\nitems:\n  - 'it'¦'s fine'\n---",
                "---\nitems:\n  - 'it'\n  - '''s fine'\n---",
            ),
            (
                "---\nitems:\n  - \"a\\¦u65e5🙂\"\n---",
                "---\nitems:\n  - \"a\"\n  - \"日🙂\"\n---",
            ),
            (
                "---\nitems:\n  - \"a\\n¦b\"\n---",
                "---\nitems:\n  - \"a\\n\"\n  - \"b\"\n---",
            ),
            (
                "---\nitems:\n  - left¦#right\n---",
                "---\nitems:\n  - left\n  - \"#right\"\n---",
            ),
            (
                "---\nitems:\n  - 'ab¦cd'\n  - 'ef¦gh' # last\nnext: yes\n---",
                "---\nitems:\n  - 'ab'\n  - 'gh' # last\nnext: yes\n---",
            ),
        ] {
            let (result, cursor) = apply(marked);
            assert_eq!(result, expected);
            assert!(!result[..cursor].ends_with("<br>"));
        }
    }

    #[test]
    fn ordinary_yaml_eol_is_not_an_html_break_and_unsafe_splits_are_protected() {
        assert_eq!(apply("---\nflag: true¦\n---").0, "---\nflag: true\n\n---");
        for marked in [
            "---\nfl¦ag: true\n---",
            "---\nitems: [one¦, two]\n---",
            "---\nobject: {key: va¦lue}\n---",
            "---\nitems:\n  -\t  ¦\n---",
        ] {
            let offset = marked.find('¦').unwrap();
            let source = marked.replace('¦', "");
            let buffer = Buffer::from_text(&source);
            assert!(
                enter_edit(
                    &buffer,
                    &markdown::parse_decorations(&source),
                    offset..offset
                )
                .is_none()
            );
        }
        let source = "| A |\n| --- |\n| B |";
        let buffer = Buffer::from_text(source);
        assert!(!touches_front_matter(
            &buffer,
            &markdown::parse_decorations(source),
            &(5..5)
        ));
    }

    #[test]
    fn plain_multiline_groups_continue_source_indent_and_protect_hash_content() {
        assert_eq!(
            apply("---\nvalue: fir¦st\n    second\nnext: ok\n---").0,
            "---\nvalue: fir\n    st\n    second\nnext: ok\n---"
        );
        assert_eq!(
            apply("---\nvalue: first\n    second¦ # keep\nnext: ok\n---").0,
            "---\nvalue: first\n    second # keep\n    \nnext: ok\n---"
        );
        let source = "---\nvalue: first\n    left#right\n---";
        let offset = source.find('#').unwrap();
        let buffer = Buffer::from_text(source);
        assert!(
            enter_edit(
                &buffer,
                &markdown::parse_decorations(source),
                offset..offset
            )
            .is_none()
        );
    }

    #[test]
    fn quoted_escaped_and_crlf_boundary_items_keep_valid_continuations() {
        for (marked, expected) in [
            (
                "---\nitems:\n  - \"a\\\\¦b\"\n---",
                "---\nitems:\n  - \"a\\\\\"\n  - \"b\"\n---",
            ),
            (
                "---\nitems:\n  - \"a\\¦\"b\" # keep\n---",
                "---\nitems:\n  - \"a\" # keep\n  - \"\\\"b\"\n---",
            ),
            (
                "---\r\nitems:\r\n  - '日¦🙂' # keep\r\n---",
                "---\r\nitems:\r\n  - '日' # keep\r\n  - '🙂'\r\n---",
            ),
            (
                "---\r\nitems:\r\n  - value\r¦\n---",
                "---\r\nitems:\r\n  - value\r\n  - \r\n---",
            ),
            (
                "---\r\nbody: |-\r\n  value\r¦\n---",
                "---\r\nbody: |-\r\n  value\r\n  \r\n---",
            ),
        ] {
            assert_eq!(apply(marked).0, expected);
        }
    }

    #[test]
    fn overlapping_and_mixed_cursor_batches_are_not_partially_applied() {
        let source = "---\nitems:\n  - 'first'\n---\ntext";
        let buffer = Buffer::from_text(source);
        let decorations = markdown::parse_decorations(source);
        let start = source.find("first").unwrap();
        let end = source.find("text").unwrap() + 4;
        assert!(
            enter_edits(&buffer, &decorations, &[start..start, start + 2..start + 2]).is_none()
        );
        assert!(enter_edits(&buffer, &decorations, &[start..start, end..end]).is_none());
        assert_eq!(buffer.text(), source);
    }

    #[test]
    fn multicursor_plans_apply_atomically_remap_primary_and_ime_and_undo() {
        let source = "---\r\nbody: |-\r\n  日🙂\r\nitems:\r\n  - 'first' # comment\r\n  - second\r\n---\r\ntext";
        let buffer = Buffer::from_text(source);
        let selections = vec![
            source.find("日").unwrap() + "日".len()..source.find("日").unwrap() + "日".len(),
            source.find("' #").unwrap() + 1..source.find("' #").unwrap() + 1,
        ];
        let plans =
            enter_edits(&buffer, &markdown::parse_decorations(source), &selections).unwrap();
        let mut buffer = Buffer::from_text(source);
        let mut cursor = Cursor {
            offset: selections[0].start,
            anchor: selections[0].end,
        };
        let mut extra = vec![Cursor {
            offset: selections[1].start,
            anchor: selections[1].end,
        }];
        let mut marked = Some(source.find("text").unwrap()..source.len());
        let mut extra_marked = vec![];
        let mut affinity = None;
        buffer.begin_edit_recording();
        for CursorEnterEdit { cursor_index, edit } in plans {
            buffer.replace(edit.range.clone(), &edit.replacement);
            super::super::remap_source_edit_positions(
                &mut cursor,
                &mut extra,
                &mut marked,
                &mut extra_marked,
                &mut affinity,
                &edit.range,
                edit.replacement.len(),
            );
            let target = if cursor_index == 0 {
                &mut cursor
            } else {
                &mut extra[cursor_index - 1]
            };
            target.offset = edit.range.start + edit.cursor;
            target.anchor = target.offset;
        }
        let edits = buffer.finish_edit_recording();
        assert_eq!(edits.len(), 2);
        assert_eq!(
            buffer.text(),
            "---\r\nbody: |-\r\n  日\r\n  🙂\r\nitems:\r\n  - 'first' # comment\r\n  - \r\n  - second\r\n---\r\ntext"
        );
        assert_eq!(
            &buffer.text()[cursor.offset..cursor.offset + "🙂".len()],
            "🙂"
        );
        assert!(buffer.text()[..extra[0].offset].ends_with("  - "));
        assert_eq!(buffer.text_for_range(marked.unwrap()), "text");
        assert!(buffer.apply_edits_inverse(&edits));
        assert_eq!(buffer.text(), source);
        assert!(buffer.apply_edits_forward(&edits));
    }
}
