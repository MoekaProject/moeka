use std::{borrow::Cow, ops::Range};

use crate::{buffer::Buffer, cursor::Cursor};

use super::*;

struct MultiCursorImeEdit {
    start: usize,
    selection_start: usize,
    selection_end: usize,
    marked_range: Option<Range<usize>>,
    is_primary: bool,
    delta: isize,
}

struct MultiCursorImeReplacement {
    cursor: Cursor,
    extra_cursors: Vec<Cursor>,
    marked_range: Option<Range<usize>>,
    extra_marked_ranges: Vec<Range<usize>>,
    range_spans_multiple_lines: bool,
}

fn multi_cursor_ime_targets_desc(
    buffer: &Buffer,
    cursor: Cursor,
    extra_cursors: &[Cursor],
    marked_range: Option<Range<usize>>,
    extra_marked_ranges: &[Range<usize>],
    primary_override_range: Option<Range<usize>>,
    source_mode: bool,
) -> Vec<(Range<usize>, bool)> {
    let primary_range = primary_override_range
        .or(marked_range)
        .unwrap_or_else(|| cursor.selection_range());
    let mut all = Vec::with_capacity(1 + extra_cursors.len());
    all.push((
        primary_range.start.min(primary_range.end)..primary_range.end.max(primary_range.start),
        true,
    ));
    for (index, cursor) in extra_cursors.iter().enumerate() {
        let range = extra_marked_ranges
            .get(index)
            .cloned()
            .unwrap_or_else(|| cursor.selection_range());
        all.push((
            range.start.min(range.end)..range.end.max(range.start),
            false,
        ));
    }
    if source_mode {
        // Resolve explicit replacements and active marks FIRST. Normalizing the
        // visible selections instead would still permit overlapping mutations.
        // Native ranges may split a grapheme at a valid character boundary.
        all = super::source_input::normalize_native_targets(
            all.into_iter()
                .map(|(range, primary)| {
                    (
                        Cursor {
                            anchor: range.start,
                            offset: range.end,
                        },
                        primary,
                    )
                })
                .collect(),
            buffer,
        )
        .into_iter()
        .map(|(cursor, primary)| (cursor.selection_range(), primary))
        .collect();
    }
    all.sort_by(|left, right| {
        right
            .0
            .start
            .cmp(&left.0.start)
            .then_with(|| right.0.end.cmp(&left.0.end))
    });
    all
}

fn replace_multi_cursor_ime_targets(
    buffer: &mut Buffer,
    targets: Vec<(Range<usize>, bool)>,
    new_text: &str,
    new_selected_range_utf16: Option<&Range<usize>>,
    keep_marked_ranges: bool,
    source_mode: bool,
) -> MultiCursorImeReplacement {
    let selection_relative = new_selected_range_utf16
        .map(|range| {
            (
                EditorView::utf8_offset_in_text_for_utf16(new_text, range.start),
                EditorView::utf8_offset_in_text_for_utf16(new_text, range.end),
            )
        })
        .unwrap_or((new_text.len(), new_text.len()));
    let mut result = MultiCursorImeReplacement {
        cursor: Cursor::new(),
        extra_cursors: Vec::new(),
        marked_range: None,
        extra_marked_ranges: Vec::new(),
        range_spans_multiple_lines: false,
    };
    let mut edits = Vec::with_capacity(targets.len());
    for (range, is_primary) in targets {
        let (start_line, _) = buffer.offset_to_line_col(range.start);
        let (end_line, _) = buffer.offset_to_line_col(range.end);
        result.range_spans_multiple_lines |= start_line != end_line;
        let before_len = buffer.len();
        buffer.replace(range.clone(), new_text);
        let mark_start = range.start;
        let mark_end = mark_start + new_text.len();
        edits.push(MultiCursorImeEdit {
            start: range.start,
            selection_start: mark_start + selection_relative.0,
            selection_end: mark_start + selection_relative.1,
            marked_range: (keep_marked_ranges && !new_text.is_empty())
                .then_some(mark_start..mark_end),
            is_primary,
            delta: buffer.len() as isize - before_len as isize,
        });
    }
    edits.sort_by_key(|edit| edit.start);
    let mut cumulative_delta = 0isize;
    for edit in edits {
        let anchor = clamp_ime_offset(buffer, edit.selection_start, cumulative_delta);
        let offset = clamp_ime_offset(buffer, edit.selection_end, cumulative_delta);
        let marked_range = edit.marked_range.map(|range| {
            if source_mode {
                // Native marks may own only part of a grapheme. Snapping them
                // to its start would make a later commit replace adjacent text.
                buffer.clamp_offset(apply_offset_delta(range.start, cumulative_delta))
                    ..buffer.clamp_offset(apply_offset_delta(range.end, cumulative_delta))
            } else {
                clamp_ime_offset(buffer, range.start, cumulative_delta)
                    ..clamp_ime_offset(buffer, range.end, cumulative_delta)
            }
        });
        if edit.is_primary {
            result.cursor = Cursor { offset, anchor };
            result.marked_range = marked_range;
        } else {
            result.extra_cursors.push(Cursor { offset, anchor });
            if let Some(marked_range) = marked_range {
                result.extra_marked_ranges.push(marked_range);
            }
        }
        cumulative_delta += edit.delta;
    }
    result
}

pub(super) fn code_fence_enter_closer(text: &str, line: usize, pending: bool) -> Option<String> {
    if pending {
        markdown::unclosed_code_fence_marker(text.lines().nth(line)?, 0)
    } else {
        markdown::unclosed_code_fence_marker(text, line)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) struct SourceEditPlan {
    pub range: Range<usize>,
    pub text: String,
    pub cursor_offset: usize,
}

impl SourceEditPlan {
    fn replace(range: Range<usize>, text: String) -> Self {
        let cursor_offset = range.start + text.len();
        Self {
            range,
            text,
            cursor_offset,
        }
    }
}

const SOURCE_CONTEXT_CHECKPOINT_LINES: usize = 64;
const SOURCE_CLOSER_LOOKAHEAD_LINES: usize = 256;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum SourceInputContext {
    #[default]
    Markdown,
    Code,
    Math,
    FrontMatter,
    Html,
}

#[derive(Clone, PartialEq, Eq)]
enum SourceHtmlBlock {
    Until {
        closer: String,
        insensitive: bool,
        opening: bool,
        quote: Option<u8>,
    },
    Tag {
        name: String,
        depth: usize,
        opening: bool,
        quote: Option<u8>,
    },
    Blank,
}

#[derive(Clone, Default, PartialEq, Eq)]
struct SourceHtmlAssistanceState {
    region: Option<SourceHtmlAssistanceRegion>,
}

#[derive(Clone, PartialEq, Eq)]
enum SourceHtmlAssistanceRegion {
    Until(&'static str),
    Declaration {
        quote: Option<u8>,
        brackets: usize,
    },
    Tag {
        name: String,
        closing: bool,
        quote: Option<u8>,
        last: u8,
    },
    Raw(String),
    Link {
        depth: usize,
        quote: Option<u8>,
    },
}

#[derive(Clone, Default)]
struct SourceYamlState {
    quote: Option<u8>,
    scalar_indent: Option<usize>,
    flow_depth: usize,
}

#[derive(Clone)]
enum SourceContainer {
    Quote { indent: usize },
    List { width: usize },
}

#[derive(Clone, Default)]
struct SourceScanState {
    containers: Vec<SourceContainer>,
    fence: Option<String>,
    math_closer: Option<&'static str>,
    paragraph: bool,
    seen_line: bool,
    front_matter: bool,
    yaml: SourceYamlState,
    html: Option<SourceHtmlBlock>,
    html_assistance: SourceHtmlAssistanceState,
}

#[derive(Clone, Default)]
struct SourceLineContext {
    kind: SourceInputContext,
    // Kept as the internal shorthand for any block literal, not just code.
    code: bool,
    literal_prefix: String,
    yaml_literal: bool,
    front_matter_opener: bool,
    math_opener: Option<&'static str>,
    inline_regions: Vec<(Range<usize>, SourceInputContext)>,
    html_boundary: bool,
    ended_fence: bool,
    fence_opener: Option<String>,
    fence_prefix: String,
}

#[derive(Clone)]
struct SourceContextCheckpoint {
    // State immediately BEFORE this line; offsets never survive an edit.
    line: usize,
    state: SourceScanState,
}

/// Sparse checkpoints plus the last queried line's two boundaries. Callers must
/// invalidate BEFORE mutation, even for equal-length replacements, or clear on
/// buffer replacement. Neither buffer length nor line count identifies an edit.
#[derive(Default)]
pub(crate) struct SourceContextCache {
    /// Canonical parser state while `EditorView::decorations` holds Source's
    /// presentation projection. Kept with the existing per-pane Source cache.
    pub(super) markdown_decorations: Option<Vec<crate::markdown::LineDecoration>>,
    checkpoints: Vec<SourceContextCheckpoint>,
    before: Option<SourceContextCheckpoint>,
    after: Option<SourceContextCheckpoint>,
    current: Option<(usize, SourceLineContext)>,
    #[cfg(test)]
    scanned_lines: usize,
    #[cfg(test)]
    lookahead_scanned_lines: std::cell::Cell<usize>,
}

impl SourceContextCache {
    pub(crate) fn invalidate_from(&mut self, line: usize) {
        let keep = self
            .checkpoints
            .partition_point(|checkpoint| checkpoint.line <= line);
        self.checkpoints.truncate(keep);
        if self
            .before
            .as_ref()
            .is_some_and(|checkpoint| checkpoint.line > line)
        {
            self.before = None;
        }
        if self
            .after
            .as_ref()
            .is_some_and(|checkpoint| checkpoint.line > line)
        {
            self.after = None;
        }
        if self
            .current
            .as_ref()
            .is_some_and(|(current, _)| *current >= line)
        {
            self.current = None;
        }
    }

    pub(crate) fn clear(&mut self) {
        self.markdown_decorations = None;
        self.checkpoints.clear();
        self.before = None;
        self.after = None;
        self.current = None;
    }

    fn line_context(&mut self, buffer: &Buffer, line: usize) -> SourceLineContext {
        if let Some((current, context)) = &self.current
            && *current == line
        {
            return context.clone();
        }
        let count = self
            .checkpoints
            .partition_point(|checkpoint| checkpoint.line <= line);
        let checkpoint = self.checkpoints[..count]
            .last()
            .into_iter()
            .chain(self.before.iter())
            .chain(self.after.iter())
            .filter(|checkpoint| checkpoint.line <= line)
            .max_by_key(|checkpoint| checkpoint.line);
        let (start, mut state) = checkpoint.map_or_else(
            || (0, SourceScanState::default()),
            |checkpoint| (checkpoint.line, checkpoint.state.clone()),
        );
        for index in start..=line {
            if index % SOURCE_CONTEXT_CHECKPOINT_LINES == 0
                && self
                    .checkpoints
                    .last()
                    .is_none_or(|checkpoint| checkpoint.line < index)
            {
                self.checkpoints.push(SourceContextCheckpoint {
                    line: index,
                    state: state.clone(),
                });
            }
            if index == line {
                self.before = Some(SourceContextCheckpoint {
                    line,
                    state: state.clone(),
                });
            }
            let content = buffer.line_content(index);
            let mut context = state.scan_line(content);
            if index == line {
                if context.kind == SourceInputContext::Markdown {
                    context.inline_regions = source_inline_regions(content);
                } else {
                    let expanded = source_expanded_tabs(content);
                    if expanded.starts_with(&context.literal_prefix) {
                        let target = context.literal_prefix.len();
                        let mut expanded_offset = 0;
                        let mut column = 0;
                        for (offset, ch) in content.char_indices() {
                            if expanded_offset == target {
                                context.literal_prefix = content[..offset].into();
                                break;
                            }
                            let width = if ch == '\t' {
                                4 - column % 4
                            } else {
                                ch.len_utf8()
                            };
                            expanded_offset += width;
                            column += if ch == '\t' { width } else { 1 };
                        }
                        if expanded_offset == target && content.trim().is_empty() {
                            context.literal_prefix = content.into();
                        }
                        if context.fence_opener.is_some() || context.math_opener.is_some() {
                            context.fence_prefix = context.literal_prefix.clone();
                        }
                    }
                }
            }
            #[cfg(test)]
            {
                self.scanned_lines += 1;
            }
            if index == line {
                self.after = Some(SourceContextCheckpoint {
                    line: line + 1,
                    state,
                });
                self.current = Some((line, context.clone()));
                return context;
            }
        }
        unreachable!("the requested line is scanned inclusively")
    }

    fn state_before_line(&mut self, buffer: &Buffer, line: usize) -> SourceScanState {
        let index = self
            .checkpoints
            .partition_point(|checkpoint| checkpoint.line < line);
        if let Some(checkpoint) = self
            .before
            .iter()
            .chain(self.after.iter())
            .chain(self.checkpoints.get(index))
            .find(|checkpoint| checkpoint.line == line)
        {
            return checkpoint.state.clone();
        }
        if line == 0 {
            return SourceScanState::default();
        }
        // Warm only the preceding lines. Scanning the target line here would
        // let the marker being tested influence its own eligibility.
        self.line_context(buffer, line - 1);
        self.after
            .as_ref()
            .expect("preceding line leaves its end boundary")
            .state
            .clone()
    }

    fn block_needs_closer(&self, buffer: &Buffer, line: usize, kind: SourceInputContext) -> bool {
        let Some(after) = self
            .after
            .as_ref()
            .filter(|checkpoint| checkpoint.line == line + 1)
        else {
            return false;
        };
        let mut state = after.state.clone();
        // A distant/unknown closer is not evidence of an unclosed block. Bound
        // lookahead and leave the hot query boundary intact, even on huge files.
        let end = buffer
            .line_count()
            .min(line + 1 + SOURCE_CLOSER_LOOKAHEAD_LINES);
        for index in line + 1..end {
            #[cfg(test)]
            self.lookahead_scanned_lines
                .set(self.lookahead_scanned_lines.get() + 1);
            let context = state.scan_line(buffer.line_content(index));
            let open = match kind {
                SourceInputContext::Code => !context.ended_fence && state.fence.is_some(),
                SourceInputContext::Math => state.math_closer.is_some(),
                SourceInputContext::FrontMatter => state.front_matter,
                _ => false,
            };
            if !open {
                return false;
            }
        }
        end == buffer.line_count()
    }
}

fn source_expanded_tabs(content: &str) -> Cow<'_, str> {
    if !content.contains('\t') {
        return Cow::Borrowed(content);
    }
    let mut expanded = String::with_capacity(content.len());
    let mut column = 0usize;
    for ch in content.chars() {
        if ch == '\t' {
            let spaces = 4 - column % 4;
            expanded.extend(std::iter::repeat_n(' ', spaces));
            column += spaces;
        } else {
            expanded.push(ch);
            column += 1;
        }
    }
    Cow::Owned(expanded)
}

fn source_quote_width(content: &str) -> Option<usize> {
    let indent = content.len() - content.trim_start_matches(' ').len();
    if indent > 3 || content.as_bytes().get(indent) != Some(&b'>') {
        return None;
    }
    Some(indent + 1 + usize::from(content.as_bytes().get(indent + 1) == Some(&b' ')))
}

fn source_is_thematic_break(content: &str) -> bool {
    let mut markers = content.bytes().filter(|byte| !matches!(byte, b' ' | b'\t'));
    let Some(first @ (b'-' | b'*' | b'_')) = markers.next() else {
        return false;
    };
    let mut count = 1;
    for marker in markers {
        if marker != first {
            return false;
        }
        count += 1;
    }
    count >= 3
}

fn source_list_container_width(content: &str) -> Option<usize> {
    let indent = content.len() - content.trim_start_matches(' ').len();
    if indent > 3 {
        return None;
    }
    let body = &content[indent..];
    let first = *body.as_bytes().first()?;
    // A thematic break is not a stack of empty list containers.
    if source_is_thematic_break(body) {
        return None;
    }
    let marker = if matches!(first, b'-' | b'*' | b'+')
        && body.as_bytes().get(1).is_none_or(|byte| *byte == b' ')
    {
        1
    } else if markdown::ordered_list_prefix_len(body) > 0 {
        body.bytes().take_while(u8::is_ascii_digit).count() + 1
    } else {
        return None;
    };
    let padding = body[marker..].len() - body[marker..].trim_start_matches(' ').len();
    Some(
        indent
            + marker
            + if (1..=4).contains(&padding) {
                padding
            } else {
                1
            },
    )
}

fn source_math_opener(content: &str) -> Option<(&'static str, bool)> {
    let trimmed = content.trim_start_matches(' ');
    if content.len() - trimmed.len() > 3 {
        return None;
    }
    let trimmed = trimmed.trim_end();
    if let Some(body) = trimmed.strip_prefix("$$") {
        Some(("$$", source_has_math_closer(body, "$$")))
    } else if trimmed == "\\[" {
        Some(("\\]", false))
    } else if let Some(body) = trimmed.strip_prefix("\\[")
        && trimmed.ends_with("\\]")
    {
        Some(("\\]", source_has_math_closer(body, "\\]")))
    } else {
        None
    }
}

fn source_has_math_closer(content: &str, closer: &str) -> bool {
    content.match_indices(closer).any(|(offset, _)| {
        content.as_bytes()[..offset]
            .iter()
            .rev()
            .take_while(|byte| **byte == b'\\')
            .count()
            % 2
            == 0
    })
}

fn source_yaml_syntax(content: &str, mut quote: Option<u8>) -> (String, Option<u8>) {
    let bytes = content.as_bytes();
    let mut syntax = bytes.to_vec();
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(delimiter) = quote {
            syntax[index] = b' ';
            if delimiter == b'"' && byte == b'\\' {
                index += 1;
                if index < bytes.len() {
                    syntax[index] = b' ';
                }
            } else if byte == delimiter {
                if delimiter == b'\'' && bytes.get(index + 1) == Some(&delimiter) {
                    index += 1;
                    syntax[index] = b' ';
                } else {
                    quote = None;
                }
            }
        } else if matches!(byte, b'\'' | b'"')
            && (index == 0
                || matches!(
                    bytes[index - 1],
                    b' ' | b'\t' | b':' | b'-' | b'[' | b',' | b'{'
                ))
        {
            quote = Some(byte);
            // Keep a value token so `key: "text"` is not mistaken for `key:`.
            syntax[index] = b'v';
        } else if byte == b'#' && (index == 0 || bytes[index - 1].is_ascii_whitespace()) {
            syntax[index..].fill(b' ');
            break;
        }
        index += 1;
    }
    (
        String::from_utf8(syntax).expect("masking complete YAML strings preserves UTF-8"),
        quote,
    )
}

fn source_yaml_value(syntax: &str) -> &str {
    syntax
        .match_indices(':')
        .filter(|(index, _)| {
            syntax
                .as_bytes()
                .get(index + 1)
                .is_none_or(u8::is_ascii_whitespace)
        })
        .next()
        .map_or_else(
            || syntax.trim_start().strip_prefix("- ").unwrap_or(syntax),
            |(index, _)| &syntax[index + 1..],
        )
        .trim()
}

fn source_yaml_block_indent(value: &str) -> Option<usize> {
    if !value.starts_with(['|', '>']) {
        return None;
    }
    let mut indent = None;
    let mut chomping = false;
    for byte in value[1..].bytes() {
        match byte {
            b'1'..=b'9' if indent.is_none() => indent = Some(usize::from(byte - b'0')),
            b'+' | b'-' if !chomping => chomping = true,
            _ => return None,
        }
    }
    Some(indent.unwrap_or(2))
}

impl SourceYamlState {
    fn scan_line(&mut self, content: &str) -> bool {
        let indent = content.len() - content.trim_start_matches([' ', '\t']).len();
        if let Some(base) = self.scalar_indent {
            if content.trim().is_empty() || indent > base {
                return true;
            }
            self.scalar_indent = None;
        }
        let was_quoted = self.quote.is_some();
        let (syntax, quote) = source_yaml_syntax(content, self.quote);
        self.quote = quote;
        let was_flow = self.flow_depth > 0;
        for byte in syntax.bytes() {
            match byte {
                b'[' | b'{' => self.flow_depth += 1,
                b']' | b'}' => self.flow_depth = self.flow_depth.saturating_sub(1),
                _ => {}
            }
        }
        let value = source_yaml_value(&syntax);
        let block_scalar = source_yaml_block_indent(value).is_some();
        let raw_value = source_yaml_value(content);
        let plain_scalar = !value.is_empty()
            && !raw_value.starts_with(['\'', '"', '[', '{', '&', '*', '!'])
            && (syntax.contains(':') || syntax.trim_start().starts_with("- "));
        if !was_quoted && !was_flow && self.flow_depth == 0 && (block_scalar || plain_scalar) {
            let sequence_mapping = syntax.trim_start().starts_with("- ") && syntax.contains(':');
            self.scalar_indent = Some(indent + if sequence_mapping { 2 } else { 0 });
        }
        was_quoted || quote.is_some() || was_flow || self.flow_depth > 0
    }
}

// This is a lexical tag reader, not an HTML/Markdown document parser. Attribute
// quotes matter: a `>` or apparent closing tag inside one must remain literal.
fn source_html_tag(content: &str) -> Option<(String, bool, bool, usize)> {
    let bytes = content.as_bytes();
    if bytes.first() != Some(&b'<') {
        return None;
    }
    let closing = bytes.get(1) == Some(&b'/');
    let start = 1 + usize::from(closing);
    if !bytes.get(start)?.is_ascii_alphabetic() {
        return None;
    }
    let mut index = start;
    while bytes
        .get(index)
        .is_some_and(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b':'))
    {
        index += 1;
    }
    if !bytes
        .get(index)
        .is_some_and(|byte| byte.is_ascii_whitespace() || matches!(byte, b'/' | b'>'))
    {
        return None;
    }
    let name = content[start..index].to_ascii_lowercase();
    let mut quote = None;
    while let Some(&byte) = bytes.get(index) {
        if let Some(delimiter) = quote {
            if byte == delimiter {
                quote = None;
            }
        } else if matches!(byte, b'\'' | b'"') {
            quote = Some(byte);
        } else if byte == b'>' {
            let void = matches!(
                name.as_str(),
                "area"
                    | "base"
                    | "br"
                    | "col"
                    | "embed"
                    | "hr"
                    | "img"
                    | "input"
                    | "link"
                    | "meta"
                    | "param"
                    | "source"
                    | "track"
                    | "wbr"
            );
            return Some((
                name,
                closing,
                void || bytes.get(index.wrapping_sub(1)) == Some(&b'/'),
                index + 1,
            ));
        }
        index += 1;
    }
    None
}

fn source_html_opening_end(
    content: &str,
    opening: &mut bool,
    quote: &mut Option<u8>,
) -> Option<usize> {
    if !*opening {
        return Some(0);
    }
    for (index, byte) in content.bytes().enumerate() {
        if let Some(delimiter) = *quote {
            if byte == delimiter {
                *quote = None;
            }
        } else if matches!(byte, b'\'' | b'"') {
            *quote = Some(byte);
        } else if byte == b'>' {
            *opening = false;
            return Some(index + 1);
        }
    }
    None
}

impl SourceHtmlBlock {
    fn advance(&mut self, content: &str) -> bool {
        match self {
            Self::Until {
                closer,
                insensitive,
                opening,
                quote,
            } => {
                let Some(start) = source_html_opening_end(content, opening, quote) else {
                    return false;
                };
                let content = &content[start..];
                if *insensitive {
                    // Raw text elements terminate only on their own closing tag.
                    let lower = content.to_ascii_lowercase();
                    lower.match_indices(closer.as_str()).any(|(index, _)| {
                        source_html_tag(&content[index..])
                            .is_some_and(|(tag, closing, _, _)| closing && tag == closer[2..])
                    })
                } else {
                    content.contains(closer.as_str())
                }
            }
            Self::Blank => content.trim().is_empty(),
            Self::Tag {
                name,
                depth,
                opening,
                quote,
            } => {
                let Some(start) = source_html_opening_end(content, opening, quote) else {
                    return false;
                };
                let mut rest = &content[start..];
                while let Some(index) = rest.find('<') {
                    rest = &rest[index..];
                    if rest.starts_with("<!--") {
                        let Some(end) = rest.find("-->") else { break };
                        rest = &rest[end + 3..];
                    } else if let Some((tag, closing, void, end)) = source_html_tag(rest) {
                        if tag == *name {
                            if closing {
                                *depth = depth.saturating_sub(1);
                                if *depth == 0 {
                                    return true;
                                }
                            } else if !void {
                                *depth += 1;
                            }
                        }
                        rest = &rest[end..];
                    } else {
                        rest = &rest[1..];
                    }
                }
                false
            }
        }
    }
}

fn source_html_block(content: &str, paragraph: bool) -> Option<SourceHtmlBlock> {
    let body = content.trim_start_matches(' ');
    if content.len() - body.len() > 3 || !body.starts_with('<') {
        return None;
    }
    let until = if body.starts_with("<!--") {
        Some("-->")
    } else if body.starts_with("<![CDATA[") {
        Some("]]>")
    } else if body.starts_with("<?") {
        Some("?>")
    } else if body.starts_with("<!") && body.as_bytes().get(2).is_some_and(u8::is_ascii_alphabetic)
    {
        Some(">")
    } else {
        None
    };
    if let Some(closer) = until {
        return Some(SourceHtmlBlock::Until {
            closer: closer.into(),
            insensitive: false,
            opening: false,
            quote: None,
        });
    }
    // Raw-text openers may be unfinished while attributes are being entered.
    let lower = body.to_ascii_lowercase();
    for name in ["script", "style", "pre", "textarea"] {
        if lower
            .strip_prefix(&format!("<{name}"))
            .is_some_and(|rest| rest.starts_with([' ', '\t', '>']) || rest.is_empty())
        {
            return Some(SourceHtmlBlock::Until {
                closer: format!("</{name}"),
                insensitive: true,
                opening: true,
                quote: None,
            });
        }
    }
    let complete = source_html_tag(body);
    let incomplete = complete.is_none();
    let (name, closing, void, end) = complete.or_else(|| {
        let closing = body.as_bytes().get(1) == Some(&b'/');
        let start = 1 + usize::from(closing);
        let tail = body.get(start..)?;
        if !tail.as_bytes().first()?.is_ascii_alphabetic() {
            return None;
        }
        let len = tail
            .bytes()
            .take_while(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b':'))
            .count();
        if !tail.as_bytes().get(len).is_none_or(u8::is_ascii_whitespace) {
            return None;
        }
        Some((tail[..len].to_ascii_lowercase(), closing, false, body.len()))
    })?;
    let block = matches!(
        name.as_str(),
        "address"
            | "article"
            | "aside"
            | "base"
            | "blockquote"
            | "body"
            | "caption"
            | "center"
            | "col"
            | "colgroup"
            | "dd"
            | "details"
            | "dialog"
            | "dir"
            | "div"
            | "dl"
            | "dt"
            | "fieldset"
            | "figcaption"
            | "figure"
            | "footer"
            | "form"
            | "frame"
            | "frameset"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "h5"
            | "h6"
            | "head"
            | "header"
            | "hr"
            | "html"
            | "iframe"
            | "legend"
            | "li"
            | "link"
            | "main"
            | "menu"
            | "menuitem"
            | "nav"
            | "noframes"
            | "ol"
            | "optgroup"
            | "option"
            | "p"
            | "param"
            | "search"
            | "section"
            | "summary"
            | "table"
            | "tbody"
            | "td"
            | "tfoot"
            | "th"
            | "thead"
            | "title"
            | "tr"
            | "track"
            | "ul"
    );
    if !block && (incomplete || paragraph || !body[end..].trim().is_empty()) {
        return None;
    }
    Some(if closing || void {
        SourceHtmlBlock::Blank
    } else {
        SourceHtmlBlock::Tag {
            name,
            depth: 1,
            opening: true,
            quote: None,
        }
    })
}

impl SourceHtmlAssistanceState {
    // A separate lexical state is needed inside ordinary HTML containers: their
    // block state alone cannot distinguish child tags from JS, comments or an
    // attribute continued onto the next line. It also covers inline HTML.
    fn scan_line(&mut self, content: &str, markdown: bool) -> bool {
        use SourceHtmlAssistanceRegion as R;
        let bytes = content.as_bytes();
        if self.region.is_none() && !content.contains('<') && !content.contains("](") {
            return false;
        }
        let protected = if markdown {
            source_inline_regions(content)
        } else {
            Vec::new()
        };
        let mut boundary = false;
        let mut index = 0;
        while index < bytes.len() {
            match self.region.take() {
                Some(R::Until(closer)) => {
                    if let Some(end) = content[index..].find(closer) {
                        index += end + closer.len();
                        boundary = true;
                    } else {
                        self.region = Some(R::Until(closer));
                        break;
                    }
                }
                Some(R::Declaration {
                    mut quote,
                    mut brackets,
                }) => {
                    let mut ended = false;
                    while let Some(&byte) = bytes.get(index) {
                        index += 1;
                        if let Some(delimiter) = quote {
                            if byte == delimiter {
                                quote = None;
                            }
                        } else {
                            match byte {
                                b'\'' | b'"' => quote = Some(byte),
                                b'[' => brackets += 1,
                                b']' => brackets = brackets.saturating_sub(1),
                                b'>' if brackets == 0 => {
                                    ended = true;
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                    if ended {
                        boundary = true;
                    } else {
                        self.region = Some(R::Declaration { quote, brackets });
                    }
                }
                Some(R::Tag {
                    name,
                    closing,
                    mut quote,
                    mut last,
                }) => {
                    let mut ended = false;
                    while let Some(&byte) = bytes.get(index) {
                        index += 1;
                        if let Some(delimiter) = quote {
                            if byte == delimiter {
                                quote = None;
                            }
                        } else if matches!(byte, b'\'' | b'"') {
                            quote = Some(byte);
                        } else if byte == b'>' {
                            ended = true;
                            break;
                        }
                        if !byte.is_ascii_whitespace() {
                            last = byte;
                        }
                    }
                    if ended {
                        boundary = true;
                        if !closing
                            && last != b'/'
                            && matches!(
                                name.as_str(),
                                "pre" | "code" | "script" | "style" | "textarea"
                            )
                        {
                            self.region = Some(R::Raw(name));
                        }
                    } else {
                        self.region = Some(R::Tag {
                            name,
                            closing,
                            quote,
                            last,
                        });
                    }
                }
                Some(R::Raw(name)) => {
                    let Some(start) = content[index..].find("</") else {
                        self.region = Some(R::Raw(name));
                        break;
                    };
                    index += start + 2;
                    let rest = &content[index..];
                    if rest
                        .get(..name.len())
                        .is_some_and(|candidate| candidate.eq_ignore_ascii_case(&name))
                        && rest
                            .as_bytes()
                            .get(name.len())
                            .is_none_or(|byte| byte.is_ascii_whitespace() || *byte == b'>')
                    {
                        index += name.len();
                        self.region = Some(R::Tag {
                            name,
                            closing: true,
                            quote: None,
                            last: 0,
                        });
                    } else {
                        self.region = Some(R::Raw(name));
                    }
                }
                Some(R::Link {
                    mut depth,
                    mut quote,
                }) => {
                    while let Some(&byte) = bytes.get(index) {
                        index += 1;
                        if byte == b'\\' {
                            index = (index + 1).min(bytes.len());
                            continue;
                        }
                        if let Some(delimiter) = quote {
                            if byte == delimiter {
                                quote = None;
                            }
                        } else {
                            match byte {
                                b'\'' | b'"' => quote = Some(byte),
                                b'(' => depth += 1,
                                b')' => {
                                    depth -= 1;
                                    if depth == 0 {
                                        break;
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    if depth > 0 {
                        self.region = Some(R::Link { depth, quote });
                    }
                }
                None => {
                    if let Some((range, _)) = protected.iter().find(|(range, kind)| {
                        matches!(kind, SourceInputContext::Code | SourceInputContext::Math)
                            && range.contains(&index)
                    }) {
                        index = range.end.min(bytes.len());
                        continue;
                    }
                    if bytes[index] == b'\\' {
                        index += 1;
                        if let Some(ch) = content[index..].chars().next() {
                            index += ch.len_utf8();
                        }
                        continue;
                    }
                    let rest = &content[index..];
                    if rest.starts_with("](") {
                        self.region = Some(R::Link {
                            depth: 1,
                            quote: None,
                        });
                        index += 2;
                        continue;
                    }
                    if bytes[index] == b'<' {
                        let special = if rest.starts_with("<!--") {
                            Some(("-->", 4))
                        } else if rest.starts_with("<![CDATA[") {
                            Some(("]]>", 9))
                        } else if rest.starts_with("<?") {
                            Some(("?>", 2))
                        } else {
                            None
                        };
                        if let Some((closer, width)) = special {
                            self.region = Some(R::Until(closer));
                            index += width;
                            boundary = true;
                            continue;
                        }
                        if rest.starts_with("<!") {
                            self.region = Some(R::Declaration {
                                quote: None,
                                brackets: 0,
                            });
                            index += 2;
                            boundary = true;
                            continue;
                        }
                        let closing = rest.as_bytes().get(1) == Some(&b'/');
                        let start = 1 + usize::from(closing);
                        let tail = rest.get(start..).unwrap_or("");
                        if tail.as_bytes().first().is_some_and(u8::is_ascii_alphabetic) {
                            let len = tail
                                .bytes()
                                .take_while(|byte| {
                                    byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b':')
                                })
                                .count();
                            if tail.as_bytes().get(len).is_none_or(|byte| {
                                byte.is_ascii_whitespace() || matches!(byte, b'/' | b'>')
                            }) {
                                self.region = Some(R::Tag {
                                    name: tail[..len].to_ascii_lowercase(),
                                    closing,
                                    quote: None,
                                    last: 0,
                                });
                                index += start + len;
                                boundary = true;
                                continue;
                            }
                            if tail[..tail.find(char::is_whitespace).unwrap_or(tail.len())]
                                .contains('@')
                            {
                                self.region = Some(R::Until(">"));
                                index += 1;
                                continue;
                            }
                        }
                    }
                    index += content[index..].chars().next().unwrap().len_utf8();
                }
            }
        }
        boundary
    }
}

impl SourceScanState {
    fn container_prefix(&self) -> String {
        let mut prefix = String::new();
        for container in &self.containers {
            match container {
                SourceContainer::Quote { indent } => {
                    prefix.extend(std::iter::repeat_n(' ', *indent));
                    prefix.push_str("> ");
                }
                SourceContainer::List { width } => prefix.extend(std::iter::repeat_n(' ', *width)),
            }
        }
        prefix
    }

    fn literal_context(&self, kind: SourceInputContext, body: &str) -> SourceLineContext {
        let mut prefix = self.container_prefix();
        prefix.push_str(&body[..body.len() - body.trim_start_matches([' ', '\t']).len()]);
        SourceLineContext {
            kind,
            code: true,
            literal_prefix: prefix,
            ..Default::default()
        }
    }

    fn scan_line(&mut self, content: &str) -> SourceLineContext {
        let mut context = self.scan_block_line(content);
        if matches!(
            context.kind,
            SourceInputContext::Markdown | SourceInputContext::Html
        ) || self.html_assistance.region.is_some()
        {
            let expanded = source_expanded_tabs(content);
            let mut body = expanded.as_ref();
            for container in &self.containers {
                let width = match container {
                    SourceContainer::Quote { .. } => source_quote_width(body),
                    SourceContainer::List { width } => {
                        let indent = body.len() - body.trim_start_matches(' ').len();
                        if indent >= *width {
                            Some(*width)
                        } else {
                            source_list_container_width(body).filter(|actual| actual == width)
                        }
                    }
                };
                let Some(width) = width else {
                    break;
                };
                body = &body[width.min(body.len())..];
            }
            context.html_boundary = self
                .html_assistance
                .scan_line(body, context.kind == SourceInputContext::Markdown);
        }
        context
    }

    fn scan_block_line(&mut self, content: &str) -> SourceLineContext {
        let first = !self.seen_line;
        self.seen_line = true;
        let front_matter_opener =
            first && content.trim_start_matches('\u{feff}').trim_end() == "---";
        if front_matter_opener || self.front_matter {
            let delimiter = matches!(content.trim_end(), "---" | "...");
            self.front_matter = front_matter_opener || !delimiter;
            let yaml_literal = !delimiter && self.yaml.scan_line(content);
            return SourceLineContext {
                kind: SourceInputContext::FrontMatter,
                code: true,
                literal_prefix: content
                    [..content.len() - content.trim_start_matches([' ', '\t']).len()]
                    .into(),
                front_matter_opener,
                yaml_literal,
                ..Default::default()
            };
        }
        let expanded = source_expanded_tabs(content);
        let mut body = expanded.as_ref();
        let mut matched = 0;
        let mut ended_fence = false;
        for container in &self.containers {
            let width = match container {
                SourceContainer::Quote { .. } => source_quote_width(body),
                SourceContainer::List { width } => {
                    let indent = body.len() - body.trim_start_matches(' ').len();
                    if body.trim().is_empty() {
                        Some(indent)
                    } else {
                        (indent >= *width).then_some(*width)
                    }
                }
            };
            let Some(width) = width else {
                break;
            };
            body = &body[width..];
            matched += 1;
        }
        // Math bodies are literal even when a row looks like a new list,
        // heading or code fence. Keep this state in the invalidated checkpoints,
        // not canonical decorations (which may be stale or lazily unparsed).
        if matched == self.containers.len()
            && let Some(closer) = self.math_closer
        {
            if source_has_math_closer(body, closer) {
                self.math_closer = None;
            }
            self.paragraph = false;
            return self.literal_context(SourceInputContext::Math, body);
        }
        if matched == self.containers.len()
            && let Some(html) = self.html.as_mut()
        {
            let blank_end = matches!(html, SourceHtmlBlock::Blank) && body.trim().is_empty();
            if html.advance(body) {
                self.html = None;
            }
            self.paragraph = false;
            if !blank_end {
                return self.literal_context(SourceInputContext::Html, body);
            }
        }
        if matched < self.containers.len() {
            let plain_paragraph = !body.trim().is_empty()
                && source_quote_width(body).is_none()
                && source_list_container_width(body).is_none()
                && code_fence_enter_closer(body, 0, false).is_none()
                && source_math_opener(body).is_none()
                && !body.trim_start().starts_with('#');
            if self.fence.is_none() && self.html.is_none() && self.paragraph && plain_paragraph {
                return SourceLineContext::default();
            }
            self.containers.truncate(matched);
            self.math_closer = None;
            self.html = None;
            ended_fence = self.fence.take().is_some();
            self.paragraph = false;
        }
        if let Some(marker) = self.fence.as_deref() {
            let trimmed = body.trim_start_matches(' ');
            if body.len() - trimmed.len() <= 3
                && let Some(rest) = trimmed.strip_prefix(marker)
                && rest
                    .trim_start_matches(marker.chars().next().unwrap())
                    .trim()
                    .is_empty()
            {
                self.fence = None;
                ended_fence = true;
            }
            return SourceLineContext {
                ended_fence,
                ..self.literal_context(SourceInputContext::Code, body)
            };
        }
        loop {
            if let Some(width) = source_quote_width(body) {
                let indent = body.len() - body.trim_start_matches(' ').len();
                self.containers.push(SourceContainer::Quote { indent });
                body = &body[width..];
            } else if let Some(width) = source_list_container_width(body) {
                self.containers.push(SourceContainer::List { width });
                body = &body[width.min(body.len())..];
            } else {
                break;
            }
        }
        if let Some(marker) = code_fence_enter_closer(body, 0, false) {
            self.fence = Some(marker.clone());
            self.paragraph = false;
            let context = self.literal_context(SourceInputContext::Code, body);
            return SourceLineContext {
                fence_opener: Some(marker),
                fence_prefix: context.literal_prefix.clone(),
                ended_fence,
                ..context
            };
        }
        if let Some((closer, closed)) = source_math_opener(body) {
            self.math_closer = (!closed).then_some(closer);
            self.paragraph = false;
            let context = self.literal_context(SourceInputContext::Math, body);
            return SourceLineContext {
                math_opener: (!closed).then_some(closer),
                fence_prefix: context.literal_prefix.clone(),
                ended_fence,
                ..context
            };
        }
        if let Some(mut html) = source_html_block(body, self.paragraph) {
            if !html.advance(body) {
                self.html = Some(html);
            }
            self.paragraph = false;
            return self.literal_context(SourceInputContext::Html, body);
        }
        let indent = body.len() - body.trim_start_matches(' ').len();
        let code = indent >= 4;
        self.paragraph = !code && !body.trim().is_empty();
        if code {
            SourceLineContext {
                ended_fence,
                ..self.literal_context(SourceInputContext::Code, body)
            }
        } else {
            SourceLineContext {
                ended_fence,
                ..Default::default()
            }
        }
    }
}

struct SourceListPrefix {
    start: usize,
    end: usize,
    continuation: String,
    indent_step: usize,
}

fn source_list_prefix(content: &str) -> Option<SourceListPrefix> {
    let quote_len = markdown::blockquote_prefix_len(content);
    let body = &content[quote_len..];
    let indent = body.len() - body.trim_start_matches([' ', '\t']).len();
    let start = quote_len + indent;
    let trimmed = &content[start..];
    if source_is_thematic_break(trimmed) {
        return None;
    }
    let (end, continuation, indent_step) = if trimmed.starts_with(['-', '*', '+'])
        && trimmed
            .as_bytes()
            .get(1)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
    {
        let end = 1 + trimmed[1..].len() - trimmed[1..].trim_start_matches([' ', '\t']).len();
        (end, trimmed[..end].to_string(), 2)
    } else {
        let len = markdown::ordered_list_prefix_len(trimmed);
        if len == 0 {
            return None;
        }
        let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
        let number = trimmed[..digits].parse::<u32>().ok()?;
        let suffix = &trimmed[digits..len];
        (
            len,
            format!(
                "{}{}{}",
                (number + 1).min(999_999_999),
                suffix,
                if suffix.len() == 1 { " " } else { "" }
            ),
            digits + 2,
        )
    };
    let rest = &trimmed[end..];
    let task = rest.as_bytes();
    if task.first() == Some(&b'[')
        && task
            .get(1)
            .is_some_and(|byte| matches!(byte, b' ' | b'x' | b'X'))
        && task.get(2) == Some(&b']')
        && task.get(3).is_none_or(|byte| matches!(byte, b' ' | b'\t'))
    {
        let spaces = rest[3..].len() - rest[3..].trim_start_matches([' ', '\t']).len();
        return Some(SourceListPrefix {
            start,
            end: start + end + 3 + spaces,
            continuation: format!(
                "{continuation}[ ]{}",
                if spaces == 0 {
                    " "
                } else {
                    &rest[3..3 + spaces]
                }
            ),
            indent_step,
        });
    }
    Some(SourceListPrefix {
        start,
        end: start + end,
        continuation,
        indent_step,
    })
}

fn source_inline_regions(content: &str) -> Vec<(Range<usize>, SourceInputContext)> {
    use SourceInputContext::{Code, Html, Math};
    let bytes = content.as_bytes();
    let mut regions = Vec::new();
    let mut index = 0;
    // Index equal-length backtick runs once, including escaped runs (escapes do
    // not apply inside a code span). This avoids quadratic unmatched-run scans.
    let mut runs = std::collections::HashMap::<usize, Vec<usize>>::new();
    let mut run_index = 0;
    while run_index < bytes.len() {
        if bytes[run_index] == b'`' {
            let run = bytes[run_index..]
                .iter()
                .take_while(|byte| **byte == b'`')
                .count();
            runs.entry(run).or_default().push(run_index);
            run_index += run;
        } else {
            run_index += 1;
        }
    }
    while index < bytes.len() {
        let rest = &content[index..];
        if rest.starts_with("\\(") {
            if let Some(end) = rest[2..]
                .match_indices("\\)")
                .find(|(offset, _)| !source_escaped(rest, offset + 2))
                .map(|(offset, _)| index + 2 + offset)
            {
                regions.push((index + 2..end + 1, Math));
                index = end + 2;
                continue;
            }
        }
        if bytes[index] == b'\\' {
            index += 1;
            if let Some(ch) = content[index..].chars().next() {
                index += ch.len_utf8();
            }
            continue;
        }
        if bytes[index] == b'`' {
            let run = bytes[index..]
                .iter()
                .take_while(|byte| **byte == b'`')
                .count();
            let candidates = &runs[&run];
            let next = candidates.partition_point(|offset| *offset <= index);
            if let Some(&end) = candidates.get(next) {
                regions.push((index + run..end + 1, Code));
                index = end + run;
                continue;
            }
            index += run;
            continue;
        }
        if bytes[index] == b'$'
            && bytes
                .get(index + 1)
                .is_some_and(|byte| !byte.is_ascii_whitespace() && *byte != b'$')
        {
            if let Some(end) = content[index + 1..]
                .match_indices('$')
                .map(|(offset, _)| index + 1 + offset)
                .find(|end| !source_escaped(content, *end) && !bytes[end - 1].is_ascii_whitespace())
            {
                regions.push((index + 1..end + 1, Math));
                index = end + 1;
                continue;
            }
        }
        if rest.starts_with("](") {
            let start = index + 2;
            let mut end = start;
            let mut depth = 1usize;
            let mut quote = None;
            while end < bytes.len() {
                let byte = bytes[end];
                if byte == b'\\' {
                    end = (end + 2).min(bytes.len());
                    continue;
                }
                if let Some(delimiter) = quote {
                    if byte == delimiter {
                        quote = None;
                    }
                } else if matches!(byte, b'\'' | b'"') {
                    quote = Some(byte);
                } else if byte == b'(' {
                    depth += 1;
                } else if byte == b')' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                end += 1;
            }
            if end > start || end < bytes.len() {
                regions.push((start..end + 1, Html));
            }
            index = (end + 1).min(bytes.len());
            continue;
        }
        if bytes[index] == b'<' {
            let special = if rest.starts_with("<!--") {
                Some("-->")
            } else if rest.starts_with("<![CDATA[") {
                Some("]]>")
            } else if rest.starts_with("<?") {
                Some("?>")
            } else {
                None
            };
            if let Some(closer) = special {
                let end = rest
                    .find(closer)
                    .map_or(bytes.len(), |offset| index + offset + closer.len());
                regions.push((index + 1..end, Html));
                index = end;
                continue;
            }
            if let Some((name, closing, _, end)) = source_html_tag(rest) {
                let mut end = index + end;
                if !closing
                    && matches!(
                        name.as_str(),
                        "code" | "pre" | "script" | "style" | "textarea"
                    )
                {
                    if let Some(close) = content[end..]
                        .to_ascii_lowercase()
                        .find(&format!("</{name}>"))
                    {
                        end += close + name.len() + 3;
                    }
                }
                regions.push((index + 1..end, Html));
                index = end;
                continue;
            }
            if let Some(end) = rest.find('>') {
                let inside = &rest[1..end];
                if !inside.chars().any(char::is_whitespace)
                    && (inside.contains(':') || inside.contains('@'))
                {
                    regions.push((index + 1..index + end + 1, Html));
                    index += end + 1;
                    continue;
                }
            } else {
                let name = rest[1..]
                    .bytes()
                    .take_while(u8::is_ascii_alphanumeric)
                    .count();
                if name > 0
                    && rest
                        .as_bytes()
                        .get(name + 1)
                        .is_some_and(u8::is_ascii_whitespace)
                {
                    regions.push((index + name + 1..bytes.len() + 1, Html));
                    break;
                }
            }
        }
        if rest.starts_with("]:")
            && content[..index]
                .trim_start_matches([' ', '\t', '>'])
                .starts_with('[')
        {
            let start = index + 2;
            if !content[start..].trim().is_empty() {
                regions.push((start..bytes.len() + 1, Html));
            }
            break;
        }
        index += content[index..].chars().next().unwrap().len_utf8();
    }
    regions
}

fn source_escaped(content: &str, offset: usize) -> bool {
    content.as_bytes()[..offset]
        .iter()
        .rev()
        .take_while(|byte| **byte == b'\\')
        .count()
        % 2
        != 0
}

/// Classifies the insertion point, not the marker just typed. Unmatched inline
/// delimiters remain Markdown so typing an opener can still create its pair.
pub(super) fn source_input_context(
    buffer: &Buffer,
    offset: usize,
    cache: &mut SourceContextCache,
) -> SourceInputContext {
    let (line, column) = buffer.offset_to_line_col(offset);
    if cache
        .current
        .as_ref()
        .is_none_or(|(current, _)| *current != line)
    {
        cache.line_context(buffer, line);
    }
    let context = &cache.current.as_ref().expect("queried line is cached").1;
    if context.kind != SourceInputContext::Markdown {
        return context.kind;
    }
    context
        .inline_regions
        .iter()
        .find(|(range, _)| range.contains(&column))
        .map_or(SourceInputContext::Markdown, |(_, kind)| *kind)
}

/// Classifies only the text before `offset`, excluding the marker/suffix at
/// that insertion point. The cached boundary is cloned: a speculative prefix
/// scan must not change the real line's context or its following scan state.
pub(super) fn source_input_context_before(
    buffer: &Buffer,
    offset: usize,
    cache: &mut SourceContextCache,
) -> SourceInputContext {
    let (line, column) = buffer.offset_to_line_col(offset);
    let prefix = &buffer.line_content(line)[..column];
    let mut state = cache.state_before_line(buffer, line);
    let context = state.scan_line(prefix);
    if context.kind != SourceInputContext::Markdown {
        return context.kind;
    }
    source_inline_regions(prefix)
        .iter()
        .find(|(range, _)| range.contains(&column))
        .map_or(SourceInputContext::Markdown, |(_, kind)| *kind)
}

/// Whether `<` at `tag_start` is an actual tag position, rather than literal
/// markup in an attribute, raw-text element, comment, URL, code, math or YAML.
/// In particular, broad `Html` input context alone does not authorize tag edits.
pub(super) fn source_html_tag_assistance_allowed(
    buffer: &Buffer,
    tag_start: usize,
    cache: &mut SourceContextCache,
) -> bool {
    if !buffer
        .text()
        .get(tag_start..)
        .is_some_and(|text| text.starts_with('<'))
    {
        return false;
    }
    let (line, column) = buffer.offset_to_line_col(tag_start);
    let content = buffer.line_content(line);
    let prefix = &content[..column];
    if source_escaped(content, column) {
        return false;
    }
    let mut state = cache.state_before_line(buffer, line);
    let context = state.scan_line(prefix);
    if matches!(
        context.kind,
        SourceInputContext::Code | SourceInputContext::Math | SourceInputContext::FrontMatter
    ) || state.html_assistance.region.is_some()
    {
        return false;
    }
    // Full current-line spans catch a matching inline closer after the tag,
    // without reading/copying any document prefix or interpreting this tag as
    // its own literal opener. A tag's own span starts AFTER its `<`.
    if source_inline_regions(content)
        .iter()
        .any(|(range, _)| range.contains(&column))
    {
        return false;
    }
    let token = prefix.rsplit(char::is_whitespace).next().unwrap_or("");
    !token.contains("://") && !token.starts_with("mailto:") && !token.starts_with("www.")
}

/// A delimiter search must not borrow an opener across one of these lines,
/// even when adjacent blocks have the same broad input context. This probe
/// leaves the cached real-line boundaries untouched.
pub(super) fn source_context_boundary_before(
    buffer: &Buffer,
    line: usize,
    cache: &mut SourceContextCache,
) -> bool {
    if line >= buffer.line_count() {
        return true;
    }
    let before = cache.state_before_line(buffer, line);
    let mut after = before.clone();
    let context = after.scan_line(buffer.line_content(line));
    context.fence_opener.is_some()
        || context.ended_fence
        || context.html_boundary
        || before.fence != after.fence
        || before.math_closer != after.math_closer
        || (context.kind == SourceInputContext::Math && before.math_closer.is_none())
        || before.front_matter != after.front_matter
        || before.html != after.html
}

/// A single-cursor source edit touches only this line/selection. In particular,
/// continuing an ordered list never renumbers any existing lines.
pub(super) fn source_enter_plan(
    buffer: &Buffer,
    cursor: Cursor,
    context_cache: &mut SourceContextCache,
) -> SourceEditPlan {
    let range = cursor.selection_range();
    let plain = SourceEditPlan::replace(range.clone(), "\n".into());
    if cursor.has_selection() {
        return plain;
    }
    let (line, col) = buffer.offset_to_line_col(cursor.offset);
    let content = buffer.line_content(line);
    let quote_len = markdown::blockquote_prefix_len(content);
    let list_prefix = source_list_prefix(content);
    let trimmed = content.trim_start_matches([' ', '\t']);
    let before = &content[..col];
    let block_candidate = col == content.len()
        && (trimmed.starts_with("```")
            || trimmed.starts_with("~~~")
            || trimmed.starts_with("$$")
            || trimmed == "\\["
            || (line == 0 && trimmed.trim_start_matches('\u{feff}') == "---"));
    let indent_candidate = content.len() != trimmed.len()
        || before.trim_end().ends_with(['{', '[', '(', ':'])
        || before.contains(':');
    if list_prefix.as_ref().is_none_or(|prefix| col < prefix.end)
        && (quote_len == 0 || col < quote_len)
        && !block_candidate
        && !indent_candidate
    {
        return plain;
    }
    let context = context_cache.line_context(buffer, line);
    if col == content.len()
        && let Some(closer) = context.fence_opener.as_deref()
    {
        let prefix = &context.fence_prefix;
        let continuation =
            if context_cache.block_needs_closer(buffer, line, SourceInputContext::Code) {
                format!("\n{prefix}\n{prefix}{closer}")
            } else {
                format!("\n{prefix}")
            };
        return SourceEditPlan {
            range,
            text: continuation,
            cursor_offset: cursor.offset + 1 + prefix.len(),
        };
    }
    if col == content.len()
        && let Some(closer) = context.math_opener
    {
        let prefix = &context.fence_prefix;
        let text = if context_cache.block_needs_closer(buffer, line, SourceInputContext::Math) {
            format!("\n{prefix}\n{prefix}{closer}")
        } else {
            format!("\n{prefix}")
        };
        return SourceEditPlan {
            range,
            text,
            cursor_offset: cursor.offset + 1 + prefix.len(),
        };
    }
    if context.kind == SourceInputContext::FrontMatter {
        if context.front_matter_opener && col == content.len() {
            return SourceEditPlan {
                range,
                text: if context_cache.block_needs_closer(
                    buffer,
                    line,
                    SourceInputContext::FrontMatter,
                ) {
                    "\n\n---".into()
                } else {
                    "\n".into()
                },
                cursor_offset: cursor.offset + 1,
            };
        }
        return source_yaml_enter_plan(buffer, cursor, &context);
    }
    if context.code {
        return source_literal_enter_plan(content, cursor, col, &context);
    }
    if context
        .inline_regions
        .iter()
        .any(|(range, _)| range.contains(&col))
    {
        return plain;
    }
    let start = buffer.line_start(line);
    if let Some(prefix) = list_prefix {
        if col < prefix.end {
            return plain;
        }
        if content[prefix.end..].trim().is_empty() {
            let indent = prefix.start - quote_len;
            let text = if indent > 0 {
                format!(
                    "{}{}",
                    &content[..quote_len],
                    &content[quote_len + prefix.indent_step.min(indent)..]
                )
            } else {
                format!("{}\n", &content[..quote_len])
            };
            return SourceEditPlan::replace(start..buffer.line_end(line), text);
        }
        return SourceEditPlan::replace(
            range,
            format!("\n{}{}", &content[..prefix.start], prefix.continuation),
        );
    }
    if quote_len > 0 && col >= quote_len {
        if content[quote_len..].trim().is_empty() {
            return SourceEditPlan::replace(start..buffer.line_end(line), "\n".into());
        }
        return SourceEditPlan::replace(range, format!("\n{}", &content[..quote_len]));
    }
    plain
}

fn source_yaml_enter_plan(
    buffer: &Buffer,
    cursor: Cursor,
    context: &SourceLineContext,
) -> SourceEditPlan {
    let (line, col) = buffer.offset_to_line_col(cursor.offset);
    let content = buffer.line_content(line);
    let before = &content[..col];
    let leading = before.len() - before.trim_start_matches([' ', '\t']).len();
    let indent = &before[..leading];
    let mut prefix = indent.to_string();
    if !context.yaml_literal && !matches!(content.trim_end(), "---" | "...") {
        let (syntax, quote) = source_yaml_syntax(before, None);
        let body = syntax.trim_start();
        let sequence = body
            .strip_prefix('-')
            .is_some_and(|rest| rest.starts_with([' ', '\t']) || rest.is_empty());
        let value = source_yaml_value(&syntax);
        let flow = syntax
            .bytes()
            .any(|byte| matches!(byte, b'[' | b'{' | b']' | b'}'));
        let scalar_indent = source_yaml_block_indent(value);
        let child = !flow
            && quote.is_none()
            && ((syntax.trim_end().ends_with(':') && value.is_empty()) || scalar_indent.is_some());
        if child {
            let mapping_in_sequence = sequence && syntax.contains(':');
            prefix.extend(std::iter::repeat_n(
                ' ',
                scalar_indent.unwrap_or(2) + if mapping_in_sequence { 2 } else { 0 },
            ));
        } else if sequence && quote.is_none() {
            if body.trim() == "-" && col == content.len() {
                return SourceEditPlan::replace(
                    buffer.line_start(line) + leading..cursor.offset,
                    String::new(),
                );
            }
            let padding = body[1..].len() - body[1..].trim_start_matches([' ', '\t']).len();
            prefix.push('-');
            prefix.push_str(if padding == 0 {
                " "
            } else {
                &body[1..1 + padding]
            });
        }
    }
    SourceEditPlan::replace(cursor.selection_range(), format!("\n{prefix}"))
}

fn source_literal_enter_plan(
    content: &str,
    cursor: Cursor,
    col: usize,
    context: &SourceLineContext,
) -> SourceEditPlan {
    let prefix = &context.literal_prefix;
    if col < prefix.len() || (context.ended_fence && context.kind == SourceInputContext::Code) {
        return SourceEditPlan::replace(
            cursor.selection_range(),
            if col < prefix.len() {
                "\n".into()
            } else {
                format!("\n{prefix}")
            },
        );
    }
    let before = content[..col].trim_end();
    let after = &content[col..];
    let closing = match before.chars().next_back() {
        Some('{') => Some('}'),
        Some('[') => Some(']'),
        Some('(') => Some(')'),
        _ => None,
    };
    if context.kind == SourceInputContext::Code
        && let Some(closing) = closing
    {
        let step = if prefix.contains('\t') { "\t" } else { "    " };
        let padding = after.len() - after.trim_start_matches([' ', '\t']).len();
        let paired = after[padding..].starts_with(closing);
        return SourceEditPlan {
            range: cursor.offset..cursor.offset + if paired { padding } else { 0 },
            text: if paired {
                format!("\n{prefix}{step}\n{prefix}")
            } else {
                format!("\n{prefix}{step}")
            },
            cursor_offset: cursor.offset + 1 + prefix.len() + step.len(),
        };
    }
    SourceEditPlan::replace(cursor.selection_range(), format!("\n{prefix}"))
}

/// Remove a visible prefix only at its content boundary. Selections and literal
/// code fall through to ordinary grapheme deletion, never a semantic delete.
pub(super) fn source_backspace_plan(
    buffer: &Buffer,
    cursor: Cursor,
    context_cache: &mut SourceContextCache,
) -> Option<SourceEditPlan> {
    if cursor.has_selection() {
        return None;
    }
    let (line, col) = buffer.offset_to_line_col(cursor.offset);
    let content = buffer.line_content(line);
    let quote_len = markdown::blockquote_prefix_len(content);
    let heading = source_heading_prefix(content).filter(|range| col == range.end);
    let range = if let Some(heading) = heading {
        Some(heading)
    } else if let Some(prefix) = source_list_prefix(content).filter(|prefix| col == prefix.end) {
        Some(prefix.start..prefix.end)
    } else if quote_len > 0 && col == quote_len {
        let leading = content.len() - content.trim_start_matches(' ').len();
        Some(leading..quote_len)
    } else {
        let leading = content.len() - content.trim_start_matches(' ').len();
        let body = &content[leading..];
        let hashes = body.bytes().take_while(|byte| *byte == b'#').count();
        let spaces = body[hashes..].len() - body[hashes..].trim_start_matches([' ', '\t']).len();
        ((1..=6).contains(&hashes) && spaces > 0 && col == leading + hashes + spaces)
            .then_some(leading..leading + hashes + spaces)
    }?;
    if source_input_context(buffer, cursor.offset, context_cache) != SourceInputContext::Markdown {
        return None;
    }
    let start = buffer.line_start(line);
    Some(SourceEditPlan::replace(
        start + range.start..start + range.end,
        String::new(),
    ))
}

fn source_heading_prefix(content: &str) -> Option<Range<usize>> {
    let mut offset = 0;
    while offset < content.len() {
        let rest = &content[offset..];
        offset += rest.len() - rest.trim_start_matches([' ', '\t']).len();
        let body = &content[offset..];
        let hashes = body.bytes().take_while(|byte| *byte == b'#').count();
        let spaces = body[hashes..].len() - body[hashes..].trim_start_matches([' ', '\t']).len();
        if (1..=6).contains(&hashes) && spaces > 0 {
            return Some(offset..offset + hashes + spaces);
        }
        if let Some(width) = source_quote_width(body) {
            offset += width;
        } else if let Some(prefix) = source_list_prefix(body) {
            offset += prefix.end;
        } else {
            break;
        }
    }
    None
}

pub(super) fn slash_menu_trigger(content: &str, column: usize, in_code: bool) -> bool {
    !in_code && content.trim() == "/" && column == content.len()
}

/// Only a slash on an otherwise blank Markdown line owns a live command query.
/// The offsets are UTF-8 byte offsets into the buffer, not character indices.
pub(super) fn slash_menu_query<'a>(
    text: &'a str,
    start: usize,
    end: usize,
    markdown: bool,
    in_code: bool,
) -> Option<&'a str> {
    if !markdown
        || in_code
        || end <= start
        || !text.is_char_boundary(start)
        || !text.is_char_boundary(end)
    {
        return None;
    }
    let before = text.get(..start)?;
    if !before.rsplit('\n').next()?.trim().is_empty() || text.get(start..start + 1)? != "/" {
        return None;
    }
    let query = text.get(start + 1..end)?;
    (!query.contains('\n')).then_some(query)
}

pub(crate) fn paired_delimiter(input: &str) -> Option<(&'static str, &'static str)> {
    match input {
        "[" => Some(("[", "]")),
        "{" => Some(("{", "}")),
        "(" => Some(("(", ")")),
        "<" => Some(("<", ">")),
        "\"" => Some(("\"", "\"")),
        "'" => Some(("'", "'")),
        "［" => Some(("［", "］")),
        "【" => Some(("【", "】")),
        "｛" => Some(("｛", "｝")),
        "（" => Some(("（", "）")),
        "＜" => Some(("＜", "＞")),
        "《" => Some(("《", "》")),
        "〈" => Some(("〈", "〉")),
        "「" => Some(("「", "」")),
        "『" => Some(("『", "』")),
        "＂" => Some(("＂", "＂")),
        "＇" => Some(("＇", "＇")),
        "“" => Some(("“", "”")),
        "‘" => Some(("‘", "’")),
        _ => None,
    }
}

pub(crate) fn paired_delimiter_at(
    text: &str,
    cursor_offset: usize,
    input: &str,
) -> Option<(&'static str, &'static str)> {
    let previous = text[..cursor_offset].chars().next_back();
    let next = text[cursor_offset..].chars().next();
    let at_line_start = text[..cursor_offset]
        .rsplit_once('\n')
        .map(|(_, line)| line.trim().is_empty())
        .unwrap_or_else(|| text[..cursor_offset].trim().is_empty());
    let previous_is_escape = previous == Some('\\');
    let next_is_word = next.is_some_and(|ch| ch.is_alphanumeric() || ch == '_');
    let previous_is_word = previous.is_some_and(|ch| ch.is_alphanumeric() || ch == '_');

    match input {
        "<" => {
            if previous_is_escape || at_line_start || next_is_word {
                None
            } else {
                Some(("<", ">"))
            }
        }
        "'" | "\"" => {
            if previous_is_escape || previous_is_word || next_is_word {
                None
            } else {
                paired_delimiter(input)
            }
        }
        "[" | "{" | "(" => {
            // Don't auto-close brackets right before a word/number — the user is
            // almost always wrapping what follows (e.g. typing "(" before "abc"
            // wants "(abc)", not "()abc"). Closing is still inserted at line/word
            // boundaries.
            if next_is_word
                || paired_delimiter(input).is_some_and(|(open, close)| {
                    previous == open.chars().next() && next == close.chars().next()
                })
            {
                None
            } else {
                paired_delimiter(input)
            }
        }
        _ => paired_delimiter(input),
    }
}

pub(crate) fn missing_wiki_link_closer(text: &str, cursor_offset: usize) -> &'static str {
    let existing = text
        .get(cursor_offset..)
        .map(|suffix| suffix.chars().take_while(|ch| *ch == ']').take(2).count())
        .unwrap_or(0);
    match existing {
        0 => "]]",
        1 => "]",
        _ => "",
    }
}

fn is_auto_closing_delimiter(input: &str) -> bool {
    matches!(
        input,
        "]" | "}"
            | ")"
            | ">"
            | "\""
            | "'"
            | "］"
            | "】"
            | "｝"
            | "）"
            | "＞"
            | "》"
            | "〉"
            | "」"
            | "』"
            | "＂"
            | "＇"
            | "”"
            | "’"
    )
}

fn auto_paired_delimiter_matches(open: char, close: char) -> bool {
    matches!(
        (open, close),
        ('[', ']')
            | ('{', '}')
            | ('(', ')')
            | ('<', '>')
            | ('"', '"')
            | ('\'', '\'')
            | ('［', '］')
            | ('【', '】')
            | ('｛', '｝')
            | ('（', '）')
            | ('＜', '＞')
            | ('《', '》')
            | ('〈', '〉')
            | ('「', '」')
            | ('『', '』')
            | ('＂', '＂')
            | ('＇', '＇')
            | ('“', '”')
            | ('‘', '’')
    )
}

fn auto_paired_backspace_range(text: &str, cursor: Cursor) -> Option<Range<usize>> {
    if cursor.has_selection() {
        return None;
    }
    let offset = cursor.offset;
    if offset >= 2
        && text.get(offset - 2..offset) == Some("[[")
        && text.get(offset..offset + 2) == Some("]]")
    {
        return Some(offset - 2..offset + 2);
    }
    let (start, open) = text.get(..offset)?.char_indices().next_back()?;
    let close = text.get(offset..)?.chars().next()?;
    auto_paired_delimiter_matches(open, close).then_some(start..offset + close.len_utf8())
}

fn clamp_ime_offset(buffer: &Buffer, offset: usize, cumulative_delta: isize) -> usize {
    buffer.clamp_grapheme_offset(apply_offset_delta(offset, cumulative_delta))
}

fn source_offset_in_code_context(
    buffer: &Buffer,
    offset: usize,
    context_cache: &mut SourceContextCache,
) -> bool {
    source_input_context(buffer, offset, context_cache) != SourceInputContext::Markdown
}

impl EditorView {
    /// Whether `offset` sits in literal syntax. Source additionally protects
    /// math, YAML Front Matter, raw HTML and inline destinations.
    pub(crate) fn offset_in_code_context(&self, offset: usize) -> bool {
        if self.source_mode && self.active_file_kind().is_some_and(FileKind::is_markdown) {
            return source_offset_in_code_context(
                &self.buffer,
                offset,
                &mut self.source_syntax_context.borrow_mut(),
            );
        }
        let (line, col) = self.buffer.offset_to_line_col(offset);
        let Some(decoration) = self.decorations.get(line) else {
            return false;
        };
        if matches!(
            decoration.block_type,
            BlockType::CodeBlock | BlockType::CodeBlockFence
        ) {
            return true;
        }
        decoration.inline_spans.iter().any(|span| {
            span.kind == crate::markdown::InlineKind::InlineCode
                && span.range.start <= col
                && col <= span.range.end
        })
    }

    pub(crate) fn should_skip_auto_closing_delimiter(&self, new_text: &str) -> bool {
        if !is_auto_closing_delimiter(new_text) {
            return false;
        }

        self.buffer
            .text()
            .get(self.cursor.offset..self.cursor.offset + new_text.len())
            == Some(new_text)
    }

    pub(crate) fn auto_paired_backspace_range(&self) -> Option<Range<usize>> {
        auto_paired_backspace_range(self.buffer.text(), self.cursor)
    }

    pub(crate) fn begin_ime_composition(&mut self) {
        if self.ime_composition_snapshot.is_none() {
            self.ime_composition_snapshot = Some(self.current_snapshot());
        }
    }

    pub(crate) fn collect_multi_cursor_ime_targets_desc(
        &self,
        primary_override_range: Option<Range<usize>>,
    ) -> Vec<(Range<usize>, bool)> {
        multi_cursor_ime_targets_desc(
            &self.buffer,
            self.cursor,
            &self.extra_cursors,
            self.marked_range.clone(),
            &self.extra_marked_ranges,
            primary_override_range,
            self.source_mode,
        )
    }

    pub(crate) fn apply_multi_cursor_ime_replace(
        &mut self,
        primary_override_range: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<&Range<usize>>,
        keep_marked_ranges: bool,
    ) -> bool {
        let targets = self.collect_multi_cursor_ime_targets_desc(primary_override_range);
        let result = replace_multi_cursor_ime_targets(
            &mut self.buffer,
            targets,
            new_text,
            new_selected_range_utf16,
            keep_marked_ranges,
            self.source_mode,
        );
        self.clear_marked_ranges();
        self.cursor = result.cursor;
        self.extra_cursors = result.extra_cursors;
        self.marked_range = result.marked_range;
        self.extra_marked_ranges = result.extra_marked_ranges;
        result.range_spans_multiple_lines
    }

    pub(crate) fn finish_ime_composition(&mut self) {
        let Some(snapshot) = self.ime_composition_snapshot.take() else {
            return;
        };

        if !self.source_mode {
            self.format_table_source_before_ime_commit();
        }
        let changed = snapshot.text != self.buffer.text()
            || snapshot.cursor_offset != self.cursor.offset
            || snapshot.cursor_anchor != self.cursor.anchor
            || snapshot.extra_cursors
                != self
                    .extra_cursors
                    .iter()
                    .map(|cursor| (cursor.offset, cursor.anchor))
                    .collect::<Vec<_>>();
        if changed {
            self.push_undo_snapshot(snapshot);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use crate::cursor::Cursor;

    fn ime_state(
        primary: Range<usize>,
        extras: &[Range<usize>],
    ) -> super::MultiCursorImeReplacement {
        super::MultiCursorImeReplacement {
            cursor: Cursor {
                anchor: primary.start,
                offset: primary.end,
            },
            extra_cursors: extras
                .iter()
                .map(|range| Cursor {
                    anchor: range.start,
                    offset: range.end,
                })
                .collect(),
            marked_range: None,
            extra_marked_ranges: Vec::new(),
            range_spans_multiple_lines: false,
        }
    }

    fn ime_replace(
        buffer: &mut Buffer,
        state: &super::MultiCursorImeReplacement,
        override_range: Option<Range<usize>>,
        text: &str,
        selection_utf16: Option<&Range<usize>>,
        marked: bool,
    ) -> super::MultiCursorImeReplacement {
        let targets = super::multi_cursor_ime_targets_desc(
            buffer,
            state.cursor,
            &state.extra_cursors,
            state.marked_range.clone(),
            &state.extra_marked_ranges,
            override_range,
            true,
        );
        super::replace_multi_cursor_ime_targets(
            buffer,
            targets,
            text,
            selection_utf16,
            marked,
            true,
        )
    }

    #[test]
    fn source_ime_overlapping_composition_updates_and_commit_replace_the_union_once() {
        let mut buffer = Buffer::from_text("abcdefgh");
        let initial = ime_state(1..5, &[3..6]);
        let targets = super::multi_cursor_ime_targets_desc(
            &buffer,
            initial.cursor,
            &initial.extra_cursors,
            None,
            &[],
            None,
            true,
        );
        assert_eq!(targets, vec![(1..6, true)]);
        assert_eq!(buffer.text(), "abcdefgh");
        let preedit = ime_replace(&mut buffer, &initial, None, "X", Some(&(1..1)), true);
        assert_eq!(buffer.text(), "aXgh");
        assert_eq!(
            preedit.cursor,
            Cursor {
                anchor: 2,
                offset: 2
            }
        );
        assert_eq!(preedit.marked_range, Some(1..2));
        assert!(preedit.extra_cursors.is_empty());
        assert!(preedit.extra_marked_ranges.is_empty());
        let update = ime_replace(&mut buffer, &preedit, None, "YZ", Some(&(1..1)), true);
        assert_eq!(buffer.text(), "aYZgh");
        assert_eq!(
            update.cursor,
            Cursor {
                anchor: 2,
                offset: 2
            }
        );
        assert_eq!(update.marked_range, Some(1..3));
        let commit = ime_replace(&mut buffer, &update, None, "語", None, false);
        assert_eq!(buffer.text(), "a語gh");
        assert_eq!(
            commit.cursor,
            Cursor {
                anchor: 4,
                offset: 4
            }
        );
        assert!(commit.marked_range.is_none());
        assert!(commit.extra_cursors.is_empty());
        assert!(commit.extra_marked_ranges.is_empty());
    }

    #[test]
    fn source_ime_surviving_extra_marks_follow_their_cursors_after_merges() {
        let mut buffer = Buffer::from_text("0123456789abcdef");
        let initial = ime_state(5..9, &[11..13, 0..2, 7..10, 12..14]);
        let preedit = ime_replace(&mut buffer, &initial, None, "X", None, true);
        assert_eq!(buffer.text(), "X234XaXef");
        assert_eq!(
            preedit.cursor,
            Cursor {
                anchor: 5,
                offset: 5
            }
        );
        assert_eq!(preedit.marked_range, Some(4..5));
        assert_eq!(
            preedit.extra_cursors,
            vec![
                Cursor {
                    anchor: 1,
                    offset: 1
                },
                Cursor {
                    anchor: 7,
                    offset: 7
                }
            ]
        );
        assert_eq!(preedit.extra_marked_ranges, vec![0..1, 6..7]);
        let update = ime_replace(&mut buffer, &preedit, None, "語🙂", Some(&(1..3)), true);
        assert_eq!(buffer.text(), "語🙂234語🙂a語🙂ef");
        assert_eq!(
            update.cursor,
            Cursor {
                anchor: 13,
                offset: 17
            }
        );
        assert_eq!(update.marked_range, Some(10..17));
        assert_eq!(
            update.extra_cursors,
            vec![
                Cursor {
                    anchor: 3,
                    offset: 7
                },
                Cursor {
                    anchor: 21,
                    offset: 25
                }
            ]
        );
        assert_eq!(update.extra_marked_ranges, vec![0..7, 18..25]);
        for (cursor, mark) in update.extra_cursors.iter().zip(&update.extra_marked_ranges) {
            assert_eq!(&buffer.text()[mark.clone()], "語🙂");
            assert_eq!(&buffer.text()[cursor.selection_range()], "🙂");
        }
        let commit = ime_replace(&mut buffer, &update, None, "!", None, false);
        assert_eq!(buffer.text(), "!234!a!ef");
        assert_eq!(
            commit.cursor,
            Cursor {
                anchor: 5,
                offset: 5
            }
        );
        assert_eq!(
            commit.extra_cursors,
            vec![
                Cursor {
                    anchor: 1,
                    offset: 1
                },
                Cursor {
                    anchor: 7,
                    offset: 7
                }
            ]
        );
        assert!(commit.marked_range.is_none());
        assert!(commit.extra_marked_ranges.is_empty());
    }

    #[test]
    fn source_ime_utf16_selection_mapping_is_relative_to_the_merged_replacement() {
        let text = "漢🙂e\u{0301}";
        for (selection, expected) in [
            (
                1..3,
                Cursor {
                    anchor: 4,
                    offset: 8,
                },
            ),
            (
                2..2,
                Cursor {
                    anchor: 8,
                    offset: 8,
                },
            ),
            (
                std::ops::Range { start: 5, end: 1 },
                Cursor {
                    anchor: 11,
                    offset: 4,
                },
            ),
            (
                99..99,
                Cursor {
                    anchor: 11,
                    offset: 11,
                },
            ),
        ] {
            let mut buffer = Buffer::from_text("a🙂bc文de");
            let initial = ime_state(5..10, &[1..7]);
            let preedit = ime_replace(&mut buffer, &initial, None, text, Some(&selection), true);
            assert_eq!(buffer.text(), format!("a{text}de"));
            assert_eq!(preedit.cursor, expected, "{selection:?}");
            assert_eq!(preedit.marked_range, Some(1..11));
            assert!(preedit.extra_cursors.is_empty());
            assert!(buffer.text().is_char_boundary(preedit.cursor.anchor));
            assert!(buffer.text().is_char_boundary(preedit.cursor.offset));
        }
    }

    #[test]
    fn source_ime_resolves_primary_override_and_each_mark_before_normalizing() {
        for override_range in [1..5, std::ops::Range { start: 5, end: 1 }] {
            let mut buffer = Buffer::from_text("abcdefgh");
            let mut initial = ime_state(0..0, &[7..7]);
            initial.marked_range = Some(2..3);
            initial.extra_marked_ranges = vec![3..6];
            let preedit = ime_replace(&mut buffer, &initial, Some(override_range), "X", None, true);
            assert_eq!(buffer.text(), "aXgh");
            assert_eq!(preedit.marked_range, Some(1..2));
            assert_eq!(
                preedit.cursor,
                Cursor {
                    anchor: 2,
                    offset: 2
                }
            );
            assert!(preedit.extra_cursors.is_empty());
            assert!(preedit.extra_marked_ranges.is_empty());
        }
        let mut buffer = Buffer::from_text("a🙂bc文de");
        let mut initial = ime_state(0..0, &[0..1]);
        initial.marked_range = Some(10..11);
        initial.extra_marked_ranges = vec![5..10];
        let native_utf16 = 1..5;
        let override_range =
            super::EditorView::utf8_offset_in_text_for_utf16(buffer.text(), native_utf16.start)
                ..super::EditorView::utf8_offset_in_text_for_utf16(buffer.text(), native_utf16.end);
        assert_eq!(override_range, 1..7);
        let preedit = ime_replace(&mut buffer, &initial, Some(override_range), "X", None, true);
        assert_eq!(buffer.text(), "aXde");
        assert_eq!(preedit.marked_range, Some(1..2));
        assert!(preedit.extra_cursors.is_empty());
    }

    #[test]
    fn source_ime_marks_take_precedence_with_unmarked_extra_selection_fallback() {
        let mut buffer = Buffer::from_text("abcdefgh");
        let mut initial = ime_state(0..0, &[0..0, 7..8]);
        initial.marked_range = Some(1..5);
        initial.extra_marked_ranges = vec![3..6];
        let preedit = ime_replace(&mut buffer, &initial, None, "X", None, true);
        assert_eq!(buffer.text(), "aXgX");
        assert_eq!(preedit.marked_range, Some(1..2));
        assert_eq!(
            preedit.extra_cursors,
            vec![Cursor {
                anchor: 4,
                offset: 4
            }]
        );
        assert_eq!(preedit.extra_marked_ranges, vec![3..4]);
        let update = ime_replace(&mut buffer, &preedit, None, "YZ", None, true);
        assert_eq!(buffer.text(), "aYZgYZ");
        assert_eq!(update.marked_range, Some(1..3));
        assert_eq!(
            update.extra_cursors,
            vec![Cursor {
                anchor: 6,
                offset: 6
            }]
        );
        assert_eq!(update.extra_marked_ranges, vec![4..6]);
    }

    #[test]
    fn source_ime_independent_targets_stay_literal_through_preedit_and_commit() {
        for text in ["(", "[", "*", "$", "`", "<u", "---", "| a | b |"] {
            let mut buffer = Buffer::from_text("ab");
            let initial = ime_state(0..0, &[2..2]);
            let preedit = ime_replace(&mut buffer, &initial, None, text, None, true);
            assert_eq!(buffer.text(), format!("{text}ab{text}"), "{text:?}");
            assert_eq!(preedit.marked_range, Some(0..text.len()));
            assert_eq!(
                preedit.extra_marked_ranges,
                vec![text.len() + 2..text.len() * 2 + 2]
            );
            let commit = ime_replace(&mut buffer, &preedit, None, "[", None, false);
            assert_eq!(buffer.text(), "[ab[");
            assert_eq!(
                commit.cursor,
                Cursor {
                    anchor: 1,
                    offset: 1
                }
            );
            assert_eq!(
                commit.extra_cursors,
                vec![Cursor {
                    anchor: 4,
                    offset: 4
                }]
            );
            assert!(commit.marked_range.is_none());
            assert!(commit.extra_marked_ranges.is_empty());
        }
        let mut buffer = Buffer::from_text("ab");
        let preedit = ime_replace(
            &mut buffer,
            &ime_state(0..0, &[2..2]),
            None,
            "(",
            None,
            true,
        );
        let empty = ime_replace(&mut buffer, &preedit, None, "", None, true);
        assert_eq!(buffer.text(), "ab");
        assert_eq!(
            empty.cursor,
            Cursor {
                anchor: 0,
                offset: 0
            }
        );
        assert_eq!(
            empty.extra_cursors,
            vec![Cursor {
                anchor: 2,
                offset: 2
            }]
        );
        assert!(empty.marked_range.is_none());
        assert!(empty.extra_marked_ranges.is_empty());
    }

    #[test]
    fn source_ime_duplicate_and_boundary_carets_preserve_primary_identity() {
        let mut buffer = Buffer::from_text("abcdef");
        let initial = ime_state(
            std::ops::Range { start: 4, end: 1 },
            &[4..4, 1..1, 5..5, 5..5],
        );
        let preedit = ime_replace(&mut buffer, &initial, None, "X", None, true);
        assert_eq!(buffer.text(), "aXeXf");
        assert_eq!(
            preedit.cursor,
            Cursor {
                anchor: 2,
                offset: 2
            }
        );
        assert_eq!(preedit.marked_range, Some(1..2));
        assert_eq!(
            preedit.extra_cursors,
            vec![Cursor {
                anchor: 4,
                offset: 4
            }]
        );
        assert_eq!(preedit.extra_marked_ranges, vec![3..4]);
    }

    #[test]
    fn source_ime_native_override_deletes_only_the_requested_character_inside_a_grapheme() {
        for (text, utf16_range, expected) in [
            ("a\u{0301}bc", 1..2, "abc"),
            ("✌\u{fe0f}bc", 1..2, "✌bc"),
            ("👩\u{200d}💻bc", 2..3, "👩💻bc"),
        ] {
            let mut buffer = Buffer::from_text(text);
            let initial = ime_state(0..0, &[buffer.len()..buffer.len()]);
            let native_range =
                super::EditorView::utf8_offset_in_text_for_utf16(text, utf16_range.start)
                    ..super::EditorView::utf8_offset_in_text_for_utf16(text, utf16_range.end);
            let targets = super::multi_cursor_ime_targets_desc(
                &buffer,
                initial.cursor,
                &initial.extra_cursors,
                None,
                &[],
                Some(native_range.clone()),
                true,
            );
            assert_eq!(
                targets,
                vec![(text.len()..text.len(), false), (native_range, true)],
                "{text:?}"
            );
            let commit = ime_replace(
                &mut buffer,
                &initial,
                Some(targets[1].0.clone()),
                "",
                None,
                false,
            );
            assert_eq!(buffer.text(), expected, "{text:?}");
            assert_eq!(
                commit.extra_cursors,
                vec![Cursor {
                    anchor: expected.len(),
                    offset: expected.len()
                }]
            );
            assert!(commit.marked_range.is_none());
            assert!(commit.extra_marked_ranges.is_empty());
        }
    }

    #[test]
    fn source_ime_accent_only_override_preedit_update_and_commit_preserve_base_characters() {
        for commit_text in ["", "X"] {
            let mut buffer = Buffer::from_text("a\u{0301}bc");
            let initial = ime_state(0..0, &[5..5]);
            let native_range = super::EditorView::utf8_offset_in_text_for_utf16(buffer.text(), 1)
                ..super::EditorView::utf8_offset_in_text_for_utf16(buffer.text(), 2);
            assert_eq!(native_range, 1..3);
            let preedit = ime_replace(
                &mut buffer,
                &initial,
                Some(native_range),
                "\u{0300}",
                Some(&(0..0)),
                true,
            );
            assert_eq!(buffer.text(), "a\u{0300}bc\u{0300}");
            assert_eq!(preedit.marked_range, Some(1..3));
            assert_eq!(preedit.extra_marked_ranges, vec![5..7]);
            // Visible selection can snap to the base; composition ownership cannot.
            assert_eq!(
                preedit.cursor,
                Cursor {
                    anchor: 0,
                    offset: 0
                }
            );
            let update = ime_replace(
                &mut buffer,
                &preedit,
                None,
                "\u{0308}\u{0301}",
                Some(&(0..1)),
                true,
            );
            assert_eq!(buffer.text(), "a\u{0308}\u{0301}bc\u{0308}\u{0301}");
            assert_eq!(update.marked_range, Some(1..5));
            assert_eq!(update.extra_marked_ranges, vec![7..11]);
            assert_eq!(
                update.cursor,
                Cursor {
                    anchor: 0,
                    offset: 0
                }
            );
            assert_eq!(
                &buffer.text()[update.marked_range.clone().unwrap()],
                "\u{0308}\u{0301}"
            );
            assert_eq!(
                &buffer.text()[update.extra_marked_ranges[0].clone()],
                "\u{0308}\u{0301}"
            );
            let commit = ime_replace(&mut buffer, &update, None, commit_text, None, false);
            assert_eq!(buffer.text(), format!("a{commit_text}bc{commit_text}"));
            assert_eq!(
                commit.extra_cursors,
                vec![Cursor {
                    anchor: buffer.len(),
                    offset: buffer.len()
                }]
            );
            assert!(commit.marked_range.is_none());
            assert!(commit.extra_marked_ranges.is_empty());
        }
    }

    #[test]
    fn source_ime_new_combining_marks_do_not_claim_the_preceding_base_character() {
        let mut buffer = Buffer::from_text("ab");
        let initial = ime_state(1..1, &[2..2]);
        let preedit = ime_replace(&mut buffer, &initial, None, "\u{0301}", None, true);
        assert_eq!(buffer.text(), "a\u{0301}b\u{0301}");
        assert_eq!(preedit.marked_range, Some(1..3));
        assert_eq!(preedit.extra_marked_ranges, vec![4..6]);
        let update = ime_replace(&mut buffer, &preedit, None, "\u{0300}", None, true);
        assert_eq!(buffer.text(), "a\u{0300}b\u{0300}");
        assert_eq!(update.marked_range, Some(1..3));
        assert_eq!(update.extra_marked_ranges, vec![4..6]);
        let commit = ime_replace(&mut buffer, &update, None, "", None, false);
        assert_eq!(buffer.text(), "ab");
        assert_eq!(
            commit.cursor,
            Cursor {
                anchor: 1,
                offset: 1
            }
        );
        assert_eq!(
            commit.extra_cursors,
            vec![Cursor {
                anchor: 2,
                offset: 2
            }]
        );
        assert!(commit.marked_range.is_none());
        assert!(commit.extra_marked_ranges.is_empty());
    }

    #[test]
    fn source_ime_mark_end_does_not_snap_back_before_an_existing_combining_character() {
        let mut buffer = Buffer::from_text("a\u{0301}bc");
        let initial = ime_state(0..1, &[5..5]);
        let preedit = ime_replace(&mut buffer, &initial, None, "X", None, true);
        assert_eq!(buffer.text(), "X\u{0301}bcX");
        assert_eq!(preedit.marked_range, Some(0..1));
        assert_eq!(preedit.extra_marked_ranges, vec![5..6]);
        assert_eq!(
            preedit.cursor,
            Cursor {
                anchor: 0,
                offset: 0
            }
        );
        let update = ime_replace(&mut buffer, &preedit, None, "Y", None, true);
        assert_eq!(buffer.text(), "Y\u{0301}bcY");
        assert_eq!(update.marked_range, Some(0..1));
        let commit = ime_replace(&mut buffer, &update, None, "Z", None, false);
        assert_eq!(buffer.text(), "Z\u{0301}bcZ");
        assert!(commit.marked_range.is_none());
        assert!(commit.extra_marked_ranges.is_empty());
    }

    #[test]
    fn source_ime_adjacent_existing_marks_within_one_grapheme_remain_independent() {
        let mut buffer = Buffer::from_text("a\u{0301}bc");
        let mut initial = ime_state(0..0, &[5..5]);
        initial.marked_range = Some(1..3);
        initial.extra_marked_ranges = vec![0..1];
        let targets = super::multi_cursor_ime_targets_desc(
            &buffer,
            initial.cursor,
            &initial.extra_cursors,
            initial.marked_range.clone(),
            &initial.extra_marked_ranges,
            None,
            true,
        );
        assert_eq!(targets, vec![(1..3, true), (0..1, false)]);
        let preedit = ime_replace(&mut buffer, &initial, None, "\u{0300}", None, true);
        assert_eq!(buffer.text(), "\u{0300}\u{0300}bc");
        assert_eq!(preedit.marked_range, Some(2..4));
        assert_eq!(preedit.extra_marked_ranges, vec![0..2]);
        let update = ime_replace(&mut buffer, &preedit, None, "X", None, true);
        assert_eq!(buffer.text(), "XXbc");
        assert_eq!(update.marked_range, Some(1..2));
        assert_eq!(update.extra_marked_ranges, vec![0..1]);
        let commit = ime_replace(&mut buffer, &update, None, "!", None, false);
        assert_eq!(buffer.text(), "!!bc");
        assert_eq!(
            commit.cursor,
            Cursor {
                anchor: 2,
                offset: 2
            }
        );
        assert_eq!(
            commit.extra_cursors,
            vec![Cursor {
                anchor: 1,
                offset: 1
            }]
        );
        assert!(commit.marked_range.is_none());
        assert!(commit.extra_marked_ranges.is_empty());
    }

    #[test]
    fn source_ime_exact_marks_do_not_change_selection_snapping_or_other_mode_marks() {
        let mut source_buffer = Buffer::from_text("ab");
        let mut other_buffer = Buffer::from_text("ab");
        let targets = vec![(2..2, false), (1..1, true)];
        let source = super::replace_multi_cursor_ime_targets(
            &mut source_buffer,
            targets.clone(),
            "\u{0301}",
            Some(&(0..0)),
            true,
            true,
        );
        let other = super::replace_multi_cursor_ime_targets(
            &mut other_buffer,
            targets,
            "\u{0301}",
            Some(&(0..0)),
            true,
            false,
        );
        assert_eq!(source_buffer.text(), other_buffer.text());
        assert_eq!(source.cursor, other.cursor);
        assert_eq!(source.extra_cursors, other.extra_cursors);
        assert_eq!(source.marked_range, Some(1..3));
        assert_eq!(source.extra_marked_ranges, vec![4..6]);
        assert_eq!(other.marked_range, Some(0..3));
        assert_eq!(other.extra_marked_ranges, vec![3..6]);
    }

    #[test]
    fn source_ime_multiline_union_keeps_scroll_invalidation_and_other_modes_unchanged() {
        let mut buffer = Buffer::from_text("ab\ncd\nef");
        let initial = ime_state(1..4, &[3..6]);
        let targets = super::multi_cursor_ime_targets_desc(
            &buffer,
            initial.cursor,
            &initial.extra_cursors,
            None,
            &[],
            None,
            false,
        );
        assert_eq!(targets, vec![(3..6, false), (1..4, true)]);
        assert_eq!(buffer.text(), "ab\ncd\nef");
        let preedit = ime_replace(&mut buffer, &initial, None, "X", None, true);
        assert_eq!(buffer.text(), "aXef");
        assert!(preedit.range_spans_multiple_lines);
        assert_eq!(preedit.marked_range, Some(1..2));
        assert!(preedit.extra_marked_ranges.is_empty());
    }

    fn source_cursor(marked: &str) -> (Buffer, Cursor) {
        let offset = marked.find('¦').expect("caret marker");
        let buffer = Buffer::from_text(&marked.replacen('¦', "", 1));
        let mut cursor = Cursor::new();
        cursor.move_to(offset, &buffer);
        (buffer, cursor)
    }

    fn source_result(marked: &str, enter: bool) -> String {
        let (mut buffer, mut cursor) = source_cursor(marked);
        let mut context_cache = super::SourceContextCache::default();
        let plan = if enter {
            Some(super::source_enter_plan(
                &buffer,
                cursor,
                &mut context_cache,
            ))
        } else {
            super::source_backspace_plan(&buffer, cursor, &mut context_cache).or_else(|| {
                super::auto_paired_backspace_range(buffer.text(), cursor)
                    .map(|range| super::SourceEditPlan::replace(range, String::new()))
            })
        };
        if let Some(plan) = plan {
            assert!(buffer.text().is_char_boundary(plan.range.start));
            assert!(buffer.text().is_char_boundary(plan.range.end));
            buffer.replace(plan.range, &plan.text);
            assert!(buffer.text().is_char_boundary(plan.cursor_offset));
            cursor.move_to(plan.cursor_offset, &buffer);
        } else {
            cursor.backspace(&mut buffer);
        }
        let mut result = buffer.text().to_string();
        result.insert(cursor.offset, '¦');
        result
    }

    #[test]
    fn source_math_blocks_keep_enter_and_backspace_literal_without_parsed_metadata() {
        for (open, close) in [("$$", "$$"), ("\\[", "\\]")] {
            for body in [
                "- x",
                "- [x] 文🙂",
                "9. x",
                "> x",
                "- ",
                "- [ ] ",
                "> ",
                "```rust",
                "~~~",
            ] {
                let input = format!("{open}\n{body}¦\n{close}");
                assert_eq!(
                    source_result(&input, true),
                    input.replace('¦', "\n¦"),
                    "{input:?}"
                );
            }
            for body in ["- ¦x", "- [x] ¦文🙂", "9. ¦x", "> ¦x", "### ¦x"] {
                let input = format!("{open}\n{body}\n{close}");
                let (buffer, cursor) = source_cursor(&input);
                for lazy_unparsed in [false, true] {
                    let mut cache = super::SourceContextCache::default();
                    if lazy_unparsed {
                        cache.markdown_decorations = Some(vec![
                                crate::markdown::LineDecoration::default();
                                buffer.line_count()
                            ]);
                    }
                    assert!(super::source_offset_in_code_context(
                        &buffer,
                        cursor.offset,
                        &mut cache
                    ));
                    assert_eq!(
                        super::source_backspace_plan(&buffer, cursor, &mut cache),
                        None,
                        "{input:?}"
                    );
                }
                assert_eq!(source_result(&input, false), input.replace(" ¦", "¦"));
            }
            let after = format!("{open}\n- x\n{close}\n- after¦");
            assert_eq!(source_result(&after, true), after.replace('¦', "\n- ¦"));
            // Markdown-looking content in the math body must not leave behind
            // list containers or an auto-closing code-fence state.
            let after = format!("{open}\n- x\n```rust\n{close}\n- after¦");
            assert_eq!(source_result(&after, true), after.replace('¦', "\n- ¦"));
        }
    }

    #[test]
    fn source_math_scanner_respects_delimiters_code_and_containers() {
        for (input, prefix) in [
            ("$$\n- x¦", ""), // In-progress math remains literal before its closer.
            ("\\[\n- x¦", ""),
            ("$$x +\n- x¦\n$$", ""),
            ("$$\n\\]\n- x¦\n$$", ""),
            ("\\[\n$$\n- x¦\n\\]", ""),
            ("$$\n\\$$\n- x¦\n$$", ""), // An escaped closer cannot end math.
            ("\\[\n\\\\]\n- x¦\n\\]", ""),
            ("> $$\n> - x¦\n> $$", "> "),
            ("- $$\n  - x¦\n  $$", "  "),
            ("- \\[\n  - x¦\n  \\]", "  "),
        ] {
            assert_eq!(
                source_result(input, true),
                input.replace('¦', &format!("\n{prefix}¦")),
                "{input:?}"
            );
        }
        for input in [
            "```\n$$\n```\n- x¦",
            "~~~\n\\[\n~~~\n- x¦",
            "    $$\n- x¦",
            "`$$`\n- x¦",
            "\\$$\n- x¦",
            "\\\\[\n- x¦",
            "$$x^2$$\n- x¦",
            "\\[x^2\\]\n- x¦",
            "- \\[\n  - body\n  \\]\n- x¦",
        ] {
            assert_eq!(
                source_result(input, true),
                input.replace('¦', "\n- ¦"),
                "{input:?}"
            );
        }
    }

    #[test]
    fn source_math_context_invalidation_ignores_stale_metadata_and_reuses_checkpoints() {
        for (open, close) in [("$$", "$$"), ("\\[", "\\]")] {
            let prefix = "paragraph\n".repeat(128);
            let text = format!(
                "{prefix}{open}\n{}- x\n{close}\n- after",
                "literal\n".repeat(128)
            );
            let mut buffer = Buffer::from_text(&text);
            let mut cache = super::SourceContextCache::default();
            cache.markdown_decorations = Some(crate::markdown::parse_decorations(&text));
            let body_line = 257;
            let query = |buffer: &Buffer, cache: &mut super::SourceContextCache, line: usize| {
                let offset = buffer.line_end(line);
                super::source_enter_plan(
                    buffer,
                    Cursor {
                        offset,
                        anchor: offset,
                    },
                    cache,
                )
                .text
            };
            assert_eq!(query(&buffer, &mut cache, body_line), "\n");
            let scanned = cache.scanned_lines;
            assert_eq!(query(&buffer, &mut cache, body_line), "\n");
            assert_eq!(cache.scanned_lines, scanned);
            // Invalidate before a same-length edit: old canonical Math previews
            // are deliberately retained, but cannot override the live scanner.
            let start = buffer.line_start(128);
            cache.invalidate_from(128);
            buffer.replace(start..start + open.len(), "xx");
            assert_eq!(query(&buffer, &mut cache, body_line), "\n- ");
            let scanned = cache.scanned_lines;
            cache.invalidate_from(128);
            buffer.replace(start..start + 2, open);
            assert_eq!(query(&buffer, &mut cache, body_line), "\n");
            assert_eq!(cache.scanned_lines, scanned + 130);
            // Local edits use the retained boundary before the math-body row.
            let scanned = cache.scanned_lines;
            cache.invalidate_from(body_line);
            buffer.insert(buffer.line_end(body_line), "文");
            assert_eq!(query(&buffer, &mut cache, body_line), "\n");
            assert_eq!(cache.scanned_lines, scanned + 1);
            let scanned = cache.scanned_lines;
            cache.invalidate_from(128);
            buffer.insert(start, "\n");
            assert_eq!(query(&buffer, &mut cache, body_line + 1), "\n");
            assert_eq!(cache.scanned_lines, scanned + 131);
            // Removing/restoring a closer updates subsequent list assistance,
            // even though that line lies beyond the edited delimiter.
            let close_line = body_line + 2;
            let after_line = body_line + 3;
            assert_eq!(query(&buffer, &mut cache, after_line), "\n- ");
            let end = buffer.line_start(close_line);
            cache.invalidate_from(close_line);
            buffer.replace(end..end + close.len(), "xx");
            assert_eq!(query(&buffer, &mut cache, after_line), "\n");
            cache.invalidate_from(close_line);
            buffer.replace(end..end + 2, close);
            assert_eq!(query(&buffer, &mut cache, after_line), "\n- ");
        }
    }

    #[test]
    fn source_list_contained_fences_keep_their_contents_literal() {
        for (input, prefix) in [
            ("- ~~~\n  - value¦\n  ~~~", "  "),
            ("- ~~~\n  - [ ] value¦\n  ~~~", "  "),
            ("1. ```md\n   2. value¦\n   ```", "   "),
            ("> - ~~~\n>   - value¦\n>   ~~~", ">   "),
            ("- > ~~~\n  > - value¦\n  > ~~~", "  > "),
            ("- parent\n    - ~~~~\n      - value¦\n      ~~~~", "      "),
            ("- ~~~\n\n  - value¦\n  ~~~", "  "),
            ("- ~~~~\n  ~~~\n  - value¦\n  ~~~~", "  "),
        ] {
            assert_eq!(
                source_result(input, true),
                input.replace('¦', &format!("\n{prefix}¦")),
                "{input:?}"
            );
            let at_prefix = input.replace("value¦", "¦value");
            let (buffer, cursor) = source_cursor(&at_prefix);
            let mut cache = super::SourceContextCache::default();
            assert!(super::source_offset_in_code_context(
                &buffer,
                cursor.offset,
                &mut cache
            ));
            assert_eq!(
                super::source_backspace_plan(&buffer, cursor, &mut cache),
                None,
                "{at_prefix:?}"
            );
            assert_eq!(
                source_result(&at_prefix, false),
                at_prefix.replace(" ¦value", "¦value")
            );
        }
    }

    #[test]
    fn source_deep_lists_are_not_indented_code() {
        for (input, expected) in [
            ("- one\n    - two¦", "- one\n    - two\n    - ¦"),
            (
                "- one\n    - two\n        - three¦",
                "- one\n    - two\n        - three\n        - ¦",
            ),
            (
                "- one\n    - two\n        - three\n            - four¦",
                "- one\n    - two\n        - three\n            - four\n            - ¦",
            ),
            (
                "> - one\n>     - [x] two¦",
                "> - one\n>     - [x] two\n>     - [ ] ¦",
            ),
            ("1. one\n    9) two¦", "1. one\n    9) two\n    10) ¦"),
            ("- one\n\t- two¦", "- one\n\t- two\n\t- ¦"),
            (
                "- ~~~\n  raw\n  ~~~\n    - two¦",
                "- ~~~\n  raw\n  ~~~\n    - two\n    - ¦",
            ),
        ] {
            assert_eq!(source_result(input, true), expected, "{input:?}");
        }
        for (input, expected) in [
            ("- one\n    - ¦two", "- one\n    ¦two"),
            (
                "- one\n    - two\n        - [ ] ¦three",
                "- one\n    - two\n        ¦three",
            ),
            ("> - one\n>     - ¦two", "> - one\n>     ¦two"),
        ] {
            assert_eq!(source_result(input, false), expected, "{input:?}");
        }
        for (input, prefix) in [
            ("        - literal¦", "        "),
            ("- one\n\n      - literal¦", "      "),
        ] {
            assert_eq!(
                source_result(input, true),
                input.replace('¦', &format!("\n{prefix}¦"))
            );
        }
    }

    #[test]
    fn source_fences_in_containers_close_without_duplicate_closers() {
        for (input, expected) in [
            ("- ~~~¦", "- ~~~\n  ¦\n  ~~~"),
            ("> - ~~~¦", "> - ~~~\n>   ¦\n>   ~~~"),
            ("- > ```rust¦", "- > ```rust\n  > ¦\n  > ```"),
            ("- ~~~¦\n  code\n  ~~~", "- ~~~\n  ¦\n  code\n  ~~~"),
            ("- ~~~¦\n- ~~~", "- ~~~\n  ¦\n- ~~~"),
        ] {
            assert_eq!(source_result(input, true), expected, "{input:?}");
        }
    }

    #[test]
    fn source_plain_enter_and_selections_do_not_scan_the_prefix() {
        let text = format!(
            "~~~\n{}ordinary text\n| a | b |\na<br>b\n",
            "literal\n".repeat(4096)
        );
        let buffer = Buffer::from_text(&text);
        let mut cache = super::SourceContextCache::default();
        for line in 4097..buffer.line_count() {
            let cursor = Cursor {
                offset: buffer.line_end(line),
                anchor: buffer.line_end(line),
            };
            assert_eq!(
                super::source_enter_plan(&buffer, cursor, &mut cache).text,
                "\n"
            );
            assert_eq!(
                super::source_backspace_plan(&buffer, cursor, &mut cache),
                None
            );
        }
        let cursor = Cursor {
            offset: buffer.len(),
            anchor: 0,
        };
        assert_eq!(
            super::source_enter_plan(&buffer, cursor, &mut cache).text,
            "\n"
        );
        assert_eq!(cache.scanned_lines, 0);
        assert!(cache.checkpoints.is_empty());
        assert!(cache.before.is_none());
    }

    #[test]
    fn source_context_cache_reuses_the_prefix_for_nearby_edits_and_pair_queries() {
        let mut buffer = Buffer::from_text(&format!(
            "{}- root\n    - value",
            "paragraph\n".repeat(1024)
        ));
        let line = 1025;
        let mut cache = super::SourceContextCache::default();
        let offset = buffer.line_end(line);
        let cursor = Cursor {
            offset,
            anchor: offset,
        };
        assert_eq!(
            super::source_enter_plan(&buffer, cursor, &mut cache).text,
            "\n    - "
        );
        assert_eq!(cache.scanned_lines, line + 1);
        let scanned = cache.scanned_lines;
        for _ in 0..16 {
            assert!(!super::source_offset_in_code_context(
                &buffer, offset, &mut cache
            ));
            assert_eq!(
                paired_delimiter_at(buffer.text(), offset, "("),
                Some(("(", ")"))
            );
            assert_eq!(
                super::source_enter_plan(&buffer, cursor, &mut cache).text,
                "\n    - "
            );
            let start = buffer.line_start(line) + 6;
            assert!(
                super::source_backspace_plan(
                    &buffer,
                    Cursor {
                        offset: start,
                        anchor: start
                    },
                    &mut cache
                )
                .is_some()
            );
        }
        assert_eq!(cache.scanned_lines, scanned);
        assert!(cache.checkpoints.len() <= line / super::SOURCE_CONTEXT_CHECKPOINT_LINES + 1);
        for _ in 0..8 {
            let offset = buffer.line_end(line);
            cache.invalidate_from(line);
            buffer.insert(offset, "()");
            assert!(!super::source_offset_in_code_context(
                &buffer,
                offset + 1,
                &mut cache
            ));
        }
        assert_eq!(cache.scanned_lines, scanned + 8);
        // Appending a structural newline retains the exact boundary before the
        // edited line, not just the preceding sparse checkpoint.
        let offset = buffer.line_end(line);
        cache.invalidate_from(line);
        buffer.insert(offset, "\n    - next");
        assert!(!super::source_offset_in_code_context(
            &buffer,
            buffer.len(),
            &mut cache
        ));
        assert_eq!(cache.scanned_lines, scanned + 10);
    }

    #[test]
    fn source_context_invalidation_handles_equal_length_fence_edits_and_line_shifts() {
        let prefix = "paragraph\n".repeat(128);
        let text = format!("{prefix}- ~~~\n  - value\n  ~~~\n    - nested");
        let mut buffer = Buffer::from_text(&text);
        let mut cache = super::SourceContextCache::default();
        assert!(cache.line_context(&buffer, 129).code);
        let scanned = cache.scanned_lines;
        let start = buffer.line_start(128);
        cache.invalidate_from(128);
        buffer.replace(start..start + 5, "- txt");
        assert!(!cache.line_context(&buffer, 129).code);
        assert_eq!(cache.scanned_lines, scanned + 2);
        cache.invalidate_from(128);
        buffer.replace(start..start + 5, "- ~~~");
        assert!(cache.line_context(&buffer, 129).code);
        assert_eq!(cache.scanned_lines, scanned + 4);
        assert!(!cache.line_context(&buffer, 131).code);
        let scanned = cache.scanned_lines;
        cache.invalidate_from(128);
        buffer.insert(start, "\n");
        assert!(cache.line_context(&buffer, 130).code);
        assert_eq!(cache.scanned_lines, scanned + 3);
        cache.invalidate_from(128);
        buffer.delete(start..start + 1);
        assert!(cache.line_context(&buffer, 129).code);
        assert_eq!(cache.scanned_lines, scanned + 5);
        // Tab switches/undo may replace the buffer without changing its size.
        cache.clear();
        let other = Buffer::from_text(&text.replacen("- ~~~", "- txt", 1));
        assert!(!cache.line_context(&other, 129).code);
    }

    #[test]
    fn source_code_context_includes_inline_code_without_losing_the_cached_prefix() {
        let buffer = Buffer::from_text("- item\n    - `文字` tail");
        let mut cache = super::SourceContextCache::default();
        let offset = buffer.text().find('字').unwrap();
        assert!(super::source_offset_in_code_context(
            &buffer, offset, &mut cache
        ));
        let scanned = cache.scanned_lines;
        assert!(!super::source_offset_in_code_context(
            &buffer,
            buffer.len(),
            &mut cache
        ));
        assert_eq!(cache.scanned_lines, scanned);
    }

    #[test]
    fn source_enter_continues_only_the_current_list_task_or_quote() {
        for (input, expected) in [
            ("- hello¦", "- hello\n- ¦"),
            ("* hello¦", "* hello\n* ¦"),
            ("+ [x] 完成¦", "+ [x] 完成\n+ [ ] ¦"),
            ("* [X] done¦", "* [X] done\n* [ ] ¦"),
            ("- [ ] todo¦", "- [ ] todo\n- [ ] ¦"),
            ("8) 文¦", "8) 文\n9) ¦"),
            ("09.\titem¦", "09.\titem\n10.\t¦"),
            ("2. text¦\n99. untouched", "2. text\n3. ¦\n99. untouched"),
            ("> > 引用¦", "> > 引用\n> > ¦"),
            ("> - [x] 文¦", "> - [x] 文\n> - [ ] ¦"),
            ("  - nested¦", "  - nested\n  - ¦"),
            ("- あ¦いう", "- あ\n- ¦いう"),
            ("- a\u{0301}¦b", "- a\u{0301}\n- ¦b"),
            ("-¦ item", "-\n¦ item"),
        ] {
            assert_eq!(source_result(input, true), expected, "{input:?}");
        }
    }

    #[test]
    fn source_thematic_breaks_do_not_trigger_list_assistance() {
        for marker in ["- - -", "* * *", "_ _ _", "-\t-\t-", "***"] {
            let end = format!("{marker}¦");
            assert_eq!(source_result(&end, true), format!("{marker}\n¦"));
            let quoted = format!("> {marker}¦");
            assert_eq!(source_result(&quoted, true), format!("> {marker}\n> ¦"));
            if let Some((first, rest)) = marker.split_once([' ', '\t']) {
                let at_prefix = format!("{first} ¦{rest}");
                let (buffer, cursor) = source_cursor(&at_prefix);
                let mut cache = super::SourceContextCache::default();
                assert_eq!(
                    super::source_backspace_plan(&buffer, cursor, &mut cache),
                    None,
                    "{at_prefix:?}"
                );
                assert_eq!(cache.scanned_lines, 0);
            }
        }
        assert_eq!(source_result("- - text¦", true), "- - text\n- ¦");
    }

    #[test]
    fn source_enter_exits_empty_prefixes_without_renumbering_neighbors() {
        for (input, expected) in [
            ("- ¦", "\n¦"),
            ("+ [ ] ¦", "\n¦"),
            ("3. ¦\n42. keep", "\n¦\n42. keep"),
            ("  - ¦", "- ¦"),
            ("   3. ¦", "3. ¦"),
            ("> - ¦", "> \n¦"),
            (">   - [ ] ¦", "> - [ ] ¦"),
            ("> ¦", "\n¦"),
            ("> > ¦", "\n¦"),
        ] {
            assert_eq!(source_result(input, true), expected, "{input:?}");
        }
    }

    #[test]
    fn source_enter_closes_only_an_unclosed_opening_fence() {
        for (input, expected) in [
            ("```rust¦", "```rust\n¦\n```"),
            ("  ````言語¦", "  ````言語\n  ¦\n  ````"),
            ("~~~¦", "~~~\n¦\n~~~"),
            ("前\n```¦\ntail", "前\n```\n¦\n```\ntail"),
            ("```rust¦\ncode\n```", "```rust\n¦\ncode\n```"),
            ("```rust¦\ncode\n````", "```rust\n¦\ncode\n````"),
            ("```\ncode\n```¦", "```\ncode\n```\n¦"),
            ("``¦`", "``\n¦`"),
            ("    ```¦", "    ```\n    ¦"),
            ("```bad`info¦", "```bad`info\n¦"),
        ] {
            assert_eq!(source_result(input, true), expected, "{input:?}");
        }
    }

    #[test]
    fn source_markdown_prefixes_are_literal_inside_code() {
        for (input, prefix) in [
            ("```md\n- 文¦\n```", ""),
            ("~~~\n- 文¦", ""),
            ("````\n```\n- 文¦\n````", ""),
            ("> ```\n> - 文¦\n> ```", "> "),
            ("```\n> ```\n- 文¦", ""),
            ("> ```\n> > ```\n> - 文¦", "> "),
            ("    - 文¦", "    "),
            ("\t- 文¦", "\t"),
            (" \t- 文¦", " \t"),
            ("- `文¦字`", ""),
            ("> ``文`¦字``", ""),
            (">     - 文¦", ">     "),
        ] {
            assert_eq!(
                source_result(input, true),
                input.replace('¦', &format!("\n{prefix}¦")),
                "{input:?}"
            );
        }
        for input in [
            "```md\n- ¦文\n```",
            "~~~\n- [ ] ¦文",
            "```\n# ¦文",
            "> ```\n> - ¦文\n> ```",
            "```\n> ```\n- ¦文",
            "    - ¦文",
            "\t- ¦文",
            " \t- ¦文",
        ] {
            let (buffer, cursor) = source_cursor(input);
            assert_eq!(
                super::source_backspace_plan(
                    &buffer,
                    cursor,
                    &mut super::SourceContextCache::default()
                ),
                None,
                "{input:?}"
            );
        }
        assert_eq!(source_result("```\n```\n- x¦", true), "```\n```\n- x\n- ¦");
        assert_eq!(source_result("> ```\n- x¦", true), "> ```\n- x\n- ¦");
    }

    #[test]
    fn source_backspace_removes_only_the_visible_prefix_and_retains_content() {
        for (input, expected) in [
            ("- ¦内容", "¦内容"),
            ("* [x] ¦内容", "¦内容"),
            ("+ [X] ¦", "¦"),
            ("8) ¦内容\n99) keep", "¦内容\n99) keep"),
            ("  - ¦a\u{0301}", "  ¦a\u{0301}"),
            ("> - [ ] ¦文", "> ¦文"),
            ("> ¦引用", "¦引用"),
            ("  > ¦引用", "  ¦引用"),
            ("### ¦見出し", "¦見出し"),
        ] {
            assert_eq!(source_result(input, false), expected, "{input:?}");
        }
        assert_eq!(source_result("- a¦b", false), "- ¦b");
        assert_eq!(source_result("¦- text", false), "¦- text");
    }

    #[test]
    fn source_tables_and_html_breaks_are_ordinary_text() {
        for (input, enter, expected) in [
            ("| a¦ | b |\n|---|---|", true, "| a\n¦ | b |\n|---|---|"),
            ("a<br>¦b", true, "a<br>\n¦b"),
            ("a<br>¦b", false, "a<br¦b"),
            ("| a |¦ b |", false, "| a ¦ b |"),
            ("文🙂¦", false, "文¦"),
            ("a\u{0301}¦b", false, "¦b"),
        ] {
            assert_eq!(source_result(input, enter), expected, "{input:?}");
        }
    }

    #[test]
    fn source_selections_replace_exactly_the_selected_utf8_text() {
        for (text, range, expected) in [
            ("- 文🙂 tail", 2..9, "- \n tail"),
            ("- [x] 文", 0..6, "\n文"),
            ("```rust", 0..7, "\n"),
            ("2. 文\n99. untouched", 3..7, "2. \n99. untouched"),
        ] {
            for reversed in [false, true] {
                let mut buffer = Buffer::from_text(text);
                let cursor = Cursor {
                    offset: if reversed { range.start } else { range.end },
                    anchor: if reversed { range.end } else { range.start },
                };
                let mut context_cache = super::SourceContextCache::default();
                assert_eq!(
                    super::source_backspace_plan(&buffer, cursor, &mut context_cache),
                    None
                );
                let mut deleted = Buffer::from_text(buffer.text());
                let mut deletion_cursor = cursor;
                deletion_cursor.backspace(&mut deleted);
                assert_eq!(
                    deleted.text(),
                    format!("{}{}", &text[..range.start], &text[range.end..])
                );
                assert_eq!(deletion_cursor.offset, range.start);
                let plan = super::source_enter_plan(&buffer, cursor, &mut context_cache);
                assert_eq!(plan.range, range);
                assert_eq!(plan.cursor_offset, range.start + 1);
                assert_eq!(plan.text, "\n");
                buffer.replace(plan.range, &plan.text);
                assert_eq!(buffer.text(), expected);
            }
        }
    }

    #[test]
    fn source_input_context_distinguishes_block_and_inline_literals() {
        use super::SourceInputContext as C;
        for (input, expected) in [
            ("前¦🙂", C::Markdown),
            ("- `文¦字` tail", C::Code),
            ("``文`¦字``", C::Code),
            ("`$文¦字$`", C::Code),
            ("a $文¦字$ tail", C::Math),
            ("a $`文¦字`$ tail", C::Math),
            ("a \\(文¦字\\) tail", C::Math),
            ("a \\$文¦字$ tail", C::Markdown),
            ("`text`¦", C::Markdown),
            ("$text$¦", C::Markdown),
            ("a <b>¦text", C::Markdown),
            ("a <span title=\"文¦字>\">", C::Html),
            ("a <https://例.测试/文¦字>", C::Html),
            ("a <name@例.测试¦>", C::Html),
            ("[文](url(a¦b) \"title\")", C::Html),
            ("![文](<文¦字> \"title\")", C::Html),
            ("[id]: https://例/文¦字", C::Html),
            ("a <code>文¦字</code>", C::Html),
            ("a <span title=\"文¦字", C::Html),
            ("---\ntitle: 文¦字\n---", C::FrontMatter),
            ("\u{feff}---\n- 文¦字\n...", C::FrontMatter),
            ("---\nx: y\n---\n- 文¦字", C::Markdown),
            ("paragraph\n---\n- 文¦字", C::Markdown),
            ("<!--\n- 文¦字\n-->", C::Html),
            ("<?instruction\n- 文¦字\n?>", C::Html),
            ("<![CDATA[\n- 文¦字\n]]>", C::Html),
            ("<!DOCTYPE\n- 文¦字\n>", C::Html),
            ("<PrE>\n- 文¦字\n</pRE>", C::Html),
            ("<pre title=\"</pre>\">\n- 文¦字\n</pre>", C::Html),
            ("<pre title=\"first\n</pre>\">\n- 文¦字\n</pre>", C::Html),
            ("<div\n title=\"</div>\">\n- 文¦字\n</div>", C::Html),
            ("<div title=\"first\n</div>\">\n- 文¦字\n</div>", C::Html),
            ("<div\n title=\"x\">\n</div>\n- 文¦字", C::Markdown),
            ("<script>\n</scripture>\n- 文¦字\n</script>", C::Html),
            ("<div>\n<div>\n</div>\n\n- 文¦字\n</div>", C::Html),
            ("<div>\n- raw\n</div>\n- 文¦字", C::Markdown),
            ("<hr>\n- 文¦字\n\nnormal", C::Html),
            ("<hr>\n\n- 文¦字", C::Markdown),
            ("> - <div>\n>   - 文¦字\n>   </div>", C::Html),
            ("- ~~~\n  <div>文¦字\n  ~~~", C::Code),
            ("$$\n<div>文¦字\n$$", C::Math),
            ("---\n<div>文¦字\n---", C::FrontMatter),
        ] {
            let (buffer, cursor) = source_cursor(input);
            let mut cache = super::SourceContextCache::default();
            assert_eq!(
                super::source_input_context(&buffer, cursor.offset, &mut cache),
                expected,
                "{input:?}"
            );
            assert_eq!(
                super::source_offset_in_code_context(&buffer, cursor.offset, &mut cache),
                expected != C::Markdown,
                "{input:?}"
            );
        }
        // A partially typed inline opener is not itself a literal region.
        for input in [
            "`¦", "``¦", "$¦", "a \\(¦", "<¦", "a <di¦", "[x](¦", "\\`x¦`",
        ] {
            let (buffer, cursor) = source_cursor(input);
            assert_eq!(
                super::source_input_context(
                    &buffer,
                    cursor.offset,
                    &mut super::SourceContextCache::default()
                ),
                C::Markdown,
                "{input:?}"
            );
        }
    }

    #[test]
    fn source_html_tag_assistance_distinguishes_tags_from_literal_markup() {
        for (input, allowed) in [
            ("<u¦", true),
            ("<u¦></u>", true),
            ("文🙂 <u¦</u>", true),
            ("<div>\n<u¦", true),
            ("<div><u¦</u></div>", true),
            ("text <span><u¦</u></span>", true),
            ("<div>\n<span><u¦</u></span>\n</div>", true),
            ("> - <div>\n>   <u¦\n>   </div>", true),
            ("- parent\n    - <u¦", true),
            ("<!-- <u¦</u> -->", false),
            ("<!--\n<u¦</u>\n-->", false),
            ("text <!--\n<u¦</u>\n-->", false),
            ("<!DOCTYPE html [ '<u¦</u>' ]>", false),
            ("<!DOCTYPE html [\n<!ENTITY x 'value'>\n<u¦</u>\n]>", false),
            ("<![CDATA[ <u¦</u> ]]>", false),
            ("<?data <u¦</u> ?>", false),
            ("<script>const s = '<u¦</u>';</script>", false),
            ("<script>\nconst s = '<u¦</u>';\n</script>", false),
            ("<SCRIPT>\nconst s = '<u¦</u>';\n</SCRIPT>", false),
            (
                "<script>\n</scripture>\nconst s = '<u¦</u>';\n</script>",
                false,
            ),
            (
                "<style>\n.x::after { content: '<u¦</u>'; }\n</style>",
                false,
            ),
            ("<textarea>\n<u¦</u>\n</textarea>", false),
            ("<pre>\n<u¦</u>\n</pre>", false),
            ("<code><u¦</u></code>", false),
            ("text <code>\n<u¦</u>\n</code>", false),
            ("<div>\n<!--\n<u¦</u>\n-->\n</div>", false),
            (
                "<div>\n<script>\nconst s = '<u¦</u>';\n</script>\n</div>",
                false,
            ),
            ("<div>\n<pre>\n<u¦</u>\n</pre>\n</div>", false),
            ("<div>\n<code>\n<u¦</u>\n</code>\n</div>", false),
            ("<div title=\"<u¦</u>\">", false),
            ("<div\n title=\"<u¦</u>\">", false),
            ("<div title=\"文🙂\n<u¦</u>\">", false),
            ("text <span title=\"<u¦</u>\">", false),
            ("text <span title=\"first\n<u¦</u>\">", false),
            ("<div>\n<span title=\"first\n<u¦</u>\">\n</div>", false),
            ("> <div\n> title=\"<u¦</u>\">", false),
            ("> <div title=\"first\n> <u¦</u>\">", false),
            ("> <div>\n> <span\n> title=\"<u¦</u>\">\n> </div>", false),
            ("~~~\n<u¦</u>\n~~~", false),
            ("$$\n<u¦</u>\n$$", false),
            ("---\ntext: '<u¦</u>'\n---", false),
            ("    <u¦</u>", false),
            ("text `<u¦</u>`", false),
            ("text $<u¦</u>$", false),
            ("text \\(<u¦</u>\\)", false),
            ("[label](path/<u¦</u>)", false),
            ("[label](\n<u¦</u>)", false),
            ("[id]: https://example/<u¦</u>", false),
            ("<https://example/<u¦</u>>", false),
            ("<user@example/<u¦</u>>", false),
            ("https://example/<u¦</u>", false),
            ("text \\<u¦</u>", false),
            ("<!-- literal --> <u¦", true),
            ("<div><!-- literal --> <u¦</div>", true),
            ("<div>\n<!-- literal -->\n<u¦\n</div>", true),
            ("<script>\nx\n</script>\n<u¦", true),
            ("<div>\n<script>\nx\n</script>\n<u¦\n</div>", true),
            ("text <code>\nx\n</code>\n<u¦", true),
            ("text <span title=\"first\nlast\"> <u¦", true),
            ("text `<script>`\n<u¦", true),
            ("text $<script>$\n<u¦", true),
            ("~~~\n<script>\n~~~\n<u¦", true),
        ] {
            let (buffer, cursor) = source_cursor(input);
            let tag_start = buffer.text()[..cursor.offset].rfind('<').unwrap();
            let mut cache = super::SourceContextCache::default();
            assert_eq!(
                super::source_html_tag_assistance_allowed(&buffer, tag_start, &mut cache),
                allowed,
                "{input:?}"
            );
            let scanned = cache.scanned_lines;
            assert_eq!(
                super::source_html_tag_assistance_allowed(&buffer, tag_start, &mut cache),
                allowed,
                "hot {input:?}"
            );
            assert_eq!(cache.scanned_lines, scanned);
        }
    }

    #[test]
    fn source_html_tag_assistance_reuses_and_invalidates_literal_boundaries() {
        let prefix = "paragraph\n".repeat(4096);
        let (mut buffer, cursor) = source_cursor(&format!(
            "{prefix}<div>\n<script>\n{}const s = '<u¦</u>';\n</script>\n</div>",
            "literal\n".repeat(128)
        ));
        let tag_start = buffer.text()[..cursor.offset].rfind('<').unwrap();
        let line = buffer.offset_to_line_col(tag_start).0;
        let mut cache = super::SourceContextCache::default();
        assert!(!super::source_html_tag_assistance_allowed(
            &buffer, tag_start, &mut cache
        ));
        let scanned = cache.scanned_lines;
        assert_eq!(scanned, line);
        for _ in 0..32 {
            cache.invalidate_from(line);
            buffer.insert(buffer.line_end(line), "文🙂");
            assert!(!super::source_html_tag_assistance_allowed(
                &buffer, tag_start, &mut cache
            ));
        }
        assert_eq!(cache.scanned_lines, scanned);
        // Same byte length, different lexical state inside an ordinary div.
        let opener = buffer.line_start(4097);
        cache.invalidate_from(4097);
        buffer.replace(opener..opener + 8, "<span>  ");
        assert!(super::source_html_tag_assistance_allowed(
            &buffer, tag_start, &mut cache
        ));
        cache.invalidate_from(4097);
        buffer.replace(opener..opener + 8, "<script>");
        assert!(!super::source_html_tag_assistance_allowed(
            &buffer, tag_start, &mut cache
        ));
        assert_eq!(cache.lookahead_scanned_lines.get(), 0);
    }

    #[test]
    fn source_context_boundaries_separate_adjacent_literal_blocks() {
        for (text, expected) in [
            (
                "~~~\n(\n~~~\n~~~\n)\n~~~",
                vec![true, false, true, true, false, true],
            ),
            (
                "$$\n(\n$$\n$$\n)\n$$",
                vec![true, false, true, true, false, true],
            ),
            (
                "\\[\n(\n\\]\n\\[\n)\n\\]",
                vec![true, false, true, true, false, true],
            ),
            (
                "<pre>\n(\n</pre>\n<pre>\n)\n</pre>",
                vec![true, false, true, true, false, true],
            ),
            (
                "<!--\n(\n-->\n<!--\n)\n-->",
                vec![true, false, true, true, false, true],
            ),
            (
                "> - ~~~\n>   (\n>   ~~~\n> - ~~~\n>   )\n>   ~~~",
                vec![true, false, true, true, false, true],
            ),
            (
                "~~~\n~~~shorter-is-content\nbody\n~~~",
                vec![true, false, false, true],
            ),
            ("````\n```\nbody\n````", vec![true, false, false, true]),
            ("$$x$$\n\\[x\\]\ntext", vec![true, true, false]),
            ("---\nx: y\n---\ntext", vec![true, false, true, false]),
            ("text `<pre>`\nordinary", vec![false, false]),
            ("text $<pre>$\nordinary", vec![false, false]),
            (
                "<div>\n<script>\n(\n</script>\n<script>\n)\n</script>\n</div>",
                vec![true, true, false, true, true, false, true, true],
            ),
        ] {
            let buffer = Buffer::from_text(text);
            let mut cache = super::SourceContextCache::default();
            for (line, expected) in expected.into_iter().enumerate() {
                assert_eq!(
                    super::source_context_boundary_before(&buffer, line, &mut cache),
                    expected,
                    "line {line}: {text:?}"
                );
                let scanned = cache.scanned_lines;
                assert_eq!(
                    super::source_context_boundary_before(&buffer, line, &mut cache),
                    expected
                );
                assert_eq!(cache.scanned_lines, scanned);
            }
            assert!(super::source_context_boundary_before(
                &buffer,
                buffer.line_count(),
                &mut cache
            ));
        }
    }

    #[test]
    fn source_context_boundary_probe_reuses_deep_state_and_equal_length_edits() {
        let mut buffer = Buffer::from_text(&format!(
            "{}~~~\n(\n~~~\n~~~\n)\n~~~",
            "paragraph\n".repeat(4096)
        ));
        let mut cache = super::SourceContextCache::default();
        let line = 4099;
        assert!(super::source_context_boundary_before(
            &buffer, line, &mut cache
        ));
        let scanned = cache.scanned_lines;
        assert_eq!(scanned, line);
        for _ in 0..32 {
            assert!(super::source_context_boundary_before(
                &buffer, line, &mut cache
            ));
        }
        assert_eq!(cache.scanned_lines, scanned);
        cache.invalidate_from(line);
        let start = buffer.line_start(line);
        buffer.replace(start..start + 3, "xxx");
        assert!(!super::source_context_boundary_before(
            &buffer, line, &mut cache
        ));
        assert_eq!(cache.scanned_lines, scanned);
        cache.invalidate_from(line);
        buffer.replace(start..start + 3, "~~~");
        assert!(super::source_context_boundary_before(
            &buffer, line, &mut cache
        ));
        assert_eq!(cache.scanned_lines, scanned);
        assert_eq!(cache.lookahead_scanned_lines.get(), 0);
    }

    #[test]
    fn source_safety_probes_preserve_the_real_hot_context_and_closer_bound() {
        use super::SourceInputContext as C;
        let (buffer, cursor) = source_cursor("<div>\n<u¦\n</div>");
        let tag_start = cursor.offset - 2;
        let mut cache = super::SourceContextCache::default();
        assert_eq!(
            super::source_input_context(&buffer, cursor.offset, &mut cache),
            C::Html
        );
        let scanned = cache.scanned_lines;
        let before = cache.before.as_ref().unwrap().state.html.clone();
        let after = cache.after.as_ref().unwrap().state.html.clone();
        for _ in 0..32 {
            assert!(super::source_html_tag_assistance_allowed(
                &buffer, tag_start, &mut cache
            ));
            assert!(super::source_context_boundary_before(
                &buffer, 1, &mut cache
            ));
            assert_eq!(
                super::source_input_context(&buffer, cursor.offset, &mut cache),
                C::Html
            );
        }
        assert_eq!(cache.scanned_lines, scanned);
        assert!(cache.before.as_ref().unwrap().state.html == before);
        assert!(cache.after.as_ref().unwrap().state.html == after);
        assert_eq!(cache.lookahead_scanned_lines.get(), 0);
        assert!(!super::source_html_tag_assistance_allowed(
            &buffer,
            buffer.len(),
            &mut cache
        ));
    }

    #[test]
    fn source_context_before_excludes_openers_but_keeps_existing_literals() {
        use super::SourceInputContext as C;
        for (input, expected) in [
            ("¦```rust", C::Markdown),
            ("¦$$", C::Markdown),
            ("¦\\[", C::Markdown),
            ("¦---", C::Markdown),
            ("\u{feff}¦---", C::Markdown),
            ("¦<div>", C::Markdown),
            ("> - ¦~~~", C::Markdown),
            ("- > ¦$$", C::Markdown),
            ("前\n> ¦\\[", C::Markdown),
            ("- parent\n    - ¦```", C::Markdown),
            ("前🙂 ¦```", C::Markdown),
            ("    ¦$$", C::Code),
            ("~~~\n¦~~~", C::Code),
            ("~~~\n\t¦$$", C::Code),
            ("$$\n¦$$", C::Math),
            ("\\[\n¦\\]", C::Math),
            ("---\n¦---", C::FrontMatter),
            ("---\ntext: 文🙂¦$$\n---", C::FrontMatter),
            ("<pre>\n¦</pre>", C::Html),
            ("<!--\n¦-->", C::Html),
            ("<div>\n¦</div>", C::Html),
            ("> - ~~~\n>   ¦$$", C::Code),
            ("> - $$\n>   ¦~~~", C::Math),
            ("- > \\[\n  > ¦```", C::Math),
            ("- <div>\n  ¦```", C::Html),
            ("> <pre>\n> ¦$$", C::Html),
            ("~~~\n~~~\n¦$$", C::Markdown),
            ("---\nx: true\n---\n¦```", C::Markdown),
            ("<div>\n</div>\n¦$$", C::Markdown),
            ("> ~~~\n¦$$", C::Markdown),
            ("```rust¦suffix", C::Code),
        ] {
            let (buffer, cursor) = source_cursor(input);
            let mut cache = super::SourceContextCache::default();
            assert_eq!(
                super::source_input_context_before(&buffer, cursor.offset, &mut cache),
                expected,
                "{input:?}"
            );
            // Only lines strictly before the insertion point are cached/scanned.
            assert_eq!(
                cache.scanned_lines,
                buffer.offset_to_line_col(cursor.offset).0,
                "{input:?}"
            );
            assert_eq!(cache.lookahead_scanned_lines.get(), 0);
        }
    }

    #[test]
    fn source_context_before_uses_only_the_current_inline_prefix() {
        use super::SourceInputContext as C;
        for (input, expected) in [
            ("前 `文`¦$$", C::Markdown),
            ("前 $文$¦```", C::Markdown),
            ("前 \\(文\\)¦$$", C::Markdown),
            ("前 ¦`文`", C::Markdown),
            ("前 ¦$文$", C::Markdown),
            ("前 `¦", C::Markdown),
            ("前 $¦", C::Markdown),
            ("[文](url(文🙂¦$$))", C::Html),
            ("前 <span title=\"文🙂¦$$\">", C::Html),
        ] {
            let (buffer, cursor) = source_cursor(input);
            let mut cache = super::SourceContextCache::default();
            assert_eq!(
                super::source_input_context_before(&buffer, cursor.offset, &mut cache),
                expected,
                "{input:?}"
            );
            assert_eq!(cache.scanned_lines, 0);
        }
    }

    #[test]
    fn source_context_before_reuses_deep_checkpoints_and_preserves_real_line_state() {
        use super::SourceInputContext as C;
        let prefix = "paragraph\n".repeat(4096);
        let (mut buffer, cursor) =
            source_cursor(&format!("{prefix}> - ¦```rust\n>   body\n>   ```"));
        let mut cache = super::SourceContextCache::default();
        assert_eq!(
            super::source_input_context_before(&buffer, cursor.offset, &mut cache),
            C::Markdown
        );
        let scanned = cache.scanned_lines;
        assert_eq!(scanned, 4096);
        for _ in 0..64 {
            assert_eq!(
                super::source_input_context_before(&buffer, cursor.offset, &mut cache),
                C::Markdown
            );
        }
        assert_eq!(cache.scanned_lines, scanned);
        assert_eq!(
            super::source_input_context(&buffer, cursor.offset, &mut cache),
            C::Code
        );
        assert_eq!(cache.scanned_lines, scanned + 1);
        let opener_cursor = Cursor {
            offset: buffer.line_end(4096),
            anchor: buffer.line_end(4096),
        };
        for _ in 0..32 {
            assert_eq!(
                super::source_input_context_before(&buffer, cursor.offset, &mut cache),
                C::Markdown
            );
            assert_eq!(
                super::source_input_context(&buffer, cursor.offset, &mut cache),
                C::Code
            );
        }
        assert_eq!(cache.scanned_lines, scanned + 1);
        assert_eq!(
            super::source_enter_plan(&buffer, opener_cursor, &mut cache).text,
            "\n>   "
        );
        assert_eq!(cache.scanned_lines, scanned + 1);
        // Equal-length edits preserve the exact boundary before the edited row.
        cache.invalidate_from(4096);
        buffer.replace(cursor.offset..cursor.offset + 3, "$$ ");
        assert_eq!(
            super::source_input_context_before(&buffer, cursor.offset, &mut cache),
            C::Markdown
        );
        assert_eq!(cache.scanned_lines, scanned + 1);
        assert_eq!(
            super::source_input_context(&buffer, cursor.offset, &mut cache),
            C::Math
        );
        assert_eq!(cache.scanned_lines, scanned + 2);
        assert!(
            cache.checkpoints.len()
                <= buffer.line_count() / super::SOURCE_CONTEXT_CHECKPOINT_LINES + 1
        );
    }

    #[test]
    fn source_context_before_keeps_deep_literal_boundaries_hot_across_local_edits() {
        use super::SourceInputContext as C;
        for (open, kind) in [
            ("~~~", C::Code),
            ("$$", C::Math),
            ("---", C::FrontMatter),
            ("<pre>", C::Html),
        ] {
            let (mut buffer, cursor) =
                source_cursor(&format!("{open}\n{}¦$$", "literal\n".repeat(4096)));
            let mut cache = super::SourceContextCache::default();
            assert_eq!(
                super::source_input_context_before(&buffer, cursor.offset, &mut cache),
                kind,
                "{open}"
            );
            let scanned = cache.scanned_lines;
            assert_eq!(scanned, 4097);
            for _ in 0..32 {
                cache.invalidate_from(4097);
                buffer.insert(buffer.len(), "文🙂");
                assert_eq!(
                    super::source_input_context_before(&buffer, cursor.offset, &mut cache),
                    kind,
                    "{open}"
                );
            }
            assert_eq!(cache.scanned_lines, scanned);
            assert_eq!(cache.lookahead_scanned_lines.get(), 0);
            // Changing an earlier opener invalidates the boundary, even if its
            // byte length and the insertion point are unchanged.
            cache.invalidate_from(0);
            buffer.replace(0..open.len(), &"x".repeat(open.len()));
            assert_eq!(
                super::source_input_context_before(&buffer, cursor.offset, &mut cache),
                C::Markdown,
                "{open}"
            );
            assert_eq!(cache.scanned_lines, scanned * 2);
        }
    }

    #[test]
    fn source_tasks_accept_ordered_markers_tabs_and_parser_spacing() {
        for (input, expected) in [
            ("2. [x] 文¦\n99. keep", "2. [x] 文\n3. [ ] ¦\n99. keep"),
            ("09)   [X]\t文¦", "09)   [X]\t文\n10)   [ ]\t¦"),
            ("-\t[x]\t文¦", "-\t[x]\t文\n-\t[ ]\t¦"),
            ("+   [x]  文¦", "+   [x]  文\n+   [ ]  ¦"),
            ("> 8.\t[ ]  文¦", "> 8.\t[ ]  文\n> 9.\t[ ]  ¦"),
            ("12. [x] ¦", "\n¦"),
            ("- [x]¦", "\n¦"),
            ("- [not] 文¦", "- [not] 文\n- ¦"),
        ] {
            assert_eq!(source_result(input, true), expected, "{input:?}");
        }
        for (input, expected) in [
            ("2. [x] ¦文", "¦文"),
            ("09)   [X]\t¦文", "¦文"),
            ("> -\t[ ]  ¦文", "> ¦文"),
            ("-   [x] ¦文", "¦文"),
        ] {
            assert_eq!(source_result(input, false), expected, "{input:?}");
        }
    }

    #[test]
    fn source_task_variants_agree_with_the_markdown_parser() {
        for text in [
            "2. [x] 文",
            "09)   [X]\t文",
            "-\t[x]\t文",
            "+   [x]  文",
            "> 8.\t[ ]  文",
        ] {
            assert!(
                pulldown_cmark::Parser::new_ext(text, pulldown_cmark::Options::ENABLE_TASKLISTS)
                    .any(|event| matches!(event, pulldown_cmark::Event::TaskListMarker(_))),
                "{text:?}"
            );
            let prefix = super::source_list_prefix(text).expect("parser-recognized task prefix");
            assert!(prefix.continuation.contains("[ ]"), "{text:?}");
            assert_eq!(&text[prefix.end..], "文");
        }
    }

    #[test]
    fn source_backspace_rolls_back_headings_inside_quotes_and_lists_only() {
        for (input, expected) in [
            ("> ### ¦文", "> ¦文"),
            ("> > ##\t¦文", "> > ¦文"),
            ("- ## ¦文", "- ¦文"),
            ("> 1. ###  ¦文", "> 1. ¦文"),
            ("- > # ¦文", "- > ¦文"),
            ("- parent\n    - [x] ### ¦文", "- parent\n    - [x] ¦文"),
        ] {
            assert_eq!(source_result(input, false), expected, "{input:?}");
        }
        for input in [
            "---\n### ¦文\n---",
            "<div>\n- ### ¦文\n</div>",
            "> ~~~\n> ### ¦文\n> ~~~",
            "$$\n### ¦文\n$$",
        ] {
            let (buffer, cursor) = source_cursor(input);
            assert!(
                super::source_backspace_plan(
                    &buffer,
                    cursor,
                    &mut super::SourceContextCache::default()
                )
                .is_none(),
                "{input:?}"
            );
        }
    }

    #[test]
    fn source_math_enter_closes_only_unclosed_openers_and_keeps_containers() {
        for (input, expected) in [
            ("$$¦", "$$\n¦\n$$"),
            ("$$x +¦", "$$x +\n¦\n$$"),
            ("\\[¦", "\\[\n¦\n\\]"),
            ("> $$¦", "> $$\n> ¦\n> $$"),
            ("- \\[¦", "- \\[\n  ¦\n  \\]"),
            ("> 1. $$¦", "> 1. $$\n>    ¦\n>    $$"),
            ("$$¦\nx\n$$", "$$\n¦\nx\n$$"),
            ("\\[¦\nx\n\\]", "\\[\n¦\nx\n\\]"),
            ("$$x$$¦", "$$x$$\n¦"),
            ("\\[x\\]¦", "\\[x\\]\n¦"),
            ("$$\nx\n$$¦", "$$\nx\n$$\n¦"),
            ("$$\n\\[¦\n$$", "$$\n\\[\n¦\n$$"),
            ("~~~\n$$¦\n~~~", "~~~\n$$\n¦\n~~~"),
            ("- $$¦\n- next", "- $$\n  ¦\n- next"),
        ] {
            assert_eq!(source_result(input, true), expected, "{input:?}");
        }
    }

    #[test]
    fn source_front_matter_enter_assists_yaml_not_markdown() {
        for (input, expected) in [
            ("---¦", "---\n¦\n---"),
            ("\u{feff}---¦", "\u{feff}---\n¦\n---"),
            ("---¦\ntitle: x\n---", "---\n¦\ntitle: x\n---"),
            ("---¦\nx: y\n...", "---\n¦\nx: y\n..."),
            ("---\nchild:¦\n---", "---\nchild:\n  ¦\n---"),
            (
                "---\n  child: # comment¦\n---",
                "---\n  child: # comment\n    ¦\n---",
            ),
            (
                "---\nitems:\n  - 文¦\n---",
                "---\nitems:\n  - 文\n  - ¦\n---",
            ),
            (
                "---\nitems:\n  - key:¦\n---",
                "---\nitems:\n  - key:\n      ¦\n---",
            ),
            ("---\ntext: |-¦\n---", "---\ntext: |-\n  ¦\n---"),
            ("---\ntext: >2-¦\n---", "---\ntext: >2-\n  ¦\n---"),
            ("---\ntext: |4¦\n---", "---\ntext: |4\n    ¦\n---"),
            ("---\n- key: |4¦\n---", "---\n- key: |4\n      ¦\n---"),
            ("---\n- |-¦\n---", "---\n- |-\n  ¦\n---"),
            ("---\n1. plain¦\n---", "---\n1. plain\n¦\n---"),
            (
                "---\ntext: |\n  - [x] 文¦\n---",
                "---\ntext: |\n  - [x] 文\n  ¦\n---",
            ),
            (
                "---\ntext: |\n  key:¦\n---",
                "---\ntext: |\n  key:\n  ¦\n---",
            ),
            (
                "---\ntext: plain\n  - 文¦\n---",
                "---\ntext: plain\n  - 文\n  ¦\n---",
            ),
            (
                "---\ntext: \"first\n  - 文¦\n  last\"\n---",
                "---\ntext: \"first\n  - 文\n  ¦\n  last\"\n---",
            ),
            (
                "---\ntext: 'first\n  - 文¦\n  last'\n---",
                "---\ntext: 'first\n  - 文\n  ¦\n  last'\n---",
            ),
            (
                "---\ntext: \"key: >\"¦\n---",
                "---\ntext: \"key: >\"\n¦\n---",
            ),
            (
                "---\nurl: https://例.test/a:¦\n---",
                "---\nurl: https://例.test/a:\n¦\n---",
            ),
            (
                "---\nflow: [\n  - text¦\n]\n---",
                "---\nflow: [\n  - text\n  ¦\n]\n---",
            ),
            ("---\n- [x] 文¦\n---", "---\n- [x] 文\n- ¦\n---"),
            ("---\n> quote¦\n---", "---\n> quote\n¦\n---"),
            ("---\n# heading¦\n---", "---\n# heading\n¦\n---"),
            ("---\n~~~¦\n---", "---\n~~~\n¦\n---"),
            ("---\n$$¦\n---", "---\n$$\n¦\n---"),
            ("---\n---¦", "---\n---\n¦"),
            ("text\n---¦", "text\n---\n¦"),
        ] {
            assert_eq!(source_result(input, true), expected, "{input:?}");
        }
    }

    #[test]
    fn source_html_enter_never_continues_markdown_inside_literal_blocks() {
        for (input, prefix) in [
            ("<div>\n- [x] 文¦\n</div>", ""),
            ("<div>\n\n- 文¦\n</div>", ""),
            ("<pre>\n  - 文¦\n</pre>", "  "),
            ("<!--\n- 文¦\n-->", ""),
            ("<script>\n  {¦}\n</script>", "  "),
            ("> <div>\n> - 文¦\n> </div>", "> "),
            ("- <div>\n  - 文¦\n  </div>", "  "),
            ("<div>\n~~~¦\n</div>", ""),
            ("<div\n title=\"</div>\">\n- 文¦\n</div>", ""),
            ("<pre title=\"</pre>\">\n- 文¦\n</pre>", ""),
        ] {
            assert_eq!(
                source_result(input, true),
                input.replace('¦', &format!("\n{prefix}¦")),
                "{input:?}"
            );
        }
        assert_eq!(
            source_result("<div>\n~~~\n</div>\n- 文¦", true),
            "<div>\n~~~\n</div>\n- 文\n- ¦"
        );
    }

    #[test]
    fn source_code_enter_preserves_indentation_and_expands_paired_braces() {
        for (input, expected) in [
            ("~~~rs\n    call();¦\n~~~", "~~~rs\n    call();\n    ¦\n~~~"),
            ("~~~rs\n\tcall();¦\n~~~", "~~~rs\n\tcall();\n\t¦\n~~~"),
            ("~~~rs\n  {¦}\n~~~", "~~~rs\n  {\n      ¦\n  }\n~~~"),
            ("~~~rs\n\t{¦}\n~~~", "~~~rs\n\t{\n\t\t¦\n\t}\n~~~"),
            ("~~~rs\n  [¦  ]\n~~~", "~~~rs\n  [\n      ¦\n  ]\n~~~"),
            (
                "> - ~~~rs\n>   {¦}\n>   ~~~",
                "> - ~~~rs\n>   {\n>       ¦\n>   }\n>   ~~~",
            ),
            ("~~~rs\nif ready {¦\n~~~", "~~~rs\nif ready {\n    ¦\n~~~"),
            ("    {¦}", "    {\n        ¦\n    }"),
            ("- `x{¦}`", "- `x{\n¦}`"),
        ] {
            assert_eq!(source_result(input, true), expected, "{input:?}");
        }
    }

    #[test]
    fn source_literal_cache_invalidates_equal_length_edits_and_keeps_hot_boundaries() {
        use super::SourceInputContext as C;
        for (open, replacement, close, kind) in [
            ("---", "xxx", "---", C::FrontMatter),
            ("<pre>", "xxxxx", "</pre>", C::Html),
            ("<div>", "xxxxx", "</div>", C::Html),
            ("<!--", "xxxx", "-->", C::Html),
            ("$$", "xx", "$$", C::Math),
            ("~~~", "xxx", "~~~", C::Code),
        ] {
            let mut buffer = Buffer::from_text(&format!(
                "{open}\n{}- 文🙂\n{close}",
                "literal\n".repeat(512)
            ));
            let mut cache = super::SourceContextCache::default();
            cache.markdown_decorations = Some(vec![
                crate::markdown::LineDecoration::default();
                buffer.line_count()
            ]);
            let line = 513;
            let query = |buffer: &Buffer, cache: &mut super::SourceContextCache| {
                super::source_input_context(buffer, buffer.line_end(line), cache)
            };
            assert_eq!(query(&buffer, &mut cache), kind, "{open}");
            let scanned = cache.scanned_lines;
            for _ in 0..32 {
                assert_eq!(query(&buffer, &mut cache), kind);
            }
            assert_eq!(cache.scanned_lines, scanned);
            for _ in 0..16 {
                cache.invalidate_from(line);
                buffer.insert(buffer.line_end(line), "文字");
                assert_eq!(query(&buffer, &mut cache), kind);
            }
            assert_eq!(cache.scanned_lines, scanned + 16);
            cache.invalidate_from(0);
            buffer.replace(0..open.len(), replacement);
            assert_eq!(query(&buffer, &mut cache), C::Markdown, "{open}");
            cache.invalidate_from(0);
            buffer.replace(0..open.len(), open);
            assert_eq!(query(&buffer, &mut cache), kind, "{open}");
            cache.invalidate_from(line);
            buffer.insert(buffer.line_start(line), "\n");
            assert_eq!(
                super::source_input_context(&buffer, buffer.line_end(line + 1), &mut cache),
                kind
            );
            assert!(
                cache.checkpoints.len()
                    <= buffer.line_count() / super::SOURCE_CONTEXT_CHECKPOINT_LINES + 1
            );
        }
    }

    #[test]
    fn source_literal_closer_edits_invalidate_later_markdown_context() {
        use super::SourceInputContext as C;
        for (open, close, replacement, kind) in [
            ("---", "---", "xxx", C::FrontMatter),
            ("<pre>", "</pre>", "xxxxxx", C::Html),
            ("<div>", "</div>", "xxxxxx", C::Html),
            ("<!--", "-->", "xxx", C::Html),
        ] {
            let mut buffer = Buffer::from_text(&format!("{open}\n文🙂\n{close}\n- after"));
            let mut cache = super::SourceContextCache::default();
            assert_eq!(
                super::source_input_context(&buffer, buffer.len(), &mut cache),
                C::Markdown
            );
            let start = buffer.line_start(2);
            cache.invalidate_from(2);
            buffer.replace(start..start + close.len(), replacement);
            assert_eq!(
                super::source_input_context(&buffer, buffer.len(), &mut cache),
                kind,
                "{open}"
            );
            cache.invalidate_from(2);
            buffer.replace(start..start + close.len(), close);
            assert_eq!(
                super::source_input_context(&buffer, buffer.len(), &mut cache),
                C::Markdown,
                "{open}"
            );
        }
    }

    #[test]
    fn source_inline_cache_invalidates_same_length_delimiters_and_handles_unicode_boundaries() {
        use super::SourceInputContext as C;
        for (open, close, replacement, kind) in [
            ("`", "`", "x", C::Code),
            ("$", "$", "x", C::Math),
            ("<b>", "</b>", "xxx", C::Html),
        ] {
            let prefix = "paragraph\n".repeat(128);
            let mut buffer = Buffer::from_text(&format!("{prefix}前 {open}文🙂{close} tail"));
            let mut cache = super::SourceContextCache::default();
            let offset = buffer.text().find('🙂').unwrap();
            assert_eq!(
                super::source_input_context(&buffer, offset, &mut cache),
                if kind == C::Html { C::Markdown } else { kind }
            );
            let marker = buffer.text().find(open).unwrap();
            let query_offset = if kind == C::Html { marker + 2 } else { offset };
            assert_eq!(
                super::source_input_context(&buffer, query_offset, &mut cache),
                kind
            );
            let scanned = cache.scanned_lines;
            cache.invalidate_from(128);
            buffer.replace(marker..marker + open.len(), replacement);
            assert_eq!(
                super::source_input_context(&buffer, query_offset, &mut cache),
                C::Markdown
            );
            assert_eq!(cache.scanned_lines, scanned + 1);
            for (offset, _) in buffer.text().char_indices().skip(prefix.chars().count()) {
                super::source_input_context(&buffer, offset, &mut cache);
            }
            assert_eq!(cache.scanned_lines, scanned + 1);
        }
    }

    #[test]
    fn source_opening_enter_bounds_lookahead_without_cooling_the_cache() {
        use super::SourceInputContext as C;
        for (open, prefix, kind) in [
            ("~~~", "", C::Code),
            ("- > ~~~", "  > ", C::Code),
            ("$$", "", C::Math),
            ("---", "", C::FrontMatter),
        ] {
            let buffer = Buffer::from_text(&format!(
                "{open}\n{}",
                format!("{prefix}literal\n").repeat(2048)
            ));
            let mut cache = super::SourceContextCache::default();
            let cursor = Cursor {
                offset: open.len(),
                anchor: open.len(),
            };
            assert_eq!(
                super::source_enter_plan(&buffer, cursor, &mut cache).text,
                format!("\n{prefix}")
            );
            assert_eq!(cache.scanned_lines, 1);
            assert_eq!(
                cache.lookahead_scanned_lines.get(),
                super::SOURCE_CLOSER_LOOKAHEAD_LINES
            );
            assert_eq!(
                super::source_input_context(&buffer, buffer.line_end(1), &mut cache),
                kind
            );
            assert_eq!(cache.scanned_lines, 2);
        }
    }

    #[test]
    fn source_tables_links_and_literal_content_are_never_reformatted() {
        for input in [
            "| a¦ | b |\n| --- | :---: |\n| 文 | 🙂 |",
            "- [label](https://例/文¦字)",
            "> `literal¦`",
            "---\ntext: |\n  | a¦ | b |\n---",
            "<div>\n| a¦ | b |\n</div>",
        ] {
            assert_eq!(
                source_result(input, true),
                input.replace(
                    '¦',
                    if input.contains("text: |") {
                        "\n  ¦"
                    } else {
                        "\n¦"
                    }
                ),
                "{input:?}"
            );
        }
        for input in ["(¦)", "[¦]", "{¦}", "**¦**", "[文](url¦)"] {
            let (buffer, cursor) = source_cursor(input);
            assert!(
                super::source_backspace_plan(
                    &buffer,
                    cursor,
                    &mut super::SourceContextCache::default()
                )
                .is_none()
            );
        }
    }

    #[test]
    fn source_keeps_generic_paired_delimiter_deletion_even_in_code() {
        for (input, expected) in [
            ("(¦)", "¦"),
            ("[[¦]]", "¦"),
            ("文「¦」字", "文¦字"),
            ("```\n{¦}\n```", "```\n¦\n```"),
        ] {
            assert_eq!(source_result(input, false), expected);
        }
        let cursor = Cursor {
            offset: 1,
            anchor: 0,
        };
        assert_eq!(super::auto_paired_backspace_range("()", cursor), None);
        assert_eq!(paired_delimiter_at("文", 3, "（"), Some(("（", "）")));
    }

    #[test]
    fn bare_fences_autocomplete_on_enter_without_duplicating_a_closer() {
        for (text, closer) in [
            ("```", "```"),
            ("  ```  ", "```"),
            ("````", "````"),
            ("~~~", "~~~"),
        ] {
            assert_eq!(
                super::code_fence_enter_closer(text, 0, false).as_deref(),
                Some(closer)
            );
        }
        assert_eq!(
            super::code_fence_enter_closer("```rust", 0, false),
            Some("```".into())
        );
        assert_eq!(
            super::code_fence_enter_closer("```rust\ncode\n```", 0, false),
            None
        );
        assert_eq!(
            super::code_fence_enter_closer("```\ncode\n```", 2, false),
            None
        );
        assert_eq!(super::code_fence_enter_closer("```\nbody", 1, false), None);
    }

    #[test]
    fn pending_fence_gets_its_own_closer_before_existing_blocks() {
        let text = "before\n\n```\n\n```rust\nlet value = 1;\n```\n";
        let closer = super::code_fence_enter_closer(text, 2, true).unwrap();
        let mut buffer = Buffer::from_text(text);
        let mut cursor = crate::cursor::Cursor::new();
        cursor.move_to(buffer.line_end(2), &buffer);
        let body_offset = cursor.offset + 1;
        cursor.insert(&mut buffer, &format!("\n\n{closer}"));
        cursor.move_to(body_offset, &buffer);
        assert_eq!(buffer.line_content(3), "");
        assert_eq!(buffer.offset_to_line_col(cursor.offset), (3, 0));
        let decorations = crate::markdown::parse_decorations(buffer.text());
        assert_eq!(
            decorations[4].block_type,
            crate::markdown::BlockType::CodeBlockFence
        );
        assert_eq!(decorations[6].code_language.as_deref(), Some("rust"));
    }

    #[test]
    fn slash_in_code_is_literal_text() {
        for content in ["/", "    /", "\t/"] {
            assert!(!super::slash_menu_trigger(content, content.len(), true));
        }
        assert!(super::slash_menu_trigger("/", 1, false));
        assert!(super::slash_menu_trigger("  /", 3, false));
        assert!(!super::slash_menu_trigger("text/", 5, false));
        assert!(!super::slash_menu_trigger("/ text", 1, false));
        assert!(!super::slash_menu_trigger(" / ", 2, false));
    }

    #[test]
    fn slash_query_only_tracks_a_blank_markdown_line() {
        let query = super::slash_menu_query;
        assert_eq!(
            query("previous\n  /todo", 11, 16, true, false),
            Some("todo")
        );
        assert_eq!(query("\t/不存在", 0, 11, true, false), None); // not at the slash
        assert_eq!(query("text/todo", 4, 9, true, false), None);
        assert_eq!(query("/todo", 0, 5, false, false), None);
        assert_eq!(query("/todo", 0, 5, true, true), None);
        assert_eq!(query("/todo\nnext", 0, 10, true, false), None);
        assert_eq!(query("/todo", 0, 0, true, false), None);
        assert_eq!(query("/文", 0, 2, true, false), None); // partial UTF-8 character
        assert_eq!(query(" /xyz", 1, 5, true, false), Some("xyz"));
        assert_eq!(query("\t/文", 1, 5, true, false), Some("文"));
    }
    use crate::buffer::Buffer;

    use super::{clamp_ime_offset, missing_wiki_link_closer, paired_delimiter_at};

    #[test]
    fn wiki_link_closer_only_fills_missing_brackets() {
        assert_eq!(missing_wiki_link_closer("[[", 2), "]]");
        assert_eq!(missing_wiki_link_closer("[[]", 2), "]");
        assert_eq!(missing_wiki_link_closer("[[]]", 2), "");
    }

    #[test]
    fn paired_delimiter_at_does_not_pair_markdown_syntax_delimiters() {
        assert_eq!(paired_delimiter_at("", 0, "`"), None);
        assert_eq!(paired_delimiter_at("foo", 3, "_"), None);
        assert_eq!(paired_delimiter_at("foo ", 4, "_"), None);
        assert_eq!(paired_delimiter_at("*", 1, "*"), None);
        assert_eq!(paired_delimiter_at("", 0, "~"), None);
        assert_eq!(paired_delimiter_at("", 0, "+"), None);
        assert_eq!(paired_delimiter_at("", 0, "="), None);
    }

    #[test]
    fn paired_delimiter_at_avoids_commonmark_autolink_and_html_conflicts() {
        assert_eq!(paired_delimiter_at("", 0, "<"), None);
        assert_eq!(paired_delimiter_at("see ", 4, "<"), Some(("<", ">")));
        assert_eq!(paired_delimiter_at("see ", 4, "'"), Some(("'", "'")));
        assert_eq!(paired_delimiter_at("word", 4, "'"), None);
    }

    #[test]
    fn paired_delimiter_at_covers_markdown_extension_delimiters() {
        assert_eq!(paired_delimiter_at("", 0, "("), Some(("(", ")")));
        assert_eq!(paired_delimiter_at("", 0, "{"), Some(("{", "}")));
        assert_eq!(paired_delimiter_at("", 0, "［"), Some(("［", "］")));
        assert_eq!(paired_delimiter_at("", 0, "【"), Some(("【", "】")));
        assert_eq!(paired_delimiter_at("", 0, "＂"), Some(("＂", "＂")));
        assert_eq!(paired_delimiter_at("", 0, "＇"), Some(("＇", "＇")));
    }

    #[test]
    fn clamp_ime_offset_snaps_into_grapheme_boundary() {
        let buffer = Buffer::from_text("a\u{0301}b");
        let combining_boundary = buffer
            .text()
            .char_indices()
            .nth(1)
            .map(|(idx, _)| idx)
            .expect("combining mark boundary");

        assert_eq!(clamp_ime_offset(&buffer, combining_boundary, 0), 0);
    }

    #[test]
    fn clamp_ime_offset_saturates_negative_delta() {
        let buffer = Buffer::from_text("abc");

        assert_eq!(clamp_ime_offset(&buffer, 1, -10), 0);
    }
}
