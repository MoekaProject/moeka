//! Pure Source-mode input. Plans use original-buffer byte ranges; their cursors
//! are absolute positions in the buffer after that plan alone. No rich-mode
//! formatting, table normalization, or persistent auto-pair provenance is used.
use std::ops::Range;

use crate::{buffer::Buffer, cursor::Cursor};

use super::input_ime::{self, SourceContextCache, SourceInputContext};

const MAX_SCAN_LINES: usize = 256;
const MAX_SCAN_BYTES: usize = 64 * 1024;
const MAX_CONTEXT_QUERIES: usize = 256;

fn char_boundary_after(text: &str, mut offset: usize) -> usize {
    while !text.is_char_boundary(offset) {
        offset += 1;
    }
    offset
}

fn char_boundary_before(text: &str, mut offset: usize) -> usize {
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn lookbehind_start(buffer: &Buffer, offset: usize) -> usize {
    let (line, _) = buffer.offset_to_line_col(offset);
    buffer
        .line_start(line.saturating_sub(MAX_SCAN_LINES - 1))
        .max(char_boundary_after(
            buffer.text(),
            offset.saturating_sub(MAX_SCAN_BYTES),
        ))
}

#[derive(Clone, Copy)]
pub(super) enum SourceAction<'a> {
    Insert(&'a str),
    Enter,
    Backspace,
    Delete,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SourceCursorEdit {
    pub range: Range<usize>,
    pub text: String,
    pub cursor: Cursor,
}

fn caret(offset: usize) -> Cursor {
    Cursor {
        offset,
        anchor: offset,
    }
}

fn replacement(range: Range<usize>, text: impl Into<String>) -> SourceCursorEdit {
    let text = text.into();
    let cursor = caret(range.start + text.len());
    SourceCursorEdit {
        range,
        text,
        cursor,
    }
}

fn delegated(edit: input_ime::SourceEditPlan) -> SourceCursorEdit {
    SourceCursorEdit {
        range: edit.range,
        text: edit.text,
        cursor: caret(edit.cursor_offset),
    }
}

pub(super) fn plan(
    buffer: &Buffer,
    cursor: Cursor,
    action: SourceAction<'_>,
    cache: &mut SourceContextCache,
) -> SourceCursorEdit {
    let cursor = Cursor {
        offset: buffer.clamp_grapheme_offset(cursor.offset),
        anchor: buffer.clamp_grapheme_offset(cursor.anchor),
    };
    match action {
        SourceAction::Insert(input) => insert(buffer, cursor, input, cache),
        SourceAction::Enter => paired_enter(buffer, cursor, cache)
            .unwrap_or_else(|| delegated(input_ime::source_enter_plan(buffer, cursor, cache))),
        SourceAction::Backspace | SourceAction::Delete => {
            if cursor.has_selection() {
                return replacement(cursor.selection_range(), "");
            }
            let backwards = matches!(action, SourceAction::Backspace);
            if let Some(edit) = empty_block_rollback(buffer, cursor.offset, cache) {
                return edit;
            }
            if let Some(edit) = empty_tag_delete(buffer, cursor.offset, backwards, cache) {
                return edit;
            }
            if let Some(edit) = empty_pair_delete(buffer, cursor.offset, backwards, cache) {
                return edit;
            }
            if backwards {
                if let Some(edit) = input_ime::source_backspace_plan(buffer, cursor, cache) {
                    return delegated(edit);
                }
                replacement(
                    buffer.prev_grapheme_boundary(cursor.offset)..cursor.offset,
                    "",
                )
            } else {
                replacement(
                    cursor.offset..buffer.next_grapheme_boundary(cursor.offset),
                    "",
                )
            }
        }
    }
}

fn escaped(text: &str, offset: usize) -> bool {
    text.as_bytes()[..offset]
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count()
        % 2
        != 0
}

fn word(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_' || ('\u{0300}'..='\u{036f}').contains(&ch)
}

fn previous(text: &str, offset: usize) -> Option<char> {
    text[..offset].chars().next_back()
}

fn next(text: &str, offset: usize) -> Option<char> {
    text[offset..].chars().next()
}

fn url_at(text: &str, offset: usize) -> bool {
    let boundary = |ch: char| ch.is_whitespace() || matches!(ch, '<' | '>' | '"' | '「' | '」');
    let lower = char_boundary_after(text, offset.saturating_sub(MAX_SCAN_BYTES));
    let upper = char_boundary_before(text, offset.saturating_add(MAX_SCAN_BYTES).min(text.len()));
    let left = text[lower..offset].rfind(boundary);
    let right = text[offset..upper].find(boundary);
    // An enormous unbroken token is not a safe place to infer Markdown syntax.
    if (left.is_none() && lower > 0) || (right.is_none() && upper < text.len()) {
        return true;
    }
    let start = left.map_or(lower, |i| {
        let i = lower + i;
        i + text[i..].chars().next().unwrap().len_utf8()
    });
    let end = right.map_or(upper, |i| offset + i);
    let token = &text[start..end];
    let url_start = if let Some(colon) = token.find("://") {
        let scheme = token[..colon]
            .char_indices()
            .rev()
            .find(|(_, ch)| !ch.is_ascii_alphanumeric() && !matches!(ch, '+' | '-' | '.'))
            .map_or(0, |(i, ch)| i + ch.len_utf8());
        if scheme == colon || !token.as_bytes()[scheme].is_ascii_alphabetic() {
            return false;
        }
        scheme
    } else if let Some(i) = token.find("www.").or_else(|| token.find("mailto:")) {
        i
    } else {
        return false;
    };
    let mut nesting = Vec::new();
    let mut url_end = token.len();
    for (i, ch) in token[url_start..].char_indices() {
        match ch {
            '(' => nesting.push(')'),
            '[' => nesting.push(']'),
            '{' => nesting.push('}'),
            ')' | ']' | '}' if nesting.last() == Some(&ch) => {
                nesting.pop();
            }
            ')' | ']' | '}' => {
                url_end = url_start + i;
                break;
            }
            _ => {}
        }
    }
    (start + url_start..=start + url_end).contains(&offset)
}

fn markdown_at(buffer: &Buffer, offset: usize, cache: &mut SourceContextCache) -> bool {
    input_ime::source_input_context(buffer, offset, cache) == SourceInputContext::Markdown
        && !url_at(buffer.text(), offset)
}

fn marker(ch: char) -> bool {
    matches!(ch, '*' | '_' | '~' | '+' | '=' | '`' | '$' | '^')
}

fn pair(ch: char) -> Option<(char, char)> {
    Some(match ch {
        '(' => ('(', ')'),
        '[' => ('[', ']'),
        '{' => ('{', '}'),
        // ASCII angles remain literal while a tag/autolink/comparison is typed.
        '<' => ('<', '>'),
        '"' => ('"', '"'),
        '\'' => ('\'', '\''),
        '［' => ('［', '］'),
        '【' => ('【', '】'),
        '｛' => ('｛', '｝'),
        '（' => ('（', '）'),
        '＜' => ('＜', '＞'),
        '《' => ('《', '》'),
        '〈' => ('〈', '〉'),
        '「' => ('「', '」'),
        '『' => ('『', '』'),
        '＂' => ('＂', '＂'),
        '＇' => ('＇', '＇'),
        '“' => ('“', '”'),
        '‘' => ('‘', '’'),
        _ => return None,
    })
}

fn quote(ch: char) -> bool {
    matches!(ch, '\'' | '"' | '＂' | '＇' | '“' | '‘')
}

fn wrap(buffer: &Buffer, cursor: Cursor, open: &str, close: &str) -> SourceCursorEdit {
    let range = cursor.selection_range();
    let mut edit = replacement(
        range.clone(),
        format!("{open}{}{close}", &buffer.text()[range.clone()]),
    );
    // The selection stays on the exact literal bytes, including its direction.
    edit.cursor = Cursor {
        offset: cursor.offset + open.len(),
        anchor: cursor.anchor + open.len(),
    };
    edit
}

fn insert(
    buffer: &Buffer,
    cursor: Cursor,
    input: &str,
    cache: &mut SourceContextCache,
) -> SourceCursorEdit {
    let text = buffer.text();
    let offset = cursor.offset;
    let plain = || replacement(cursor.selection_range(), input);
    let mut chars = input.chars();
    let Some(ch) = chars.next() else {
        return plain();
    };
    // Pasted strings and IME commits are literal, even if they spell a marker.
    if chars.next().is_some() {
        return plain();
    }
    if cursor.has_selection() {
        if let Some((open, close)) = pair(ch) {
            return wrap(buffer, cursor, &open.to_string(), &close.to_string());
        }
        if marker(ch) && markdown_at(buffer, cursor.selection_range().start, cache) {
            let delimiter = if matches!(ch, '+' | '=') {
                ch.to_string().repeat(2)
            } else {
                ch.to_string()
            };
            return wrap(buffer, cursor, &delimiter, &delimiter);
        }
        return plain();
    }

    if ch == '\\'
        && !escaped(text, offset)
        && ((text[offset..].starts_with("\\]") && unclosed_tex(buffer, offset, '[', cache))
            || (text[offset..].starts_with("\\)") && unclosed_tex(buffer, offset, '(', cache)))
    {
        return SourceCursorEdit {
            range: offset..offset,
            text: String::new(),
            cursor: caret(offset + 1),
        };
    }
    // A single escaped bracket is a TeX delimiter, not an ordinary empty pair.
    if escaped(text, offset) {
        if next(text, offset) == Some(ch)
            && matches!(ch, ']' | ')')
            && unclosed_tex(buffer, offset - 1, if ch == ']' { '[' } else { '(' }, cache)
        {
            return SourceCursorEdit {
                range: offset..offset,
                text: String::new(),
                cursor: caret(offset + 1),
            };
        }
        if matches!(ch, '[' | '(')
            && offset > 0
            && !escaped(text, offset - 1)
            && markdown_at(buffer, offset - 1, cache)
            && opener_boundary(text, offset - 1)
            && next(text, offset).is_none_or(|c| !word(c))
        {
            let close = if ch == '[' { "\\]" } else { "\\)" };
            let mut edit = replacement(offset..offset, format!("{ch}{close}"));
            edit.cursor = caret(offset + 1);
            return edit;
        }
        return plain();
    }

    if ch == ' ' && text[..offset].ends_with('*') && text[offset..].starts_with('*') {
        let start = offset - 1;
        if list_position(text, start) && markdown_at(buffer, start, cache) {
            return replacement(offset..offset + 1, " ");
        }
    }

    if marker(ch) {
        if ch == '^' && previous(text, offset) == Some('[') {
            return plain();
        }
        if let Some((start, left, right)) = empty_marker(text, offset, ch)
            && left == right
            && empty_run_end(text, start + left + right)
            && marker_allowed(buffer, start, ch, left, cache)
        {
            let max = match ch {
                '*' | '_' => 3,
                '$' | '+' | '=' => 2,
                '`' | '~' => usize::MAX,
                _ => 1,
            };
            if left < max {
                let mut edit = replacement(offset..offset, ch.to_string().repeat(2));
                edit.cursor = caret(offset + 1);
                return edit;
            }
        }
        // + and = must remain usable as a list marker / Setext underline.
        if matches!(ch, '+' | '=') && previous(text, offset) == Some(ch) {
            let start = offset - 1;
            if opener_boundary(text, start)
                && markdown_at(buffer, start, cache)
                && next(text, offset).is_none_or(|c| !word(c) && c != ch)
            {
                let mut edit = replacement(offset..offset, ch.to_string().repeat(3));
                edit.cursor = caret(offset + 1);
                return edit;
            }
        }
        if text[offset..].starts_with(ch) && matching_marker(buffer, offset, ch, cache) {
            return SourceCursorEdit {
                range: offset..offset,
                text: String::new(),
                cursor: caret(offset + 1),
            };
        }
        if !matches!(ch, '+' | '=')
            && markdown_at(buffer, offset, cache)
            && opener_boundary(text, offset)
            && next(text, offset).is_none_or(|c| !word(c) && c != ch)
        {
            let mut edit = replacement(offset..offset, ch.to_string().repeat(2));
            edit.cursor = caret(offset + 1);
            return edit;
        }
        return plain();
    }

    if ch == '>' {
        if let Some(edit) = html_completion(buffer, offset, cache) {
            return edit;
        }
        return plain();
    }
    if next(text, offset) == Some(ch) && balanced_closer(buffer, offset, ch, cache) {
        return SourceCursorEdit {
            range: offset..offset,
            text: String::new(),
            cursor: caret(offset + ch.len_utf8()),
        };
    }
    if let Some((open, close)) = pair(ch)
        && ch != '<'
        && next(text, offset).is_none_or(|c| !word(c))
        && (!quote(ch) || previous(text, offset).is_none_or(|c| !word(c)))
    {
        let mut edit = replacement(offset..offset, format!("{open}{close}"));
        edit.cursor = caret(offset + open.len_utf8());
        return edit;
    }
    plain()
}

fn opener_boundary(text: &str, offset: usize) -> bool {
    let slashes = text.as_bytes()[..offset]
        .iter()
        .rev()
        .take_while(|b| **b == b'\\')
        .count();
    slashes % 2 == 0
        && previous(text, offset - slashes).is_none_or(|ch| {
            !word(ch) && !matches!(ch, '\\' | '/' | '*' | '~' | '+' | '=' | '`' | '$' | '^')
        })
        && !url_at(text, offset)
}

fn list_position(text: &str, offset: usize) -> bool {
    let start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    text[start..offset]
        .chars()
        .all(|ch| matches!(ch, ' ' | '\t' | '>'))
}

fn empty_run_end(text: &str, offset: usize) -> bool {
    next(text, offset).is_none_or(|ch| {
        ch.is_whitespace()
            || matches!(
                ch,
                ')' | ']'
                    | '}'
                    | '>'
                    | '」'
                    | '』'
                    | '）'
                    | '］'
                    | '】'
                    | '》'
                    | '〉'
                    | '.'
                    | ','
                    | '!'
                    | '?'
                    | ';'
                    | ':'
                    | '，'
                    | '。'
                    | '！'
                    | '？'
            )
    })
}

fn empty_marker(text: &str, offset: usize, ch: char) -> Option<(usize, usize, usize)> {
    let left = text.as_bytes()[..offset]
        .iter()
        .rev()
        .take(MAX_SCAN_BYTES + 1)
        .take_while(|b| **b == ch as u8)
        .count();
    let right = text.as_bytes()[offset..]
        .iter()
        .take(MAX_SCAN_BYTES + 1)
        .take_while(|b| **b == ch as u8)
        .count();
    (left > 0 && right > 0 && left <= MAX_SCAN_BYTES && right <= MAX_SCAN_BYTES).then_some((
        offset - left,
        left,
        right,
    ))
}

/// A same-line fence is already classified as Code/Math. Query its prefix using
/// the scanner's cached pre-line state, never a cloned/scanned document prefix.
/// Fence-looking text inside an existing literal block still remains literal.
fn marker_allowed(
    buffer: &Buffer,
    start: usize,
    ch: char,
    run: usize,
    cache: &mut SourceContextCache,
) -> bool {
    let (line, column) = buffer.offset_to_line_col(start);
    if column > MAX_SCAN_BYTES || !opener_boundary(buffer.text(), start) {
        return false;
    }
    if markdown_at(buffer, start, cache) {
        return true;
    }
    if !((matches!(ch, '`' | '~') && run > 0) || (ch == '$' && run <= 2) || ch == '\\') {
        return false;
    }
    let before = &buffer.text()[buffer.line_start(line)..start];
    container_prefix(before).is_some()
        && input_ime::source_input_context_before(buffer, start, cache)
            == SourceInputContext::Markdown
}

fn matching_marker(
    buffer: &Buffer,
    offset: usize,
    ch: char,
    cache: &mut SourceContextCache,
) -> bool {
    let text = buffer.text();
    let (line, _) = buffer.offset_to_line_col(offset);
    let line_start = buffer.line_start(line);
    if offset - line_start > MAX_SCAN_BYTES {
        return false;
    }
    let closing_start = text[line_start..offset]
        .char_indices()
        .rev()
        .find(|(_, c)| *c != ch)
        .map_or(line_start, |(i, c)| line_start + i + c.len_utf8());
    let right = text.as_bytes()[offset..]
        .iter()
        .take(MAX_SCAN_BYTES + 1)
        .take_while(|b| **b == ch as u8)
        .count();
    if right > MAX_SCAN_BYTES {
        return false;
    }
    let closing_end = offset + right;
    let run = closing_end - closing_start;
    if run > 0
        && run % 2 == 0
        && empty_run_end(text, closing_end)
        && offset >= closing_start + run / 2
        && marker_allowed(buffer, closing_start, ch, run / 2, cache)
    {
        return true;
    }
    if run == 0
        || escaped(text, closing_start)
        || (ch != '`' && next(text, closing_end).is_some_and(word))
        || previous(text, closing_start).is_none_or(char::is_whitespace)
    {
        return false;
    }
    let mut index = line_start;
    let mut open = None;
    let mut queries = 0;
    while index < closing_start {
        if text.as_bytes()[index] == ch as u8 {
            queries += 1;
            if queries > MAX_CONTEXT_QUERIES {
                return false;
            }
            let count = text.as_bytes()[index..closing_start]
                .iter()
                .take_while(|b| **b == ch as u8)
                .count();
            let literal = !matches!(ch, '`' | '~' | '$')
                && input_ime::source_input_context(buffer, index, cache)
                    != SourceInputContext::Markdown;
            if count == run && !escaped(text, index) && !literal {
                if open.is_some() {
                    open = None;
                } else if (marker_allowed(buffer, index, ch, run, cache)
                    || (ch == '`' && markdown_at(buffer, index, cache)))
                    && next(text, index + run).is_some_and(|c| !c.is_whitespace())
                {
                    open = Some(index);
                }
            }
            index += count;
        } else {
            index += text[index..].chars().next().unwrap().len_utf8();
        }
    }
    open.is_some()
}

/// A closer is skippable only if a real unclosed opener survives a small lexical
/// scan. Quotes hide brackets; apostrophes in words never start a quote.
fn balanced_closer(
    buffer: &Buffer,
    offset: usize,
    closing: char,
    cache: &mut SourceContextCache,
) -> bool {
    let text = buffer.text();
    let (mut line, _) = buffer.offset_to_line_col(offset);
    let lower = lookbehind_start(buffer, offset);
    let mut start = buffer.line_start(line);
    if start < lower {
        return false;
    }
    let context = input_ime::source_input_context(buffer, offset, cache);
    // An unmatched quote/bracket in a different paragraph or literal block must
    // not make a later, unrelated closer disappear.
    if !input_ime::source_context_boundary_before(buffer, line, cache) {
        while line > 0 {
            if buffer.line_start(line - 1) < lower {
                return false;
            }
            // Distinct adjacent literal blocks can have the same enum kind.
            // Do not include their opener/closer or container-boundary lines.
            if input_ime::source_context_boundary_before(buffer, line - 1, cache)
                || (context == SourceInputContext::Markdown
                    && buffer.line_content(line - 1).trim().is_empty())
                || input_ime::source_input_context(buffer, buffer.line_end(line - 1), cache)
                    != context
            {
                break;
            }
            line -= 1;
            start = buffer.line_start(line);
        }
    }
    let mut stack = Vec::new();
    let mut quoted = None;
    let mut queries = 0;
    for (relative, ch) in text[start..offset].char_indices() {
        let index = start + relative;
        if pair(ch).is_none()
            && !matches!(
                ch,
                ')' | ']'
                    | '}'
                    | '］'
                    | '】'
                    | '｝'
                    | '）'
                    | '＞'
                    | '》'
                    | '〉'
                    | '」'
                    | '』'
                    | '”'
                    | '’'
            )
        {
            continue;
        }
        queries += 1;
        if queries > MAX_CONTEXT_QUERIES {
            return false;
        }
        let at = input_ime::source_input_context(buffer, index, cache);
        let link_destination =
            ch == '(' && previous(text, index) == Some(']') && context == SourceInputContext::Html;
        if at != context && !link_destination {
            continue;
        }
        if escaped(text, index) {
            continue;
        }
        if let Some(close) = quoted {
            if ch == close {
                quoted = None;
            }
            continue;
        }
        if let Some((_, close)) = pair(ch) {
            if quote(ch) {
                if previous(text, index).is_none_or(|c| !word(c)) {
                    quoted = Some(close);
                }
            } else if ch != '<' {
                stack.push(close);
            }
        } else if stack.last() == Some(&ch) {
            stack.pop();
        } else if matches!(
            ch,
            ')' | ']' | '}' | '］' | '】' | '｝' | '）' | '＞' | '》' | '〉' | '」' | '』'
        ) {
            stack.clear();
        }
    }
    !escaped(text, offset)
        && (quoted == Some(closing) || (quoted.is_none() && stack.last() == Some(&closing)))
}

fn unclosed_tex(
    buffer: &Buffer,
    offset: usize,
    opening: char,
    cache: &mut SourceContextCache,
) -> bool {
    let text = buffer.text();
    let open = format!("\\{opening}");
    let close = if opening == '[' { "\\]" } else { "\\)" };
    let lower = lookbehind_start(buffer, offset);
    let Some((start, _)) = text[lower..offset]
        .rmatch_indices(&open)
        .map(|(i, matched)| (lower + i, matched))
        .find(|(i, _)| !escaped(text, *i))
    else {
        return false;
    };
    !text[start + open.len()..offset]
        .match_indices(close)
        .any(|(i, _)| !escaped(text, start + open.len() + i))
        && marker_allowed(buffer, start, '\\', 1, cache)
}

fn empty_pair_delete(
    buffer: &Buffer,
    offset: usize,
    backwards: bool,
    cache: &mut SourceContextCache,
) -> Option<SourceCursorEdit> {
    let text = buffer.text();
    // Source historically removes the whole empty wiki pair; literal nested
    // brackets in code/math/YAML keep ordinary single-pair deletion instead.
    let wiki_start = if text[..offset].ends_with("[[") && text[offset..].starts_with("]]") {
        Some(offset - 2)
    } else if !backwards && text[offset..].starts_with("[[]]") {
        Some(offset)
    } else {
        None
    };
    if let Some(start) = wiki_start
        && !escaped(text, start)
        && markdown_at(buffer, start, cache)
    {
        return Some(replacement(start..start + 4, ""));
    }
    for (open, close) in [("\\[", "\\]"), ("\\(", "\\)")] {
        let start = if text[..offset].ends_with(open) {
            Some(offset - open.len())
        } else if !backwards && text[offset..].starts_with(open) {
            Some(offset)
        } else {
            None
        };
        if let Some(start) = start
            && text[start + open.len()..].starts_with(close)
            && marker_allowed(buffer, start, '\\', 1, cache)
        {
            return Some(replacement(start..start + open.len() + close.len(), ""));
        }
    }
    if let Some(ch) = next(text, offset).filter(|c| marker(*c)) {
        let (start, left, right) = empty_marker(text, offset, ch).unwrap_or_else(|| {
            let run = text.as_bytes()[offset..]
                .iter()
                .take(MAX_SCAN_BYTES + 1)
                .take_while(|b| **b == ch as u8)
                .count();
            (offset, 0, run)
        });
        let (start, count, center) = if left == right && left > 0 {
            (start, left, true)
        } else if !backwards && left == 0 && right % 2 == 0 {
            (offset, right / 2, false)
        } else {
            return None;
        };
        let max = match ch {
            '*' | '_' => 3,
            '$' | '+' | '=' => 2,
            '`' | '~' => usize::MAX,
            _ => 1,
        };
        if count > 0
            && count * 2 <= MAX_SCAN_BYTES
            && count <= max
            && empty_run_end(text, start + count * 2)
            && marker_allowed(buffer, start, ch, count, cache)
        {
            let mut edit = replacement(
                start..start + count * 2,
                ch.to_string().repeat((count - 1) * 2),
            );
            edit.cursor = caret(start + if center { count - 1 } else { 0 });
            return Some(edit);
        }
    }
    let (start, open) = if let Some((start, ch)) = text[..offset].char_indices().next_back()
        && let Some((_, close)) = pair(ch)
        && next(text, offset) == Some(close)
    {
        (start, ch)
    } else if !backwards {
        let ch = next(text, offset)?;
        let (_, close) = pair(ch)?;
        if next(text, offset + ch.len_utf8()) != Some(close) {
            return None;
        }
        (offset, ch)
    } else {
        return None;
    };
    let (_, close) = pair(open)?;
    if open == '<'
        || escaped(text, start)
        || (quote(open) && previous(text, start).is_some_and(word))
    {
        return None;
    }
    Some(replacement(
        start..start + open.len_utf8() + close.len_utf8(),
        "",
    ))
}

/// Return the indentation/quote/list continuation corresponding to a fence's
/// prefix. Four-space indented code is intentionally not a fence opener.
fn container_prefix(prefix: &str) -> Option<String> {
    let mut rest = prefix;
    let mut continuation = String::new();
    loop {
        let spaces = rest.bytes().take_while(|b| *b == b' ').count();
        if spaces > 3 {
            return None;
        }
        continuation.push_str(&rest[..spaces]);
        rest = &rest[spaces..];
        if rest.is_empty() {
            return Some(continuation);
        }
        if let Some(after) = rest.strip_prefix('>') {
            continuation.push('>');
            rest = after;
            if let Some(after) = rest.strip_prefix(' ') {
                continuation.push(' ');
                rest = after;
            }
            continue;
        }
        let list = if ["- ", "+ ", "* "].iter().any(|p| rest.starts_with(p)) {
            2
        } else {
            let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
            if digits > 0
                && digits <= 9
                && matches!(rest.as_bytes().get(digits), Some(b'.' | b')'))
                && rest.as_bytes().get(digits + 1) == Some(&b' ')
            {
                digits + 2
            } else {
                return None;
            }
        };
        continuation.push_str(&" ".repeat(list));
        rest = &rest[list..];
    }
}

fn paired_enter(
    buffer: &Buffer,
    cursor: Cursor,
    cache: &mut SourceContextCache,
) -> Option<SourceCursorEdit> {
    if cursor.has_selection() {
        return None;
    }
    let text = buffer.text();
    let (line, col) = buffer.offset_to_line_col(cursor.offset);
    let content = buffer.line_content(line);
    if content.len() > MAX_SCAN_BYTES {
        return None;
    }
    let before = &content[..col];
    let after = &content[col..];
    let (start, ch, run) = if let Some(ch @ ('`' | '~')) = after.chars().next() {
        let run = after.bytes().take_while(|b| *b == ch as u8).count();
        if run < 3 || !after[run..].trim().is_empty() {
            return None;
        }
        let start = before.find(ch)?;
        if before[start..]
            .bytes()
            .take_while(|b| *b == ch as u8)
            .count()
            != run
        {
            return None;
        }
        let language = &before[start + run..];
        if !language
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '.' | '#'))
        {
            return None;
        }
        (start, ch, run)
    } else if after.trim_end() == "$$" && before.ends_with("$$") {
        (before.len() - 2, '$', 2)
    } else if after.trim_end() == "\\]" && before.ends_with("\\[") {
        (before.len() - 2, '\\', 1)
    } else {
        return None;
    };
    let prefix = container_prefix(&before[..start])?;
    let absolute = buffer.line_start(line) + start;
    if !marker_allowed(buffer, absolute, ch, run, cache) || escaped(text, absolute) {
        return None;
    }
    let mut edit = replacement(
        cursor.offset..cursor.offset,
        format!("\n{prefix}\n{prefix}"),
    );
    edit.cursor = caret(cursor.offset + 1 + prefix.len());
    Some(edit)
}

const HTML_TAGS: &str = " a abbr address article aside audio b bdi bdo blockquote body button canvas caption cite code colgroup data datalist dd del details dfn dialog div dl dt em fieldset figcaption figure footer form h1 h2 h3 h4 h5 h6 head header hgroup html i iframe ins kbd label legend li main map mark menu meter nav noscript object ol optgroup option output p picture pre progress q rp rt ruby s samp script search section select slot small span strong style sub summary sup table tbody td template textarea tfoot th thead time title tr u ul var video ";
const HTML_VOID: &str = " area base br col embed hr img input link meta param source track wbr ";

/// Parse only a known opening tag with complete, single-line attributes. This
/// rejects comparisons, autolinks, declarations, closing tags and custom names.
fn html_name(body: &str) -> Option<&str> {
    let bytes = body.as_bytes();
    let len = bytes
        .iter()
        .take_while(|b| b.is_ascii_alphanumeric())
        .count();
    if len == 0 {
        return None;
    }
    let name = &body[..len];
    let lower = name.to_ascii_lowercase();
    if !HTML_TAGS.contains(&format!(" {lower} ")) && !HTML_VOID.contains(&format!(" {lower} ")) {
        return None;
    }
    let mut i = len;
    while i < bytes.len() {
        if !matches!(bytes[i], b' ' | b'\t') {
            return None;
        }
        while bytes.get(i).is_some_and(|b| matches!(b, b' ' | b'\t')) {
            i += 1;
        }
        if i == bytes.len() {
            return Some(name);
        }
        if !bytes[i].is_ascii_alphabetic() && !matches!(bytes[i], b'_' | b':') {
            return None;
        }
        i += 1;
        while bytes
            .get(i)
            .is_some_and(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b':' | b'-' | b'.'))
        {
            i += 1;
        }
        let end = i;
        while bytes.get(i).is_some_and(|b| matches!(b, b' ' | b'\t')) {
            i += 1;
        }
        if bytes.get(i) != Some(&b'=') {
            i = end;
            continue;
        }
        i += 1;
        while bytes.get(i).is_some_and(|b| matches!(b, b' ' | b'\t')) {
            i += 1;
        }
        let value = *bytes.get(i)?;
        if matches!(value, b'\'' | b'"') {
            i += 1;
            while bytes
                .get(i)
                .is_some_and(|b| *b != value && !matches!(b, b'\n' | b'\r'))
            {
                i += 1;
            }
            if bytes.get(i) != Some(&value) {
                return None;
            }
            i += 1;
        } else {
            let start = i;
            while bytes.get(i).is_some_and(|b| {
                !b.is_ascii_whitespace() && !matches!(b, b'<' | b'>' | b'\'' | b'"' | b'=' | b'`')
            }) {
                i += 1;
            }
            if start == i {
                return None;
            }
        }
    }
    Some(name)
}

fn html_completion(
    buffer: &Buffer,
    offset: usize,
    cache: &mut SourceContextCache,
) -> Option<SourceCursorEdit> {
    let text = buffer.text();
    let (line, column) = buffer.offset_to_line_col(offset);
    if column > MAX_SCAN_BYTES {
        return None;
    }
    let start = text[buffer.line_start(line)..offset].rfind('<')? + buffer.line_start(line);
    if escaped(text, start) {
        return None;
    }
    let body = text[start + 1..offset].trim_end_matches([' ', '\t']);
    let selfclosing = body.ends_with('/');
    let name = html_name(if selfclosing {
        &body[..body.len() - 1]
    } else {
        body
    })?;
    if !input_ime::source_html_tag_assistance_allowed(buffer, start, cache) || url_at(text, start) {
        return None;
    }
    if text[offset..].starts_with('>') {
        return Some(SourceCursorEdit {
            range: offset..offset,
            text: String::new(),
            cursor: caret(offset + 1),
        });
    }
    if selfclosing || HTML_VOID.contains(&format!(" {} ", name.to_ascii_lowercase())) {
        return None;
    }
    let suffix = &text[offset..];
    let close = format!("</{name}>");
    let adjacent = suffix
        .get(..close.len())
        .is_some_and(|s| s.eq_ignore_ascii_case(&close));
    let mut edit = replacement(
        offset..offset,
        if adjacent {
            ">".into()
        } else {
            format!(">{close}")
        },
    );
    edit.cursor = caret(offset + 1);
    Some(edit)
}

fn nonvoid_html_close(opening: &str) -> Option<String> {
    let body = opening
        .strip_prefix('<')?
        .strip_suffix('>')?
        .trim_end_matches([' ', '\t']);
    if body.ends_with('/') {
        return None;
    }
    let name = html_name(body)?;
    (!HTML_VOID.contains(&format!(" {} ", name.to_ascii_lowercase()))).then(|| format!("</{name}>"))
}

fn tag_rollback_allowed(buffer: &Buffer, start: usize, cache: &mut SourceContextCache) -> bool {
    buffer.offset_to_line_col(start).1 <= MAX_SCAN_BYTES
        && !escaped(buffer.text(), start)
        && !url_at(buffer.text(), start)
        && input_ime::source_html_tag_assistance_allowed(buffer, start, cache)
}

fn empty_tag_delete(
    buffer: &Buffer,
    offset: usize,
    backwards: bool,
    cache: &mut SourceContextCache,
) -> Option<SourceCursorEdit> {
    let text = buffer.text();
    let (line, column) = buffer.offset_to_line_col(offset);
    if column > MAX_SCAN_BYTES {
        return None;
    }
    let (start, end) = if text[..offset].ends_with('>') && text[offset..].starts_with("</") {
        (
            buffer.line_start(line) + text[buffer.line_start(line)..offset].rfind('<')?,
            offset,
        )
    } else if !backwards && text[offset..].starts_with('<') {
        let limit = buffer
            .line_end(line)
            .min(offset.saturating_add(MAX_SCAN_BYTES));
        let mut quote = None;
        let mut end = None;
        for (relative, byte) in text.as_bytes()[offset..limit].iter().enumerate() {
            match quote {
                Some(q) if *byte == q => quote = None,
                Some(_) => {}
                None if matches!(byte, b'\'' | b'"') => quote = Some(*byte),
                None if *byte == b'>' => {
                    end = Some(offset + relative + 1);
                    break;
                }
                _ => {}
            }
        }
        (offset, end?)
    } else {
        return None;
    };
    let close = nonvoid_html_close(&text[start..end])?;
    if !text[end..]
        .get(..close.len())
        .is_some_and(|s| s.eq_ignore_ascii_case(&close))
        || !tag_rollback_allowed(buffer, start, cache)
    {
        return None;
    }
    Some(replacement(start..end + close.len(), ""))
}

/// Reverse the locally recognizable three-line expansion, not arbitrary blank
/// content somewhere inside a block. Preserve the opener (including language or
/// attributes) and collapse back to its same-line empty pair first.
fn empty_block_rollback(
    buffer: &Buffer,
    offset: usize,
    cache: &mut SourceContextCache,
) -> Option<SourceCursorEdit> {
    let (line, _) = buffer.offset_to_line_col(offset);
    if line == 0
        || line + 1 >= buffer.line_count()
        || offset != buffer.line_end(line)
        || buffer.line_end(line + 1) - buffer.line_start(line - 1) > MAX_SCAN_BYTES
    {
        return None;
    }
    let opening = buffer.line_content(line - 1);
    let middle = buffer.line_content(line);
    let closing = buffer.line_content(line + 1);
    if let Some(open) = opening
        .chars()
        .next_back()
        .filter(|c| matches!(c, '(' | '[' | '{'))
    {
        let indent_len = opening.len() - opening.trim_start_matches([' ', '\t']).len();
        let indent = &opening[..indent_len];
        let step = if indent.contains('\t') { "\t" } else { "    " };
        let (_, close) = pair(open)?;
        if middle == format!("{indent}{step}")
            && closing == format!("{indent}{close}")
            && input_ime::source_input_context(buffer, buffer.line_end(line - 1), cache)
                == SourceInputContext::Code
        {
            return Some(replacement(
                buffer.line_end(line - 1)..buffer.line_start(line + 1) + indent.len(),
                "",
            ));
        }
    }
    let (continuation, closer, start, html) =
        if opening.trim_start_matches([' ', '\t']).starts_with('<') {
            let body = opening.trim_start_matches([' ', '\t']);
            let indent = &opening[..opening.len() - body.len()];
            (
                indent.to_string(),
                nonvoid_html_close(body)?,
                buffer.line_start(line - 1) + indent.len(),
                true,
            )
        } else {
            let marker = opening.find(['`', '~', '$', '\\'])?;
            let prefix = container_prefix(&opening[..marker])?;
            let body = &opening[marker..];
            let first = body.as_bytes()[0];
            let closer = match first {
                b'`' | b'~' => {
                    let run = body.bytes().take_while(|b| *b == first).count();
                    let info = body[run..].trim();
                    if run < 3
                        || !info.chars().all(|c| {
                            c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '+' | '.' | '#')
                        })
                    {
                        return None;
                    }
                    body[..run].to_string()
                }
                b'$' if body == "$$" => "$$".into(),
                b'\\' if body == "\\[" => "\\]".into(),
                _ => return None,
            };
            (prefix, closer, buffer.line_start(line - 1) + marker, false)
        };
    if middle != continuation {
        return None;
    }
    let expected = format!("{continuation}{closer}");
    if if html {
        !closing.eq_ignore_ascii_case(&expected)
    } else {
        closing != expected
    } {
        return None;
    }
    let allowed = if html {
        tag_rollback_allowed(buffer, start, cache)
    } else {
        input_ime::source_input_context_before(buffer, start, cache) == SourceInputContext::Markdown
    };
    if !allowed || escaped(buffer.text(), start) {
        return None;
    }
    Some(replacement(
        buffer.line_end(line - 1)..buffer.line_start(line + 1) + continuation.len(),
        "",
    ))
}

#[derive(Clone)]
struct Change {
    range: Range<usize>,
    text: String,
}

fn unchanged(buffer: &Buffer, edit: &SourceCursorEdit) -> bool {
    buffer.text().get(edit.range.clone()) == Some(edit.text.as_str())
}

fn own_boundary(buffer: &Buffer, edit: &SourceCursorEdit, offset: usize) -> bool {
    let Some(end) = edit.range.start.checked_add(edit.text.len()) else {
        return false;
    };
    if offset < edit.range.start {
        buffer.text().is_char_boundary(offset)
    } else if offset <= end {
        edit.text.is_char_boundary(offset - edit.range.start)
    } else {
        offset
            .checked_sub(edit.text.len())
            .and_then(|p| p.checked_add(edit.range.len()))
            .is_some_and(|p| p <= buffer.len() && buffer.text().is_char_boundary(p))
    }
}

// Validate each plan independently, then sort and resolve only adjacent changes.
// No pairwise target scan; source/replacement byte comparisons are separate from
// the O(n log n) range normalization cost.
fn changes(buffer: &Buffer, plans: &[(SourceCursorEdit, bool)]) -> Option<Vec<Change>> {
    for (edit, _) in plans {
        if edit.range.start > edit.range.end
            || edit.range.end > buffer.len()
            || !buffer.text().is_char_boundary(edit.range.start)
            || !buffer.text().is_char_boundary(edit.range.end)
            || !own_boundary(buffer, edit, edit.cursor.offset)
            || !own_boundary(buffer, edit, edit.cursor.anchor)
        {
            return None;
        }
    }
    let mut changes: Vec<_> = plans
        .iter()
        .filter(|(e, _)| !unchanged(buffer, e))
        .map(|(e, _)| Change {
            range: e.range.clone(),
            text: e.text.clone(),
        })
        .collect();
    changes.sort_by_key(|e| (e.range.start, e.range.end));
    let mut result: Vec<Change> = Vec::new();
    for change in changes {
        if let Some(last) = result.last_mut() {
            if last.range == change.range && last.text == change.text {
                continue;
            }
            if last.text.is_empty()
                && change.text.is_empty()
                && change.range.start <= last.range.end
            {
                last.range.end = last.range.end.max(change.range.end);
                continue;
            }
            // Insertion at a replacement's start or inside it is ambiguous;
            // adjacent edits, including insertion at its end, are independent.
            if last.range.end > change.range.start || last.range.start == change.range.start {
                return None;
            }
        }
        result.push(change);
    }
    let mut len = buffer.len();
    for change in &result {
        len = len
            .checked_sub(change.range.len())?
            .checked_add(change.text.len())?;
    }
    Some(result)
}

/// The parent may validate before invalidating its syntax cache. For one plan it
/// can instead transfer the incremental cache using the exposed range/text.
/// `apply` repeats this validation and never writes on failure. All plans must
/// still refer to the same original buffer when either function is called.
pub(super) fn validate(buffer: &Buffer, plans: &[(SourceCursorEdit, bool)]) -> bool {
    changes(buffer, plans).is_some()
}

/// Cumulative edit effects, stored as projected starts to avoid signed-delta
/// overflow. Normalized changes have strictly increasing, nonoverlapping starts.
/// Build once in O(m); each anchor then needs at most one O(log m) lookup.
struct RemapIndex<'a> {
    changes: &'a [Change],
    mapped_starts: Vec<usize>,
    #[cfg(test)]
    comparisons: std::cell::Cell<usize>,
}

impl<'a> RemapIndex<'a> {
    fn new(changes: &'a [Change]) -> Self {
        let mut mapped_starts = Vec::with_capacity(changes.len());
        let mut original_end = 0;
        let mut mapped_end = 0;
        for change in changes {
            let start = mapped_end + (change.range.start - original_end);
            mapped_starts.push(start);
            original_end = change.range.end;
            mapped_end = start + change.text.len();
        }
        Self {
            changes,
            mapped_starts,
            #[cfg(test)]
            comparisons: std::cell::Cell::new(0),
        }
    }

    fn partition(&self, offset: usize, include_equal: bool) -> usize {
        self.changes.partition_point(|change| {
            #[cfg(test)]
            self.comparisons.set(self.comparisons.get() + 1);
            if include_equal {
                change.range.start <= offset
            } else {
                change.range.start < offset
            }
        })
    }

    /// Original-buffer positions have right affinity, including insertions at a
    /// previous replacement's end. Positions inside deletions map to the union's
    /// start; positions inside other replacements map to their inserted end.
    fn remap(&self, offset: usize) -> usize {
        let count = self.partition(offset, true);
        if count == 0 {
            return offset;
        }
        let index = count - 1;
        let change = &self.changes[index];
        self.mapped_starts[index] + change.text.len() + offset.saturating_sub(change.range.end)
    }

    /// Only called for an actual nonempty replacement in the normalized batch.
    /// Duplicates share this start; validation rejects any conflicting same-start
    /// change. Own anchors must stay inside it rather than acquire right affinity.
    fn replacement_start(&self, start: usize) -> usize {
        let index = self.partition(start, false);
        debug_assert_eq!(self.changes[index].range.start, start);
        self.mapped_starts[index]
    }
}

fn remap_cursor(buffer: &Buffer, edit: &SourceCursorEdit, index: &RemapIndex<'_>) -> Cursor {
    let no_op = unchanged(buffer, edit);
    let inserted_end = edit.range.start + edit.text.len();
    let own_start = if no_op {
        None
    } else if edit.text.is_empty() {
        Some(index.remap(edit.range.start))
    } else {
        Some(index.replacement_start(edit.range.start))
    };
    let map = |offset: usize| {
        if no_op {
            return index.remap(offset);
        }
        if (edit.range.start..=inserted_end).contains(&offset) {
            return own_start.unwrap() + (offset - edit.range.start);
        }
        let original = if offset < edit.range.start {
            offset
        } else {
            offset - edit.text.len() + edit.range.len()
        };
        index.remap(original)
    };
    Cursor {
        offset: map(edit.cursor.offset),
        anchor: map(edit.cursor.anchor),
    }
}

/// Atomic with respect to rejected plans: validation and cursor remapping finish
/// before the first mutation. Duplicate mutations/no-ops and overlapping pure
/// deletions coalesce, but every input cursor and its primary flag are returned.
/// Callers must invalidate their context cache BEFORE these raw mutations.
pub(super) fn apply(
    buffer: &mut Buffer,
    plans: Vec<(SourceCursorEdit, bool)>,
) -> Option<Vec<(Cursor, bool)>> {
    let changes = changes(buffer, &plans)?;
    let index = RemapIndex::new(&changes);
    let cursors = plans
        .iter()
        .map(|(edit, primary)| (remap_cursor(buffer, edit, &index), *primary))
        .collect();
    for change in changes.iter().rev() {
        buffer.replace(change.range.clone(), &change.text);
    }
    Some(cursors)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marked(source: &str) -> (Buffer, Cursor) {
        let offset = source.find('¦').unwrap();
        (
            Buffer::from_text(&source.replacen('¦', "", 1)),
            caret(offset),
        )
    }

    fn step(
        buffer: &mut Buffer,
        cursor: Cursor,
        action: SourceAction<'_>,
        cache: &mut SourceContextCache,
    ) -> Cursor {
        let edit = plan(buffer, cursor, action, cache);
        assert!(validate(buffer, &[(edit.clone(), true)]));
        cache.invalidate_from(buffer.offset_to_line_col(edit.range.start).0);
        apply(buffer, vec![(edit, true)]).unwrap()[0].0
    }

    fn run(source: &str, action: SourceAction<'_>, expected: &str) {
        let (mut buffer, cursor) = marked(source);
        let cursor = step(
            &mut buffer,
            cursor,
            action,
            &mut SourceContextCache::default(),
        );
        let (expected, expected_cursor) = marked(expected);
        assert_eq!(buffer.text(), expected.text(), "source: {source:?}");
        assert_eq!(cursor, expected_cursor, "source: {source:?}");
    }

    fn sequence(inputs: &[&str], expected: &str) {
        let mut buffer = Buffer::new();
        let mut cursor = caret(0);
        let mut cache = SourceContextCache::default();
        for input in inputs {
            cursor = step(&mut buffer, cursor, SourceAction::Insert(input), &mut cache);
        }
        let (expected, at) = marked(expected);
        assert_eq!(buffer.text(), expected.text());
        assert_eq!(cursor, at);
    }

    #[test]
    fn typing_pairs_wiki_images_and_footnotes() {
        sequence(&["[", "[", "文", "]", "]"], "[[文]]¦");
        sequence(&["!", "[", "[", "🙂", "]", "]"], "![[🙂]]¦");
        sequence(&["!", "[", "x", "]", "(", "url", ")"], "![x](url)¦");
        sequence(&["[", "^", "n", "]", ":", " "], "[^n]: ¦");
        sequence(&["「", "文", "」"], "「文」¦");
        run("¦", SourceAction::Insert("**"), "**¦");
        run("x¦", SourceAction::Insert("|"), "x|¦");
        run("¦", SourceAction::Insert(":"), ":¦");
    }

    #[test]
    fn emphasis_upgrades_and_list_markers() {
        sequence(&["*", "*", "*", "text", "*", "*", "*"], "***text***¦");
        sequence(&["_", "_", "text", "_", "_"], "__text__¦");
        sequence(&["~", "~", "text", "~", "~"], "~~text~~¦");
        sequence(&["+", "+", "text", "+", "+"], "++text++¦");
        sequence(&["=", "=", "text", "=", "="], "==text==¦");
        sequence(&["*", " "], "* ¦");
        sequence(&["+", " "], "+ ¦");
        run("> *¦*", SourceAction::Insert(" "), "> * ¦");
        run("paragraph\n=¦", SourceAction::Insert(" "), "paragraph\n= ¦");
        run("¦", SourceAction::Insert("^"), "^¦^");
    }

    #[test]
    fn wrapping_is_literal_and_preserves_selection_direction() {
        for input in ["*", "_", "~", "`", "$", "^", "[", "「", "+", "="] {
            for reverse in [false, true] {
                let mut buffer = Buffer::from_text("a **文🙂** z");
                let range = 2.."a **文🙂**".len();
                let cursor = if reverse {
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
                let edit = plan(
                    &buffer,
                    cursor,
                    SourceAction::Insert(input),
                    &mut SourceContextCache::default(),
                );
                let out = apply(&mut buffer, vec![(edit, true)]).unwrap()[0].0;
                assert_eq!(&buffer.text()[out.selection_range()], "**文🙂**");
                assert_eq!(out.offset < out.anchor, reverse);
                assert!(buffer.text().contains("**文🙂**"));
            }
        }
        let buffer = Buffer::from_text("word");
        let cursor = Cursor {
            offset: 0,
            anchor: 4,
        };
        let edit = plan(
            &buffer,
            cursor,
            SourceAction::Insert("**"),
            &mut SourceContextCache::default(),
        );
        assert_eq!(edit.text, "**");
        assert_eq!(edit.cursor, caret(2));
    }

    #[test]
    fn escapes_words_contractions_and_unmatched_closers() {
        run("can¦t", SourceAction::Insert("'"), "can'¦t");
        run("can¦'t", SourceAction::Insert("'"), "can'¦'t");
        run("foo¦bar", SourceAction::Insert("_"), "foo_¦bar");
        run("foo¦", SourceAction::Insert("*"), "foo*¦");
        run("'word¦'", SourceAction::Insert("'"), "'word'¦");
        run("¦)", SourceAction::Insert(")"), ")¦)");
        run("(word)¦)", SourceAction::Insert(")"), "(word))¦)");
        run("(文¦)", SourceAction::Insert(")"), "(文)¦");
        run("\"(\" ¦)", SourceAction::Insert(")"), "\"(\" )¦)");
        run("\\¦", SourceAction::Insert("*"), "\\*¦");
        run("\\\\ ¦", SourceAction::Insert("*"), "\\\\ *¦*");
        run("\\\\¦", SourceAction::Insert("("), "\\\\(¦)");
        run("\\¦", SourceAction::Insert("\""), "\\\"¦");
        run("\\\\ ¦", SourceAction::Insert("\""), "\\\\ \"¦\"");
    }

    #[test]
    fn literal_regions_and_urls_do_not_gain_markdown_assistance() {
        for source in [
            "```rust\n¦\n```",
            "~~~\n¦\n~~~",
            "    ¦",
            "`x ¦ y`",
            "$x ¦ y$",
            "$$\n¦\n$$",
            "---\nvalue: ¦\n---",
            "---\nvalue: |\n  ¦\n---",
            "<div>\n¦\n</div>",
            "<code>x ¦ y</code>",
            "[x](https://host/¦)",
            "<https://host/¦>",
            "https://host/¦",
            "[x]: https://host/¦",
        ] {
            for input in ["*", "_", "~", "+", "=", "`", "$", "^"] {
                let expected = source.replace('¦', &format!("{input}¦"));
                run(source, SourceAction::Insert(input), &expected);
            }
        }
        run("```\n¦\n```", SourceAction::Insert("("), "```\n(¦)\n```");
        run("`x` ¦", SourceAction::Insert("*"), "`x` *¦*");
        run("$x$ ¦", SourceAction::Insert("*"), "$x$ *¦*");
        run(
            "```\nx\n```\n¦",
            SourceAction::Insert("*"),
            "```\nx\n```\n*¦*",
        );
        run(
            "---\nx: 1\n---\n¦",
            SourceAction::Insert("*"),
            "---\nx: 1\n---\n*¦*",
        );
    }

    #[test]
    fn same_line_fences_upgrade_and_expand_without_identifier_completion() {
        sequence(&["`", "`", "`", "rust"], "```rust¦```");
        sequence(&["`", "`", "`", "`"], "````¦````");
        sequence(&["$", "$"], "$$¦$$");
        sequence(&["\\", "["], "\\[¦\\]");
        run("```rust¦```", SourceAction::Enter, "```rust\n¦\n```");
        run("````¦````", SourceAction::Enter, "````\n¦\n````");
        run("$$¦$$", SourceAction::Enter, "$$\n¦\n$$");
        run("\\[¦\\]", SourceAction::Enter, "\\[\n¦\n\\]");
        run("> ```rs¦```", SourceAction::Enter, "> ```rs\n> ¦\n> ```");
        run("- ```rs¦```", SourceAction::Enter, "- ```rs\n  ¦\n  ```");
        run(
            "```\n```rust¦```\n```",
            SourceAction::Enter,
            "```\n```rust\n¦```\n```",
        );
        run("`code¦`", SourceAction::Insert("`"), "`code`¦");
        run("```rs¦```", SourceAction::Insert("t"), "```rst¦```");
    }

    #[test]
    fn enter_and_backspace_delegate_source_structure_not_tables() {
        run("- item¦", SourceAction::Enter, "- item\n- ¦");
        run("- ¦", SourceAction::Backspace, "¦");
        run("# ¦title", SourceAction::Backspace, "¦title");
        run("```rs¦", SourceAction::Enter, "```rs\n¦\n```");
        run("$$¦", SourceAction::Enter, "$$\n¦\n$$");
        run("| a | b ¦|", SourceAction::Insert("|"), "| a | b |¦|");
        run("| a | b |¦", SourceAction::Enter, "| a | b |\n¦");
    }

    #[test]
    fn empty_pair_deletion_is_bidirectional_and_stepwise() {
        for action in [SourceAction::Backspace, SourceAction::Delete] {
            run("(¦)", action, "¦");
            run("「¦」", action, "¦");
            run("***¦***", action, "**¦**");
            run("**¦**", action, "*¦*");
            run("*¦*", action, "¦");
            run("``¦``", action, "`¦`");
            run("$$¦$$", action, "$¦$");
            run("[[¦]]", action, "¦");
            run("\\[¦\\]", action, "¦");
        }
        run("¦()", SourceAction::Delete, "¦");
        run("¦「」", SourceAction::Delete, "¦");
        run("¦******", SourceAction::Delete, "¦****");
        run("¦\\[\\]", SourceAction::Delete, "¦");
        run("(¦text)", SourceAction::Backspace, "¦text)");
        run("(¦text)", SourceAction::Delete, "(¦ext)");
        run("**¦text**", SourceAction::Backspace, "*¦text**");
        run("```\n*¦*\n```", SourceAction::Backspace, "```\n¦*\n```");
        run("\\(¦)", SourceAction::Backspace, "\\¦)");
        run("文e\u{301}¦🙂", SourceAction::Backspace, "文¦🙂");
        run("文¦👨‍👩‍👧‍👦x", SourceAction::Delete, "文¦x");
    }

    #[test]
    fn html_completion_is_conservative_and_does_not_repeat_closes() {
        run("<div¦", SourceAction::Insert(">"), "<div>¦</div>");
        run(
            "text <span class=\"x\"¦",
            SourceAction::Insert(">"),
            "text <span class=\"x\">¦</span>",
        );
        run(
            "<DIV data-x='>'¦",
            SourceAction::Insert(">"),
            "<DIV data-x='>'>¦</DIV>",
        );
        run("<div¦</DIV>", SourceAction::Insert(">"), "<div>¦</DIV>");
        run("<div¦></div>", SourceAction::Insert(">"), "<div>¦</div>");
        for source in [
            "<br¦",
            "<img src=x¦",
            "<div /¦",
            "<span/¦",
            "<https://host¦",
            "<a@b.test¦",
            "a < b¦",
            "<unknown¦",
            "</div¦",
            "<!-- x¦",
            "<div title=\"x¦",
            "`<div¦`",
            "```\n<div¦\n```",
            "---\nx: <div¦\n---",
        ] {
            run(
                source,
                SourceAction::Insert(">"),
                &source.replace('¦', ">¦"),
            );
        }
    }

    #[test]
    fn html_assistance_rejects_comments_raw_text_and_attribute_values() {
        for source in [
            "<!-- <u¦ -->",
            "<!--\n<u¦\n-->",
            "<!-- <u¦> -->",
            "<?proc <u¦ ?>",
            "<![CDATA[<u¦]]>",
            "<script>const s=\"<u¦</u>\";</script>",
            "<script>\nconst s=\"<u¦</u>\";\n</script>",
            "<style>.x::after { content: '<u¦</u>'; }</style>",
            "<pre><u¦</u></pre>",
            "<code><u¦</u></code>",
            "<textarea><u¦</u></textarea>",
            "<div title=\"<u¦</u>\">body</div>",
            "<div title=\"quoted > <u¦\">body</div>",
            "<div\n title=\"<u¦\"></div>",
        ] {
            run(
                source,
                SourceAction::Insert(">"),
                &source.replacen('¦', ">¦", 1),
            );
        }
        for source in [
            "<!-- <u>¦</u> -->",
            "<?proc <u>¦</u> ?>",
            "<![CDATA[<u>¦</u>]]>",
            "<script>const s=\"<u>¦</u>\";</script>",
            "<script>\nconst s=\"<u>¦</u>\";\n</script>",
            "<style>.x::after { content: '<u>¦</u>'; }</style>",
            "<pre><u>¦</u></pre>",
            "<code><u>¦</u></code>",
            "<textarea><u>¦</u></textarea>",
            "<div title=\"<u>¦</u>\">body</div>",
            "<div\n title=\"<u>¦</u>\"></div>",
        ] {
            run(
                source,
                SourceAction::Backspace,
                &source.replacen(">¦", "¦", 1),
            );
            run(source, SourceAction::Delete, &source.replacen("¦<", "¦", 1));
        }
        run(
            "<!-- ¦<u></u> -->",
            SourceAction::Delete,
            "<!-- ¦u></u> -->",
        );
        for source in [
            "<!--\n<u>\n¦\n</u>\n-->",
            "<script>\n<u>\n¦\n</u>\n</script>",
        ] {
            let (buffer, cursor) = marked(source);
            assert!(
                empty_block_rollback(&buffer, cursor.offset, &mut SourceContextCache::default())
                    .is_none()
            );
        }
        run(
            "<div>\n<u¦\n</div>",
            SourceAction::Insert(">"),
            "<div>\n<u>¦</u>\n</div>",
        );
        run(
            "<div><u>¦</u></div>",
            SourceAction::Backspace,
            "<div>¦</div>",
        );
    }

    #[test]
    fn existing_html_opening_angles_only_skip_without_synthesizing_closers() {
        for source in [
            "<div¦>\nbody\n</div>",
            "<div¦>body</div>",
            "<div¦>",
            "<DIV class='文'¦>\nbody\n</DIV>",
            "<div>\n  <u¦>\n    body\n  </u>\n</div>",
            "see <span¦>body</span>",
            "<div /¦>",
            "<br¦>",
        ] {
            let (mut buffer, cursor) = marked(source);
            let mut cache = SourceContextCache::default();
            let edit = plan(&buffer, cursor, SourceAction::Insert(">"), &mut cache);
            assert_eq!(edit.range, cursor.offset..cursor.offset, "{source:?}");
            assert!(edit.text.is_empty(), "{source:?}");
            assert_eq!(edit.cursor, caret(cursor.offset + 1), "{source:?}");
            buffer.begin_edit_recording();
            let cursor = step(&mut buffer, cursor, SourceAction::Insert(">"), &mut cache);
            let (expected, expected_cursor) = marked(&source.replacen("¦>", ">¦", 1));
            assert_eq!(buffer.text(), expected.text());
            assert_eq!(cursor, expected_cursor);
            assert!(buffer.finish_edit_recording().is_empty());
        }
    }

    #[test]
    fn closer_scans_do_not_cross_adjacent_literal_block_boundaries() {
        for fence in ["~~~", "```"] {
            let source = format!("{fence}\n(\n{fence}\n{fence}\n¦)\n{fence}");
            run(
                &source,
                SourceAction::Insert(")"),
                &source.replacen('¦', ")¦", 1),
            );
            let source = format!("{fence}\n(\n{fence}\n{fence}\n(\nx¦)\n{fence}");
            run(
                &source,
                SourceAction::Insert(")"),
                &source.replacen("¦)", ")¦", 1),
            );
            let source = format!("{fence}language(\n¦)\n{fence}");
            run(
                &source,
                SourceAction::Insert(")"),
                &source.replacen('¦', ")¦", 1),
            );
        }
        for source in [
            "$$\n(\n$$\n$$\n¦)\n$$",
            "\\[\n(\n\\]\n\\[\n¦)\n\\]",
            "<pre>\n(\n</pre>\n<pre>\n¦)\n</pre>",
        ] {
            run(
                source,
                SourceAction::Insert(")"),
                &source.replacen('¦', ")¦", 1),
            );
        }
    }

    #[test]
    fn tilde_fences_upgrade_expand_and_roll_back_without_breaking_strikethrough() {
        sequence(&["~"], "~¦~");
        sequence(&["~", "~"], "~~¦~~");
        sequence(&["~", "~", "~"], "~~~¦~~~");
        sequence(&["~", "~", "~", "~", "rust"], "~~~~rust¦~~~~");
        sequence(&["~", "~", "word", "~", "~"], "~~word~~¦");
        run("~~~rust¦~~~", SourceAction::Enter, "~~~rust\n¦\n~~~");
        run("~~~~¦~~~~", SourceAction::Enter, "~~~~\n¦\n~~~~");
        run("> ~~~rs¦~~~", SourceAction::Enter, "> ~~~rs\n> ¦\n> ~~~");
        run("- ~~~rs¦~~~", SourceAction::Enter, "- ~~~rs\n  ¦\n  ~~~");
        run("~~~rust¦", SourceAction::Enter, "~~~rust\n¦\n~~~");
        // Two literal tildes, or a two-per-side pair, are not yet paired fences.
        run("~~¦", SourceAction::Enter, "~~\n¦");
        run("~~¦~~", SourceAction::Enter, "~~\n¦~~");
        run("~~¦", SourceAction::Insert("~"), "~~~¦");
        run("~~word¦~~", SourceAction::Insert("~"), "~~word~¦~");
        run("~~~¦~~~", SourceAction::Backspace, "~~¦~~");
        run("~~~~¦~~~~", SourceAction::Delete, "~~~¦~~~");
        for source in [
            "```\n~~¦~~\n```",
            "~~~\n~~¦~~\n~~~",
            "`~~¦~~`",
            "$~~¦~~$",
            "---\nx: ~~¦~~\n---",
            "<code>~~¦~~</code>",
        ] {
            run(
                source,
                SourceAction::Insert("~"),
                &source.replacen('¦', "~¦", 1),
            );
        }
        for language in ["rust", "c++", "c#", "foo.bar", "objective-c"] {
            run(
                &format!("~~~{language}¦~~~"),
                SourceAction::Enter,
                &format!("~~~{language}\n¦\n~~~"),
            );
        }
        for language in ["ru st", "rüst", "rust extra", "'rust'"] {
            let (buffer, cursor) = marked(&format!("~~~{language}¦~~~"));
            assert!(paired_enter(&buffer, cursor, &mut SourceContextCache::default()).is_none());
        }
        let mut buffer = Buffer::new();
        let mut cursor = caret(0);
        let mut cache = SourceContextCache::default();
        for input in ["~", "~", "~"] {
            cursor = step(&mut buffer, cursor, SourceAction::Insert(input), &mut cache);
        }
        cursor = step(&mut buffer, cursor, SourceAction::Enter, &mut cache);
        assert_eq!(buffer.text(), "~~~\n\n~~~");
        cursor = step(&mut buffer, cursor, SourceAction::Backspace, &mut cache);
        assert_eq!(buffer.text(), "~~~~~~");
        assert_eq!(cursor, caret(3));
        cursor = step(&mut buffer, cursor, SourceAction::Backspace, &mut cache);
        assert_eq!(buffer.text(), "~~~~");
        assert_eq!(cursor, caret(2));
    }

    #[test]
    fn multicursor_wraps_and_shifted_skip_cursors_preserve_both_anchors() {
        let mut buffer = Buffer::from_text("文 (x) tail");
        let mut cache = SourceContextCache::default();
        let first = plan(&buffer, caret(0), SourceAction::Insert("🙂"), &mut cache);
        let skip = plan(
            &buffer,
            caret("文 (x".len()),
            SourceAction::Insert(")"),
            &mut cache,
        );
        let selected = Cursor {
            offset: "文 (x) ".len(),
            anchor: buffer.len(),
        };
        let wrap = plan(&buffer, selected, SourceAction::Insert("["), &mut cache);
        let cursors = apply(
            &mut buffer,
            vec![(skip, false), (wrap, true), (first, false)],
        )
        .unwrap();
        assert_eq!(buffer.text(), "🙂文 (x) [tail]");
        assert_eq!(cursors[0], (caret("🙂文 (x)".len()), false));
        assert_eq!(&buffer.text()[cursors[1].0.selection_range()], "tail");
        assert!(cursors[1].0.offset < cursors[1].0.anchor);
        assert!(cursors[1].1);
        assert_eq!(cursors[2].0, caret("🙂".len()));
    }

    #[test]
    fn overlapping_deletions_duplicates_and_noops_coalesce() {
        let mut buffer = Buffer::from_text("abcdef🙂");
        buffer.begin_edit_recording();
        let plans = vec![
            (replacement(1..4, ""), true),
            (replacement(3..6, ""), false),
            (replacement(1..4, ""), false),
            (
                SourceCursorEdit {
                    range: 2..2,
                    text: String::new(),
                    cursor: Cursor {
                        offset: buffer.len(),
                        anchor: 0,
                    },
                },
                false,
            ),
        ];
        assert!(validate(&buffer, &plans));
        let cursors = apply(&mut buffer, plans).unwrap();
        assert_eq!(buffer.text(), "a🙂");
        assert_eq!(buffer.finish_edit_recording().len(), 1);
        assert_eq!(cursors[0].0, caret(1));
        assert_eq!(cursors[1].0, caret(1));
        assert_eq!(
            cursors[3].0,
            Cursor {
                offset: "a🙂".len(),
                anchor: 0
            }
        );
        let mut buffer = Buffer::from_text("abc");
        let edit = replacement(1..1, "()");
        apply(&mut buffer, vec![(edit.clone(), false), (edit, true)]).unwrap();
        assert_eq!(buffer.text(), "a()bc");
    }

    #[test]
    fn boundaries_do_not_leak_across_paragraphs_inline_literals_or_links() {
        run("\\\\¦", SourceAction::Insert("*"), "\\\\*¦*");
        run("\\\\\\¦", SourceAction::Insert("*"), "\\\\\\*¦");
        run("文，¦", SourceAction::Insert("*"), "文，*¦*");
        run(
            "[label ¦](https://host/a)",
            SourceAction::Insert("*"),
            "[label *¦*](https://host/a)",
        );
        run(
            "[x](https://host/a)¦",
            SourceAction::Insert("*"),
            "[x](https://host/a)*¦*",
        );
        run(
            "https://host/a(b/¦)",
            SourceAction::Insert("*"),
            "https://host/a(b/*¦)",
        );
        run(
            "(unmatched\n\n¦)",
            SourceAction::Insert(")"),
            "(unmatched\n\n)¦)",
        );
        run("`(` ¦)", SourceAction::Insert(")"), "`(` )¦)");
        run(
            "```\n(\n```\n\n¦)",
            SourceAction::Insert(")"),
            "```\n(\n```\n\n)¦)",
        );
        run("(word\n¦)", SourceAction::Insert(")"), "(word\n)¦");
        run(
            "```\n(\n¦)\n```",
            SourceAction::Insert(")"),
            "```\n(\n)¦\n```",
        );
        run(
            "*word `*` tail¦*",
            SourceAction::Insert("*"),
            "*word `*` tail*¦",
        );
        sequence(&["^", "^"], "^^¦");
        sequence(&["*", "*", "*", "*", "*", "*"], "******¦");
        sequence(&["+", "+", "+", "+"], "++++¦");
        sequence(&["$", "$", "$", "$"], "$$$$¦");
        run("<br¦>", SourceAction::Insert(">"), "<br>¦");
        run("<div /¦>", SourceAction::Insert(">"), "<div />¦");
        run("¦**text**", SourceAction::Delete, "¦*text**");
        run("*¦*text**", SourceAction::Backspace, "¦*text**");
        run("¦**(text)**", SourceAction::Delete, "¦*(text)**");
        run("¦``code``", SourceAction::Delete, "¦`code``");
        run("*¦*text**", SourceAction::Insert("*"), "**¦*text**");
        sequence(&["\\", "[", "x", "\\", "]"], "\\[x\\]¦");
        sequence(&["\\", "(", "x", "\\", ")"], "\\(x\\)¦");
        run("\\[\nx\\¦]\n", SourceAction::Insert("]"), "\\[\nx\\]¦\n");
        run("\\¦]", SourceAction::Insert("]"), "\\]¦]");
        run("a`code¦`word", SourceAction::Insert("`"), "a`code`¦word");
    }

    #[test]
    fn adjacent_edits_pin_own_cursors_and_remap_external_anchors() {
        let mut buffer = Buffer::from_text("abcd");
        let deletion = replacement(0..1, "");
        let insert = replacement(1..1, "🙂");
        let outside = SourceCursorEdit {
            range: 3..4,
            text: "Z".into(),
            cursor: Cursor {
                offset: 2,
                anchor: 4,
            },
        };
        let cursors = apply(
            &mut buffer,
            vec![(deletion, true), (insert, false), (outside, false)],
        )
        .unwrap();
        assert_eq!(buffer.text(), "🙂bcZ");
        assert_eq!(cursors[0].0, caret(0));
        assert_eq!(cursors[1].0, caret(4));
        assert_eq!(
            cursors[2].0,
            Cursor {
                offset: 5,
                anchor: 7
            }
        );
        let mut buffer = Buffer::from_text("abcdef");
        let mut cache = SourceContextCache::default();
        let a = plan(
            &buffer,
            Cursor {
                offset: 1,
                anchor: 4,
            },
            SourceAction::Insert("*"),
            &mut cache,
        );
        let b = plan(
            &buffer,
            Cursor {
                offset: 3,
                anchor: 5,
            },
            SourceAction::Insert("*"),
            &mut cache,
        );
        buffer.begin_edit_recording();
        assert!(!validate(&buffer, &[(a.clone(), true), (b.clone(), false)]));
        assert!(apply(&mut buffer, vec![(a, true), (b, false)]).is_none());
        assert!(buffer.finish_edit_recording().is_empty());
        assert_eq!(buffer.text(), "abcdef");
    }

    #[test]
    fn large_batch_remaps_own_and_skip_anchors_with_logarithmic_queries() {
        let count = 10_000usize;
        let source = "x\n".repeat(count);
        let mut buffer = Buffer::from_text(&source);
        let mut plans = Vec::with_capacity(count * 2);
        let mut expected = Vec::with_capacity(count * 2);
        let map_original = |offset: usize| {
            if offset == source.len() {
                count * 5
            } else {
                offset / 2 * 5 + 4
            }
        };
        for i in 0..count {
            let start = i * 2;
            let primary = i == count / 2;
            let (offset, anchor) = if i % 2 == 0 { (3, 4) } else { (4, 3) };
            plans.push((
                SourceCursorEdit {
                    range: start..start + 1,
                    text: "文x".into(),
                    cursor: Cursor {
                        offset: start + offset,
                        anchor: start + anchor,
                    },
                },
                primary,
            ));
            expected.push((
                Cursor {
                    offset: i * 5 + offset,
                    anchor: i * 5 + anchor,
                },
                primary,
            ));
            let (offset, anchor) = if i % 2 == 0 {
                (start + 2, start + 1)
            } else {
                (start + 1, start + 2)
            };
            plans.push((
                SourceCursorEdit {
                    range: start..start,
                    text: String::new(),
                    cursor: Cursor { offset, anchor },
                },
                false,
            ));
            expected.push((
                Cursor {
                    offset: map_original(offset),
                    anchor: map_original(anchor),
                },
                false,
            ));
        }
        plans.reverse();
        expected.reverse();
        assert!(validate(&buffer, &plans));
        let normalized = changes(&buffer, &plans).unwrap();
        assert_eq!(normalized.len(), count);
        let index = RemapIndex::new(&normalized);
        for ((edit, primary), expected) in plans.iter().zip(&expected) {
            assert_eq!((remap_cursor(&buffer, edit, &index), *primary), *expected);
        }
        let comparisons = index.comparisons.get();
        let per_lookup = (usize::BITS - count.leading_zeros()) as usize + 1;
        // One own-start lookup per replacement, two endpoint lookups per skip.
        assert!(comparisons >= count * 3);
        assert!(
            comparisons <= count * 3 * per_lookup,
            "{comparisons} comparisons"
        );
        assert_eq!(buffer.text(), source);
        buffer.begin_edit_recording();
        let cursors = apply(&mut buffer, plans).unwrap();
        assert_eq!(cursors, expected);
        assert_eq!(buffer.text(), "文x\n".repeat(count));
        assert_eq!(buffer.finish_edit_recording().len(), count);

        // Same-start destructive plans must still coalesce in one sorted pass.
        let mut buffer = Buffer::from_text(&"x".repeat(count));
        let plans: Vec<_> = (1..=count)
            .rev()
            .map(|end| (replacement(0..end, ""), end == count))
            .collect();
        assert!(validate(&buffer, &plans));
        buffer.begin_edit_recording();
        let cursors = apply(&mut buffer, plans).unwrap();
        assert_eq!(buffer.text(), "");
        assert_eq!(buffer.finish_edit_recording().len(), 1);
        assert_eq!(cursors.len(), count);
        for (i, (cursor, primary)) in cursors.into_iter().enumerate() {
            assert_eq!(cursor, caret(0));
            assert_eq!(primary, i == 0);
        }
    }

    #[test]
    fn indexed_remapping_matches_linear_semantics_for_all_small_anchor_pairs() {
        fn linear_remap(offset: usize, changes: &[Change]) -> usize {
            let mut mapped = offset;
            for change in changes.iter().rev() {
                if offset >= change.range.end {
                    mapped = mapped - change.range.len() + change.text.len();
                } else if offset >= change.range.start {
                    mapped = change.range.start + change.text.len();
                }
            }
            mapped
        }
        fn linear_cursor(buffer: &Buffer, edit: &SourceCursorEdit, changes: &[Change]) -> Cursor {
            let map = |offset| {
                if unchanged(buffer, edit) {
                    return linear_remap(offset, changes);
                }
                if (edit.range.start..=edit.range.start + edit.text.len()).contains(&offset) {
                    if edit.text.is_empty() {
                        return linear_remap(edit.range.start, changes);
                    }
                    let mut start = edit.range.start;
                    for change in changes
                        .iter()
                        .take_while(|c| c.range.start < edit.range.start)
                    {
                        start = start - change.range.len() + change.text.len();
                    }
                    return start + offset - edit.range.start;
                }
                let original = if offset < edit.range.start {
                    offset
                } else {
                    offset - edit.text.len() + edit.range.len()
                };
                linear_remap(original, changes)
            };
            Cursor {
                offset: map(edit.cursor.offset),
                anchor: map(edit.cursor.anchor),
            }
        }
        let buffer = Buffer::from_text("abcdefghijklmnopqrstuvwxyz");
        let plans = vec![
            (replacement(0..1, "🙂"), true),
            (replacement(3..7, ""), false),
            (replacement(5..9, ""), false),
            (replacement(9..9, "xy"), false),
            (replacement(12..15, "Q"), false),
            (replacement(19..19, "文"), false),
            (replacement(19..19, "文"), false),
            (replacement(10..12, "kl"), false),
            (replacement(0..0, ""), false),
        ];
        assert!(validate(&buffer, &plans));
        let normalized = changes(&buffer, &plans).unwrap();
        let index = RemapIndex::new(&normalized);
        for offset in 0..=buffer.len() {
            assert_eq!(index.remap(offset), linear_remap(offset, &normalized));
        }
        for (mut edit, _) in plans {
            let len = buffer.len() - edit.range.len() + edit.text.len();
            let boundaries: Vec<_> = (0..=len)
                .filter(|offset| own_boundary(&buffer, &edit, *offset))
                .collect();
            for &offset in &boundaries {
                for &anchor in &boundaries {
                    edit.cursor = Cursor { offset, anchor };
                    assert_eq!(
                        remap_cursor(&buffer, &edit, &index),
                        linear_cursor(&buffer, &edit, &normalized),
                        "{edit:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn merged_deletion_own_cursors_and_reversed_anchors_keep_boundary_affinity() {
        let source = "abcdefghijklmn";
        let mut buffer = Buffer::from_text(source);
        let plans = vec![
            (replacement(0..0, "文"), true),
            (
                SourceCursorEdit {
                    range: 0..0,
                    text: "文".into(),
                    cursor: Cursor {
                        offset: 0,
                        anchor: 3,
                    },
                },
                false,
            ),
            (replacement(2..6, ""), false),
            (replacement(4..8, ""), false),
            (replacement(2..6, ""), false),
            (replacement(8..8, "XYZ"), false),
            (
                SourceCursorEdit {
                    range: 10..12,
                    text: "<kl>".into(),
                    cursor: Cursor {
                        offset: 11,
                        anchor: 13,
                    },
                },
                false,
            ),
            (
                SourceCursorEdit {
                    range: 0..source.len(),
                    text: source.into(),
                    cursor: Cursor {
                        offset: 14,
                        anchor: 1,
                    },
                },
                false,
            ),
            (
                SourceCursorEdit {
                    range: 0..0,
                    text: String::new(),
                    cursor: Cursor {
                        offset: 8,
                        anchor: 2,
                    },
                },
                false,
            ),
            (
                SourceCursorEdit {
                    range: 0..0,
                    text: String::new(),
                    cursor: Cursor {
                        offset: 2,
                        anchor: 8,
                    },
                },
                false,
            ),
            (replacement(0..0, ""), false),
        ];
        assert!(validate(&buffer, &plans));
        buffer.begin_edit_recording();
        let cursors = apply(&mut buffer, plans).unwrap();
        assert_eq!(buffer.text(), "文abXYZij<kl>mn");
        assert_eq!(buffer.finish_edit_recording().len(), 4);
        assert_eq!(
            cursors,
            vec![
                (caret(3), true),
                (
                    Cursor {
                        offset: 0,
                        anchor: 3
                    },
                    false
                ),
                (caret(5), false),
                (caret(5), false),
                (caret(5), false),
                (caret(8), false),
                (
                    Cursor {
                        offset: 11,
                        anchor: 13
                    },
                    false
                ),
                (
                    Cursor {
                        offset: 16,
                        anchor: 4
                    },
                    false
                ),
                (
                    Cursor {
                        offset: 8,
                        anchor: 5
                    },
                    false
                ),
                (
                    Cursor {
                        offset: 5,
                        anchor: 8
                    },
                    false
                ),
                (caret(3), false),
            ]
        );
    }

    #[test]
    fn same_start_conflicts_stay_atomic_and_identical_noops_do_not_conflict() {
        for pair in [
            vec![
                (replacement(1..1, "X"), true),
                (replacement(1..1, "Y"), false),
            ],
            vec![
                (replacement(1..1, "X"), true),
                (replacement(1..2, "Y"), false),
            ],
            vec![
                (replacement(1..2, "X"), true),
                (replacement(1..2, ""), false),
            ],
            vec![
                (replacement(0..2, ""), true),
                (replacement(1..1, "X"), false),
            ],
        ] {
            for reverse in [false, true] {
                let mut plans = pair.clone();
                if reverse {
                    plans.reverse();
                }
                plans.push((replacement(4..4, "tail"), false));
                let mut buffer = Buffer::from_text("abcd");
                buffer.begin_edit_recording();
                assert!(!validate(&buffer, &plans));
                assert!(apply(&mut buffer, plans).is_none());
                assert_eq!(buffer.text(), "abcd");
                assert!(buffer.finish_edit_recording().is_empty());
            }
        }
        let plans = vec![
            (
                SourceCursorEdit {
                    range: 1..1,
                    text: "X".into(),
                    cursor: Cursor {
                        offset: 1,
                        anchor: 2,
                    },
                },
                true,
            ),
            (
                SourceCursorEdit {
                    range: 1..2,
                    text: "b".into(),
                    cursor: Cursor {
                        offset: 1,
                        anchor: 2,
                    },
                },
                false,
            ),
        ];
        let mut buffer = Buffer::from_text("abcd");
        assert!(validate(&buffer, &plans));
        let cursors = apply(&mut buffer, plans).unwrap();
        assert_eq!(buffer.text(), "aXbcd");
        assert_eq!(
            cursors,
            vec![
                (
                    Cursor {
                        offset: 1,
                        anchor: 2
                    },
                    true
                ),
                (
                    Cursor {
                        offset: 2,
                        anchor: 3
                    },
                    false
                )
            ]
        );
    }

    #[test]
    fn generated_plans_stay_valid_through_unicode_edit_sequences() {
        let mut seed = 7_u64;
        let inputs = [
            "(", ")", "[", "]", "*", "_", "`", "$", "+", "=", "~", "^", "\\", "\"", "'", " ", "文",
            "🙂", "e\u{301}", "paste**", ">", "<",
        ];
        let mut buffer = Buffer::new();
        let mut cursor = caret(0);
        let mut cache = SourceContextCache::default();
        for _ in 0..400 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let choice = (seed >> 32) as usize;
            if choice % 7 == 0 {
                cursor.offset = buffer.clamp_grapheme_offset(choice % (buffer.len() + 1));
                cursor.anchor = if choice % 3 == 0 {
                    buffer.clamp_grapheme_offset((choice / 7) % (buffer.len() + 1))
                } else {
                    cursor.offset
                };
            }
            let action = match choice % 9 {
                0 => SourceAction::Backspace,
                1 => SourceAction::Delete,
                2 => SourceAction::Enter,
                _ => SourceAction::Insert(inputs[choice % inputs.len()]),
            };
            cursor = step(&mut buffer, cursor, action, &mut cache);
            assert!(cursor.offset <= buffer.len() && buffer.text().is_char_boundary(cursor.offset));
            assert!(cursor.anchor <= buffer.len() && buffer.text().is_char_boundary(cursor.anchor));
        }
    }

    #[test]
    fn empty_html_pairs_roll_back_locally_without_touching_nonempty_content() {
        for action in [SourceAction::Backspace, SourceAction::Delete] {
            run("<u>¦</u>", action, "¦");
            run("前 <SPAN title='文'>¦</span> 后", action, "前 ¦ 后");
            run("<div>\n  <u>¦</u>\n</div>", action, "<div>\n  ¦\n</div>");
        }
        run("¦<u></u>", SourceAction::Delete, "¦");
        run("<u>¦x</u>", SourceAction::Backspace, "<u¦x</u>");
        run("<u>¦x</u>", SourceAction::Delete, "<u>¦</u>");
        run("<u>¦</b>", SourceAction::Backspace, "<u¦</b>");
        run("<u>¦ </u>", SourceAction::Backspace, "<u¦ </u>");
        run("<br>¦</br>", SourceAction::Backspace, "<br¦</br>");
        run("<u/>¦</u>", SourceAction::Backspace, "<u/¦</u>");
        for source in [
            "`<u>¦</u>`",
            "$<u>¦</u>$",
            "```\n<u>¦</u>\n```",
            "---\nx: <u>¦</u>\n---",
            "\\<u>¦</u>",
            "[x](https://host/<u>¦</u>)",
        ] {
            let (buffer, cursor) = marked(source);
            let edit = plan(
                &buffer,
                cursor,
                SourceAction::Backspace,
                &mut SourceContextCache::default(),
            );
            assert_eq!(edit.range, cursor.offset - 1..cursor.offset, "{source:?}");
        }
        let (mut buffer, cursor) = marked("¦");
        let mut cache = SourceContextCache::default();
        let cursor = step(&mut buffer, cursor, SourceAction::Insert("<u"), &mut cache);
        let cursor = step(&mut buffer, cursor, SourceAction::Insert(">"), &mut cache);
        let cursor = step(&mut buffer, cursor, SourceAction::Backspace, &mut cache);
        assert_eq!(buffer.text(), "");
        assert_eq!(cursor, caret(0));
    }

    #[test]
    fn empty_block_enter_backspace_roundtrips_preserve_openers_and_containers() {
        for (source, collapsed) in [
            ("```rust\n¦\n```", "```rust¦```"),
            ("````\n¦\n````", "````¦````"),
            ("~~~rs\n¦\n~~~", "~~~rs¦~~~"),
            ("$$\n¦\n$$", "$$¦$$"),
            ("\\[\n¦\n\\]", "\\[¦\\]"),
            ("> ```rs\n> ¦\n> ```", "> ```rs¦```"),
            ("- ```rs\n  ¦\n  ```", "- ```rs¦```"),
            ("<div class='x'>\n¦\n</div>", "<div class='x'>¦</div>"),
            ("  <u>\n  ¦\n  </u>", "  <u>¦</u>"),
        ] {
            for action in [SourceAction::Backspace, SourceAction::Delete] {
                run(source, action, collapsed);
            }
        }
        run(
            "```rs\nfn f() {\n    ¦\n}\n```",
            SourceAction::Backspace,
            "```rs\nfn f() {¦}\n```",
        );
        run(
            "```rs\n\t{\n\t\t¦\n\t}\n```",
            SourceAction::Backspace,
            "```rs\n\t{¦}\n```",
        );
        let (mut buffer, cursor) = marked("```¦```");
        let mut cache = SourceContextCache::default();
        let cursor = step(&mut buffer, cursor, SourceAction::Enter, &mut cache);
        let cursor = step(&mut buffer, cursor, SourceAction::Backspace, &mut cache);
        assert_eq!(buffer.text(), "``````");
        assert_eq!(cursor, caret(3));
        let cursor = step(&mut buffer, cursor, SourceAction::Backspace, &mut cache);
        assert_eq!(buffer.text(), "````");
        assert_eq!(cursor, caret(2));
        for source in [
            "```rs\ntext¦\n```",
            "```rs\n¦\ntext\n```",
            "```rs\n¦\n````",
            "```\n$$\n¦\n$$\n```",
            "````\n```rs\n¦\n```\n````",
            "---\nx: |\n  ```rs\n  ¦\n  ```\n---",
            "`<u>\n¦\n</u>`",
            "```\n<u>\n¦\n</u>\n```",
            "<u>\n¦ \n</u>",
            "<u>\n¦\n</b>",
        ] {
            let (buffer, cursor) = marked(source);
            assert!(
                empty_block_rollback(&buffer, cursor.offset, &mut SourceContextCache::default())
                    .is_none(),
                "{source:?}"
            );
        }
    }

    #[test]
    fn wiki_empty_pairs_keep_whole_pair_deletion_only_in_markdown() {
        run("![[¦]]", SourceAction::Backspace, "!¦");
        run("¦[[]]", SourceAction::Delete, "¦");
        run("```\n[[¦]]\n```", SourceAction::Backspace, "```\n[¦]\n```");
        run("[[¦text]]", SourceAction::Backspace, "[¦text]]");
    }

    #[test]
    fn selection_crossing_literal_boundary_wraps_only_its_exact_bytes() {
        let source = "left `code 文` right";
        let start = source.find('`').unwrap();
        let end = source.find('文').unwrap();
        for reversed in [false, true] {
            let mut buffer = Buffer::from_text(source);
            let selected = if reversed {
                Cursor {
                    offset: start,
                    anchor: end,
                }
            } else {
                Cursor {
                    offset: end,
                    anchor: start,
                }
            };
            let edit = plan(
                &buffer,
                selected,
                SourceAction::Insert("*"),
                &mut SourceContextCache::default(),
            );
            assert_eq!(edit.range, start..end);
            let cursor = apply(&mut buffer, vec![(edit, true)]).unwrap()[0].0;
            assert_eq!(
                buffer.text(),
                format!(
                    "{}*{}*{}",
                    &source[..start],
                    &source[start..end],
                    &source[end..]
                )
            );
            assert_eq!(
                &buffer.text()[cursor.selection_range()],
                &source[start..end]
            );
            assert_eq!(cursor.offset < cursor.anchor, reversed);
        }
    }

    #[test]
    fn lexical_skips_decline_beyond_line_byte_and_query_budgets() {
        for source in [
            format!("```\n(\n{}¦)\n```", "x\n".repeat(MAX_SCAN_LINES + 8)),
            format!("({}¦)", "文".repeat(MAX_SCAN_BYTES / 3 + 8)),
            format!("{}(x¦)", "`code` () ".repeat(MAX_CONTEXT_QUERIES)),
        ] {
            let expected = source.replacen('¦', ")¦", 1);
            run(&source, SourceAction::Insert(")"), &expected);
        }
        for source in [
            format!("\\[\n{}¦\\]", "x\n".repeat(MAX_SCAN_LINES + 8)),
            format!("\\[{}¦\\]", "文".repeat(MAX_SCAN_BYTES / 3 + 8)),
        ] {
            let (buffer, cursor) = marked(&source);
            assert!(!unclosed_tex(
                &buffer,
                cursor.offset,
                '[',
                &mut SourceContextCache::default()
            ));
            let expected = source.replacen('¦', "\\¦", 1);
            run(&source, SourceAction::Insert("\\"), &expected);
        }
        let source = format!("```\n(\n{}¦)\n```", "x\n".repeat(MAX_SCAN_LINES / 2));
        run(
            &source,
            SourceAction::Insert(")"),
            &source.replacen("¦)", ")¦", 1),
        );
    }

    #[test]
    fn same_line_fence_queries_reuse_deep_context_without_prefix_buffers() {
        let prefix = "paragraph\n\n".repeat(4096);
        for (tail, input, expected_text) in [("```¦```", "`", "``"), ("$$¦$$", "$", "")] {
            let (buffer, cursor) = marked(&format!("{prefix}{tail}"));
            let mut cache = SourceContextCache::default();
            input_ime::source_input_context(&buffer, cursor.offset, &mut cache);
            for _ in 0..64 {
                let edit = plan(&buffer, cursor, SourceAction::Insert(input), &mut cache);
                assert_eq!(edit.text, expected_text);
            }
        }
        let (buffer, cursor) = marked(&format!("{prefix}```\n```¦```\n```"));
        let edit = plan(
            &buffer,
            cursor,
            SourceAction::Insert("`"),
            &mut SourceContextCache::default(),
        );
        assert_eq!(edit.text, "`");
    }

    #[test]
    fn incompatible_and_invalid_plans_are_atomic() {
        for plans in [
            vec![
                (replacement(0..2, "x"), true),
                (replacement(1..3, "y"), false),
            ],
            vec![
                (replacement(0..0, "x"), true),
                (replacement(0..0, "y"), false),
            ],
            vec![
                (replacement(0..2, ""), true),
                (replacement(1..1, "x"), false),
            ],
            vec![(replacement(4..5, "x"), true)],
            vec![(
                SourceCursorEdit {
                    range: 0..0,
                    text: "🙂".into(),
                    cursor: caret(1),
                },
                true,
            )],
            vec![(
                SourceCursorEdit {
                    range: 0..0,
                    text: String::new(),
                    cursor: caret(100),
                },
                true,
            )],
        ] {
            let mut buffer = Buffer::from_text("abc文");
            buffer.begin_edit_recording();
            assert!(!validate(&buffer, &plans));
            assert!(apply(&mut buffer, plans).is_none());
            assert_eq!(buffer.text(), "abc文");
            assert!(buffer.finish_edit_recording().is_empty());
        }
    }
}
