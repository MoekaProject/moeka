use std::ops::Range;

use chrono::{DateTime, NaiveDate, NaiveDateTime};
use gpui::Context;

use crate::buffer::Buffer;
use crate::front_matter::{FrontMatterLineInfo, FrontMatterValue, has_mapping_key};
use crate::markdown::LineDecoration;

use super::EditorView;
use super::front_matter_enter;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub(super) enum FrontMatterType {
    Text,
    MultilineText,
    Checkbox,
    Date,
    DateTime,
    List,
    Number,
    Empty,
    Object,
    Raw,
}

pub(super) const TYPES: [FrontMatterType; 9] = [
    FrontMatterType::Text,
    FrontMatterType::MultilineText,
    FrontMatterType::Checkbox,
    FrontMatterType::Date,
    FrontMatterType::DateTime,
    FrontMatterType::List,
    FrontMatterType::Number,
    FrontMatterType::Empty,
    FrontMatterType::Object,
];

struct Property<'a> {
    info: &'a FrontMatterLineInfo,
    token: &'a str,
    indent: usize,
    end_line: usize,
}

fn sequence_row(source: &str) -> bool {
    source.starts_with('-') && source.as_bytes().get(1).is_none_or(u8::is_ascii_whitespace)
}

fn source_indent(source: &str) -> usize {
    source.len() - source.trim_start_matches(' ').len()
}

/// Strip node properties without resolving tags or aliases. Root anchors are
/// retained on resets so an untouched sibling alias still refers to a valid node.
fn node_properties(mut token: &str) -> (Vec<&str>, &str) {
    let mut anchors = Vec::new();
    while token.starts_with(['&', '!']) {
        let end = crate::front_matter::node_property_end(token);
        let property = &token[..end];
        if property.starts_with('&') {
            anchors.push(property);
        }
        token = token[end..].trim_start();
    }
    (anchors, token)
}

fn block_indent(token: &str) -> Option<Option<usize>> {
    let mut chars = token.chars();
    if !matches!(chars.next(), Some('|' | '>')) {
        return None;
    }
    let mut indent = None;
    let mut chomp = false;
    for ch in chars {
        match ch {
            '+' | '-' if !chomp => chomp = true,
            '1'..='9' if indent.is_none() => indent = Some(ch as usize - '0' as usize),
            _ => return None,
        }
    }
    Some(indent)
}

type ValueLexicalState = crate::front_matter::LexicalState;

fn scan_property_row(
    lexical: &mut ValueLexicalState,
    source: &str,
    info: &FrontMatterLineInfo,
) -> Option<usize> {
    if lexical.open() {
        return lexical.scan(source);
    }
    if info.key_range.is_some() || !info.display_prefix.is_empty() {
        lexical.scan(source.get(info.value_range.clone())?);
        return crate::front_matter::plain_comment_start(source.get(info.value_range.end..)?)
            .map(|start| info.value_range.end + start);
    }
    // The parser already owns nested literal/plain scalar continuations.
    // Their brackets, quotes and anchor-shaped text are data, not new nodes.
    crate::front_matter::plain_comment_start(source)
}

/// Only walk this property's descendants. Pending blank/comment rows belong
/// to a collection only when followed by another child, not to its next sibling.
fn property<'a>(
    buffer: &'a Buffer,
    decorations: &'a [LineDecoration],
    line: usize,
) -> Option<Property<'a>> {
    owned_property(buffer, decorations, line, false)
}

fn owned_property<'a>(
    buffer: &'a Buffer,
    decorations: &'a [LineDecoration],
    line: usize,
    valid_document: bool,
) -> Option<Property<'a>> {
    if line >= buffer.line_count() {
        return None;
    }
    let decoration = decorations.get(line)?;
    let info = decoration.front_matter.as_ref()?;
    let key = info.key_range.as_ref()?;
    let source = buffer.line_content(line);
    source.get(key.clone())?;
    let token = source.get(info.value_range.clone())?;
    let indent = key.start;
    let node = node_properties(token).1;
    let literal = block_indent(node);
    let mut content_indent = literal.flatten().map(|extra| indent + extra);
    let mut lexical = ValueLexicalState::default();
    lexical.scan(token);
    let mut end_line = line + 1;
    for row in line + 1..buffer.line_count().min(decorations.len()) {
        if decorations[row].front_matter.is_none() {
            break;
        }
        let child = buffer.line_content(row).trim_end_matches('\r');
        let child_indent = source_indent(child);
        let blank = child.trim().is_empty();
        if literal.is_some() {
            if blank {
                end_line = row + 1;
                continue;
            }
            // Explicit indentation is key-relative; otherwise the first
            // nonblank row sets it. Dedented comments are trivia, not data.
            if child_indent <= indent || child_indent < *content_indent.get_or_insert(child_indent)
            {
                break;
            }
            end_line = row + 1;
            continue;
        }
        if lexical.open() {
            // A dedented mapping-looking row in an unfinished quoted/flow node
            // is ambiguous: never claim it as disposable continuation data.
            if !valid_document
                && child_indent <= indent
                && crate::front_matter::has_mapping_key(child)
            {
                break;
            }
            lexical.scan(child);
            end_line = row + 1;
            continue;
        }
        if blank || child.trim_start().starts_with('#') {
            continue;
        }
        if child_indent > indent
            || (node.is_empty() && child_indent == indent && sequence_row(&child[child_indent..]))
        {
            scan_property_row(&mut lexical, child, decorations[row].front_matter.as_ref()?);
            end_line = row + 1;
        } else {
            break;
        }
    }
    Some(Property {
        info,
        token,
        indent,
        end_line,
    })
}

/// Typing `:` at the end of a bare key row completes a mapping key, so the
/// caller inserts a following space for the value. Body text, sequence rows,
/// blank rows and mid-line colons keep the typed source unchanged.
pub(super) fn key_colon_needs_space(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    offset: usize,
) -> bool {
    let (line, _) = buffer.offset_to_line_col(offset);
    let start = buffer.line_start(line);
    let source = buffer.line_content(line).trim_end_matches('\r');
    if line == 0 || offset != start + source.len() {
        return false;
    }
    let before = &source[..offset - start];
    let trimmed = before.trim_start();
    if trimmed.is_empty()
        || before.contains([':', '#'])
        || trimmed == "-"
        || trimmed.starts_with("- ")
        || trimmed.starts_with("-\t")
        || !has_mapping_key(&format!("{before}:"))
    {
        return false;
    }
    match front_matter_enter::rows(buffer, decorations) {
        Some(rows) => {
            rows.contains(&line)
                && front_matter_enter::block_owner(buffer, decorations, line).is_none()
                && decorations
                    .get(line)
                    .and_then(|decoration| decoration.front_matter.as_ref())
                    .is_some_and(|info| {
                        info.key_range.is_none()
                            && info.display_prefix.is_empty()
                            && matches!(info.value, FrontMatterValue::Raw)
                    })
        }
        None => unkeyed_metadata_line(buffer, line),
    }
}

/// Markdown admits metadata only once some line has a mapping key, so the first
/// key of an empty block has no decorations yet. No keys means no block owners.
fn unkeyed_metadata_line(buffer: &Buffer, line: usize) -> bool {
    if buffer.line_content(0).trim_start_matches('\u{feff}').trim() != "---" {
        return false;
    }
    let Some(closing) = (1..buffer.line_count())
        .find(|&row| matches!(buffer.line_content(row).trim_end(), "---" | "..."))
    else {
        return false;
    };
    line < closing
        && (1..closing).all(|row| !has_mapping_key(buffer.line_content(row).trim_end_matches('\r')))
}

/// Rendering never parses a document (or a subtree). Quoting overrides the
/// display metadata's date/numeric interpretation: quoted YAML is still text.
pub(super) fn property_type(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    line: usize,
) -> Option<FrontMatterType> {
    let property = property(buffer, decorations, line)?;
    let token = property.token;
    let ty = if token.starts_with(['&', '*', '!']) {
        FrontMatterType::Raw
    } else if matches!(property.info.value, FrontMatterValue::BlockHeader) {
        FrontMatterType::MultilineText
    } else if token.is_empty() {
        let child = (line + 1..property.end_line)
            .map(|row| buffer.line_content(row).trim())
            .find(|source| !source.is_empty() && !source.starts_with('#'));
        match child {
            Some(source) if sequence_row(source) => FrontMatterType::List,
            Some(_) => FrontMatterType::Object,
            None => FrontMatterType::Empty,
        }
    } else if property.end_line > line + 1 {
        if matches!(property.info.value, FrontMatterValue::Text(_)) {
            FrontMatterType::MultilineText
        } else {
            FrontMatterType::Raw
        }
    } else if token.starts_with(['\'', '"']) {
        // A complete quoted scalar may contain escaped newlines and thus have
        // Raw display metadata; token decoding is deferred until an edit.
        if token.len() >= 2 && token.ends_with(token.chars().next()?) {
            FrontMatterType::Text
        } else {
            FrontMatterType::Raw
        }
    } else if matches!(token, "null" | "Null" | "NULL" | "~") {
        FrontMatterType::Empty
    } else if token.starts_with('[') && token.ends_with(']') {
        FrontMatterType::List
    } else if token.starts_with('{') && token.ends_with('}') {
        FrontMatterType::Object
    } else {
        match &property.info.value {
            FrontMatterValue::Boolean(_) => FrontMatterType::Checkbox,
            FrontMatterValue::DateTime(_) if token.len() == 10 => FrontMatterType::Date,
            FrontMatterValue::DateTime(_) => FrontMatterType::DateTime,
            FrontMatterValue::Text(_) if format_yaml_date_time(token, "").is_some() => {
                if token.len() == 10 {
                    FrontMatterType::Date
                } else {
                    FrontMatterType::DateTime
                }
            }
            FrontMatterValue::Text(_) if yaml_number(token).is_some() => FrontMatterType::Number,
            FrontMatterValue::Text(_) => FrontMatterType::Text,
            _ => FrontMatterType::Raw,
        }
    };
    Some(ty)
}

fn yaml_number(source: &str) -> Option<String> {
    // Token-local parsing retains YAML's supported hex/octal/exponent forms
    // rather than treating an arbitrary Rust float as a valid YAML number.
    if !source
        .as_bytes()
        .first()
        .is_some_and(|byte| matches!(byte, b'+' | b'-' | b'.' | b'0'..=b'9'))
        || source.contains(char::is_whitespace)
    {
        return None;
    }
    match serde_yaml::from_str::<serde_yaml::Value>(source).ok()? {
        serde_yaml::Value::Number(_) => Some(source.to_owned()),
        _ => None,
    }
}

/// Detect node properties that serde_yaml would otherwise resolve/drop when
/// decoding. Comments are retained separately; literal scalar bodies are data.
fn safe_yaml_fragment(source: &str) -> bool {
    let bytes = source.as_bytes();
    let mut quote = None;
    let mut node_start = true;
    let mut i = 0;
    while i < bytes.len() {
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
                b'#' if i == 0 || bytes[i - 1].is_ascii_whitespace() => {
                    while i < bytes.len() && bytes[i] != b'\n' {
                        i += 1;
                    }
                    node_start = true;
                    continue;
                }
                b'&' | b'*' | b'!' | b'?' if node_start => return false,
                b'\'' | b'"' if node_start => {
                    quote = Some(byte);
                    node_start = false;
                }
                b'[' | b'{' | b',' | b'\n' => node_start = true,
                b':' if bytes.get(i + 1).is_none_or(u8::is_ascii_whitespace) => {
                    node_start = true;
                }
                b'-' if node_start && bytes.get(i + 1).is_some_and(u8::is_ascii_whitespace) => {}
                b' ' | b'\t' | b'\r' => {}
                _ => node_start = false,
            }
        }
        i += 1;
    }
    quote.is_none()
}

fn property_value(
    buffer: &Buffer,
    line: usize,
    property: &Property<'_>,
) -> Option<serde_yaml::Value> {
    if !safe_yaml_fragment(property.token) {
        return None;
    }
    let literal = matches!(property.info.value, FrontMatterValue::BlockHeader);
    let mut fragment = format!("value: {}\n", property.token);
    for row in line + 1..property.end_line {
        let source = buffer.line_content(row).trim_end_matches('\r');
        // Check before deindenting: a low-indented standalone comment must
        // never have its '#' sliced away and become apparent value content.
        if !literal && !safe_yaml_fragment(source) {
            return None;
        }
        let child = if (!literal && source.trim_start().starts_with('#'))
            || (source.trim().is_empty() && (!literal || source.len() < property.indent))
        {
            ""
        } else {
            &source[property.indent.min(source_indent(source))..]
        };
        fragment.push_str(child);
        fragment.push('\n');
    }
    let serde_yaml::Value::Mapping(mut mapping) = serde_yaml::from_str(&fragment).ok()? else {
        return None;
    };
    if mapping.len() != 1 {
        return None;
    }
    mapping.remove(&serde_yaml::Value::String("value".into()))
}

fn scalar_text(value: &serde_yaml::Value) -> Option<String> {
    match value {
        serde_yaml::Value::Null => Some(String::new()),
        serde_yaml::Value::Bool(value) => Some(value.to_string()),
        serde_yaml::Value::Number(value) => Some(value.to_string()),
        serde_yaml::Value::String(value) => Some(value.clone()),
        _ => None,
    }
}

fn quoted_text(value: &str) -> Option<String> {
    // JSON's quoted scalars are YAML-safe once YAML-specific line separators
    // are escaped too (serde_json normally leaves these literal).
    Some(
        serde_json::to_string(value)
            .ok()?
            .replace('\u{85}', "\\u0085")
            .replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029"),
    )
}

fn iso_value(source: &str, target: FrontMatterType) -> Option<String> {
    if source.len() == 10 {
        let date = NaiveDate::parse_from_str(source, "%Y-%m-%d").ok()?;
        if date.format("%Y-%m-%d").to_string() != source {
            return None;
        }
        return Some(if target == FrontMatterType::Date {
            source.to_owned()
        } else {
            format!("{source}T00:00:00")
        });
    }
    if let Ok(timestamp) = DateTime::parse_from_rfc3339(source) {
        if target == FrontMatterType::Date {
            // Dropping a meaningful time or offset is not a safe conversion.
            return None;
        }
        return Some(timestamp.to_rfc3339());
    }
    let timestamp = NaiveDateTime::parse_from_str(source, "%Y-%m-%dT%H:%M:%S%.f")
        .or_else(|_| NaiveDateTime::parse_from_str(source, "%Y-%m-%d %H:%M:%S%.f"))
        .ok()?;
    if target == FrontMatterType::Date {
        if timestamp.time() != chrono::NaiveTime::from_hms_opt(0, 0, 0)? {
            return None;
        }
        Some(timestamp.date().format("%Y-%m-%d").to_string())
    } else {
        Some(timestamp.format("%Y-%m-%dT%H:%M:%S%.f").to_string())
    }
}

/// Prefer semantic conversion; the caller supplies base syntax on failure.
fn converted_value(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    line: usize,
    property: &Property<'_>,
    current: FrontMatterType,
    target: FrontMatterType,
    block_extra: usize,
) -> Option<(String, Option<String>)> {
    let value = property_value(buffer, line, property)?;
    let empty = match &value {
        serde_yaml::Value::Null => true,
        serde_yaml::Value::String(value) => value.is_empty(),
        serde_yaml::Value::Sequence(value) => value.is_empty(),
        serde_yaml::Value::Mapping(value) => value.is_empty(),
        _ => false,
    };
    // Complex collections cannot be flattened losslessly; use the reset path.
    let text = match &value {
        serde_yaml::Value::Sequence(items) => items
            .iter()
            .map(scalar_text)
            .collect::<Option<Vec<_>>>()?
            .join("\n"),
        serde_yaml::Value::Mapping(mapping) if mapping.is_empty() => String::new(),
        _ => scalar_text(&value)?,
    };
    // Retain significant numeric source spelling when converting to text/list.
    let text = if current == FrontMatterType::Number {
        property.token.to_owned()
    } else {
        text
    };
    let eol = if buffer.line_content(line).ends_with('\r') {
        "\r\n"
    } else {
        "\n"
    };
    let indent = " ".repeat(property.indent + block_extra);
    let (token, body) = match target {
        FrontMatterType::Text => (quoted_text(&text)?, None),
        FrontMatterType::MultilineText => {
            if text.chars().any(|ch| {
                (ch.is_control() && !matches!(ch, '\n' | '\t'))
                    || matches!(ch, '\u{85}' | '\u{2028}' | '\u{2029}')
            }) {
                return None;
            }
            let trailing = text.bytes().rev().take_while(|byte| *byte == b'\n').count();
            // YAML clip chomping drops an entirely blank scalar, even its one
            // newline. Keep chomping is required for newline-only text too.
            let keep = trailing > 1 || (trailing > 0 && text.trim_end_matches('\n').is_empty());
            // A new literal must not absorb a previously standalone comment,
            // or (with keep chomping) unrelated separator blank lines.
            for row in property.end_line..buffer.line_count().min(decorations.len()) {
                if decorations[row].front_matter.is_none() {
                    break;
                }
                let source = buffer.line_content(row).trim_end_matches('\r');
                if source.trim().is_empty() {
                    if keep {
                        return None;
                    }
                    continue;
                }
                if source.trim_start().starts_with('#')
                    && source_indent(source) >= property.indent + block_extra
                {
                    return None;
                }
                break;
            }
            let chomp = if keep {
                "+"
            } else if trailing == 0 {
                "-"
            } else {
                ""
            };
            let explicit = block_extra != 2
                || text
                    .split('\n')
                    .find(|row| !row.is_empty())
                    .is_some_and(|row| row.starts_with([' ', '\t']));
            let token = if explicit {
                format!("|{block_extra}{chomp}")
            } else {
                format!("|{chomp}")
            };
            let content = text.strip_suffix('\n').unwrap_or(&text);
            let body = content
                .split('\n')
                .map(|row| format!("{indent}{row}"))
                .collect::<Vec<_>>()
                .join(eol);
            (token, Some(body))
        }
        FrontMatterType::Checkbox => {
            let token = if empty {
                "false"
            } else {
                match text.as_str() {
                    "true" | "True" | "TRUE" => "true",
                    "false" | "False" | "FALSE" => "false",
                    _ => return None,
                }
            };
            if matches!(value, serde_yaml::Value::Sequence(_)) && !empty {
                return None;
            }
            (token.to_owned(), None)
        }
        FrontMatterType::Number => {
            if matches!(value, serde_yaml::Value::Sequence(_)) && !empty {
                return None;
            }
            (
                if empty {
                    "0".into()
                } else {
                    yaml_number(&text)?
                },
                None,
            )
        }
        FrontMatterType::Date | FrontMatterType::DateTime => {
            if matches!(value, serde_yaml::Value::Sequence(_)) && !empty {
                return None;
            }
            let source = if empty {
                chrono::Local::now()
                    .date_naive()
                    .format("%Y-%m-%d")
                    .to_string()
            } else {
                text
            };
            (iso_value(&source, target)?, None)
        }
        FrontMatterType::List => {
            if empty {
                ("[]".into(), None)
            } else if matches!(
                value,
                serde_yaml::Value::Bool(_) | serde_yaml::Value::Number(_)
            ) {
                (String::new(), Some(format!("{indent}- {}", property.token)))
            } else {
                // Text lines become string items, not implicitly typed YAML.
                // split (not lines) keeps a final empty item when text ends in LF.
                let body = text
                    .split('\n')
                    .map(|item| Some(format!("{indent}- {}", quoted_text(item)?)))
                    .collect::<Option<Vec<_>>>()?
                    .join(eol);
                (String::new(), Some(body))
            }
        }
        FrontMatterType::Empty => ("null".into(), None),
        FrontMatterType::Object => {
            if !empty && !matches!(&value, serde_yaml::Value::String(value) if value == "{}") {
                return None;
            }
            ("{}".into(), None)
        }
        FrontMatterType::Raw => return None,
    };
    Some((token, body))
}

fn base_value(target: FrontMatterType, indent: usize) -> (String, Option<String>) {
    let token = match target {
        FrontMatterType::Text => "\"\"".into(),
        FrontMatterType::MultilineText => "|-".into(),
        FrontMatterType::Checkbox => "false".into(),
        FrontMatterType::Number => "0".into(),
        FrontMatterType::Empty => "null".into(),
        FrontMatterType::List => "[]".into(),
        FrontMatterType::Object => "{}".into(),
        FrontMatterType::Date | FrontMatterType::DateTime => {
            let date = chrono::Local::now()
                .date_naive()
                .format("%Y-%m-%d")
                .to_string();
            if target == FrontMatterType::Date {
                date
            } else {
                format!("{date}T00:00:00")
            }
        }
        FrontMatterType::Raw => unreachable!(),
    };
    (
        token,
        (target == FrontMatterType::MultilineText).then(|| " ".repeat(indent)),
    )
}

/// Retain independent comments, not comment-shaped literal/quoted scalar data.
fn subtree_comments(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    line: usize,
    property: &Property<'_>,
) -> Vec<String> {
    if block_indent(node_properties(property.token).1).is_some() {
        return Vec::new();
    }
    let mut lexical = ValueLexicalState::default();
    lexical.scan(property.token);
    let mut comments = Vec::new();
    let mut literal_body: Option<(usize, Option<usize>)> = None;
    for row in line + 1..property.end_line {
        let source = buffer.line_content(row).trim_end_matches('\r');
        if let Some((parent, content)) = literal_body.as_mut() {
            if source.trim().is_empty() {
                continue;
            }
            let indent = source_indent(source);
            if indent > *parent && indent >= *content.get_or_insert(indent) {
                continue;
            }
            literal_body = None;
        }
        let continued = lexical.open();
        if let Some(start) = scan_property_row(
            &mut lexical,
            source,
            decorations[row].front_matter.as_ref().unwrap(),
        ) {
            if source[..start].trim().is_empty() {
                comments.push(source.to_owned());
            } else {
                comments.push(format!(
                    "{}{}",
                    &source[..source_indent(source)],
                    &source[start..]
                ));
            }
        } else if source.trim().is_empty() && !lexical.open() {
            comments.push(source.to_owned());
        }
        if !continued
            && let Some(info) = decorations[row].front_matter.as_ref()
            && let Some(token) = source.get(info.value_range.clone())
            && let Some(extra) = block_indent(node_properties(token).1)
        {
            let parent = info.key_range.as_ref().map_or_else(
                || {
                    source[..info.value_range.start]
                        .rfind('-')
                        .unwrap_or_else(|| source_indent(source))
                },
                |key| key.start,
            );
            literal_body = Some((parent, extra.map(|extra| parent + extra)));
        }
    }
    comments
}

fn external_alias_bindings_preserved(
    before: &str,
    after: &str,
    edit: Range<usize>,
    retained_anchors: &[(usize, usize)],
) -> bool {
    let replacement_len = after.len() - (before.len() - edit.len());
    let remap = |offset| {
        if offset < edit.start {
            Some(offset)
        } else if offset >= edit.end {
            Some(offset - edit.len() + replacement_len)
        } else {
            retained_anchors
                .iter()
                .find_map(|(old, new)| (*old == offset).then_some(*new))
        }
    };
    let new_bindings = crate::front_matter::alias_bindings(after)
        .into_iter()
        .collect::<std::collections::HashMap<_, _>>();
    crate::front_matter::alias_bindings(before)
        .into_iter()
        .filter(|(alias, _)| !edit.contains(alias))
        .all(|(alias, binding)| {
            let Some(new_alias) = remap(alias) else {
                return false;
            };
            let expected = match binding {
                Some(anchor) => {
                    let Some(new_anchor) = remap(anchor) else {
                        return false;
                    };
                    Some(new_anchor)
                }
                None => None,
            };
            new_bindings.get(&new_alias) == Some(&expected)
        })
}

/// Return one source replacement, never a document serialization. A chosen
/// type authorizes clearing incompatible data, but not deleting ambiguous rows.
/// Root anchors survive resets; document parsing protects external alias users.
pub(super) fn type_change_edit(
    buffer: &Buffer,
    decorations: &[LineDecoration],
    line: usize,
    target: FrontMatterType,
) -> Option<(Range<usize>, String)> {
    let current = property_type(buffer, decorations, line)?;
    if target == FrontMatterType::Raw || current == target {
        return None;
    }
    // A valid document proves that even dedented mapping-looking rows inside
    // a quoted/flow node belong to that node. Invalid documents stay conservative.
    let first = (0..=line)
        .rev()
        .take_while(|row| decorations[*row].front_matter.is_some())
        .last()?;
    let last = (line..buffer.line_count().min(decorations.len()))
        .take_while(|row| decorations[*row].front_matter.is_some())
        .last()?;
    let metadata_start = buffer.line_start(first);
    let metadata_end = buffer.line_start(last) + buffer.line_content(last).len();
    let valid_document = matches!(
        serde_yaml::from_str::<serde_yaml::Value>(
            buffer.text_for_range(metadata_start..metadata_end)
        ),
        Ok(serde_yaml::Value::Mapping(_))
    );
    let property = owned_property(buffer, decorations, line, valid_document)?;
    let following = (property.end_line..buffer.line_count().min(decorations.len()))
        .take_while(|row| decorations[*row].front_matter.is_some());
    if block_indent(node_properties(property.token).1).is_some()
        && following
            .clone()
            .map(|row| buffer.line_content(row))
            .find(|source| !source.trim().is_empty() && !source.trim_start().starts_with('#'))
            .is_some_and(|source| source_indent(source) > property.indent)
    {
        // A malformed, underindented block body could become a continuation
        // of the new scalar. Parsing alone would accept that as plain text.
        return None;
    }
    let mut comments = subtree_comments(buffer, decorations, line, &property);
    let trailing_trivia = following
        .take_while(|row| {
            let source = buffer.line_content(*row);
            source.trim().is_empty() || source.trim_start().starts_with('#')
        })
        .collect::<Vec<_>>();
    // An explicit block indent prevents retained comments from becoming text.
    let mut block_extra = 2;
    let mut relocate_comments = false;
    let mut edit_end_line = property.end_line;
    if target == FrontMatterType::MultilineText {
        for source in comments.iter().map(String::as_str).chain(
            trailing_trivia
                .iter()
                .map(|row| buffer.line_content(*row).trim_end_matches('\r')),
        ) {
            if !source.trim().is_empty() {
                block_extra =
                    block_extra.max(source_indent(source).saturating_sub(property.indent) + 1);
            }
        }
        if block_extra > 9 {
            // YAML indentation indicators have only one digit. Move this
            // known trivia before the header rather than absorb or discard it.
            relocate_comments = true;
            block_extra = 2;
            if let Some(last) = trailing_trivia.last() {
                edit_end_line = last + 1;
            }
            comments.extend(
                trailing_trivia
                    .iter()
                    .map(|row| buffer.line_content(*row).trim_end_matches('\r').to_owned()),
            );
        }
    }
    let conversion_decorations = if relocate_comments {
        &decorations[..property.end_line]
    } else {
        decorations
    };
    let (mut token, body) = converted_value(
        buffer,
        conversion_decorations,
        line,
        &property,
        current,
        target,
        block_extra,
    )
    .unwrap_or_else(|| {
        let (mut token, body) = base_value(target, property.indent + block_extra);
        if target == FrontMatterType::MultilineText && block_extra != 2 {
            token = format!("|{block_extra}-");
        }
        (token, body)
    });
    let anchors = node_properties(property.token).0;
    if !anchors.is_empty() {
        token = format!("{} {token}", anchors.join(" "));
    }
    let eol = if buffer.line_content(line).ends_with('\r') {
        "\r\n"
    } else {
        "\n"
    };
    let header = buffer.line_content(line).trim_end_matches('\r');
    let token_range = &property.info.value_range;
    let mut replacement = String::new();
    if relocate_comments {
        for comment in &comments {
            replacement.push_str(comment);
            replacement.push_str(eol);
        }
        replacement.push_str(header.get(..token_range.start)?);
    }
    if token_range.is_empty()
        && header.as_bytes().get(token_range.start.wrapping_sub(1)) == Some(&b':')
        && !token.is_empty()
    {
        replacement.push(' ');
    }
    let replacement_token_start = replacement.len();
    replacement.push_str(&token);
    let suffix = header.get(token_range.end..)?;
    if suffix.starts_with('#') && !token.is_empty() {
        replacement.push(' ');
    }
    let start = buffer.line_start(line)
        + if relocate_comments {
            0
        } else {
            token_range.start
        };
    let has_old_body = edit_end_line > line + 1;
    let end = if has_old_body || body.is_some() {
        replacement.push_str(suffix);
        if let Some(body) = body {
            replacement.push_str(eol);
            replacement.push_str(&body);
        }
        if !relocate_comments {
            for comment in &comments {
                replacement.push_str(eol);
                replacement.push_str(comment);
            }
        }
        let last = edit_end_line - 1;
        buffer.line_start(last) + buffer.line_content(last).trim_end_matches('\r').len()
    } else {
        buffer.line_start(line) + token_range.end
    };
    let range = start..end;
    if buffer.text_for_range(range.clone()) == replacement {
        return None;
    }
    // Parse only at edit time. An unfinished node must not hide later sibling
    // rows, and removing a descendant anchor must not leave dangling aliases.
    let mut metadata = buffer.text_for_range(metadata_start..start).to_owned();
    metadata.push_str(&replacement);
    metadata.push_str(buffer.text_for_range(end..metadata_end));
    if !matches!(
        serde_yaml::from_str::<serde_yaml::Value>(&metadata),
        Ok(serde_yaml::Value::Mapping(_))
    ) {
        return None;
    }
    let mut retained_anchors = Vec::new();
    let mut old_search = 0;
    let mut new_relative = replacement_token_start;
    for anchor in anchors {
        let old_relative = old_search + property.token[old_search..].find(anchor)?;
        retained_anchors.push((
            buffer.line_start(line) + token_range.start + old_relative - metadata_start,
            start + new_relative - metadata_start,
        ));
        old_search = old_relative + anchor.len();
        new_relative += anchor.len() + 1;
    }
    if !external_alias_bindings_preserved(
        buffer.text_for_range(metadata_start..metadata_end),
        &metadata,
        start - metadata_start..end - metadata_start,
        &retained_anchors,
    ) {
        return None;
    }
    Some((range, replacement))
}

impl EditorView {
    pub(super) fn change_front_matter_type(
        &mut self,
        line: usize,
        target: FrontMatterType,
        cx: &mut Context<Self>,
    ) {
        if self.active_tab_is_read_only() {
            return;
        }
        let Some((range, replacement)) =
            type_change_edit(&self.buffer, &self.decorations, line, target)
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
        // A clamp into a replaced UTF-8 token must still land on a character.
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

pub(super) fn wrap_indent(prefix_width: f32, available_width: Option<f32>) -> f32 {
    // Match the minimum 30% value width used during shaping: very long keys
    // must not push wrapped values or their controls outside a narrow viewport.
    available_width
        .map(|width| prefix_width.min(width.max(0.0) * 0.7))
        .unwrap_or(prefix_width)
}

pub(super) fn apply_date_time_format(
    decoration: &mut crate::markdown::LineDecoration,
    format: &str,
) {
    let Some(info) = decoration.front_matter.as_mut() else {
        return;
    };
    if let crate::front_matter::FrontMatterValue::DateTime(source) = &info.value
        && let Some(formatted) = format_yaml_date_time(source, format)
    {
        info.value = crate::front_matter::FrontMatterValue::Text(formatted);
    }
}

/// Project a combined settings format onto its date component. Validate the
/// entire format first, including directives in the discarded time component.
fn date_format_items(format: &str) -> Option<Vec<chrono::format::Item<'_>>> {
    use chrono::format::{Fixed, Item, Numeric, StrftimeItems};
    let items = StrftimeItems::new(format).collect::<Vec<_>>();
    if items.iter().any(|item| matches!(item, Item::Error)) {
        return None;
    }
    let is_date = |item: &Item<'_>| {
        matches!(
            item,
            Item::Numeric(
                Numeric::Year
                    | Numeric::YearDiv100
                    | Numeric::YearMod100
                    | Numeric::IsoYear
                    | Numeric::IsoYearDiv100
                    | Numeric::IsoYearMod100
                    | Numeric::Month
                    | Numeric::Day
                    | Numeric::WeekFromSun
                    | Numeric::WeekFromMon
                    | Numeric::IsoWeek
                    | Numeric::NumDaysFromSun
                    | Numeric::WeekdayFromMon
                    | Numeric::Ordinal,
                _
            ) | Item::Fixed(
                Fixed::ShortMonthName
                    | Fixed::LongMonthName
                    | Fixed::ShortWeekdayName
                    | Fixed::LongWeekdayName
            )
        )
    };
    let time_start = items
        .iter()
        .position(|item| matches!(item, Item::Numeric(..) | Item::Fixed(..)) && !is_date(item));
    let Some(time_start) = time_start else {
        return Some(items);
    };
    let date_end = items[..time_start].iter().rposition(is_date)? + 1;
    let mut date_items = items[..date_end].to_vec();
    // Keep date units and closing punctuation, but not a bracket opening the
    // discarded time component. Include Space items for localized units like г.
    let suffix = items[date_end..time_start]
        .iter()
        .filter_map(|item| match item {
            Item::Literal(text) | Item::Space(text) => Some(*text),
            _ => None,
        })
        .collect::<String>();
    let mut brackets = Vec::new();
    for (offset, ch) in suffix.char_indices() {
        let closing = match ch {
            '(' => Some(')'),
            '[' => Some(']'),
            '{' => Some('}'),
            '（' => Some('）'),
            '【' => Some('】'),
            '「' => Some('」'),
            '〈' => Some('〉'),
            '《' => Some('》'),
            _ => None,
        };
        if let Some(closing) = closing {
            brackets.push((offset, closing));
        } else if brackets.last().is_some_and(|(_, closing)| *closing == ch) {
            brackets.pop();
        }
    }
    let suffix = suffix[..brackets.first().map_or(suffix.len(), |(offset, _)| *offset)]
        .trim_end_matches(|ch: char| {
            ch.is_whitespace() || matches!(ch, ',' | ';' | ':' | '/' | '-' | '–' | '—')
        });
    let suffix = if suffix == "T" { "" } else { suffix };
    if !suffix.is_empty() {
        date_items.push(Item::OwnedLiteral(suffix.into()));
    }
    Some(date_items)
}

/// Formats an unquoted YAML date/timestamp without supplying missing fields.
/// Date-only values use the format's date component; real midnight timestamps
/// still use the complete format. Timezone-dependent timestamps need an offset.
pub(super) fn format_yaml_date_time(source: &str, format: &str) -> Option<String> {
    let bytes = source.as_bytes();
    if bytes.len() < 10
        || bytes[..10].iter().enumerate().any(|(index, byte)| {
            if index == 4 || index == 7 {
                *byte != b'-'
            } else {
                !byte.is_ascii_digit()
            }
        })
    {
        return None;
    }

    let mut formatted = String::new();
    if let Ok(timestamp) = DateTime::parse_from_rfc3339(source) {
        // Keep the parsed fixed offset, not the machine's local timezone or UTC.
        timestamp.format(format).write_to(&mut formatted).ok()?;
    } else if source.len() == 10 {
        let date = NaiveDate::parse_from_str(source, "%Y-%m-%d").ok()?;
        let items = date_format_items(format)?;
        date.format_with_items(items.iter())
            .write_to(&mut formatted)
            .ok()?;
    } else {
        let timestamp = NaiveDateTime::parse_from_str(source, "%Y-%m-%dT%H:%M:%S%.f")
            .or_else(|_| NaiveDateTime::parse_from_str(source, "%Y-%m-%d %H:%M:%S%.f"))
            .ok()?;
        // Unlike to_string(), write_to reports invalid/unsupported formats.
        timestamp.format(format).write_to(&mut formatted).ok()?;
    }
    Some(formatted)
}

/// Toggles a scalar token while retaining its supported YAML boolean casing.
/// The caller replaces only the token's source range, preserving YAML trivia.
pub(super) fn toggled_yaml_boolean(source: &str) -> Option<&'static str> {
    match source {
        "true" => Some("false"),
        "True" => Some("False"),
        "TRUE" => Some("FALSE"),
        "false" => Some("true"),
        "False" => Some("True"),
        "FALSE" => Some("TRUE"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        FrontMatterType as Type, TYPES, apply_date_time_format, format_yaml_date_time,
        key_colon_needs_space, property_type, toggled_yaml_boolean, type_change_edit, wrap_indent,
    };
    use crate::buffer::Buffer;
    use crate::markdown::parse_decorations;

    fn colon_space_at_line_end(source: &str, line: usize) -> bool {
        let buffer = Buffer::from_text(source);
        let offset = buffer.line_end(line);
        key_colon_needs_space(&buffer, &parse_decorations(source), offset)
    }

    #[test]
    fn typed_colon_completes_bare_key_rows_at_line_end() {
        // The first key of an empty block is not admitted as front matter yet.
        assert!(colon_space_at_line_end("---\ntitle\n---", 1));
        assert!(colon_space_at_line_end("---\na: 1\ntitle\n---", 2));
        assert!(colon_space_at_line_end("---\nparent:\n  child\n---", 2));
        assert!(colon_space_at_line_end(
            "---\nkey: |-\n  text\nnext\n---",
            3
        ));

        assert!(!colon_space_at_line_end("---\nkey: |-\n  text\n---", 2));
        assert!(!colon_space_at_line_end("---\nitems:\n  - first\n---", 2));
        assert!(!colon_space_at_line_end("---\n- first\n---", 1));
        assert!(!colon_space_at_line_end("---\n\n---", 1));
        assert!(!colon_space_at_line_end("---\na: 1\ntitle: a\n---", 2));
        assert!(!colon_space_at_line_end("---\ntitle\n---\nbody", 3));
        assert!(!colon_space_at_line_end("---\n# note\n---", 1));

        let source = "---\ntitle\n---";
        let buffer = Buffer::from_text(source);
        let decorations = parse_decorations(source);
        assert!(!key_colon_needs_space(
            &buffer,
            &decorations,
            buffer.line_start(1) + 2
        ));
    }

    fn converted(source: &str, line: usize, target: Type) -> String {
        let mut buffer = Buffer::from_text(source);
        let decorations = parse_decorations(source);
        let (range, replacement) = type_change_edit(&buffer, &decorations, line, target)
            .unwrap_or_else(|| panic!("disabled {target:?}: {source}"));
        // Source plans are pure and every replacement boundary is UTF-8 safe.
        assert_eq!(buffer.text(), source);
        assert!(source.is_char_boundary(range.start));
        assert!(source.is_char_boundary(range.end));
        buffer.replace(range, &replacement);
        buffer.text().to_owned()
    }

    fn value(source: &str) -> serde_yaml::Value {
        let metadata = source
            .lines()
            .skip(1)
            .take_while(|line| !matches!(*line, "---" | "..." | "-"))
            .collect::<Vec<_>>()
            .join("\n");
        serde_yaml::from_str(&metadata).unwrap_or_else(|error| panic!("{error}: {source}"))
    }

    fn field(source: &str) -> serde_yaml::Value {
        value(source)["value"].clone()
    }

    fn disabled(source: &str, line: usize, target: Type) {
        let buffer = Buffer::from_text(source);
        assert!(
            type_change_edit(&buffer, &parse_decorations(source), line, target).is_none(),
            "unexpected {target:?}: {source}"
        );
        assert_eq!(buffer.text(), source);
    }

    #[test]
    fn front_matter_type_detection_uses_yaml_source_not_formatted_display() {
        for (token, expected) in [
            ("plain", Type::Text),
            ("'true'", Type::Text),
            ("\"1997-04-04\"", Type::Text),
            ("'0x10'", Type::Text),
            ("\"a\\nb\"", Type::Text),
            ("TRUE", Type::Checkbox),
            ("1997-04-04", Type::Date),
            ("1997-04-04T12:13:00+08:00", Type::DateTime),
            ("0x10", Type::Number),
            ("1.25e+3", Type::Number),
            (".nan", Type::Number),
            ("null", Type::Empty),
            ("~", Type::Empty),
            ("", Type::Empty),
            ("[]", Type::List),
            ("[a, b]", Type::List),
            ("{}", Type::Object),
            ("{a: b}", Type::Object),
            ("|-\n  content", Type::MultilineText),
            (">\n  folded\n  content", Type::MultilineText),
            ("first\n  second", Type::MultilineText),
            ("\n  - a", Type::List),
            ("\n- a", Type::List),
            ("\n  nested: a", Type::Object),
            ("!custom value", Type::Raw),
            ("&anchor value", Type::Raw),
            ("*missing", Type::Raw),
            ("[incomplete", Type::Raw),
        ] {
            let source = format!("---\nvalue: {token}\n---\nbody");
            let buffer = Buffer::from_text(&source);
            let decorations = parse_decorations(&source);
            assert_eq!(
                property_type(&buffer, &decorations, 1),
                Some(expected),
                "{token}"
            );
            assert_eq!(property_type(&buffer, &decorations, 0), None);
            assert_eq!(
                property_type(&buffer, &decorations, buffer.line_count() - 1),
                None
            );
            assert_eq!(property_type(&buffer, &decorations, usize::MAX), None);
        }
        let source = "---\nvalue:\n  - a\n  - b\n---";
        let buffer = Buffer::from_text(source);
        let decorations = parse_decorations(source);
        assert_eq!(property_type(&buffer, &decorations, 2), None);
        assert_eq!(property_type(&buffer, &decorations, 3), None);
        assert_eq!(TYPES.len(), 9);
        assert!(!TYPES.contains(&Type::Raw));
        let source = "---\nvalue: 1997-04-04T12:13:00\n---";
        let buffer = Buffer::from_text(source);
        let mut decorations = parse_decorations(source);
        apply_date_time_format(&mut decorations[1], "%Y年%m月%d日");
        assert_eq!(
            property_type(&buffer, &decorations, 1),
            Some(Type::DateTime)
        );
    }

    #[test]
    fn front_matter_type_same_type_and_raw_are_no_ops() {
        for source in [
            "---\nvalue: text\n---",
            "---\nvalue: true\n---",
            "---\nvalue: 42\n---",
            "---\nvalue: |\n  text\n---",
            "---\nvalue:\n  - a\n---",
            "---\nvalue:\n  nested: true\n---",
            "---\nvalue: null\n---",
            "---\nvalue: 1997-04-04\n---",
            "---\nvalue: 1997-04-04T00:00:00\n---",
        ] {
            let buffer = Buffer::from_text(source);
            let decorations = parse_decorations(source);
            disabled(source, 1, property_type(&buffer, &decorations, 1).unwrap());
            disabled(source, 1, Type::Raw);
        }
    }

    #[test]
    fn front_matter_type_text_is_quoted_yaml_safe_and_keeps_content() {
        for token in ["true", "0x0010", "null", "1997-04-04", "[a, b]", "{}"] {
            let source = format!("---\nvalue: {token} # keep\nnext: untouched\n---");
            let result = converted(&source, 1, Type::Text);
            assert!(result.contains(" # keep\nnext: untouched\n---"));
            let expected = match token {
                "null" | "{}" => "",
                "[a, b]" => "a\nb",
                _ => token,
            };
            assert_eq!(field(&result), serde_yaml::Value::String(expected.into()));
        }
        for text in [
            "a: b # not comment",
            "引号\"与\\路径🙂",
            "line\nnext",
            "\u{85}\u{2028}\u{2029}",
        ] {
            let token = super::quoted_text(text).unwrap();
            assert_eq!(serde_yaml::from_str::<String>(&token).unwrap(), text);
        }
    }

    #[test]
    fn front_matter_type_multiline_preserves_chomping_indentation_and_unicode() {
        for text in [
            "",
            "a",
            "日🙂\n本",
            "a\n",
            "a\n\n",
            "\n",
            "\n\n",
            "  leading\nnext",
            "\n  leading\n",
            "\tleading",
            " ",
            "  ",
            "\t",
            "\n ",
            " \n",
            "  \n\n",
            " \n \n",
            "\n\ncontent",
            "\n  \n",
            "\t\n",
            "\n\t",
            "a\n  \n",
            "# literal\n&also literal",
        ] {
            let source = format!(
                "---\nvalue: {} # header\nnext: keep\n---",
                super::quoted_text(text).unwrap()
            );
            let result = converted(&source, 1, Type::MultilineText);
            assert_eq!(
                field(&result),
                serde_yaml::Value::String(text.into()),
                "{result}"
            );
            assert!(result.contains(" # header\n"));
            assert!(result.ends_with("next: keep\n---"));
            let roundtrip = converted(&result, 1, Type::Text);
            assert_eq!(field(&roundtrip), serde_yaml::Value::String(text.into()));
            assert!(!roundtrip.contains("\n  "));
        }
        for (token, expected) in [
            ("|-\n  a\n  b", "a\nb"),
            (">-\n  a\n  b", "a b"),
            ("|2-\n    indented", "  indented"),
            ("first\n  continued", "first continued"),
        ] {
            let source = format!("---\nvalue: {token}\nnext: keep\n---");
            let result = converted(&source, 1, Type::Text);
            assert_eq!(field(&result), serde_yaml::Value::String(expected.into()));
            assert!(result.ends_with("\nnext: keep\n---"));
        }
    }

    #[test]
    fn front_matter_type_semantic_scalar_conversions_and_lossy_resets() {
        for (token, target, expected) in [
            ("'True'", Type::Checkbox, "true"),
            ("'FALSE'", Type::Checkbox, "false"),
            ("|-\n  true", Type::Checkbox, "true"),
            ("'0x10'", Type::Number, "0x10"),
            ("'1.25e+3'", Type::Number, "1.25e+3"),
            ("|-\n  -12.5", Type::Number, "-12.5"),
            ("'1997-04-04'", Type::Date, "1997-04-04"),
            ("'1997-04-04'", Type::DateTime, "1997-04-04T00:00:00"),
            ("1997-04-04", Type::DateTime, "1997-04-04T00:00:00"),
            ("1997-04-04T00:00:00", Type::Date, "1997-04-04"),
            (
                "'1997-04-04 12:13:00.125'",
                Type::DateTime,
                "1997-04-04T12:13:00.125",
            ),
            (
                "'1997-04-04T12:13:00.125+08:00'",
                Type::DateTime,
                "1997-04-04T12:13:00.125+08:00",
            ),
            ("'{}'", Type::Object, "{}"),
            ("'discard by choice'", Type::Empty, "null"),
            ("|-\n  discard\n  by choice", Type::Empty, "null"),
        ] {
            let source = format!("---\nvalue: {token}\nnext: keep\n---");
            assert_eq!(
                converted(&source, 1, target),
                format!("---\nvalue: {expected}\nnext: keep\n---")
            );
        }
        for token in [
            "'hello'",
            "'yes'",
            "' true '",
            "'1'",
            "123",
            "[true]",
            "[false, true]",
        ] {
            let result = converted(&format!("---\nvalue: {token}\n---"), 1, Type::Checkbox);
            assert_eq!(field(&result), serde_yaml::Value::Bool(false));
        }
        for token in ["'NaN'", "'1_000'", "'12px'", "'1:20'", "true", "[42]"] {
            let result = converted(&format!("---\nvalue: {token}\n---"), 1, Type::Number);
            assert_eq!(field(&result), serde_yaml::Value::Number(0.into()));
        }
        for token in [
            "'hello'",
            "'1997-02-29'",
            "'1997-4-4'",
            "'1997-04-04T25:13:00'",
            "42",
            "[1997-04-04]",
        ] {
            for target in [Type::Date, Type::DateTime] {
                let result = converted(&format!("---\nvalue: {token}\n---"), 1, target);
                let text = field(&result).as_str().unwrap().to_owned();
                assert!(super::iso_value(&text, target).is_some());
            }
        }
        for token in [
            "1997-04-04T12:13:00",
            "1997-04-04T00:00:00.1",
            "1997-04-04T00:00:00+08:00",
        ] {
            let result = converted(&format!("---\nvalue: {token}\n---"), 1, Type::Date);
            let date = field(&result).as_str().unwrap().to_owned();
            assert!(super::iso_value(&date, Type::Date).is_some());
        }
        for token in ["text", "true", "42", "[a]", "['{}']", "'{a: b}'"] {
            let result = converted(&format!("---\nvalue: {token}\n---"), 1, Type::Object);
            assert_eq!(
                field(&result),
                serde_yaml::Value::Mapping(Default::default())
            );
        }
    }

    #[test]
    fn front_matter_type_empty_defaults_are_explicit_and_keep_header_trivia() {
        for token in ["null", "~", "''", "[]", "{}", ""] {
            for target in TYPES {
                let source = format!("---\nvalue: {token} # note\nnext: keep\n---");
                let buffer = Buffer::from_text(&source);
                let decorations = parse_decorations(&source);
                if property_type(&buffer, &decorations, 1) == Some(target) {
                    disabled(&source, 1, target);
                    continue;
                }
                let result = converted(&source, 1, target);
                let actual = field(&result);
                let expected = match target {
                    Type::Text | Type::MultilineText => serde_yaml::Value::String(String::new()),
                    Type::Checkbox => serde_yaml::Value::Bool(false),
                    Type::Number => serde_yaml::Value::Number(0.into()),
                    Type::Empty => serde_yaml::Value::Null,
                    Type::List => serde_yaml::Value::Sequence(vec![]),
                    Type::Object => serde_yaml::Value::Mapping(Default::default()),
                    Type::Date | Type::DateTime => {
                        let date = chrono::Local::now()
                            .date_naive()
                            .format("%Y-%m-%d")
                            .to_string();
                        serde_yaml::Value::String(if target == Type::Date {
                            date
                        } else {
                            format!("{date}T00:00:00")
                        })
                    }
                    Type::Raw => unreachable!(),
                };
                assert_eq!(actual, expected, "{result}");
                assert!(result.contains("# note\n"));
                assert!(result.ends_with("next: keep\n---"));
            }
        }
        assert_eq!(
            converted("---\nvalue:\n---", 1, Type::Number),
            "---\nvalue: 0\n---"
        );
        assert_eq!(
            converted("---\nvalue: # note\n---", 1, Type::Checkbox),
            "---\nvalue: false # note\n---"
        );
    }

    #[test]
    fn front_matter_type_lists_have_owned_rows_and_yaml_safe_items() {
        let source = "---\nvalue: 'true\n\n  next' # header\nnext: keep\n---";
        // Lexically owned multiline quotes can be cleared even when a token-
        // local lossless decode is unavailable.
        assert_eq!(
            field(&converted(source, 1, Type::List)),
            serde_yaml::Value::Sequence(vec![])
        );
        for text in ["one", "true\n42\nnull\na: b\n# literal", "日🙂\n本\n"] {
            let source = format!(
                "---\nvalue: {} # keep\nnext: keep\n---",
                super::quoted_text(text).unwrap()
            );
            let result = converted(&source, 1, Type::List);
            let expected = text
                .split('\n')
                .map(|item| serde_yaml::Value::String(item.into()))
                .collect();
            assert_eq!(field(&result), serde_yaml::Value::Sequence(expected));
            assert!(result.contains("\n  - \""));
            assert!(result.ends_with("next: keep\n---"));
            let roundtrip = converted(&result, 1, Type::Text);
            assert_eq!(field(&roundtrip), serde_yaml::Value::String(text.into()));
            assert!(!roundtrip.contains("\n  -"));
        }
        for token in ["true", "0x10"] {
            let source = format!("---\nvalue: {token}\n---");
            let result = converted(&source, 1, Type::List);
            assert_eq!(
                field(&result),
                serde_yaml::Value::Sequence(vec![field(&source)])
            );
        }
        for token in ["\n  - one\n  - two", "\n- one\n- two", "[one, two]"] {
            for target in [Type::Text, Type::MultilineText, Type::Empty] {
                let source = format!("---\nvalue: {token} # final comment\nnext: keep\n---");
                let result = converted(&source, 1, target);
                assert!(result.contains("# final comment"));
                assert!(result.ends_with("next: keep\n---"));
                value(&result);
                let source = format!("---\nvalue: {token}\nnext: keep\n---");
                let result = converted(&source, 1, target);
                assert!(!result.contains("- one"));
                assert!(!result.contains("- two"));
                assert!(result.ends_with("next: keep\n---"));
                assert_eq!(
                    field(&result),
                    if target == Type::Empty {
                        serde_yaml::Value::Null
                    } else {
                        serde_yaml::Value::String("one\ntwo".into())
                    }
                );
            }
        }
    }

    #[test]
    fn front_matter_type_nested_compact_and_indentless_ownership_keeps_siblings() {
        let source = "-\nsections:\n  - title: keep\n    tags: # header\n    - 日\n    - 本\n    sibling: yes\n  - other: untouched\n-\nbody🙂";
        let result = converted(source, 3, Type::Text);
        assert_eq!(
            result,
            "-\nsections:\n  - title: keep\n    tags: \"日\\n本\" # header\n    sibling: yes\n  - other: untouched\n-\nbody🙂"
        );
        let source = "---\nparent:\n  '日🙂': |- # header\n    first\n    second\n  sibling: untouched\nnext: yes\n...\nbody";
        let result = converted(source, 2, Type::List);
        assert_eq!(
            result,
            "---\nparent:\n  '日🙂':  # header\n    - \"first\"\n    - \"second\"\n  sibling: untouched\nnext: yes\n...\nbody"
        );
        let parsed = value(&result);
        assert_eq!(
            parsed["parent"]["日🙂"],
            serde_yaml::Value::Sequence(vec!["first".into(), "second".into()])
        );
        let source = "---\nitems:\n  - name: \"a\\nb\"\n    sibling: keep\n  - other: keep\n---";
        let result = converted(source, 2, Type::MultilineText);
        assert_eq!(
            result,
            "---\nitems:\n  - name: |-\n      a\n      b\n    sibling: keep\n  - other: keep\n---"
        );
        assert_eq!(value(&source), value(&result));
    }

    #[test]
    fn front_matter_type_dedented_scalar_comments_stay_byte_exact() {
        for (parent, key, line, token, content_indent, comment_indent) in [
            ("", "value", 1, "|2-", 2, 1),
            ("", "value", 1, "|-2", 2, 1),
            ("", "value", 1, "|-", 4, 2),
            ("parent:\n", "  '日🙂'", 2, "|2-", 4, 3),
            ("parent:\n", "  '日🙂'", 2, "|-", 6, 4),
        ] {
            for eol in ["\n", "\r\n"] {
                for blank_before_comment in ["", "\n"] {
                    let content_spaces = " ".repeat(content_indent);
                    let key_spaces = " ".repeat(super::source_indent(key));
                    let prefix = format!("---\n{parent}{key}: ");
                    let tail = format!(
                        "{}# retain 日🙂\n{content_spaces}# retain after dedent too\n{key_spaces}next: keep # sibling\n...\nbody🙂\n",
                        " ".repeat(comment_indent),
                    );
                    let source = format!(
                        "{prefix}{token}  # header\n{content_spaces}content\n{content_spaces}# literal data\n{blank_before_comment}{tail}"
                    ).replace('\n', eol);
                    let buffer = Buffer::from_text(&source);
                    let decorations = parse_decorations(&source);
                    let owned = super::property(&buffer, &decorations, line).unwrap();
                    assert_eq!(
                        owned.end_line,
                        line + 3 + usize::from(!blank_before_comment.is_empty()),
                        "{source}",
                    );
                    for target in [Type::Text, Type::Empty, Type::List] {
                        let replacement = match target {
                            Type::Text => "\"content\\n# literal data\"  # header".to_owned(),
                            Type::Empty => "null  # header".to_owned(),
                            Type::List => format!(
                                "  # header\n{key_spaces}  - \"content\"\n{key_spaces}  - \"# literal data\""
                            ),
                            _ => unreachable!(),
                        };
                        let expected = format!("{prefix}{replacement}\n{tail}").replace('\n', eol);
                        let (range, _) =
                            type_change_edit(&buffer, &decorations, line, target).unwrap();
                        assert!(range.end < source.find("# retain 日🙂").unwrap());
                        assert!(buffer.text_for_range(range).contains("# literal data"));
                        let result = converted(&source, line, target);
                        assert_eq!(result, expected, "{token}: {target:?}");
                        let parsed = value(&result);
                        let actual = if parent.is_empty() {
                            parsed["value"].clone()
                        } else {
                            parsed["parent"]["日🙂"].clone()
                        };
                        let expected_value = match target {
                            Type::Text => {
                                serde_yaml::Value::String("content\n# literal data".into())
                            }
                            Type::Empty => serde_yaml::Value::Null,
                            Type::List => serde_yaml::Value::Sequence(vec![
                                "content".into(),
                                "# literal data".into(),
                            ]),
                            _ => unreachable!(),
                        };
                        assert_eq!(actual, expected_value);
                    }
                }
            }
        }
    }

    #[test]
    fn front_matter_type_crlf_utf8_and_comments_stay_byte_exact() {
        let source = "---\r\nparent:\r\n  \"日🙂\": |-   # header\r\n    引号\"\r\n    与🙂\r\n  # standalone\r\n  sibling:  keep # sibling\r\n...\r\n\r\n正文🙂\r\n";
        let result = converted(source, 2, Type::Text);
        assert_eq!(
            result,
            "---\r\nparent:\r\n  \"日🙂\": \"引号\\\"\\n与🙂\"   # header\r\n  # standalone\r\n  sibling:  keep # sibling\r\n...\r\n\r\n正文🙂\r\n"
        );
        let roundtrip = converted(&result, 2, Type::MultilineText);
        assert_eq!(roundtrip, source);
        let source = "---\r\nvalue: 'true'  # note\r\nnext: keep\r\n---\r\n";
        assert_eq!(
            converted(source, 1, Type::Checkbox),
            source.replace("'true'", "true")
        );
    }

    #[test]
    fn front_matter_type_comments_raw_and_complex_nodes_allow_resets() {
        for token in [
            "*missing",
            "&anchor text",
            "!custom text",
            "!!str text",
            "[a, *missing]",
            "[a, !custom b]",
            "[{nested: value}]",
            "[[a, b]]",
            "{nested: value}",
            "\n  nested: value",
            "[incomplete",
            "'unterminated",
            "\n  - a # item comment\n  - b",
            "\n  - a\n  # standalone\n  - b",
            "first\n  # comment\n  second",
            "[a, # comment\n b]",
            "!!str |-\n  # literal data\n  old body",
            "!custom &anchor\n- a\n- b",
            "!!str \"first\n  # quoted data\n  last\"",
        ] {
            let source = format!("---\nvalue: {token}\nnext: keep\n---");
            let buffer = Buffer::from_text(&source);
            let decorations = parse_decorations(&source);
            for target in TYPES {
                if property_type(&buffer, &decorations, 1) == Some(target) {
                    disabled(&source, 1, target);
                    continue;
                }
                let result = converted(&source, 1, target);
                value(&result);
                assert!(result.ends_with("next: keep\n---"), "{result}");
                assert!(!result.contains("old body"), "{result}");
                assert!(!result.contains("# quoted data"), "{result}");
            }
        }
        let source = "---\nparent:\n  value:\n    - a\n#   - comment shaped like a child\n    - b\n  next: keep\n---";
        for target in TYPES.into_iter().filter(|target| *target != Type::List) {
            let result = converted(source, 2, target);
            value(&result);
            assert!(result.contains("\n#   - comment shaped like a child\n"));
            assert!(result.ends_with("  next: keep\n---"));
        }
        let source = "---\noriginal: &anchor text\nvalue: [*anchor]\nnext: keep\n---";
        for target in [Type::Empty, Type::Text] {
            let result = converted(source, 2, target);
            value(&result);
            assert!(result.starts_with("---\noriginal: &anchor text\n"));
        }
        let source = "---\nvalue:\n  - a\n  # trailing comment\nnext: keep\n---";
        for target in [Type::MultilineText, Type::Text] {
            let result = converted(source, 1, target);
            assert!(result.contains("\n  # trailing comment\nnext: keep"));
            assert_eq!(field(&result), serde_yaml::Value::String("a".into()));
        }
        let source = "---\nvalue: |-\n  # literal content\n  !not a tag\nnext: keep\n---";
        let result = converted(source, 1, Type::Text);
        assert_eq!(
            field(&result),
            serde_yaml::Value::String("# literal content\n!not a tag".into())
        );
        for token in [
            "\"a\\rb\"",
            "\"a\\u0000b\"",
            "\"a\\u0085b\"",
            "\"a\\u2028b\"",
        ] {
            let result = converted(&format!("---\nvalue: {token}\n---"), 1, Type::MultilineText);
            assert_eq!(field(&result), serde_yaml::Value::String(String::new()));
            assert!(result.contains("value: |-\n  \n"));
        }
        let result = converted(
            "---\nvalue: \"a\\n\\n\"\n\nnext: keep\n---",
            1,
            Type::MultilineText,
        );
        assert_eq!(field(&result), serde_yaml::Value::String(String::new()));
        assert!(result.ends_with("\n\nnext: keep\n---"));
    }

    #[test]
    fn front_matter_type_all_nine_by_nine_preserve_siblings_and_source_trivia() {
        let sources = [
            ("'ordinary text'", Type::Text),
            ("|-\n    first\n    # literal data", Type::MultilineText),
            ("true", Type::Checkbox),
            ("1997-04-04", Type::Date),
            ("1997-04-04T12:13:00+08:00", Type::DateTime),
            ("\n    - one\n    - two", Type::List),
            ("0x10", Type::Number),
            ("null", Type::Empty),
            ("\n    nested:\n      old: data", Type::Object),
        ];
        for eol in ["\n", "\r\n"] {
            for (token, current) in sources {
                let prefix = "---\noriginal: &original keep # untouched\nparent:\n  '日🙂': ";
                let tail = "\n  # independent\n  sibling:  *original # sibling\nnext: keep\n...\n\nbody🙂\n";
                let source = format!("{prefix}{token}{tail}").replace('\n', eol);
                for target in TYPES {
                    let buffer = Buffer::from_text(&source);
                    let decorations = parse_decorations(&source);
                    assert_eq!(property_type(&buffer, &decorations, 3), Some(current));
                    if target == current {
                        disabled(&source, 3, target);
                        continue;
                    }
                    let result = converted(&source, 3, target);
                    assert!(
                        result.starts_with(&prefix.replace('\n', eol)),
                        "{current:?} -> {target:?}: {result}"
                    );
                    assert!(
                        result.ends_with(&tail.replace('\n', eol)),
                        "{current:?} -> {target:?}: {result}"
                    );
                    let parsed = value(&result);
                    assert_eq!(
                        parsed["parent"]["sibling"],
                        serde_yaml::Value::String("keep".into())
                    );
                    let result_buffer = Buffer::from_text(&result);
                    assert_eq!(
                        property_type(&result_buffer, &parse_decorations(&result), 3),
                        Some(target),
                        "{current:?} -> {target:?}: {result}"
                    );
                    if current == Type::Object {
                        assert!(!result.contains("old: data"));
                    }
                }
            }
        }
    }

    #[test]
    fn front_matter_type_root_anchors_survive_resets_without_rewriting_aliases() {
        for token in [
            "&keep !custom text",
            "!!str &keep |-\n  old body\n  # literal data",
            "&keep {nested: [old, data]}",
            "!custom &keep\n- old\n- data",
        ] {
            for eol in ["\n", "\r\n"] {
                let tail = "\ncopy: *keep # unchanged\nnext: keep\n...\nbody🙂";
                let source = format!("---\nvalue: {token}{tail}").replace('\n', eol);
                let tail = tail.replace('\n', eol);
                for target in TYPES {
                    let result = converted(&source, 1, target);
                    let parsed = value(&result);
                    assert_eq!(parsed["copy"], parsed["value"], "{result}");
                    assert!(result.ends_with(&tail));
                    assert!(result.contains("value: &keep "));
                    assert!(!result.contains("old body"));
                }
            }
        }
    }

    #[test]
    fn front_matter_type_valid_dedented_flow_rows_are_owned_not_new_siblings() {
        for token in [
            "[first,\nnext: flow data]",
            "{first: old,\nnext: flow data}",
            "\"first\nnext: quoted data\"",
            "!!str \"first\nnext: quoted data\"",
            "&keep [first,\nnext: flow data]",
        ] {
            let source = format!("---\nvalue: {token}\nsibling: keep # unrelated\n...\nbody🙂");
            value(&source);
            for target in TYPES {
                let result = converted(&source, 1, target);
                value(&result);
                assert!(result.ends_with("sibling: keep # unrelated\n...\nbody🙂"));
                assert!(!result.contains("next: flow data"), "{result}");
                assert!(!result.contains("next: quoted data"), "{result}");
                assert_eq!(value(&result).as_mapping().unwrap().len(), 2, "{result}");
            }
        }
    }

    #[test]
    fn front_matter_type_nested_literal_comments_are_data_not_retained_trivia() {
        for (token, current) in [
            (
                "\n  nested: |- # inner header\n    # literal old data\n    &literal text\n  # independent\n  other: old",
                Type::Object,
            ),
            (
                "\n  - |- # inner header\n    # literal old data\n    &literal text\n  # independent\n  - old",
                Type::List,
            ),
            (
                "\n  nested: !!str |2- # inner header\n    # literal old data\n    &literal text\n  # independent\n  other: old",
                Type::Object,
            ),
        ] {
            for target in TYPES.into_iter().filter(|target| *target != current) {
                let source = format!("---\nvalue: {token}\nnext: keep\n---");
                let result = converted(&source, 1, target);
                value(&result);
                assert!(result.contains("# inner header"), "{result}");
                assert!(result.contains("# independent"), "{result}");
                assert!(!result.contains("# literal old data"), "{result}");
                assert!(!result.contains("&literal text"), "{result}");
                assert!(result.ends_with("next: keep\n---"));
            }
        }
    }

    #[test]
    fn front_matter_type_deep_comment_trivia_moves_without_becoming_literal_content() {
        for eol in ["\n", "\r\n"] {
            for token in ["'text'", "\n  - text", "{nested: old}"] {
                let source = format!("---\nvalue: {token} # header\n            # deep independent\nnext: keep\n...\nbody🙂").replace('\n', eol);
                let result = converted(&source, 1, Type::MultilineText);
                assert!(
                    result.contains(&format!("            # deep independent{eol}value:")),
                    "{result}"
                );
                assert!(result.ends_with(&format!("next: keep{eol}...{eol}body🙂")));
                let expected = if token.starts_with('{') { "" } else { "text" };
                assert_eq!(field(&result), serde_yaml::Value::String(expected.into()));
            }
        }
    }

    #[test]
    fn front_matter_type_ambiguous_rows_and_external_descendant_aliases_are_not_deleted() {
        for source in [
            "---\nvalue: |2-\n underindented\n  broken\nnext: keep\n---",
            "---\nvalue: [incomplete\npossible: sibling\n  more: data]\nnext: keep\n---",
            "---\nvalue:\n  nested: &child old\ncopy: *child\nnext: keep\n---",
        ] {
            for target in TYPES.into_iter().filter(|target| *target != Type::Object) {
                disabled(source, 1, target);
            }
        }
        // A bad single-row token with a known sibling boundary is repairable.
        for token in [
            "[incomplete",
            "'unterminated",
            "*missing",
            "!custom [unfinished",
        ] {
            for target in TYPES {
                let result = converted(&format!("---\nvalue: {token}\nnext: keep\n---"), 1, target);
                value(&result);
                assert!(result.ends_with("next: keep\n---"));
            }
        }
    }

    #[test]
    fn front_matter_type_nested_scalar_syntax_is_data_not_sibling_ownership() {
        for token in [
            "\n  nested: |-\n    [",
            "\n  nested: |-\n    \"",
            "\n  - |-\n    [",
            "\n  nested: first\n    [",
            "\n  nested: first\n    \"",
            "\n  - first\n    [",
        ] {
            for eol in ["\n", "\r\n"] {
                let tail = "\nsibling: keep # untouched\n...\nbody🙂".replace('\n', eol);
                let source = format!("---\nvalue: {token}").replace('\n', eol) + &tail;
                value(&source);
                assert_eq!(
                    converted(&source, 1, Type::Empty),
                    format!("---{eol}value: null{tail}")
                );
            }
        }
    }

    #[test]
    fn front_matter_type_flow_anchor_delimiters_preserve_siblings_and_alias_users() {
        for token in ["[&keep]", "{key: &keep}", "[&keep, text]", "[&keep, *keep]"] {
            let source = format!("---\nvalue: {token}\nsibling: keep\n---");
            assert_eq!(
                converted(&source, 1, Type::Empty),
                "---\nvalue: null\nsibling: keep\n---"
            );
            let source = format!("---\nvalue: {token}\ncopy: *keep\nsibling: keep\n---");
            value(&source);
            disabled(&source, 1, Type::Empty);
        }
    }

    #[test]
    fn front_matter_type_flow_plain_colons_preserve_siblings_and_text() {
        for token in ["[a:'b]", "[a:\"b]", "[a:'b,\n two]"] {
            let source = format!("---\nvalue: {token}\nsibling: keep\n---");
            let result = converted(&source, 1, Type::Empty);
            assert_eq!(result, "---\nvalue: null\nsibling: keep\n---");
            let result = converted(&source, 1, Type::Text);
            let expected = field(&source)
                .as_sequence()
                .unwrap()
                .iter()
                .map(|item| item.as_str().unwrap())
                .collect::<Vec<_>>()
                .join("\n");
            assert_eq!(field(&result), serde_yaml::Value::String(expected));
            assert!(result.ends_with("sibling: keep\n---"));
        }
    }

    #[test]
    fn front_matter_type_dedented_flow_deindent_never_removes_content_bytes() {
        for continuation in ["two]", " two]", "  two]", "日本]", " 日本]"] {
            for eol in ["\n", "\r\n"] {
                let source = format!(
                    "---\nparent:\n  value: [one,\n{continuation}\n  sibling: keep # untouched\n---"
                )
                .replace('\n', eol);
                let original = value(&source);
                let expected = original["parent"]["value"]
                    .as_sequence()
                    .unwrap()
                    .iter()
                    .map(|item| item.as_str().unwrap())
                    .collect::<Vec<_>>()
                    .join("\n");
                for target in [Type::Text, Type::MultilineText] {
                    let result = converted(&source, 2, target);
                    assert_eq!(value(&result)["parent"]["value"], expected);
                    assert!(result.ends_with(&format!("  sibling: keep # untouched{eol}---")));
                }
            }
        }
    }

    #[test]
    fn front_matter_type_external_aliases_cannot_retarget_shadowed_anchors() {
        for earlier in ["earlier", "later"] {
            for eol in ["\n", "\r\n"] {
                let source = format!("---\noriginal: &child {earlier}\nvalue:\n  nested: &child later\ncopy: *child\nsibling: keep\n---").replace('\n', eol);
                value(&source);
                for target in TYPES.into_iter().filter(|target| *target != Type::Object) {
                    disabled(&source, 2, target);
                }
            }
        }
        // Shadowed definitions entirely inside the reset are disposable.
        let source = "---\noriginal: &child earlier\nvalue:\n  nested: &child later\n  copy: *child\nsibling: keep\n---";
        assert_eq!(
            converted(source, 2, Type::Empty),
            "---\noriginal: &child earlier\nvalue: null\nsibling: keep\n---"
        );
    }

    #[test]
    fn front_matter_type_edit_recording_positions_and_undo_are_one_replacement() {
        let source = "---\nvalue:\n  - 日\n  - 本\nnext: keep\n---\nbody🙂";
        let mut buffer = Buffer::from_text(source);
        let (range, replacement) =
            type_change_edit(&buffer, &parse_decorations(source), 1, Type::Text).unwrap();
        let mut cursor = crate::cursor::Cursor {
            offset: source.find("body").unwrap(),
            anchor: 0,
        };
        let mut extra = vec![crate::cursor::Cursor {
            offset: source.len(),
            anchor: range.start,
        }];
        let mut marked = Some(range.end..source.len());
        let mut extra_marked = vec![range.clone()];
        let mut affinity = Some(super::super::WrapBoundaryAffinity {
            offset: cursor.offset,
            kind: super::super::WrapBoundaryAffinityKind::Upstream,
        });
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
        let result = buffer.text().to_owned();
        assert_eq!(cursor.offset, result.find("body").unwrap());
        assert_eq!(extra[0].offset, result.len());
        assert_eq!(
            extra_marked[0],
            range.start..range.start + replacement.len()
        );
        assert_eq!(marked, Some(range.start + replacement.len()..result.len()));
        assert_eq!(affinity.unwrap().offset, cursor.offset);
        let edits = buffer.finish_edit_recording();
        assert_eq!(edits.len(), 1);
        assert!(buffer.apply_edits_inverse(&edits));
        assert_eq!(buffer.text(), source);
        assert!(buffer.apply_edits_forward(&edits));
        assert_eq!(buffer.text(), result);
    }

    #[test]
    fn front_matter_wrap_indent_keeps_values_inside_narrow_viewports() {
        assert_eq!(wrap_indent(60.0, Some(200.0)), 60.0);
        assert_eq!(wrap_indent(900.0, Some(200.0)), 140.0);
        assert_eq!(wrap_indent(900.0, None), 900.0);
        assert_eq!(wrap_indent(900.0, Some(0.0)), 0.0);
    }

    #[test]
    fn front_matter_date_setting_changes_display_without_changing_focused_source() {
        let source = "---\n日期: '1997-04-04T12:13:00' # original\n---";
        let original = crate::markdown::parse_decorations(source).remove(1);
        let line = source.lines().nth(1).unwrap();
        let display = |decoration: &crate::markdown::LineDecoration, active: bool| {
            let active_ranges = if active {
                vec![line.len()..line.len()]
            } else {
                vec![]
            };
            super::super::helpers::build_display_text(
                line,
                decoration,
                &active_ranges,
                active,
                None,
                0,
                3,
                3,
                4,
                0,
                0.8,
                0.8,
                0.8,
            )
            .0
        };
        for (format, expected) in [
            ("%Y年%m月%d日 %H:%M", "1997年04月04日 12:13"),
            ("%d/%m/%Y", "04/04/1997"),
        ] {
            let mut decoration = original.clone();
            apply_date_time_format(&mut decoration, format);
            assert!(display(&decoration, false).contains(expected));
            assert!(display(&decoration, true).contains("'1997-04-04T12:13:00' # original"));
        }
        let mut invalid = original.clone();
        apply_date_time_format(&mut invalid, "%Q");
        assert_eq!(invalid.front_matter, original.front_matter);
        assert!(display(&invalid, false).contains("'1997-04-04T12:13:00'"));
    }

    #[test]
    fn date_time_uses_chinese_format_exactly() {
        assert_eq!(
            format_yaml_date_time("1997-04-04T12:13:00", "%Y年%m月%d日 %H时%M分%S秒"),
            Some("1997年04月04日 12时13分00秒".to_string()),
        );
        assert_eq!(
            format_yaml_date_time("1997-04-04T12:13:00", "  %-m月%-d日 %%  "),
            Some("  4月4日 %  ".to_string()),
        );
        assert_eq!(
            format_yaml_date_time("1997-04-04T12:13:00", ""),
            Some(String::new()),
        );
    }

    #[test]
    fn naive_date_time_uses_default_format() {
        assert_eq!(
            format_yaml_date_time("1997-04-04T12:13:00", "%Y-%m-%d %H:%M"),
            Some("1997-04-04 12:13".to_string()),
        );
        assert_eq!(
            format_yaml_date_time("1997-04-04T12:13:00.123456789", "%H:%M:%S%.9f"),
            Some("12:13:00.123456789".to_string()),
        );
    }

    #[test]
    fn date_only_uses_date_component_without_inventing_time_or_timezone() {
        assert_eq!(
            format_yaml_date_time("1997-04-04", "%Y-%m-%d %H:%M"),
            Some("1997-04-04".to_string()),
        );
        assert_eq!(
            format_yaml_date_time("2000-02-29", "%Y年%-m月%-d日 %H:%M:%S%.3f %p"),
            Some("2000年2月29日".to_string()),
        );
        for source in ["1997-04-04", "1997-04-04T12:13:00"] {
            for format in ["%z", "%:z", "%Z", "%+"] {
                assert_eq!(format_yaml_date_time(source, format), None);
            }
        }
    }

    #[test]
    fn date_only_supports_all_settings_presets_and_custom_date_formats() {
        for (format, expected) in [
            ("%Y-%m-%d %H:%M", "1997-04-04"),
            ("%Y/%m/%d %H:%M", "1997/04/04"),
            ("%m/%d/%Y %I:%M %p", "04/04/1997"),
            ("%d/%m/%Y %H:%M", "04/04/1997"),
            ("%d.%m.%Y %H:%M", "04.04.1997"),
            ("%d %b %Y %H:%M", "04 Apr 1997"),
            ("%B %d, %Y %I:%M %p", "April 04, 1997"),
            ("%a, %d %b %Y %H:%M:%S %z", "Fri, 04 Apr 1997"),
            ("%Y-%m-%dT%H:%M:%S%:z", "1997-04-04"),
            ("%a %d%b %H:%M:%S %z", "Fri 04Apr"),
            ("%d %B %Y", "04 April 1997"),
            ("  %-m月%-d日 %%  ", "  4月4日 %  "),
            ("%Y年%m月%d日", "1997年04月04日"),
            ("%F", "1997-04-04"),
            ("", ""),
        ] {
            assert_eq!(
                format_yaml_date_time("1997-04-04", format),
                Some(expected.into()),
                "{format}"
            );
        }
        for (source, format, expected) in [
            ("1997-04-04T00:00:00", "%Y-%m-%d %H:%M", "1997-04-04 00:00"),
            (
                "1997-04-04T00:00:00",
                "%B %d, %Y %I:%M %p",
                "April 04, 1997 12:00 AM",
            ),
            (
                "1997-04-04T00:00:00+08:00",
                "%Y-%m-%dT%H:%M:%S%:z",
                "1997-04-04T00:00:00+08:00",
            ),
        ] {
            assert_eq!(format_yaml_date_time(source, format), Some(expected.into()));
        }
    }

    #[test]
    fn front_matter_date_only_preserves_date_punctuation_not_time_brackets() {
        for (format, expected) in [
            ("%Y-%m-%d (%a) %H:%M", "1997-04-04 (Fri)"),
            ("%Y-%m-%d [%a] (%H:%M)", "1997-04-04 [Fri]"),
            ("%Y-%m-%d (date) [%H:%M]", "1997-04-04 (date)"),
            ("%Y-%m-%d（%a） %H:%M", "1997-04-04（Fri）"),
            ("%Y年%m月%d日（%H:%M）", "1997年04月04日"),
            ("%Y年%m月%d日【%H:%M】", "1997年04月04日"),
            ("%Y-%m-%d. %H:%M", "1997-04-04."),
            ("%d %B %Y г. %H:%M", "04 April 1997 г."),
            ("%Y-%m-%dT%H:%M:%S", "1997-04-04"),
        ] {
            assert_eq!(
                format_yaml_date_time("1997-04-04", format),
                Some(expected.into()),
                "{format}"
            );
        }
        assert_eq!(
            format_yaml_date_time("1997-04-04T12:13:00", "%Y-%m-%d (%a) %H:%M"),
            Some("1997-04-04 (Fri) 12:13".into())
        );
    }

    #[test]
    fn date_only_setting_changes_display_without_midnight_or_source_edits() {
        let source = "---\nvalue: 1997-04-04 # original\n---";
        let original = parse_decorations(source)[1].clone();
        for (format, expected) in [
            ("%Y-%m-%d %H:%M", "1997-04-04"),
            ("%B %d, %Y %I:%M %p", "April 04, 1997"),
            ("%Y-%m-%dT%H:%M:%S%:z", "1997-04-04"),
        ] {
            let mut decoration = original.clone();
            apply_date_time_format(&mut decoration, format);
            assert_eq!(
                decoration.front_matter.as_ref().unwrap().value,
                crate::front_matter::FrontMatterValue::Text(expected.into())
            );
            let buffer = Buffer::from_text(source);
            assert_eq!(
                property_type(
                    &buffer,
                    &[parse_decorations(source)[0].clone(), decoration],
                    1
                ),
                Some(Type::Date)
            );
            assert_eq!(buffer.text(), source);
        }
        let mut invalid = original.clone();
        apply_date_time_format(&mut invalid, "%Y-%m-%d %Q");
        assert_eq!(invalid.front_matter, original.front_matter);
    }

    #[test]
    fn offset_timestamps_keep_their_wall_time_and_fraction() {
        for (source, expected) in [
            (
                "1997-04-04T00:13:00.123456789+08:00",
                "1997-04-04 00:13:00.123456789 +08:00",
            ),
            (
                "1997-04-04T23:13:00.5-07:30",
                "1997-04-04 23:13:00.500000000 -07:30",
            ),
            (
                "1997-04-04T12:13:00Z",
                "1997-04-04 12:13:00.000000000 +00:00",
            ),
            (
                "1997-04-04T12:13:00.125+00:00",
                "1997-04-04 12:13:00.125000000 +00:00",
            ),
        ] {
            assert_eq!(
                format_yaml_date_time(source, "%Y-%m-%d %H:%M:%S%.9f %:z"),
                Some(expected.to_string()),
                "{source}",
            );
        }
        assert_eq!(
            format_yaml_date_time("1997-04-04T12:13:00.125+08:00", "%+"),
            Some("1997-04-04T12:13:00.125+08:00".to_string()),
        );
    }

    #[test]
    fn malformed_dates_and_timestamps_are_not_formatted() {
        for source in [
            "",
            "null",
            "~",
            "1997-02-29",
            "1900-02-29",
            "1997-13-04",
            "1997-04-31",
            "1997-4-04",
            "1997-04-4",
            "1997-04-04T25:13:00",
            "1997-04-04T12:60:00",
            "1997-04-04T12:13",
            "1997-04-04T12:13:00.invalid",
            "1997-04-04T12:13:00+25:00",
            "1997-04-04T12:13:00Zjunk",
            "1997-04-04 trailing",
            "'1997-04-04'",
            "\"1997-04-04T12:13:00Z\"",
            "日期日期日期日期日期",
        ] {
            assert_eq!(
                format_yaml_date_time(source, "%Y-%m-%d %H:%M"),
                None,
                "{source}"
            );
        }
    }

    #[test]
    fn invalid_chrono_formats_return_none_without_panicking() {
        for source in ["1997-04-04", "1997-04-04T12:13:00", "1997-04-04T12:13:00Z"] {
            for format in ["%", "%Q", "%Y-%m-%d %", "%2f", "%:Q"] {
                assert_eq!(
                    format_yaml_date_time(source, format),
                    None,
                    "{source}: {format}"
                );
            }
        }
    }

    #[test]
    fn boolean_toggle_preserves_supported_casing_and_is_reversible() {
        for (source, expected) in [
            ("true", "false"),
            ("True", "False"),
            ("TRUE", "FALSE"),
            ("false", "true"),
            ("False", "True"),
            ("FALSE", "TRUE"),
        ] {
            assert_eq!(toggled_yaml_boolean(source), Some(expected));
            assert_eq!(toggled_yaml_boolean(expected), Some(source));
        }
    }

    #[test]
    fn boolean_toggle_rejects_non_boolean_tokens() {
        for source in [
            "",
            "null",
            "Null",
            "NULL",
            "~",
            "\"true\"",
            "'false'",
            "yes",
            "NO",
            "on",
            "off",
            "0",
            "1",
            "tRuE",
            "FaLsE",
            " true",
            "false ",
            "true # comment",
        ] {
            assert_eq!(toggled_yaml_boolean(source), None, "{source}");
        }
    }
}
