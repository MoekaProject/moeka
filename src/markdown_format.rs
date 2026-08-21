//! Normalizes Markdown source with line-level rewrites only.
//!
//! A parse/re-emit round trip was tried first and rejected: pulldown-cmark does
//! not know this editor's callouts, wiki links or highlights, so re-emitting
//! escaped them (`> \[!NOTE\]`, `\[\[page\]\]`) and broke rendering. Everything
//! here works on whole lines and leaves inline text untouched.

use crate::markdown;

/// Byte length of a leading YAML front matter block, which is passed through
/// untouched.
fn front_matter_len(source: &str) -> usize {
    let mut lines = source.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return 0;
    };
    let compact = match first.trim_start_matches('\u{feff}').trim_end() {
        "---" => false,
        "-" => true,
        _ => return 0,
    };

    let mut consumed = first.len();
    for line in lines {
        consumed += line.len();
        let trimmed = line.trim_end();
        let closes = if compact {
            trimmed == "-"
        } else {
            matches!(trimmed, "---" | "...")
        };
        if closes {
            return consumed;
        }
    }
    0
}

fn fence_opener(content: &str) -> Option<(char, usize)> {
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
    (run >= 3).then_some((marker, run))
}

fn is_fence_closer(content: &str, marker: char, open_run: usize) -> bool {
    let leading = content.len() - content.trim_start_matches(' ').len();
    if leading > 3 {
        return false;
    }
    let rest = &content[leading..];
    let run = rest.chars().take_while(|ch| *ch == marker).count();
    run >= open_run && rest[run..].trim().is_empty()
}

/// A run of `*`, `-` or `_` is a rule, not a list marker.
fn is_thematic_break(content: &str) -> bool {
    let trimmed = content.trim();
    let Some(marker) = trimmed.chars().next() else {
        return false;
    };
    if !matches!(marker, '*' | '-' | '_') {
        return false;
    }
    trimmed.chars().filter(|ch| *ch == marker).count() >= 3
        && trimmed
            .chars()
            .all(|ch| ch == marker || ch == ' ' || ch == '\t')
}

/// Keeps a hard line break (two trailing spaces) but drops stray padding.
fn trim_trailing(content: &str) -> String {
    let trimmed = content.trim_end_matches([' ', '\t']);
    if trimmed.is_empty() {
        return String::new();
    }
    if content.len() - trimmed.len() >= 2 {
        format!("{trimmed}  ")
    } else {
        trimmed.to_string()
    }
}

/// Drops the optional closing `###` run of an ATX heading. The opening marker is
/// left alone: `#text` without a space is not a heading, so adding one would
/// change the meaning.
fn normalize_atx_heading(content: &str) -> String {
    let indent_len = content.len() - content.trim_start_matches(' ').len();
    if indent_len > 3 {
        return content.to_string();
    }
    let (indent, rest) = content.split_at(indent_len);
    let level = rest.bytes().take_while(|byte| *byte == b'#').count();
    if level == 0 || level > 6 {
        return content.to_string();
    }
    let body = rest[level..].trim_end();
    if !body.starts_with(' ') {
        return content.to_string();
    }
    let closed = body.trim_end_matches('#');
    // Only a space-separated run closes a heading; `# c##` keeps its hashes.
    if closed.len() == body.len() || !closed.ends_with(' ') {
        return format!("{indent}{}{body}", "#".repeat(level));
    }
    format!(
        "{indent}{}{}",
        "#".repeat(level),
        closed.trim_end_matches(' ')
    )
}

fn setext_level(content: &str) -> Option<usize> {
    let trimmed = content.trim();
    if !trimmed.is_empty() && trimmed.bytes().all(|byte| byte == b'=') {
        return Some(1);
    }
    // Three or more dashes only: a shorter run is ambiguous with a list marker.
    if trimmed.len() >= 3 && trimmed.bytes().all(|byte| byte == b'-') {
        return Some(2);
    }
    None
}

/// True when `content` is ordinary paragraph text that a Setext underline can
/// turn into a heading.
fn is_setext_candidate(content: &str) -> bool {
    let trimmed = content.trim();
    if trimmed.is_empty() || trimmed.contains('|') {
        return false;
    }
    let first = trimmed.as_bytes()[0];
    !matches!(first, b'#' | b'>' | b'-' | b'*' | b'+' | b'`' | b'~')
        && markdown::ordered_list_prefix_len(trimmed) == 0
}

fn is_table_row(content: &str) -> bool {
    let trimmed = content.trim();
    trimmed.starts_with('|') && trimmed.len() > 1
}

fn is_table_separator(content: &str) -> bool {
    let cells = markdown::parse_table_cells_with_ranges(content);
    !cells.is_empty()
        && cells.iter().all(|(cell, _)| {
            let marker = cell.trim();
            let marker = marker.strip_prefix(':').unwrap_or(marker);
            let marker = marker.strip_suffix(':').unwrap_or(marker);
            !marker.is_empty() && marker.bytes().all(|byte| byte == b'-')
        })
}

/// One open list level, used to rebuild nesting indents from the parent's
/// marker width the way `docs/help-*.md` are written.
struct ListLevel {
    source_indent: usize,
    output_indent: usize,
    marker_width: usize,
    indent_delta: isize,
    next_number: u32,
}

struct ListItem {
    indent: usize,
    number: Option<u32>,
    body: String,
}

fn parse_list_item(content: &str) -> Option<ListItem> {
    if is_thematic_break(content) {
        return None;
    }
    let indent = content.len() - content.trim_start_matches([' ', '\t']).len();
    let rest = &content[indent..];

    if let Some(marker) = rest.chars().next()
        && matches!(marker, '-' | '*' | '+')
    {
        let after = &rest[1..];
        if after.is_empty() || after.starts_with([' ', '\t']) {
            return Some(ListItem {
                indent,
                number: None,
                body: after.trim_start_matches([' ', '\t']).to_string(),
            });
        }
    }

    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0 && digits <= 9 && matches!(rest.as_bytes().get(digits), Some(b'.') | Some(b')')) {
        let after = &rest[digits + 1..];
        if after.is_empty() || after.starts_with([' ', '\t']) {
            return Some(ListItem {
                indent,
                number: rest[..digits].parse().ok(),
                body: after.trim_start_matches([' ', '\t']).to_string(),
            });
        }
    }

    None
}

fn is_atx_heading(line: &str) -> bool {
    let trimmed = line.trim_start_matches(' ');
    let level = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    (1..=6).contains(&level) && trimmed[level..].starts_with(' ')
}

/// Normalizes `source`; returns `None` when nothing changed.
pub(crate) fn format_markdown(source: &str) -> Option<String> {
    let front_matter_len = front_matter_len(source);
    let (front_matter, body) = source.split_at(front_matter_len);

    let lines: Vec<&str> = body.split('\n').collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut open_fence: Option<(char, usize)> = None;
    let mut in_math = false;
    let mut list_stack: Vec<ListLevel> = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let raw = lines[index];
        let quote_len = markdown::blockquote_prefix_len(raw);
        let (quote, content) = raw.split_at(quote_len);

        // Code and math blocks are copied byte for byte.
        if let Some((marker, run)) = open_fence {
            out.push(raw.to_string());
            if is_fence_closer(content, marker, run) {
                open_fence = None;
            }
            index += 1;
            continue;
        }
        if in_math {
            out.push(raw.to_string());
            if content.trim() == "$$" {
                in_math = false;
            }
            index += 1;
            continue;
        }
        if let Some(fence) = fence_opener(content) {
            open_fence = Some(fence);
            out.push(raw.to_string());
            index += 1;
            continue;
        }
        if content.trim() == "$$" {
            in_math = true;
            out.push(raw.to_string());
            index += 1;
            continue;
        }

        if quote_len == 0
            && is_table_row(content)
            && lines
                .get(index + 1)
                .is_some_and(|next| is_table_separator(next))
        {
            let mut end = index + 2;
            while end < lines.len() && is_table_row(lines[end]) {
                end += 1;
            }
            let block: Vec<String> = lines[index..end].iter().map(|l| l.to_string()).collect();
            out.extend(crate::table_ops::serialize_table(
                &crate::table_ops::parse_table_block(&block),
            ));
            index = end;
            continue;
        }

        // Blank lines stay put: a loose list keeps its levels open across them.
        if content.trim().is_empty() {
            out.push(String::new());
            index += 1;
            continue;
        }

        let source_indent = content.len() - content.trim_start_matches([' ', '\t']).len();

        // Outside any list a deep indent is an indented code block.
        if list_stack.is_empty() && source_indent >= 4 {
            out.push(raw.to_string());
            index += 1;
            continue;
        }

        if let Some(item) = parse_list_item(content) {
            while list_stack
                .last()
                .is_some_and(|level| item.indent < level.source_indent)
            {
                list_stack.pop();
            }

            let deeper = list_stack
                .last()
                .is_some_and(|level| item.indent > level.source_indent);
            if deeper || list_stack.is_empty() {
                let output_indent = list_stack
                    .last()
                    .map(|level| level.output_indent + level.marker_width)
                    .unwrap_or(0);
                list_stack.push(ListLevel {
                    source_indent: item.indent,
                    output_indent,
                    marker_width: 0,
                    indent_delta: output_indent as isize - item.indent as isize,
                    next_number: item.number.unwrap_or(1),
                });
            }

            let level = list_stack.last_mut().expect("a level was just ensured");
            let indent = " ".repeat(level.output_indent);
            let rendered = match item.number {
                Some(_) => {
                    let number = level.next_number;
                    level.next_number = number.saturating_add(1);
                    level.marker_width = number.to_string().len() + 2;
                    if item.body.is_empty() {
                        format!("{indent}{number}.")
                    } else {
                        format!("{indent}{number}. {}", item.body)
                    }
                }
                None => {
                    level.marker_width = 2;
                    if item.body.is_empty() {
                        format!("{indent}-")
                    } else {
                        format!("{indent}- {}", item.body)
                    }
                }
            };
            out.push(format!("{quote}{}", trim_trailing(&rendered)));
            index += 1;
            continue;
        }

        // A more-indented plain line continues the current item; shift it by the
        // same amount so it stays attached after re-indenting.
        if let Some(level) = list_stack.last()
            && source_indent > level.source_indent
        {
            let shifted = (source_indent as isize + level.indent_delta).max(0) as usize;
            let normalized = format!("{}{}", " ".repeat(shifted), content.trim_start());
            out.push(format!("{quote}{}", trim_trailing(&normalized)));
            index += 1;
            continue;
        }
        list_stack.clear();

        let underline_level = lines.get(index + 1).and_then(|next| {
            let next_quote = markdown::blockquote_prefix_len(next);
            if next_quote != quote_len {
                return None;
            }
            setext_level(&next[next_quote..])
        });
        if let Some(level) = underline_level
            && is_setext_candidate(content)
        {
            out.push(format!("{quote}{} {}", "#".repeat(level), content.trim()));
            index += 2;
            continue;
        }

        if is_thematic_break(content) {
            out.push(format!("{quote}***"));
            index += 1;
            continue;
        }

        let normalized = normalize_atx_heading(content);
        out.push(format!("{quote}{}", trim_trailing(&normalized)));
        index += 1;
    }

    // Collapse blank runs, then close the gaps around headings the way the
    // bundled help documents are written.
    let mut packed: Vec<String> = Vec::with_capacity(out.len());
    for line in out {
        if line.trim().is_empty() && packed.last().is_some_and(|last| last.trim().is_empty()) {
            continue;
        }
        packed.push(line);
    }
    let mut result = String::with_capacity(front_matter.len() + body.len());
    result.push_str(front_matter);
    for (position, line) in packed.iter().enumerate() {
        if line.trim().is_empty() {
            let after_heading = position
                .checked_sub(1)
                .and_then(|previous| packed.get(previous))
                .is_some_and(|previous| is_atx_heading(previous));
            let before_heading = packed
                .get(position + 1)
                .is_some_and(|next| is_atx_heading(next));
            // Leading blanks would also separate the front matter from the body.
            if position == 0 || after_heading || before_heading {
                continue;
            }
        }
        result.push_str(line);
        result.push('\n');
    }
    while result.ends_with("\n\n") {
        result.pop();
    }
    if result.is_empty() {
        result.push('\n');
    }

    (result != source).then_some(result)
}

#[cfg(test)]
mod tests {
    use super::format_markdown;

    #[test]
    fn list_markers_and_ordered_delimiters_are_normalized() {
        let formatted = format_markdown("* one\n+ two\n\n1) first\n2) second\n").unwrap();
        assert_eq!(formatted, "- one\n- two\n\n1. first\n2. second\n");
    }

    #[test]
    fn setext_headings_become_atx() {
        let formatted = format_markdown("Title\n=====\n\nSub\n---\n").unwrap();
        assert_eq!(formatted, "# Title\n## Sub\n");
    }

    #[test]
    fn headings_are_packed_against_their_content() {
        let formatted = format_markdown("# Title\n\n\n正文\n\n## Sub\n\n- one\n").unwrap();
        assert_eq!(formatted, "# Title\n正文\n## Sub\n- one\n");
    }

    #[test]
    fn nested_lists_are_reindented_to_the_parent_marker_width() {
        let source = concat!(
            "- top\n",
            "    * child\n",
            "        + grandchild\n",
            "1. first\n",
            "     1. nested\n",
            "     1. sibling\n",
        );
        let formatted = format_markdown(source).unwrap();
        assert_eq!(
            formatted,
            concat!(
                "- top\n",
                "  - child\n",
                "    - grandchild\n",
                "1. first\n",
                "   1. nested\n",
                "   2. sibling\n",
            )
        );
    }

    #[test]
    fn task_lists_keep_their_checkbox_and_nesting() {
        let formatted = format_markdown("* [x] done\n   * [ ] todo\n").unwrap();
        assert_eq!(formatted, "- [x] done\n  - [ ] todo\n");
    }

    #[test]
    fn item_continuation_lines_follow_their_marker() {
        let formatted = format_markdown("- top\n    * child\n      wrapped\n").unwrap();
        assert_eq!(formatted, "- top\n  - child\n    wrapped\n");
    }

    #[test]
    fn thematic_breaks_use_asterisks() {
        let formatted = format_markdown("text\n\n___\n\nmore\n").unwrap();
        assert_eq!(formatted, "text\n\n***\n\nmore\n");
    }

    #[test]
    fn indented_code_blocks_are_left_alone() {
        assert_eq!(format_markdown("text\n\n    - not a list\n"), None);
    }

    /// `docs/help-zh-cn.md` is the reference for this house style, so a
    /// formatting pass over it has to be a no-op.
    #[test]
    fn the_reference_help_document_is_already_in_house_style() {
        assert_eq!(format_markdown(include_str!("../docs/help-zh-cn.md")), None);
    }

    #[test]
    fn editor_specific_inline_syntax_is_left_alone() {
        let source = concat!(
            "> [!NOTE]\n",
            "> 提示内容\n",
            "\n",
            "参见 [[某页面]] 与 ==高亮== 和 H~2~O、x^2^。\n",
            "\n",
            "脚注[^1]\n",
            "\n",
            "[^1]: 注解\n",
        );
        assert_eq!(format_markdown(source), None);
    }

    #[test]
    fn code_and_math_blocks_are_copied_verbatim() {
        let source = "```mermaid\ngraph TD\n  * A-->B   \n```\n\n$$\n1 * 2\n$$\n";
        assert_eq!(format_markdown(source), None);
    }

    #[test]
    fn quoted_lists_keep_their_quote_prefix() {
        let formatted = format_markdown("> * one\n> * two\n").unwrap();
        assert_eq!(formatted, "> - one\n> - two\n");
    }

    #[test]
    fn thematic_breaks_are_not_treated_as_list_markers() {
        assert_eq!(format_markdown("text\n\n***\n\nmore\n"), None);
    }

    #[test]
    fn closing_hash_runs_and_stray_padding_are_dropped() {
        let formatted = format_markdown("## Title ##\ntrailing   \n").unwrap();
        assert_eq!(formatted, "## Title\ntrailing  \n");
    }

    #[test]
    fn blank_runs_collapse_and_the_file_ends_with_one_newline() {
        let formatted = format_markdown("a\n\n\n\nb\n\n\n").unwrap();
        assert_eq!(formatted, "a\n\nb\n");
    }

    #[test]
    fn front_matter_is_preserved_verbatim() {
        let source = "---\ntitle:  Draft\n---\n\n* item\n";
        let formatted = format_markdown(source).unwrap();
        assert_eq!(formatted, "---\ntitle:  Draft\n---\n- item\n");
    }

    #[test]
    fn tables_are_realigned() {
        let formatted = format_markdown("|a|b|\n|-|-|\n|1|2|\n").unwrap();
        assert_eq!(formatted, "| a | b |\n| --- | --- |\n| 1 | 2 |\n");
    }

    #[test]
    fn already_normalized_text_reports_no_change() {
        assert!(format_markdown("# Title\n- one\n- two\n").is_none());
    }
}
