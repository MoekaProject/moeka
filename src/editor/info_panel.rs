use std::fs;
use std::mem::size_of;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::SystemTime;

use gpui::{
    AnyElement, Context, CursorStyle, FontWeight, MouseButton, SharedString, div, prelude::*, px,
    svg,
};
#[cfg(not(target_os = "macos"))]
use printpdf::{Mm, PdfDocument};
use pulldown_cmark::{
    BlockQuoteKind, CodeBlockKind, CowStr, Event, LinkType, Options, Parser, Tag, TagEnd,
    html::push_html,
};
use similar::{Algorithm, ChangeTag, InlineChange, TextDiff};
use unicode_segmentation::UnicodeSegmentation;

#[cfg(target_os = "macos")]
use objc::{class, msg_send, sel, sel_impl};

#[cfg(target_os = "macos")]
#[link(name = "WebKit", kind = "framework")]
unsafe extern "C" {}

use super::*;

#[derive(Clone)]
struct PendingGitDiffRow {
    kind: GitDiffRowKind,
    text: String,
    inline_highlights: Vec<Range<usize>>,
    old_line_index: Option<usize>,
    new_line_index: Option<usize>,
}

#[derive(Clone)]
struct InfoPanelRow {
    label: String,
    value: String,
}

#[derive(Clone)]
pub(crate) struct InfoPanelData {
    primary_rows: Vec<InfoPanelRow>,
    attachment_items: Vec<InfoPanelReferenceItem>,
    image_items: Vec<InfoPanelReferenceItem>,
    external_link_items: Vec<InfoPanelReferenceItem>,
    wiki_link_items: Vec<InfoPanelReferenceItem>,
    supports_text_export: bool,
}

#[derive(Clone)]
struct InfoPanelReferenceItem {
    label: String,
    source_line: usize,
    share_action: InfoPanelShareAction,
}

#[derive(Clone)]
enum InfoPanelShareAction {
    RevealPath(PathBuf),
    OpenUrl(String),
}

#[derive(Clone)]
enum InfoPanelReferenceTarget {
    LocalPath(PathBuf),
    ExternalUrl(String),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PendingPanelReferenceKind {
    Link,
    Image,
}

struct PendingPanelReference {
    kind: PendingPanelReferenceKind,
    target: String,
    label: String,
    source_line: usize,
}

#[derive(Clone)]
pub(crate) struct GitHistoryItem {
    commit_hash: Option<String>,
    short_hash: String,
    message: String,
    time_label: String,
    is_working_tree: bool,
}

pub(super) struct GitRepoContext {
    pub(super) repo_root: PathBuf,
    pub(super) file_path: PathBuf,
    pub(super) relative_path: String,
}

pub(super) fn git_repo_context_for_file(file_path: &Path) -> Option<GitRepoContext> {
    let file_dir = file_path.parent()?;
    let output = Command::new("git")
        .arg("-C")
        .arg(file_dir)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    let repo_root = PathBuf::from(String::from_utf8_lossy(&output.stdout).trim().to_string());
    let relative_path = file_path
        .strip_prefix(&repo_root)
        .ok()?
        .to_string_lossy()
        .replace('\\', "/");
    Some(GitRepoContext {
        repo_root,
        file_path: file_path.to_path_buf(),
        relative_path,
    })
}

fn collect_git_history_items_for_context(
    language: crate::i18n::UiLanguage,
    context: &GitRepoContext,
) -> Option<Vec<GitHistoryItem>> {
    let mut items = Vec::new();
    let status_output = run_git_command(
        &context.repo_root,
        &[
            "status",
            "--porcelain=v1",
            "--",
            context.relative_path.as_str(),
        ],
    )
    .unwrap_or_default();

    if !status_output.trim().is_empty() {
        items.push(GitHistoryItem {
            commit_hash: None,
            short_hash: crate::i18n::translate(language, crate::i18n::TextKey::WorkingTree)
                .to_string(),
            message: crate::i18n::translate(language, crate::i18n::TextKey::UncommittedChanges)
                .to_string(),
            time_label: crate::i18n::translate(language, crate::i18n::TextKey::Current).to_string(),
            is_working_tree: true,
        });
    }

    let log_output = run_git_command(
        &context.repo_root,
        &[
            "log",
            "--follow",
            "--format=%H%x1f%s%x1f%ct",
            "--",
            context.relative_path.as_str(),
        ],
    )
    .unwrap_or_default();

    for line in log_output.lines() {
        let mut parts = line.split('\x1f');
        let Some(commit_hash) = parts.next() else {
            continue;
        };
        let Some(message) = parts.next() else {
            continue;
        };
        let Some(timestamp) = parts.next() else {
            continue;
        };
        let short_hash = commit_hash.chars().take(8).collect::<String>();
        let time_label = timestamp
            .parse::<i64>()
            .map(|value| crate::i18n::format_unix_timestamp_relative(language, value))
            .unwrap_or_else(|_| crate::i18n::format_relative_time(language, SystemTime::now()));
        items.push(GitHistoryItem {
            commit_hash: Some(commit_hash.to_string()),
            short_hash,
            message: message.to_string(),
            time_label,
            is_working_tree: false,
        });
    }

    Some(items)
}

fn collect_git_history_items_for_file(
    language: crate::i18n::UiLanguage,
    file_path: &Path,
) -> Option<Vec<GitHistoryItem>> {
    let context = git_repo_context_for_file(file_path)?;
    collect_git_history_items_for_context(language, &context)
}

fn format_file_size(size: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;
    const GB: f64 = MB * 1024.0;

    let size = size as f64;
    if size >= GB {
        format!("{:.1} GB", size / GB)
    } else if size >= MB {
        format!("{:.1} MB", size / MB)
    } else if size >= KB {
        format!("{:.1} KB", size / KB)
    } else {
        format!("{} B", size as u64)
    }
}

fn export_dialog_path(default_name: &str, title: &str) -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .set_title(title)
        .set_file_name(default_name)
        .save_file()
}

fn html_escape(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

fn percent_encode_file_url_path(path: &Path) -> String {
    let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let path_text = absolute.to_string_lossy();
    let mut encoded = String::with_capacity(path_text.len() + 8);

    for byte in path_text.as_bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                encoded.push(*byte as char)
            }
            _ => {
                use std::fmt::Write;
                let _ = write!(encoded, "%{byte:02X}");
            }
        }
    }

    if !encoded.ends_with('/') {
        encoded.push('/');
    }

    encoded
}

fn html_base_href(source_file_path: Option<&Path>) -> Option<String> {
    let directory = source_file_path?.parent()?;
    Some(format!(
        "file://{}",
        percent_encode_file_url_path(directory)
    ))
}

fn export_preview_font_families() -> helpers::GeneratedPreviewFontFamilies<'static> {
    helpers::GeneratedPreviewFontFamilies {
        monospace: "SFMono-Regular, Consolas, Liberation Mono, Menlo, monospace",
        body_cjk: "PingFang SC, Hiragino Sans GB, Microsoft YaHei, Noto Sans CJK SC, sans-serif",
    }
}

fn render_export_preview_svg(kind: crate::markdown::BlockPreviewKind, source: &str) -> String {
    let theme = crate::theme::ThemeKind::OneLight.build();
    helpers::build_generated_block_preview_svg(kind, source, &theme, export_preview_font_families())
}

fn export_code_block_preview_html(kind: &CodeBlockKind<'_>, source: &str) -> Option<String> {
    let language = match kind {
        CodeBlockKind::Fenced(info) => info
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .trim()
            .to_ascii_lowercase(),
        CodeBlockKind::Indented => String::new(),
    };

    let preview_kind = match language.as_str() {
        "mermaid" | "mmd" => crate::markdown::BlockPreviewKind::Mermaid,
        "math" | "latex" | "tex" => crate::markdown::BlockPreviewKind::Math,
        _ => return None,
    };

    let svg = render_export_preview_svg(preview_kind, source);
    Some(format!(
        "<figure class=\"export-preview export-preview-{}\">{}</figure>",
        match preview_kind {
            crate::markdown::BlockPreviewKind::Math => "math",
            crate::markdown::BlockPreviewKind::Mermaid => "mermaid",
        },
        svg
    ))
}

/// Syntax-highlights a fenced code block for export using the same syntect
/// syntaxes the editor uses, emitting inline-styled HTML so the PDF is
/// self-contained. Uses a bundled light theme (independent of the app theme so
/// dark mode does not produce unreadable code on the light page). Returns
/// `None` when the language is unknown so the caller falls back to a plain
/// `<pre><code>` block.
fn highlight_code_block_html(kind: &CodeBlockKind<'_>, source: &str) -> Option<String> {
    let language = match kind {
        CodeBlockKind::Fenced(info) => info.split_whitespace().next().unwrap_or_default().trim(),
        CodeBlockKind::Indented => return None,
    };
    if language.is_empty() {
        return None;
    }

    let syntax = helpers::find_syntax_for_language(language)?;
    let syntax_set = helpers::syntax_set();

    static EXPORT_THEME: std::sync::OnceLock<Option<syntect::highlighting::Theme>> =
        std::sync::OnceLock::new();
    let theme = EXPORT_THEME
        .get_or_init(|| {
            syntect::highlighting::ThemeSet::load_defaults()
                .themes
                .remove("InspiredGitHub")
        })
        .as_ref()?;

    let source = source.strip_suffix('\n').unwrap_or(source);
    syntect::html::highlighted_html_for_string(source, syntax_set, syntax, theme).ok()
}

fn escape_export_html_text(text: &str, out: &mut String) {
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
}

fn try_export_paired_syntax(
    text: &str,
    open: usize,
    delimiter: &str,
    tag: &str,
    out: &mut String,
) -> Option<usize> {
    let inner_start = open + delimiter.len();
    let rest = text.get(inner_start..)?;
    let close_rel = rest.find(delimiter)?;
    if close_rel == 0 {
        return None;
    }
    let inner = &rest[..close_rel];
    if inner.contains('\n') {
        return None;
    }
    out.push('<');
    out.push_str(tag);
    out.push('>');
    escape_export_html_text(inner, out);
    out.push_str("</");
    out.push_str(tag);
    out.push('>');
    Some(inner_start + close_rel + delimiter.len())
}

/// Attempts to convert a `:shortcode:` emoji starting at `open` (the `:`) into
/// its emoji character, matching the editor's `emojis` lookup. Returns the byte
/// index just past the closing `:` on success.
fn try_export_emoji(text: &str, open: usize, out: &mut String) -> Option<usize> {
    let inner_start = open + 1;
    let rest = text.get(inner_start..)?;
    let mut end_rel = 0;
    for ch in rest.chars() {
        if ch == ':' {
            break;
        }
        if !(ch.is_ascii_alphanumeric() || matches!(ch, '_' | '+' | '-')) {
            return None;
        }
        end_rel += ch.len_utf8();
    }
    if rest.get(end_rel..).and_then(|r| r.chars().next()) != Some(':') || end_rel == 0 {
        return None;
    }
    let emoji = emojis::get_by_shortcode(&rest[..end_rel])?;
    out.push_str(emoji.as_str());
    Some(inner_start + end_rel + 1)
}

/// Converts the editor's inline syntaxes that pulldown-cmark does not emit on
/// its own (or that it fails to form when extensions conflict) into inline
/// HTML, escaping the surrounding literal text: `==highlight==`,
/// `++underline++`, `^superscript^`, `~subscript~`, `~~strikethrough~~` and
/// `:emoji:` shortcodes. Returns `None` when none are present so the caller
/// keeps pulldown's default (escaped) text handling.
fn convert_inline_custom_syntax(text: &str) -> Option<String> {
    if !text.contains("==")
        && !text.contains("++")
        && !text.contains('^')
        && !text.contains('~')
        && !text.contains(':')
    {
        return None;
    }

    let mut out = String::with_capacity(text.len() + 16);
    let mut index = 0;
    let mut changed = false;

    while index < text.len() {
        let two_char = text.get(index..index + 2);
        if two_char == Some("==")
            && let Some(next) = try_export_paired_syntax(text, index, "==", "mark", &mut out)
        {
            changed = true;
            index = next;
            continue;
        }
        if two_char == Some("++")
            && let Some(next) = try_export_paired_syntax(text, index, "++", "u", &mut out)
        {
            changed = true;
            index = next;
            continue;
        }
        if two_char == Some("~~")
            && let Some(next) = try_export_paired_syntax(text, index, "~~", "del", &mut out)
        {
            changed = true;
            index = next;
            continue;
        }

        let ch = text[index..].chars().next().unwrap_or('\u{0}');
        if ch == '^'
            && let Some(next) = try_export_paired_syntax(text, index, "^", "sup", &mut out)
        {
            changed = true;
            index = next;
            continue;
        }
        if ch == '~'
            && let Some(next) = try_export_paired_syntax(text, index, "~", "sub", &mut out)
        {
            changed = true;
            index = next;
            continue;
        }
        if ch == ':'
            && let Some(next) = try_export_emoji(text, index, &mut out)
        {
            changed = true;
            index = next;
            continue;
        }

        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
        index += ch.len_utf8();
    }

    changed.then_some(out)
}

const CALLOUT_NOTE_SVG: &str = include_str!("../../assets/icons/format/callout-note.svg");
const CALLOUT_TIP_SVG: &str = include_str!("../../assets/icons/format/callout-tip.svg");
const CALLOUT_IMPORTANT_SVG: &str = include_str!("../../assets/icons/format/callout-important.svg");
const CALLOUT_WARNING_SVG: &str = include_str!("../../assets/icons/format/callout-warning.svg");
const CALLOUT_CAUTION_SVG: &str = include_str!("../../assets/icons/format/callout-caution.svg");

/// Renders an editor-style callout blockquote (`> [!KIND] optional title`) as a
/// styled `<blockquote>` carrying the matching icon and title. `inner` is the
/// event stream between the blockquote's start and end tags (exclusive).
/// Returns `None` when the blockquote is not a callout so the caller renders it
/// as a normal blockquote.
fn callout_assets(kind: BlockQuoteKind) -> (&'static str, &'static str, &'static str) {
    match kind {
        BlockQuoteKind::Note => ("note", "Note", CALLOUT_NOTE_SVG),
        BlockQuoteKind::Tip => ("tip", "Tip", CALLOUT_TIP_SVG),
        BlockQuoteKind::Important => ("important", "Important", CALLOUT_IMPORTANT_SVG),
        BlockQuoteKind::Warning => ("warning", "Warning", CALLOUT_WARNING_SVG),
        BlockQuoteKind::Caution => ("caution", "Caution", CALLOUT_CAUTION_SVG),
    }
}

/// Renders a callout blockquote with the given kind, optional title and body
/// events. An empty `title` falls back to the kind's label.
fn build_callout_html(kind: BlockQuoteKind, title: &str, body_events: Vec<Event<'_>>) -> String {
    let (kind_class, kind_label, icon) = callout_assets(kind);
    let icon = icon.replacen(
        "<svg ",
        "<svg viewBox=\"0 0 24 24\" aria-hidden=\"true\" ",
        1,
    );

    let mut body_html = String::new();
    push_html(
        &mut body_html,
        transform_export_events(body_events.into_iter()).into_iter(),
    );

    let mut title_html = String::new();
    if title.is_empty() {
        title_html.push_str(kind_label);
    } else {
        escape_export_html_text(title, &mut title_html);
    }

    format!(
        "<blockquote class=\"callout callout-{kind_class}\">\
         <p class=\"callout-title\">{icon}<span>{title_html}</span></p>\
         {body_html}</blockquote>"
    )
}

/// Detects the editor's same-line callout syntax (`> [!KIND] optional title`),
/// which pulldown leaves as a plain blockquote because the title shares the
/// marker line. `inner` is the event stream between the blockquote tags
/// (exclusive). Returns `None` when it is not such a callout.
fn try_render_callout(inner: &[Event<'_>]) -> Option<String> {
    if !matches!(inner.first(), Some(Event::Start(Tag::Paragraph))) {
        return None;
    }

    // pulldown fragments `[!KIND]` across several text events (a leading `[`
    // looks like a reference link), so reconstruct the blockquote's first line
    // by concatenating its inline text up to the first line break.
    let para_end = inner
        .iter()
        .position(|event| matches!(event, Event::End(TagEnd::Paragraph)))?;
    let mut first_line = String::new();
    let mut first_line_end = para_end;
    for (index, event) in inner.iter().enumerate().take(para_end).skip(1) {
        match event {
            Event::Text(text) | Event::Code(text) => first_line.push_str(text),
            Event::SoftBreak | Event::HardBreak => {
                first_line_end = index;
                break;
            }
            _ => {}
        }
    }

    let rest = first_line.trim_start().strip_prefix("[!")?;
    let close = rest.find(']')?;
    let kind = match rest[..close].to_ascii_uppercase().as_str() {
        "NOTE" => BlockQuoteKind::Note,
        "TIP" => BlockQuoteKind::Tip,
        "IMPORTANT" => BlockQuoteKind::Important,
        "WARNING" => BlockQuoteKind::Warning,
        "CAUTION" => BlockQuoteKind::Caution,
        _ => return None,
    };
    let title = rest[close + 1..].trim();

    // The body is the remainder of the marker paragraph (after its first line
    // break) plus any later blocks in the blockquote.
    let first_body = if matches!(
        inner.get(first_line_end),
        Some(Event::SoftBreak | Event::HardBreak)
    ) {
        &inner[first_line_end + 1..para_end]
    } else {
        &[][..]
    };

    let mut body_events: Vec<Event<'_>> = Vec::new();
    if !first_body.is_empty() {
        body_events.push(Event::Start(Tag::Paragraph));
        body_events.extend(first_body.iter().cloned());
        body_events.push(Event::End(TagEnd::Paragraph));
    }
    body_events.extend(inner[para_end + 1..].iter().cloned());

    Some(build_callout_html(kind, title, body_events))
}

/// Walks pulldown's event stream and rewrites the constructs the export needs to
/// render itself: fenced code-block previews (mermaid/math), math, the editor's
/// custom inline syntaxes, and `[!KIND]` callouts. Blockquotes are buffered so a
/// callout can be detected from their first line; the function recurses for
/// blockquote bodies so nested constructs are handled too.
fn transform_export_events<'a>(parser: impl Iterator<Item = Event<'a>>) -> Vec<Event<'a>> {
    let mut events: Vec<Event<'a>> = Vec::new();
    let mut pending_code_block: Option<(CodeBlockKind<'a>, Vec<Event<'a>>, String)> = None;
    let mut pending_blockquote: Option<(usize, Option<BlockQuoteKind>, Vec<Event<'a>>)> = None;

    for event in parser {
        if let Some((depth, kind, mut buffer)) = pending_blockquote.take() {
            match &event {
                Event::Start(Tag::BlockQuote(_)) => {
                    buffer.push(event);
                    pending_blockquote = Some((depth + 1, kind, buffer));
                }
                Event::End(TagEnd::BlockQuote(_)) if depth == 1 => {
                    if let Some(callout_html) = try_render_callout(&buffer) {
                        events.push(Event::Html(CowStr::Boxed(callout_html.into_boxed_str())));
                    } else if let Some(alert_kind) = kind {
                        // A GitHub-style alert (`> [!KIND]` on its own line):
                        // pulldown already parsed the kind and stripped the
                        // marker, so render the buffered body as a callout.
                        let callout_html = build_callout_html(alert_kind, "", buffer);
                        events.push(Event::Html(CowStr::Boxed(callout_html.into_boxed_str())));
                    } else {
                        events.push(Event::Start(Tag::BlockQuote(kind)));
                        events.extend(transform_export_events(buffer.into_iter()));
                        events.push(Event::End(TagEnd::BlockQuote(kind)));
                    }
                }
                Event::End(TagEnd::BlockQuote(_)) => {
                    buffer.push(event);
                    pending_blockquote = Some((depth - 1, kind, buffer));
                }
                _ => {
                    buffer.push(event);
                    pending_blockquote = Some((depth, kind, buffer));
                }
            }
            continue;
        }

        if let Some((kind, mut buffered, mut source)) = pending_code_block.take() {
            if let Event::Text(content) = &event {
                source.push_str(content);
            }
            let is_end = matches!(event, Event::End(TagEnd::CodeBlock));
            buffered.push(event);
            if is_end {
                if let Some(preview_html) = export_code_block_preview_html(&kind, &source) {
                    events.push(Event::Html(CowStr::Boxed(preview_html.into_boxed_str())));
                } else if let Some(highlighted) = highlight_code_block_html(&kind, &source) {
                    events.push(Event::Html(CowStr::Boxed(highlighted.into_boxed_str())));
                } else {
                    events.extend(buffered);
                }
            } else {
                pending_code_block = Some((kind, buffered, source));
            }
            continue;
        }

        match event {
            Event::Start(Tag::BlockQuote(kind)) => {
                pending_blockquote = Some((1, kind, Vec::new()));
            }
            Event::Start(Tag::CodeBlock(kind)) => {
                pending_code_block = Some((
                    kind.clone(),
                    vec![Event::Start(Tag::CodeBlock(kind))],
                    String::new(),
                ));
            }
            Event::DisplayMath(content) => {
                let svg =
                    render_export_preview_svg(crate::markdown::BlockPreviewKind::Math, &content);
                events.push(Event::Html(CowStr::Boxed(
                    format!("<figure class=\"export-preview export-preview-math\">{svg}</figure>")
                        .into_boxed_str(),
                )));
            }
            Event::InlineMath(content) => {
                let svg =
                    render_export_preview_svg(crate::markdown::BlockPreviewKind::Math, &content);
                events.push(Event::Html(CowStr::Boxed(
                    format!("<span class=\"export-math-inline\">{svg}</span>").into_boxed_str(),
                )));
            }
            Event::Text(content) => {
                // pulldown emits the editor's custom inline syntaxes (highlight,
                // underline, super/subscript, strikethrough, emoji shortcodes)
                // as plain text; convert them to inline HTML so the export
                // matches the editor.
                if let Some(html) = convert_inline_custom_syntax(&content) {
                    events.push(Event::Html(CowStr::Boxed(html.into_boxed_str())));
                } else {
                    events.push(Event::Text(content));
                }
            }
            _ => events.push(event),
        }
    }

    if let Some((_kind, buffered, _source)) = pending_code_block.take() {
        events.extend(buffered);
    }
    if let Some((_depth, kind, buffer)) = pending_blockquote.take() {
        events.push(Event::Start(Tag::BlockQuote(kind)));
        events.extend(transform_export_events(buffer.into_iter()));
        events.push(Event::End(TagEnd::BlockQuote(kind)));
    }

    events
}

fn render_markdown_for_export(markdown: &str) -> String {
    // Disable pulldown's inline extensions that overlap the editor's custom
    // delimiters. Subscript/strikethrough both use `~` and, with both enabled,
    // pulldown fragments `~x~` into separate text events without forming a
    // subscript. Superscript (`^`) works on its own but we disable it too so
    // every custom inline syntax (`^sup^`, `~sub~`, `~~strike~~`, `==hi==`,
    // `++u++`) is handled uniformly in `convert_inline_custom_syntax`, where
    // these delimiters are plain characters and the text is never split.
    let mut options = Options::all();
    options.remove(Options::ENABLE_SUBSCRIPT);
    options.remove(Options::ENABLE_SUPERSCRIPT);
    options.remove(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(markdown, options);
    let mut html = String::new();
    push_html(&mut html, transform_export_events(parser).into_iter());
    html
}

fn build_html_document(title: &str, markdown: &str, source_file_path: Option<&Path>) -> String {
    let html = render_markdown_for_export(markdown);

    let escaped_title = html_escape(title);
    let base_href = html_base_href(source_file_path)
        .map(|href| format!("<base href=\"{}\">", html_escape(&href)))
        .unwrap_or_default();

    let github_markdown_css = concat!(
        ".markdown-body{box-sizing:border-box;min-width:200px;max-width:980px;margin:0 auto;padding:45px;color:#1f2328;background:#fff;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI','Noto Sans',Helvetica,Arial,sans-serif,'Apple Color Emoji','Segoe UI Emoji';font-size:16px;line-height:1.5;word-wrap:break-word;}",
        ".markdown-body h1,.markdown-body h2{padding-bottom:.3em;border-bottom:1px solid #d8dee4;}",
        ".markdown-body h1{margin:.67em 0 16px;font-size:2em;}",
        ".markdown-body h2{margin:24px 0 16px;font-size:1.5em;}",
        ".markdown-body h3{margin:24px 0 16px;font-size:1.25em;}",
        ".markdown-body h4{margin:24px 0 16px;font-size:1em;}",
        ".markdown-body h5{margin:24px 0 16px;font-size:.875em;}",
        ".markdown-body h6{margin:24px 0 16px;color:#57606a;font-size:.85em;}",
        ".markdown-body p,.markdown-body blockquote,.markdown-body ul,.markdown-body ol,.markdown-body dl,.markdown-body table,.markdown-body pre,.markdown-body details{margin-top:0;margin-bottom:16px;}",
        ".markdown-body ul,.markdown-body ol{padding-left:1.5em;}",
        ".markdown-body>ul:has(>li>input[type=checkbox]){padding-left:0;}",
        ".markdown-body li:has(>input[type=checkbox]){list-style:none;}.markdown-body li>input[type=checkbox]{-webkit-appearance:none;appearance:none;box-sizing:border-box;display:inline-block;width:1.14em;height:1.14em;margin:0 .5em 0 0;border:1.5px solid #8c959f;border-radius:.25em;background:#fff;vertical-align:-.05em;}",
        ".markdown-body li>input[type=checkbox]:checked{border-color:#0969da;background:#0969da url(data:image/svg+xml,%3Csvg%20xmlns='http://www.w3.org/2000/svg'%20viewBox='0%200%2016%2016'%3E%3Cpath%20fill='none'%20stroke='white'%20stroke-width='2.2'%20stroke-linecap='round'%20stroke-linejoin='round'%20d='M3.5%208.5l3%203%206-7'/%3E%3C/svg%3E) center/85% 85% no-repeat;}",
        ".markdown-body a{color:#0969da;text-decoration:none;}.markdown-body a:hover{text-decoration:underline;}",
        ".markdown-body blockquote{margin-left:0;margin-right:0;padding:0 1em;color:#57606a;border-left:.25em solid #d0d7de;}",
        ".markdown-body code,.markdown-body tt{padding:.2em .4em;margin:0;font-size:85%;white-space:break-spaces;background-color:rgba(175,184,193,.2);border-radius:6px;}",
        ".markdown-body pre{padding:16px;overflow:auto;font-size:85%;line-height:1.45;background-color:#f6f8fa;border-radius:6px;}",
        ".markdown-body pre code{padding:0;margin:0;font-size:100%;white-space:pre;background:transparent;border:0;}",
        ".markdown-body pre{font-family:ui-monospace,SFMono-Regular,'SF Mono',Menlo,Consolas,'Liberation Mono',monospace;border:1px solid #d0d7de;}",
        ".markdown-body table{display:table;width:100%;border-spacing:0;border-collapse:collapse;}",
        ".markdown-body table th{font-weight:600;}.markdown-body table th,.markdown-body table td{padding:6px 13px;border:1px solid #d0d7de;}",
        ".markdown-body table tr{background-color:#fff;border-top:1px solid #d8dee4;}.markdown-body table tr:nth-child(2n){background-color:#f6f8fa;}",
        ".markdown-body img{box-sizing:content-box;max-width:100%;height:auto;background-color:#fff;}",
        ".markdown-body hr{height:.25em;padding:0;margin:24px 0;background-color:#d0d7de;border:0;}",
        ".markdown-body kbd{display:inline-block;padding:3px 5px;font:11px ui-monospace,SFMono-Regular,SF Mono,Menlo,Consolas,Liberation Mono,monospace;line-height:10px;color:#24292f;vertical-align:middle;background-color:#f6f8fa;border:solid 1px rgba(175,184,193,.2);border-bottom-color:rgba(175,184,193,.2);border-radius:6px;box-shadow:inset 0 -1px 0 rgba(175,184,193,.2);}",
        ".export-preview{margin:16px 0;padding:12px;overflow:auto;text-align:center;background:#fff;border:1px solid #d0d7de;border-radius:6px;}",
        ".export-preview svg{max-width:100%;height:auto;}.export-math-inline{display:inline-flex;max-width:100%;vertical-align:-.25em;}.export-math-inline svg{height:1.35em;width:auto;max-width:100%;}",
        ".markdown-body blockquote.callout{border-left:.25em solid var(--callout-color);background:color-mix(in srgb,var(--callout-color) 8%,transparent);padding:8px 1em;border-radius:0 6px 6px 0;color:#1f2328;}",
        ".markdown-body blockquote.callout>:last-child{margin-bottom:0;}",
        ".markdown-body .callout-title{display:flex;align-items:center;gap:.4em;margin:0 0 8px;font-weight:600;color:var(--callout-color);}",
        ".markdown-body .callout-title svg{flex:0 0 auto;width:.9em;height:.9em;color:var(--callout-color);overflow:visible;}",
        ".markdown-body .callout-note{--callout-color:#0969da;}",
        ".markdown-body .callout-tip{--callout-color:#1a7f37;}",
        ".markdown-body .callout-important{--callout-color:#8250df;}",
        ".markdown-body .callout-warning{--callout-color:#9a6700;}",
        ".markdown-body .callout-caution{--callout-color:#cf222e;}"
    );

    format!(
        concat!(
            "<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\">",
            "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">",
            "<title>{}</title>",
            "{}",
            "<style>",
            "{}",
            "@media(max-width:767px){{.markdown-body{{padding:15px;}}}}",
            "@page{{size:A4;margin:18mm;}}",
            "@media print{{html,body{{background:#fff;}}body{{margin:0;}}.markdown-body{{max-width:none;padding:0;font-size:12pt;line-height:1.55;}}.markdown-body a{{color:inherit;text-decoration:none;}}.markdown-body h1,.markdown-body h2,.markdown-body h3,.markdown-body h4,.markdown-body h5,.markdown-body h6{{break-after:avoid-page;page-break-after:avoid;}}.markdown-body pre,.markdown-body blockquote,.markdown-body table,.markdown-body img,.markdown-body svg,.export-preview{{break-inside:avoid;page-break-inside:avoid;}}.markdown-body pre{{white-space:pre-wrap;word-break:break-word;}}.markdown-body tr{{break-inside:avoid;page-break-inside:avoid;}}.markdown-body p{{orphans:3;widows:3;}}}}",
            "</style></head><body><article class=\"markdown-body\">{}</article></body></html>"
        ),
        escaped_title, base_href, github_markdown_css, html
    )
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
#[repr(C)]
struct NSPoint {
    x: f64,
    y: f64,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
#[repr(C)]
struct NSSize {
    width: f64,
    height: f64,
}

#[cfg(target_os = "macos")]
#[derive(Clone, Copy)]
#[repr(C)]
struct NSRect {
    origin: NSPoint,
    size: NSSize,
}

#[cfg(target_os = "macos")]
fn nsstring(text: &str) -> *mut objc::runtime::Object {
    unsafe {
        let c_string = std::ffi::CString::new(text).expect("NSString input contains interior NUL");
        msg_send![class!(NSString), stringWithUTF8String: c_string.as_ptr()]
    }
}

fn nsurl_from_directory(path: &Path) -> *mut objc::runtime::Object {
    unsafe {
        msg_send![
            class!(NSURL),
            fileURLWithPath: nsstring(&path.to_string_lossy())
            isDirectory: true
        ]
    }
}

#[cfg(target_os = "macos")]
async fn build_pdf_document_from_html_macos(
    html: &str,
    source_file_path: Option<&Path>,
    cx: &mut gpui::AsyncApp,
) -> anyhow::Result<Vec<u8>> {
    unsafe {
        let page_width = 595.0_f64;
        let page_height = 842.0_f64;
        // Lay the web content out at full A4 width (595pt) so the generated PDF
        // page is A4-sized; the `.markdown-body` padding provides the page
        // margins.
        let frame = NSRect {
            origin: NSPoint { x: 0.0, y: 0.0 },
            size: NSSize {
                width: page_width,
                height: page_height,
            },
        };
        let nil = std::ptr::null_mut::<objc::runtime::Object>();
        // WKWebView renders its content out-of-process, so (unlike the legacy
        // WebHTMLView, which laid out but drew nothing here) it produces real PDF
        // output via `createPDFWithConfiguration:` even from a hidden window.
        let configuration: *mut objc::runtime::Object =
            msg_send![class!(WKWebViewConfiguration), new];
        let web_view: *mut objc::runtime::Object = msg_send![class!(WKWebView), alloc];
        let web_view: *mut objc::runtime::Object =
            msg_send![web_view, initWithFrame: frame configuration: configuration];

        // Host the WebView in a window placed ON SCREEN but fully transparent
        // (alpha 0). The legacy WebHTMLView only flows and *draws* its content
        // when its window has a live on-screen graphics context; an offscreen or
        // never-ordered window makes the print operation snapshot a blank page.
        // Alpha 0 + ignoring mouse keeps it invisible and inert to the user.
        let host_window: *mut objc::runtime::Object = msg_send![class!(NSWindow), alloc];
        let host_window: *mut objc::runtime::Object = msg_send![
            host_window,
            initWithContentRect: frame
            styleMask: 0u64
            backing: 2u64
            defer: false
        ];
        let () = msg_send![host_window, setReleasedWhenClosed: false];
        let () = msg_send![host_window, setAlphaValue: 0.0_f64];
        let () = msg_send![host_window, setIgnoresMouseEvents: true];
        let () = msg_send![host_window, setContentView: web_view];
        let on_screen_origin = NSPoint { x: 0.0, y: 0.0 };
        let () = msg_send![host_window, setFrameOrigin: on_screen_origin];
        let () = msg_send![host_window, orderFrontRegardless];

        let base_url = source_file_path
            .and_then(Path::parent)
            .map(nsurl_from_directory)
            .unwrap_or(nil);
        let () = msg_send![web_view, loadHTMLString: nsstring(html) baseURL: base_url];

        let start = std::time::Instant::now();
        loop {
            // Yield to GPUI's real main run loop so WKWebView's out-of-process
            // content can actually load. A nested `runUntilDate` pump does not
            // service the WebContent process, which made loading hang.
            cx.background_executor()
                .timer(std::time::Duration::from_millis(20))
                .await;
            let is_loading: bool = msg_send![web_view, isLoading];
            if !is_loading {
                break;
            }
            if start.elapsed() > std::time::Duration::from_secs(20) {
                let () = msg_send![host_window, close];
                anyhow::bail!("timed out waiting for WebKit HTML rendering");
            }
        }
        // Allow a final layout/paint pass before snapshotting.
        cx.background_executor()
            .timer(std::time::Duration::from_millis(100))
            .await;

        // `createPDFWithConfiguration:completionHandler:` is asynchronous; capture
        // the result in a completion block and pump the run loop until it fires.
        let result: std::rc::Rc<std::cell::RefCell<Option<Result<Vec<u8>, String>>>> =
            std::rc::Rc::new(std::cell::RefCell::new(None));
        let block_result = result.clone();
        let completion = block::ConcreteBlock::new(
            move |pdf_data: *mut objc::runtime::Object, error: *mut objc::runtime::Object| {
                let outcome = if !pdf_data.is_null() {
                    let length: usize = msg_send![pdf_data, length];
                    let ptr: *const u8 = msg_send![pdf_data, bytes];
                    if ptr.is_null() || length == 0 {
                        Err("createPDF returned empty data".to_string())
                    } else {
                        Ok(std::slice::from_raw_parts(ptr, length).to_vec())
                    }
                } else {
                    let message = if error.is_null() {
                        "createPDF returned no data".to_string()
                    } else {
                        let desc: *mut objc::runtime::Object =
                            msg_send![error, localizedDescription];
                        let cstr: *const std::os::raw::c_char = msg_send![desc, UTF8String];
                        if cstr.is_null() {
                            "unknown createPDF error".to_string()
                        } else {
                            std::ffi::CStr::from_ptr(cstr)
                                .to_string_lossy()
                                .into_owned()
                        }
                    };
                    Err(message)
                };
                *block_result.borrow_mut() = Some(outcome);
            },
        );
        let completion = completion.copy();
        let pdf_config: *mut objc::runtime::Object = msg_send![class!(WKPDFConfiguration), new];
        let () = msg_send![
            web_view,
            createPDFWithConfiguration: pdf_config
            completionHandler: &*completion
        ];

        let wait_start = std::time::Instant::now();
        while result.borrow().is_none() {
            if wait_start.elapsed() > std::time::Duration::from_secs(30) {
                break;
            }
            cx.background_executor()
                .timer(std::time::Duration::from_millis(20))
                .await;
        }

        let () = msg_send![host_window, close];

        match result.borrow_mut().take() {
            Some(Ok(bytes)) => Ok(bytes),
            Some(Err(message)) => anyhow::bail!("WKWebView createPDF failed: {message}"),
            None => anyhow::bail!("timed out waiting for WKWebView PDF generation"),
        }
    }
}

fn compact_panel_label(label: &str) -> Option<String> {
    let compact = label.split_whitespace().collect::<Vec<_>>().join(" ");
    (!compact.is_empty()).then_some(compact)
}

fn reference_scheme(target: &str) -> Option<&str> {
    let trimmed = target.trim();
    let scheme_end = trimmed.find(':')?;
    let first_separator = trimmed
        .find(|ch| matches!(ch, '/' | '\\' | '?' | '#'))
        .unwrap_or(trimmed.len());
    if scheme_end > first_separator {
        return None;
    }

    let scheme = &trimmed[..scheme_end];
    if scheme.len() == 1 && scheme.as_bytes()[0].is_ascii_alphabetic() {
        return None;
    }

    let mut chars = scheme.chars();
    let first = chars.next()?;
    if !first.is_ascii_alphabetic() {
        return None;
    }
    chars
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
        .then_some(scheme)
}

fn decode_file_url_path(target: &str) -> String {
    if let Some(stripped) = target.strip_prefix("file://") {
        let mut result = String::with_capacity(stripped.len());
        let bytes = stripped.as_bytes();
        let mut index = 0usize;
        while index < bytes.len() {
            if bytes[index] == b'%' && index + 2 < bytes.len() {
                if let Ok(byte) = u8::from_str_radix(
                    std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or(""),
                    16,
                ) {
                    result.push(byte as char);
                    index += 3;
                    continue;
                }
            }
            result.push(bytes[index] as char);
            index += 1;
        }
        result
    } else {
        target.to_string()
    }
}

fn external_reference_host(target: &str) -> Option<String> {
    let trimmed = target.trim();
    if let Some(rest) = trimmed.strip_prefix("mailto:") {
        return rest
            .rsplit_once('@')
            .map(|(_, domain)| domain.to_string())
            .filter(|domain| !domain.is_empty());
    }

    let authority = trimmed
        .split_once("://")
        .map(|(_, rest)| rest)
        .or_else(|| trimmed.strip_prefix("//"))?;
    let host_port = authority.split(['/', '?', '#']).next().unwrap_or_default();
    let host = host_port
        .rsplit('@')
        .next()
        .unwrap_or_default()
        .trim_matches(['[', ']']);
    if host.is_empty() {
        None
    } else {
        Some(host.split(':').next().unwrap_or(host).to_string())
    }
}

fn path_reference_name(path: &Path, fallback_target: &str) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
        .or_else(|| {
            Path::new(fallback_target)
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| !name.is_empty())
                .map(ToString::to_string)
        })
        .unwrap_or_else(|| fallback_target.to_string())
}

fn fallback_reference_label(
    label: &str,
    raw_target: &str,
    target: &InfoPanelReferenceTarget,
) -> String {
    if let Some(compact) = compact_panel_label(label) {
        return compact;
    }

    match target {
        InfoPanelReferenceTarget::LocalPath(path) => path_reference_name(path, raw_target),
        InfoPanelReferenceTarget::ExternalUrl(url) => external_reference_host(url)
            .filter(|host| !host.is_empty())
            .unwrap_or_else(|| raw_target.to_string()),
    }
}

fn count_document_words(text: &str) -> usize {
    UnicodeSegmentation::unicode_words(text).count()
}

#[cfg(not(target_os = "macos"))]
fn pdf_display_width(text: &str) -> usize {
    text.chars()
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

#[cfg(not(target_os = "macos"))]
fn wrap_text_for_pdf(text: &str, max_units: usize) -> Vec<String> {
    let mut wrapped = Vec::new();

    for raw_line in text.replace('\r', "").lines() {
        let expanded = raw_line.replace('\t', "    ");
        if expanded.is_empty() {
            wrapped.push(String::new());
            continue;
        }

        let mut current = String::new();
        let mut current_width = 0usize;
        for grapheme in UnicodeSegmentation::graphemes(expanded.as_str(), true) {
            let grapheme_width = pdf_display_width(grapheme);
            if current_width + grapheme_width > max_units && !current.is_empty() {
                wrapped.push(current);
                current = String::new();
                current_width = 0;
            }

            current.push_str(grapheme);
            current_width += grapheme_width;
        }

        wrapped.push(current);
    }

    if wrapped.is_empty() {
        wrapped.push(String::new());
    }

    wrapped
}

#[cfg(not(target_os = "macos"))]
fn pdf_font_candidates() -> Vec<PathBuf> {
    let mut candidates = vec![
        PathBuf::from("/System/Library/Fonts/Supplemental/Arial Unicode.ttf"),
        PathBuf::from("/System/Library/Fonts/Supplemental/Arial.ttf"),
        PathBuf::from("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf"),
        PathBuf::from("/usr/share/fonts/truetype/liberation/LiberationSans-Regular.ttf"),
        PathBuf::from("C:/Windows/Fonts/arial.ttf"),
    ];

    if let Ok(home) = std::env::var("HOME") {
        candidates.push(PathBuf::from(home).join("Library/Fonts/Arial Unicode.ttf"));
    }

    candidates
}

#[cfg(not(target_os = "macos"))]
fn find_pdf_font_path() -> Option<PathBuf> {
    pdf_font_candidates().into_iter().find(|path| path.exists())
}

#[cfg(not(target_os = "macos"))]
fn build_pdf_document_with_printpdf(title: &str, markdown: &str) -> anyhow::Result<Vec<u8>> {
    let font_path = find_pdf_font_path()
        .ok_or_else(|| anyhow::anyhow!("could not find a usable system font for PDF export"))?;
    let (document, first_page, first_layer) =
        PdfDocument::new(title, Mm(210.0), Mm(297.0), "Layer 1");
    let font = document.add_external_font(fs::File::open(&font_path)?)?;

    let page_width_mm = 210.0;
    let page_height_mm = 297.0;
    let margin_x_mm = 18.0;
    let margin_y_mm = 18.0;
    let line_height_mm = 6.0;
    let font_size_pt = 11.0;
    let max_units = 60usize;

    let lines = wrap_text_for_pdf(markdown, max_units);
    let mut page = first_page;
    let mut layer = first_layer;
    let mut y = page_height_mm - margin_y_mm;

    for line in lines {
        if y < margin_y_mm {
            let (new_page, new_layer) =
                document.add_page(Mm(page_width_mm), Mm(page_height_mm), "Layer 1");
            page = new_page;
            layer = new_layer;
            y = page_height_mm - margin_y_mm;
        }

        let current_layer = document.get_page(page).get_layer(layer);
        let content = if line.is_empty() {
            " ".to_string()
        } else {
            line
        };
        current_layer.use_text(content, font_size_pt, Mm(margin_x_mm), Mm(y), &font);
        y -= line_height_mm;
    }

    let mut bytes = Vec::new();
    {
        let mut writer = std::io::BufWriter::new(&mut bytes);
        document.save(&mut writer)?;
    }

    Ok(bytes)
}

#[cfg(not(target_os = "macos"))]
fn build_pdf_document(
    title: &str,
    markdown: &str,
    _source_file_path: Option<&Path>,
) -> anyhow::Result<Vec<u8>> {
    build_pdf_document_with_printpdf(title, markdown)
}

fn run_git_command(repo_root: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }

    Some(
        String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_string(),
    )
}

pub(super) fn git_show_file_at_revision(
    repo_root: &Path,
    revision: &str,
    relative_path: &str,
) -> String {
    let object = format!("{revision}:{relative_path}");
    let Ok(output) = Command::new("git")
        .arg("-C")
        .arg(repo_root)
        .args(["show", &object])
        .output()
    else {
        return String::new();
    };

    if output.status.success() {
        String::from_utf8_lossy(&output.stdout).into_owned()
    } else {
        String::new()
    }
}

fn build_git_diff_rows(
    language: crate::i18n::UiLanguage,
    base_text: &str,
    target_text: &str,
    is_markdown: bool,
    code_language: Option<String>,
) -> Vec<GitDiffRow> {
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .diff_lines(base_text, target_text);
    let base_decorations = if is_markdown {
        crate::markdown::parse_decorations(base_text)
    } else {
        super::plain_text_decorations(base_text, code_language.clone())
    };
    let target_decorations = if is_markdown {
        crate::markdown::parse_decorations(target_text)
    } else {
        super::plain_text_decorations(target_text, code_language.clone())
    };
    let mut pending_rows = Vec::new();

    for op in diff.ops() {
        for change in diff.iter_inline_changes(op) {
            let (text, inline_highlights) = build_inline_change_text_and_highlights(&change);
            pending_rows.push(PendingGitDiffRow {
                kind: match change.tag() {
                    ChangeTag::Equal => GitDiffRowKind::Context,
                    ChangeTag::Delete => GitDiffRowKind::Removed,
                    ChangeTag::Insert => GitDiffRowKind::Added,
                },
                inline_highlights,
                text,
                old_line_index: change.old_index(),
                new_line_index: change.new_index(),
            });
        }
    }

    let mut rows = pending_rows
        .into_iter()
        .map(|row| build_git_diff_row(row, &base_decorations, &target_decorations))
        .collect::<Vec<_>>();

    if is_markdown {
        normalize_diff_table_metadata(&mut rows);
        normalize_diff_block_preview_metadata(&mut rows);
    }

    if rows.is_empty() {
        rows.push(GitDiffRow {
            kind: GitDiffRowKind::Context,
            text: crate::i18n::translate(language, crate::i18n::TextKey::NoTextDiff).to_string(),
            decoration: crate::markdown::LineDecoration::default(),
            source_line_number: None,
            is_hunk_label: false,
        });
    }

    rows
}

fn build_git_diff_row(
    row: PendingGitDiffRow,
    base_decorations: &[crate::markdown::LineDecoration],
    target_decorations: &[crate::markdown::LineDecoration],
) -> GitDiffRow {
    let source_line_number = match row.kind {
        GitDiffRowKind::Removed => row.old_line_index,
        GitDiffRowKind::Context | GitDiffRowKind::Added => {
            row.new_line_index.or(row.old_line_index)
        }
    }
    .map(|index| index + 1);

    let mut decoration = match row.kind {
        GitDiffRowKind::Context | GitDiffRowKind::Added => row
            .new_line_index
            .and_then(|index| target_decorations.get(index).cloned())
            .or_else(|| {
                row.old_line_index
                    .and_then(|index| base_decorations.get(index).cloned())
            })
            .unwrap_or_default(),
        GitDiffRowKind::Removed => row
            .old_line_index
            .and_then(|index| base_decorations.get(index).cloned())
            .unwrap_or_default(),
    };

    decoration.diff_added_spans.clear();
    decoration.diff_removed_spans.clear();
    match row.kind {
        GitDiffRowKind::Context => {}
        GitDiffRowKind::Added => {
            decoration.diff_added_spans = row.inline_highlights.clone();
        }
        GitDiffRowKind::Removed => {
            decoration.diff_removed_spans = row.inline_highlights.clone();
        }
    }

    if decoration
        .table_info
        .as_ref()
        .is_some_and(|table_info| !table_info.is_front_matter)
    {
        decoration.block_type = crate::markdown::BlockType::CodeBlock;
        decoration.code_language = Some("table".to_string());
    }

    GitDiffRow {
        kind: row.kind,
        text: row.text,
        decoration,
        source_line_number,
        is_hunk_label: false,
    }
}

fn build_git_diff_line_label_row(line_number: usize) -> GitDiffRow {
    GitDiffRow {
        kind: GitDiffRowKind::Context,
        text: format!("L{line_number}"),
        decoration: crate::markdown::LineDecoration::default(),
        source_line_number: Some(line_number),
        is_hunk_label: true,
    }
}

fn normalize_diff_table_metadata(rows: &mut [GitDiffRow]) {
    let mut row_index = 0usize;
    while row_index < rows.len() {
        if rows[row_index].decoration.table_info.is_none() {
            row_index += 1;
            continue;
        }

        let table_start = row_index;
        let mut table_end = row_index + 1;
        let mut column_widths = Vec::new();

        while table_end < rows.len() && rows[table_end].decoration.table_info.is_some() {
            table_end += 1;
        }

        for row in &rows[table_start..table_end] {
            let Some(table_info) = row.decoration.table_info.as_ref() else {
                continue;
            };
            if column_widths.len() < table_info.column_widths.len() {
                column_widths.resize(table_info.column_widths.len(), 3);
            }
            for (column_index, width) in table_info.column_widths.iter().enumerate() {
                column_widths[column_index] = column_widths[column_index].max(*width);
            }
        }

        for row in &mut rows[table_start..table_end] {
            let Some(table_info) = row.decoration.table_info.as_mut() else {
                continue;
            };
            table_info.table_start_line = table_start;
            table_info.table_end_line = table_end;
            if table_info.column_widths.len() < column_widths.len() {
                table_info.column_widths.resize(column_widths.len(), 3);
            }
            for (column_index, width) in column_widths.iter().enumerate() {
                table_info.column_widths[column_index] =
                    table_info.column_widths[column_index].max(*width);
            }
        }

        row_index = table_end;
    }
}

fn normalize_diff_block_preview_metadata(rows: &mut [GitDiffRow]) {
    let mut row_index = 0usize;
    while row_index < rows.len() {
        let Some(preview) = rows[row_index].decoration.block_preview.as_ref() else {
            row_index += 1;
            continue;
        };

        let group_kind = rows[row_index].kind;
        let group_cache_key = preview.cache_key.clone();
        let group_start = row_index;
        let mut group_end = row_index + 1;

        while group_end < rows.len() {
            let Some(next_preview) = rows[group_end].decoration.block_preview.as_ref() else {
                break;
            };
            if rows[group_end].kind != group_kind || next_preview.cache_key != group_cache_key {
                break;
            }
            group_end += 1;
        }

        let normalized_end_line = group_end.saturating_sub(1);
        for (line_index, row) in rows[group_start..group_end].iter_mut().enumerate() {
            let Some(preview) = row.decoration.block_preview.as_mut() else {
                continue;
            };
            preview.start_line = group_start;
            preview.end_line = normalized_end_line;
            preview.is_anchor = line_index == 0;
        }

        row_index = group_end;
    }
}

pub(super) fn build_inline_change_text_and_highlights(
    change: &InlineChange<'_, str>,
) -> (String, Vec<Range<usize>>) {
    let mut text = String::new();
    let mut ranges = Vec::new();

    for (emphasized, value) in change.iter_strings_lossy() {
        let segment = value.trim_end_matches('\n');
        let start = text.len();
        text.push_str(segment);
        let end = text.len();
        if emphasized {
            if start < end {
                ranges.push(start..end);
            }
        }
    }

    if ranges.is_empty() && !matches!(change.tag(), ChangeTag::Equal) && !text.is_empty() {
        ranges.push(0..text.len());
    }

    (text, merge_adjacent_inline_ranges(ranges))
}

pub(super) fn merge_adjacent_inline_ranges(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
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

fn build_git_diff_snapshot_text(rows: &[GitDiffRow]) -> String {
    rows.iter()
        .map(|row| row.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

fn visible_git_diff_rows(rows: &[GitDiffRow], show_only_changes: bool) -> Vec<GitDiffRow> {
    if !show_only_changes {
        return rows.to_vec();
    }

    let mut included = vec![false; rows.len()];
    for (row_index, row) in rows.iter().enumerate() {
        if row.kind == GitDiffRowKind::Context {
            continue;
        }

        included[row_index] = true;
        if let Some(table_info) = row
            .decoration
            .table_info
            .as_ref()
            .filter(|table_info| !table_info.is_front_matter)
        {
            let table_start = table_info.table_start_line.min(rows.len());
            let table_end = table_info.table_end_line.min(rows.len());
            for included_row in &mut included[table_start..table_end] {
                *included_row = true;
            }
        }
    }

    let mut filtered = Vec::new();
    let mut at_hunk_start = true;

    for (row_index, row) in rows.iter().enumerate() {
        if !included[row_index] {
            at_hunk_start = true;
            continue;
        }

        if at_hunk_start {
            if let Some(line_number) = row.source_line_number {
                filtered.push(build_git_diff_line_label_row(line_number));
            }
            at_hunk_start = false;
        }

        filtered.push(row.clone());
    }

    if filtered.is_empty() {
        rows.to_vec()
    } else {
        filtered
    }
}

fn estimate_line_display_cache(lines: &[Option<Box<LineDisplayInfo>>]) -> (usize, usize) {
    let mut cached = 0usize;
    let mut bytes = lines.len() * size_of::<Option<Box<LineDisplayInfo>>>();
    for info in lines.iter().flatten() {
        cached += 1;
        bytes += size_of::<LineDisplayInfo>();
        bytes += info.display_text.capacity();
        bytes += info.display_to_buffer.capacity() * size_of::<usize>();
        bytes += info.buffer_to_display.capacity() * size_of::<usize>();
        bytes += info.base_runs.capacity() * size_of::<gpui::TextRun>();
        bytes +=
            info.wrap_boundary_upstream_xs.capacity() * (size_of::<usize>() + size_of::<f32>());
    }
    (cached, bytes)
}

impl EditorView {
    pub(crate) fn runtime_profile_diagnostics(&self) -> super::profiler::RuntimeProfileDiagnostics {
        let (line_display_cached, line_display_estimated_bytes) =
            estimate_line_display_cache(&self.line_display_info);
        let (tab_layout_cached_slots, tab_layout_cached_lines, tab_layout_estimated_bytes) = self
            .tab_layout_cache
            .values()
            .fold((0usize, 0usize, 0usize), |acc, (lines, _)| {
                let (cached, bytes) = estimate_line_display_cache(lines);
                (acc.0 + lines.len(), acc.1 + cached, acc.2 + bytes)
            });
        let (info_panel_primary_rows, info_panel_reference_items) = self
            .info_panel_cache_result
            .as_ref()
            .and_then(|result| result.as_ref())
            .map(|data| {
                (
                    data.primary_rows.len(),
                    data.attachment_items.len()
                        + data.image_items.len()
                        + data.external_link_items.len()
                        + data.wiki_link_items.len(),
                )
            })
            .unwrap_or((0, 0));

        super::profiler::RuntimeProfileDiagnostics {
            active_tab_id: self.active_tab_id.clone(),
            buffer_bytes: self.buffer.len(),
            buffer_lines: self.buffer.line_count(),
            decorations_len: self.decorations.len(),
            decorations_estimated_struct_bytes: self.decorations.len()
                * size_of::<LineDecoration>(),
            line_display_slots: self.line_display_info.len(),
            line_display_cached,
            line_display_estimated_bytes,
            tab_layout_cached_tabs: self.tab_layout_cache.len(),
            tab_layout_cached_slots,
            tab_layout_cached_lines,
            tab_layout_estimated_bytes,
            image_cache_entries: self.image_cache.len(),
            image_cache_loaded_entries: self
                .image_cache
                .values()
                .filter(|image| image.is_some())
                .count(),
            generated_preview_keys: self.generated_preview_keys.len(),
            generated_preview_svg_entries: self.generated_preview_svg_cache.len(),
            generated_preview_svg_bytes: self
                .generated_preview_svg_cache
                .values()
                .map(|svg| svg.len())
                .sum(),
            generated_preview_overlay_entries: self.generated_preview_overlay_cache.len(),
            generated_preview_view_entries: self.generated_preview_views.len(),
            outline_heading_count: self.outline_headings_cache.borrow().len(),
            info_panel_primary_rows,
            info_panel_reference_items,
            sidebar_items: self.sidebar_items_cache.len(),
            file_list_items: self.file_list_items.len(),
            open_tabs: self.tabs.len(),
        }
    }

    pub(crate) fn invalidate_info_panel_cache(&mut self) {
        self.info_panel_cache_tab_id = None;
        self.info_panel_cache_result = None;
    }

    pub(crate) fn invalidate_git_history_cache(&mut self) {
        self.git_history_cache_dirty = true;
        self.git_history_cache_scanned_at = None;
        self.git_history_refresh_epoch = self.git_history_refresh_epoch.wrapping_add(1);
    }

    pub(crate) fn invalidate_active_file_panel_caches(&mut self) {
        self.invalidate_info_panel_cache();
        self.invalidate_git_history_cache();
    }

    fn cached_info_panel_data(&mut self) -> Option<Arc<InfoPanelData>> {
        let current_tab_id = self.active_tab_id.clone();
        if self.info_panel_cache_tab_id != current_tab_id {
            self.info_panel_cache_tab_id = current_tab_id;
            self.info_panel_cache_result = Some(self.collect_info_panel_data().map(Arc::new));
        }

        self.info_panel_cache_result
            .as_ref()
            .and_then(|result| result.as_ref().cloned())
    }

    fn schedule_git_history_refresh(&mut self, cx: &mut Context<Self>) {
        if !self.git_history_cache_dirty || self.git_history_refresh_task.is_some() {
            return;
        }

        let Some(requested_file_path) = self.git_history_cache_path.clone() else {
            self.git_history_cache_result = None;
            self.git_history_cache_scanned_at = Some(SystemTime::now());
            self.git_history_cache_dirty = false;
            return;
        };

        let epoch = self.git_history_refresh_epoch;
        let language = self.ui_language;
        let delay = remaining_refresh_delay(
            self.git_history_cache_scanned_at,
            GIT_HISTORY_REFRESH_MIN_INTERVAL_MS,
        );
        let background_file_path = requested_file_path.clone();
        let task = cx.spawn(async move |this, cx| {
            if !delay.is_zero() {
                cx.background_executor().timer(delay).await;
            }

            let items = cx
                .background_executor()
                .spawn(async move {
                    collect_git_history_items_for_file(language, &background_file_path)
                })
                .await;

            this.update(cx, |this, cx| {
                this.git_history_refresh_task = None;

                if this.git_history_cache_path.as_ref() != Some(&requested_file_path)
                    || this.git_history_refresh_epoch != epoch
                {
                    if this.git_history_cache_dirty {
                        this.schedule_git_history_refresh(cx);
                    }
                    return;
                }

                this.git_history_cache_result = Some(items);
                this.git_history_cache_scanned_at = Some(SystemTime::now());
                this.git_history_cache_dirty = false;
                cx.notify();
            })
            .ok();
        });
        self.git_history_refresh_task = Some(task);
    }

    fn cached_git_history_items(&mut self, cx: &mut Context<Self>) -> Option<Vec<GitHistoryItem>> {
        let current_path = self.current_file_path.clone();
        if self.git_history_cache_path != current_path {
            self.git_history_cache_path = current_path.clone();
            self.git_history_cache_result = None;
            self.git_history_cache_scanned_at = None;
            self.git_history_cache_dirty = true;
            self.git_history_refresh_epoch = self.git_history_refresh_epoch.wrapping_add(1);
            self.git_history_refresh_task = None;
        }

        if self.git_history_cache_dirty {
            self.schedule_git_history_refresh(cx);
        }

        self.git_history_cache_result.clone().flatten()
    }

    fn git_repo_context_for_path(&self, file_path: &Path) -> Option<GitRepoContext> {
        git_repo_context_for_file(file_path)
    }

    fn git_repo_context(&self) -> Option<GitRepoContext> {
        let file_path = self.current_file_path.clone()?;
        self.git_repo_context_for_path(&file_path)
    }

    fn build_git_diff_state_for_source(
        &self,
        path: &Path,
        source: GitDiffSource,
    ) -> Option<GitDiffState> {
        let context = self.git_repo_context_for_path(path)?;
        let (source_label, target_label, base_text, target_text) = match &source {
            GitDiffSource::WorkingTree => {
                let base_text =
                    git_show_file_at_revision(&context.repo_root, "HEAD", &context.relative_path);
                let target_text = self.current_file_text_for_git_diff(&context.file_path);
                (
                    if base_text.is_empty() {
                        self.t(crate::i18n::TextKey::EmptyFile).to_string()
                    } else {
                        "HEAD".to_string()
                    },
                    self.t(crate::i18n::TextKey::WorkingTree).to_string(),
                    base_text,
                    target_text,
                )
            }
            GitDiffSource::Commit {
                commit_hash,
                short_hash,
            } => {
                let parent_revision = format!("{commit_hash}^");
                let base_text = git_show_file_at_revision(
                    &context.repo_root,
                    &parent_revision,
                    &context.relative_path,
                );
                let target_text = git_show_file_at_revision(
                    &context.repo_root,
                    commit_hash,
                    &context.relative_path,
                );
                (
                    format!("{short_hash}^"),
                    short_hash.clone(),
                    base_text,
                    target_text,
                )
            }
        };

        let file_kind = super::classify_file_kind_for_bytes(path, target_text.as_bytes());
        let is_markdown = matches!(file_kind, FileKind::Markdown);
        let code_language = match &file_kind {
            FileKind::Code { language } => Some(language.clone()),
            _ => None,
        };

        Some(GitDiffState {
            source,
            source_label,
            target_label,
            all_rows: build_git_diff_rows(
                self.ui_language,
                &base_text,
                &target_text,
                is_markdown,
                code_language,
            ),
            rows: Vec::new(),
            show_only_changes: false,
        })
        .map(|mut diff| {
            diff.rows = visible_git_diff_rows(&diff.all_rows, diff.show_only_changes);
            diff
        })
    }

    pub(crate) fn refresh_git_diff_tab(&mut self, tab_index: usize) -> bool {
        let Some(tab) = self.tabs.get(tab_index) else {
            return false;
        };
        if tab.kind != TabKind::Diff {
            return false;
        }
        let Some(path) = tab.path.clone() else {
            return false;
        };
        let Some((source, show_only_changes)) = tab
            .git_diff
            .as_ref()
            .map(|diff| (diff.source.clone(), diff.show_only_changes))
        else {
            return false;
        };
        let Some(mut diff) = self.build_git_diff_state_for_source(&path, source) else {
            return false;
        };
        diff.show_only_changes = show_only_changes;
        diff.rows = visible_git_diff_rows(&diff.all_rows, diff.show_only_changes);

        let snapshot_text = build_git_diff_snapshot_text(&diff.rows);
        let tab = &mut self.tabs[tab_index];
        tab.snapshot.text = snapshot_text;
        tab.snapshot.cursor_offset = tab.snapshot.cursor_offset.min(tab.snapshot.text.len());
        tab.snapshot.cursor_anchor = tab.snapshot.cursor_anchor.min(tab.snapshot.text.len());
        tab.git_diff = Some(diff);
        true
    }

    fn active_git_diff_tab_index(&self) -> Option<usize> {
        self.active_tab_index().filter(|index| {
            self.tabs
                .get(*index)
                .is_some_and(|tab| tab.kind == TabKind::Diff)
        })
    }

    fn sync_git_diff_tab_snapshot(&mut self, tab_index: usize) {
        let Some(tab) = self.tabs.get_mut(tab_index) else {
            return;
        };
        let Some(diff) = tab.git_diff.as_ref() else {
            return;
        };
        tab.snapshot.text = build_git_diff_snapshot_text(&diff.rows);
        tab.snapshot.cursor_offset = tab.snapshot.cursor_offset.min(tab.snapshot.text.len());
        tab.snapshot.cursor_anchor = tab.snapshot.cursor_anchor.min(tab.snapshot.text.len());
    }

    fn reload_active_git_diff_snapshot(&mut self) {
        let Some(tab_index) = self.active_git_diff_tab_index() else {
            return;
        };
        let Some(snapshot) = self.tabs.get(tab_index).map(|tab| tab.snapshot.clone()) else {
            return;
        };
        self.buffer = crate::buffer::Buffer::from_text(&snapshot.text);
        self.cursor.offset = snapshot.cursor_offset.min(self.buffer.len());
        self.cursor.anchor = snapshot.cursor_anchor.min(self.buffer.len());
        self.reset_cursor_visual_targeting();
        self.reparse();
    }

    fn first_diff_row_line(rows: &[GitDiffRow]) -> Option<usize> {
        rows.iter()
            .position(|row| row.kind != GitDiffRowKind::Context)
    }

    /// Groups consecutive changed (non-context) rows into contiguous diff blocks
    /// (hunks). Each returned range covers one block, which may span many lines.
    fn diff_blocks(rows: &[GitDiffRow]) -> Vec<std::ops::Range<usize>> {
        let mut blocks: Vec<std::ops::Range<usize>> = Vec::new();
        let mut start: Option<usize> = None;
        for (index, row) in rows.iter().enumerate() {
            if row.kind != GitDiffRowKind::Context {
                if start.is_none() {
                    start = Some(index);
                }
            } else if let Some(begin) = start.take() {
                blocks.push(begin..index);
            }
        }
        if let Some(begin) = start.take() {
            blocks.push(begin..rows.len());
        }
        blocks
    }

    /// (current block index 1-based, total block count) for the active git diff.
    pub(crate) fn active_git_diff_block_progress(&self) -> Option<(usize, usize)> {
        let diff = self.active_git_diff_state()?;
        let blocks = Self::diff_blocks(&diff.rows);
        let total = blocks.len();
        if total == 0 {
            return Some((0, 0));
        }
        let current_line = self.cursor_line();
        let current_index = blocks
            .iter()
            .position(|block| block.contains(&current_line))
            .or_else(|| blocks.iter().position(|block| block.start >= current_line))
            .unwrap_or(total - 1);
        Some((current_index + 1, total))
    }

    fn nearest_diff_row_line(rows: &[GitDiffRow], current_line: usize) -> Option<usize> {
        if rows.is_empty() {
            return None;
        }
        if rows
            .get(current_line)
            .is_some_and(|row| row.kind != GitDiffRowKind::Context)
        {
            return Some(current_line);
        }
        rows.iter()
            .enumerate()
            .skip(current_line)
            .find(|(_, row)| row.kind != GitDiffRowKind::Context)
            .map(|(index, _)| index)
            .or_else(|| {
                rows.iter()
                    .enumerate()
                    .rev()
                    .find(|(_, row)| row.kind != GitDiffRowKind::Context)
                    .map(|(index, _)| index)
            })
    }

    fn adjacent_diff_row_line(
        rows: &[GitDiffRow],
        current_line: usize,
        forward: bool,
    ) -> Option<usize> {
        // Navigate between diff blocks (hunks) by jumping to their start line,
        // so the [X/Y] counter (which counts blocks) stays in sync with the
        // up/down navigation buttons.
        let blocks = Self::diff_blocks(rows);
        if forward {
            blocks
                .iter()
                .map(|block| block.start)
                .find(|&start| start > current_line)
        } else {
            blocks
                .iter()
                .map(|block| block.start)
                .rev()
                .find(|&start| start < current_line)
        }
    }

    pub(crate) fn active_git_diff_show_only_changes(&self) -> bool {
        self.active_git_diff_state()
            .is_some_and(|diff| diff.show_only_changes)
    }

    /// Returns the (added, removed) line counts for the active git diff tab.
    pub(crate) fn active_git_diff_added_removed_stats(&self) -> Option<(usize, usize)> {
        self.active_git_diff_state().map(|diff| {
            let mut added = 0usize;
            let mut removed = 0usize;
            for row in &diff.all_rows {
                match row.kind {
                    GitDiffRowKind::Added => added += 1,
                    GitDiffRowKind::Removed => removed += 1,
                    GitDiffRowKind::Context => {}
                }
            }
            (added, removed)
        })
    }

    pub(crate) fn can_navigate_previous_git_diff_row(&self) -> bool {
        self.active_git_diff_state().is_some_and(|diff| {
            Self::adjacent_diff_row_line(&diff.rows, self.cursor_line(), false).is_some()
        })
    }

    pub(crate) fn can_navigate_next_git_diff_row(&self) -> bool {
        self.active_git_diff_state().is_some_and(|diff| {
            Self::adjacent_diff_row_line(&diff.rows, self.cursor_line(), true).is_some()
        })
    }

    pub(crate) fn toggle_git_diff_show_only_changes(&mut self, cx: &mut Context<Self>) {
        let Some(tab_index) = self.active_git_diff_tab_index() else {
            return;
        };
        let current_line = self.cursor_line();

        let target_line = {
            let Some(diff) = self
                .tabs
                .get_mut(tab_index)
                .and_then(|tab| tab.git_diff.as_mut())
            else {
                return;
            };
            diff.show_only_changes = !diff.show_only_changes;
            diff.rows = visible_git_diff_rows(&diff.all_rows, diff.show_only_changes);
            Self::nearest_diff_row_line(&diff.rows, current_line)
                .or_else(|| Self::first_diff_row_line(&diff.rows))
                .unwrap_or(0)
        };

        self.sync_git_diff_tab_snapshot(tab_index);
        self.reload_active_git_diff_snapshot();
        self.scroll_to_line(target_line, cx);
        self.request_editor_focus(cx);
    }

    pub(crate) fn navigate_previous_git_diff_row(&mut self, cx: &mut Context<Self>) {
        let Some(diff) = self.active_git_diff_state() else {
            return;
        };
        let Some(target_line) = Self::adjacent_diff_row_line(&diff.rows, self.cursor_line(), false)
        else {
            return;
        };
        self.scroll_to_line(target_line, cx);
        self.request_editor_focus(cx);
    }

    pub(crate) fn navigate_next_git_diff_row(&mut self, cx: &mut Context<Self>) {
        let Some(diff) = self.active_git_diff_state() else {
            return;
        };
        let Some(target_line) = Self::adjacent_diff_row_line(&diff.rows, self.cursor_line(), true)
        else {
            return;
        };
        self.scroll_to_line(target_line, cx);
        self.request_editor_focus(cx);
    }

    fn current_file_text_for_git_diff(&self, path: &Path) -> String {
        if self.current_file_path.as_deref() == Some(path) && !self.active_tab_is_diff() {
            return self.buffer.text().to_string();
        }

        fs::read_to_string(path).unwrap_or_default()
    }

    fn open_git_diff_tab(
        &mut self,
        diff_title: String,
        path: PathBuf,
        diff: GitDiffState,
        cx: &mut Context<Self>,
    ) {
        let snapshot = EditorSnapshot {
            text: build_git_diff_snapshot_text(&diff.rows),
            cursor_offset: 0,
            cursor_anchor: 0,
            scroll_offset: 0.0,
            extra_cursors: Vec::new(),
        };
        let file_kind = super::classify_file_kind_for_bytes(&path, snapshot.text.as_bytes());
        if let Some(index) = self.tabs.iter().position(|tab| tab.kind == TabKind::Diff) {
            self.tabs[index].title = diff_title;
            self.tabs[index].path = Some(path);
            self.tabs[index].file_kind = Some(file_kind);
            self.tabs[index].snapshot = snapshot;
            self.tabs[index].git_diff = Some(diff);
            if self.active_tab_index() == Some(index) {
                self.activate_tab_by_index_without_persist(index, cx);
            } else {
                self.activate_tab_by_index(index, cx);
            }
            return;
        }

        self.persist_active_tab_state();
        let insert_index = self
            .active_tab_index()
            .map(|index| index + 1)
            .unwrap_or(self.tabs.len());
        let tab = OpenTab {
            id: self.next_tab_identity(),
            title: diff_title,
            kind: TabKind::Diff,
            path: Some(path),
            file_kind: Some(file_kind),
            editor_left_aligned: false,
            snapshot,
            image_view: ImageViewState::default(),
            whiteboard: None,
            git_diff: Some(diff),
            inline_diff: None,
            is_preview: false,
            is_dirty: false,
            is_deleted: false,
            is_pinned: false,
            recovery: config_store::TabRecoveryState::default(),
            history: history_store::TabHistory::default(),
        };
        self.tabs.insert(insert_index, tab);
        self.activate_tab_by_index(insert_index, cx);
    }

    fn open_git_history_item_diff(&mut self, item: GitHistoryItem, cx: &mut Context<Self>) {
        let Some(context) = self.git_repo_context() else {
            return;
        };

        // Working-tree changes are shown in place in the editor via the inline
        // diff (gutter indicators, expandable hunks, per-hunk revert), so just
        // focus the file instead of opening a read-only diff tab. Historical
        // commits still open the dedicated read-only diff tab.
        if item.is_working_tree {
            self.open_path_in_preview_tab(context.file_path, true, cx);
            return;
        }

        let diff_title = Self::title_for_path(&context.file_path);
        let source = GitDiffSource::Commit {
            commit_hash: item.commit_hash.clone().unwrap_or_default(),
            short_hash: item.short_hash.clone(),
        };

        let Some(diff) = self.build_git_diff_state_for_source(&context.file_path, source) else {
            return;
        };

        self.open_git_diff_tab(diff_title, context.file_path, diff, cx);
    }

    fn resolve_info_panel_reference_path(&self, raw: &str) -> Option<PathBuf> {
        let decoded = decode_file_url_path(raw);
        let raw = decoded.trim();
        if raw.is_empty() || raw.starts_with('#') {
            return None;
        }

        let expanded = if let Some(rest) = raw.strip_prefix("~/") {
            PathBuf::from(std::env::var("HOME").ok()?).join(rest)
        } else {
            PathBuf::from(raw)
        };

        if expanded.is_absolute() {
            return Some(expanded);
        }

        if let Some(file_dir) = self
            .current_file_path
            .as_ref()
            .and_then(|path| path.parent())
        {
            let candidate = file_dir.join(&expanded);
            if candidate.exists() {
                return Some(candidate);
            }
        }

        for root in &self.workspaces {
            let candidate = root.join(&expanded);
            if candidate.exists() {
                return Some(candidate);
            }
        }

        if let Some(file_dir) = self
            .current_file_path
            .as_ref()
            .and_then(|path| path.parent())
        {
            return Some(file_dir.join(expanded));
        }

        Some(expanded)
    }

    fn classify_info_panel_reference_target(&self, raw: &str) -> Option<InfoPanelReferenceTarget> {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return None;
        }

        if trimmed.starts_with("file://") {
            return self
                .resolve_info_panel_reference_path(raw)
                .map(InfoPanelReferenceTarget::LocalPath);
        }

        if reference_scheme(trimmed).is_some() || trimmed.starts_with("//") {
            let url = if trimmed.starts_with("//") {
                format!("https:{trimmed}")
            } else {
                trimmed.to_string()
            };
            return Some(InfoPanelReferenceTarget::ExternalUrl(url));
        }

        self.resolve_info_panel_reference_path(raw)
            .map(InfoPanelReferenceTarget::LocalPath)
    }

    fn reveal_panel_reference_in_finder(&self, path: &Path) {
        if path.exists() {
            self.reveal_in_finder(path);
            return;
        }

        if let Some(parent) = path.parent().filter(|parent| parent.exists()) {
            if let Err(error) = Command::new("open").arg(parent).spawn() {
                eprintln!("failed to open parent folder {}: {error}", parent.display());
            }
        }
    }

    fn trigger_info_panel_share_action(&self, action: &InfoPanelShareAction) {
        match action {
            InfoPanelShareAction::RevealPath(path) => self.reveal_panel_reference_in_finder(path),
            InfoPanelShareAction::OpenUrl(url) => {
                if let Err(error) = Command::new("open").arg(url).spawn() {
                    eprintln!("failed to open URL {url}: {error}");
                }
            }
        }
    }

    fn collect_info_panel_resources(
        &self,
        text: &str,
    ) -> (
        Vec<InfoPanelReferenceItem>,
        Vec<InfoPanelReferenceItem>,
        Vec<InfoPanelReferenceItem>,
        Vec<InfoPanelReferenceItem>,
    ) {
        let mut attachment_items = Vec::new();
        let mut image_items = Vec::new();
        let mut external_link_items = Vec::new();
        let mut wiki_link_items = Vec::new();
        let mut pending_references = Vec::new();

        let mut commit_reference = |pending: PendingPanelReference| match pending.kind {
            PendingPanelReferenceKind::Link => {
                let Some(target) = self.classify_info_panel_reference_target(&pending.target)
                else {
                    return;
                };
                let label = fallback_reference_label(&pending.label, &pending.target, &target);

                match target {
                    InfoPanelReferenceTarget::LocalPath(path) => {
                        attachment_items.push(InfoPanelReferenceItem {
                            label,
                            source_line: pending.source_line,
                            share_action: InfoPanelShareAction::RevealPath(path),
                        });
                    }
                    InfoPanelReferenceTarget::ExternalUrl(url) => {
                        external_link_items.push(InfoPanelReferenceItem {
                            label,
                            source_line: pending.source_line,
                            share_action: InfoPanelShareAction::OpenUrl(url),
                        });
                    }
                }
            }
            PendingPanelReferenceKind::Image => {
                let Some(target) = self.classify_info_panel_reference_target(&pending.target)
                else {
                    return;
                };

                let share_action = match &target {
                    InfoPanelReferenceTarget::LocalPath(path) => {
                        InfoPanelShareAction::RevealPath(path.clone())
                    }
                    InfoPanelReferenceTarget::ExternalUrl(url) => {
                        InfoPanelShareAction::OpenUrl(url.clone())
                    }
                };

                image_items.push(InfoPanelReferenceItem {
                    label: fallback_reference_label(&pending.label, &pending.target, &target),
                    source_line: pending.source_line,
                    share_action,
                });
            }
        };

        for (event, source_range) in Parser::new_ext(text, Options::all()).into_offset_iter() {
            let source_line = self.buffer.offset_to_line_col(source_range.start).0;
            match event {
                Event::Start(Tag::Link {
                    link_type,
                    dest_url,
                    ..
                }) => {
                    if matches!(
                        link_type,
                        LinkType::WikiLink { .. } | LinkType::ShortcutUnknown
                    ) {
                        continue;
                    }
                    pending_references.push(PendingPanelReference {
                        kind: PendingPanelReferenceKind::Link,
                        target: dest_url.to_string(),
                        label: String::new(),
                        source_line,
                    });
                }
                Event::Start(Tag::Image { dest_url, .. }) => {
                    pending_references.push(PendingPanelReference {
                        kind: PendingPanelReferenceKind::Image,
                        target: dest_url.to_string(),
                        label: String::new(),
                        source_line,
                    });
                }
                Event::Text(content) | Event::Code(content) => {
                    if let Some(pending) = pending_references.last_mut() {
                        pending.label.push_str(&content);
                    }
                }
                Event::SoftBreak | Event::HardBreak => {
                    if let Some(pending) = pending_references.last_mut()
                        && !pending.label.ends_with(' ')
                    {
                        pending.label.push(' ');
                    }
                }
                Event::End(TagEnd::Link) | Event::End(TagEnd::Image) => {
                    if let Some(pending) = pending_references.pop() {
                        commit_reference(pending);
                    }
                }
                _ => {}
            }
        }

        for (line, decoration) in self.decorations.iter().enumerate() {
            for wiki_link in &decoration.wiki_links {
                let resolved_path =
                    resolve_wiki_link_target_path(&self.sidebar_root, &wiki_link.target_path)
                        .or_else(|| Some(self.sidebar_root.join(&wiki_link.target_path)));
                let Some(resolved_path) = resolved_path else {
                    continue;
                };

                let target = InfoPanelReferenceTarget::LocalPath(resolved_path.clone());

                wiki_link_items.push(InfoPanelReferenceItem {
                    label: fallback_reference_label("", &wiki_link.target_path, &target),
                    source_line: line,
                    share_action: InfoPanelShareAction::RevealPath(resolved_path),
                });
            }
        }

        attachment_items.sort_by_key(|item| item.source_line);
        image_items.sort_by_key(|item| item.source_line);
        external_link_items.sort_by_key(|item| item.source_line);
        wiki_link_items.sort_by_key(|item| item.source_line);

        (
            attachment_items,
            image_items,
            external_link_items,
            wiki_link_items,
        )
    }

    fn collect_info_panel_data(&self) -> Option<InfoPanelData> {
        let active_index = self.active_tab_index()?;
        let tab = self.tabs.get(active_index)?;
        let path = tab.path.as_deref();
        let relative_path = path
            .and_then(|path| self.relative_path_for_path(path))
            .or_else(|| path.map(|path| path.to_string_lossy().into_owned()))
            .unwrap_or_else(|| self.t(crate::i18n::TextKey::Unsaved).to_string());
        let metadata = path.and_then(|path| fs::metadata(path).ok());
        let created_at = metadata
            .as_ref()
            .and_then(|meta| meta.created().or_else(|_| meta.modified()).ok())
            .map(|time| crate::i18n::format_relative_time(self.ui_language, time))
            .unwrap_or_else(|| self.t(crate::i18n::TextKey::Unavailable).to_string());
        let modified_at = metadata
            .as_ref()
            .and_then(|meta| meta.modified().ok())
            .map(|time| crate::i18n::format_relative_time(self.ui_language, time))
            .unwrap_or_else(|| self.t(crate::i18n::TextKey::Unavailable).to_string());

        let supports_text_export = tab
            .file_kind
            .as_ref()
            .is_some_and(FileKind::supports_text_buffer);
        let text = if supports_text_export {
            self.buffer.text()
        } else {
            ""
        };

        let file_size = metadata
            .as_ref()
            .map(|meta| meta.len())
            .unwrap_or_else(|| text.len() as u64);

        let word_count = if supports_text_export {
            count_document_words(text)
        } else {
            0
        };
        let line_count = if supports_text_export {
            self.buffer.line_count()
        } else {
            0
        };

        let (attachment_items, image_items, external_link_items, wiki_link_items) =
            if supports_text_export {
                self.collect_info_panel_resources(text)
            } else {
                (Vec::new(), Vec::new(), Vec::new(), Vec::new())
            };

        Some(InfoPanelData {
            primary_rows: vec![
                InfoPanelRow {
                    label: self.t(crate::i18n::TextKey::InfoName).to_string(),
                    value: path
                        .and_then(|path| path.file_name())
                        .and_then(|name| name.to_str())
                        .filter(|name| !name.is_empty())
                        .map(ToString::to_string)
                        .unwrap_or(relative_path),
                },
                InfoPanelRow {
                    label: self.t(crate::i18n::TextKey::InfoCreated).to_string(),
                    value: created_at,
                },
                InfoPanelRow {
                    label: self.t(crate::i18n::TextKey::InfoModified).to_string(),
                    value: modified_at,
                },
                InfoPanelRow {
                    label: self.t(crate::i18n::TextKey::InfoSize).to_string(),
                    value: format_file_size(file_size),
                },
                InfoPanelRow {
                    label: self.t(crate::i18n::TextKey::InfoWords).to_string(),
                    value: word_count.to_string(),
                },
                InfoPanelRow {
                    label: self.t(crate::i18n::TextKey::InfoLines).to_string(),
                    value: line_count.to_string(),
                },
            ],
            attachment_items,
            image_items,
            external_link_items,
            wiki_link_items,
            supports_text_export,
        })
    }

    fn export_default_name(&self, extension: &str) -> String {
        let title = self
            .current_file_path
            .as_deref()
            .and_then(Path::file_stem)
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .unwrap_or(self.t(crate::i18n::TextKey::Untitled));
        format!("{}.{}", title, extension)
    }

    fn export_current_markdown(&mut self, cx: &mut Context<Self>) {
        let default_name = self.export_default_name("md");
        let text = self.buffer.text().to_string();
        let ui_language = self.ui_language;

        cx.spawn(async move |_this, cx| {
            let maybe_path = cx
                .background_executor()
                .spawn(async move {
                    let title = crate::i18n::export_dialog_title(ui_language, "Markdown");
                    export_dialog_path(&default_name, &title)
                })
                .await;
            if let Some(path) = maybe_path
                && let Err(error) = fs::write(&path, text)
            {
                eprintln!("failed to export markdown to {:?}: {error}", path);
            }
        })
        .detach();
    }

    fn export_current_html(&mut self, cx: &mut Context<Self>) {
        let title = self
            .current_file_path
            .as_deref()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or(self.t(crate::i18n::TextKey::Untitled))
            .to_string();
        let default_name = self.export_default_name("html");
        let document = build_html_document(
            &title,
            self.buffer.text(),
            self.current_file_path.as_deref(),
        );
        let ui_language = self.ui_language;

        cx.spawn(async move |_this, cx| {
            let maybe_path = cx
                .background_executor()
                .spawn(async move {
                    let title = crate::i18n::export_dialog_title(ui_language, "HTML");
                    export_dialog_path(&default_name, &title)
                })
                .await;
            if let Some(path) = maybe_path
                && let Err(error) = fs::write(&path, document)
            {
                eprintln!("failed to export html to {:?}: {error}", path);
            }
        })
        .detach();
    }

    fn export_current_pdf(&mut self, cx: &mut Context<Self>) {
        let title = self
            .current_file_path
            .as_deref()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or(self.t(crate::i18n::TextKey::Untitled))
            .to_string();
        let default_name = self.export_default_name("pdf");
        let markdown = self.buffer.text().to_string();
        let source_file_path = self.current_file_path.clone();
        let ui_language = self.ui_language;

        cx.spawn(async move |_this, cx| {
            let maybe_path = cx
                .background_executor()
                .spawn(async move {
                    let title = crate::i18n::export_dialog_title(ui_language, "PDF");
                    export_dialog_path(&default_name, &title)
                })
                .await;
            let Some(path) = maybe_path else {
                return;
            };

            #[cfg(target_os = "macos")]
            let export_result = {
                let html = build_html_document(&title, &markdown, source_file_path.as_deref());
                build_pdf_document_from_html_macos(&html, source_file_path.as_deref(), cx).await
            };

            #[cfg(not(target_os = "macos"))]
            let export_result =
                cx.background_executor()
                    .spawn(async move {
                        build_pdf_document(&title, &markdown, source_file_path.as_deref())
                    })
                    .await;

            match export_result {
                Ok(bytes) => {
                    if let Err(error) = fs::write(&path, bytes) {
                        eprintln!("failed to export pdf to {:?}: {error}", path);
                    }
                }
                Err(error) => {
                    eprintln!("failed to render pdf document: {error}");
                }
            }
        })
        .detach();
    }

    fn render_info_section_title(&self, title: &str, icon_path: &'static str) -> AnyElement {
        let ups = &self.styles.utility_panel;
        let icon_size = self.scaled(ups.utility_panel_section_title_font_size_px);
        div()
            .mb(px(self.scaled(5.0)))
            .px(px(self.scaled(ups.utility_panel_section_padding_x_px)))
            .flex()
            .items_center()
            .gap(px(self.scaled(6.0)))
            .text_size(px(self.scaled(ups.utility_panel_section_title_font_size_px)))
            .text_color(ups.utility_panel_section_title_text_color)
            .font_weight(FontWeight::BOLD)
            .child(
                svg()
                    .path(icon_path)
                    .size(px(icon_size + self.scaled(3.0)))
                    .text_color(ups.utility_panel_section_title_text_color),
            )
            .child(SharedString::from(title.to_string()))
            .into_any_element()
    }

    fn render_info_row(&self, row: &InfoPanelRow) -> AnyElement {
        let ups = &self.styles.utility_panel;
        div()
            .px(px(self.scaled(ups.utility_panel_section_padding_x_px)))
            .min_h(px(self.scaled(ups.utility_panel_item_height_px)))
            .w_full()
            .flex()
            .items_start()
            .justify_between()
            .gap(px(self.scaled(ups.utility_panel_item_gap_px)))
            .text_size(px(self.scaled(ups.utility_panel_item_font_size_px)))
            .child(
                div()
                    .text_color(ups.utility_panel_item_label_text_color)
                    .child(SharedString::from(row.label.clone())),
            )
            .child(
                div()
                    .text_color(ups.utility_panel_item_value_text_color)
                    .text_right()
                    .max_w(px(self.utility_panel_layout_width() * 0.6))
                    .child(SharedString::from(row.value.clone())),
            )
            .into_any_element()
    }

    fn render_export_button(
        &self,
        label: &str,
        enabled: bool,
        cx: &mut Context<Self>,
        on_click: impl Fn(&mut Self, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let ups = &self.styles.utility_panel;
        let button = div()
            .min_h(px(self.scaled(ups.utility_panel_button_height_px)))
            .px(px(self.scaled(ups.utility_panel_button_padding_x_px)))
            .flex()
            .items_center()
            .justify_center()
            .rounded(px(self.scaled(ups.utility_panel_button_radius_px)))
            .border(px(self.scaled(ups.utility_panel_button_border_width_px)))
            .border_color(ups.utility_panel_button_border_color)
            .text_size(px(self.scaled(ups.utility_panel_item_font_size_px)))
            .text_color(ups.utility_panel_button_text_color)
            .opacity(if enabled { 1.0 } else { 0.45 })
            .cursor(CursorStyle::Arrow)
            .child(SharedString::from(label.to_string()));

        if enabled {
            button
                .hover(move |style| style.bg(ups.utility_panel_button_hover_bg))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        on_click(this, cx);
                    }),
                )
                .into_any_element()
        } else {
            button.into_any_element()
        }
    }

    fn render_info_empty_line(&self, text: &str) -> AnyElement {
        let ups = &self.styles.utility_panel;
        div()
            .px(px(self.scaled(ups.utility_panel_section_padding_x_px)))
            .min_h(px(self.scaled(ups.utility_panel_item_height_px)))
            .w_full()
            .flex()
            .items_center()
            .text_size(px(self.scaled(ups.utility_panel_item_font_size_px)))
            .text_color(ups.utility_panel_empty_text_color)
            .child(SharedString::from(text.to_string()))
            .into_any_element()
    }

    fn render_reference_item(
        &self,
        item: &InfoPanelReferenceItem,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ups = &self.styles.utility_panel;
        let share_action = item.share_action.clone();
        let source_line = item.source_line;
        let icon_size = self.scaled(ups.utility_panel_item_font_size_px);
        let share_icon_color = if self.theme_kind.is_dark() {
            gpui::white()
        } else {
            self.theme.text_primary
        };
        div()
            .px(px(self.scaled(ups.utility_panel_section_padding_x_px)))
            .min_h(px(self.scaled(ups.utility_panel_item_height_px)))
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(self.scaled(ups.utility_panel_item_gap_px)))
            .text_size(px(self.scaled(ups.utility_panel_item_font_size_px)))
            .cursor(CursorStyle::Arrow)
            .hover(move |style| style.bg(ups.utility_panel_button_hover_bg))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.scroll_to_line(source_line, cx);
                    this.request_editor_focus(cx);
                }),
            )
            .child(
                div()
                    .min_w(px(0.0))
                    .flex_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .truncate()
                    .text_color(ups.utility_panel_item_value_text_color)
                    .child(SharedString::from(item.label.clone())),
            )
            .child(tooltip::with_tooltip(
                div()
                    .flex_none()
                    .w(px(icon_size + self.scaled(8.0)))
                    .h(px(icon_size + self.scaled(8.0)))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(self.scaled(6.0)))
                    .text_color(share_icon_color)
                    .opacity(0.5)
                    .hover(move |style| style.bg(ups.utility_panel_button_hover_bg))
                    .hover(move |style| style.opacity(0.6))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            cx.stop_propagation();
                            this.trigger_info_panel_share_action(&share_action);
                        }),
                    )
                    .child(
                        svg()
                            .path(SHARE_SVG_PATH)
                            .size(px(icon_size))
                            .text_color(share_icon_color),
                    ),
                cx.entity().clone(),
                self.t(crate::i18n::TextKey::RevealInFinder),
                None::<&'static str>,
            ))
            .into_any_element()
    }

    fn render_git_history_item(&self, item: &GitHistoryItem, cx: &mut Context<Self>) -> AnyElement {
        let ups = &self.styles.utility_panel;
        let history_item = item.clone();
        let font_size = self.scaled(ups.utility_panel_item_font_size_px);
        let row_padding_y = self.scaled(ups.utility_panel_item_gap_px * 0.5);
        let row_gap = self.scaled(ups.utility_panel_item_gap_px * 0.4);
        let row_height = font_size * 2.0 + row_gap + row_padding_y * 2.0;
        let is_active = self.active_git_diff_state().is_some_and(|diff| {
            match (
                &diff.source,
                item.is_working_tree,
                item.commit_hash.as_deref(),
            ) {
                (GitDiffSource::WorkingTree, true, _) => true,
                (GitDiffSource::Commit { commit_hash, .. }, false, Some(item_hash)) => {
                    commit_hash == item_hash
                }
                _ => false,
            }
        });
        let row_bg = if is_active {
            ups.utility_panel_button_hover_bg
        } else {
            gpui::transparent_black()
        };
        div()
            .px(px(self.scaled(ups.utility_panel_section_padding_x_px)))
            .py(px(row_padding_y))
            .w_full()
            .min_h(px(row_height))
            .flex()
            .flex_col()
            .gap(px(row_gap))
            .bg(row_bg)
            .overflow_hidden()
            .cursor(CursorStyle::Arrow)
            .hover(move |style| style.bg(ups.utility_panel_button_hover_bg))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.open_git_history_item_diff(history_item.clone(), cx);
                }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(self.scaled(ups.utility_panel_item_gap_px)))
                    .text_size(px(font_size))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .text_color(ups.utility_panel_item_value_text_color)
                            .whitespace_nowrap()
                            .truncate()
                            .child(SharedString::from(item.short_hash.clone())),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_color(ups.utility_panel_item_label_text_color)
                            .whitespace_nowrap()
                            .child(SharedString::from(item.time_label.clone())),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .w_full()
                    .text_size(px(font_size))
                    .text_color(ups.utility_panel_item_value_text_color)
                    .whitespace_nowrap()
                    .truncate()
                    .child(SharedString::from(item.message.clone())),
            )
            .into_any_element()
    }

    fn push_info_reference_section(
        &self,
        children: &mut Vec<AnyElement>,
        content_y: &mut f32,
        title: &str,
        icon_path: &'static str,
        empty_text: &str,
        items: &[InfoPanelReferenceItem],
        cx: &mut Context<Self>,
    ) {
        let ups = &self.styles.utility_panel;
        let section_gap = self.scaled(ups.utility_panel_section_gap_px);
        let item_height = self.scaled(ups.utility_panel_item_height_px).max(1.0);
        let section_title_height = self
            .scaled(ups.utility_panel_section_title_font_size_px + 8.0)
            .max(1.0);

        children.push(div().h(px(section_gap)).flex_shrink_0().into_any_element());
        *content_y += section_gap;
        children.push(self.render_info_section_title(title, icon_path));
        *content_y += section_title_height;

        if items.is_empty() {
            children.push(self.render_info_empty_line(empty_text));
            *content_y += item_height;
            return;
        }

        let (visible_start, visible_end, top_gap, bottom_gap) = self.visible_uniform_list_range(
            &self.info_panel_scroll_handle,
            *content_y,
            item_height,
            items.len(),
            10,
        );
        if top_gap > 0.0 {
            children.push(div().h(px(top_gap)).flex_shrink_0().into_any_element());
        }
        for item in &items[visible_start..visible_end] {
            children.push(self.render_reference_item(item, cx));
        }
        if bottom_gap > 0.0 {
            children.push(div().h(px(bottom_gap)).flex_shrink_0().into_any_element());
        }
        *content_y += items.len() as f32 * item_height;
    }

    pub(crate) fn render_utility_panel(
        &mut self,
        is_chat_focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match self.displayed_utility_panel {
            Some(RightUtilityPanel::Chat) => self.render_chat_panel(is_chat_focused, cx),
            Some(RightUtilityPanel::Info) => self.render_info_panel(cx),
            Some(RightUtilityPanel::GitHistory) => self.render_git_history_panel(cx),
            Some(RightUtilityPanel::Outline) => self.render_outline_panel(cx),
            None => div().into_any_element(),
        }
    }

    pub(crate) fn render_info_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        // On whiteboard tabs the file-info panel hosts the canvas properties.
        if self.active_tab_is_whiteboard() {
            return self.render_whiteboard_properties_panel(cx);
        }
        let data = self.cached_info_panel_data();
        let ups = &self.styles.utility_panel;
        let top_spacer = self.scaled(ups.utility_panel_top_spacer_height_px);
        let bottom_spacer = self.scaled(ups.utility_panel_bottom_spacer_height_px);
        let header_height = self.scaled(ups.utility_panel_header_height_px);
        let header_padding_x = self.scaled(ups.utility_panel_header_padding_x_px);
        let header_font_size = self.scaled(ups.utility_panel_header_font_size_px);
        let section_gap = self.scaled(ups.utility_panel_section_gap_px);
        let item_gap = self.scaled(ups.utility_panel_item_gap_px);
        let section_padding_x = self.scaled(ups.utility_panel_section_padding_x_px);
        let empty_font_size = self.scaled(ups.utility_panel_empty_font_size_px);
        let item_height = self.scaled(ups.utility_panel_item_height_px).max(1.0);
        let section_title_height = self
            .scaled(ups.utility_panel_section_title_font_size_px + 8.0)
            .max(1.0);
        let export_height = self
            .scaled(ups.utility_panel_button_height_px)
            .max(item_height);
        let mut content_y = top_spacer + header_height;

        let mut children: Vec<AnyElement> = vec![
            div().h(px(top_spacer)).flex_shrink_0().into_any_element(),
            div()
                .h(px(header_height))
                .flex_shrink_0()
                .flex()
                .items_center()
                .px(px(header_padding_x))
                .text_size(px(header_font_size))
                .text_color(ups.utility_panel_header_text_color)
                .child(self.t(crate::i18n::TextKey::FileInfo))
                .into_any_element(),
        ];

        if let Some(data) = data {
            children.push(div().h(px(section_gap)).flex_shrink_0().into_any_element());
            content_y += section_gap;
            children.push(self.render_info_section_title(
                self.t(crate::i18n::TextKey::Statistics),
                FILE_INFO_SECTION_INFO_SVG_PATH,
            ));
            content_y += section_title_height;
            for row in &data.primary_rows {
                children.push(self.render_info_row(row));
            }
            content_y += data.primary_rows.len() as f32 * item_height;

            children.push(div().h(px(section_gap)).flex_shrink_0().into_any_element());
            content_y += section_gap;
            children.push(self.render_info_section_title(
                self.t(crate::i18n::TextKey::Export),
                FILE_INFO_SECTION_EXPORT_SVG_PATH,
            ));
            content_y += section_title_height;
            children.push(
                div()
                    .px(px(section_padding_x))
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(ups.utility_panel_button_gap_px)))
                    .children(vec![
                        self.render_export_button(
                            "Markdown",
                            data.supports_text_export,
                            cx,
                            |this, cx| this.export_current_markdown(cx),
                        ),
                        self.render_export_button(
                            "HTML",
                            data.supports_text_export,
                            cx,
                            |this, cx| this.export_current_html(cx),
                        ),
                        self.render_export_button(
                            "PDF",
                            data.supports_text_export,
                            cx,
                            |this, cx| this.export_current_pdf(cx),
                        ),
                    ])
                    .into_any_element(),
            );
            content_y += export_height;

            if data.supports_text_export {
                self.push_info_reference_section(
                    &mut children,
                    &mut content_y,
                    self.t(crate::i18n::TextKey::Attachments),
                    FILE_INFO_SECTION_ATTACHMENT_SVG_PATH,
                    self.t(crate::i18n::TextKey::NoLocalAttachments),
                    &data.attachment_items,
                    cx,
                );
                self.push_info_reference_section(
                    &mut children,
                    &mut content_y,
                    self.t(crate::i18n::TextKey::Images),
                    FMT_IMAGE_SVG,
                    self.t(crate::i18n::TextKey::NoImageLinks),
                    &data.image_items,
                    cx,
                );
                self.push_info_reference_section(
                    &mut children,
                    &mut content_y,
                    self.t(crate::i18n::TextKey::Links),
                    FMT_LINK_SVG,
                    self.t(crate::i18n::TextKey::NoRemoteLinks),
                    &data.external_link_items,
                    cx,
                );
                self.push_info_reference_section(
                    &mut children,
                    &mut content_y,
                    self.t(crate::i18n::TextKey::Backlinks),
                    FMT_WIKI_LINK_SVG,
                    self.t(crate::i18n::TextKey::NoBacklinks),
                    &data.wiki_link_items,
                    cx,
                );
            }
        } else {
            children.push(
                div()
                    .px(px(section_padding_x))
                    .pt(px(section_gap))
                    .text_size(px(empty_font_size))
                    .text_color(ups.utility_panel_empty_text_color)
                    .child(SharedString::from(
                        self.t(crate::i18n::TextKey::NoFileInfo).to_string(),
                    ))
                    .into_any_element(),
            );
        }

        children.push(
            div()
                .h(px(item_gap + bottom_spacer))
                .flex_shrink_0()
                .into_any_element(),
        );

        let scrollbar = self.render_scrollbar_for_handle(
            PanelScrollbarKind::Info,
            &self.info_panel_scroll_handle,
            f32::from(self.info_panel_scroll_handle.bounds().size.height),
            0.0,
            self.scaled(self.styles.editor.editor_scrollbar_right_margin_px),
            cx,
        );

        div()
            .id("info-panel")
            .h_full()
            .w_full()
            .bg(ups.utility_panel_bg)
            .relative()
            .overflow_hidden()
            .border_l(px(ups.utility_panel_border_width_px))
            .border_color(ups.utility_panel_border_color)
            .child(
                div()
                    .id("info-panel-scroll")
                    .h_full()
                    .w_full()
                    .overflow_y_scroll()
                    .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                        this.reveal_panel_scrollbar(PanelScrollbarKind::Info, cx);
                    }))
                    .track_scroll(&self.info_panel_scroll_handle)
                    .flex()
                    .flex_col()
                    .children(children),
            )
            .when_some(scrollbar, |this, scrollbar| this.child(scrollbar))
            .into_any_element()
    }

    pub(crate) fn render_git_history_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let history_items = self.cached_git_history_items(cx);
        let ups = &self.styles.utility_panel;
        let top_spacer = self.scaled(ups.utility_panel_top_spacer_height_px);
        let bottom_spacer = self.scaled(ups.utility_panel_bottom_spacer_height_px);
        let header_height = self.scaled(ups.utility_panel_header_height_px);
        let header_padding_x = self.scaled(ups.utility_panel_header_padding_x_px);
        let header_font_size = self.scaled(ups.utility_panel_header_font_size_px);
        let section_gap = self.scaled(ups.utility_panel_section_gap_px);
        let section_padding_x = self.scaled(ups.utility_panel_section_padding_x_px);
        let empty_font_size = self.scaled(ups.utility_panel_empty_font_size_px);
        let mut children: Vec<AnyElement> = vec![
            div().h(px(top_spacer)).flex_shrink_0().into_any_element(),
            div()
                .h(px(header_height))
                .flex_shrink_0()
                .flex()
                .items_center()
                .px(px(header_padding_x))
                .text_size(px(header_font_size))
                .text_color(ups.utility_panel_header_text_color)
                .child(self.t(crate::i18n::TextKey::GitHistory))
                .into_any_element(),
        ];

        match history_items {
            Some(items) if !items.is_empty() => {
                children.push(div().h(px(section_gap)).flex_shrink_0().into_any_element());
                let item_height = self.scaled(ups.utility_panel_item_font_size_px * 3.4);
                let prefix_height = top_spacer + header_height + section_gap;
                let (visible_start, visible_end, top_gap, bottom_gap) = self
                    .visible_uniform_list_range(
                        &self.git_history_scroll_handle,
                        prefix_height,
                        item_height,
                        items.len(),
                        12,
                    );
                if top_gap > 0.0 {
                    children.push(div().h(px(top_gap)).flex_shrink_0().into_any_element());
                }
                for item in &items[visible_start..visible_end] {
                    children.push(self.render_git_history_item(item, cx));
                }
                if bottom_gap > 0.0 {
                    children.push(div().h(px(bottom_gap)).flex_shrink_0().into_any_element());
                }
            }
            Some(_) => {
                children.push(
                    div()
                        .px(px(section_padding_x))
                        .pt(px(section_gap))
                        .text_size(px(empty_font_size))
                        .text_color(ups.utility_panel_empty_text_color)
                        .child(SharedString::from(
                            self.t(crate::i18n::TextKey::NoGitHistory).to_string(),
                        ))
                        .into_any_element(),
                );
            }
            None if self.git_history_refresh_task.is_some() => {
                children.push(div().h(px(section_gap)).flex_shrink_0().into_any_element());
            }
            None => {
                children.push(
                    div()
                        .px(px(section_padding_x))
                        .pt(px(section_gap))
                        .text_size(px(empty_font_size))
                        .text_color(ups.utility_panel_empty_text_color)
                        .child(SharedString::from(
                            self.t(crate::i18n::TextKey::NotInGitRepo).to_string(),
                        ))
                        .into_any_element(),
                );
            }
        }

        children.push(
            div()
                .h(px(bottom_spacer))
                .flex_shrink_0()
                .into_any_element(),
        );

        let scrollbar = self.render_scrollbar_for_handle(
            PanelScrollbarKind::GitHistory,
            &self.git_history_scroll_handle,
            f32::from(self.git_history_scroll_handle.bounds().size.height),
            0.0,
            self.scaled(self.styles.editor.editor_scrollbar_right_margin_px),
            cx,
        );

        div()
            .id("git-history-panel")
            .h_full()
            .w_full()
            .bg(ups.utility_panel_bg)
            .relative()
            .overflow_hidden()
            .border_l(px(ups.utility_panel_border_width_px))
            .border_color(ups.utility_panel_border_color)
            .child(
                div()
                    .id("git-history-panel-scroll")
                    .h_full()
                    .w_full()
                    .overflow_y_scroll()
                    .on_scroll_wheel(cx.listener(|this, _, _, cx| {
                        this.reveal_panel_scrollbar(PanelScrollbarKind::GitHistory, cx);
                    }))
                    .track_scroll(&self.git_history_scroll_handle)
                    .flex()
                    .flex_col()
                    .children(children),
            )
            .when_some(scrollbar, |this, scrollbar| this.child(scrollbar))
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GitDiffRow, GitDiffRowKind, InfoPanelReferenceTarget, build_git_diff_rows,
        build_git_diff_snapshot_text, build_html_document, external_reference_host,
        fallback_reference_label, path_reference_name, reference_scheme, visible_git_diff_rows,
    };
    use pulldown_cmark::{Event, LinkType, Options, Parser, Tag};
    use std::path::PathBuf;

    #[test]
    fn export_highlights_code_blocks_and_styles_tables() {
        let md = "```rust\nfn main() { let x = 1; }\n```\n";
        let html = super::render_markdown_for_export(md);
        // syntect emits inline-styled spans for highlighted tokens.
        assert!(
            html.contains("<span style=\"color:"),
            "code highlighting: {html}"
        );

        let doc = super::build_html_document("T", md, None);
        assert!(
            doc.contains(".markdown-body table{display:table;width:100%"),
            "full-width tables: missing rule"
        );
        assert!(
            doc.contains("li:has(>input[type=checkbox]){list-style:none;}"),
            "task-list bullets removed: missing rule"
        );
        assert!(
            doc.contains("li>input[type=checkbox]{-webkit-appearance:none;appearance:none;")
                && doc.contains("width:1.14em;height:1.14em;"),
            "task-list checkbox sizing: missing rule"
        );
        assert!(
            doc.contains(".markdown-body ul,.markdown-body ol{padding-left:1.5em;}"),
            "list indentation: missing rule"
        );
        assert!(
            doc.contains(".markdown-body>ul:has(>li>input[type=checkbox]){padding-left:0;}"),
            "top-level task list alignment: missing rule"
        );
        assert!(
            doc.contains(".markdown-body blockquote{margin-left:0;margin-right:0;"),
            "blockquote content-edge alignment: missing margin reset"
        );
        assert!(
            doc.contains(".markdown-body .callout-title svg{flex:0 0 auto;width:.9em;height:.9em;"),
            "callout icon sizing: missing rule"
        );
    }

    #[test]
    fn export_renders_custom_inline_syntax() {
        let md = "H^2^O X~n~ ==hi== ++u++ ~~s~~ :rocket:";
        let html = super::render_markdown_for_export(md);
        assert!(html.contains("<sup>2</sup>"), "superscript: {html}");
        assert!(html.contains("<sub>n</sub>"), "subscript: {html}");
        assert!(html.contains("<mark>hi</mark>"), "highlight: {html}");
        assert!(html.contains("<u>u</u>"), "underline: {html}");
        assert!(html.contains("<del>s</del>"), "strikethrough: {html}");
        assert!(html.contains('\u{1F680}'), "emoji: {html}");
    }

    #[test]
    fn export_renders_callout_with_title_and_body() {
        let md = "> [!TIP] Handy tip\n> body line";
        let html = super::render_markdown_for_export(md);
        assert!(
            html.contains("blockquote class=\"callout callout-tip\""),
            "callout class: {html}"
        );
        assert!(html.contains("callout-title"), "callout title row: {html}");
        assert!(html.contains("Handy tip"), "callout title text: {html}");
        assert!(html.contains("<svg"), "callout icon: {html}");
        assert!(html.contains("body line"), "callout body: {html}");
        assert!(!html.contains("[!TIP]"), "marker removed: {html}");
    }

    #[test]
    fn export_renders_callout_without_title() {
        let md = "> [!WARNING]\n> watch out";
        let html = super::render_markdown_for_export(md);
        assert!(
            html.contains("callout callout-warning"),
            "callout class: {html}"
        );
        assert!(html.contains("Warning"), "default title label: {html}");
        assert!(html.contains("watch out"), "callout body: {html}");
        assert!(!html.contains("[!WARNING]"), "marker removed: {html}");
    }

    #[test]
    fn fallback_label_prefers_url_host_when_description_missing() {
        let target = InfoPanelReferenceTarget::ExternalUrl(
            "https://example.com/path/to/file.ext".to_string(),
        );

        assert_eq!(
            fallback_reference_label("", "https://example.com/path/to/file.ext", &target),
            "example.com"
        );
    }

    #[test]
    fn fallback_label_prefers_local_file_name_when_description_missing() {
        let target = InfoPanelReferenceTarget::LocalPath(PathBuf::from("/tmp/path/to/file.ext"));

        assert_eq!(
            fallback_reference_label("", "/tmp/path/to/file.ext", &target),
            "file.ext"
        );
    }

    #[test]
    fn reference_scheme_ignores_windows_drive_letters() {
        assert_eq!(reference_scheme("C:\\notes\\a.md"), None);
        assert_eq!(reference_scheme("https://example.com"), Some("https"));
    }

    #[test]
    fn external_reference_host_extracts_mail_domain() {
        assert_eq!(
            external_reference_host("mailto:hello@example.com"),
            Some("example.com".to_string())
        );
    }

    #[test]
    fn path_reference_name_uses_file_name() {
        assert_eq!(
            path_reference_name(&PathBuf::from("./notes/file.md"), "./notes/file.md"),
            "file.md"
        );
    }

    #[test]
    fn pulldown_cmark_exposes_double_brackets_as_wiki_links() {
        let mut parser = Parser::new_ext("[[foo/bar.md]]", Options::all()).into_offset_iter();
        let first_link = parser.find_map(|(event, _)| match event {
            Event::Start(Tag::Link { link_type, .. }) => Some(link_type),
            _ => None,
        });

        assert!(matches!(first_link, Some(LinkType::WikiLink { .. })));
    }

    #[test]
    fn html_export_includes_print_styles_and_base_href() {
        let html = build_html_document(
            "Note",
            "![img](./assets/pic.png)",
            Some(&PathBuf::from("/tmp/demo/note.md")),
        );

        assert!(html.contains("@page{size:A4;margin:18mm;}"));
        assert!(html.contains("<base href=\"file:///tmp/demo/\">"));
    }

    #[test]
    fn diff_rows_emit_removed_line_before_added_line_for_replacements() {
        let rows = build_git_diff_rows(
            crate::i18n::UiLanguage::English,
            "alpha\nbefore\nomega\n",
            "alpha\nafter\nomega\n",
            true,
            None,
        );

        assert_eq!(rows.len(), 4);
        assert!(matches!(rows[0].kind, GitDiffRowKind::Context));
        assert_eq!(rows[0].text, "alpha");
        assert!(matches!(rows[1].kind, GitDiffRowKind::Removed));
        assert_eq!(rows[1].text, "before");
        assert!(matches!(rows[2].kind, GitDiffRowKind::Added));
        assert_eq!(rows[2].text, "after");
        assert!(matches!(rows[3].kind, GitDiffRowKind::Context));
        assert_eq!(rows[3].text, "omega");
    }

    #[test]
    fn diff_snapshot_text_preserves_row_order() {
        let rows = vec![
            GitDiffRow {
                kind: GitDiffRowKind::Context,
                text: "alpha".to_string(),
                decoration: crate::markdown::LineDecoration::default(),
                source_line_number: Some(1),
                is_hunk_label: false,
            },
            GitDiffRow {
                kind: GitDiffRowKind::Removed,
                text: "before".to_string(),
                decoration: crate::markdown::LineDecoration {
                    diff_removed_spans: vec![0..6],
                    ..crate::markdown::LineDecoration::default()
                },
                source_line_number: Some(2),
                is_hunk_label: false,
            },
            GitDiffRow {
                kind: GitDiffRowKind::Added,
                text: "after".to_string(),
                decoration: crate::markdown::LineDecoration {
                    diff_added_spans: vec![0..5],
                    ..crate::markdown::LineDecoration::default()
                },
                source_line_number: Some(2),
                is_hunk_label: false,
            },
        ];

        assert_eq!(build_git_diff_snapshot_text(&rows), "alpha\nbefore\nafter");
    }

    #[test]
    fn collapsed_diff_rows_insert_line_label_before_hunk() {
        let rows = vec![
            GitDiffRow {
                kind: GitDiffRowKind::Context,
                text: "alpha".to_string(),
                decoration: crate::markdown::LineDecoration::default(),
                source_line_number: Some(11),
                is_hunk_label: false,
            },
            GitDiffRow {
                kind: GitDiffRowKind::Removed,
                text: "before".to_string(),
                decoration: crate::markdown::LineDecoration::default(),
                source_line_number: Some(12),
                is_hunk_label: false,
            },
            GitDiffRow {
                kind: GitDiffRowKind::Added,
                text: "after".to_string(),
                decoration: crate::markdown::LineDecoration::default(),
                source_line_number: Some(12),
                is_hunk_label: false,
            },
        ];

        let visible = visible_git_diff_rows(&rows, true);
        assert_eq!(visible.len(), 3);
        assert_eq!(visible[0].text, "L12");
        assert!(visible[0].is_hunk_label);
        assert_eq!(visible[1].text, "before");
        assert_eq!(visible[2].text, "after");
    }

    #[test]
    fn collapsed_diff_rows_keep_full_table_block_when_changed() {
        let rows = build_git_diff_rows(
            crate::i18n::UiLanguage::English,
            "| Name | Value |\n| --- | --- |\n| keep | 1 |\n",
            "| Name | Value |\n| --- | --- |\n| keep | 2 |\n",
            true,
            None,
        );

        let visible = visible_git_diff_rows(&rows, true);
        assert_eq!(visible.len(), 5);
        assert_eq!(visible[0].text, "L1");
        assert!(visible[0].is_hunk_label);
        assert_eq!(visible[1].text, "| Name | Value |");
        assert_eq!(visible[2].text, "| --- | --- |");
        assert_eq!(visible[3].text, "| keep | 1 |");
        assert_eq!(visible[4].text, "| keep | 2 |");
    }

    #[test]
    fn diff_rows_use_empty_state_for_identical_empty_text() {
        let rows = build_git_diff_rows(crate::i18n::UiLanguage::English, "", "", true, None);

        assert_eq!(rows.len(), 1);
        assert!(matches!(rows[0].kind, GitDiffRowKind::Context));
        assert_eq!(
            rows[0].text,
            crate::i18n::translate(
                crate::i18n::UiLanguage::English,
                crate::i18n::TextKey::NoTextDiff
            )
        );
        assert!(rows[0].decoration.diff_added_spans.is_empty());
        assert!(rows[0].decoration.diff_removed_spans.is_empty());
    }

    #[test]
    fn diff_rows_preserve_multibyte_replacement_order() {
        let rows = build_git_diff_rows(
            crate::i18n::UiLanguage::ChineseSimplified,
            "你好\n世界\n",
            "您好\n世界\n",
            true,
            None,
        );

        assert_eq!(rows.len(), 3);
        assert!(matches!(rows[0].kind, GitDiffRowKind::Removed));
        assert_eq!(rows[0].text, "你好");
        assert!(matches!(rows[1].kind, GitDiffRowKind::Added));
        assert_eq!(rows[1].text, "您好");
        assert!(matches!(rows[2].kind, GitDiffRowKind::Context));
        assert_eq!(rows[2].text, "世界");
    }

    #[test]
    fn diff_rows_capture_inline_highlights_for_replacements() {
        let rows = build_git_diff_rows(
            crate::i18n::UiLanguage::English,
            "prefix old suffix\n",
            "prefix new suffix\n",
            true,
            None,
        );

        assert_eq!(rows.len(), 2);
        assert!(matches!(rows[0].kind, GitDiffRowKind::Removed));
        assert_eq!(rows[0].decoration.diff_removed_spans, vec![7..10]);
        assert!(matches!(rows[1].kind, GitDiffRowKind::Added));
        assert_eq!(rows[1].decoration.diff_added_spans, vec![7..10]);
    }

    #[test]
    fn diff_rows_highlight_entire_unpaired_insertions() {
        let rows = build_git_diff_rows(
            crate::i18n::UiLanguage::English,
            "",
            "**new**\n",
            true,
            None,
        );

        assert_eq!(rows.len(), 1);
        assert!(matches!(rows[0].kind, GitDiffRowKind::Added));
        assert_eq!(rows[0].decoration.diff_added_spans, vec![0..7]);
    }

    #[test]
    fn diff_rows_preserve_code_block_decorations() {
        let rows = build_git_diff_rows(
            crate::i18n::UiLanguage::English,
            "```rust\nlet old_value = 1;\n```\n",
            "```rust\nlet new_value = 2;\n```\n",
            true,
            None,
        );

        assert_eq!(rows.len(), 4);
        assert!(matches!(
            rows[0].decoration.block_type,
            crate::markdown::BlockType::CodeBlockFence
        ));
        assert!(matches!(
            rows[1].decoration.block_type,
            crate::markdown::BlockType::CodeBlock
        ));
        assert!(matches!(
            rows[2].decoration.block_type,
            crate::markdown::BlockType::CodeBlock
        ));
        assert!(matches!(
            rows[3].decoration.block_type,
            crate::markdown::BlockType::CodeBlockFence
        ));
    }

    #[test]
    fn diff_rows_preserve_ordered_and_unordered_list_decorations() {
        let unordered_rows = build_git_diff_rows(
            crate::i18n::UiLanguage::English,
            "- old item\n",
            "- new item\n",
            true,
            None,
        );
        assert!(matches!(
            unordered_rows[0].decoration.block_type,
            crate::markdown::BlockType::UnorderedList
        ));
        assert!(matches!(
            unordered_rows[1].decoration.block_type,
            crate::markdown::BlockType::UnorderedList
        ));

        let ordered_rows = build_git_diff_rows(
            crate::i18n::UiLanguage::English,
            "1. old item\n",
            "1. new item\n",
            true,
            None,
        );
        assert!(matches!(
            ordered_rows[0].decoration.block_type,
            crate::markdown::BlockType::OrderedList
        ));
        assert!(matches!(
            ordered_rows[1].decoration.block_type,
            crate::markdown::BlockType::OrderedList
        ));
    }

    #[test]
    fn diff_rows_preserve_image_decorations() {
        let rows = build_git_diff_rows(
            crate::i18n::UiLanguage::English,
            "![old](assets/old.png)\n",
            "![new](assets/new.png)\n",
            true,
            None,
        );

        assert_eq!(
            rows[0].decoration.image_path.as_deref(),
            Some("assets/old.png")
        );
        assert_eq!(
            rows[1].decoration.image_path.as_deref(),
            Some("assets/new.png")
        );
    }

    #[test]
    fn diff_rows_normalize_table_metadata_for_replacements() {
        let rows = build_git_diff_rows(
            crate::i18n::UiLanguage::English,
            "| Name | Value |\n| --- | --- |\n| old | 1 |\n",
            "| Name | Value |\n| --- | --- |\n| new | 22 |\n",
            true,
            None,
        );

        assert_eq!(rows.len(), 4);
        for row in &rows {
            let table_info = row
                .decoration
                .table_info
                .as_ref()
                .expect("table row decoration");
            assert_eq!(table_info.table_start_line, 0);
            assert_eq!(table_info.table_end_line, 4);
            assert_eq!(table_info.column_widths.len(), 2);
        }
    }

    #[test]
    fn diff_rows_normalize_math_block_preview_metadata_for_replacements() {
        let rows = build_git_diff_rows(
            crate::i18n::UiLanguage::English,
            "$$\na + b\n$$\n",
            "$$\na + c\n$$\n",
            true,
            None,
        );

        let removed_preview = rows[0]
            .decoration
            .block_preview
            .as_ref()
            .expect("removed math preview");
        let added_preview = rows[2]
            .decoration
            .block_preview
            .as_ref()
            .expect("added math preview");

        assert!(matches!(rows[0].kind, GitDiffRowKind::Removed));
        assert!(removed_preview.is_anchor);
        assert_eq!(removed_preview.start_line, 0);
        assert_eq!(removed_preview.end_line, 1);

        assert!(matches!(rows[1].kind, GitDiffRowKind::Removed));
        assert!(
            !rows[1]
                .decoration
                .block_preview
                .as_ref()
                .expect("removed math continuation")
                .is_anchor
        );

        assert!(matches!(rows[2].kind, GitDiffRowKind::Added));
        assert!(added_preview.is_anchor);
        assert_eq!(added_preview.start_line, 2);
        assert_eq!(added_preview.end_line, 3);
    }

    #[test]
    fn diff_rows_normalize_mermaid_block_preview_metadata_for_replacements() {
        let rows = build_git_diff_rows(
            crate::i18n::UiLanguage::English,
            "```mermaid\ngraph TD\nA-->B\n```\n",
            "```mermaid\ngraph TD\nA-->C\n```\n",
            true,
            None,
        );

        let removed_preview = rows[0]
            .decoration
            .block_preview
            .as_ref()
            .expect("removed mermaid preview");
        let added_preview = rows[3]
            .decoration
            .block_preview
            .as_ref()
            .expect("added mermaid preview");

        assert!(removed_preview.is_anchor);
        assert_eq!(removed_preview.start_line, 0);
        assert_eq!(removed_preview.end_line, 2);

        assert!(added_preview.is_anchor);
        assert_eq!(added_preview.start_line, 3);
        assert_eq!(added_preview.end_line, 5);
    }
}
