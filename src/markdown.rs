use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};
use std::ops::Range;

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

#[derive(Debug, Clone)]
pub struct InlineSpan {
    pub kind: InlineKind,
    pub range: Range<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InlineKind {
    Bold,
    Italic,
    Strikethrough,
    InlineCode,
    Link,
    Image,
}

/// A byte range to hide when syntax is folded, plus a trigger range that
/// controls when to show it (cursor column in trigger_range → range is shown).
#[derive(Debug, Clone)]
pub struct SyntaxRange {
    pub range: Range<usize>,
    pub trigger_range: Range<usize>,
}

#[derive(Debug, Clone)]
pub struct LineDecoration {
    pub block_type: BlockType,
    pub inline_spans: Vec<InlineSpan>,
    pub syntax_ranges: Vec<SyntaxRange>,
    pub table_info: Option<TableLineInfo>,
    pub code_language: Option<String>,
    /// For image lines, the local file path from `![alt](path)`.
    pub image_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct TableLineInfo {
    pub cells: Vec<String>,
    pub cell_ranges: Vec<Range<usize>>,
    pub is_header: bool,
    pub is_separator: bool,
    pub table_start_line: usize,
    pub table_end_line: usize,
    pub column_widths: Vec<usize>,
}

impl Default for LineDecoration {
    fn default() -> Self {
        Self {
            block_type: BlockType::Normal,
            inline_spans: Vec::new(),
            syntax_ranges: Vec::new(),
            table_info: None,
            code_language: None,
            image_path: None,
        }
    }
}

pub fn parse_decorations(text: &str) -> Vec<LineDecoration> {
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

    let offset_to_line = |offset: usize| -> usize {
        match line_starts.binary_search(&offset) {
            Ok(line) => line,
            Err(line) => line.saturating_sub(1),
        }
    };

    let offset_to_line_col = |offset: usize| -> (usize, usize) {
        let line = offset_to_line(offset);
        let col = offset - line_starts[line];
        (line, col)
    };

    let opts = Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_HEADING_ATTRIBUTES
        | Options::ENABLE_TABLES;

    let parser = Parser::new_ext(text, opts);

    let mut in_code_block = false;
    let mut current_code_language: Option<String> = None;
    let mut in_heading: Option<u8> = None;
    let mut in_blockquote = false;
    let mut inline_stack: Vec<(InlineKind, usize, Option<String>)> = Vec::new();

    for (event, source_range) in parser.into_offset_iter() {
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                let line = offset_to_line(source_range.start);
                let lvl = level as u8;
                in_heading = Some(lvl);
                if line < decorations.len() {
                    decorations[line].block_type = BlockType::Heading(lvl);
                    let prefix_len = (lvl as usize) + 1;
                    let line_len = {
                        let start = line_starts[line];
                        let end = if line + 1 < line_starts.len() {
                            line_starts[line + 1].min(text.len())
                        } else {
                            text.len()
                        };
                        end - start
                    };
                    if prefix_len <= line_len {
                        decorations[line].syntax_ranges.push(SyntaxRange {
                            range: 0..prefix_len,
                            trigger_range: 0..prefix_len,
                        });
                    }
                }
            }
            Event::End(TagEnd::Heading(_)) => {
                in_heading = None;
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                let line = offset_to_line(source_range.start);
                in_code_block = true;
                current_code_language = match kind {
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
                if line < decorations.len() {
                    decorations[line].block_type = BlockType::CodeBlockFence;
                    decorations[line].code_language = current_code_language.clone();
                    let line_content_len = text[source_range.start..]
                        .find('\n')
                        .unwrap_or(text.len() - source_range.start);
                    decorations[line].syntax_ranges.push(SyntaxRange {
                        range: 0..line_content_len,
                        trigger_range: 0..line_content_len,
                    });
                }
            }
            Event::End(TagEnd::CodeBlock) => {
                in_code_block = false;
                current_code_language = None;
                let line = offset_to_line(source_range.end.saturating_sub(1));
                if line < decorations.len() && decorations[line].block_type == BlockType::Normal {
                    decorations[line].block_type = BlockType::CodeBlockFence;
                }
            }
            Event::Start(Tag::BlockQuote(_)) => {
                in_blockquote = true;
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                in_blockquote = false;
            }
            Event::Start(Tag::List(_)) => {}
            Event::Start(Tag::Item) => {
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
                    let leading_ws = content.len() - content.trim_start_matches([' ', '\t']).len();
                    let trimmed = &content[leading_ws..];
                    if trimmed.starts_with("- ")
                        || trimmed.starts_with("* ")
                        || trimmed.starts_with("+ ")
                    {
                        decorations[line].block_type = BlockType::UnorderedList;
                        decorations[line].syntax_ranges.push(SyntaxRange {
                            range: leading_ws..leading_ws + 2,
                            trigger_range: leading_ws..leading_ws + 2,
                        });
                    } else {
                        let prefix_len = ordered_list_prefix_len(trimmed);
                        if prefix_len > 0 {
                            decorations[line].block_type = BlockType::OrderedList;
                            decorations[line].syntax_ranges.push(SyntaxRange {
                                range: leading_ws..leading_ws + prefix_len,
                                trigger_range: leading_ws..leading_ws + prefix_len,
                            });
                        } else {
                            decorations[line].block_type = BlockType::UnorderedList;
                        }
                    }
                }
            }
            Event::Start(Tag::Emphasis) => {
                inline_stack.push((InlineKind::Italic, source_range.start, None));
            }
            Event::End(TagEnd::Emphasis) => {
                if let Some((kind, start, _)) = inline_stack.pop() {
                    if kind == InlineKind::Italic {
                        add_inline_span(
                            &mut decorations,
                            &line_starts,
                            &offset_to_line_col,
                            start,
                            source_range.end,
                            InlineKind::Italic,
                            text,
                        );
                    }
                }
            }
            Event::Start(Tag::Strong) => {
                inline_stack.push((InlineKind::Bold, source_range.start, None));
            }
            Event::End(TagEnd::Strong) => {
                if let Some((kind, start, _)) = inline_stack.pop() {
                    if kind == InlineKind::Bold {
                        add_inline_span(
                            &mut decorations,
                            &line_starts,
                            &offset_to_line_col,
                            start,
                            source_range.end,
                            InlineKind::Bold,
                            text,
                        );
                    }
                }
            }
            Event::Start(Tag::Strikethrough) => {
                inline_stack.push((InlineKind::Strikethrough, source_range.start, None));
            }
            Event::End(TagEnd::Strikethrough) => {
                if let Some((kind, start, _)) = inline_stack.pop() {
                    if kind == InlineKind::Strikethrough {
                        add_inline_span(
                            &mut decorations,
                            &line_starts,
                            &offset_to_line_col,
                            start,
                            source_range.end,
                            InlineKind::Strikethrough,
                            text,
                        );
                    }
                }
            }
            Event::Start(Tag::Link { .. }) => {
                inline_stack.push((InlineKind::Link, source_range.start, None));
            }
            Event::End(TagEnd::Link) => {
                if let Some((kind, start, _)) = inline_stack.pop() {
                    if kind == InlineKind::Link {
                        add_inline_span(
                            &mut decorations,
                            &line_starts,
                            &offset_to_line_col,
                            start,
                            source_range.end,
                            InlineKind::Link,
                            text,
                        );
                    }
                }
            }
            Event::Start(Tag::Image { dest_url, .. }) => {
                inline_stack.push((
                    InlineKind::Image,
                    source_range.start,
                    Some(dest_url.to_string()),
                ));
            }
            Event::End(TagEnd::Image) => {
                if let Some((kind, start, dest_url)) = inline_stack.pop() {
                    if kind == InlineKind::Image {
                        add_inline_span(
                            &mut decorations,
                            &line_starts,
                            &offset_to_line_col,
                            start,
                            source_range.end,
                            InlineKind::Image,
                            text,
                        );
                        // Store image path for preview rendering
                        if let Some(url) = dest_url {
                            if !url.starts_with("http://") && !url.starts_with("https://") {
                                let line = offset_to_line(start);
                                if line < decorations.len() {
                                    decorations[line].image_path = Some(url);
                                }
                            }
                        }
                    }
                }
            }
            Event::Code(_) => {
                let (line, col) = offset_to_line_col(source_range.start);
                if line < decorations.len() {
                    let end_col = col + source_range.end - source_range.start;
                    let trigger = col..end_col;
                    decorations[line].syntax_ranges.push(SyntaxRange {
                        range: col..col + 1,
                        trigger_range: trigger.clone(),
                    });
                    decorations[line].syntax_ranges.push(SyntaxRange {
                        range: end_col - 1..end_col,
                        trigger_range: trigger,
                    });
                    decorations[line].inline_spans.push(InlineSpan {
                        kind: InlineKind::InlineCode,
                        range: col + 1..end_col - 1,
                    });
                }
            }
            Event::Text(_) => {
                if in_code_block {
                    let start_line = offset_to_line(source_range.start);
                    let end_line = offset_to_line(source_range.end.saturating_sub(1));
                    for line in start_line..=end_line.min(decorations.len().saturating_sub(1)) {
                        if decorations[line].block_type == BlockType::Normal {
                            decorations[line].block_type = BlockType::CodeBlock;
                        }
                        decorations[line].code_language = current_code_language.clone();
                    }
                } else if in_blockquote {
                    let line = offset_to_line(source_range.start);
                    if line < decorations.len() && decorations[line].block_type == BlockType::Normal
                    {
                        decorations[line].block_type = BlockType::Blockquote;
                        // Add '> ' prefix to syntax_ranges for folding
                        let line_start = line_starts[line];
                        let line_end_offset =
                            line_starts.get(line + 1).copied().unwrap_or(text.len());
                        let line_content = &text[line_start..line_end_offset];
                        let prefix_len = if line_content.starts_with("> ") {
                            2
                        } else if line_content.starts_with(">") {
                            1
                        } else {
                            0
                        };
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
                    decorations[line].block_type = BlockType::TaskList(checked);
                    // Add checkbox marker ([ ] or [x]) to syntax ranges
                    let line_start = line_starts[line];
                    let line_end_offset = line_starts.get(line + 1).copied().unwrap_or(text.len());
                    let line_content = &text[line_start..line_end_offset];
                    let leading_ws =
                        line_content.len() - line_content.trim_start_matches([' ', '\t']).len();
                    let mut col_end = source_range.end.saturating_sub(line_start);
                    // Include trailing space after ] if present
                    if col_end < line_content.len() && line_content.as_bytes()[col_end] == b' ' {
                        col_end += 1;
                    }
                    decorations[line]
                        .syntax_ranges
                        .retain(|sr| sr.range != (leading_ws..leading_ws + 2));
                    decorations[line].syntax_ranges.push(SyntaxRange {
                        range: leading_ws..col_end,
                        trigger_range: leading_ws..col_end,
                    });
                }
            }
            _ => {}
        }
    }

    // Second pass: detect table lines
    detect_tables(&mut decorations, text, &line_starts);

    decorations
}

fn add_inline_span(
    decorations: &mut [LineDecoration],
    line_starts: &[usize],
    offset_to_line_col: &dyn Fn(usize) -> (usize, usize),
    start: usize,
    end: usize,
    kind: InlineKind,
    text: &str,
) {
    let (start_line, start_col) = offset_to_line_col(start);
    let (end_line, end_col) = offset_to_line_col(end);

    if start_line >= decorations.len() || start_line != end_line {
        // Multi-line spans: only handle the first line portion
        if start_line < decorations.len() {
            let marker_len = match &kind {
                InlineKind::Bold => 2,
                InlineKind::Italic => 1,
                InlineKind::Strikethrough => 2,
                _ => 0,
            };
            let line_end = if start_line + 1 < line_starts.len() {
                line_starts[start_line + 1] - line_starts[start_line]
            } else {
                text.len() - line_starts[start_line]
            };
            decorations[start_line].inline_spans.push(InlineSpan {
                kind,
                range: start_col + marker_len..line_end,
            });
        }
        return;
    }

    let full_trigger = start_col..end_col;

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
            if let Some(bracket_pos) = source.find("](") {
                let text_end_col = start_col + bracket_pos;
                decorations[start_line].inline_spans.push(InlineSpan {
                    kind,
                    range: start_col + prefix_len..text_end_col,
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
        }
        _ => {}
    }
}

pub fn ordered_list_prefix_len(content: &str) -> usize {
    let bytes = content.as_bytes();
    let mut i = 0;
    while i < bytes.len() && bytes[i].is_ascii_digit() {
        i += 1;
    }
    if i == 0 || i >= bytes.len() {
        return 0;
    }
    if bytes[i] == b'.' && i + 1 < bytes.len() && bytes[i + 1] == b' ' {
        i + 2
    } else {
        0
    }
}

fn get_line_content<'a>(
    text: &'a str,
    line_starts: &[usize],
    line: usize,
    line_count: usize,
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

fn is_table_line(content: &str) -> bool {
    let trimmed = content.trim();
    trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.len() >= 3
}

pub fn is_separator_row(cells: &[String]) -> bool {
    cells.iter().all(|c| {
        let t = c.trim();
        !t.is_empty() && t.chars().all(|ch| ch == '-' || ch == ':' || ch == ' ') && t.contains('-')
    })
}

pub fn parse_table_cells(content: &str) -> Vec<String> {
    parse_table_cells_with_ranges(content)
        .into_iter()
        .map(|(cell, _)| cell)
        .collect()
}

pub fn parse_table_cells_with_ranges(content: &str) -> Vec<(String, Range<usize>)> {
    let trimmed = content.trim();
    let trimmed_offset = content.find(trimmed).unwrap_or(0);
    let inner = if trimmed.starts_with('|') && trimmed.ends_with('|') {
        &trimmed[1..trimmed.len() - 1]
    } else {
        trimmed
    };
    let inner_offset = trimmed_offset + usize::from(trimmed.starts_with('|'));
    let mut cells = Vec::new();
    let mut segment_start = 0;

    for (index, ch) in inner.char_indices() {
        if ch != '|' {
            continue;
        }
        let raw = &inner[segment_start..index];
        let leading_ws = raw.len() - raw.trim_start_matches([' ', '\t']).len();
        let trailing_ws = raw.len() - raw.trim_end_matches([' ', '\t']).len();
        let trimmed_start = segment_start + leading_ws;
        let trimmed_end = index.saturating_sub(trailing_ws);
        let text = if trimmed_start <= trimmed_end {
            inner[trimmed_start..trimmed_end].to_string()
        } else {
            String::new()
        };
        let anchor = inner_offset + segment_start + leading_ws.min(raw.len());
        let range = if trimmed_start <= trimmed_end {
            (inner_offset + trimmed_start)..(inner_offset + trimmed_end)
        } else {
            anchor..anchor
        };
        cells.push((text, range));
        segment_start = index + ch.len_utf8();
    }

    let raw = &inner[segment_start..];
    let leading_ws = raw.len() - raw.trim_start_matches([' ', '\t']).len();
    let trailing_ws = raw.len() - raw.trim_end_matches([' ', '\t']).len();
    let trimmed_start = segment_start + leading_ws;
    let trimmed_end = inner.len().saturating_sub(trailing_ws);
    let text = if trimmed_start <= trimmed_end {
        inner[trimmed_start..trimmed_end].to_string()
    } else {
        String::new()
    };
    let anchor = inner_offset + segment_start + leading_ws.min(raw.len());
    let range = if trimmed_start <= trimmed_end {
        (inner_offset + trimmed_start)..(inner_offset + trimmed_end)
    } else {
        anchor..anchor
    };
    cells.push((text, range));

    cells
}

fn detect_tables(decorations: &mut [LineDecoration], text: &str, line_starts: &[usize]) {
    let line_count = decorations.len();
    let mut line_idx = 0;

    while line_idx < line_count {
        let content = get_line_content(text, line_starts, line_idx, line_count);
        if is_table_line(content) {
            let table_start = line_idx;
            while line_idx < line_count {
                let c = get_line_content(text, line_starts, line_idx, line_count);
                if !is_table_line(c) {
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
                let mut col_widths = vec![0usize; max_cols];
                for row in &all_cells {
                    if is_separator_row(row) {
                        continue;
                    }
                    for (ci, cell) in row.iter().enumerate() {
                        if ci < max_cols {
                            col_widths[ci] = col_widths[ci].max(cell.len());
                        }
                    }
                }
                // Minimum column width
                for w in col_widths.iter_mut() {
                    *w = (*w).max(3);
                }

                for (ri, l) in (table_start..table_end).enumerate() {
                    let is_sep = is_separator_row(&all_cells[ri]);
                    decorations[l].block_type = BlockType::Table;
                    decorations[l].table_info = Some(TableLineInfo {
                        cells: all_cells[ri].clone(),
                        cell_ranges: parsed_rows[ri]
                            .iter()
                            .map(|(_, range)| range.clone())
                            .collect(),
                        is_header: ri == 0,
                        is_separator: is_sep,
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
