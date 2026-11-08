//! Source-local YAML presentation metadata. Nothing here serializes document source.
use std::ops::Range;

#[derive(Debug, Clone, PartialEq)]
pub struct FrontMatterLineInfo {
    pub key_range: Option<Range<usize>>,
    pub value_range: Range<usize>,
    /// Nesting relative to the aligned value column, not the scalar's source indent.
    pub indent_columns: usize,
    pub display_prefix: String,
    pub value: FrontMatterValue,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FrontMatterValue {
    Raw,
    Text(String),
    Boolean(bool),
    DateTime(String),
    BlockHeader,
}

#[derive(Clone, Copy)]
pub(crate) struct LexicalState {
    quote: Option<u8>,
    flow_depth: usize,
    node_start: bool,
    non_plain_node: bool,
}

impl Default for LexicalState {
    fn default() -> Self {
        Self {
            quote: None,
            flow_depth: 0,
            node_start: true,
            non_plain_node: false,
        }
    }
}

impl LexicalState {
    pub(crate) fn open(&self) -> bool {
        self.quote.is_some() || self.flow_depth > 0
    }

    pub(crate) fn scan(&mut self, source: &str) -> Option<usize> {
        if !self.open() {
            self.node_start = true;
            self.non_plain_node = false;
        }
        let (_, end) = scan(source, 0, self);
        (end < source.len()).then_some(end)
    }
}

pub(crate) fn node_property_end(token: &str) -> usize {
    // Verbatim tags may contain flow punctuation in their URI.
    if token.starts_with("!<") {
        return token.find('>').map_or(token.len(), |end| end + 1);
    }
    token
        .char_indices()
        .find(|(_, ch)| ch.is_whitespace() || matches!(ch, ',' | '[' | ']' | '{' | '}'))
        .map_or(token.len(), |(end, _)| end)
}

pub(crate) fn plain_comment_start(source: &str) -> Option<usize> {
    source.as_bytes().iter().enumerate().find_map(|(i, byte)| {
        (*byte == b'#' && (i == 0 || source.as_bytes()[i - 1].is_ascii_whitespace())).then_some(i)
    })
}

/// Recognize separators only outside quotes/flow collections. Quotes embedded
/// in plain scalars (O'Reilly, for example) are ordinary characters.
fn scan(source: &str, start: usize, state: &mut LexicalState) -> (Option<usize>, usize) {
    scan_nodes(source, start, state, &mut |_, _| {})
}

fn scan_nodes(
    source: &str,
    start: usize,
    state: &mut LexicalState,
    reference: &mut impl FnMut(Range<usize>, bool),
) -> (Option<usize>, usize) {
    let bytes = source.as_bytes();
    let mut colon = None;
    let mut i = start;
    let mut node_start = state.node_start;
    while i < bytes.len() {
        let byte = bytes[i];
        if let Some(quote) = state.quote {
            if quote == b'"' && byte == b'\\' {
                i = (i + 2).min(bytes.len());
                continue;
            }
            if byte == quote {
                if quote == b'\'' && bytes.get(i + 1) == Some(&b'\'') {
                    i += 2;
                    continue;
                }
                state.quote = None;
                state.non_plain_node = true;
            }
        } else {
            match byte {
                b'#' if i == start || bytes[i - 1].is_ascii_whitespace() => {
                    state.node_start = node_start;
                    return (colon, i);
                }
                b'&' | b'!' | b'*' if node_start => {
                    let end = i + node_property_end(&source[i..]);
                    if matches!(byte, b'&' | b'*') && end > i + 1 {
                        reference(i..end, byte == b'&');
                    }
                    node_start = byte != b'*';
                    state.non_plain_node = false;
                    i = end;
                    continue;
                }
                b'\'' | b'"' if node_start => {
                    state.quote = Some(byte);
                    state.non_plain_node = false;
                    node_start = false;
                }
                b'[' | b'{' if node_start => {
                    state.flow_depth += 1;
                    state.non_plain_node = false;
                    node_start = true;
                }
                b']' | b'}' if state.flow_depth > 0 => {
                    state.flow_depth -= 1;
                    state.non_plain_node = true;
                    node_start = false;
                }
                b',' if state.flow_depth > 0 => {
                    node_start = true;
                    state.non_plain_node = false;
                }
                b':' if bytes.get(i + 1).is_none_or(u8::is_ascii_whitespace)
                    || (state.flow_depth > 0
                        && (state.non_plain_node
                            || bytes.get(i + 1).is_some_and(|byte| {
                                matches!(byte, b',' | b'[' | b']' | b'{' | b'}')
                            }))) =>
                {
                    if state.flow_depth == 0 && colon.is_none() {
                        colon = Some(i);
                    }
                    node_start = true;
                    state.non_plain_node = false;
                }
                b'?' if node_start && bytes.get(i + 1).is_some_and(u8::is_ascii_whitespace) => {}
                b' ' | b'\t' | b'\r' => {}
                _ => {
                    node_start = false;
                    state.non_plain_node = false;
                }
            }
        }
        i += 1;
    }
    state.node_start = node_start;
    (colon, source.len())
}

fn source_indent(source: &str) -> usize {
    source.len() - source.trim_start_matches(' ').len()
}

fn trim_range(source: &str, range: Range<usize>) -> Range<usize> {
    let slice = &source[range.clone()];
    let start = range.start + slice.len() - slice.trim_start_matches([' ', '\t']).len();
    let end = range.end - slice.len() + slice.trim_end_matches([' ', '\t', '\r']).len();
    start..end.max(start)
}

/// The same lexical predicate is used to admit incomplete front matter.
pub(crate) fn has_mapping_key(source: &str) -> bool {
    let start = source_indent(source);
    let (colon, _) = scan(source, start, &mut LexicalState::default());
    colon.is_some_and(|colon| real_key(source, start..colon).is_some())
}

fn real_key(source: &str, range: Range<usize>) -> Option<Range<usize>> {
    let range = trim_range(source, range);
    let key = &source[range.clone()];
    if key.is_empty() || key.starts_with(['?', ':', '[', '{', '&', '*', '!']) {
        return None;
    }
    // Quoted keys must themselves be complete scalars. Keep their source quotes.
    if key.starts_with(['\'', '"'])
        && !matches!(
            serde_yaml::from_str::<serde_yaml::Value>(key),
            Ok(serde_yaml::Value::String(_))
        )
    {
        return None;
    }
    Some(range)
}

fn strip_node_properties(mut token: &str) -> &str {
    while token.starts_with(['&', '!']) {
        token = token[node_property_end(token)..].trim_start();
    }
    token
}

fn block_indicator(token: &str) -> Option<Option<usize>> {
    let mut chars = token.chars();
    if !matches!(chars.next(), Some('|' | '>')) {
        return None;
    }
    let mut indent = None;
    let mut chomp = false;
    for ch in chars {
        match ch {
            '+' | '-' if !chomp => chomp = true,
            '1'..='9' if indent.is_none() => indent = ch.to_digit(10).map(|n| n as usize),
            _ => return None,
        }
    }
    Some(indent)
}

fn is_iso_date_time(token: &str) -> bool {
    let bytes = token.as_bytes();
    let date_shape = bytes.len() >= 10
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[..10]
            .iter()
            .enumerate()
            .all(|(i, b)| matches!(i, 4 | 7) || b.is_ascii_digit());
    if !date_shape || chrono::NaiveDate::parse_from_str(&token[..10], "%Y-%m-%d").is_err() {
        return false;
    }
    if token.len() == 10 {
        return true;
    }
    if bytes.len() < 19
        || !matches!(bytes.get(10), Some(b'T' | b't' | b' '))
        || bytes[13] != b':'
        || bytes[16] != b':'
        || ![11, 12, 14, 15, 17, 18]
            .iter()
            .all(|index| bytes[*index].is_ascii_digit())
    {
        return false;
    }
    chrono::DateTime::parse_from_rfc3339(token).is_ok()
        || ["%Y-%m-%dT%H:%M:%S%.f", "%Y-%m-%d %H:%M:%S%.f"]
            .iter()
            .any(|format| chrono::NaiveDateTime::parse_from_str(token, format).is_ok())
}

fn decode_scalar(token: &str, sequence_item: bool) -> FrontMatterValue {
    if token.is_empty() {
        return if sequence_item {
            FrontMatterValue::Text(String::new())
        } else {
            FrontMatterValue::Raw
        };
    }
    if token.starts_with(['&', '*', '!']) {
        return FrontMatterValue::Raw;
    }
    match serde_yaml::from_str::<serde_yaml::Value>(token) {
        Ok(serde_yaml::Value::Bool(value)) => FrontMatterValue::Boolean(value),
        Ok(serde_yaml::Value::String(value))
            if !value.contains(['\n', '\r', '\u{85}', '\u{2028}', '\u{2029}']) =>
        {
            if is_iso_date_time(&value) {
                FrontMatterValue::DateTime(value)
            } else {
                FrontMatterValue::Text(value)
            }
        }
        // Text is source-preserving presentation metadata, not the YAML type.
        // Null sequence items need the same value-column alignment as populated
        // items, including the empty row created by Enter before any input.
        Ok(serde_yaml::Value::Null) if sequence_item => FrontMatterValue::Text(token.to_string()),
        // Preserve numeric spelling (hex, exponents, significant zeros).
        Ok(serde_yaml::Value::Number(_)) => FrontMatterValue::Text(token.to_string()),
        _ => FrontMatterValue::Raw,
    }
}

struct ScalarBody {
    parent_indent: usize,
    content_indent: Option<usize>,
    display_indent: usize,
    literal: bool,
}

/// Lex each physical row independently, carrying only scalar/collection context.
/// Invalid documents retain lexical keys but never hide or decode their values.
pub(crate) fn parse_lines(lines: &[&str]) -> Vec<FrontMatterLineInfo> {
    parse_lines_with_references(lines, &mut |_, _| {})
}

/// Alias identities are source offsets of their most recent anchor definition,
/// not resolved values: equal-valued or shadowed anchors are still distinct.
pub(crate) fn alias_bindings(source: &str) -> Vec<(usize, Option<usize>)> {
    let mut anchors = std::collections::HashMap::new();
    let mut aliases = Vec::new();
    parse_lines_with_references(
        &source.split('\n').collect::<Vec<_>>(),
        &mut |range, anchor| {
            let name = &source[range.start + 1..range.end];
            if anchor {
                anchors.insert(name, range.start);
            } else {
                aliases.push((range.start, anchors.get(name).copied()));
            }
        },
    );
    aliases
}

/// Validity is per top-level entry, so an incomplete sibling leaves unrelated
/// values decoded. Comment and blank rows belong to the entry that surrounds them.
fn valid_entry_rows(lines: &[&str]) -> Vec<bool> {
    if matches!(
        serde_yaml::from_str::<serde_yaml::Value>(&lines.join("\n")),
        Ok(serde_yaml::Value::Mapping(_))
    ) {
        return vec![true; lines.len()];
    }
    let mut valid = vec![false; lines.len()];
    let is_content = |source: &str| {
        let trimmed = source.trim_start();
        !trimmed.is_empty() && !trimmed.starts_with('#')
    };
    let Some(base) = lines
        .iter()
        .copied()
        .find(|source| is_content(source))
        .map(source_indent)
    else {
        return valid;
    };
    let mut start = 0;
    for row in 0..=lines.len() {
        let boundary = row == lines.len() || {
            let source = lines[row];
            let indent = source_indent(source);
            is_content(source)
                && (indent < base || (indent == base && !sequence_row(&source[indent..])))
        };
        if boundary {
            let entry = lines[start..row].join("\n");
            valid[start..row].fill(matches!(
                serde_yaml::from_str::<serde_yaml::Value>(&entry),
                Ok(serde_yaml::Value::Mapping(_) | serde_yaml::Value::Null)
            ));
            start = row;
        }
    }
    valid
}

fn sequence_row(source: &str) -> bool {
    source.starts_with('-') && source.as_bytes().get(1).is_none_or(u8::is_ascii_whitespace)
}

fn parse_lines_with_references(
    lines: &[&str],
    reference: &mut impl FnMut(Range<usize>, bool),
) -> Vec<FrontMatterLineInfo> {
    let valid_rows = valid_entry_rows(lines);
    let mut rows = Vec::with_capacity(lines.len());
    let mut body: Option<ScalarBody> = None;
    let mut containers: Vec<usize> = Vec::new();
    let mut sequences: Vec<(usize, usize)> = Vec::new();
    let mut carried = LexicalState::default();
    let mut next_offset = 0;
    for (line_index, source) in lines.iter().copied().enumerate() {
        let valid = valid_rows[line_index];
        let offset = next_offset;
        next_offset += source.len() + 1;
        let mut row_reference = |range: Range<usize>, anchor| {
            reference(offset + range.start..offset + range.end, anchor);
        };
        let indent = source_indent(source);
        let blank = source.trim().is_empty();
        if carried.quote.is_none()
            && ((blank && body.is_none())
                || (source[indent..].starts_with('#')
                    && body.as_ref().is_none_or(|scalar| !scalar.literal)))
        {
            rows.push(FrontMatterLineInfo {
                key_range: None,
                value_range: 0..source.len(),
                indent_columns: 0,
                display_prefix: String::new(),
                value: FrontMatterValue::Raw,
            });
            continue;
        }
        if let Some(scalar) = body.as_mut() {
            if blank || indent > scalar.parent_indent {
                let required = if blank {
                    scalar.content_indent.unwrap_or(indent)
                } else {
                    *scalar.content_indent.get_or_insert(indent)
                };
                if blank || indent >= required {
                    let start = required.min(indent);
                    let end = if scalar.literal {
                        source.len()
                    } else {
                        let end = plain_comment_start(source).unwrap_or(source.len());
                        source[..end].trim_end_matches([' ', '\t', '\r']).len()
                    };
                    let range = start..end.max(start);
                    rows.push(FrontMatterLineInfo {
                        key_range: None,
                        value_range: range.clone(),
                        indent_columns: scalar.display_indent,
                        display_prefix: String::new(),
                        value: if valid {
                            FrontMatterValue::Text(source[range].to_string())
                        } else {
                            FrontMatterValue::Raw
                        },
                    });
                    continue;
                }
            }
            body = None;
        }
        if carried.quote.is_some() || carried.flow_depth > 0 {
            scan_nodes(source, indent, &mut carried, &mut row_reference);
            rows.push(FrontMatterLineInfo {
                key_range: None,
                value_range: 0..source.len(),
                indent_columns: 0,
                display_prefix: String::new(),
                value: FrontMatterValue::Raw,
            });
            continue;
        }

        containers.retain(|parent| *parent < indent);
        let mut start = indent;
        let mut sequence_markers = Vec::new();
        while source.as_bytes().get(start) == Some(&b'-')
            && source
                .as_bytes()
                .get(start + 1)
                .is_none_or(u8::is_ascii_whitespace)
        {
            sequence_markers.push(start);
            start += 1;
            while source
                .as_bytes()
                .get(start)
                .is_some_and(|b| matches!(b, b' ' | b'\t'))
            {
                start += 1;
            }
        }
        sequences.retain(|(source_indent, _)| *source_indent < indent);
        let mut display_indent = indent;
        for marker in &sequence_markers {
            let container = containers.last().copied().unwrap_or(0);
            display_indent = match sequences.last() {
                Some((parent_source_indent, parent_display_indent))
                    if container < *parent_source_indent =>
                {
                    parent_display_indent + 2
                }
                _ => container,
            };
            sequences.push((*marker, display_indent));
        }
        let sequence_count = sequence_markers.len();
        // A new block row begins a node; only multiline quote/flow rows carry
        // the pending-node state across physical line boundaries.
        carried.node_start = true;
        carried.non_plain_node = false;
        let (colon, end) = scan_nodes(source, start, &mut carried, &mut row_reference);
        let key = colon.and_then(|colon| real_key(source, start..colon));
        let display_indent = if key.is_some() { start } else { display_indent };
        let value_start = if key.is_some() {
            colon.unwrap() + 1
        } else {
            start
        };
        let value_range = trim_range(source, value_start..end);
        let token = &source[value_range.clone()];
        let mut value = if valid {
            decode_scalar(token, key.is_none() && sequence_count > 0)
        } else {
            FrontMatterValue::Raw
        };
        let node_token = strip_node_properties(token);
        if let Some(explicit_indent) = block_indicator(node_token) {
            if valid && node_token == token {
                value = FrontMatterValue::BlockHeader;
            }
            // A sequence item's block indent is relative to the dash, not its
            // value token. Compact sequence mappings use the key's indentation.
            let parent_indent = if key.is_some() {
                start
            } else {
                sequence_markers.last().copied().unwrap_or(indent)
            };
            body = Some(ScalarBody {
                parent_indent,
                content_indent: explicit_indent
                    .map(|extra| parent_indent + extra)
                    .or_else(|| {
                        lines[line_index + 1..]
                            .iter()
                            .find(|line| !line.trim().is_empty())
                            .map(|next| source_indent(next))
                            .filter(|indent| *indent > parent_indent)
                    }),
                display_indent,
                literal: true,
            });
        } else if key.is_some() || sequence_count > 0 {
            let parent_indent = if key.is_some() {
                start
            } else {
                sequence_markers.last().copied().unwrap_or(indent)
            };
            if node_token.is_empty() || node_token.starts_with('#') {
                containers.push(parent_indent);
            } else if carried.quote.is_none()
                && carried.flow_depth == 0
                && !node_token.starts_with(['[', '{', '*'])
            {
                body = Some(ScalarBody {
                    parent_indent,
                    content_indent: None,
                    display_indent,
                    literal: false,
                });
                // A multiline plain scalar is text as a whole, never a boolean
                // or date on just its first physical row.
                if lines[line_index + 1..]
                    .iter()
                    .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
                    .is_some_and(|next| source_indent(next) > parent_indent)
                    && !token.starts_with(['\'', '"', '&', '!'])
                    && valid
                {
                    value = FrontMatterValue::Text(token.to_string());
                }
            }
        }
        // Standalone comments and unsupported rows remain entirely source-visible.
        let unsupported = key.is_none() && sequence_count == 0;
        rows.push(FrontMatterLineInfo {
            key_range: key,
            value_range: if unsupported {
                0..source.len()
            } else {
                value_range
            },
            indent_columns: display_indent,
            display_prefix: if sequence_count > 0 {
                "• ".repeat(sequence_count)
            } else {
                String::new()
            },
            value: if unsupported {
                FrontMatterValue::Raw
            } else {
                value
            },
        });
    }
    rows
}

/// Exact source spans for decoded characters, including YAML quote/escape syntax.
/// A formatted date or another unrelated replacement has no character identity;
/// its caller must anchor it to the token boundaries instead of interpolating bytes.
pub(crate) fn decoded_char_ranges(token: &str, decoded: &str) -> Option<Vec<Range<usize>>> {
    if token == decoded {
        return Some(
            token
                .char_indices()
                .map(|(i, ch)| i..i + ch.len_utf8())
                .collect(),
        );
    }
    let quote = *token.as_bytes().first()?;
    if !matches!(quote, b'\'' | b'"') || token.as_bytes().last() != Some(&quote) {
        return None;
    }
    let mut result = String::new();
    let mut ranges = Vec::new();
    let mut i = 1;
    while i < token.len() - 1 {
        let start = i;
        let ch = token[i..].chars().next()?;
        if quote == b'\'' && ch == '\'' && token.as_bytes().get(i + 1) == Some(&b'\'') {
            i += 2;
            result.push('\'');
            ranges.push(start..i);
        } else if quote == b'"' && ch == '\\' {
            let escape = *token.as_bytes().get(i + 1)?;
            let len = match escape {
                b'x' => 4,
                b'u' => 6,
                b'U' => 10,
                _ => 2,
            };
            i += len;
            let fragment = token.get(start..i)?;
            let decoded_escape = serde_yaml::from_str::<String>(&format!("\"{fragment}\"")).ok()?;
            for ch in decoded_escape.chars() {
                result.push(ch);
                ranges.push(start..i);
            }
        } else {
            i += ch.len_utf8();
            result.push(ch);
            ranges.push(start..i);
        }
    }
    (result == decoded).then_some(ranges)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn front_matter_lexes_typed_values_without_changing_source() {
        let lines = [
            "title: \"true\" # text",
            "enabled: true # switch",
            "empty: null",
            "number: 0x2a",
            "date: 2026-10-12",
            "invalid_date: 2026-02-30",
            "\"键:名\": '你好: ''世界''' # note",
            "tags:",
            "  - \"one: two\"",
            "  - 'true'",
            "nested:",
            "  child: false",
            "description: |-",
            "  1. true",
            "  2026-10-12",
            "  key: value # literal",
            "folded: >-",
            "  false",
            "  more text",
        ];
        let rows = parse_lines(&lines);
        assert_eq!(rows[0].value, FrontMatterValue::Text("true".into()));
        assert_eq!(rows[1].value, FrontMatterValue::Boolean(true));
        assert_eq!(&lines[1][rows[1].value_range.clone()], "true");
        assert_eq!(rows[2].value, FrontMatterValue::Raw);
        assert_eq!(rows[3].value, FrontMatterValue::Text("0x2a".into()));
        assert_eq!(
            rows[4].value,
            FrontMatterValue::DateTime("2026-10-12".into())
        );
        assert_eq!(rows[5].value, FrontMatterValue::Text("2026-02-30".into()));
        assert_eq!(&lines[6][rows[6].key_range.clone().unwrap()], "\"键:名\"");
        assert_eq!(rows[6].value, FrontMatterValue::Text("你好: '世界'".into()));
        assert_eq!(rows[8].key_range, None);
        assert_eq!(rows[8].display_prefix, "• ");
        assert_eq!(rows[8].value, FrontMatterValue::Text("one: two".into()));
        assert_eq!(rows[11].indent_columns, 2);
        assert_eq!(rows[12].value, FrontMatterValue::BlockHeader);
        for index in 13..=15 {
            assert!(rows[index].key_range.is_none());
            assert_eq!(rows[index].indent_columns, 0);
            assert_eq!(
                rows[index].value,
                FrontMatterValue::Text(lines[index][2..].into())
            );
        }
        assert_eq!(rows[16].value, FrontMatterValue::BlockHeader);
        assert_eq!(rows[17].value, FrontMatterValue::Text("false".into()));
    }

    #[test]
    fn front_matter_empty_null_sequence_rows_keep_prefix_ranges_and_value_indent() {
        for ending in ["", "\r"] {
            let lines = [
                "parent:",
                "  项目:",
                "      - '日🙂'",
                "      -",
                "      -   ",
                "      -   # 空🙂",
                "      - null # keep",
                "      - ~",
                "      - NULL",
                "      - --日🙂",
                "  next: null",
            ]
            .map(|line| format!("{line}{ending}"));
            serde_yaml::from_str::<serde_yaml::Value>(&lines.join("\n")).unwrap();
            let rows = parse_lines(&lines.iter().map(String::as_str).collect::<Vec<_>>());
            for (index, token, start) in [
                (3, "", 7),
                (4, "", 10),
                (5, "", 10),
                (6, "null", 8),
                (7, "~", 8),
                (8, "NULL", 8),
                (9, "--日🙂", 8),
            ] {
                let row = &rows[index];
                assert_eq!(row.key_range, None);
                assert_eq!(row.display_prefix, "• ");
                assert_eq!(row.indent_columns, rows[2].indent_columns);
                assert_eq!(row.indent_columns, 2);
                assert_eq!(row.value_range, start..start + token.len());
                assert_eq!(row.value, FrontMatterValue::Text(token.into()));
                assert_eq!(&lines[index][row.value_range.clone()], token);
            }
            // Mapping nulls keep their existing source-only presentation.
            assert_eq!(rows[10].value, FrontMatterValue::Raw);
        }
    }

    #[test]
    fn front_matter_empty_compact_sequences_keep_each_dash_but_invalid_rows_stay_raw() {
        let lines = [
            "items:",
            "  -   -",
            "      -  ~",
            "  - --literal",
            "next: null",
        ];
        serde_yaml::from_str::<serde_yaml::Value>(&lines.join("\n")).unwrap();
        let rows = parse_lines(&lines);
        assert_eq!(rows[1].display_prefix, "• • ");
        assert_eq!(rows[1].value_range, 7..7);
        assert_eq!(rows[1].value, FrontMatterValue::Text(String::new()));
        assert_eq!(rows[2].display_prefix, "• ");
        assert_eq!(rows[2].indent_columns, 2);
        assert_eq!(rows[2].value_range, 9..10);
        assert_eq!(rows[3].display_prefix, "• ");
        assert_eq!(rows[3].value, FrontMatterValue::Text("--literal".into()));
        let tabbed = parse_lines(&["items:", "  -\t  "]);
        assert_eq!(tabbed[1].display_prefix, "• ");
        assert_eq!(tabbed[1].value, FrontMatterValue::Raw);
        for token in ["", "null", "~", "!tag null", "&anchor null", "*anchor"] {
            let lines = [
                "items:".to_owned(),
                format!("  - {token}"),
                "broken: [".into(),
            ];
            let rows = parse_lines(&lines.iter().map(String::as_str).collect::<Vec<_>>());
            assert_eq!(rows[1].display_prefix, "• ");
            // Only the incomplete sibling is degraded; valid entries keep decoding.
            let expected = if token.starts_with(['!', '&', '*']) {
                FrontMatterValue::Raw
            } else {
                FrontMatterValue::Text(token.into())
            };
            assert_eq!(rows[1].value, expected, "{token}");
        }
    }

    #[test]
    fn front_matter_nested_sequences_mappings_and_comments_keep_depth() {
        let lines = [
            "items:",
            "  - - 'first'",
            "    - 'second'",
            "  - name: 'record'",
            "    enabled: true",
            "    tags:",
            "    # comment between container and items",
            "      - 'nested'",
            "config:",
            "  tags:",
            "  # comment",
            "    - 'value'",
            "enabled: true",
            "  # a comment is not a multiline scalar",
        ];
        let rows = parse_lines(&lines);
        assert_eq!(rows[1].display_prefix, "• • ");
        assert_eq!(rows[2].indent_columns, 2);
        assert_eq!(rows[2].value, FrontMatterValue::Text("second".into()));
        assert_eq!(&lines[3][rows[3].key_range.clone().unwrap()], "name");
        assert_eq!(rows[4].value, FrontMatterValue::Boolean(true));
        assert_eq!(rows[7].indent_columns, 4);
        assert_eq!(rows[11].indent_columns, 2);
        assert_eq!(rows[12].value, FrontMatterValue::Boolean(true));
    }

    #[test]
    fn front_matter_explicit_block_indent_and_plain_continuation_comments() {
        let lines = [
            "literal: |2- # header",
            "    ",
            "  true",
            "    extra",
            "  # literal",
            "  ---",
            "folded: >2+",
            "  1. plain",
            "plain: first",
            "  second # comment",
            "last: false",
        ];
        let rows = parse_lines(&lines);
        assert_eq!(rows[0].value, FrontMatterValue::BlockHeader);
        assert_eq!(rows[1].value_range, 2..4);
        assert_eq!(rows[1].value, FrontMatterValue::Text("  ".into()));
        assert_eq!(rows[3].value, FrontMatterValue::Text("  extra".into()));
        assert_eq!(rows[4].value, FrontMatterValue::Text("# literal".into()));
        assert_eq!(rows[7].value, FrontMatterValue::Text("1. plain".into()));
        assert_eq!(&lines[9][rows[9].value_range.clone()], "second");
        assert_eq!(rows[10].value, FrontMatterValue::Boolean(false));
    }

    #[test]
    fn front_matter_multiline_and_unsupported_nodes_are_safe() {
        let rows = parse_lines(&["plain: first", "  second: still scalar", "broken: [one,"]);
        assert!(rows.iter().all(|row| row.value == FrontMatterValue::Raw));
        assert!(rows[1].key_range.is_none());
        let rows = parse_lines(&[
            "flow: [one,",
            "  {key: true}]",
            "quoted: \"first",
            "  next: line\"",
            "ok: true",
        ]);
        assert_eq!(rows[0].value, FrontMatterValue::Raw);
        assert!(rows[1].key_range.is_none());
        assert_eq!(rows[2].value, FrontMatterValue::Raw);
        assert!(rows[3].key_range.is_none());
        assert_eq!(rows[4].value, FrontMatterValue::Boolean(true));
        let rows = parse_lines(&["text: \"line\\nnext\""]);
        assert_eq!(rows[0].value, FrontMatterValue::Raw);
    }

    #[test]
    fn front_matter_unsupported_node_properties_preserve_scalar_context() {
        let lines = [
            "body: &body |-",
            "  key: literal",
            "  true",
            "copy: *body",
            "quote: !!str \"first",
            "  # end\"",
            "flow: &items [one,",
            "  {nested: true}]",
            "enabled: true",
        ];
        let rows = parse_lines(&lines);
        assert_eq!(rows[0].value, FrontMatterValue::Raw);
        assert_eq!(rows[1].key_range, None);
        assert_eq!(rows[1].value, FrontMatterValue::Text("key: literal".into()));
        assert_eq!(rows[2].value, FrontMatterValue::Text("true".into()));
        assert_eq!(rows[3].value, FrontMatterValue::Raw);
        assert_eq!(rows[4].value, FrontMatterValue::Raw);
        assert_eq!(rows[5].key_range, None);
        assert_eq!(rows[7].key_range, None);
        assert_eq!(rows[8].value, FrontMatterValue::Boolean(true));
    }

    #[test]
    fn front_matter_flow_and_multiline_quotes_do_not_consume_later_keys() {
        for lines in [
            vec![
                "flow: {\"x\":\"colon: hash # quoted\",\"array\":[\"a\",\"b\"]}",
                "flag: true",
            ],
            vec!["flow: {\"x\":", "  \"colon: hash # quoted\"}", "flag: true"],
            vec!["quote: \"first", "  # end\"", "flag: true"],
        ] {
            let rows = parse_lines(&lines);
            assert_eq!(
                rows.last().unwrap().value,
                FrontMatterValue::Boolean(true),
                "{lines:?}: {:?}",
                serde_yaml::from_str::<serde_yaml::Value>(&lines.join("\n"))
            );
            assert_eq!(
                &lines.last().unwrap()[rows.last().unwrap().key_range.clone().unwrap()],
                "flag"
            );
        }
        for token in [
            "2026-10-12T1:02:03",
            "2026-10-12T01:2:03",
            "2026-10-12T01:02:3",
            "2026-02-30",
            "2026-10-12T25:00:00Z",
        ] {
            assert!(!is_iso_date_time(token));
        }
        for token in [
            "2026-10-12",
            "2026-10-12T01:02:03",
            "2026-10-12T01:02:03.123+08:00",
            "2026-10-12 01:02:03",
        ] {
            assert!(is_iso_date_time(token));
        }
        for token in [
            "\"line\\nnext\"",
            "\"line\\rnext\"",
            "\"line\\Lnext\"",
            "\"line\\Pnext\"",
            "\"line\\Nnext\"",
        ] {
            assert_eq!(decode_scalar(token, false), FrontMatterValue::Raw);
        }
    }

    #[test]
    fn front_matter_flow_anchor_delimiters_and_plain_colons_keep_sibling_keys() {
        for token in [
            "[&keep]",
            "{key: &keep}",
            "[&keep, text]",
            "[&keep, *keep]",
            "[a:'b]",
            "[a:\"b]",
            "{key: a:'b}",
            "{\"key\":\"quoted: # value\"}",
            "[a:'b,\n two]",
            "!<tag:example.com,2026:[x]> [one, two]",
        ] {
            let metadata = format!("value: {token}\nsibling: true # untouched");
            assert!(serde_yaml::from_str::<serde_yaml::Value>(&metadata).is_ok());
            let lines = metadata.split('\n').collect::<Vec<_>>();
            let rows = parse_lines(&lines);
            let last = rows.last().unwrap();
            assert_eq!(last.key_range, Some(0..7), "{metadata}");
            assert_eq!(last.value, FrontMatterValue::Boolean(true), "{metadata}");
        }
    }

    #[test]
    fn front_matter_nested_scalar_data_does_not_open_syntax() {
        for body in ["|-\n    [", "|-\n    \"", "first\n    [", "first\n    \""] {
            let metadata = format!("value:\n  nested: {body}\nsibling: true");
            let lines = metadata.split('\n').collect::<Vec<_>>();
            let rows = parse_lines(&lines);
            assert!(rows[2].key_range.is_none());
            assert_eq!(rows.last().unwrap().value, FrontMatterValue::Boolean(true));
        }
    }

    #[test]
    fn front_matter_alias_bindings_track_shadowed_definitions_not_scalar_data() {
        let metadata = "original: &child earlier\nvalue:\n  nested: &child later\ncopy: *child\nliteral: |-\n  &child *child\nplain: first\n  &child *child\nflow: [&empty, *empty]\nquote: \"&child *child\"\nlast: *child";
        assert!(serde_yaml::from_str::<serde_yaml::Value>(metadata).is_ok());
        let aliases = alias_bindings(metadata);
        let child = metadata.find("&child later").unwrap();
        let empty = metadata.find("&empty").unwrap();
        assert_eq!(
            aliases,
            vec![
                (metadata.find("copy: *child").unwrap() + 6, Some(child)),
                (metadata.find("*empty").unwrap(), Some(empty)),
                (metadata.find("last: *child").unwrap() + 6, Some(child)),
            ]
        );
    }

    #[test]
    fn front_matter_quotes_comments_and_escape_ranges_are_exact() {
        let rows = parse_lines(&[
            "author: O'Reilly",
            "hash: abc#def",
            "url: https://example.com/a:b",
            "escape: \"中\\u263a\\\"x\" # tail",
        ]);
        assert_eq!(rows[0].value, FrontMatterValue::Text("O'Reilly".into()));
        assert_eq!(rows[1].value, FrontMatterValue::Text("abc#def".into()));
        assert_eq!(
            rows[2].value,
            FrontMatterValue::Text("https://example.com/a:b".into())
        );
        let token = "\"中\\u263a\\\"x\"";
        let ranges = decoded_char_ranges(token, "中☺\"x").unwrap();
        assert_eq!(&token[ranges[0].clone()], "中");
        assert_eq!(&token[ranges[1].clone()], "\\u263a");
        assert_eq!(&token[ranges[2].clone()], "\\\"");
        assert!(!has_mapping_key("https://example.com"));
        assert!(!has_mapping_key("\"not: a key\""));
        assert!(has_mapping_key("\"键:名\": value"));
    }

    #[test]
    fn incomplete_entries_leave_sibling_values_decoded() {
        let lines = [
            "a: true",
            "broken",
            "b: 2024-01-02",
            "c: |",
            "  body",
            "# comment",
            "d: [x]",
        ];
        let rows = parse_lines(&lines);
        assert_eq!(rows[0].value, FrontMatterValue::Boolean(true));
        assert_eq!(rows[1].key_range, None);
        assert_eq!(rows[1].value, FrontMatterValue::Raw);
        assert_eq!(
            rows[2].value,
            FrontMatterValue::DateTime("2024-01-02".into())
        );
        assert_eq!(rows[3].value, FrontMatterValue::BlockHeader);
        assert!(matches!(rows[4].value, FrontMatterValue::Text(_)));
        assert_eq!(rows[6].value, FrontMatterValue::Raw);

        let rows = parse_lines(&["a: true", "b: \"open"]);
        assert_eq!(rows[0].value, FrontMatterValue::Boolean(true));
        assert_eq!(rows[1].value, FrontMatterValue::Raw);
    }

    #[test]
    fn key_typed_at_column_zero_of_blank_body_row_is_a_top_level_key() {
        let rows = parse_lines(&["nested:", "  body: |-", "abc: 1"]);
        assert_eq!(rows[2].key_range, Some(0..3));
        assert_eq!(rows[2].value, FrontMatterValue::Text("1".into()));
    }
}
