use crate::front_matter::{self, FrontMatterLineInfo};
use emojis::get_by_shortcode;
use pulldown_cmark::{BrokenLink, CodeBlockKind, Event, LinkType, Options, Parser, Tag, TagEnd};
use regex::Regex;
use std::ops::Range;
use std::path::Path;
use std::sync::{Arc, OnceLock};

#[path = "inline_syntax.rs"]
mod inline_syntax;
pub(crate) use inline_syntax::CjkInlineSource;

fn cell_display_width(s: &str) -> usize {
    s.chars()
        .map(|ch| {
            let code = ch as u32;
            if (0x1100..=0x115F).contains(&code)
                || (0x2E80..=0x33BF).contains(&code)
                || (0x3040..=0x9FFF).contains(&code)
                || (0xA000..=0xA4CF).contains(&code)
                || (0xAC00..=0xD7AF).contains(&code)
                || (0xF900..=0xFAFF).contains(&code)
                || (0xFE30..=0xFE6F).contains(&code)
                || (0xFF01..=0xFF60).contains(&code)
                || (0xFFE0..=0xFFE6).contains(&code)
                || (0x20000..=0x2FA1F).contains(&code)
            {
                2
            } else {
                1
            }
        })
        .sum()
}

#[derive(Debug, Clone, PartialEq)]
pub enum BlockType {
    Normal,
    Heading(u8),
    CodeBlock,
    CodeBlockFence,
    Blockquote,
    UnorderedList,
    OrderedList,
    TaskList(bool),
    HorizontalRule,
    Table,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockContext {
    Blockquote,
    OrderedList,
    UnorderedList,
    ListItem,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InlineSpan {
    pub kind: InlineKind,
    pub range: Range<usize>,
    pub target: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InlineKind {
    Bold,
    Italic,
    Strikethrough,
    Underline,
    InlineCode,
    Link,
    Image,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalloutKind {
    Note,
    Tip,
    Important,
    Warning,
    Caution,
}

/// A byte range to hide when syntax is folded, plus a trigger range that
/// controls when to show it (cursor column in trigger_range → range is shown).
#[derive(Debug, Clone, PartialEq)]
pub struct SyntaxRange {
    pub range: Range<usize>,
    pub trigger_range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct WikiLinkSpan {
    pub full_range: Range<usize>,
    pub collapsed_range: Range<usize>,
    pub target_path: String,
    pub has_alias: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LocalLinkSyntax {
    Markdown,
    WikiLink,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct LocalLinkTarget {
    pub source_range: Range<usize>,
    pub target_range: Range<usize>,
    pub target: String,
    pub syntax: LocalLinkSyntax,
    pub is_embed: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FootnoteReferenceSpan {
    #[allow(dead_code)]
    pub full_range: Range<usize>,
    pub collapsed_range: Range<usize>,
    #[allow(dead_code)]
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DisplayReplacement {
    pub source_range: Range<usize>,
    pub display_text: String,
    pub trigger_range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct TableCellFragment {
    pub range: Range<usize>,
    pub block_type: BlockType,
    /// Complete source prefix, including indentation; zero keeps it literal.
    pub prefix_len: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockPreviewKind {
    Math,
    Mermaid,
}

#[derive(Debug, Clone, PartialEq)]
pub struct BlockPreview {
    pub kind: BlockPreviewKind,
    pub cache_key: String,
    pub source_text: String,
    pub source_range: Range<usize>,
    pub start_line: usize,
    pub end_line: usize,
    pub is_anchor: bool,
}

fn merge_byte_ranges(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
    if ranges.is_empty() {
        return ranges;
    }

    ranges.sort_by(|left, right| left.start.cmp(&right.start).then(left.end.cmp(&right.end)));

    let mut merged: Vec<Range<usize>> = Vec::with_capacity(ranges.len());
    for range in ranges {
        if let Some(last) = merged.last_mut()
            && range.start <= last.end
        {
            last.end = last.end.max(range.end);
        } else {
            merged.push(range);
        }
    }

    merged
}

pub(crate) fn inline_span_source_range(
    decoration: &LineDecoration,
    span: &InlineSpan,
) -> Option<Range<usize>> {
    decoration
        .syntax_ranges
        .iter()
        .filter(|syntax_range| {
            let table_conceal = decoration.table_info.as_ref().is_some_and(|table| {
                !table.is_front_matter
                    && syntax_range.range.start == 0
                    && syntax_range.range == syntax_range.trigger_range
                    && table
                        .cell_ranges
                        .last()
                        .is_some_and(|cell| syntax_range.range.end >= cell.end)
            });
            !table_conceal
                && syntax_range.trigger_range.start <= span.range.start
                && syntax_range.trigger_range.end >= span.range.end
        })
        .map(|syntax_range| syntax_range.trigger_range.clone())
        .min_by_key(|range| (range.end.saturating_sub(range.start), range.start))
}

pub(crate) fn link_target_at_column(decoration: &LineDecoration, column: usize) -> Option<&str> {
    decoration
        .inline_spans
        .iter()
        .find_map(|span| {
            if span.kind != InlineKind::Link {
                return None;
            }
            let source_range =
                inline_span_source_range(decoration, span).unwrap_or_else(|| span.range.clone());
            (column >= source_range.start && column <= source_range.end)
                .then(|| span.target.as_deref())
                .flatten()
        })
        .or_else(|| {
            decoration.wiki_links.iter().find_map(|link| {
                (column >= link.full_range.start && column <= link.full_range.end)
                    .then_some(link.target_path.as_str())
            })
        })
}

fn structural_excluded_ranges(decoration: &LineDecoration) -> Vec<Range<usize>> {
    let mut excluded = decoration.protected_ranges.clone();

    for span in decoration.inline_spans.iter().filter(|span| {
        matches!(
            span.kind,
            InlineKind::InlineCode | InlineKind::Link | InlineKind::Image
        )
    }) {
        let full_range = inline_span_source_range(decoration, span);

        match full_range {
            Some(range) => excluded.push(range),
            None if matches!(span.kind, InlineKind::InlineCode) => {
                excluded.push(span.range.start.saturating_sub(1)..span.range.end.saturating_add(1));
            }
            None => {}
        }
    }

    merge_byte_ranges(excluded)
}

fn custom_inline_excluded_ranges(decoration: &LineDecoration) -> Vec<Range<usize>> {
    // Presentation syntax may style visible link labels and Wiki aliases, but
    // never structural targets or semantically opaque inline content.
    let mut excluded = decoration.protected_ranges.clone();

    for span in &decoration.inline_spans {
        match span.kind {
            InlineKind::InlineCode | InlineKind::Image => {
                if let Some(range) = inline_span_source_range(decoration, span) {
                    excluded.push(range);
                }
            }
            InlineKind::Link => {
                let source_range = inline_span_source_range(decoration, span);
                excluded.extend(
                    decoration
                        .syntax_ranges
                        .iter()
                        .filter(|syntax_range| {
                            source_range
                                .as_ref()
                                .is_some_and(|source| syntax_range.trigger_range == *source)
                        })
                        .map(|syntax_range| syntax_range.range.clone()),
                );
            }
            _ => {}
        }
    }

    for wiki_link in &decoration.wiki_links {
        if wiki_link.has_alias {
            excluded.push(wiki_link.full_range.start..wiki_link.collapsed_range.start);
            excluded.push(wiki_link.collapsed_range.end..wiki_link.full_range.end);
        } else {
            excluded.push(wiki_link.full_range.clone());
        }
    }
    excluded.extend(
        decoration
            .footnote_references
            .iter()
            .map(|reference| reference.full_range.clone()),
    );

    merge_byte_ranges(excluded)
}

pub(crate) fn unordered_list_prefix_len(content: &str) -> usize {
    let bytes = content.as_bytes();
    let Some((&marker, rest)) = bytes.split_first() else {
        return 0;
    };
    if !matches!(marker, b'-' | b'*' | b'+') {
        return 0;
    }
    if rest.is_empty() {
        return 0;
    }
    if !matches!(rest[0], b' ' | b'\t') {
        return 0;
    }
    1 + rest
        .iter()
        .take_while(|&&byte| matches!(byte, b' ' | b'\t'))
        .count()
}

fn empty_unordered_list_item_prefix_len(content: &str) -> Option<(usize, usize)> {
    let (marker_start, prefix_len) = physical_unordered_list_prefix(content)?;
    (content[marker_start + prefix_len..]
        .trim_end_matches('\r')
        .is_empty())
    .then_some((marker_start, prefix_len))
}

fn physical_unordered_list_prefix(content: &str) -> Option<(usize, usize)> {
    let quote_len = blockquote_prefix_len(content);
    let body = &content[quote_len..];
    let indent = body.len() - body.trim_start_matches([' ', '\t']).len();
    let marker_start = quote_len + indent;
    let prefix_len = unordered_list_prefix_len(&content[marker_start..]);
    (prefix_len > 0).then_some((marker_start, prefix_len))
}

/// A `-` marker with nothing but whitespace after it, as typed while starting a
/// list. Returns the leading indent and the concealable prefix length, which is
/// zero until the separating space is typed.
fn in_progress_unordered_marker(content: &str) -> Option<(usize, usize)> {
    let leading_ws = content.len() - content.trim_start_matches([' ', '\t']).len();
    let trimmed = &content[leading_ws..];
    let rest = trimmed.strip_prefix('-')?;
    if !rest.trim_matches([' ', '\t']).is_empty() {
        return None;
    }
    Some((leading_ws, unordered_list_prefix_len(trimmed)))
}

/// Setext underlines only retroactively promote the paragraph above them, so a
/// short run is treated as text still being typed: `-` starts a list and `=`/`==`
/// starts a `==highlight==`. Runs of three or more still form headings.
fn is_in_progress_setext_underline(content: &str) -> bool {
    content.trim_matches([' ', '\t']).chars().count() < 3
}

pub fn blockquote_prefix_len(content: &str) -> usize {
    let leading_spaces = content.bytes().take_while(|&byte| byte == b' ').count();
    if leading_spaces > 3 {
        return 0;
    }
    let mut index = leading_spaces;
    let mut found = false;
    while content.as_bytes().get(index) == Some(&b'>') {
        found = true;
        index += 1;
        if content
            .as_bytes()
            .get(index)
            .is_some_and(|byte| matches!(byte, b' ' | b'\t'))
        {
            index += 1;
        }
    }
    usize::from(found) * index
}

/// Splits a line into the prefix a continuation line must repeat (blockquote
/// markers plus the indentation after them) and the content that follows.
pub fn split_continuation_prefix(content: &str) -> (String, &str) {
    let quote_len = blockquote_prefix_len(content);
    let body = &content[quote_len..];
    let indent = body.len() - body.trim_start_matches([' ', '\t']).len();
    (
        format!("{}{}", &content[..quote_len], " ".repeat(indent)),
        &body[indent..],
    )
}

fn setext_heading_underline_range(content: &str) -> Option<Range<usize>> {
    let container_len = blockquote_prefix_len(content);
    let trimmed_end = content[container_len..].trim_end_matches([' ', '\t', '\r']);
    let leading_spaces = trimmed_end
        .as_bytes()
        .iter()
        .take_while(|&&byte| byte == b' ')
        .count();
    if leading_spaces > 3 {
        return None;
    }
    let marker = &trimmed_end[leading_spaces..];
    if marker.is_empty() {
        return None;
    }
    let mut chars = marker.chars();
    let first = chars.next()?;
    if !matches!(first, '=' | '-') {
        return None;
    }
    chars
        .all(|ch| ch == first)
        .then_some(container_len + leading_spaces..container_len + trimmed_end.len())
}

fn heading_attribute_range(content: &str) -> Option<Range<usize>> {
    let trimmed_end = content.trim_end_matches([' ', '\t', '\r']);
    if !trimmed_end.ends_with('}') {
        return None;
    }
    let start = trimmed_end.rfind('{')?;
    let attributes = &trimmed_end[start + 1..trimmed_end.len() - 1];
    attributes
        .split_whitespace()
        .any(|token| token.starts_with(['#', '.']) || token.contains('='))
        .then_some(start..trimmed_end.len())
}

pub(crate) fn protected_inline_html_tag(source: &str) -> Option<(String, bool, bool)> {
    let trimmed = source.trim();
    if trimmed.starts_with("<!--") {
        return Some(("!--".to_string(), false, true));
    }
    let inner = trimmed.strip_prefix('<')?.strip_suffix('>')?.trim();
    let closing = inner.starts_with('/');
    let inner = inner.strip_prefix('/').unwrap_or(inner).trim_start();
    let tag = inner
        .split(|ch: char| ch.is_whitespace() || matches!(ch, '/' | '>'))
        .next()?
        .to_ascii_lowercase();
    if !matches!(tag.as_str(), "code" | "pre" | "script" | "style") {
        return None;
    }
    Some((tag, closing, inner.ends_with('/')))
}

fn link_text_close_offset(source: &str, prefix_len: usize) -> Option<usize> {
    if !source.is_char_boundary(prefix_len) || source.len() <= prefix_len {
        return None;
    }

    let mut depth = 1usize;
    let mut escaped = false;
    for (relative_index, ch) in source[prefix_len..].char_indices() {
        let index = prefix_len + relative_index;
        if escaped {
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        match ch {
            '[' => depth += 1,
            ']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }
    None
}

fn offset_to_line(line_starts: &[usize], offset: usize) -> usize {
    match line_starts.binary_search(&offset) {
        Ok(line) => line,
        Err(line) => line.saturating_sub(1),
    }
}

fn push_block_context_for_range(
    decorations: &mut [LineDecoration],
    line_starts: &[usize],
    start: usize,
    end: usize,
    context: BlockContext,
) {
    if decorations.is_empty() || start >= end {
        return;
    }
    let start_line = offset_to_line(line_starts, start).min(decorations.len().saturating_sub(1));
    let end_line =
        offset_to_line(line_starts, end.saturating_sub(1)).min(decorations.len().saturating_sub(1));
    for decoration in &mut decorations[start_line..=end_line] {
        if !decoration.block_contexts.contains(&context) {
            decoration.block_contexts.push(context);
        }
    }
}

fn range_intersects_byte(range: &Range<usize>, start: usize, end: usize) -> bool {
    range.start < end && range.end > start
}

fn next_char_boundary(text: &str, offset: usize) -> usize {
    if offset >= text.len() {
        return text.len();
    }

    let mut next = offset + 1;
    while next < text.len() && !text.is_char_boundary(next) {
        next += 1;
    }
    next
}

pub(crate) fn is_escaped_at(text: &str, offset: usize) -> bool {
    let mut backslashes = 0usize;
    let mut index = offset;
    while index > 0 && text.as_bytes().get(index - 1) == Some(&b'\\') {
        backslashes += 1;
        index -= 1;
    }
    backslashes % 2 == 1
}

fn delimiter_can_open(text: &str, token_end: usize) -> bool {
    text.get(token_end..)
        .and_then(|rest| rest.chars().next())
        .is_some_and(|ch| !ch.is_whitespace())
}

fn delimiter_can_close(text: &str, token_start: usize) -> bool {
    text.get(..token_start)
        .and_then(|prefix| prefix.chars().next_back())
        .is_some_and(|ch| !ch.is_whitespace())
}

pub(crate) fn raw_wiki_link_ranges(
    line_content: &str,
    decoration: Option<&LineDecoration>,
) -> Vec<Range<usize>> {
    let excluded_ranges = decoration
        .map(structural_excluded_ranges)
        .unwrap_or_default();
    let mut open_stack = Vec::new();
    let mut pairs = Vec::new();
    let mut index = 0usize;

    while index < line_content.len() {
        let token_end = index.saturating_add(2);
        if token_end <= line_content.len()
            && !excluded_ranges
                .iter()
                .any(|range| range_intersects_byte(range, index, token_end))
        {
            if line_content.get(index..token_end) == Some("[[")
                && !is_escaped_at(line_content, index)
            {
                open_stack.push(index);
                index += 2;
                continue;
            }

            if line_content.get(index..token_end) == Some("]]")
                && !is_escaped_at(line_content, index)
            {
                if let Some(open) = open_stack.pop() {
                    pairs.push(open..token_end);
                }
                index += 2;
                continue;
            }
        }

        let Some(ch) = line_content
            .get(index..)
            .and_then(|rest| rest.chars().next())
        else {
            break;
        };
        index += ch.len_utf8();
    }

    pairs.sort_by_key(|range| range.end.saturating_sub(range.start));
    let mut non_overlapping = Vec::new();
    for range in pairs {
        if non_overlapping
            .iter()
            .all(|kept: &Range<usize>| !range_intersects_byte(kept, range.start, range.end))
        {
            non_overlapping.push(range);
        }
    }
    non_overlapping.sort_by_key(|range| range.start);
    non_overlapping
}

#[derive(Debug, Clone, PartialEq)]
pub struct LineDecoration {
    pub block_type: BlockType,
    payload: Option<Box<LineDecorationPayload>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LineDecorationPayload {
    pub block_contexts: Vec<BlockContext>,
    pub callout_kind: Option<CalloutKind>,
    pub inline_spans: Vec<InlineSpan>,
    pub inline_math_spans: Vec<Range<usize>>,
    pub protected_ranges: Vec<Range<usize>>,
    pub syntax_ranges: Vec<SyntaxRange>,
    pub display_replacements: Vec<DisplayReplacement>,
    pub(crate) table_cell_fragments: Vec<TableCellFragment>,
    pub(crate) table_reference_definitions: Option<Arc<str>>,
    pub block_preview: Option<BlockPreview>,
    pub highlight_spans: Vec<Range<usize>>,
    pub superscript_spans: Vec<Range<usize>>,
    pub subscript_spans: Vec<Range<usize>>,
    pub wiki_links: Vec<WikiLinkSpan>,
    pub footnote_references: Vec<FootnoteReferenceSpan>,
    pub footnote_definition_label: Option<String>,
    pub table_info: Option<TableLineInfo>,
    pub front_matter: Option<FrontMatterLineInfo>,
    pub code_language: Option<String>,
    /// For image lines, the local file path from `![alt](path)`.
    pub image_path: Option<String>,
}

impl LineDecorationPayload {
    fn is_empty(&self) -> bool {
        self.block_contexts.is_empty()
            && self.callout_kind.is_none()
            && self.inline_spans.is_empty()
            && self.inline_math_spans.is_empty()
            && self.protected_ranges.is_empty()
            && self.syntax_ranges.is_empty()
            && self.display_replacements.is_empty()
            && self.table_cell_fragments.is_empty()
            && self.table_reference_definitions.is_none()
            && self.block_preview.is_none()
            && self.highlight_spans.is_empty()
            && self.superscript_spans.is_empty()
            && self.subscript_spans.is_empty()
            && self.wiki_links.is_empty()
            && self.footnote_references.is_empty()
            && self.footnote_definition_label.is_none()
            && self.table_info.is_none()
            && self.front_matter.is_none()
            && self.code_language.is_none()
            && self.image_path.is_none()
    }
}

impl std::ops::Deref for LineDecoration {
    type Target = LineDecorationPayload;

    fn deref(&self) -> &Self::Target {
        static EMPTY: OnceLock<LineDecorationPayload> = OnceLock::new();
        self.payload
            .as_deref()
            .unwrap_or_else(|| EMPTY.get_or_init(LineDecorationPayload::default))
    }
}

impl std::ops::DerefMut for LineDecoration {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.payload
            .get_or_insert_with(|| Box::new(LineDecorationPayload::default()))
    }
}

impl LineDecoration {
    pub(crate) fn payload_mut(&mut self) -> Option<&mut LineDecorationPayload> {
        self.payload.as_deref_mut()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TableAlign {
    Left,
    Center,
    Right,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TableLineInfo {
    pub cells: Vec<String>,
    pub cell_ranges: Vec<Range<usize>>,
    pub column_alignments: Vec<TableAlign>,
    pub is_header: bool,
    pub is_separator: bool,
    pub is_front_matter: bool,
    pub table_start_line: usize,
    pub table_end_line: usize,
    pub column_widths: Vec<usize>,
}

impl Default for LineDecoration {
    fn default() -> Self {
        Self {
            block_type: BlockType::Normal,
            payload: None,
        }
    }
}

pub fn parse_decorations(text: &str) -> Vec<LineDecoration> {
    parse_decorations_with_options(text, true, None, None, false)
}

pub(crate) fn parse_decorations_fragment(text: &str) -> Vec<LineDecoration> {
    parse_decorations_with_options(text, false, None, None, false)
}

pub(crate) fn parse_decorations_with_pending_fence(text: &str, line: usize) -> Vec<LineDecoration> {
    parse_decorations_with_options(text, true, Some(line), None, false)
}

pub(crate) fn parse_table_cell_decorations(
    text: &str,
    row: &LineDecoration,
) -> Vec<LineDecoration> {
    parse_decorations_with_options(
        text,
        true,
        None,
        row.table_reference_definitions.as_deref(),
        row.table_info
            .as_ref()
            .is_some_and(|table| !table.is_front_matter),
    )
}

fn parse_decorations_with_options(
    text: &str,
    detect_document_front_matter: bool,
    pending_fence_line: Option<usize>,
    reference_definitions: Option<&str>,
    table_cell_math: bool,
) -> Vec<LineDecoration> {
    let line_count = text.lines().count().max(1);
    let line_count = if text.ends_with('\n') {
        line_count + 1
    } else {
        line_count
    };
    let mut decorations: Vec<LineDecoration> =
        (0..line_count).map(|_| LineDecoration::default()).collect();

    let line_starts: Vec<usize> = {
        let mut starts = vec![0];
        for (i, byte) in text.bytes().enumerate() {
            if byte == b'\n' {
                starts.push(i + 1);
            }
        }
        starts
    };

    let offset_to_line = |offset: usize| -> usize { offset_to_line(&line_starts, offset) };

    let offset_to_line_col = |offset: usize| -> (usize, usize) {
        let line = offset_to_line(offset);
        let col = offset - line_starts[line];
        (line, col)
    };

    let opts = Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_SUBSCRIPT
        | Options::ENABLE_SUPERSCRIPT
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_MATH
        | Options::ENABLE_TABLES;

    let mut parser_source = text.to_string();
    if detect_document_front_matter {
        detect_front_matter(&mut decorations, text, &line_starts);
        if let Some(info) = decorations[0]
            .table_info
            .as_ref()
            .filter(|info| info.is_front_matter)
        {
            let end = line_starts
                .get(info.table_end_line)
                .copied()
                .unwrap_or(text.len());
            let masked: String = text[..end]
                .bytes()
                .map(|byte| {
                    if matches!(byte, b'\n' | b'\r') {
                        byte as char
                    } else {
                        ' '
                    }
                })
                .collect();
            parser_source.replace_range(..end, &masked);
        }
    }
    if let Some(line) = pending_fence_line
        && let Some(start) = line_starts.get(line).copied()
    {
        let content = get_line_content(text, &line_starts, line, decorations.len());
        if let Some((_, run)) = code_fence_opener(content) {
            let leading = content.len() - content.trim_start_matches(' ').len();
            parser_source.replace_range(start + leading..start + leading + run, &"x".repeat(run));
        }
    }
    if let Some(span) = unclosed_code_fence_marker_span(&parser_source)
        && ((&parser_source[span.clone()] == "~~~~"
            && parser_source[span.start..]
                .lines()
                .next()
                .is_some_and(|line| line.trim_end() == "~~~~"))
            || !parser_source[span.end..].contains('\n'))
    {
        parser_source.replace_range(span.clone(), &"x".repeat(span.len()));
    }
    // Resolve external references before custom inline detection, so identifiers
    // never become emoji, scripts, or highlights in an isolated cell parse.
    let reference_context = reference_definitions.map(|source| Parser::new_ext(source, opts));
    let resolve_reference = |link: BrokenLink<'_>| {
        let definition = reference_context
            .as_ref()?
            .reference_definitions()
            .get(link.reference.as_ref())?;
        Some((
            definition.dest.clone().into_static(),
            definition
                .title
                .clone()
                .unwrap_or_else(|| "".into())
                .into_static(),
        ))
    };
    let parser_source = CjkInlineSource::new(&parser_source, || {
        Parser::new_with_broken_link_callback(&parser_source, opts, Some(resolve_reference))
            .into_offset_iter()
    });
    let parser = Parser::new_with_broken_link_callback(
        parser_source.as_str(),
        opts,
        Some(|link: BrokenLink<'_>| {
            parser_source.resolve_reference(&link).or_else(|| {
                if parser_source.reference_allows_fallback(&link) {
                    resolve_reference(link)
                } else {
                    None
                }
            })
        }),
    );
    // Share only resolved declarations, not the entire document, with table
    // rows. Re-parsing these retains pulldown's Unicode label matching rules.
    let mut definitions = parser.reference_definitions().iter().collect::<Vec<_>>();
    definitions.sort_by_key(|(label, _)| *label);
    let table_reference_definitions = (!definitions.is_empty()).then(|| {
        let mut source = String::new();
        for (label, definition) in definitions {
            source.push('[');
            source.push_str(label);
            source.push_str("]: <");
            for ch in definition.dest.chars() {
                // Entities may resolve to line breaks or other controls, which
                // cannot be written literally in an angle-bracket destination.
                if ch.is_ascii_control() {
                    source.push_str(&format!("&#{};", ch as u32));
                } else {
                    if matches!(ch, '\\' | '<' | '>' | '&') {
                        source.push('\\');
                    }
                    source.push(ch);
                }
            }
            source.push_str(">\n");
        }
        Arc::<str>::from(source)
    });

    let mut in_code_block = false;
    let mut in_fenced_code_block = false;
    let mut current_code_language: Option<String> = None;
    let mut blockquote_depth = 0usize;
    enum OpenInline {
        Plain {
            kind: InlineKind,
            start: usize,
            target: Option<String>,
        },
        Image {
            start: usize,
            dest_url: String,
        },
        WikiLink {
            start: usize,
        },
    }

    struct OpenMermaidBlock {
        start: usize,
        start_line: usize,
        source_text: String,
    }

    let mut inline_stack: Vec<OpenInline> = Vec::new();
    let mut protected_html_stack: Vec<(String, usize)> = Vec::new();
    let mut list_stack: Vec<BlockContext> = Vec::new();
    let mut open_mermaid_block: Option<OpenMermaidBlock> = None;
    let mut display_math_ranges = Vec::new();

    for (event, source_range) in parser.into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                let (line, source_col) = offset_to_line_col(source_range.start);
                let lvl = level as u8;
                if line < decorations.len() {
                    let line_content =
                        get_line_content(text, &line_starts, line, decorations.len());
                    let source_prefix = line_content.get(source_col..).unwrap_or(line_content);
                    let leading_ws =
                        source_prefix.len() - source_prefix.trim_start_matches([' ', '\t']).len();
                    let marker_start = source_col + leading_ws;
                    let trimmed = &source_prefix[leading_ws..];
                    let atx_marker_len = trimmed.bytes().take_while(|byte| *byte == b'#').count();
                    let is_atx = atx_marker_len == lvl as usize
                        && trimmed
                            .as_bytes()
                            .get(atx_marker_len)
                            .is_none_or(|byte| matches!(byte, b' ' | b'\t' | b'\r'));
                    if is_atx {
                        decorations[line].block_type = BlockType::Heading(lvl);
                        let marker_end = marker_start + lvl as usize;
                        let line_len = line_content.len();
                        let has_space = line_content
                            .as_bytes()
                            .get(marker_end)
                            .is_some_and(|&byte| matches!(byte, b' ' | b'\t'));
                        if marker_end <= line_len && has_space {
                            let prefix_end = marker_end
                                + line_content.as_bytes()[marker_end..]
                                    .iter()
                                    .take_while(|&&byte| matches!(byte, b' ' | b'\t'))
                                    .count();
                            decorations[line].syntax_ranges.push(SyntaxRange {
                                range: marker_start..prefix_end,
                                trigger_range: marker_start..prefix_end,
                            });
                        }
                    } else {
                        let end_line = offset_to_line(source_range.end.saturating_sub(1));
                        let underline_line = (line..=end_line.min(decorations.len() - 1))
                            .rev()
                            .find(|&candidate| {
                                let candidate_content = get_line_content(
                                    text,
                                    &line_starts,
                                    candidate,
                                    decorations.len(),
                                );
                                setext_heading_underline_range(candidate_content).is_some()
                            });

                        if let Some(underline_line) = underline_line {
                            let underline_content = get_line_content(
                                text,
                                &line_starts,
                                underline_line,
                                decorations.len(),
                            );
                            // A short `-`/`=` run under a paragraph is still being
                            // typed, so it must not promote that paragraph to a
                            // heading; `-` additionally starts a list.
                            let underline_range = setext_heading_underline_range(underline_content)
                                .expect("the underline was validated above");
                            if lvl == 2
                                && line > 0
                                && decorations[line - 1].block_type == BlockType::HorizontalRule
                                && decorations[line - 1].block_contexts
                                    == decorations[line].block_contexts
                            {
                                decorations[underline_line].block_type = BlockType::HorizontalRule;
                                decorations[underline_line].syntax_ranges.push(SyntaxRange {
                                    range: underline_range.clone(),
                                    trigger_range: underline_range,
                                });
                                continue;
                            }
                            if is_in_progress_setext_underline(
                                &underline_content[underline_range.clone()],
                            ) {
                                if let Some((marker_ws, marker_len)) =
                                    in_progress_unordered_marker(underline_content)
                                    && marker_len > 0
                                {
                                    decorations[underline_line].block_type =
                                        BlockType::UnorderedList;
                                    decorations[underline_line].syntax_ranges.push(SyntaxRange {
                                        range: marker_ws..marker_ws + marker_len,
                                        trigger_range: marker_ws..marker_ws + marker_len,
                                    });
                                }
                                continue;
                            }
                            for heading_line in line..underline_line {
                                decorations[heading_line].block_type = BlockType::Heading(lvl);
                            }
                            decorations[underline_line].syntax_ranges.push(SyntaxRange {
                                range: underline_range.clone(),
                                trigger_range: underline_range,
                            });
                        } else {
                            decorations[line].block_type = BlockType::Heading(lvl);
                        }
                    }
                    if let Some(attribute_range) = heading_attribute_range(line_content) {
                        decorations[line].syntax_ranges.push(SyntaxRange {
                            range: attribute_range.clone(),
                            trigger_range: attribute_range,
                        });
                    }
                }
            }
            Event::End(TagEnd::Heading(_)) => {}
            Event::Start(Tag::CodeBlock(kind)) => {
                let line = offset_to_line(source_range.start);
                in_code_block = true;
                in_fenced_code_block = matches!(kind, CodeBlockKind::Fenced(_));
                current_code_language = match &kind {
                    CodeBlockKind::Fenced(info) => {
                        let first = info.split_whitespace().next().unwrap_or_default().trim();
                        if first.is_empty() {
                            None
                        } else {
                            Some(first.to_string())
                        }
                    }
                    CodeBlockKind::Indented => None,
                };
                if current_code_language.as_deref() == Some("mermaid") {
                    open_mermaid_block = Some(OpenMermaidBlock {
                        start: source_range.start,
                        start_line: line,
                        source_text: String::new(),
                    });
                }
                if line < decorations.len() {
                    decorations[line].block_type = if in_fenced_code_block {
                        BlockType::CodeBlockFence
                    } else {
                        BlockType::CodeBlock
                    };
                    decorations[line].code_language = current_code_language.clone();
                    if in_fenced_code_block {
                        let line_content_len = text[source_range.start..]
                            .find('\n')
                            .unwrap_or(text.len() - source_range.start);
                        decorations[line].syntax_ranges.push(SyntaxRange {
                            range: 0..line_content_len,
                            trigger_range: 0..line_content_len,
                        });
                    }
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                if let Some(open_mermaid) = open_mermaid_block.take() {
                    let end_line = offset_to_line(source_range.end.saturating_sub(1));
                    assign_block_preview(
                        &mut decorations,
                        open_mermaid.start..source_range.end,
                        open_mermaid.start_line,
                        end_line,
                        BlockPreviewKind::Mermaid,
                        open_mermaid.source_text.trim().to_string(),
                    );
                }
                in_code_block = false;
                current_code_language = None;
                let line = offset_to_line(source_range.end.saturating_sub(1));
                if in_fenced_code_block
                    && line < decorations.len()
                    && decorations[line].block_type == BlockType::Normal
                {
                    decorations[line].block_type = BlockType::CodeBlockFence;
                }
                in_fenced_code_block = false;
            }
            Event::DisplayMath(content) => {
                if table_cell_math {
                    add_inline_math_span(
                        &mut decorations,
                        &line_starts,
                        &offset_to_line_col,
                        source_range.start,
                        source_range.end,
                        text,
                    );
                    continue;
                }
                display_math_ranges.push(source_range.clone());
                let start_line = offset_to_line(source_range.start);
                let end_line = offset_to_line(source_range.end.saturating_sub(1));
                assign_block_preview(
                    &mut decorations,
                    source_range.clone(),
                    start_line,
                    end_line,
                    BlockPreviewKind::Math,
                    content.trim().to_string(),
                );
            }
            Event::InlineMath(_) => {
                add_inline_math_span(
                    &mut decorations,
                    &line_starts,
                    &offset_to_line_col,
                    source_range.start,
                    source_range.end,
                    text,
                );
            }
            Event::Html(_) => {
                add_protected_range_for_abs_range(
                    &mut decorations,
                    &line_starts,
                    &offset_to_line_col,
                    source_range.start,
                    source_range.end,
                    text,
                );
            }
            Event::InlineHtml(source) => {
                if let Some((tag, closing, self_closing)) =
                    protected_inline_html_tag(source.as_ref())
                {
                    if self_closing {
                        add_protected_range_for_abs_range(
                            &mut decorations,
                            &line_starts,
                            &offset_to_line_col,
                            source_range.start,
                            source_range.end,
                            text,
                        );
                    } else if closing {
                        if let Some(position) = protected_html_stack
                            .iter()
                            .rposition(|(open_tag, _)| *open_tag == tag)
                        {
                            let (_, start) = protected_html_stack.remove(position);
                            add_protected_range_for_abs_range(
                                &mut decorations,
                                &line_starts,
                                &offset_to_line_col,
                                start,
                                source_range.end,
                                text,
                            );
                        }
                    } else {
                        protected_html_stack.push((tag, source_range.start));
                    }
                }
            }
            Event::Start(Tag::BlockQuote(_)) => {
                blockquote_depth += 1;
                push_block_context_for_range(
                    &mut decorations,
                    &line_starts,
                    source_range.start,
                    source_range.end,
                    BlockContext::Blockquote,
                );
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                blockquote_depth = blockquote_depth.saturating_sub(1);
            }
            Event::Start(Tag::List(start)) => {
                let list_context = if start.is_some() {
                    BlockContext::OrderedList
                } else {
                    BlockContext::UnorderedList
                };
                list_stack.push(list_context);
                push_block_context_for_range(
                    &mut decorations,
                    &line_starts,
                    source_range.start,
                    source_range.end,
                    list_context,
                );
            }
            Event::End(TagEnd::List(_)) => {
                list_stack.pop();
            }
            Event::Start(Tag::Item) => {
                push_block_context_for_range(
                    &mut decorations,
                    &line_starts,
                    source_range.start,
                    source_range.end,
                    BlockContext::ListItem,
                );
                let line = offset_to_line(source_range.start);
                if line < decorations.len() {
                    let content = {
                        let start = line_starts[line];
                        let end = if line + 1 < line_starts.len() {
                            line_starts[line + 1].min(text.len())
                        } else {
                            text.len()
                        };
                        &text[start..end]
                    };
                    let current_list_context = list_stack.last().copied();
                    // Inside a blockquote the marker follows `> `, so measure
                    // the indent from after that prefix or the marker range
                    // would be empty and render as plain text.
                    let quote_prefix_len = blockquote_prefix_len(content);
                    let after_quote = &content[quote_prefix_len..];
                    let leading_ws =
                        after_quote.len() - after_quote.trim_start_matches([' ', '\t']).len();
                    let marker_start = quote_prefix_len + leading_ws;
                    let trimmed = &after_quote[leading_ws..];
                    let ordered_prefix_len = ordered_list_prefix_len(trimmed);
                    let unordered_prefix_len = unordered_list_prefix_len(trimmed);
                    // A nested list that opens on the same physical line (`- 1.`)
                    // fires a second Item event for that line; the outermost
                    // marker owns the line prefix, so don't retype it.
                    let marker_already_recorded = decorations[line]
                        .syntax_ranges
                        .iter()
                        .any(|syntax_range| syntax_range.range.start == marker_start);
                    if marker_already_recorded {
                        // Keep the classification the outer marker established.
                    } else if ordered_prefix_len > 0 {
                        decorations[line].block_type = BlockType::OrderedList;
                        decorations[line].syntax_ranges.push(SyntaxRange {
                            range: marker_start..marker_start + ordered_prefix_len,
                            trigger_range: marker_start..marker_start + ordered_prefix_len,
                        });
                    } else if unordered_prefix_len > 0 {
                        decorations[line].block_type = BlockType::UnorderedList;
                        decorations[line].syntax_ranges.push(SyntaxRange {
                            range: marker_start..marker_start + unordered_prefix_len,
                            trigger_range: marker_start..marker_start + unordered_prefix_len,
                        });
                    } else if current_list_context == Some(BlockContext::OrderedList) {
                        decorations[line].block_type = BlockType::OrderedList;
                    } else if current_list_context == Some(BlockContext::UnorderedList) {
                        decorations[line].block_type = BlockType::UnorderedList;
                    }
                }
            }
            Event::Start(Tag::Emphasis) => {
                inline_stack.push(OpenInline::Plain {
                    kind: InlineKind::Italic,
                    start: source_range.start,
                    target: None,
                });
            }
            Event::End(TagEnd::Emphasis) => {
                if let Some(OpenInline::Plain {
                    kind,
                    start,
                    target: _,
                }) = inline_stack.pop()
                {
                    if kind == InlineKind::Italic {
                        add_inline_span(
                            &mut decorations,
                            &line_starts,
                            &offset_to_line_col,
                            start,
                            source_range.end,
                            InlineKind::Italic,
                            None,
                            text,
                        );
                    }
                }
            }
            Event::Start(Tag::Strong) => {
                inline_stack.push(OpenInline::Plain {
                    kind: InlineKind::Bold,
                    start: source_range.start,
                    target: None,
                });
            }
            Event::End(TagEnd::Strong) => {
                if let Some(OpenInline::Plain {
                    kind,
                    start,
                    target: _,
                }) = inline_stack.pop()
                {
                    if kind == InlineKind::Bold {
                        add_inline_span(
                            &mut decorations,
                            &line_starts,
                            &offset_to_line_col,
                            start,
                            source_range.end,
                            InlineKind::Bold,
                            None,
                            text,
                        );
                    }
                }
            }
            Event::Start(Tag::Strikethrough) => {
                inline_stack.push(OpenInline::Plain {
                    kind: InlineKind::Strikethrough,
                    start: source_range.start,
                    target: None,
                });
            }
            Event::End(TagEnd::Strikethrough) => {
                if let Some(OpenInline::Plain {
                    kind,
                    start,
                    target: _,
                }) = inline_stack.pop()
                {
                    if kind == InlineKind::Strikethrough {
                        let (start_line, _) = offset_to_line_col(start);
                        let (end_line, _) = offset_to_line_col(source_range.end);
                        if start_line != end_line {
                            continue;
                        }
                        add_inline_span(
                            &mut decorations,
                            &line_starts,
                            &offset_to_line_col,
                            start,
                            source_range.end,
                            InlineKind::Strikethrough,
                            None,
                            text,
                        );
                    }
                }
            }
            Event::Start(Tag::Link {
                link_type,
                dest_url,
                ..
            }) => {
                if matches!(link_type, LinkType::WikiLink { .. }) {
                    inline_stack.push(OpenInline::WikiLink {
                        start: source_range.start,
                    });
                } else {
                    inline_stack.push(OpenInline::Plain {
                        kind: InlineKind::Link,
                        start: source_range.start,
                        target: Some(dest_url.to_string()),
                    });
                }
            }
            Event::End(TagEnd::Link) => {
                if let Some(open_inline) = inline_stack.pop() {
                    match open_inline {
                        OpenInline::Plain {
                            kind,
                            start,
                            target,
                        } if kind == InlineKind::Link => {
                            add_inline_span(
                                &mut decorations,
                                &line_starts,
                                &offset_to_line_col,
                                start,
                                source_range.end,
                                InlineKind::Link,
                                target,
                                text,
                            );
                        }
                        OpenInline::WikiLink { start } => {
                            add_wiki_link_span(
                                &mut decorations,
                                &line_starts,
                                &offset_to_line_col,
                                start,
                                source_range.end,
                                text,
                            );
                        }
                        _ => {}
                    }
                }
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                inline_stack.push(OpenInline::Image {
                    start: source_range.start,
                    dest_url: dest_url.to_string(),
                });
            }
            Event::End(TagEnd::Image) => {
                if let Some(OpenInline::Image { start, dest_url }) = inline_stack.pop() {
                    add_inline_span(
                        &mut decorations,
                        &line_starts,
                        &offset_to_line_col,
                        start,
                        source_range.end,
                        InlineKind::Image,
                        Some(dest_url.clone()),
                        text,
                    );
                    // Store image path for preview rendering
                    let line = offset_to_line(start);
                    if line < decorations.len() {
                        decorations[line].image_path = Some(dest_url);
                    }
                }
            }
            Event::Start(Tag::FootnoteDefinition(label)) => {
                let line = offset_to_line(source_range.start);
                if line < decorations.len() {
                    decorations[line].footnote_definition_label = Some(label.to_string());
                    let line_content =
                        get_line_content(text, &line_starts, line, decorations.len());
                    let prefix = format!("[^{}]:", label);
                    if let Some(prefix_start) = line_content.find(&prefix) {
                        let prefix_end = prefix_start + prefix.len();
                        let trigger_end = if line_content.as_bytes().get(prefix_end) == Some(&b' ')
                        {
                            prefix_end + 1
                        } else {
                            prefix_end
                        };
                        let label_start = prefix_start + 2;
                        let label_end = label_start + label.len();
                        let trigger_range = prefix_start..trigger_end;
                        decorations[line].syntax_ranges.push(SyntaxRange {
                            range: prefix_start..label_start,
                            trigger_range: trigger_range.clone(),
                        });
                        decorations[line].syntax_ranges.push(SyntaxRange {
                            range: label_end..trigger_end,
                            trigger_range: trigger_range.clone(),
                        });
                        decorations[line]
                            .display_replacements
                            .push(DisplayReplacement {
                                source_range: trigger_range.clone(),
                                display_text: if trigger_end > prefix_end {
                                    format!("{}: ", label)
                                } else {
                                    format!("{}:", label)
                                },
                                trigger_range,
                            });
                    }
                }
            }
            Event::FootnoteReference(label) => {
                add_footnote_reference(
                    &mut decorations,
                    &line_starts,
                    &offset_to_line_col,
                    source_range.start,
                    source_range.end,
                    label.as_ref(),
                    text,
                );
            }
            Event::Code(_) => {
                add_inline_code_span(
                    &mut decorations,
                    &offset_to_line_col,
                    source_range.start,
                    source_range.end,
                    text,
                );
            }
            Event::Text(content) => {
                if in_code_block {
                    if let Some(open_mermaid) = open_mermaid_block.as_mut() {
                        open_mermaid.source_text.push_str(content.as_ref());
                    }
                    let start_line = offset_to_line(source_range.start);
                    let end_line = offset_to_line(source_range.end.saturating_sub(1));
                    for line in start_line..=end_line.min(decorations.len().saturating_sub(1)) {
                        if decorations[line].block_type == BlockType::Normal {
                            decorations[line].block_type = BlockType::CodeBlock;
                        }
                        decorations[line].code_language = current_code_language.clone();
                    }
                } else if blockquote_depth > 0 {
                    let line = offset_to_line(source_range.start);
                    if line < decorations.len() && decorations[line].block_type == BlockType::Normal
                    {
                        decorations[line].block_type = BlockType::Blockquote;
                        // Add '> ' prefix to syntax_ranges for folding
                        let line_start = line_starts[line];
                        let line_end_offset =
                            line_starts.get(line + 1).copied().unwrap_or(text.len());
                        let line_content = &text[line_start..line_end_offset];
                        let prefix_len = blockquote_prefix_len(line_content);
                        if prefix_len > 0 {
                            decorations[line].syntax_ranges.push(SyntaxRange {
                                range: 0..prefix_len,
                                trigger_range: 0..prefix_len,
                            });
                        }
                    }
                }
            }
            Event::Rule => {
                let line = offset_to_line(source_range.start);
                if line < decorations.len() {
                    decorations[line].block_type = BlockType::HorizontalRule;
                    // Add the entire HR content to syntax_ranges for folding
                    let line_start = line_starts[line];
                    let line_end_offset = line_starts.get(line + 1).copied().unwrap_or(text.len());
                    let mut line_len = line_end_offset - line_start;
                    if line_len > 0
                        && text.as_bytes().get(line_start + line_len - 1) == Some(&b'\n')
                    {
                        line_len -= 1;
                    }
                    if line_len > 0 {
                        decorations[line].syntax_ranges.push(SyntaxRange {
                            range: 0..line_len,
                            trigger_range: 0..line_len,
                        });
                    }
                }
            }
            Event::TaskListMarker(checked) => {
                let line = offset_to_line(source_range.start);
                if line < decorations.len() {
                    let line_start = line_starts[line];
                    let line_end_offset = line_starts.get(line + 1).copied().unwrap_or(text.len());
                    let line_content = &text[line_start..line_end_offset];
                    let quote_prefix_len = blockquote_prefix_len(line_content);
                    let after_quote = &line_content[quote_prefix_len..];
                    let leading_ws = quote_prefix_len
                        + (after_quote.len() - after_quote.trim_start_matches([' ', '\t']).len());
                    let marker = &line_content[leading_ws..];
                    let marker_len =
                        unordered_list_prefix_len(marker).max(ordered_list_prefix_len(marker));
                    if marker_len == 0 || line_start + leading_ws + marker_len != source_range.start
                    {
                        continue;
                    }
                    decorations[line].block_type = BlockType::TaskList(checked);
                    // Add checkbox marker ([ ] or [x]) to syntax ranges.
                    let mut col_end = source_range.end.saturating_sub(line_start);
                    // Include trailing space after ] if present
                    if col_end < line_content.len() && line_content.as_bytes()[col_end] == b' ' {
                        col_end += 1;
                    }
                    decorations[line]
                        .syntax_ranges
                        .retain(|sr| sr.range.start != leading_ws);
                    decorations[line].syntax_ranges.push(SyntaxRange {
                        range: leading_ws..col_end,
                        trigger_range: leading_ws..col_end,
                    });
                }
            }
            _ => {}
        }
    }

    assign_fenced_math_block_previews(&mut decorations, text, &line_starts);
    decorate_math_block_previews_as_code_blocks(&mut decorations, text, &line_starts);

    detect_callouts(&mut decorations, text, &line_starts);

    // Second pass: detect table lines
    detect_tables(&mut decorations, text, &line_starts);
    for source_range in display_math_ranges {
        let line = offset_to_line(source_range.start);
        let relative = source_range.start - line_starts[line]..source_range.end - line_starts[line];
        if decorations[line].table_info.as_ref().is_some_and(|table| {
            !table.is_front_matter
                && table
                    .cell_ranges
                    .iter()
                    .any(|cell| relative.start >= cell.start && relative.end <= cell.end)
        }) {
            decorations[line].block_preview = None;
            decorations[line].code_language = None;
            decorations[line]
                .syntax_ranges
                .retain(|syntax| syntax.range != relative);
            add_inline_math_span(
                &mut decorations,
                &line_starts,
                &offset_to_line_col,
                source_range.start,
                source_range.end,
                text,
            );
        }
    }
    for decoration in &mut decorations {
        if decoration
            .table_info
            .as_ref()
            .is_some_and(|table| !table.is_front_matter && !table.is_separator)
        {
            decoration.table_reference_definitions = table_reference_definitions.clone();
            recover_table_cell_links(decoration);
        }
    }

    detect_raw_wiki_links(&mut decorations, text, &line_starts);
    detect_raw_footnote_references(&mut decorations, text, &line_starts);
    detect_bare_urls(&mut decorations, text, &line_starts);
    detect_underlines(&mut decorations, text, &line_starts);
    detect_highlights(&mut decorations, text, &line_starts);
    detect_superscript_spans(&mut decorations, text, &line_starts);
    detect_subscript_spans(&mut decorations, text, &line_starts);
    detect_emoji_shortcodes(&mut decorations, text, &line_starts);

    // CommonMark: an ordered-list marker can only interrupt a paragraph when
    // pulldown-cmark itself recognizes it as a list item. Lines that pulldown
    // left as plain paragraph text (lazy continuations such as
    // "windows is\n14. doors") must not be force-decorated as ordered lists.
    let line_count = decorations.len();
    let lazy_paragraph_continuation: Vec<bool> = (0..line_count)
        .map(|line| {
            if line == 0 {
                return false;
            }
            let current = &decorations[line];
            let previous = &decorations[line - 1];
            let current_plain = current.block_type == BlockType::Normal
                && current.table_info.is_none()
                && !current.block_contexts.contains(&BlockContext::ListItem);
            if !current_plain {
                return false;
            }
            let previous_content = get_line_content(text, &line_starts, line - 1, line_count);
            previous.block_type == BlockType::Normal
                && previous.table_info.is_none()
                && !previous_content.trim().is_empty()
        })
        .collect();

    let decoration_count = decorations.len();
    for (line, decoration) in decorations.iter_mut().enumerate() {
        if decoration.table_info.is_some() {
            continue;
        }

        if matches!(
            decoration.block_type,
            BlockType::CodeBlock | BlockType::CodeBlockFence
        ) {
            continue;
        }

        let start = line_starts[line];
        let end = line_starts.get(line + 1).copied().unwrap_or(text.len());
        let content = &text[start..end];
        let line_len = content.strip_suffix('\n').unwrap_or(content);
        if block_recovery_is_protected(decoration, line_len) {
            continue;
        }
        let leading_ws = line_len.len() - line_len.trim_start_matches([' ', '\t']).len();
        let trimmed = &line_len[leading_ws..];
        let prefix_len = ordered_list_prefix_len(trimmed);
        if prefix_len > 0
            && matches!(
                decoration.block_type,
                BlockType::Normal | BlockType::OrderedList | BlockType::UnorderedList
            )
            && !lazy_paragraph_continuation[line]
            && (leading_ws <= 3
                || matches!(
                    decoration.block_type,
                    BlockType::OrderedList | BlockType::UnorderedList | BlockType::TaskList(_)
                ))
        {
            decoration
                .syntax_ranges
                .retain(|syntax_range| syntax_range.range.start != leading_ws);
            decoration.block_type = BlockType::OrderedList;
            decoration.syntax_ranges.push(SyntaxRange {
                range: leading_ws..leading_ws + prefix_len,
                trigger_range: leading_ws..leading_ws + prefix_len,
            });
            continue;
        }

        let is_list_item = decoration.block_contexts.contains(&BlockContext::ListItem)
            || matches!(
                decoration.block_type,
                BlockType::OrderedList | BlockType::UnorderedList | BlockType::TaskList(_)
            );
        if !is_list_item {
            continue;
        }

        if decoration.block_type != BlockType::Normal {
            continue;
        }

        let unordered_prefix_len = unordered_list_prefix_len(trimmed);
        if unordered_prefix_len > 0 {
            decoration.block_type = BlockType::UnorderedList;
            decoration.syntax_ranges.push(SyntaxRange {
                range: leading_ws..leading_ws + unordered_prefix_len,
                trigger_range: leading_ws..leading_ws + unordered_prefix_len,
            });
        }
    }

    // CommonMark can treat an indented `- ` as a Setext underline. In an editor,
    // a trailing space on a single marker signals an in-progress nested list.
    // Recover that physical-line pattern at any nesting depth (and inside a
    // blockquote) without changing ordinary `Title\n-` / `Title\n---` headings.
    for line in 0..decorations.len().saturating_sub(1) {
        if (line..=line + 1).any(|candidate| {
            block_recovery_is_protected(
                &decorations[candidate],
                get_line_content(text, &line_starts, candidate, decoration_count),
            )
        }) {
            continue;
        }
        let content = get_line_content(text, &line_starts, line, decoration_count);
        let Some((first_marker_start, first_prefix_len)) = physical_unordered_list_prefix(content)
        else {
            continue;
        };
        let next_line = line + 1;
        let next_content = get_line_content(text, &line_starts, next_line, decorations.len());
        let Some((next_leading_ws, next_prefix_len)) =
            empty_unordered_list_item_prefix_len(next_content)
        else {
            continue;
        };

        let explicit_heading = decorations[line].syntax_ranges.iter().any(|range| {
            content
                .get(range.range.start..)
                .and_then(|source| source.strip_prefix("##"))
                .is_some_and(|rest| rest.is_empty() || rest.starts_with([' ', '\t', '\r']))
        });
        if matches!(
            decorations[line].block_type,
            BlockType::Normal | BlockType::UnorderedList
        ) || (decorations[line].block_type == BlockType::Heading(2) && !explicit_heading)
        {
            decorations[line].block_type = BlockType::UnorderedList;
            if !decorations[line].syntax_ranges.iter().any(|range| {
                range.range == (first_marker_start..first_marker_start + first_prefix_len)
            }) {
                decorations[line].syntax_ranges.push(SyntaxRange {
                    range: first_marker_start..first_marker_start + first_prefix_len,
                    trigger_range: first_marker_start..first_marker_start + first_prefix_len,
                });
            }
        }
        decorations[next_line].block_type = BlockType::UnorderedList;
        decorations[next_line].syntax_ranges.clear();
        decorations[next_line].syntax_ranges.push(SyntaxRange {
            range: next_leading_ws..next_leading_ws + next_prefix_len,
            trigger_range: next_leading_ws..next_leading_ws + next_prefix_len,
        });
    }

    // pulldown-cmark emits no blockquote event for an empty in-progress `> `
    // line. Promote that physical prefix immediately so the editor responds
    // while the user is still typing the quote body.
    let decoration_count = decorations.len();
    for (line, decoration) in decorations.iter_mut().enumerate() {
        if decoration.block_type != BlockType::Normal || decoration.table_info.is_some() {
            continue;
        }
        let content = get_line_content(text, &line_starts, line, decoration_count);
        if block_recovery_is_protected(decoration, content) {
            continue;
        }
        if blockquote_prefix_len(content) > 0 {
            decoration.block_type = BlockType::Blockquote;
            if !decoration
                .block_contexts
                .contains(&BlockContext::Blockquote)
            {
                decoration.block_contexts.push(BlockContext::Blockquote);
            }
        }
    }

    // Conceal the `>` markers on every blockquote line — including list rows,
    // blank `>` separators, and nested content, which the event loop above
    // does not cover — so an uninterrupted run of quote lines renders as one
    // merged quote block instead of bare `>` characters.
    for (line, decoration) in decorations.iter_mut().enumerate() {
        let in_blockquote = decoration.block_type == BlockType::Blockquote
            || decoration
                .block_contexts
                .contains(&BlockContext::Blockquote);
        if !in_blockquote || decoration.table_info.is_some() {
            continue;
        }
        let start = line_starts[line];
        let end = line_starts.get(line + 1).copied().unwrap_or(text.len());
        let content = &text[start..end];
        let content = content.strip_suffix('\n').unwrap_or(content);
        let prefix_len = blockquote_prefix_len(content);
        if prefix_len == 0 {
            continue;
        }
        let covered = decoration.syntax_ranges.iter().any(|syntax_range| {
            syntax_range.range.start == 0 && syntax_range.range.end >= prefix_len
        });
        if !covered {
            decoration.syntax_ranges.insert(
                0,
                SyntaxRange {
                    range: 0..prefix_len,
                    trigger_range: 0..prefix_len,
                },
            );
        }
    }

    for decoration in &mut decorations {
        if decoration
            .payload
            .as_deref()
            .is_some_and(LineDecorationPayload::is_empty)
        {
            decoration.payload = None;
        }
    }
    decorations
}

fn recover_table_cell_links(row: &mut LineDecoration) {
    let recovered_kind = |kind: &InlineKind| {
        matches!(
            kind,
            InlineKind::Link | InlineKind::Image | InlineKind::InlineCode
        )
    };
    // GFM splits protected pipes differently from semantic table cells. Drop
    // its link/code metadata first, including false links inside split code.
    let old_sources = row
        .inline_spans
        .iter()
        .filter(|span| recovered_kind(&span.kind))
        .filter_map(|span| inline_span_source_range(row, span))
        .collect::<Vec<_>>();
    row.syntax_ranges
        .retain(|syntax| !old_sources.contains(&syntax.trigger_range));
    row.inline_spans.retain(|span| !recovered_kind(&span.kind));
    row.protected_ranges.clear();
    let cell_count = row.table_info.as_ref().map_or(0, |table| table.cells.len());
    for index in 0..cell_count {
        let table = row.table_info.as_ref().unwrap();
        let cell = &table.cells[index];
        let cell_start = table.cell_ranges[index].start;
        let cell_len = cell.len();
        // Use the semantic cell rather than pulldown's GFM columns: unescaped
        // pipes in code and Wiki aliases belong to the cell in this editor.
        let parsed = parse_table_cell_decorations(&format!("x {cell}"), row)
            .into_iter()
            .next()
            .unwrap_or_default();
        let project = |range: &Range<usize>| {
            (range.start >= 2 && range.end <= cell_len + 2)
                .then(|| cell_start + range.start - 2..cell_start + range.end - 2)
        };
        // In particular, math after a protected pipe may have no native GFM
        // event. Preserve its opacity for the row's later Wiki/custom scans.
        row.protected_ranges
            .extend(parsed.protected_ranges.iter().filter_map(&project));
        for span in parsed
            .inline_spans
            .iter()
            .filter(|span| recovered_kind(&span.kind))
        {
            let Some(range) = project(&span.range) else {
                continue;
            };
            if !row.inline_spans.iter().any(|existing| {
                existing.kind == span.kind
                    && existing.range == range
                    && existing.target == span.target
            }) {
                row.inline_spans.push(InlineSpan {
                    range,
                    ..span.clone()
                });
            }
            let Some(source) = inline_span_source_range(&parsed, span) else {
                continue;
            };
            for syntax in parsed
                .syntax_ranges
                .iter()
                .filter(|syntax| syntax.trigger_range == source)
            {
                let (Some(range), Some(trigger_range)) = (
                    project(&syntax.range),
                    project(&(syntax.trigger_range.start.max(2)..syntax.trigger_range.end)),
                ) else {
                    continue;
                };
                if !row.syntax_ranges.iter().any(|existing| {
                    existing.range == range && existing.trigger_range == trigger_range
                }) {
                    row.syntax_ranges.push(SyntaxRange {
                        range,
                        trigger_range,
                    });
                }
            }
        }
    }
    row.inline_spans.sort_by_key(|span| span.range.start);
}

fn decorate_math_block_previews_as_code_blocks(
    decorations: &mut [LineDecoration],
    text: &str,
    line_starts: &[usize],
) {
    let line_count = decorations.len();
    if line_count == 0 {
        return;
    }

    for anchor_line in 0..line_count {
        let Some(preview) = decorations[anchor_line].block_preview.clone() else {
            continue;
        };
        if !preview.is_anchor || preview.kind != BlockPreviewKind::Math {
            continue;
        }

        let start_line = preview.start_line.min(line_count.saturating_sub(1));
        let end_line = preview.end_line.min(line_count.saturating_sub(1));
        let start_col = preview.source_range.start.saturating_sub(
            *line_starts
                .get(start_line)
                .unwrap_or(&preview.source_range.start),
        );
        let end_line_start = line_starts
            .get(end_line)
            .copied()
            .unwrap_or(preview.source_range.end);
        let end_col = preview.source_range.end.saturating_sub(end_line_start);

        for line in start_line..=end_line {
            let line_len = get_line_content(text, line_starts, line, line_count).len();
            decorations[line].code_language = Some("latex".to_string());

            if start_line == end_line {
                decorations[line].block_type = BlockType::CodeBlockFence;
                let range_start = start_col.min(line_len);
                let range_end = end_col.min(line_len).max(range_start);
                decorations[line].syntax_ranges.push(SyntaxRange {
                    range: range_start..range_end,
                    trigger_range: range_start..range_end,
                });
            } else if line == start_line {
                decorations[line].block_type = BlockType::CodeBlockFence;
                let range_start = start_col.min(line_len);
                decorations[line].syntax_ranges.push(SyntaxRange {
                    range: range_start..line_len,
                    trigger_range: range_start..line_len,
                });
            } else if line == end_line {
                decorations[line].block_type = BlockType::CodeBlockFence;
                let range_end = end_col.min(line_len);
                decorations[line].syntax_ranges.push(SyntaxRange {
                    range: 0..range_end,
                    trigger_range: 0..range_end,
                });
            } else {
                decorations[line].block_type = BlockType::CodeBlock;
            }
        }
    }
}

fn assign_fenced_math_block_previews(
    decorations: &mut [LineDecoration],
    text: &str,
    line_starts: &[usize],
) {
    let line_count = decorations.len();
    if line_count == 0 {
        return;
    }

    let mut line = 0;
    while line < line_count {
        let opener = get_line_content(text, line_starts, line, line_count);
        let trimmed_opener = opener.trim();
        if decorations[line].block_type != BlockType::Normal
            || block_marker_is_protected(&decorations[line], opener)
        {
            line += 1;
            continue;
        }

        if trimmed_opener.starts_with("\\[")
            && trimmed_opener.ends_with("\\]")
            && trimmed_opener.len() > 4
        {
            let start_col = opener.find("\\[").unwrap_or(0);
            let end_col = opener.rfind("\\]").unwrap_or(start_col) + 2;
            let source_text = opener[start_col + 2..end_col - 2].trim();
            assign_block_preview(
                decorations,
                (line_starts[line] + start_col)..(line_starts[line] + end_col),
                line,
                line,
                BlockPreviewKind::Math,
                source_text.to_string(),
            );
            line += 1;
            continue;
        }

        let (open_marker, close_marker) = match trimmed_opener {
            "$$" => ("$$", "$$"),
            "\\[" => ("\\[", "\\]"),
            _ => {
                line += 1;
                continue;
            }
        };
        if decorations[line]
            .block_preview
            .as_ref()
            .is_some_and(|preview| preview.kind == BlockPreviewKind::Math)
        {
            line += 1;
            continue;
        }

        let mut close_line = line + 1;
        while close_line < line_count {
            let closer = get_line_content(text, line_starts, close_line, line_count);
            if closer.trim() == close_marker
                && decorations[close_line].block_type == BlockType::Normal
                && !block_marker_is_protected(&decorations[close_line], closer)
            {
                break;
            }
            close_line += 1;
        }

        if close_line >= line_count {
            line += 1;
            continue;
        }

        let already_has_math_preview = (line..=close_line).any(|idx| {
            decorations[idx]
                .block_preview
                .as_ref()
                .is_some_and(|preview| preview.kind == BlockPreviewKind::Math)
        });

        if !already_has_math_preview {
            let source_text = if close_line > line + 1 {
                ((line + 1)..close_line)
                    .map(|idx| get_line_content(text, line_starts, idx, line_count))
                    .collect::<Vec<_>>()
                    .join("\n")
            } else {
                String::new()
            };
            let start_col = opener.find(open_marker).unwrap_or(0);
            let end_col = get_line_content(text, line_starts, close_line, line_count)
                .rfind(close_marker)
                .unwrap_or(0)
                + close_marker.len();
            let source_range = (line_starts[line] + start_col)..(line_starts[close_line] + end_col);
            assign_block_preview(
                decorations,
                source_range,
                line,
                close_line,
                BlockPreviewKind::Math,
                source_text.trim().to_string(),
            );
        }

        line = close_line + 1;
    }
}

fn detect_front_matter(decorations: &mut [LineDecoration], text: &str, line_starts: &[usize]) {
    let line_count = decorations.len();
    if line_count < 2 {
        return;
    }

    let opening = get_line_content(text, line_starts, 0, line_count)
        .trim_start_matches('\u{feff}')
        .trim();
    let compact_delimiter = match opening {
        "---" => false,
        "-" => true,
        _ => return,
    };

    let Some(closing_line) = (1..line_count).find(|&line| {
        // Indented delimiters can be literal scalar content or sequence items.
        let candidate = get_line_content(text, line_starts, line, line_count).trim_end();
        if compact_delimiter {
            candidate == "-"
        } else {
            matches!(candidate, "---" | "...")
        }
    }) else {
        return;
    };

    let metadata_lines = (1..closing_line)
        .map(|line| get_line_content(text, line_starts, line, line_count))
        .collect::<Vec<_>>();
    let metadata = metadata_lines.join("\n");
    let valid_mapping = serde_yaml::from_str::<serde_yaml::Value>(&metadata)
        .ok()
        .and_then(|value| match value {
            serde_yaml::Value::Mapping(mapping) => Some(!mapping.is_empty()),
            _ => None,
        })
        .unwrap_or(false);
    let has_mapping_entry = metadata_lines
        .iter()
        .any(|line| front_matter::has_mapping_key(line));
    if !valid_mapping && !has_mapping_entry {
        return;
    }

    let table_start = 0;
    let table_end = closing_line + 1;
    let line_infos = front_matter::parse_lines(&metadata_lines);
    let mut parsed_rows: Vec<(Vec<String>, Vec<Range<usize>>, bool)> = Vec::new();
    let mut key_width = 3usize;
    let mut value_width = 3usize;

    for line in table_start..table_end {
        let content = get_line_content(text, line_starts, line, line_count);
        if line == table_start || line == closing_line {
            parsed_rows.push((Vec::new(), Vec::new(), true));
            continue;
        }

        let info = &line_infos[line - 1];
        let key_range = info.key_range.clone().unwrap_or(0..0);
        let value_range = info.value_range.clone();
        let key = content[key_range.clone()].to_string();
        let value = content[value_range.clone()].to_string();
        key_width = key_width.max(cell_display_width(&key));
        value_width = value_width.max(cell_display_width(&value));
        parsed_rows.push((vec![key, value], vec![key_range, value_range], false));
    }

    let column_widths = vec![key_width, value_width];
    for (row_index, line) in (table_start..table_end).enumerate() {
        let content = get_line_content(text, line_starts, line, line_count);
        let (cells, cell_ranges, is_separator) = &parsed_rows[row_index];
        // Markdown has already parsed these rows; YAML owns their presentation.
        if let Some(payload) = decorations[line].payload_mut() {
            *payload = LineDecorationPayload::default();
        }
        decorations[line].block_type = BlockType::Table;
        decorations[line].front_matter = if *is_separator {
            None
        } else {
            Some(line_infos[line - 1].clone())
        };
        decorations[line].table_info = Some(TableLineInfo {
            cells: cells.clone(),
            cell_ranges: cell_ranges.clone(),
            column_alignments: vec![TableAlign::Left; column_widths.len()],
            is_header: false,
            is_separator: *is_separator,
            is_front_matter: true,
            table_start_line: table_start,
            table_end_line: table_end,
            column_widths: column_widths.clone(),
        });
        decorations[line].syntax_ranges.push(SyntaxRange {
            range: 0..content.len(),
            trigger_range: 0..content.len(),
        });
        if !content.is_empty() {
            decorations[line].protected_ranges.push(0..content.len());
        }
    }
}

fn parse_callout_kind(label: &str) -> Option<CalloutKind> {
    match label.to_ascii_uppercase().as_str() {
        "NOTE" => Some(CalloutKind::Note),
        "TIP" => Some(CalloutKind::Tip),
        "IMPORTANT" => Some(CalloutKind::Important),
        "WARNING" => Some(CalloutKind::Warning),
        "CAUTION" => Some(CalloutKind::Caution),
        _ => None,
    }
}

fn callout_replacement_text(kind: CalloutKind) -> &'static str {
    // One em plus one thin space reserves approximately a 1em icon and its label gap.
    match kind {
        CalloutKind::Note => "\u{2003}\u{2009}",
        CalloutKind::Tip => "\u{2003}\u{2009}",
        CalloutKind::Important => "\u{2003}\u{2009}",
        CalloutKind::Warning => "\u{2003}\u{2009}",
        CalloutKind::Caution => "\u{2003}\u{2009}",
    }
}

fn default_callout_title(kind: CalloutKind) -> &'static str {
    match kind {
        CalloutKind::Note => "NOTE",
        CalloutKind::Tip => "TIP",
        CalloutKind::Important => "IMPORTANT",
        CalloutKind::Warning => "WARNING",
        CalloutKind::Caution => "CAUTION",
    }
}

fn detect_callouts(decorations: &mut [LineDecoration], text: &str, line_starts: &[usize]) {
    let line_count = decorations.len();
    let quote_lines = decorations
        .iter()
        .map(|decoration| {
            decoration.block_type == BlockType::Blockquote
                || decoration
                    .block_contexts
                    .contains(&BlockContext::Blockquote)
        })
        .collect::<Vec<_>>();
    for (line, decoration) in decorations.iter_mut().enumerate() {
        if line > 0 && quote_lines[line - 1] {
            continue;
        }
        if !decoration
            .block_contexts
            .contains(&BlockContext::Blockquote)
            && decoration.block_type != BlockType::Blockquote
        {
            continue;
        }
        let line_content = get_line_content(text, line_starts, line, line_count);
        let leading_ws = line_content.len() - line_content.trim_start_matches([' ', '\t']).len();
        let Some(after_quote) = line_content.get(leading_ws..) else {
            continue;
        };
        if !after_quote.starts_with('>') {
            continue;
        }
        let mut marker_start = leading_ws + 1;
        if line_content.as_bytes().get(marker_start) == Some(&b' ') {
            marker_start += 1;
        }
        let Some(rest) = line_content.get(marker_start..) else {
            continue;
        };
        if !rest.starts_with("[!") {
            continue;
        }
        let Some(close_relative) = rest.find(']') else {
            continue;
        };
        let label = &rest[2..close_relative];
        let Some(kind) = parse_callout_kind(label) else {
            continue;
        };
        let marker_end = marker_start + close_relative + 1;
        let replacement_end = line_content.as_bytes()[marker_end..]
            .iter()
            .take_while(|&&byte| matches!(byte, b' ' | b'\t'))
            .count()
            + marker_end;
        decoration.callout_kind = Some(kind);
        let title_end = line_content.trim_end_matches([' ', '\t', '\r']).len();
        let mut display_text = callout_replacement_text(kind).to_string();
        let title_range = if replacement_end >= title_end {
            marker_start..replacement_end
        } else {
            replacement_end..title_end
        };
        if replacement_end >= title_end {
            // No title text after `[!KIND]`: show the default kind label.
            display_text.push_str(default_callout_title(kind));
        }
        decoration.display_replacements.push(DisplayReplacement {
            source_range: marker_start..replacement_end,
            display_text,
            trigger_range: marker_start..replacement_end,
        });
        if title_range.start < title_range.end {
            decoration.inline_spans.push(InlineSpan {
                kind: InlineKind::Bold,
                range: title_range,
                target: None,
            });
        }
    }
}

fn add_inline_span(
    decorations: &mut [LineDecoration],
    line_starts: &[usize],
    offset_to_line_col: &dyn Fn(usize) -> (usize, usize),
    start: usize,
    end: usize,
    kind: InlineKind,
    target: Option<String>,
    text: &str,
) {
    let (start_line, start_col) = offset_to_line_col(start);
    let (end_line, end_col) = offset_to_line_col(end);

    if start_line >= decorations.len() || start_line != end_line {
        if end_line >= decorations.len() {
            return;
        }

        if matches!(kind, InlineKind::Link | InlineKind::Image) {
            let source = &text[start..end];
            let prefix_len = if kind == InlineKind::Image { 2 } else { 1 };
            let bracket_pos = source
                .find("](")
                .or_else(|| link_text_close_offset(source, prefix_len));
            let Some(bracket_pos) = bracket_pos else {
                return;
            };
            let visible_start = start + prefix_len;
            let visible_end = start + bracket_pos;
            add_inline_span_for_abs_range(
                decorations,
                line_starts,
                offset_to_line_col,
                visible_start,
                visible_end,
                kind,
                target,
                text,
            );
            add_syntax_range_for_abs_range(
                decorations,
                line_starts,
                offset_to_line_col,
                start,
                visible_start,
                start,
                end,
                text,
            );
            add_syntax_range_for_abs_range(
                decorations,
                line_starts,
                offset_to_line_col,
                visible_end,
                end,
                start,
                end,
                text,
            );
            return;
        }

        let marker_len = match &kind {
            InlineKind::Bold => 2,
            InlineKind::Italic => 1,
            InlineKind::Strikethrough => 2,
            _ => return,
        };

        for line in start_line..=end_line {
            let line_content = get_line_content(text, line_starts, line, decorations.len());
            let line_len = line_content.len();
            let range = if line == start_line {
                start_col + marker_len..line_len
            } else if line == end_line {
                0..end_col.saturating_sub(marker_len)
            } else {
                0..line_len
            };
            if range.start < range.end {
                decorations[line].inline_spans.push(InlineSpan {
                    kind: kind.clone(),
                    range,
                    target: target.clone(),
                });
            }
        }

        let start_line_len =
            get_line_content(text, line_starts, start_line, decorations.len()).len();
        decorations[start_line].syntax_ranges.push(SyntaxRange {
            range: start_col..(start_col + marker_len).min(start_line_len),
            trigger_range: start_col..start_line_len,
        });
        if end_col >= marker_len {
            decorations[end_line].syntax_ranges.push(SyntaxRange {
                range: end_col - marker_len..end_col,
                trigger_range: 0..end_col,
            });
        }
        return;
    }

    let line_len = text[line_starts[start_line]..]
        .find('\n')
        .unwrap_or(text.len() - line_starts[start_line]);
    let line_content = &text[line_starts[start_line]..line_starts[start_line] + line_len];
    let trigger_start = line_content[..start_col]
        .char_indices()
        .next_back()
        .map_or(0, |(offset, _)| offset);
    let trigger_end = end_col
        + line_content[end_col..]
            .chars()
            .next()
            .map_or(0, char::len_utf8);
    let full_trigger = trigger_start..trigger_end;

    match &kind {
        InlineKind::Bold | InlineKind::Italic | InlineKind::Strikethrough => {
            let marker_len = match &kind {
                InlineKind::Bold => 2,
                InlineKind::Italic => 1,
                InlineKind::Strikethrough => 2,
                _ => 0,
            };
            decorations[start_line].inline_spans.push(InlineSpan {
                kind,
                range: start_col + marker_len..end_col.saturating_sub(marker_len),
                target,
            });
            decorations[start_line].syntax_ranges.push(SyntaxRange {
                range: start_col..start_col + marker_len,
                trigger_range: full_trigger.clone(),
            });
            decorations[start_line].syntax_ranges.push(SyntaxRange {
                range: end_col - marker_len..end_col,
                trigger_range: full_trigger,
            });
        }
        InlineKind::Link | InlineKind::Image => {
            let source = &text[start..end];
            let prefix_len = if kind == InlineKind::Image { 2 } else { 1 };
            if kind == InlineKind::Link && source.starts_with('<') && source.ends_with('>') {
                let link_start_col = start_col + 1;
                let link_end_col = end_col.saturating_sub(1);
                if link_start_col < link_end_col {
                    decorations[start_line].inline_spans.push(InlineSpan {
                        kind,
                        range: link_start_col..link_end_col,
                        target,
                    });
                }
                decorations[start_line].syntax_ranges.push(SyntaxRange {
                    range: start_col..link_start_col,
                    trigger_range: full_trigger.clone(),
                });
                decorations[start_line].syntax_ranges.push(SyntaxRange {
                    range: link_end_col..end_col,
                    trigger_range: full_trigger,
                });
            } else if let Some(bracket_pos) = source.find("](") {
                let text_end_col = start_col + bracket_pos;
                let desc_is_empty = bracket_pos == prefix_len;
                if desc_is_empty {
                    // No description: show the URL as visible text when folded
                    // Extract URL portion (between `](` and `)`)
                    let url_start_col = text_end_col + 2; // skip `](`
                    let url_end_col = end_col.saturating_sub(1); // skip trailing `)`
                    decorations[start_line].inline_spans.push(InlineSpan {
                        kind,
                        range: url_start_col..url_end_col,
                        target,
                    });
                    // Hide prefix `[` or `![`, `](` and trailing `)`
                    decorations[start_line].syntax_ranges.push(SyntaxRange {
                        range: start_col..url_start_col,
                        trigger_range: full_trigger.clone(),
                    });
                    decorations[start_line].syntax_ranges.push(SyntaxRange {
                        range: url_end_col..end_col,
                        trigger_range: full_trigger,
                    });
                } else {
                    decorations[start_line].inline_spans.push(InlineSpan {
                        kind,
                        range: start_col + prefix_len..text_end_col,
                        target,
                    });
                    // Hide opening `[` or `![`
                    decorations[start_line].syntax_ranges.push(SyntaxRange {
                        range: start_col..start_col + prefix_len,
                        trigger_range: full_trigger.clone(),
                    });
                    // Hide `](url)`
                    decorations[start_line].syntax_ranges.push(SyntaxRange {
                        range: text_end_col..end_col,
                        trigger_range: full_trigger,
                    });
                }
            } else if let Some(bracket_pos) = link_text_close_offset(source, prefix_len) {
                let text_start_col = start_col + prefix_len;
                let text_end_col = start_col + bracket_pos;
                if text_start_col < text_end_col {
                    decorations[start_line].inline_spans.push(InlineSpan {
                        kind,
                        range: text_start_col..text_end_col,
                        target,
                    });
                }
                decorations[start_line].syntax_ranges.push(SyntaxRange {
                    range: start_col..text_start_col,
                    trigger_range: full_trigger.clone(),
                });
                decorations[start_line].syntax_ranges.push(SyntaxRange {
                    range: text_end_col..end_col,
                    trigger_range: full_trigger,
                });
            }
        }
        _ => {}
    }
}

fn add_inline_span_for_abs_range(
    decorations: &mut [LineDecoration],
    line_starts: &[usize],
    offset_to_line_col: &dyn Fn(usize) -> (usize, usize),
    start: usize,
    end: usize,
    kind: InlineKind,
    target: Option<String>,
    text: &str,
) {
    if start >= end {
        return;
    }
    let (start_line, start_col) = offset_to_line_col(start);
    let (end_line, end_col) = offset_to_line_col(end);
    if start_line >= decorations.len() || end_line >= decorations.len() {
        return;
    }

    for line in start_line..=end_line {
        let line_len = get_line_content(text, line_starts, line, decorations.len()).len();
        let range = if line == start_line && line == end_line {
            start_col..end_col
        } else if line == start_line {
            start_col..line_len
        } else if line == end_line {
            0..end_col
        } else {
            0..line_len
        };
        if range.start < range.end {
            decorations[line].inline_spans.push(InlineSpan {
                kind: kind.clone(),
                range,
                target: target.clone(),
            });
        }
    }
}

fn add_syntax_range_for_abs_range(
    decorations: &mut [LineDecoration],
    line_starts: &[usize],
    offset_to_line_col: &dyn Fn(usize) -> (usize, usize),
    start: usize,
    end: usize,
    trigger_start: usize,
    trigger_end: usize,
    text: &str,
) {
    if start >= end {
        return;
    }
    let (start_line, start_col) = offset_to_line_col(start);
    let (end_line, end_col) = offset_to_line_col(end);
    if start_line >= decorations.len() || end_line >= decorations.len() {
        return;
    }
    let (trigger_start_line, trigger_start_col) = offset_to_line_col(trigger_start);
    let (trigger_end_line, trigger_end_col) = offset_to_line_col(trigger_end);

    for line in start_line..=end_line {
        let line_len = get_line_content(text, line_starts, line, decorations.len()).len();
        let range = if line == start_line && line == end_line {
            start_col..end_col
        } else if line == start_line {
            start_col..line_len
        } else if line == end_line {
            0..end_col
        } else {
            0..line_len
        };
        if range.start >= range.end {
            continue;
        }
        let trigger_range = if line == trigger_start_line && line == trigger_end_line {
            trigger_start_col..trigger_end_col
        } else if line == trigger_start_line {
            trigger_start_col..line_len
        } else if line == trigger_end_line {
            0..trigger_end_col
        } else {
            0..line_len
        };
        decorations[line].syntax_ranges.push(SyntaxRange {
            range,
            trigger_range,
        });
    }
}

fn add_protected_range_for_abs_range(
    decorations: &mut [LineDecoration],
    line_starts: &[usize],
    offset_to_line_col: &dyn Fn(usize) -> (usize, usize),
    start: usize,
    end: usize,
    text: &str,
) {
    if start >= end {
        return;
    }
    let (start_line, start_col) = offset_to_line_col(start);
    let (end_line, end_col) = offset_to_line_col(end);
    if start_line >= decorations.len() || end_line >= decorations.len() {
        return;
    }
    for line in start_line..=end_line {
        let line_len = get_line_content(text, line_starts, line, decorations.len()).len();
        let range = if line == start_line && line == end_line {
            start_col..end_col
        } else if line == start_line {
            start_col..line_len
        } else if line == end_line {
            0..end_col
        } else {
            0..line_len
        };
        if range.start < range.end {
            decorations[line].protected_ranges.push(range);
        }
    }
}

fn add_inline_math_span(
    decorations: &mut [LineDecoration],
    line_starts: &[usize],
    offset_to_line_col: &dyn Fn(usize) -> (usize, usize),
    start: usize,
    end: usize,
    text: &str,
) {
    add_protected_range_for_abs_range(
        decorations,
        line_starts,
        offset_to_line_col,
        start,
        end,
        text,
    );
    let source = &text[start..end];
    let marker_len = source.bytes().take_while(|&byte| byte == b'$').count().min(
        source
            .bytes()
            .rev()
            .take_while(|&byte| byte == b'$')
            .count(),
    );
    if marker_len == 0 {
        return;
    }
    let (start_line, start_col) = offset_to_line_col(start);
    let (end_line, end_col) = offset_to_line_col(end);
    if start_line >= decorations.len() || start_line != end_line {
        return;
    }
    let content_start = start_col + marker_len;
    let content_end = end_col.saturating_sub(marker_len);
    if content_start < content_end {
        decorations[start_line]
            .inline_math_spans
            .push(content_start..content_end);
    }
    let trigger_range = start_col..end_col;
    decorations[start_line].syntax_ranges.push(SyntaxRange {
        range: start_col..content_start,
        trigger_range: trigger_range.clone(),
    });
    decorations[start_line].syntax_ranges.push(SyntaxRange {
        range: content_end..end_col,
        trigger_range,
    });
}

fn code_span_delimiter_len(source: &str) -> usize {
    let leading = source.bytes().take_while(|&byte| byte == b'`').count();
    let trailing = source
        .bytes()
        .rev()
        .take_while(|&byte| byte == b'`')
        .count();
    leading.min(trailing)
}

fn add_inline_code_span(
    decorations: &mut [LineDecoration],
    offset_to_line_col: &dyn Fn(usize) -> (usize, usize),
    start: usize,
    end: usize,
    text: &str,
) {
    let source = &text[start..end];
    let marker_len = code_span_delimiter_len(source);
    if marker_len == 0 {
        return;
    }

    let (start_line, start_col) = offset_to_line_col(start);
    let (end_line, end_col) = offset_to_line_col(end);
    if start_line >= decorations.len() || end_line >= decorations.len() {
        return;
    }

    // Deliberate deviation from CommonMark: a code span must close on the line
    // it opened on. While editing, a stray backtick run would otherwise pair
    // with an unrelated run further down and swallow everything between them.
    if start_line != end_line {
        return;
    }

    let trigger = start_col..end_col;
    if start_col + marker_len <= end_col.saturating_sub(marker_len) {
        decorations[start_line].inline_spans.push(InlineSpan {
            kind: InlineKind::InlineCode,
            range: start_col + marker_len..end_col.saturating_sub(marker_len),
            target: None,
        });
    }
    decorations[start_line].syntax_ranges.push(SyntaxRange {
        range: start_col..start_col + marker_len,
        trigger_range: trigger.clone(),
    });
    decorations[start_line].syntax_ranges.push(SyntaxRange {
        range: end_col.saturating_sub(marker_len)..end_col,
        trigger_range: trigger,
    });
}

pub fn ordered_list_prefix_len(content: &str) -> usize {
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() && i < 9 {
        i += 1;
    }
    if i == 9 && bytes.get(i).is_some_and(|byte| byte.is_ascii_digit()) {
        return 0;
    }
    if i == 0 || i >= bytes.len() {
        return 0;
    }
    if !matches!(bytes[i], b'.' | b')') {
        return 0;
    }
    let marker_end = i + 1;
    if marker_end == bytes.len() {
        return marker_end;
    }
    if !matches!(bytes[marker_end], b' ' | b'\t') {
        return 0;
    }
    marker_end
        + bytes[marker_end..]
            .iter()
            .take_while(|&&byte| matches!(byte, b' ' | b'\t'))
            .count()
}

fn block_marker_is_protected(decoration: &LineDecoration, content: &str) -> bool {
    let quote_len = blockquote_prefix_len(content);
    let marker_start = quote_len + content[quote_len..].len()
        - content[quote_len..].trim_start_matches([' ', '\t']).len();
    decoration
        .protected_ranges
        .iter()
        .any(|range| range.contains(&marker_start))
}

fn block_recovery_is_protected(decoration: &LineDecoration, content: &str) -> bool {
    decoration.table_info.is_some()
        || matches!(
            decoration.block_type,
            BlockType::CodeBlock | BlockType::CodeBlockFence
        )
        || block_marker_is_protected(decoration, content)
}

fn get_line_content<'a>(
    text: &'a str,
    line_starts: &[usize],
    line: usize,
    _line_count: usize,
) -> &'a str {
    let start = line_starts[line];
    let end = if line + 1 < line_starts.len() {
        let e = line_starts[line + 1];
        if e > 0 && text.as_bytes().get(e - 1) == Some(&b'\n') {
            e - 1
        } else {
            e
        }
    } else {
        text.len()
    };
    &text[start..end]
}

fn table_row_source(content: &str) -> (&str, usize) {
    let mut offset = content.len() - content.trim_start_matches([' ', '\t']).len();
    let mut source = &content[offset..];
    while let Some(after_quote) = source.strip_prefix('>') {
        offset += 1;
        source = after_quote;
        if let Some(after_space) = source.strip_prefix(' ') {
            offset += 1;
            source = after_space;
        }
    }
    (source.trim_end_matches([' ', '\t', '\r']), offset)
}

fn backtick_run_len(text: &str, start: usize) -> usize {
    text.as_bytes()[start..]
        .iter()
        .take_while(|&&byte| byte == b'`')
        .count()
}

fn has_closing_backtick_run(text: &str, mut start: usize, expected: usize) -> bool {
    while start < text.len() {
        let Some(relative) = text[start..].find('`') else {
            return false;
        };
        let run_start = start + relative;
        let run_len = backtick_run_len(text, run_start);
        if run_len == expected {
            return true;
        }
        start = run_start + run_len;
    }
    false
}

/// Marker character and run length when `content` opens a fenced code block.
fn code_fence_opener(content: &str) -> Option<(char, usize)> {
    let leading = content.len() - content.trim_start_matches(' ').len();
    if leading > 3 {
        return None;
    }
    let rest = &content[leading..];
    let marker = rest.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let run = rest.chars().take_while(|ch| *ch == marker).count();
    if run < 3 {
        return None;
    }
    // A backtick fence's info string may not itself contain backticks.
    if marker == '`' && rest[run..].contains('`') {
        return None;
    }
    Some((marker, run))
}

pub(crate) fn newly_typed_fence_is_pending(
    text: &str,
    line: usize,
    previous: &[LineDecoration],
) -> bool {
    if previous.get(line).is_some_and(|decoration| {
        matches!(
            decoration.block_type,
            BlockType::CodeBlock | BlockType::CodeBlockFence
        )
    }) {
        return false;
    }
    let Some(content) = text.lines().nth(line) else {
        return false;
    };
    let Some((marker, run)) = code_fence_opener(content) else {
        return false;
    };
    if fenced_code_block_line_ranges(text).iter().any(|range| {
        range.start < line
            && range.end == line + 1
            && previous
                .get(range.start)
                .is_some_and(|decoration| decoration.block_type == BlockType::CodeBlockFence)
    }) {
        return false;
    }
    !text
        .lines()
        .enumerate()
        .skip(line + 1)
        .any(|(index, content)| {
            !previous.get(index).is_some_and(|decoration| {
                matches!(
                    decoration.block_type,
                    BlockType::CodeBlock | BlockType::CodeBlockFence
                )
            }) && is_code_fence_closer(content, marker, run)
        })
}

fn is_code_fence_closer(content: &str, marker: char, open_run: usize) -> bool {
    let leading = content.len() - content.trim_start_matches(' ').len();
    if leading > 3 {
        return false;
    }
    let rest = &content[leading..];
    let run = rest.chars().take_while(|ch| *ch == marker).count();
    run >= open_run && rest[run..].trim().is_empty()
}

pub(crate) fn fenced_code_block_line_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut open: Option<(usize, char, usize)> = None;
    for (line, content) in text.lines().enumerate() {
        match open {
            Some((start, marker, run)) => {
                if is_code_fence_closer(content, marker, run) {
                    ranges.push(start..line + 1);
                    open = None;
                }
            }
            None => {
                if let Some((marker, run)) = code_fence_opener(content) {
                    open = Some((line, marker, run));
                }
            }
        }
    }
    ranges
}

/// Byte range of the marker run of a fence that is never closed.
pub(crate) fn unclosed_code_fence_marker_span(text: &str) -> Option<Range<usize>> {
    for (event, range) in Parser::new(text).into_offset_iter() {
        if !matches!(
            event,
            Event::Start(Tag::CodeBlock(CodeBlockKind::Fenced(_)))
        ) {
            continue;
        }
        let mut lines = text[range.clone()].lines();
        let Some(opener) = lines.next() else { continue };
        let prefix = blockquote_prefix_len(opener);
        let Some((marker, run)) = code_fence_opener(&opener[prefix..]) else {
            continue;
        };
        if lines.last().is_some_and(|closer| {
            is_code_fence_closer(&closer[blockquote_prefix_len(closer)..], marker, run)
        }) {
            continue;
        }
        let leading = opener[prefix..].len() - opener[prefix..].trim_start_matches(' ').len();
        let start = range.start + prefix + leading;
        return Some(start..start + run);
    }
    None
}

/// The closing marker a fence opened on `line` still needs, or `None` when that
/// line does not open a fence or the fence is already closed.
pub fn unclosed_code_fence_marker(text: &str, line: usize) -> Option<String> {
    let mut open: Option<(char, usize)> = None;
    let mut opened_at = 0usize;

    for (index, content) in text.lines().enumerate() {
        match open {
            Some((marker, run)) => {
                if is_code_fence_closer(content, marker, run) {
                    if opened_at == line {
                        return None;
                    }
                    open = None;
                }
            }
            None => {
                if let Some((marker, run)) = code_fence_opener(content) {
                    open = Some((marker, run));
                    opened_at = index;
                }
            }
        }
    }

    match open {
        Some((marker, run)) if opened_at == line => {
            Some(std::iter::repeat_n(marker, run).collect())
        }
        _ => None,
    }
}

pub(crate) fn table_pipe_offsets(content: &str) -> Vec<usize> {
    let mut offsets = Vec::new();
    let mut code_delimiter = None;
    let mut index = 0usize;

    while index < content.len() {
        let Some(ch) = content[index..].chars().next() else {
            break;
        };
        if ch == '\\' {
            index += ch.len_utf8();
            if let Some(escaped) = content[index..].chars().next() {
                index += escaped.len_utf8();
            }
            continue;
        }
        if ch == '`' {
            let run_len = backtick_run_len(content, index);
            match code_delimiter {
                Some(expected) if expected == run_len => code_delimiter = None,
                None if has_closing_backtick_run(content, index + run_len, run_len) => {
                    code_delimiter = Some(run_len)
                }
                _ => {}
            }
            index += run_len;
            continue;
        }
        if code_delimiter.is_none()
            && content[index..].starts_with("[[")
            && let Some(close) = content[index + 2..].find("]]")
        {
            index += close + 4;
            continue;
        }
        if ch == '|' && code_delimiter.is_none() {
            offsets.push(index);
        }
        index += ch.len_utf8();
    }

    offsets
}

fn is_table_line(content: &str) -> bool {
    let (source, _) = table_row_source(content);
    !table_pipe_offsets(source).is_empty()
}

pub fn is_separator_row(cells: &[String]) -> bool {
    cells.iter().all(|c| {
        let t = c.trim();
        let marker = t.strip_prefix(':').unwrap_or(t);
        let marker = marker.strip_suffix(':').unwrap_or(marker);
        marker.len() >= 3 && marker.bytes().all(|byte| byte == b'-')
    })
}

pub fn parse_table_cells_with_ranges(content: &str) -> Vec<(String, Range<usize>)> {
    let (source, source_offset) = table_row_source(content);
    let pipe_offsets = table_pipe_offsets(source);
    let has_leading_pipe = pipe_offsets.first() == Some(&0);
    let has_trailing_pipe = pipe_offsets.last().copied() == source.len().checked_sub(1);
    let inner_start = usize::from(has_leading_pipe);
    // A lone `|` counts as both the leading and trailing pipe; clamp so the
    // inner region stays a valid (possibly empty) range.
    let inner_end = source
        .len()
        .saturating_sub(usize::from(has_trailing_pipe))
        .max(inner_start);
    let separators = pipe_offsets
        .into_iter()
        .filter(|offset| *offset >= inner_start && *offset < inner_end);
    let mut cells = Vec::new();
    let mut segment_start = inner_start;

    for index in separators.chain(std::iter::once(inner_end)) {
        let raw = &source[segment_start..index];
        let leading_ws = raw.len() - raw.trim_start_matches([' ', '\t']).len();
        let trailing_ws = raw.len() - raw.trim_end_matches([' ', '\t']).len();
        let trimmed_start = segment_start + leading_ws;
        let trimmed_end = index.saturating_sub(trailing_ws);
        let text = if trimmed_start <= trimmed_end {
            source[trimmed_start..trimmed_end].to_string()
        } else {
            String::new()
        };
        let anchor = source_offset + segment_start + leading_ws.min(raw.len());
        let range = if trimmed_start <= trimmed_end {
            (source_offset + trimmed_start)..(source_offset + trimmed_end)
        } else {
            anchor..anchor
        };
        cells.push((text, range));
        segment_start = index.saturating_add(1);
    }

    cells
}

fn table_row_has_cell_math(decoration: &LineDecoration, content: &str, line_start: usize) -> bool {
    let Some(preview) = decoration.block_preview.as_ref() else {
        return false;
    };
    if preview.kind != BlockPreviewKind::Math || preview.start_line != preview.end_line {
        return false;
    }
    parse_table_cells_with_ranges(content)
        .iter()
        .any(|(_, cell)| {
            preview.source_range.start >= line_start + cell.start
                && preview.source_range.end <= line_start + cell.end
        })
}

fn detect_tables(decorations: &mut [LineDecoration], text: &str, line_starts: &[usize]) {
    let line_count = decorations.len();
    let mut line_idx = 0;
    let quote_depth = |content: &str| {
        content[..blockquote_prefix_len(content)]
            .bytes()
            .filter(|byte| *byte == b'>')
            .count()
    };

    while line_idx < line_count {
        let content = get_line_content(text, line_starts, line_idx, line_count);
        if decorations[line_idx].table_info.is_some()
            || block_marker_is_protected(&decorations[line_idx], content)
            || (matches!(
                decorations[line_idx].block_type,
                BlockType::CodeBlock | BlockType::CodeBlockFence
            ) && !table_row_has_cell_math(
                &decorations[line_idx],
                content,
                line_starts[line_idx],
            ))
        {
            line_idx += 1;
            continue;
        }
        let header_cells = parse_table_cells_with_ranges(content);
        let separator_cells = (line_idx + 1 < line_count).then(|| {
            parse_table_cells_with_ranges(get_line_content(
                text,
                line_starts,
                line_idx + 1,
                line_count,
            ))
        });
        let starts_table = is_table_line(content)
            && line_idx + 1 < line_count
            && quote_depth(content)
                == quote_depth(get_line_content(
                    text,
                    line_starts,
                    line_idx + 1,
                    line_count,
                ))
            && !block_marker_is_protected(
                &decorations[line_idx + 1],
                get_line_content(text, line_starts, line_idx + 1, line_count),
            )
            && separator_cells.as_ref().is_some_and(|cells| {
                cells.len() == header_cells.len()
                    && !cells.is_empty()
                    && is_separator_row(
                        &cells
                            .iter()
                            .map(|(cell, _)| cell.clone())
                            .collect::<Vec<_>>(),
                    )
            });
        if starts_table {
            let table_start = line_idx;
            let table_column_count = header_cells.len();
            line_idx += 2;
            while line_idx < line_count {
                let c = get_line_content(text, line_starts, line_idx, line_count);
                if quote_depth(c) != quote_depth(content)
                    || block_marker_is_protected(&decorations[line_idx], c)
                {
                    break;
                }
                if !matches!(
                    decorations[line_idx].block_type,
                    BlockType::Normal | BlockType::Blockquote
                ) && !table_row_has_cell_math(&decorations[line_idx], c, line_starts[line_idx])
                {
                    break;
                }
                if !is_table_line(c) || parse_table_cells_with_ranges(c).len() != table_column_count
                {
                    break;
                }
                line_idx += 1;
            }
            let table_end = line_idx;

            if table_end - table_start >= 2 {
                let mut parsed_rows: Vec<Vec<(String, Range<usize>)>> = Vec::new();
                for l in table_start..table_end {
                    let c = get_line_content(text, line_starts, l, line_count);
                    parsed_rows.push(parse_table_cells_with_ranges(c));
                }

                let all_cells: Vec<Vec<String>> = parsed_rows
                    .iter()
                    .map(|row| row.iter().map(|(cell, _)| cell.clone()).collect())
                    .collect();

                let max_cols = all_cells.iter().map(|r| r.len()).max().unwrap_or(0);
                let column_alignments = all_cells
                    .get(1)
                    .map(|cells| {
                        cells
                            .iter()
                            .map(|cell| {
                                let marker = cell.trim();
                                match (marker.starts_with(':'), marker.ends_with(':')) {
                                    (true, true) => TableAlign::Center,
                                    (false, true) => TableAlign::Right,
                                    _ => TableAlign::Left,
                                }
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let mut col_widths = vec![0usize; max_cols];
                for (row_index, row) in all_cells.iter().enumerate() {
                    if row_index == 1 {
                        continue;
                    }
                    for (ci, cell) in row.iter().enumerate() {
                        if ci < max_cols {
                            col_widths[ci] = col_widths[ci].max(cell_display_width(cell));
                        }
                    }
                }
                // Minimum column width
                for w in col_widths.iter_mut() {
                    *w = (*w).max(3);
                }

                for (ri, l) in (table_start..table_end).enumerate() {
                    let is_sep = ri == 1;
                    decorations[l].block_type = BlockType::Table;
                    decorations[l].table_info = Some(TableLineInfo {
                        cells: all_cells[ri].clone(),
                        cell_ranges: parsed_rows[ri]
                            .iter()
                            .map(|(_, range)| range.clone())
                            .collect(),
                        column_alignments: column_alignments.clone(),
                        is_header: ri == 0,
                        is_separator: is_sep,
                        is_front_matter: false,
                        table_start_line: table_start,
                        table_end_line: table_end,
                        column_widths: col_widths.clone(),
                    });
                    // Mark entire line as syntax (hidden when folded)
                    let line_len = get_line_content(text, line_starts, l, line_count).len();
                    decorations[l].syntax_ranges.push(SyntaxRange {
                        range: 0..line_len,
                        trigger_range: 0..line_len,
                    });
                }
            }
        } else {
            line_idx += 1;
        }
    }
}

fn parse_wiki_link_parts(source: &str) -> Option<(String, Range<usize>)> {
    if !source.starts_with("[[") || !source.ends_with("]]") {
        return None;
    }

    let inner_start = 2;
    let inner_end = source.len().saturating_sub(2);
    let inner = &source[inner_start..inner_end];
    if inner.trim().is_empty() {
        return None;
    }

    if let Some(separator) = inner.find('|') {
        let target = inner[..separator].trim();
        let alias_start = inner_start + separator + 1;
        let alias_end = inner_end;
        if target.is_empty() || alias_start >= alias_end {
            return None;
        }
        return Some((target.to_string(), alias_start..alias_end));
    }

    let target = inner.trim();
    if target.is_empty() {
        return None;
    }

    let collapsed_start = inner_start
        + target
            .rfind(|ch| ch == '/' || ch == '\\')
            .map(|index| index + 1)
            .unwrap_or(0);
    let basename = &inner[collapsed_start - inner_start..];
    let filename = basename.split(['#', '?']).next().unwrap_or(basename);
    let visible = filename
        .strip_suffix(".md")
        .filter(|stem| !stem.is_empty())
        .unwrap_or(filename);
    Some((
        target.to_string(),
        collapsed_start..collapsed_start + visible.len(),
    ))
}

fn is_wiki_image_target(target: &str) -> bool {
    let path = target.split(['#', '?']).next().unwrap_or(target);
    Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "webp" | "gif" | "svg" | "bmp" | "tif" | "tiff"
            )
        })
}

fn add_wiki_link_span(
    decorations: &mut [LineDecoration],
    _line_starts: &[usize],
    offset_to_line_col: &dyn Fn(usize) -> (usize, usize),
    start: usize,
    end: usize,
    text: &str,
) {
    let (start_line, start_col) = offset_to_line_col(start);
    let (end_line, end_col) = offset_to_line_col(end);
    if start_line >= decorations.len() || start_line != end_line {
        return;
    }

    let source = &text[start..end];
    let Some((target_path, collapsed_range)) = parse_wiki_link_parts(source) else {
        return;
    };

    let full_trigger = start_col..end_col;
    let collapsed_start = start_col + collapsed_range.start;
    let collapsed_end = start_col + collapsed_range.end;
    if collapsed_start >= collapsed_end {
        return;
    }

    let decoration = &mut decorations[start_line];
    decoration.syntax_ranges.push(SyntaxRange {
        range: start_col..collapsed_start,
        trigger_range: full_trigger.clone(),
    });
    decoration.syntax_ranges.push(SyntaxRange {
        range: collapsed_end..end_col,
        trigger_range: full_trigger.clone(),
    });
    decoration.wiki_links.push(WikiLinkSpan {
        full_range: full_trigger,
        collapsed_range: collapsed_start..collapsed_end,
        target_path,
        has_alias: source[2..source.len().saturating_sub(2)].contains('|'),
    });
}

fn detect_raw_wiki_links(decorations: &mut [LineDecoration], text: &str, line_starts: &[usize]) {
    let line_count = decorations.len();
    for (line, decoration) in decorations.iter_mut().enumerate() {
        if matches!(
            decoration.block_type,
            BlockType::CodeBlock | BlockType::CodeBlockFence
        ) {
            continue;
        }

        let line_content = get_line_content(text, line_starts, line, line_count);
        for full_range in raw_wiki_link_ranges(line_content, Some(decoration)) {
            if decoration
                .wiki_links
                .iter()
                .any(|span| span.full_range == full_range)
            {
                continue;
            }
            let Some(source) = line_content.get(full_range.clone()) else {
                continue;
            };
            let Some((target_path, collapsed_range)) = parse_wiki_link_parts(source) else {
                continue;
            };
            let collapsed_start = full_range.start + collapsed_range.start;
            let collapsed_end = full_range.start + collapsed_range.end;
            if collapsed_start >= collapsed_end {
                continue;
            }
            decoration.syntax_ranges.push(SyntaxRange {
                range: full_range.start..collapsed_start,
                trigger_range: full_range.clone(),
            });
            decoration.syntax_ranges.push(SyntaxRange {
                range: collapsed_end..full_range.end,
                trigger_range: full_range.clone(),
            });
            decoration.wiki_links.push(WikiLinkSpan {
                full_range: full_range.clone(),
                collapsed_range: collapsed_start..collapsed_end,
                target_path: target_path.clone(),
                has_alias: source[2..source.len().saturating_sub(2)].contains('|'),
            });
            if full_range.start > 0
                && line_content.as_bytes().get(full_range.start - 1) == Some(&b'!')
                && is_wiki_image_target(&target_path)
            {
                decoration.syntax_ranges.push(SyntaxRange {
                    range: full_range.start - 1..full_range.start,
                    trigger_range: full_range.start - 1..full_range.end,
                });
                decoration.image_path = Some(target_path);
            }
        }
    }
}

fn detect_bare_urls(decorations: &mut [LineDecoration], text: &str, line_starts: &[usize]) {
    static BARE_URL: OnceLock<Regex> = OnceLock::new();
    let matcher = BARE_URL.get_or_init(|| {
        Regex::new(r"(?i)(?:https?://|mailto:)[^\s<>]+")
            .expect("bare URL regular expression is valid")
    });
    let line_count = decorations.len();

    for (line, decoration) in decorations.iter_mut().enumerate() {
        if matches!(
            decoration.block_type,
            BlockType::CodeBlock | BlockType::CodeBlockFence
        ) || decoration.table_info.is_some()
        {
            // Markdown-table URLs were recovered from individual cells, so a
            // bare URL cannot consume a delimiter or neighboring cell text.
            continue;
        }

        let line_content = get_line_content(text, line_starts, line, line_count);
        let mut excluded = structural_excluded_ranges(decoration);
        excluded.extend(
            decoration
                .wiki_links
                .iter()
                .map(|link| link.full_range.clone()),
        );
        excluded.extend(
            decoration
                .footnote_references
                .iter()
                .map(|reference| reference.full_range.clone()),
        );
        let excluded = merge_byte_ranges(excluded);

        for candidate in matcher.find_iter(line_content) {
            let start = candidate.start();
            if line_content[..start]
                .chars()
                .next_back()
                .is_some_and(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '<'))
            {
                continue;
            }
            let end = trim_bare_url_end(line_content, start, candidate.end());
            if end <= start
                || (line_content[..start].trim_end().ends_with('<')
                    && line_content[end..].trim_start().starts_with('>'))
                || excluded
                    .iter()
                    .any(|range| range_intersects_byte(range, start, end))
            {
                continue;
            }
            let target = line_content[start..end].to_string();
            decoration.inline_spans.push(InlineSpan {
                kind: InlineKind::Link,
                range: start..end,
                target: Some(target),
            });
        }
        decoration.inline_spans.sort_by_key(|span| span.range.start);
    }
}

fn trim_bare_url_end(text: &str, start: usize, mut end: usize) -> usize {
    loop {
        let Some(ch) = text[start..end].chars().next_back() else {
            return start;
        };
        let trim = matches!(
            ch,
            '.' | ','
                | ';'
                | ':'
                | '!'
                | '?'
                | '"'
                | '\''
                | '。'
                | '，'
                | '；'
                | '：'
                | '！'
                | '？'
                | '、'
                | '”'
                | '’'
        ) || (ch == ')'
            && text[start..end].matches(')').count() > text[start..end].matches('(').count())
            || (ch == ']'
                && text[start..end].matches(']').count() > text[start..end].matches('[').count())
            || (ch == '}'
                && text[start..end].matches('}').count() > text[start..end].matches('{').count());
        if !trim {
            return end;
        }
        end -= ch.len_utf8();
    }
}

fn detect_underlines(decorations: &mut [LineDecoration], text: &str, line_starts: &[usize]) {
    let line_count = decorations.len();
    for (line, decoration) in decorations.iter_mut().enumerate() {
        if matches!(
            decoration.block_type,
            BlockType::CodeBlock | BlockType::CodeBlockFence
        ) {
            continue;
        }
        let line_content = get_line_content(text, line_starts, line, line_count);
        let excluded_ranges = custom_inline_excluded_ranges(decoration);

        let mut index = 0usize;
        let mut open: Option<usize> = None;
        while index + 1 < line_content.len() {
            let token_end = index + 2;
            if excluded_ranges
                .iter()
                .any(|range| range_intersects_byte(range, index, token_end))
            {
                index = next_char_boundary(line_content, index);
                continue;
            }
            if line_content.get(index..token_end) == Some("++")
                && !is_escaped_at(line_content, index)
            {
                if let Some(start) = open {
                    if delimiter_can_close(line_content, index) && start + 2 < index {
                        let visible_range = (start + 2)..index;
                        decoration.inline_spans.push(InlineSpan {
                            kind: InlineKind::Underline,
                            range: visible_range,
                            target: None,
                        });
                        let trigger_range = start..token_end;
                        decoration.syntax_ranges.push(SyntaxRange {
                            range: start..start + 2,
                            trigger_range: trigger_range.clone(),
                        });
                        decoration.syntax_ranges.push(SyntaxRange {
                            range: index..token_end,
                            trigger_range,
                        });
                        open = None;
                    }
                } else if delimiter_can_open(line_content, token_end) {
                    open = Some(index);
                }
                index = token_end;
                continue;
            }
            let Some(ch) = line_content
                .get(index..)
                .and_then(|rest| rest.chars().next())
            else {
                break;
            };
            index += ch.len_utf8();
        }

        let lower = line_content.to_ascii_lowercase();
        let mut html_search_start = 0usize;
        while let Some(open_relative) = lower[html_search_start..].find("<u") {
            let open_start = html_search_start + open_relative;
            let tag_suffix = lower.as_bytes().get(open_start + 2).copied();
            if !tag_suffix.is_some_and(|byte| byte == b'>' || byte.is_ascii_whitespace()) {
                html_search_start = open_start + 2;
                continue;
            }
            let Some(open_end_relative) = lower[open_start + 2..].find('>') else {
                break;
            };
            let content_start = open_start + 2 + open_end_relative + 1;
            let Some(close_relative) = lower[content_start..].find("</u>") else {
                break;
            };
            let close_start = content_start + close_relative;
            let close_end = close_start + 4;
            if content_start < close_start
                && !is_escaped_at(line_content, open_start)
                && !excluded_ranges
                    .iter()
                    .any(|range| range_intersects_byte(range, open_start, close_end))
            {
                decoration.inline_spans.push(InlineSpan {
                    kind: InlineKind::Underline,
                    range: content_start..close_start,
                    target: None,
                });
                decoration.syntax_ranges.push(SyntaxRange {
                    range: open_start..content_start,
                    trigger_range: open_start..close_end,
                });
                decoration.syntax_ranges.push(SyntaxRange {
                    range: close_start..close_end,
                    trigger_range: open_start..close_end,
                });
            }
            html_search_start = close_end;
        }
    }
}

fn add_footnote_reference(
    decorations: &mut [LineDecoration],
    line_starts: &[usize],
    offset_to_line_col: &dyn Fn(usize) -> (usize, usize),
    start: usize,
    end: usize,
    label: &str,
    text: &str,
) {
    let (start_line, start_col) = offset_to_line_col(start);
    let (end_line, end_col) = offset_to_line_col(end);
    if start_line >= decorations.len() || start_line != end_line {
        return;
    }

    let source = &text[start..end];
    if !source.starts_with("[^") || !source.ends_with(']') {
        return;
    }

    let line_len = text[line_starts[start_line]..]
        .find('\n')
        .unwrap_or(text.len() - line_starts[start_line]);
    let full_trigger = start_col..end_col.min(line_len);
    let collapsed_range = (start_col + 2)..end_col.saturating_sub(1);
    if collapsed_range.start >= collapsed_range.end {
        return;
    }

    let decoration = &mut decorations[start_line];
    decoration.syntax_ranges.push(SyntaxRange {
        range: start_col..collapsed_range.start,
        trigger_range: full_trigger.clone(),
    });
    decoration.syntax_ranges.push(SyntaxRange {
        range: collapsed_range.end..end_col,
        trigger_range: full_trigger.clone(),
    });
    decoration.footnote_references.push(FootnoteReferenceSpan {
        full_range: full_trigger,
        collapsed_range,
        label: label.to_string(),
    });
}

fn detect_raw_footnote_references(
    decorations: &mut [LineDecoration],
    text: &str,
    line_starts: &[usize],
) {
    fn valid_label(label: &str) -> bool {
        !label.is_empty()
            && label
                .chars()
                .all(|ch| !ch.is_whitespace() && !matches!(ch, '[' | ']'))
    }

    let line_count = decorations.len();
    for (line, decoration) in decorations.iter_mut().enumerate() {
        if matches!(
            decoration.block_type,
            BlockType::CodeBlock | BlockType::CodeBlockFence
        ) {
            continue;
        }

        let line_content = get_line_content(text, line_starts, line, line_count);
        let mut excluded_ranges = structural_excluded_ranges(decoration);
        excluded_ranges.extend(
            decoration
                .wiki_links
                .iter()
                .map(|span| span.full_range.clone()),
        );
        excluded_ranges = merge_byte_ranges(excluded_ranges);

        let mut index = 0usize;
        while index < line_content.len() {
            let Some(relative_start) = line_content[index..].find("[^") else {
                break;
            };
            let start = index + relative_start;
            if is_escaped_at(line_content, start) {
                index = start + 2;
                continue;
            }
            if excluded_ranges
                .iter()
                .any(|range| range_intersects_byte(range, start, start + 2))
            {
                index = start + 2;
                continue;
            }

            let label_start = start + 2;
            let Some(close_relative) = line_content[label_start..].find(']') else {
                break;
            };
            let close = label_start + close_relative;
            let full_range = start..close + 1;
            if decoration
                .footnote_references
                .iter()
                .any(|reference| reference.full_range == full_range)
            {
                index = close + 1;
                continue;
            }
            let Some(label) = line_content.get(label_start..close) else {
                index = close + 1;
                continue;
            };
            if !valid_label(label) {
                index = close + 1;
                continue;
            }

            decoration.syntax_ranges.push(SyntaxRange {
                range: start..label_start,
                trigger_range: full_range.clone(),
            });
            decoration.syntax_ranges.push(SyntaxRange {
                range: close..close + 1,
                trigger_range: full_range.clone(),
            });
            decoration.footnote_references.push(FootnoteReferenceSpan {
                full_range,
                collapsed_range: label_start..close,
                label: label.to_string(),
            });
            index = close + 1;
        }
    }
}

fn detect_highlights(decorations: &mut [LineDecoration], text: &str, line_starts: &[usize]) {
    let line_count = decorations.len();

    for (line, decoration) in decorations.iter_mut().enumerate() {
        if matches!(
            decoration.block_type,
            BlockType::CodeBlock | BlockType::CodeBlockFence
        ) {
            continue;
        }

        let line_content = get_line_content(text, line_starts, line, line_count);
        let excluded_ranges = custom_inline_excluded_ranges(decoration);

        let mut index = 0usize;
        let mut open: Option<usize> = None;
        while index + 1 < line_content.len() {
            let token_end = index + 2;
            if excluded_ranges
                .iter()
                .any(|range| range_intersects_byte(range, index, token_end))
            {
                index = next_char_boundary(line_content, index);
                continue;
            }

            if line_content.get(index..token_end) == Some("==")
                && !is_escaped_at(line_content, index)
            {
                if let Some(start) = open {
                    if delimiter_can_close(line_content, index) && start + 2 < index {
                        let full_trigger = start..token_end;
                        let collapsed_range = (start + 2)..index;
                        decoration.highlight_spans.push(collapsed_range.clone());
                        decoration.syntax_ranges.push(SyntaxRange {
                            range: start..(start + 2),
                            trigger_range: full_trigger.clone(),
                        });
                        decoration.syntax_ranges.push(SyntaxRange {
                            range: index..token_end,
                            trigger_range: full_trigger,
                        });
                        open = None;
                    }
                } else if delimiter_can_open(line_content, token_end) {
                    open = Some(index);
                }
                index = token_end;
                continue;
            }

            let Some(ch) = line_content
                .get(index..)
                .and_then(|rest| rest.chars().next())
            else {
                break;
            };
            index += ch.len_utf8();
        }
    }
}

fn shortcode_char_is_valid(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '+' | '-')
}

fn detect_emoji_shortcodes(decorations: &mut [LineDecoration], text: &str, line_starts: &[usize]) {
    let line_count = decorations.len();

    for (line, decoration) in decorations.iter_mut().enumerate() {
        if matches!(
            decoration.block_type,
            BlockType::CodeBlock | BlockType::CodeBlockFence
        ) {
            continue;
        }

        let line_content = get_line_content(text, line_starts, line, line_count);
        let excluded_ranges = custom_inline_excluded_ranges(decoration);

        let mut index = 0usize;
        while index < line_content.len() {
            if excluded_ranges.iter().any(|range| {
                range_intersects_byte(range, index, next_char_boundary(line_content, index))
            }) {
                index = next_char_boundary(line_content, index);
                continue;
            }

            let Some(ch) = line_content
                .get(index..)
                .and_then(|rest| rest.chars().next())
            else {
                break;
            };
            if ch != ':' || is_escaped_at(line_content, index) {
                index += ch.len_utf8();
                continue;
            }

            let mut cursor = index + ch.len_utf8();
            let shortcode_start = cursor;
            let mut closing = None;
            while cursor < line_content.len() {
                let Some(inner) = line_content
                    .get(cursor..)
                    .and_then(|rest| rest.chars().next())
                else {
                    break;
                };
                if inner == ':' {
                    closing = Some(cursor);
                    break;
                }
                if !shortcode_char_is_valid(inner) {
                    break;
                }
                cursor += inner.len_utf8();
            }

            let Some(closing) = closing else {
                index += ch.len_utf8();
                continue;
            };

            if shortcode_start == closing {
                index = closing + 1;
                continue;
            }

            let Some(shortcode) = line_content.get(shortcode_start..closing) else {
                index += ch.len_utf8();
                continue;
            };
            if let Some(emoji) = get_by_shortcode(shortcode) {
                let source_range = index..(closing + 1);
                decoration.display_replacements.push(DisplayReplacement {
                    source_range: source_range.clone(),
                    display_text: emoji.as_str().to_string(),
                    trigger_range: source_range,
                });
                index = closing + 1;
                continue;
            }

            index += ch.len_utf8();
        }
    }
}

fn detect_single_char_delimited_spans(
    decorations: &mut [LineDecoration],
    text: &str,
    line_starts: &[usize],
    delimiter: char,
    skip_double_delimiter: bool,
    push_range: impl Fn(&mut LineDecoration, Range<usize>),
) {
    fn is_valid_compact_inline_script(content: &str) -> bool {
        !content.is_empty()
            && content
                .chars()
                .all(|ch| !ch.is_whitespace() && !matches!(ch, '[' | ']'))
    }

    let line_count = decorations.len();

    for (line, decoration) in decorations.iter_mut().enumerate() {
        if matches!(
            decoration.block_type,
            BlockType::CodeBlock | BlockType::CodeBlockFence
        ) {
            continue;
        }

        let line_content = get_line_content(text, line_starts, line, line_count);
        let excluded_ranges = custom_inline_excluded_ranges(decoration);

        let delimiter_len = delimiter.len_utf8();
        let mut index = 0usize;
        let mut open: Option<usize> = None;

        while index < line_content.len() {
            if excluded_ranges
                .iter()
                .any(|range| range_intersects_byte(range, index, index + delimiter_len))
            {
                index = next_char_boundary(line_content, index);
                continue;
            }

            let Some(ch) = line_content
                .get(index..)
                .and_then(|rest| rest.chars().next())
            else {
                break;
            };
            if ch != delimiter || is_escaped_at(line_content, index) {
                index += ch.len_utf8();
                continue;
            }

            if skip_double_delimiter {
                let next_index = index + delimiter_len;
                if line_content
                    .get(next_index..)
                    .is_some_and(|rest| rest.starts_with(delimiter))
                {
                    index = next_index + delimiter_len;
                    continue;
                }
            }

            if let Some(start) = open {
                if delimiter_can_close(line_content, index) && start + delimiter_len < index {
                    let visible_range = (start + delimiter_len)..index;
                    if line_content
                        .get(visible_range.clone())
                        .is_some_and(is_valid_compact_inline_script)
                    {
                        push_range(decoration, visible_range.clone());
                        let full_trigger = start..(index + delimiter_len);
                        decoration.syntax_ranges.push(SyntaxRange {
                            range: start..(start + delimiter_len),
                            trigger_range: full_trigger.clone(),
                        });
                        decoration.syntax_ranges.push(SyntaxRange {
                            range: index..(index + delimiter_len),
                            trigger_range: full_trigger,
                        });
                    }
                    open = None;
                }
            } else if delimiter_can_open(line_content, index + delimiter_len) {
                open = Some(index);
            }

            index += delimiter_len;
        }
    }
}

fn detect_superscript_spans(decorations: &mut [LineDecoration], text: &str, line_starts: &[usize]) {
    detect_single_char_delimited_spans(
        decorations,
        text,
        line_starts,
        '^',
        false,
        |decoration, range| decoration.superscript_spans.push(range),
    );
}

fn detect_subscript_spans(decorations: &mut [LineDecoration], text: &str, line_starts: &[usize]) {
    detect_single_char_delimited_spans(
        decorations,
        text,
        line_starts,
        '~',
        true,
        |decoration, range| decoration.subscript_spans.push(range),
    );
}

fn assign_block_preview(
    decorations: &mut [LineDecoration],
    source_range: Range<usize>,
    start_line: usize,
    end_line: usize,
    kind: BlockPreviewKind,
    source_text: String,
) {
    if decorations.is_empty() || start_line >= decorations.len() {
        return;
    }

    let end_line = end_line.min(decorations.len().saturating_sub(1));
    let cache_key = format!(
        "__generated_preview__:{}:{}",
        match kind {
            BlockPreviewKind::Math => "math",
            BlockPreviewKind::Mermaid => "mermaid",
        },
        source_text
    );

    for line in start_line..=end_line {
        decorations[line].block_preview = Some(BlockPreview {
            kind,
            cache_key: cache_key.clone(),
            source_text: source_text.clone(),
            source_range: source_range.clone(),
            start_line,
            end_line,
            is_anchor: line == start_line,
        });
    }
}

pub(crate) fn parse_local_link_targets(text: &str) -> Vec<LocalLinkTarget> {
    enum OpenLink {
        Markdown { start: usize, is_embed: bool },
        WikiLink,
    }

    let options = Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_SUBSCRIPT
        | Options::ENABLE_SUPERSCRIPT
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_MATH
        | Options::ENABLE_TABLES;
    let mut open_links = Vec::new();
    let mut links = Vec::new();
    for (event, source_range) in Parser::new_ext(text, options).into_offset_iter() {
        match event {
            Event::Start(Tag::Link { link_type, .. }) => {
                if matches!(link_type, LinkType::WikiLink { .. }) {
                    open_links.push(OpenLink::WikiLink);
                } else {
                    open_links.push(OpenLink::Markdown {
                        start: source_range.start,
                        is_embed: false,
                    });
                }
            }
            Event::Start(Tag::Image { .. }) => open_links.push(OpenLink::Markdown {
                start: source_range.start,
                is_embed: true,
            }),
            Event::End(TagEnd::Link | TagEnd::Image) => {
                let Some(open) = open_links.pop() else {
                    continue;
                };
                let OpenLink::Markdown { start, is_embed } = open else {
                    continue;
                };
                let end = source_range.end.min(text.len());
                let Some(source) = text.get(start..end) else {
                    continue;
                };
                let Some(relative_target_range) = markdown_inline_target_range(source) else {
                    continue;
                };
                let target_range =
                    start + relative_target_range.start..start + relative_target_range.end;
                let Some(target) = text.get(target_range.clone()) else {
                    continue;
                };
                links.push(LocalLinkTarget {
                    source_range: start..end,
                    target_range,
                    target: target.to_string(),
                    syntax: LocalLinkSyntax::Markdown,
                    is_embed,
                });
            }
            _ => {}
        }
    }

    let decorations = parse_decorations(text);
    let mut line_start = 0usize;
    for (line, line_content) in text.split('\n').enumerate() {
        let Some(decoration) = decorations.get(line) else {
            break;
        };
        for wiki_link in &decoration.wiki_links {
            let Some(source) = line_content.get(wiki_link.full_range.clone()) else {
                continue;
            };
            let inner_end = source.len().saturating_sub(2);
            let target_end = source[2..inner_end]
                .find('|')
                .map_or(inner_end, |separator| 2 + separator);
            let Some(raw_target) = source.get(2..target_end) else {
                continue;
            };
            let leading = raw_target
                .len()
                .saturating_sub(raw_target.trim_start().len());
            let trailing = raw_target.len().saturating_sub(raw_target.trim_end().len());
            let target_start = line_start + wiki_link.full_range.start + 2 + leading;
            let target_end = line_start + wiki_link.full_range.start + target_end - trailing;
            if target_start >= target_end {
                continue;
            }
            let is_embed = wiki_link.full_range.start > 0
                && line_content.as_bytes().get(wiki_link.full_range.start - 1) == Some(&b'!');
            let source_start = line_start + wiki_link.full_range.start - usize::from(is_embed);
            links.push(LocalLinkTarget {
                source_range: source_start..line_start + wiki_link.full_range.end,
                target_range: target_start..target_end,
                target: text[target_start..target_end].to_string(),
                syntax: LocalLinkSyntax::WikiLink,
                is_embed,
            });
        }
        line_start += line_content.len() + 1;
    }
    links.sort_by_key(|link| link.source_range.start);
    links
}

pub(crate) fn markdown_inline_target_range(source: &str) -> Option<Range<usize>> {
    let mut start = source.find("](")? + 2;
    while source
        .as_bytes()
        .get(start)
        .is_some_and(|byte| byte.is_ascii_whitespace())
    {
        start += 1;
    }
    if source.as_bytes().get(start) == Some(&b'<') {
        let target_start = start + 1;
        let end = source.get(target_start..)?.find('>')? + target_start;
        return (target_start < end).then_some(target_start..end);
    }

    let mut escaped = false;
    let mut parenthesis_depth = 0usize;
    let mut end = start;
    for (offset, character) in source.get(start..)?.char_indices() {
        if escaped {
            escaped = false;
            end = start + offset + character.len_utf8();
            continue;
        }
        if character == '\\' {
            escaped = true;
            end = start + offset + 1;
            continue;
        }
        match character {
            '(' => parenthesis_depth += 1,
            ')' if parenthesis_depth > 0 => parenthesis_depth -= 1,
            ')' => break,
            _ if character.is_whitespace() && parenthesis_depth == 0 => break,
            _ => {}
        }
        end = start + offset + character.len_utf8();
    }
    (start < end).then_some(start..end)
}

#[cfg(test)]
mod tests {
    use std::ops::Range;

    use super::{
        BlockContext, BlockPreviewKind, BlockType, CalloutKind, InlineKind, LineDecoration,
        LocalLinkSyntax, TableAlign, blockquote_prefix_len, link_target_at_column,
        parse_decorations, parse_local_link_targets, parse_table_cells_with_ranges,
        raw_wiki_link_ranges, split_continuation_prefix, unclosed_code_fence_marker,
    };

    fn assert_cjk_inline(source: &str, decoration: &LineDecoration, delimiter: &str, inner: &str) {
        let expected_kind = match delimiter {
            "**" | "__" => Some(InlineKind::Bold),
            "*" | "_" => Some(InlineKind::Italic),
            "~~" => Some(InlineKind::Strikethrough),
            "++" => Some(InlineKind::Underline),
            _ => None,
        };
        let ranges = if let Some(kind) = expected_kind {
            decoration
                .inline_spans
                .iter()
                .filter(|span| span.kind == kind)
                .map(|span| span.range.clone())
                .collect::<Vec<_>>()
        } else {
            match delimiter {
                "==" => decoration.highlight_spans.clone(),
                "^" => decoration.superscript_spans.clone(),
                "~" => decoration.subscript_spans.clone(),
                _ => unreachable!(),
            }
        };
        let start = source
            .find(&format!("{delimiter}{inner}{delimiter}"))
            .unwrap();
        let visible = start + delimiter.len()..start + delimiter.len() + inner.len();
        assert!(
            ranges.contains(&visible),
            "{source}: {delimiter} {ranges:?}, expected {visible:?}"
        );
        for syntax in &decoration.syntax_ranges {
            assert!(
                source.get(syntax.range.clone()).is_some(),
                "{source}: {syntax:?}"
            );
            assert!(
                source.get(syntax.trigger_range.clone()).is_some(),
                "{source}: {syntax:?}"
            );
        }
        for marker in [
            start..visible.start,
            visible.end..visible.end + delimiter.len(),
        ] {
            assert!(
                decoration
                    .syntax_ranges
                    .iter()
                    .any(|syntax| syntax.range == marker),
                "missing marker {marker:?} in {source}"
            );
        }
    }

    #[test]
    fn cjk_inline_punctuation_document_table_and_cell_pipeline() {
        for delimiter in ["**", "__", "*", "_", "~~", "==", "++", "^", "~"] {
            for inner in [
                "你好。",
                "「你好」",
                "（你好）",
                "你好！",
                "你好!",
                "你好🙂",
                "“你好”",
                "你好、世界",
                "。",
                "𠮷。",
                "日本語。",
                "한글。",
            ] {
                for (before, after) in [
                    ("", ""),
                    ("前", "后"),
                    ("前", ""),
                    ("", "后"),
                    ("前，", "。后"),
                ] {
                    let source = format!("{before}{delimiter}{inner}{delimiter}{after}");
                    let ordinary = parse_decorations(&source);
                    assert_cjk_inline(&source, &ordinary[0], delimiter, inner);
                    let row_source = format!("| {source} |");
                    let table_source = format!("{row_source}\n| --- |\n{row_source}\n");
                    let table = parse_decorations(&table_source);
                    for row in [0, 2] {
                        assert_cjk_inline(&row_source, &table[row], delimiter, inner);
                        let cell = super::parse_table_cell_decorations(&source, &table[row]);
                        assert_cjk_inline(&source, &cell[0], delimiter, inner);
                    }
                }
            }
        }
    }

    #[test]
    fn cjk_inline_punctuation_is_a_commonmark_flanking_extension() {
        use pulldown_cmark::{Event, Parser, Tag};
        assert!(Parser::new("**你好。**").any(|event| matches!(event, Event::Start(Tag::Strong))));
        assert!(
            !Parser::new("前**你好。**后").any(|event| matches!(event, Event::Start(Tag::Strong)))
        );
        for source in [
            "foo_bar_baz",
            "foo__bar__baz",
            "a_你好_b",
            "a__你好__b",
            "foo_!_bar",
            "é__mot!__é",
        ] {
            assert!(
                parse_decorations(source)[0].inline_spans.is_empty(),
                "{source}"
            );
        }
        for source in ["前_你好_后", "前__你好__后"] {
            assert!(
                !parse_decorations(source)[0].inline_spans.is_empty(),
                "{source}"
            );
        }
        for source in [
            "前** 你好。**后",
            "前**你好。 **后",
            "前**你好。后",
            "前*你好。后",
        ] {
            assert!(
                parse_decorations(source)[0].inline_spans.is_empty(),
                "{source}"
            );
        }
    }

    #[test]
    fn cjk_inline_punctuation_preserves_nested_delimiter_runs_and_source_bytes() {
        for source in [
            "前***你好。***后",
            "前**外*你好。*内。**后",
            "前*外**你好。**内。*后",
            "前**==++你好。++==**后",
            "前~~H~2~O。~~后",
        ] {
            let decoration = &parse_decorations(source)[0];
            assert!(!decoration.inline_spans.is_empty(), "{source}");
            for span in &decoration.inline_spans {
                assert!(
                    source.get(span.range.clone()).is_some(),
                    "{source}: {span:?}"
                );
            }
            for syntax in &decoration.syntax_ranges {
                assert!(source.get(syntax.range.clone()).is_some());
                assert!(source.get(syntax.trigger_range.clone()).is_some());
            }
        }
        for (source, outer_delimiter, inner_delimiter) in [
            ("前**外*你好。*内。**后", "**", "*"),
            ("前*外**你好。**内。*后", "*", "**"),
        ] {
            let decoration = &parse_decorations(source)[0];
            assert_cjk_inline(source, decoration, inner_delimiter, "你好。");
            let outer_content = format!("外{inner_delimiter}你好。{inner_delimiter}内。");
            assert_cjk_inline(source, decoration, outer_delimiter, &outer_content);
        }
        let source = "前~~H~2~O。~~后";
        let decoration = &parse_decorations(source)[0];
        assert_cjk_inline(source, decoration, "~~", "H~2~O。");
        assert_cjk_inline(source, decoration, "~", "2");
        let source = "前***你好。***后";
        let decoration = &parse_decorations(source)[0];
        assert!(
            decoration
                .inline_spans
                .iter()
                .any(|span| span.kind == InlineKind::Bold)
        );
        assert!(
            decoration
                .inline_spans
                .iter()
                .any(|span| span.kind == InlineKind::Italic)
        );
        for prefix in ["# ", "> ", "- ", "1. ", "- [ ] ", "😀 "] {
            let source = format!("{prefix}前**你好。**后");
            assert_cjk_inline(&source, &parse_decorations(&source)[0], "**", "你好。");
        }
        let source = "前**你好。\n世界。**后";
        let decorations = parse_decorations(source);
        assert!(decorations.iter().all(|line| {
            line.inline_spans
                .iter()
                .any(|span| span.kind == InlineKind::Bold)
        }));
    }

    #[test]
    fn cjk_reference_labels_survive_document_and_inherited_table_parsing() {
        for delimiter in ["**", "__", "*", "_"] {
            for suffix in ["", "[]"] {
                let label = format!("前{delimiter}你好。{delimiter}后");
                let usage = format!("[{label}]{suffix}");
                let definition = format!("[{label}]: /target \"original title\"\n");
                let document = format!("{usage}\n\n{definition}");
                let decorations = parse_decorations(&document);
                let assert_link = |source: &str, decoration: &LineDecoration| {
                    let links = decoration
                        .inline_spans
                        .iter()
                        .filter(|span| span.kind == InlineKind::Link)
                        .collect::<Vec<_>>();
                    assert_eq!(links.len(), 1, "{source}: {decoration:?}");
                    assert_eq!(links[0].target.as_deref(), Some("/target"), "{source}");
                    assert_eq!(&source[links[0].range.clone()], label, "{source}");
                    assert_cjk_inline(source, decoration, delimiter, "你好。");
                };
                assert_link(&usage, &decorations[0]);
                let row_source = format!("| {usage} |");
                let table_source = format!("{row_source}\n| --- |\n{row_source}\n\n{definition}");
                let table = parse_decorations(&table_source);
                for row in [0, 2] {
                    assert_link(&row_source, &table[row]);
                    let cell = super::parse_table_cell_decorations(&usage, &table[row]);
                    assert_link(&usage, &cell[0]);
                }
            }
        }
    }

    #[test]
    fn cjk_reference_labels_unknown_inherited_keys_never_bind_masked_spelling() {
        for (delimiter, masked_label) in [
            ("**", "前**你好。**⸀"),
            ("*", "前*你好。*⸀"),
            ("__", "⸀__⸀好。__⸀"),
            ("_", "⸀_⸀好。_⸀"),
        ] {
            let label = format!("前{delimiter}你好。{delimiter}后");
            for suffix in ["", "[]", "[known]"] {
                for known in [false, true] {
                    let usage = format!("[{label}]{suffix}");
                    let original_definition = if known {
                        format!("[{label}]: /right\n")
                    } else {
                        String::new()
                    };
                    let document = format!(
                        "| Value |\n| --- |\n| {usage} |\n\n[{masked_label}]: /wrong\n[known]: /known\n{original_definition}"
                    );
                    let table = parse_decorations(&document);
                    let cell = super::parse_table_cell_decorations(&usage, &table[2]);
                    let expected = if suffix == "[known]" {
                        Some("/known")
                    } else if known {
                        Some("/right")
                    } else {
                        None
                    };
                    for decoration in [&table[2], &cell[0]] {
                        let targets = decoration
                            .inline_spans
                            .iter()
                            .filter(|span| span.kind == InlineKind::Link)
                            .map(|span| span.target.as_deref())
                            .collect::<Vec<_>>();
                        assert_eq!(
                            targets,
                            expected.into_iter().map(Some).collect::<Vec<_>>(),
                            "{document}: {decoration:?}"
                        );
                    }
                    assert_cjk_inline(&usage, &cell[0], delimiter, "你好。");
                }
            }
        }
    }

    #[test]
    fn cjk_reference_labels_prefer_original_inherited_target_over_masked_key() {
        // In a cell the marker is absent from source, but may exist in inherited
        // definitions. Resolving its masked spelling first would pick /wrong.
        let usage = "[前**你好。**后][]";
        let document = format!(
            "| Value |\n| --- |\n| {usage} |\n\n[前**你好。**后]: /right\n[前**你好。**⸀]: /wrong\n"
        );
        let table = parse_decorations(&document);
        let cell = super::parse_table_cell_decorations(usage, &table[2]);
        for decoration in [&table[2], &cell[0]] {
            let links = decoration
                .inline_spans
                .iter()
                .filter(|span| span.kind == InlineKind::Link)
                .collect::<Vec<_>>();
            assert_eq!(links.len(), 1, "{decoration:?}");
            assert_eq!(links[0].target.as_deref(), Some("/right"));
        }
        assert_cjk_inline(usage, &cell[0], "**", "你好。");
    }

    #[test]
    fn cjk_inline_punctuation_preserves_reference_context_and_nested_opaque_content() {
        let cell_source = "[前**你好。**后][前__文。__后]";
        let row_source = format!("| {cell_source} |");
        let document = format!(
            "| Value |\n| --- |\n{row_source}\n\n[前__文。__后]: <doc_前_后.md> \"前__标题__后\"\n"
        );
        let decorations = parse_decorations(&document);
        assert_cjk_inline(&row_source, &decorations[2], "**", "你好。");
        let cell = super::parse_table_cell_decorations(cell_source, &decorations[2]);
        assert_cjk_inline(cell_source, &cell[0], "**", "你好。");
        for decoration in [&decorations[2], &cell[0]] {
            assert!(
                decoration
                    .inline_spans
                    .iter()
                    .any(|span| span.kind == InlineKind::Link
                        && span.target.as_deref() == Some("doc_前_后.md"))
            );
        }
        for inner in [
            "「`_代码_`」",
            "「$x_y$」",
            "「<code>前__代码。__后</code>」",
        ] {
            let source = format!("前**{inner}**后");
            assert_cjk_inline(&source, &parse_decorations(&source)[0], "**", inner);
        }
    }

    #[test]
    fn cjk_inline_punctuation_does_not_adapt_opaque_sources() {
        use pulldown_cmark::{Options, Parser};
        for source in [
            r"前\**你好。\**后",
            "`前**你好。**后`",
            "```\n前**你好。**后\n```",
            "    前**你好。**后",
            "$前**你好。**后$",
            "$$前**你好。**后$$",
            "\\[前**你好。**后\\]",
            "\\[\n前**你好。**后\n\\]",
            "<code>前**你好。**后</code>",
            "<pre>前**你好。**后</pre>",
            "<span title=\"前**你好。**后\">text</span>",
            "<!-- 前**你好。**后 -->",
            "[label](前**你好。**后)",
            "![前**你好。**后](image.png)",
            "https://host/前**你好。**后",
            "custom:前**你好。**后",
            "<https://host/前**你好。**后>",
            "[[前**你好。**后]]",
            "[[前**你好。**后|label]]",
            "[前**你好。**后]: /path\n\n[label][前**你好。**后]",
        ] {
            let adapted = super::CjkInlineSource::new(source, || {
                Parser::new_ext(source, Options::all()).into_offset_iter()
            });
            assert_eq!(adapted.as_str(), source, "{source}");
        }
        let source = "[前**你好。**后](path_你好_字)";
        let decoration = &parse_decorations(source)[0];
        assert_cjk_inline(source, decoration, "**", "你好。");
        assert!(
            decoration
                .inline_spans
                .iter()
                .any(|span| span.kind == InlineKind::Link
                    && span.target.as_deref() == Some("path_你好_字"))
        );
    }

    #[test]
    fn lone_pipe_row_parses_without_panic() {
        // A row consisting of a single `|` counts as both the leading and
        // trailing pipe; it must not slice out of bounds.
        let cells = parse_table_cells_with_ranges("|");
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0].0, "");
        assert!(!parse_table_cells_with_ranges("  |  ").is_empty());
    }

    #[derive(Debug, serde::Deserialize)]
    struct CommonMarkExample {
        markdown: String,
        html: String,
        example: usize,
        section: String,
    }

    fn commonmark_examples() -> Vec<CommonMarkExample> {
        serde_json::from_str(include_str!(
            "../tests/fixtures/commonmark-0.31.2-spec.json"
        ))
        .expect("valid CommonMark spec fixture")
    }

    fn commonmark_example(example: usize) -> CommonMarkExample {
        commonmark_examples()
            .into_iter()
            .find(|item| item.example == example)
            .expect("CommonMark example exists")
    }

    fn has_block_context(decorations: &[LineDecoration], context: BlockContext) -> bool {
        decorations
            .iter()
            .any(|decoration| decoration.block_contexts.contains(&context))
    }

    fn has_inline_kind(decorations: &[LineDecoration], kind: InlineKind) -> bool {
        decorations
            .iter()
            .any(|decoration| decoration.inline_spans.iter().any(|span| span.kind == kind))
    }

    fn markdown_source_has_link_syntax(example: &CommonMarkExample) -> bool {
        let trimmed = example.markdown.trim_start();
        let has_autolink = example.markdown.match_indices('<').any(|(start, _)| {
            let Some(rest) = example.markdown.get(start + 1..) else {
                return false;
            };
            let Some(end) = rest.find('>') else {
                return false;
            };
            let candidate = &rest[..end];
            !candidate.starts_with('a')
                && !candidate.starts_with('/')
                && (candidate.contains(':') || candidate.contains('@'))
        });
        example.section == "Autolinks"
            || has_autolink
            || example.markdown.contains("](")
            || example.markdown.contains("][")
            || example.markdown.contains("\\![")
            || example.markdown.contains("]:")
            || (example.markdown.contains("\n[") && trimmed.starts_with('['))
    }

    fn markdown_source_has_image_syntax(example: &CommonMarkExample) -> bool {
        example.markdown.contains("![")
    }

    fn assert_first_line_link(example: usize, expected_range: std::ops::Range<usize>) {
        let example = commonmark_example(example);
        assert_eq!(example.section, "Links");
        let decorations = parse_decorations(&example.markdown);
        let link_span = decorations[0]
            .inline_spans
            .iter()
            .find(|span| span.kind == InlineKind::Link)
            .expect("link span on first line");
        assert_eq!(link_span.range, expected_range);
        assert_eq!(link_span.target.as_deref(), Some("/url"));
    }

    #[test]
    fn wiki_link_ranges_ignore_inline_code_tokens() {
        let text = "`[[` 文本 `]]`";
        let decorations = parse_decorations(text);

        assert!(decorations[0].wiki_links.is_empty());
        assert!(raw_wiki_link_ranges(text, decorations.first()).is_empty());
    }

    #[test]
    fn wiki_link_ranges_detect_plain_wiki_link() {
        let text = "before [[notes/file.md]] after";
        let decorations = parse_decorations(text);

        assert_eq!(decorations[0].wiki_links.len(), 1);
        assert_eq!(decorations[0].wiki_links[0].target_path, "notes/file.md");
        assert_eq!(decorations[0].wiki_links[0].collapsed_range, 15..19);
    }

    #[test]
    fn wiki_link_ranges_detect_piped_anchor_wiki_link() {
        let text = "before [[notes/file.md#intro|Overview]] after";
        let decorations = parse_decorations(text);

        assert_eq!(decorations[0].wiki_links.len(), 1);
        assert_eq!(
            decorations[0].wiki_links[0].target_path,
            "notes/file.md#intro"
        );
        assert_eq!(decorations[0].wiki_links[0].collapsed_range, 29..37);
    }

    #[test]
    fn wiki_link_displays_filename_without_markdown_suffix_or_fragment() {
        let text = "[[notes/方案.md#标题]] and [[assets/photo.png]] and [[notes/a.md|自定义]]";
        let decoration = &parse_decorations(text)[0];
        let labels: Vec<_> = decoration
            .wiki_links
            .iter()
            .map(|link| &text[link.collapsed_range.clone()])
            .collect();
        assert_eq!(labels, ["方案", "photo.png", "自定义"]);
        assert_eq!(decoration.wiki_links[0].target_path, "notes/方案.md#标题");
    }

    #[test]
    fn wiki_image_embed_sets_image_preview_path() {
        let text = "![[assets/示例 image.png|320]]";
        let decorations = parse_decorations(text);

        assert_eq!(
            decorations[0].image_path.as_deref(),
            Some("assets/示例 image.png")
        );
        assert_eq!(
            decorations[0].wiki_links[0].target_path,
            "assets/示例 image.png"
        );
        assert!(
            decorations[0]
                .syntax_ranges
                .iter()
                .any(|range| range.range == (0..1))
        );
    }

    #[test]
    fn wiki_file_embed_does_not_set_image_preview_path() {
        let decorations = parse_decorations("![[attachments/document.pdf]]");

        assert!(decorations[0].image_path.is_none());
    }

    #[test]
    fn nested_wiki_links_do_not_create_overlapping_ranges() {
        let decorations = parse_decorations("[[outer [[inner]] tail]]");

        assert_eq!(decorations[0].wiki_links.len(), 1);
        assert_eq!(decorations[0].wiki_links[0].target_path, "inner");
    }

    #[test]
    fn wiki_images_allow_query_and_fragment_suffixes() {
        let decorations = parse_decorations("![[image.PNG?size=2#preview]]");

        assert_eq!(
            decorations[0].image_path.as_deref(),
            Some("image.PNG?size=2#preview")
        );
    }

    #[test]
    fn local_link_targets_preserve_standard_and_wiki_metadata() {
        let text = "![alt](../assets/a%20b.png \"title\") and ![[assets/图.png|320]]";
        let links = parse_local_link_targets(text);

        assert_eq!(links.len(), 2);
        assert_eq!(links[0].target, "../assets/a%20b.png");
        assert_eq!(links[0].syntax, LocalLinkSyntax::Markdown);
        assert!(links[0].is_embed);
        assert_eq!(links[1].target, "assets/图.png");
        assert_eq!(links[1].syntax, LocalLinkSyntax::WikiLink);
        assert!(links[1].is_embed);
    }

    #[test]
    fn local_link_targets_support_balanced_parentheses() {
        let links = parse_local_link_targets("[doc](docs/a(b).md)");

        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target, "docs/a(b).md");
    }

    #[test]
    fn local_link_targets_skip_reference_style_and_code() {
        let text = "`[code](ignored.png)` [ref][image]\n\n[image]: actual.png";

        assert!(parse_local_link_targets(text).is_empty());
    }

    #[test]
    fn parse_decorations_detects_superscript_and_subscript() {
        let text = "E=mc^2^ and H~2~O";
        let decorations = parse_decorations(text);

        assert_eq!(decorations[0].superscript_spans, vec![5..6]);
        assert_eq!(decorations[0].subscript_spans, vec![14..15]);
    }

    #[test]
    fn parse_decorations_distinguishes_subscript_and_underline() {
        let text = "~U~ ++U++ <u>U</u> <u class=\"note\">A</u>";
        let decorations = parse_decorations(text);

        assert_eq!(decorations[0].subscript_spans, vec![1..2]);
        let underline_ranges = decorations[0]
            .inline_spans
            .iter()
            .filter(|span| span.kind == InlineKind::Underline)
            .map(|span| span.range.clone())
            .collect::<Vec<_>>();
        assert_eq!(underline_ranges, vec![6..7, 13..14, 35..36]);
    }

    #[test]
    fn parse_decorations_detects_github_style_callout() {
        let text = "> [!TIP] Use it\n> Useful detail";
        let decorations = parse_decorations(text);

        assert_eq!(decorations[0].callout_kind, Some(CalloutKind::Tip));
        assert!(
            decorations[0]
                .display_replacements
                .iter()
                .any(|replacement| {
                    replacement.source_range == (2..9)
                        && replacement.display_text
                            == super::callout_replacement_text(CalloutKind::Tip)
                })
        );
        assert!(
            decorations[0]
                .inline_spans
                .iter()
                .any(|span| span.kind == InlineKind::Bold && span.range == (9..15))
        );
        assert!(decorations[1].callout_kind.is_none());
        assert!(
            decorations[0]
                .block_contexts
                .contains(&BlockContext::Blockquote)
        );
    }

    #[test]
    fn callout_marker_only_applies_at_quote_group_start() {
        let decorations = parse_decorations("> ordinary\n> [!NOTE] not a callout\n");

        assert!(
            decorations
                .iter()
                .all(|decoration| decoration.callout_kind.is_none())
        );
    }

    #[test]
    fn callout_without_title_shows_default_kind_label() {
        let decorations = parse_decorations("> [!WARNING]\n> body text");

        assert_eq!(decorations[0].callout_kind, Some(CalloutKind::Warning));
        let replacement = decorations[0]
            .display_replacements
            .first()
            .expect("callout display replacement");
        assert_eq!(replacement.display_text, "\u{2003}\u{2009}WARNING");
        assert!(
            decorations[0]
                .inline_spans
                .iter()
                .any(|span| span.kind == InlineKind::Bold && span.range == (2..12))
        );

        let titled = parse_decorations("> [!WARNING] Custom title");
        let titled_replacement = titled[0]
            .display_replacements
            .first()
            .expect("titled callout display replacement");
        assert_eq!(titled_replacement.display_text, "\u{2003}\u{2009}");
    }

    #[test]
    fn nested_and_indented_blockquotes_keep_quote_context_and_prefix_ranges() {
        let decorations = parse_decorations("  > outer\n  > > inner\n  > outer again\n");

        assert!(decorations[..3].iter().all(|decoration| {
            decoration
                .block_contexts
                .contains(&BlockContext::Blockquote)
                && !decoration.syntax_ranges.is_empty()
        }));
    }

    #[test]
    fn parse_decorations_detects_highlight() {
        let text = "Use ==important== text";
        let decorations = parse_decorations(text);

        assert_eq!(decorations[0].highlight_spans, vec![6..15]);
    }

    #[test]
    fn custom_inline_syntax_respects_backslash_escapes() {
        let text = r"\[[wiki]] \==mark== \++underline++ \[^note] \:rocket: \^sup^ \~sub~";
        let decoration = &parse_decorations(text)[0];

        assert!(decoration.wiki_links.is_empty());
        assert!(decoration.highlight_spans.is_empty());
        assert!(decoration.inline_spans.is_empty());
        assert!(decoration.footnote_references.is_empty());
        assert!(decoration.display_replacements.is_empty());
        assert!(decoration.superscript_spans.is_empty());
        assert!(decoration.subscript_spans.is_empty());
    }

    #[test]
    fn custom_inline_escape_parity_allows_even_backslashes() {
        let text = r"\==off== \\==on== \:rocket: \\:rocket:";
        let decoration = &parse_decorations(text)[0];
        let highlights = decoration
            .highlight_spans
            .iter()
            .map(|range| &text[range.clone()])
            .collect::<Vec<_>>();

        assert_eq!(highlights, vec!["on"]);
        assert_eq!(decoration.display_replacements.len(), 1);
        assert_eq!(
            &text[decoration.display_replacements[0].source_range.clone()],
            ":rocket:"
        );
    }

    #[test]
    fn protected_contexts_suppress_all_custom_inline_syntax() {
        for text in [
            "`==mark== ++under++ :rocket: ^sup^ ~sub~`",
            "$==mark== ++under++ :rocket: ^sup^ ~sub~$",
            "<code>==mark== ++under++ :rocket: ^sup^ ~sub~</code>",
            "![==mark== ++under++ :rocket: ^sup^ ~sub~](image.png)",
            "[^==mark==]",
        ] {
            let decoration = &parse_decorations(text)[0];
            assert!(decoration.highlight_spans.is_empty(), "highlight: {text}");
            assert!(
                decoration
                    .inline_spans
                    .iter()
                    .all(|span| span.kind != InlineKind::Underline),
                "underline: {text}"
            );
            assert!(
                decoration.display_replacements.is_empty(),
                "replacement: {text}"
            );
            assert!(
                decoration.superscript_spans.is_empty(),
                "superscript: {text}"
            );
            assert!(decoration.subscript_spans.is_empty(), "subscript: {text}");
        }
    }

    #[test]
    fn custom_inline_syntax_is_allowed_in_link_labels_and_wiki_aliases() {
        let text = "[==label== ++under++ :rocket:](path==target==) [[target==literal==|==alias== ++wiki++ :rocket:]] [[plain==target==]]";
        let decoration = &parse_decorations(text)[0];
        let highlights = decoration
            .highlight_spans
            .iter()
            .map(|range| &text[range.clone()])
            .collect::<Vec<_>>();
        let underlines = decoration
            .inline_spans
            .iter()
            .filter(|span| span.kind == InlineKind::Underline)
            .map(|span| &text[span.range.clone()])
            .collect::<Vec<_>>();

        assert_eq!(highlights, vec!["label", "alias"]);
        assert_eq!(underlines, vec!["under", "wiki"]);
        assert_eq!(decoration.display_replacements.len(), 2);
        assert_eq!(decoration.wiki_links.len(), 2);
        assert!(decoration.wiki_links[0].has_alias);
        assert!(!decoration.wiki_links[1].has_alias);
    }

    #[test]
    fn nested_presentation_syntaxes_are_composable() {
        let text = "**==bold==** ==++both++== ~~H~2~O~~ *++italic++* ==`code`==";
        let decoration = &parse_decorations(text)[0];

        for kind in [
            InlineKind::Bold,
            InlineKind::Italic,
            InlineKind::Strikethrough,
            InlineKind::Underline,
            InlineKind::InlineCode,
        ] {
            assert!(
                decoration.inline_spans.iter().any(|span| span.kind == kind),
                "missing {kind:?}"
            );
        }
        assert_eq!(decoration.highlight_spans.len(), 3);
        assert_eq!(decoration.subscript_spans.len(), 1);
    }

    #[test]
    fn front_matter_is_opaque_but_table_cells_allow_inline_syntax() {
        let text = "---\ntitle: ==literal==\nemoji: \":rocket:\"\n---\n\n| A | B |\n| --- | --- |\n| ==mark== | ++under++ :rocket: |\n";
        let decorations = parse_decorations(text);

        assert!(decorations[..4].iter().all(|decoration| {
            decoration.highlight_spans.is_empty()
                && decoration.display_replacements.is_empty()
                && decoration
                    .inline_spans
                    .iter()
                    .all(|span| span.kind != InlineKind::Underline)
        }));
        assert_eq!(decorations[7].highlight_spans.len(), 1);
        assert!(
            decorations[7]
                .inline_spans
                .iter()
                .any(|span| span.kind == InlineKind::Underline)
        );
        assert_eq!(decorations[7].display_replacements.len(), 1);
    }

    fn assert_front_matter_presentation_is_yaml_only(text: &str) {
        let decorations = parse_decorations(text);
        let table_end = decorations[0].table_info.as_ref().unwrap().table_end_line;
        for (line, (source, decoration)) in
            text.lines().zip(&decorations).take(table_end).enumerate()
        {
            let expected = super::LineDecorationPayload {
                table_info: decoration.table_info.clone(),
                front_matter: decoration.front_matter.clone(),
                syntax_ranges: vec![super::SyntaxRange {
                    range: 0..source.len(),
                    trigger_range: 0..source.len(),
                }],
                protected_ranges: if source.is_empty() {
                    Vec::new()
                } else {
                    vec![0..source.len()]
                },
                ..super::LineDecorationPayload::default()
            };
            assert_eq!(
                decoration.block_type,
                BlockType::Table,
                "row {line}: {text}"
            );
            assert_eq!(&**decoration, &expected, "row {line}: {text}");
        }
    }

    #[test]
    fn front_matter_literal_and_raw_rows_discard_markdown_previews_and_syntax() {
        for (kind, content) in [
            ("math", "\\[\nx^2\n\\]"),
            ("math", "$$\nx^2\n$$"),
            ("image", "![caption](literal.png)"),
            ("code", "```rust\nlet x = 1;\n```"),
            ("math", "```mermaid\ngraph TD; A-->B\n```"),
            (
                "list",
                "- [ ] literal\n  - child\n  - \n\n1. ordered\n\n> [!NOTE]\n> literal",
            ),
            (
                "inline",
                "**bold** *italic* ~~strike~~ `code` $x$ [link](literal.md)\n\n[^note]: literal\n\n[^note]",
            ),
        ] {
            let indented = content
                .lines()
                .map(|line| format!("  {line}"))
                .collect::<Vec<_>>()
                .join("\n");
            let literal = format!("---\ndescription: |-\n\n{indented}\n\n---\n\nBody\n");
            let raw = format!("---\ndescription: [unfinished\n\n{content}\n\n---\n\nBody\n");
            for (text, is_raw) in [(&literal, false), (&raw, true)] {
                // Confirm the fixtures exercise state created before YAML conversion.
                let markdown = super::parse_decorations_fragment(text);
                let has_presentation = markdown.iter().any(|decoration| match kind {
                    "math" => decoration.block_preview.is_some(),
                    "image" => decoration.image_path.is_some(),
                    "code" => decoration.code_language.is_some(),
                    "list" => !decoration.block_contexts.is_empty(),
                    _ => !decoration.inline_spans.is_empty(),
                });
                assert!(has_presentation, "fixture must exercise {kind}: {text}");
                assert_front_matter_presentation_is_yaml_only(text);
                let decorations = parse_decorations(text);
                let value = &decorations[3].front_matter.as_ref().unwrap().value;
                if is_raw {
                    assert_eq!(value, &crate::front_matter::FrontMatterValue::Raw);
                } else {
                    assert_eq!(
                        value,
                        &crate::front_matter::FrontMatterValue::Text(
                            content.lines().next().unwrap().to_string()
                        ),
                    );
                }
            }
        }
    }

    #[test]
    fn front_matter_cannot_define_links_or_open_fences_in_the_body() {
        for newline in ["\n", "\r\n"] {
            let text = "---\ndescription: |\n\n  [private]: https://private.example\n\n---\n\n[label][private]".replace('\n', newline);
            let rows = parse_decorations(&text);
            assert!(
                rows.last()
                    .unwrap()
                    .inline_spans
                    .iter()
                    .all(|span| span.kind != InlineKind::Link)
            );
            for marker in ["```", "~~~~"] {
                let text = format!("---\ndescription: |\n  {marker}\n---\n\n# Body")
                    .replace('\n', newline);
                let rows = parse_decorations(&text);
                assert_eq!(
                    rows.last().unwrap().block_type,
                    BlockType::Heading(1),
                    "{text:?}"
                );
            }
        }
    }

    #[test]
    fn closed_blocks_do_not_leak_syntax_state_into_following_blocks() {
        let prefixes = [
            "```text\n~~~~\n# literal\n```",
            "~~~~\n```\n# literal\n~~~~",
            "<pre>\n~~~~\n[hidden]: https://hidden.example\n</pre>",
            "<!--\n**literal**\n-->",
            "---\nvalue: |\n  ```\n  [hidden]: https://hidden.example\n---",
            "> | A | B |\n> | --- | --- |\n> | one | two |",
            "\\[\nx + y\n\\]",
            "```mermaid\ngraph TD; A-->B\n```",
        ];
        let suffixes = [
            "# Heading\n\ntext",
            "Heading\n===",
            "- item\n  - child\n- [x] done",
            "1. first\n2. second",
            "> [!NOTE]\n> body",
            "| A | B |\n| --- | --- |\n| one | two |",
            "\\[\nx + y\n\\]",
            "**bold\nsecond** *italic* ~~strike~~",
            "==highlight== ++underline++ H~2~O x^2^ :rocket:",
            "[[note|alias]] [link](https://example.test) ![image](image.png)",
            "[label][visible]\n\n[visible]: https://example.test",
            "[^note]\n\n[^note]: definition",
        ];
        for newline in ["\n", "\r\n"] {
            for prefix in prefixes {
                for suffix in suffixes {
                    let source = format!("{prefix}\n\n{suffix}").replace('\n', newline);
                    let actual = parse_decorations(&source);
                    let expected = parse_decorations(&suffix.replace('\n', newline));
                    let first_line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 2;
                    assert_eq!(actual.len() - first_line, expected.len(), "{source:?}");
                    for (actual, expected) in actual[first_line..].iter().zip(expected) {
                        assert_eq!(actual.block_type, expected.block_type, "{source:?}");
                        assert_eq!(actual.inline_spans, expected.inline_spans, "{source:?}");
                        assert_eq!(actual.syntax_ranges, expected.syntax_ranges, "{source:?}");
                        assert_eq!(
                            actual.protected_ranges, expected.protected_ranges,
                            "{source:?}"
                        );
                        assert_eq!(actual.block_contexts, expected.block_contexts, "{source:?}");
                        assert_eq!(
                            actual.highlight_spans, expected.highlight_spans,
                            "{source:?}"
                        );
                        assert_eq!(
                            actual.superscript_spans, expected.superscript_spans,
                            "{source:?}"
                        );
                        assert_eq!(
                            actual.subscript_spans, expected.subscript_spans,
                            "{source:?}"
                        );
                        assert_eq!(actual.wiki_links, expected.wiki_links, "{source:?}");
                        assert_eq!(
                            actual.footnote_references, expected.footnote_references,
                            "{source:?}"
                        );
                        assert_eq!(
                            actual.display_replacements, expected.display_replacements,
                            "{source:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn multiline_raw_html_blocks_are_opaque() {
        let text = "<pre>\n==literal==\n++literal++ :rocket:\n</pre>\n";
        let decorations = parse_decorations(text);

        assert!(decorations[..4].iter().all(|decoration| {
            decoration.highlight_spans.is_empty()
                && decoration.display_replacements.is_empty()
                && decoration
                    .inline_spans
                    .iter()
                    .all(|span| span.kind != InlineKind::Underline)
        }));
    }

    #[test]
    fn inline_math_and_raw_html_code_are_protected_from_custom_syntax() {
        let text = "$x^2^$ <code>==literal== ++literal++</code>";
        let decoration = &parse_decorations(text)[0];

        assert_eq!(decoration.inline_math_spans, vec![1..5]);
        assert!(decoration.superscript_spans.is_empty());
        assert!(decoration.highlight_spans.is_empty());
        assert!(
            decoration
                .inline_spans
                .iter()
                .all(|span| span.kind != InlineKind::Underline)
        );
    }

    #[test]
    fn multiple_inline_math_spans_preserve_cjk_neighbors() {
        let text = "前 $E=mc^2$ 后 $\\frac{a}{b}$";
        let decoration = &parse_decorations(text)[0];

        assert_eq!(decoration.inline_math_spans, vec![5..11, 18..29]);
        assert_eq!(&text[decoration.inline_math_spans[0].clone()], "E=mc^2");
        assert_eq!(
            &text[decoration.inline_math_spans[1].clone()],
            r"\frac{a}{b}"
        );
    }

    #[test]
    fn parse_decorations_preserves_inline_link_target() {
        let text = "跳到 [标题](#标题1)";
        let decorations = parse_decorations(text);

        assert_eq!(decorations[0].inline_spans.len(), 1);
        assert_eq!(
            decorations[0].inline_spans[0].target.as_deref(),
            Some("#标题1")
        );
    }

    #[test]
    fn bare_urls_become_links_without_trailing_punctuation() {
        let text = "See https://example.com/a_(b)。 Email mailto:hello@example.com!";
        let decorations = parse_decorations(text);
        let links = decorations[0]
            .inline_spans
            .iter()
            .filter(|span| span.kind == InlineKind::Link)
            .collect::<Vec<_>>();

        assert_eq!(links.len(), 2);
        assert_eq!(
            links[0].target.as_deref(),
            Some("https://example.com/a_(b)")
        );
        assert_eq!(links[1].target.as_deref(), Some("mailto:hello@example.com"));
    }

    #[test]
    fn bare_url_detection_skips_code_and_existing_links() {
        let text = "`https://code.example` [label](https://target.example)";
        let decorations = parse_decorations(text);
        let links = decorations[0]
            .inline_spans
            .iter()
            .filter(|span| span.kind == InlineKind::Link)
            .collect::<Vec<_>>();

        assert_eq!(links.len(), 1);
        assert_eq!(links[0].target.as_deref(), Some("https://target.example"));
    }

    #[test]
    fn links_in_table_rows_are_navigable_by_source_column() {
        let text = "| Link |\n| --- |\n| https://example.com |";
        let decorations = parse_decorations(text);
        let row = &decorations[2];
        let column = text.lines().nth(2).unwrap().find("https://").unwrap() + 4;

        assert_eq!(
            link_target_at_column(row, column),
            Some("https://example.com")
        );
    }

    #[test]
    fn commonmark_reference_links_get_link_spans_and_targets() {
        assert_first_line_link(527, 1..4);
        assert_first_line_link(553, 1..4);
        assert_first_line_link(557, 1..4);
    }

    #[test]
    fn commonmark_spec_decoration_feature_scan() {
        fn source_has_single_line_code_span(markdown: &str) -> bool {
            markdown.lines().any(|line| {
                let mut index = 0;
                while let Some(relative) = line[index..].find('`') {
                    let run_start = index + relative;
                    let run_len = super::backtick_run_len(line, run_start);
                    if super::has_closing_backtick_run(line, run_start + run_len, run_len) {
                        return true;
                    }
                    index = run_start + run_len;
                }
                false
            })
        }

        let mut missing = Vec::new();

        for example in commonmark_examples() {
            let decorations = parse_decorations(&example.markdown);
            let html = example.html.as_str();
            let skip_raw_html_inline_checks =
                matches!(example.section.as_str(), "Raw HTML" | "HTML blocks");

            let has_heading = decorations
                .iter()
                .any(|decoration| matches!(decoration.block_type, BlockType::Heading(_)));
            let has_hr = decorations
                .iter()
                .any(|decoration| decoration.block_type == BlockType::HorizontalRule);
            let has_code_block = decorations.iter().any(|decoration| {
                matches!(
                    decoration.block_type,
                    BlockType::CodeBlock | BlockType::CodeBlockFence
                )
            });
            let has_blockquote = has_block_context(&decorations, BlockContext::Blockquote)
                || decorations
                    .iter()
                    .any(|decoration| decoration.block_type == BlockType::Blockquote);
            let has_ordered_list = has_block_context(&decorations, BlockContext::OrderedList)
                || decorations
                    .iter()
                    .any(|decoration| decoration.block_type == BlockType::OrderedList);
            let has_unordered_list = has_block_context(&decorations, BlockContext::UnorderedList)
                || decorations.iter().any(|decoration| {
                    matches!(
                        decoration.block_type,
                        BlockType::UnorderedList | BlockType::TaskList(_)
                    )
                });
            let has_em = has_inline_kind(&decorations, InlineKind::Italic);
            let has_strong = has_inline_kind(&decorations, InlineKind::Bold);
            let has_code_span = has_inline_kind(&decorations, InlineKind::InlineCode);
            let has_link = has_inline_kind(&decorations, InlineKind::Link);
            let has_image = has_inline_kind(&decorations, InlineKind::Image);

            let expects_heading = html.contains("<h1")
                || html.contains("<h2")
                || html.contains("<h3")
                || html.contains("<h4")
                || html.contains("<h5")
                || html.contains("<h6");
            let expects_hr = html.contains("<hr");
            let expects_code_block = html.contains("<pre><code");
            let expects_blockquote = html.contains("<blockquote");
            let expects_ordered_list = html.contains("<ol");
            let expects_unordered_list = html.contains("<ul");
            let expects_em = html.contains("<em>");
            let expects_strong = html.contains("<strong>");
            let expects_code_span = html.contains("<code>") && !html.contains("<pre><code");
            let expects_link = html.contains("<a ") && markdown_source_has_link_syntax(&example);
            let expects_image =
                html.contains("<img ") && markdown_source_has_image_syntax(&example);

            let mut record = |feature: &str, present: bool, expected: bool| {
                if expected && !present {
                    missing.push(format!(
                        "example {} [{}] missing {}",
                        example.example, example.section, feature
                    ));
                } else if !expected && present {
                    missing.push(format!(
                        "example {} [{}] unexpected {}",
                        example.example, example.section, feature
                    ));
                }
            };

            let rule_delimited_paragraph = example.example == 96;
            record(
                "heading",
                has_heading,
                expects_heading && !rule_delimited_paragraph,
            );
            record("thematic break", has_hr, expects_hr);
            // An unclosed fence stays plain text here so it can be typed, which
            // the spec's own unterminated-fence examples do not expect.
            if super::unclosed_code_fence_marker_span(&example.markdown).is_none() {
                record("code block", has_code_block, expects_code_block);
            }
            record("blockquote context", has_blockquote, expects_blockquote);
            record(
                "ordered list context",
                has_ordered_list,
                expects_ordered_list,
            );
            record(
                "unordered list context",
                has_unordered_list,
                expects_unordered_list,
            );

            if !skip_raw_html_inline_checks {
                let image_description_can_contain_inlines =
                    markdown_source_has_image_syntax(&example);
                if expects_em || !image_description_can_contain_inlines {
                    record("emphasis", has_em, expects_em);
                }
                if expects_strong || !image_description_can_contain_inlines {
                    record("strong", has_strong, expects_strong);
                }
                if expects_code_span || !image_description_can_contain_inlines {
                    // Code spans are deliberately single-line here, so spec
                    // examples that only close on a later line are exempt.
                    if expects_code_span && !source_has_single_line_code_span(&example.markdown) {
                        // Deliberate deviation; nothing to record.
                    } else {
                        record("code span", has_code_span, expects_code_span);
                    }
                }
                if expects_link || !image_description_can_contain_inlines {
                    let allows_bare_link_extension = example.markdown.lines().any(|line| {
                        let line = line.trim_start();
                        line.starts_with("http://")
                            || line.starts_with("https://")
                            || line.starts_with("mailto:")
                    });
                    record("link", has_link, expects_link || allows_bare_link_extension);
                }
                record("image", has_image, expects_image);
            }
        }

        assert!(
            missing.is_empty(),
            "CommonMark decoration feature gaps:\n{}",
            missing
                .iter()
                .take(80)
                .cloned()
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    #[test]
    fn commonmark_code_spans_support_multi_backtick_delimiters() {
        let double_tick = commonmark_example(329);
        assert_eq!(double_tick.section, "Code spans");
        let decorations = parse_decorations(&double_tick.markdown);
        assert!(
            decorations[0]
                .inline_spans
                .iter()
                .any(|span| span.kind == InlineKind::InlineCode && span.range == (2..13))
        );
        assert!(
            decorations[0]
                .syntax_ranges
                .iter()
                .any(|range| range.range == (0..2))
        );
        assert!(
            decorations[0]
                .syntax_ranges
                .iter()
                .any(|range| range.range == (13..15))
        );
    }

    #[test]
    fn code_spans_do_not_pair_across_lines() {
        // Editing deviation from CommonMark example 337: an unclosed run stays
        // literal instead of swallowing the following lines.
        let multiline = commonmark_example(337);
        assert_eq!(multiline.section, "Code spans");
        let decorations = parse_decorations(&multiline.markdown);
        for decoration in &decorations {
            assert!(
                !decoration
                    .inline_spans
                    .iter()
                    .any(|span| span.kind == InlineKind::InlineCode)
            );
        }

        let stray = parse_decorations("use ``double`` here\ntext\nand `` again");
        assert!(
            stray[0]
                .inline_spans
                .iter()
                .any(|span| span.kind == InlineKind::InlineCode && span.range == (6..12))
        );
        assert!(stray[1].inline_spans.is_empty());
        assert!(
            !stray[2]
                .inline_spans
                .iter()
                .any(|span| span.kind == InlineKind::InlineCode)
        );
    }

    #[test]
    fn commonmark_spec_fixture_covers_initial_supported_examples() {
        let setext = commonmark_example(80);
        assert_eq!(setext.section, "Setext headings");
        let decorations = parse_decorations(&setext.markdown);
        assert_eq!(decorations[0].block_type, BlockType::Heading(1));
        assert_eq!(decorations[1].syntax_ranges[0].range, 0..9);
        assert_eq!(decorations[3].block_type, BlockType::Heading(2));
        assert_eq!(decorations[4].syntax_ranges[0].range, 0..9);

        let empty_ordered_item = commonmark_example(283);
        assert_eq!(empty_ordered_item.section, "List items");
        let decorations = parse_decorations(&empty_ordered_item.markdown);
        assert_eq!(decorations[0].block_type, BlockType::OrderedList);
        assert_eq!(decorations[1].block_type, BlockType::OrderedList);
        assert_eq!(decorations[2].block_type, BlockType::OrderedList);

        let decorations = parse_decorations("1. ");
        assert_eq!(decorations[0].block_type, BlockType::OrderedList);
        assert_eq!(decorations[0].syntax_ranges[0].range, 0..3);

        let changed_ordered_delimiter = commonmark_example(302);
        assert_eq!(changed_ordered_delimiter.section, "Lists");
        let decorations = parse_decorations(&changed_ordered_delimiter.markdown);
        assert_eq!(decorations[0].block_type, BlockType::OrderedList);
        assert_eq!(decorations[1].block_type, BlockType::OrderedList);
        assert_eq!(decorations[2].block_type, BlockType::OrderedList);
        assert_eq!(decorations[2].syntax_ranges[0].range, 0..3);

        let multiline_emphasis = commonmark_example(405);
        assert_eq!(multiline_emphasis.section, "Emphasis and strong emphasis");
        let decorations = parse_decorations(&multiline_emphasis.markdown);
        assert!(
            decorations[0]
                .inline_spans
                .iter()
                .any(|span| span.kind == InlineKind::Italic && span.range == (1..4))
        );
        assert!(
            decorations[1]
                .inline_spans
                .iter()
                .any(|span| span.kind == InlineKind::Italic && span.range == (0..3))
        );
    }

    #[test]
    fn commonmark_setext_headings_mark_content_and_fold_underline() {
        let text = "Foo *bar*\n---------\n";
        let decorations = parse_decorations(text);

        assert_eq!(decorations[0].block_type, BlockType::Heading(2));
        assert_ne!(decorations[1].block_type, BlockType::HorizontalRule);
        assert_eq!(decorations[1].syntax_ranges.len(), 1);
        assert_eq!(decorations[1].syntax_ranges[0].range, 0..9);
    }

    #[test]
    fn heading_attributes_are_foldable_syntax() {
        let decorations = parse_decorations("# Title {#custom .wide}\n");

        assert!(
            decorations[0]
                .syntax_ranges
                .iter()
                .any(|range| range.range == (8..23))
        );
    }

    #[test]
    fn front_matter_requires_yaml_mapping_and_supports_bom_and_document_end() {
        let scalar = parse_decorations("---\nhttps://example.com\n---\n");
        assert!(scalar.iter().all(|decoration| {
            decoration
                .table_info
                .as_ref()
                .is_none_or(|table| !table.is_front_matter)
        }));

        let mapping = parse_decorations("\u{feff}---\ntitle: Example\ntags:\n  - one\n...\n");
        assert!(mapping[..5].iter().all(|decoration| {
            decoration
                .table_info
                .as_ref()
                .is_some_and(|table| table.is_front_matter)
        }));
    }

    #[test]
    fn compact_incomplete_front_matter_is_not_parsed_as_setext_heading() {
        let text = "-\ncategory: 入门教程\ndescription: 介绍小语文稿的使用\nefefef\n-";
        let decorations = parse_decorations(text);

        assert!(decorations.iter().all(|decoration| {
            decoration
                .table_info
                .as_ref()
                .is_some_and(|table| table.is_front_matter)
        }));
        assert!(
            decorations
                .iter()
                .all(|decoration| decoration.block_type == BlockType::Table)
        );
    }

    #[test]
    fn unordered_nested_list_marker_is_not_setext_heading() {
        let text = "- parent\n  - child\n";
        let decorations = parse_decorations(text);

        assert_eq!(decorations[0].block_type, BlockType::UnorderedList);
        assert_eq!(decorations[1].block_type, BlockType::UnorderedList);
    }

    #[test]
    fn incomplete_nested_unordered_item_is_not_setext_heading() {
        let decorations = parse_decorations("- A\n  - \n");

        assert_eq!(decorations[0].block_type, BlockType::UnorderedList);
        assert_eq!(decorations[1].block_type, BlockType::UnorderedList);
        assert_eq!(decorations[0].syntax_ranges[0].range, 0..2);
        assert_eq!(decorations[1].syntax_ranges[0].range, 2..4);
    }

    #[test]
    fn incomplete_third_level_unordered_item_does_not_promote_parent_to_heading() {
        let decorations = parse_decorations("- A\n  - B\n    - \n");

        assert_eq!(decorations[0].block_type, BlockType::UnorderedList);
        assert_eq!(decorations[1].block_type, BlockType::UnorderedList);
        assert_eq!(decorations[2].block_type, BlockType::UnorderedList);
        assert_eq!(decorations[1].syntax_ranges[0].range, 2..4);
        assert_eq!(decorations[2].syntax_ranges[0].range, 4..6);
    }

    #[test]
    fn all_unordered_markers_render_immediately_at_nested_levels() {
        for marker in ["-", "*", "+"] {
            let text = format!("- A\n  - B\n    {marker} \n");
            let decorations = parse_decorations(&text);
            assert_eq!(
                decorations[2].block_type,
                BlockType::UnorderedList,
                "marker {marker}"
            );
            assert!(
                decorations[2]
                    .syntax_ranges
                    .iter()
                    .any(|range| range.range == (4..6))
            );
        }
    }

    #[test]
    fn incomplete_nested_unordered_item_inside_quote_stays_a_list() {
        let decorations = parse_decorations("> - A\n>   - \n");

        assert_eq!(decorations[0].block_type, BlockType::UnorderedList);
        assert_eq!(decorations[1].block_type, BlockType::UnorderedList);
        assert!(
            decorations[1]
                .syntax_ranges
                .iter()
                .any(|range| range.range == (4..6))
        );
    }

    #[test]
    fn task_items_keep_their_markers_when_followed_by_empty_list_items() {
        for checked in [false, true] {
            for marker in ["-", "+", "*"] {
                for quote in ["", "> "] {
                    for indent in ["", "  "] {
                        let checkbox = if checked { "[x]" } else { "[ ]" };
                        let text = format!(
                            "{quote}{indent}{marker} first\n{quote}{indent}{marker} {checkbox} **task**\n{quote}{indent}{marker} "
                        );
                        let decorations = parse_decorations(&text);
                        assert_eq!(
                            decorations[1].block_type,
                            BlockType::TaskList(checked),
                            "{text:?}"
                        );
                        let start = quote.len() + indent.len();
                        assert!(
                            decorations[1].syntax_ranges.iter().any(|range| {
                                range.range
                                    == (start..start + marker.len() + 1 + checkbox.len() + 1)
                            }),
                            "{text:?}: {:?}",
                            decorations[1].syntax_ranges
                        );
                        assert!(
                            decorations[1]
                                .inline_spans
                                .iter()
                                .any(|span| span.kind == InlineKind::Bold),
                            "{text:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn ordered_task_items_retain_checkbox_semantics() {
        for text in [
            "1. [x] done\n2. next\n",
            "1. [ ] todo\n2. \n",
            "> 1. [x] done\n> 2. \n",
        ] {
            let expected = BlockType::TaskList(text.contains("[x]"));
            let decorations = parse_decorations(text);
            assert_eq!(decorations[0].block_type, expected, "{text:?}");
            let checkbox_end = text.find(']').unwrap() + 2;
            assert!(
                decorations[0]
                    .syntax_ranges
                    .iter()
                    .any(|range| range.range.end == checkbox_end),
                "{text:?}: {:?}",
                decorations[0].syntax_ranges
            );
        }
    }

    #[test]
    fn task_recovery_preserves_nested_quotes_and_crlf() {
        for (text, index, checked, following) in [
            (
                "- parent\n  - [X] child\n    - ",
                1,
                true,
                BlockType::UnorderedList,
            ),
            (
                "> - parent\n>   - [ ] child\n>     + ",
                1,
                false,
                BlockType::UnorderedList,
            ),
            (
                "- first\r\n- [x] **done**\r\n- \r\n",
                1,
                true,
                BlockType::UnorderedList,
            ),
            ("1. [ ] task\r\n2. \r\n", 0, false, BlockType::OrderedList),
        ] {
            let decorations = parse_decorations(text);
            assert_eq!(
                decorations[index].block_type,
                BlockType::TaskList(checked),
                "{text:?}"
            );
            assert_eq!(decorations[index + 1].block_type, following, "{text:?}");
        }
    }

    #[test]
    fn empty_list_recovery_does_not_change_other_block_types() {
        for (before, suffix, checked_lines) in [
            ("# Heading", "\n- ", &[0][..]),
            ("A heading\n---", "\n- ", &[0][..]),
            ("> quoted", "\n- ", &[0][..]),
            ("```rust\n- [x] code\n```", "\n- ", &[0, 1, 2][..]),
            ("    - [x] code", "\n    - ", &[0][..]),
            (
                "| A | B |\n| --- | --- |\n| **bold** | text |",
                "\n- ",
                &[0, 1, 2][..],
            ),
            ("\\- [x] literal", "\n- ", &[0][..]),
        ] {
            let original = parse_decorations(before);
            let extended = parse_decorations(&format!("{before}{suffix}"));
            for &line in checked_lines {
                assert_eq!(
                    original[line].block_type, extended[line].block_type,
                    "{before:?} + {suffix:?}, line {line}"
                );
                assert_eq!(
                    original[line].inline_spans, extended[line].inline_spans,
                    "{before:?} + {suffix:?}, line {line}"
                );
            }
        }
    }

    #[test]
    fn adding_empty_list_item_preserves_previous_line_inline_syntax() {
        for first in [
            "- **bold**",
            "- *italic*",
            "- ~~strike~~",
            "- `code`",
            "- ==highlight==",
            "- [[wiki]]",
            "- [link](https://example.com)",
            "- $x$",
            "- [x] **done**",
            "- [ ] [link](https://example.com)",
            "- # heading",
            "- ## heading",
            "- 1. ## heading",
            "- ---",
        ] {
            let before = parse_decorations(first);
            let after = parse_decorations(&format!("{first}\n- "));
            assert_eq!(before[0].block_type, after[0].block_type, "{first}");
            assert_eq!(before[0].syntax_ranges, after[0].syntax_ranges, "{first}");
            assert_eq!(before[0].inline_spans, after[0].inline_spans, "{first}");
            assert_eq!(
                before[0].highlight_spans, after[0].highlight_spans,
                "{first}"
            );
            assert_eq!(before[0].wiki_links, after[0].wiki_links, "{first}");
            assert_eq!(
                before[0].inline_math_spans, after[0].inline_math_spans,
                "{first}"
            );
        }
    }

    #[test]
    fn ordered_list_recovery_does_not_override_nested_headings_or_rules() {
        for (text, expected) in [
            ("1. # heading", BlockType::Heading(1)),
            ("1. ---", BlockType::HorizontalRule),
        ] {
            let decorations = parse_decorations(text);
            assert_eq!(decorations[0].block_type, expected, "{text:?}");
        }
    }

    #[test]
    fn adjacent_ordered_and_unordered_items_preserve_nested_block_syntax() {
        for first in [
            "1. # heading",
            "1. ## heading",
            "1. ---",
            "1. [x] **done**",
            "1. [ ] [[notes]]",
        ] {
            let before = parse_decorations(first);
            for following in ["2. ", "- ", "* "] {
                let after = parse_decorations(&format!("{first}\n{following}"));
                assert_eq!(
                    before[0].block_type, after[0].block_type,
                    "{first:?} -> {following:?}"
                );
                assert_eq!(
                    before[0].inline_spans, after[0].inline_spans,
                    "{first:?} -> {following:?}"
                );
            }
        }
    }

    #[test]
    fn ordinary_setext_heading_still_parses() {
        let decorations = parse_decorations("Title\n---\n");

        assert_eq!(decorations[0].block_type, BlockType::Heading(2));
    }

    #[test]
    fn neighboring_list_markers_do_not_reclassify_literal_blocks() {
        for newline in ["\n", "\r\n"] {
            for source in [
                "```text\n- literal\n  - \n```\n",
                "    - literal\n      - \n",
            ] {
                let source = source.replace('\n', newline);
                let decorations = parse_decorations(&source);
                for (line, content) in source.lines().enumerate() {
                    if content.trim_start().starts_with("- ") {
                        assert_eq!(
                            decorations[line].block_type,
                            BlockType::CodeBlock,
                            "{source:?}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn quoted_multiline_setext_uses_the_container_relative_underline() {
        for newline in ["\n", "\r\n"] {
            let source = "> first\n> second\n> ===\n".replace('\n', newline);
            let decorations = parse_decorations(&source);
            assert_eq!(decorations[0].block_type, BlockType::Heading(1));
            assert_eq!(decorations[1].block_type, BlockType::Heading(1));
            assert!(
                decorations[2]
                    .syntax_ranges
                    .iter()
                    .any(|syntax| syntax.range == (2..5))
            );
        }
    }

    #[test]
    fn neighboring_blocks_keep_literal_and_heading_boundaries() {
        for newline in ["\n", "\r\n"] {
            for (source, line, expected) in [
                ("---\ncate\n---", 1, BlockType::Normal),
                ("---\n\ncate\n\n---", 2, BlockType::Normal),
                ("cate\n\n---", 0, BlockType::Normal),
                ("cate\n***", 0, BlockType::Normal),
                ("cate\n===", 0, BlockType::Heading(1)),
                ("> first\n> second\n> --", 0, BlockType::Blockquote),
                ("<pre>\n- literal\n  - \n</pre>", 1, BlockType::Normal),
                ("<div>\n- literal\n  - \n</div>", 1, BlockType::Normal),
                ("$$\n- literal\n  - \n$$", 1, BlockType::CodeBlock),
                ("```\ncate\n---\n```", 1, BlockType::CodeBlock),
            ] {
                let source = source.replace('\n', newline);
                let decorations = parse_decorations(&source);
                assert_eq!(decorations[line].block_type, expected, "{source:?}");
            }
        }
    }

    #[test]
    fn horizontal_rules_surround_plain_text_without_promoting_it_to_a_heading() {
        for newline in ["\n", "\r\n"] {
            for source in ["---\ncate\n---", "intro\n\n---\ncate\nsecond line\n---"] {
                let source = source.replace('\n', newline);
                let decorations = parse_decorations(&source);
                for (line, content) in source.lines().enumerate() {
                    if content == "---" {
                        assert_eq!(decorations[line].block_type, BlockType::HorizontalRule);
                    } else {
                        assert_eq!(decorations[line].block_type, BlockType::Normal);
                    }
                    assert!(decorations[line].front_matter.is_none());
                }
            }
            assert_eq!(
                parse_decorations(&"cate\n---".replace('\n', newline))[0].block_type,
                BlockType::Heading(2)
            );
            assert_eq!(
                parse_decorations(&"---\n# cate\n---".replace('\n', newline))[1].block_type,
                BlockType::Heading(1)
            );
            assert!(
                parse_decorations(&"---\ncate: value\n---".replace('\n', newline))[1]
                    .front_matter
                    .is_some()
            );
        }
    }

    #[test]
    fn html_blocks_do_not_gain_tables_or_math_from_neighboring_rows() {
        for newline in ["\n", "\r\n"] {
            for source in [
                "<div>\n| A | B |\n| --- | --- |\n| one | two |\n</div>",
                "<pre>\n\\[\nx + y\n\\]\n</pre>",
                "<!--\n\\[x + y\\]\n-->",
            ] {
                let source = source.replace('\n', newline);
                for row in parse_decorations(&source) {
                    assert!(row.table_info.is_none(), "{source:?}");
                    assert!(row.block_preview.is_none(), "{source:?}");
                }
            }
        }
    }

    #[test]
    fn setext_multiline_content_is_not_confused_with_atx_or_list_markers() {
        for newline in ["\n", "\r\n"] {
            for (source, level) in [("#plain\nsecond\n---", 2), ("- first\n  second\n  ===", 1)] {
                let source = source.replace('\n', newline);
                let rows = parse_decorations(&source);
                assert_eq!(rows[0].block_type, BlockType::Heading(level), "{source:?}");
                assert_eq!(rows[1].block_type, BlockType::Heading(level), "{source:?}");
                assert!(!rows[2].syntax_ranges.is_empty(), "{source:?}");
            }
        }
    }

    #[test]
    fn callout_default_titles_do_not_depend_on_line_endings() {
        for newline in ["\n", "\r\n"] {
            let text = ["> [!NOTE]", "> body"].join(newline);
            let rows = parse_decorations(&text);
            assert_eq!(rows[0].callout_kind, Some(super::CalloutKind::Note));
            assert!(
                rows[0]
                    .display_replacements
                    .iter()
                    .any(|replacement| { replacement.display_text.ends_with("NOTE") }),
                "{text:?}"
            );
        }
    }

    #[test]
    fn neighboring_literal_blocks_isolate_all_custom_inline_markers() {
        let markers = "==hi== ++under++ H~2~O x^2^ :rocket: [[note|alias]] [^n]";
        for newline in ["\n", "\r\n"] {
            for (opener, closer) in [
                ("```text", "```"),
                ("<pre>", "</pre>"),
                ("<!--", "-->"),
                ("---\nkey: |", "---"),
            ] {
                let body = if opener.starts_with("---") {
                    format!("  {markers}")
                } else {
                    markers.to_string()
                };
                let text =
                    format!("{opener}\n{body}\n{closer}\n\n{markers}").replace('\n', newline);
                let rows = parse_decorations(&text);
                let last = rows.len() - 1;
                for row in &rows[..last] {
                    assert!(row.highlight_spans.is_empty(), "{text:?}");
                    assert!(row.superscript_spans.is_empty(), "{text:?}");
                    assert!(row.subscript_spans.is_empty(), "{text:?}");
                    assert!(row.wiki_links.is_empty(), "{text:?}");
                    assert!(row.footnote_references.is_empty(), "{text:?}");
                    assert!(row.display_replacements.is_empty(), "{text:?}");
                    assert!(
                        !row.inline_spans
                            .iter()
                            .any(|span| span.kind == InlineKind::Underline),
                        "{text:?}"
                    );
                }
                assert!(!rows[last].highlight_spans.is_empty(), "{text:?}");
                assert!(!rows[last].superscript_spans.is_empty(), "{text:?}");
                assert!(!rows[last].subscript_spans.is_empty(), "{text:?}");
                assert!(!rows[last].wiki_links.is_empty(), "{text:?}");
                assert!(!rows[last].display_replacements.is_empty(), "{text:?}");
            }
        }
    }

    #[test]
    fn lone_dash_under_paragraph_starts_a_list_instead_of_a_setext_heading() {
        let typing_marker = parse_decorations("text\n-");
        assert_eq!(typing_marker[0].block_type, BlockType::Normal);
        assert_eq!(typing_marker[1].block_type, BlockType::Normal);
        assert!(typing_marker[1].syntax_ranges.is_empty());

        let with_space = parse_decorations("text\n- ");
        assert_eq!(with_space[0].block_type, BlockType::Normal);
        assert_eq!(with_space[1].block_type, BlockType::UnorderedList);
        assert_eq!(with_space[1].syntax_ranges[0].range, 0..2);

        let nested = parse_decorations("text\n  - ");
        assert_eq!(nested[0].block_type, BlockType::Normal);
        assert_eq!(nested[1].block_type, BlockType::UnorderedList);
        assert_eq!(nested[1].syntax_ranges[0].range, 2..4);
    }

    #[test]
    fn short_underlines_do_not_promote_the_paragraph_above_while_typing() {
        for underline in ["-", "--", "=", "=="] {
            let decorations = parse_decorations(&format!("text\n{underline}"));
            assert_eq!(
                decorations[0].block_type,
                BlockType::Normal,
                "{underline:?} should not promote the paragraph above"
            );
        }

        let highlight = parse_decorations("text\n==marked==");
        assert_eq!(highlight[0].block_type, BlockType::Normal);
        assert_eq!(highlight[1].highlight_spans, vec![2..8]);
    }

    #[test]
    fn full_length_underlines_still_form_setext_headings() {
        assert_eq!(
            parse_decorations("Title\n---")[0].block_type,
            BlockType::Heading(2)
        );
        assert_eq!(
            parse_decorations("Title\n===")[0].block_type,
            BlockType::Heading(1)
        );
    }

    #[test]
    fn quoted_lines_keep_their_line_level_block_syntax() {
        let cases = [
            ("> - item\n", BlockType::UnorderedList),
            ("> 1. item\n", BlockType::OrderedList),
            ("> - [ ] todo\n", BlockType::TaskList(false)),
            ("> - [x] done\n", BlockType::TaskList(true)),
            ("> # heading\n", BlockType::Heading(1)),
        ];

        for (text, expected) in cases {
            let decorations = parse_decorations(text);
            assert_eq!(decorations[0].block_type, expected, "{text:?}");
            assert!(
                decorations[0]
                    .block_contexts
                    .contains(&BlockContext::Blockquote),
                "{text:?} should stay inside the quote"
            );
            assert!(
                !decorations[0].syntax_ranges.is_empty(),
                "{text:?} should conceal its markers"
            );
        }
    }

    #[test]
    fn quoted_lists_indent_after_the_quote_marker() {
        let line = "> - b";
        let quote_len = blockquote_prefix_len(line);
        assert_eq!(quote_len, 2);

        // Tab inserts at the indent anchor, never in front of the `>`.
        let indented = format!("{}  {}", &line[..quote_len], &line[quote_len..]);
        assert_eq!(indented, ">   - b");

        let decorations = parse_decorations(&format!("> - a\n{indented}\n"));
        assert_eq!(decorations[1].block_type, BlockType::UnorderedList);
        assert_eq!(
            decorations[1]
                .syntax_ranges
                .iter()
                .map(|syntax| syntax.range.clone())
                .collect::<Vec<_>>(),
            vec![0..2, 4..6],
            "the quote marker and the nested bullet stay separately concealed"
        );
    }

    #[test]
    fn continuation_prefix_repeats_the_quote_and_its_nested_indent() {
        for (line, prefix, rest) in [
            ("- item", "", "- item"),
            ("  - item", "  ", "- item"),
            ("> - item", "> ", "- item"),
            (">   - item", ">   ", "- item"),
            ("> 1. item", "> ", "1. item"),
            ("> - [ ] todo", "> ", "- [ ] todo"),
            (">> - item", ">> ", "- item"),
            ("> text", "> ", "text"),
        ] {
            assert_eq!(
                split_continuation_prefix(line),
                (prefix.to_string(), rest),
                "{line:?}"
            );
        }
    }

    #[test]
    fn bare_unordered_markers_are_visible_until_space_is_typed() {
        for marker in ["-", "+", "*"] {
            let decorations = parse_decorations(marker);
            assert!(decorations[0].syntax_ranges.is_empty());
        }

        for marker in ["- ", "+ ", "* "] {
            let decorations = parse_decorations(marker);
            assert_eq!(decorations[0].block_type, BlockType::UnorderedList);
            assert_eq!(decorations[0].syntax_ranges[0].range, 0..2);
        }
    }

    #[test]
    fn block_markers_render_immediately_while_typing() {
        assert_eq!(parse_decorations("# ")[0].block_type, BlockType::Heading(1));
        assert_eq!(parse_decorations("> ")[0].block_type, BlockType::Blockquote);
        assert_eq!(parse_decorations(">")[0].block_type, BlockType::Blockquote);
        assert_eq!(
            parse_decorations(">quoted")[0].block_type,
            BlockType::Blockquote
        );
        assert_eq!(
            parse_decorations(">>nested")[0].block_type,
            BlockType::Blockquote
        );
        assert_eq!(
            parse_decorations("1. ")[0].block_type,
            BlockType::OrderedList
        );
        assert_eq!(
            parse_decorations("- [ ] ")[0].block_type,
            BlockType::TaskList(false)
        );
    }

    #[test]
    fn a_fence_becomes_code_after_newline_or_a_matching_closer() {
        assert_eq!(parse_decorations("```")[0].block_type, BlockType::Normal);
        assert_eq!(
            parse_decorations("```rust")[0].block_type,
            BlockType::Normal
        );
        for source in ["```\n", "```rust\n", "~~~\n"] {
            assert_eq!(
                parse_decorations(source)[0].block_type,
                BlockType::CodeBlockFence,
                "{source}"
            );
        }

        let closed = parse_decorations("```\ncode\n```\n");
        assert_eq!(closed[0].block_type, BlockType::CodeBlockFence);
        assert_eq!(closed[1].block_type, BlockType::CodeBlock);
        assert_eq!(closed[2].block_type, BlockType::CodeBlockFence);
    }

    #[test]
    fn typing_a_new_fence_does_not_break_previously_matched_fences() {
        let prefix = "```rust\nlet value = 1;\n```\n\n";
        let pending = parse_decorations(&format!("{prefix}```"));
        assert_eq!(pending[0].block_type, BlockType::CodeBlockFence);
        assert_eq!(pending[1].block_type, BlockType::CodeBlock);
        assert_eq!(pending[2].block_type, BlockType::CodeBlockFence);
        assert_eq!(pending[4].block_type, BlockType::Normal);
        let entered = parse_decorations(&format!("{prefix}```\n/"));
        assert_eq!(entered[4].block_type, BlockType::CodeBlockFence);
        assert_eq!(entered[5].block_type, BlockType::CodeBlock);
        let paired = parse_decorations(&format!("{prefix}```\n/\n```"));
        assert_eq!(paired[6].block_type, BlockType::CodeBlockFence);
    }

    #[test]
    fn literal_tilde_fences_do_not_unpair_later_real_fences() {
        for newline in ["\n", "\r\n"] {
            for opener in ["~~~~rust", "~~~~~"] {
                let text = format!("{opener}\n# literal heading").replace('\n', newline);
                assert_eq!(parse_decorations(&text)[1].block_type, BlockType::CodeBlock);
            }
            for literal in ["```text\n~~~~\n```", "<pre>\n~~~~\n</pre>"] {
                let text = format!("{literal}\n\n~~~~\n# literal heading\n~~~~\n\nafter")
                    .replace('\n', newline);
                let rows = parse_decorations(&text);
                assert_eq!(rows[4].block_type, BlockType::CodeBlockFence, "{text:?}");
                assert_eq!(rows[5].block_type, BlockType::CodeBlock, "{text:?}");
                assert_eq!(rows[6].block_type, BlockType::CodeBlockFence, "{text:?}");
                assert_eq!(rows[8].block_type, BlockType::Normal, "{text:?}");
            }
        }
    }

    #[test]
    fn pending_fence_in_the_middle_cannot_steal_an_existing_pair() {
        let before = parse_decorations("``\n\n```rust\nlet value = 1;\n```\n");
        let text = "```\n\n```rust\nlet value = 1;\n```\n";
        assert!(super::newly_typed_fence_is_pending(text, 0, &before));
        let pending = super::parse_decorations_with_pending_fence(text, 0);
        assert_eq!(pending[0].block_type, BlockType::Normal);
        assert_eq!(pending[2].block_type, BlockType::CodeBlockFence);
        assert_eq!(pending[3].block_type, BlockType::CodeBlock);
        assert_eq!(pending[4].block_type, BlockType::CodeBlockFence);
        assert!(!super::newly_typed_fence_is_pending(
            "```\nbody\n```",
            0,
            &parse_decorations("``\nbody\n```")
        ));
        assert!(!super::newly_typed_fence_is_pending(
            "```\nbody\n```",
            2,
            &parse_decorations("```\nbody\n")
        ));
    }

    #[test]
    fn incomplete_paired_inline_syntax_remains_visible() {
        for source in ["**bold", "[link]", "`code", "==highlight"] {
            let decoration = &parse_decorations(source)[0];
            assert!(decoration.syntax_ranges.is_empty(), "{source}");
        }
    }

    #[test]
    fn ordered_list_marker_inside_fenced_code_stays_code() {
        let text = "```\n1. item\n```\n";
        let decorations = parse_decorations(text);

        assert_eq!(decorations[0].block_type, BlockType::CodeBlockFence);
        assert_eq!(decorations[1].block_type, BlockType::CodeBlock);
        assert_eq!(decorations[2].block_type, BlockType::CodeBlockFence);
    }

    #[test]
    fn unclosed_code_fence_marker_reports_only_the_open_fence() {
        assert_eq!(
            unclosed_code_fence_marker("```rust", 0).as_deref(),
            Some("```")
        );
        assert_eq!(
            unclosed_code_fence_marker("text\n````\nbody", 1).as_deref(),
            Some("````")
        );
        assert_eq!(unclosed_code_fence_marker("```\nbody\n```", 0), None);
        assert_eq!(unclosed_code_fence_marker("```\nbody\n```", 2), None);
        // Inside an open fence a second marker line is body text, not an opener.
        assert_eq!(unclosed_code_fence_marker("```\n```js", 1), None);
        assert_eq!(unclosed_code_fence_marker("`` inline", 0), None);
        assert_eq!(unclosed_code_fence_marker("```a`b", 0), None);
    }

    #[test]
    fn unordered_marker_owns_the_line_when_an_ordered_list_starts_on_it() {
        for source in ["- 1.", "- 1. item", "* 2) nested", "  - 3. deep"] {
            let decoration = &parse_decorations(source)[0];
            assert_eq!(decoration.block_type, BlockType::UnorderedList, "{source}");
            let leading_ws = source.len() - source.trim_start_matches(' ').len();
            assert_eq!(
                decoration
                    .syntax_ranges
                    .iter()
                    .map(|range| range.range.clone())
                    .collect::<Vec<_>>(),
                vec![leading_ws..leading_ws + 2],
                "{source}"
            );
        }
    }

    #[test]
    fn nested_task_on_the_same_line_keeps_the_outer_list_marker() {
        for (text, expected) in [
            ("- 1. [x] nested\n- ", BlockType::UnorderedList),
            ("1. - [ ] nested\n2. ", BlockType::OrderedList),
        ] {
            let decorations = parse_decorations(text);
            assert_eq!(decorations[0].block_type, expected, "{text:?}");
            assert!(!matches!(decorations[0].block_type, BlockType::TaskList(_)));
        }
    }

    #[test]
    fn an_entered_fence_renders_the_following_lines_as_code() {
        let decorations = parse_decorations("```rust\n# not swallowed\n");

        assert_eq!(decorations[0].block_type, BlockType::CodeBlockFence);
        assert_eq!(decorations[1].block_type, BlockType::CodeBlock);
    }

    #[test]
    fn empty_strikethrough_marker_is_not_an_unclosed_tilde_fence() {
        let decorations = parse_decorations("before\n\n~~~~\n\nafter *italic*");
        assert_eq!(decorations[2].block_type, BlockType::Normal);
        assert_eq!(decorations[4].block_type, BlockType::Normal);
        assert!(
            decorations[4]
                .inline_spans
                .iter()
                .any(|span| span.kind == InlineKind::Italic)
        );

        let fenced = parse_decorations("~~~~\ncode\n~~~~");
        assert_eq!(fenced[0].block_type, BlockType::CodeBlockFence);
        assert_eq!(fenced[1].block_type, BlockType::CodeBlock);
    }

    #[test]
    fn strikethrough_delimiters_do_not_pair_across_physical_lines() {
        for text in ["~~1~~\n~~1 ~~\n\n~~ 1~~\n", "~~1~~\n~~1 ~~\n~~ 1~~\n"] {
            let decorations = parse_decorations(text);
            assert!(
                decorations[0]
                    .inline_spans
                    .iter()
                    .any(|span| span.kind == InlineKind::Strikethrough)
            );
            for decoration in decorations.iter().take(4).skip(1) {
                assert!(
                    !decoration
                        .inline_spans
                        .iter()
                        .any(|span| span.kind == InlineKind::Strikethrough)
                );
            }
        }
    }

    #[test]
    fn setext_heading_inside_blockquote_keeps_heading_and_quote_context() {
        let decorations = parse_decorations("> Title\n> ===\n");

        assert_eq!(decorations[0].block_type, BlockType::Heading(1));
        assert!(
            decorations[0]
                .block_contexts
                .contains(&BlockContext::Blockquote)
        );
    }

    #[test]
    fn non_initial_front_matter_shape_does_not_override_setext_heading() {
        let decorations = parse_decorations("Title\n---\nkey: value\n---\n");

        assert_eq!(decorations[0].block_type, BlockType::Heading(2));
        assert!(decorations.iter().all(|decoration| {
            decoration
                .table_info
                .as_ref()
                .is_none_or(|table| !table.is_front_matter)
        }));
    }

    #[test]
    fn atx_heading_is_not_overridden_by_adjacent_table_shaped_lines() {
        let decorations = parse_decorations("# Title\n| a | b |\n| --- | --- |\n");

        assert_eq!(decorations[0].block_type, BlockType::Heading(1));
        assert_eq!(decorations[1].block_type, BlockType::Table);
    }

    #[test]
    fn indented_code_has_no_synthetic_fence_lines() {
        let text = "    first line\n    second line\n";
        let decorations = parse_decorations(text);

        for decoration in &decorations[..2] {
            assert_eq!(decoration.block_type, BlockType::CodeBlock);
            assert!(decoration.syntax_ranges.is_empty());
        }
    }

    #[test]
    fn table_shaped_lines_inside_fenced_code_stay_code() {
        let text = "```text\n<hex>.blob.data\n+---+---+\n| a | b |\n| c | d |\n+---+---+\n\n<hex>.blob.meta\n+---+---+\n| e | f |\n| g | h |\n+---+---+\n```\n";
        let decorations = parse_decorations(text);
        let closing_fence = decorations.len() - 2;

        assert_eq!(decorations[0].block_type, BlockType::CodeBlockFence);
        assert_eq!(
            decorations[closing_fence].block_type,
            BlockType::CodeBlockFence
        );
        for decoration in &decorations[1..closing_fence] {
            assert_eq!(decoration.block_type, BlockType::CodeBlock);
            assert!(decoration.table_info.is_none());
        }
    }

    #[test]
    fn tables_require_a_separator_and_allow_optional_outer_pipes() {
        let prose = parse_decorations("| one | two |\n| three | four |\n");
        assert!(
            prose
                .iter()
                .all(|decoration| decoration.table_info.is_none())
        );

        let table = parse_decorations("one | two\n--- | ---\nthree | four\n");
        assert!(table[..3].iter().all(|decoration| {
            decoration.block_type == BlockType::Table && decoration.table_info.is_some()
        }));
    }

    #[test]
    fn table_body_stops_before_heading_or_mismatched_columns() {
        let text = "| A | B |\n| --- | --- |\n| 1 | 2 |\n# ad|bc\n| x | y | z |\n";
        let decorations = parse_decorations(text);

        assert!(decorations[..3].iter().all(|decoration| {
            decoration.block_type == BlockType::Table && decoration.table_info.is_some()
        }));
        assert_eq!(decorations[3].block_type, BlockType::Heading(1));
        assert!(decorations[3].table_info.is_none());
        assert!(decorations[4].table_info.is_none());
    }

    #[test]
    fn tables_do_not_cross_blockquote_container_boundaries() {
        for newline in ["\n", "\r\n"] {
            for source in [
                "> | A | B |\n> | --- | --- |\n| outside | row |",
                "| A | B |\n| --- | --- |\n> | outside | row |",
                "> | A | B |\n> | --- | --- |\n>> | nested | row |",
            ] {
                let source = source.replace('\n', newline);
                let rows = parse_decorations(&source);
                assert_eq!(
                    rows[0].table_info.as_ref().unwrap().table_end_line,
                    2,
                    "{source:?}"
                );
                assert!(rows[2].table_info.is_none(), "{source:?}");
            }
            let source = "> | A | B |\n| --- | --- |".replace('\n', newline);
            assert!(
                parse_decorations(&source)
                    .iter()
                    .all(|row| row.table_info.is_none())
            );
        }
    }

    #[test]
    fn table_decorations_preserve_column_alignments() {
        let table = parse_decorations("| A | B | C |\n| :--- | :---: | ---: |\n| 1 | 2 | 3 |\n");
        let expected = vec![TableAlign::Left, TableAlign::Center, TableAlign::Right];
        for decoration in &table[..3] {
            assert_eq!(
                decoration
                    .table_info
                    .as_ref()
                    .expect("table line")
                    .column_alignments,
                expected
            );
        }
    }

    #[test]
    fn tables_inside_blockquotes_preserve_quote_context() {
        let table = parse_decorations("> A | B\n> --- | ---\n> 1 | 2\n");

        assert!(table[..3].iter().all(|decoration| {
            decoration.block_type == BlockType::Table
                && decoration
                    .block_contexts
                    .contains(&BlockContext::Blockquote)
        }));
    }

    #[test]
    fn table_cells_ignore_escaped_and_code_span_pipes() {
        let text = "| A | B |\n| --- | --- |\n| left \\| right | `a|b` |\n";
        let decorations = parse_decorations(text);
        let cells = &decorations[2].table_info.as_ref().expect("table row").cells;

        assert_eq!(cells, &["left \\| right", "`a|b`"]);
    }

    #[test]
    fn commonmark_multiline_emphasis_spans_each_source_line() {
        let text = "*foo\nbar*";
        let decorations = parse_decorations(text);

        assert!(
            decorations[0]
                .inline_spans
                .iter()
                .any(|span| span.kind == InlineKind::Italic && span.range == (1..4))
        );
        assert!(
            decorations[1]
                .inline_spans
                .iter()
                .any(|span| span.kind == InlineKind::Italic && span.range == (0..3))
        );
        assert!(
            decorations[0]
                .syntax_ranges
                .iter()
                .any(|range| range.range == (0..1))
        );
        assert!(
            decorations[1]
                .syntax_ranges
                .iter()
                .any(|range| range.range == (3..4))
        );
    }

    #[test]
    fn commonmark_ordered_list_markers_accept_parenthesis_tabs_and_empty_items() {
        let text = "1) one\n2.\ttwo\n3.\n";
        let decorations = parse_decorations(text);

        assert_eq!(decorations[0].block_type, BlockType::OrderedList);
        assert_eq!(decorations[1].block_type, BlockType::OrderedList);
        assert_eq!(decorations[2].block_type, BlockType::OrderedList);
        assert_eq!(decorations[0].syntax_ranges[0].range, 0..3);
        assert_eq!(decorations[1].syntax_ranges[0].range, 0..3);
        assert_eq!(decorations[2].syntax_ranges[0].range, 0..2);
    }

    #[test]
    fn parse_decorations_collects_footnote_references_and_definitions() {
        let text = "Text[^note]\n\n[^note]: footnote body\n";
        let decorations = parse_decorations(text);

        assert_eq!(decorations[0].footnote_references.len(), 1);
        assert_eq!(decorations[0].footnote_references[0].label, "note");
        assert_eq!(
            decorations[2].footnote_definition_label.as_deref(),
            Some("note")
        );
        assert_eq!(decorations[2].display_replacements.len(), 1);
        assert_eq!(
            decorations[2].display_replacements[0].display_text,
            "note: "
        );
    }

    #[test]
    fn parse_decorations_detects_emoji_shortcode_replacements() {
        let text = "launch :rocket: now";
        let decorations = parse_decorations(text);

        assert_eq!(decorations[0].display_replacements.len(), 1);
        assert_eq!(decorations[0].display_replacements[0].source_range, 7..15);
        assert_eq!(decorations[0].display_replacements[0].display_text, "🚀");
    }

    #[test]
    fn parse_decorations_handles_cjk_text_without_utf8_boundary_panics() {
        let text = "有关于更多 Markdown 格式的说明，可以参考 Github 的 [Markdown 语法指南](https://docs.github.com/en/get-started)，了解和尝试 ==高亮== 与 H~2~O。";
        let decorations = parse_decorations(text);

        assert_eq!(decorations.len(), 1);
        assert_eq!(decorations[0].highlight_spans.len(), 1);
        assert_eq!(decorations[0].subscript_spans.len(), 1);
    }

    #[test]
    fn nested_syntax_ranges_stay_within_utf8_line_boundaries() {
        fn assert_range(line: &str, range: &Range<usize>, context: &str) {
            assert!(range.start <= range.end, "reversed {context}: {range:?}");
            assert!(
                range.end <= line.len(),
                "out of bounds {context}: {range:?}, line len {}, line {line:?}",
                line.len()
            );
            assert!(
                line.is_char_boundary(range.start),
                "start boundary {context}: {range:?}"
            );
            assert!(
                line.is_char_boundary(range.end),
                "end boundary {context}: {range:?}"
            );
        }

        let documents = [
            "**==粗体 `代码`==** ++下划线 :rocket:++ H~2~O",
            "[==标签==](路径(一).md) [[目录/文档.md|++别名++ :rocket:]]",
            "$x^2^ + 中文$ <code>==字面量==</code>",
            "| 列一 | 列二 |\n| --- | --- |\n| **粗体** | ==高亮== ++线++ |",
            "> [!NOTE] **标题**\n> 正文 ==高亮==",
            "---\ntitle: \"==字面量==\"\n---",
            r"\==escaped== ==open ++broken ^x ~y [[nested [[inner]] tail]]",
        ];

        for document in documents {
            let decorations = parse_decorations(document);
            for (line, decoration) in document.split('\n').zip(&decorations) {
                for span in &decoration.inline_spans {
                    assert_range(line, &span.range, "inline span");
                }
                for range in decoration
                    .inline_math_spans
                    .iter()
                    .chain(&decoration.protected_ranges)
                    .chain(&decoration.highlight_spans)
                    .chain(&decoration.superscript_spans)
                    .chain(&decoration.subscript_spans)
                {
                    assert_range(line, range, "decoration");
                }
                for syntax in &decoration.syntax_ranges {
                    assert_range(line, &syntax.range, "syntax");
                    assert_range(line, &syntax.trigger_range, "syntax trigger");
                }
                for replacement in &decoration.display_replacements {
                    assert_range(line, &replacement.source_range, "replacement");
                    assert_range(line, &replacement.trigger_range, "replacement trigger");
                }
                for wiki_link in &decoration.wiki_links {
                    assert_range(line, &wiki_link.full_range, "wiki link");
                    assert_range(line, &wiki_link.collapsed_range, "wiki label");
                }
                for footnote in &decoration.footnote_references {
                    assert_range(line, &footnote.full_range, "footnote");
                    assert_range(line, &footnote.collapsed_range, "footnote label");
                }
                if let Some(table) = &decoration.table_info {
                    for range in &table.cell_ranges {
                        assert_range(line, range, "table cell");
                    }
                }
            }
        }
    }

    #[test]
    fn parse_decorations_detects_math_and_mermaid_block_previews() {
        let text = "$$\na^2 + b^2 = c^2\n$$\n\n```mermaid\ngraph TD\nA-->B\n```\n";
        let decorations = parse_decorations(text);

        let math_preview = decorations[0].block_preview.as_ref().expect("math preview");
        assert_eq!(math_preview.kind, BlockPreviewKind::Math);
        assert!(math_preview.is_anchor);
        assert_eq!(math_preview.end_line, 2);

        let mermaid_preview = decorations[4]
            .block_preview
            .as_ref()
            .expect("mermaid preview");
        assert_eq!(mermaid_preview.kind, BlockPreviewKind::Mermaid);
        assert!(mermaid_preview.is_anchor);
        assert_eq!(mermaid_preview.end_line, 7);
    }

    #[test]
    fn inference_rule_inside_dollar_fence_becomes_math_preview() {
        let text = "$$\n\\frac{t_1\\in T}{\\texttt{succ }t_1\\in T}\n$$\n";
        let decorations = parse_decorations(text);
        let preview = decorations
            .iter()
            .filter_map(|decoration| decoration.block_preview.as_ref())
            .find(|preview| preview.is_anchor)
            .expect("math preview");

        assert_eq!(preview.kind, BlockPreviewKind::Math);
        assert_eq!(
            preview.source_text,
            r"\frac{t_1\in T}{\texttt{succ }t_1\in T}"
        );
    }

    #[test]
    fn latex_display_brackets_become_math_previews() {
        for text in ["\\[\\frac{a}{b}\\]\n", "\\[\n\\frac{a}{b}\n\\]\n"] {
            let decorations = parse_decorations(text);
            let preview = decorations
                .iter()
                .filter_map(|decoration| decoration.block_preview.as_ref())
                .find(|preview| preview.is_anchor)
                .expect("math preview");
            assert_eq!(preview.kind, BlockPreviewKind::Math);
            assert_eq!(preview.source_text, r"\frac{a}{b}");
        }
    }

    #[test]
    fn plain_square_brackets_do_not_become_math_preview() {
        let decorations = parse_decorations(r"[ \frac{a}{b} ]");
        assert!(
            decorations
                .iter()
                .all(|decoration| decoration.block_preview.is_none())
        );
    }

    #[test]
    fn parse_decorations_treats_display_math_as_code_block_group() {
        let text = "$$\n\\frac{a}{b} \\to c\n$$\n";
        let decorations = parse_decorations(text);

        assert_eq!(decorations[0].block_type, BlockType::CodeBlockFence);
        assert_eq!(decorations[1].block_type, BlockType::CodeBlock);
        assert_eq!(decorations[2].block_type, BlockType::CodeBlockFence);
        assert_eq!(decorations[0].code_language.as_deref(), Some("latex"));
        assert_eq!(decorations[1].code_language.as_deref(), Some("latex"));
        assert_eq!(decorations[2].code_language.as_deref(), Some("latex"));
    }

    #[test]
    fn parse_decorations_handles_editor_document_without_utf8_boundary_panics() {
        let text = include_str!("../docs/help-zh-cn.md");
        let decorations = parse_decorations(text);

        assert!(!decorations.is_empty());
        assert!(
            decorations
                .iter()
                .any(|decoration| !decoration.highlight_spans.is_empty())
        );
        assert!(
            decorations
                .iter()
                .any(|decoration| !decoration.footnote_references.is_empty())
        );
    }

    #[test]
    fn parse_decorations_detects_all_editor_math_block_previews() {
        let text = include_str!("../docs/help-zh-cn.md");
        let decorations = parse_decorations(text);

        let math_previews: Vec<_> = decorations
            .iter()
            .filter_map(|decoration| decoration.block_preview.as_ref())
            .filter(|preview| preview.is_anchor && preview.kind == BlockPreviewKind::Math)
            .collect();

        assert_eq!(math_previews.len(), 1);
        assert!(
            math_previews
                .iter()
                .any(|preview| preview.source_text.contains(r"arg\,min"))
        );
        assert!(text.lines().zip(&decorations).any(|(line, decoration)| {
            line.contains("E = mc^2") && !decoration.inline_math_spans.is_empty()
        }));
    }
}

#[cfg(test)]
mod quote_merge_tests {
    use super::{BlockContext, BlockType, parse_decorations};

    #[test]
    fn consecutive_quote_lines_conceal_markers_on_every_line() {
        // List rows, blank `>` separators, and nested content inside a quote
        // must all hide their `>` prefix so the run renders as one block.
        let text = "> a\n>\n> - item one\n> - item two\n>\n> 1. ordered\n> b";
        let decorations = parse_decorations(text);
        for (index, (line, decoration)) in text.lines().zip(decorations.iter()).enumerate() {
            let in_quote = decoration.block_type == BlockType::Blockquote
                || decoration
                    .block_contexts
                    .contains(&BlockContext::Blockquote);
            assert!(in_quote, "line {index} {line:?} should be in the quote");
            let marker_len = line.len() - line.trim_start_matches(['>', ' ']).len();
            assert!(
                decoration
                    .syntax_ranges
                    .iter()
                    .any(|syntax_range| syntax_range.range.start == 0
                        && syntax_range.range.end >= marker_len.min(2)),
                "line {index} {line:?} should conceal its quote marker"
            );
        }
    }
}
