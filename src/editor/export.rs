//! Document export orchestration and portable Markdown/HTML/PDF conversion.
use super::*;
use gpui::Context;
#[cfg(not(target_os = "macos"))]
use printpdf::{Mm, PdfDocument};
use pulldown_cmark::{
    BlockQuoteKind, CodeBlockKind, CowStr, Event, LinkType, Options, Parser, Tag, TagEnd,
    html::push_html,
};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
#[cfg(not(target_os = "macos"))]
use unicode_segmentation::UnicodeSegmentation;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos::*;

#[derive(Clone, Copy)]
pub(crate) enum ExportFormat {
    Markdown,
    Html,
    Pdf,
    Png,
}

impl ExportFormat {
    #[cfg(not(target_os = "macos"))]
    fn label(self) -> &'static str {
        match self {
            Self::Markdown => "Markdown",
            Self::Html => "HTML",
            Self::Pdf => "PDF",
            Self::Png => "PNG",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Markdown => "md",
            Self::Html => "html",
            Self::Pdf => "pdf",
            Self::Png => "png",
        }
    }
}

const EXPORT_PAGE_MARGIN_PX: f64 = 51.0;
const CSS_PX_TO_PDF_PT: f64 = 72.0 / 96.0;

/// Export at the configured content width, independent of pane size and zoom.
/// Pages grow with that width instead of squeezing the content into fixed A4.
#[derive(Clone, Copy, Debug)]
struct ExportLayout {
    content_width_px: f64,
    page_width_px: f64,
    page_height_px: f64,
}

impl ExportLayout {
    fn from_settings(settings: &AppSettings) -> Self {
        let content_width_px = f64::from(if settings.editor_content_width.is_finite() {
            settings.effective_editor_content_width()
        } else {
            AppSettings::default().effective_editor_content_width()
        });
        let page_width_px = content_width_px + 2.0 * EXPORT_PAGE_MARGIN_PX;
        Self {
            content_width_px,
            page_width_px,
            page_height_px: page_width_px * 297.0 / 210.0,
        }
    }

    #[cfg(any(target_os = "macos", test))]
    fn content_height_px(self) -> f64 {
        self.page_height_px - 2.0 * EXPORT_PAGE_MARGIN_PX
    }

    #[cfg(any(target_os = "macos", test))]
    fn png_snapshot_width_pt(self, backing_scale: f64) -> f64 {
        self.page_width_px * 2.0 / backing_scale.max(1.0)
    }
}

#[cfg(not(target_os = "macos"))]
fn export_dialog_path(default_name: &str, title: &str) -> Option<std::path::PathBuf> {
    let default_name = default_name.to_string();
    let title = title.to_string();
    crate::platform::dialog::run(move || {
        rfd::FileDialog::new()
            .set_title(title)
            .set_file_name(default_name)
            .save_file()
    })
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
    let path_text = path.to_string_lossy();
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
    render_export_preview_svg_with_font_families(kind, source, export_preview_font_families())
}

fn render_export_preview_svg_with_font_families(
    kind: crate::markdown::BlockPreviewKind,
    source: &str,
    font_families: helpers::GeneratedPreviewFontFamilies<'_>,
) -> String {
    let theme = crate::theme::ThemeKind::PaperWhite.build();
    helpers::build_generated_block_preview_svg(kind, source, &theme, font_families)
}

fn export_code_block_preview_html(
    kind: &CodeBlockKind<'_>,
    source: &str,
    font_families: helpers::GeneratedPreviewFontFamilies<'_>,
) -> Option<String> {
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

    let svg = render_export_preview_svg_with_font_families(preview_kind, source, font_families);
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
    if matches!(tag, "sup" | "sub")
        && inner
            .chars()
            .any(|ch| ch.is_whitespace() || matches!(ch, '[' | ']'))
    {
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

fn protected_export_source_ranges(markdown: &str, options: Options) -> Vec<Range<usize>> {
    enum OpenSource {
        CodeBlock(usize),
        Link(usize, bool),
        Image(usize),
        Html(String, usize),
        HtmlBlock(usize),
    }

    let mut ranges = Vec::new();
    let mut open = Vec::new();
    for (event, source_range) in Parser::new_ext(markdown, options).into_offset_iter() {
        match event {
            Event::Start(Tag::CodeBlock(_)) => open.push(OpenSource::CodeBlock(source_range.start)),
            Event::Start(Tag::HtmlBlock) => open.push(OpenSource::HtmlBlock(source_range.start)),
            Event::End(TagEnd::HtmlBlock) => {
                if let Some(position) = open
                    .iter()
                    .rposition(|item| matches!(item, OpenSource::HtmlBlock(_)))
                    && let OpenSource::HtmlBlock(start) = open.remove(position)
                {
                    ranges.push(start..source_range.end);
                }
            }
            Event::Start(Tag::Link { link_type, .. }) => open.push(OpenSource::Link(
                source_range.start,
                matches!(link_type, LinkType::WikiLink { .. }),
            )),
            Event::Start(Tag::Image { .. }) => open.push(OpenSource::Image(source_range.start)),
            Event::End(TagEnd::CodeBlock) => {
                if let Some(position) = open
                    .iter()
                    .rposition(|item| matches!(item, OpenSource::CodeBlock(_)))
                    && let OpenSource::CodeBlock(start) = open.remove(position)
                {
                    ranges.push(start..source_range.end);
                }
            }
            Event::End(TagEnd::Link) => {
                if let Some(position) = open
                    .iter()
                    .rposition(|item| matches!(item, OpenSource::Link(_, _)))
                    && let OpenSource::Link(start, is_wiki) = open.remove(position)
                {
                    let end = source_range.end.min(markdown.len());
                    if let Some(source) = markdown.get(start..end) {
                        let target_range = if is_wiki {
                            let inner_end = source.len().saturating_sub(2);
                            (source.starts_with("[[") && source.ends_with("]]")).then(|| {
                                let target_end = source[2..inner_end]
                                    .find('|')
                                    .map_or(inner_end, |separator| 2 + separator);
                                2..target_end
                            })
                        } else {
                            crate::markdown::markdown_inline_target_range(source)
                        };
                        if let Some(target_range) = target_range {
                            ranges.push(start + target_range.start..start + target_range.end);
                        }
                    }
                }
            }
            Event::End(TagEnd::Image) => {
                if let Some(position) = open
                    .iter()
                    .rposition(|item| matches!(item, OpenSource::Image(_)))
                    && let OpenSource::Image(start) = open.remove(position)
                {
                    ranges.push(start..source_range.end);
                }
            }
            Event::Code(_) | Event::InlineMath(_) | Event::DisplayMath(_) | Event::Html(_) => {
                ranges.push(source_range);
            }
            Event::InlineHtml(source) => {
                if let Some((tag, closing, self_closing)) =
                    crate::markdown::protected_inline_html_tag(source.as_ref())
                {
                    if self_closing {
                        ranges.push(source_range);
                    } else if closing {
                        if let Some(position) = open.iter().rposition(
                            |item| matches!(item, OpenSource::Html(open_tag, _) if *open_tag == tag),
                        ) && let OpenSource::Html(_, start) = open.remove(position)
                        {
                            ranges.push(start..source_range.end);
                        }
                    } else {
                        open.push(OpenSource::Html(tag, source_range.start));
                    }
                }
            }
            _ => {}
        }
    }
    ranges.sort_by_key(|range| range.start);
    ranges
}

fn escaped_custom_syntax_placeholder(ch: char) -> Option<char> {
    match ch {
        '=' => Some('\u{e000}'),
        '+' => Some('\u{e001}'),
        '^' => Some('\u{e002}'),
        '~' => Some('\u{e003}'),
        ':' => Some('\u{e004}'),
        _ => None,
    }
}

fn restore_escaped_custom_syntax(text: &str) -> String {
    text.chars()
        .map(|ch| match ch {
            '\u{e000}' => '=',
            '\u{e001}' => '+',
            '\u{e002}' => '^',
            '\u{e003}' => '~',
            '\u{e004}' => ':',
            _ => ch,
        })
        .collect()
}

fn mask_escaped_custom_syntax(markdown: &str, options: Options) -> String {
    let protected = protected_export_source_ranges(markdown, options);
    let mut masked = String::with_capacity(markdown.len());
    let mut index = 0usize;
    while index < markdown.len() {
        let Some(ch) = markdown[index..].chars().next() else {
            break;
        };
        if ch == '\\'
            && !crate::markdown::is_escaped_at(markdown, index)
            && !protected
                .iter()
                .any(|range| range.start <= index && range.end > index)
        {
            let next_index = index + ch.len_utf8();
            if let Some(next) = markdown[next_index..].chars().next()
                && let Some(placeholder) = escaped_custom_syntax_placeholder(next)
            {
                masked.push(placeholder);
                index = next_index + next.len_utf8();
                continue;
            }
        }
        masked.push(ch);
        index += ch.len_utf8();
    }
    masked
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
fn build_callout_html(
    kind: BlockQuoteKind,
    title: &str,
    body_events: Vec<Event<'_>>,
    font_families: helpers::GeneratedPreviewFontFamilies<'_>,
) -> String {
    let (kind_class, kind_label, icon) = callout_assets(kind);
    let icon = icon.replacen(
        "<svg ",
        "<svg viewBox=\"0 0 24 24\" aria-hidden=\"true\" ",
        1,
    );

    let mut body_html = String::new();
    push_html(
        &mut body_html,
        transform_export_events(body_events.into_iter(), font_families).into_iter(),
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
fn try_render_callout(
    inner: &[Event<'_>],
    font_families: helpers::GeneratedPreviewFontFamilies<'_>,
) -> Option<String> {
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

    Some(build_callout_html(kind, title, body_events, font_families))
}

/// Walks pulldown's event stream and rewrites the constructs the export needs to
/// render itself: fenced code-block previews (mermaid/math), math, the editor's
/// custom inline syntaxes, and `[!KIND]` callouts. Blockquotes are buffered so a
/// callout can be detected from their first line; the function recurses for
/// blockquote bodies so nested constructs are handled too.
fn transform_export_events<'a>(
    parser: impl Iterator<Item = Event<'a>>,
    font_families: helpers::GeneratedPreviewFontFamilies<'_>,
) -> Vec<Event<'a>> {
    let mut events: Vec<Event<'a>> = Vec::new();
    let mut pending_code_block: Option<(CodeBlockKind<'a>, Vec<Event<'a>>, String)> = None;
    let mut pending_blockquote: Option<(usize, Option<BlockQuoteKind>, Vec<Event<'a>>)> = None;
    let mut custom_syntax_suppression_depth = 0usize;

    for event in parser {
        if let Some((depth, kind, mut buffer)) = pending_blockquote.take() {
            match &event {
                Event::Start(Tag::BlockQuote(_)) => {
                    buffer.push(event);
                    pending_blockquote = Some((depth + 1, kind, buffer));
                }
                Event::End(TagEnd::BlockQuote(_)) if depth == 1 => {
                    if let Some(callout_html) = try_render_callout(&buffer, font_families) {
                        events.push(Event::Html(CowStr::Boxed(callout_html.into_boxed_str())));
                    } else if let Some(alert_kind) = kind {
                        // A GitHub-style alert (`> [!KIND]` on its own line):
                        // pulldown already parsed the kind and stripped the
                        // marker, so render the buffered body as a callout.
                        let callout_html =
                            build_callout_html(alert_kind, "", buffer, font_families);
                        events.push(Event::Html(CowStr::Boxed(callout_html.into_boxed_str())));
                    } else {
                        events.push(Event::Start(Tag::BlockQuote(kind)));
                        events.extend(transform_export_events(buffer.into_iter(), font_families));
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
                if let Some(preview_html) =
                    export_code_block_preview_html(&kind, &source, font_families)
                {
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
            Event::Start(Tag::Image { .. }) => {
                custom_syntax_suppression_depth += 1;
                events.push(event);
            }
            Event::End(TagEnd::Image) => {
                custom_syntax_suppression_depth = custom_syntax_suppression_depth.saturating_sub(1);
                events.push(event);
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
            Event::InlineHtml(ref source) => {
                if let Some((_tag, closing, self_closing)) =
                    crate::markdown::protected_inline_html_tag(source.as_ref())
                {
                    if closing {
                        custom_syntax_suppression_depth =
                            custom_syntax_suppression_depth.saturating_sub(1);
                    } else if !self_closing {
                        custom_syntax_suppression_depth += 1;
                    }
                }
                events.push(event);
            }
            Event::Text(content) => {
                // pulldown emits the editor's custom inline syntaxes (highlight,
                // underline, super/subscript, strikethrough, emoji shortcodes)
                // as plain text; convert them to inline HTML so the export
                // matches the editor.
                if custom_syntax_suppression_depth == 0
                    && let Some(html) = convert_inline_custom_syntax(content.as_ref())
                {
                    events.push(Event::Html(CowStr::Boxed(
                        restore_escaped_custom_syntax(&html).into_boxed_str(),
                    )));
                } else {
                    events.push(Event::Text(CowStr::Boxed(
                        restore_escaped_custom_syntax(content.as_ref()).into_boxed_str(),
                    )));
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
        events.extend(transform_export_events(buffer.into_iter(), font_families));
        events.push(Event::End(TagEnd::BlockQuote(kind)));
    }

    events
}

#[cfg(test)]
fn render_markdown_for_export(markdown: &str) -> String {
    render_markdown_for_export_with_font_families(markdown, export_preview_font_families())
}

fn render_markdown_for_export_with_font_families(
    markdown: &str,
    font_families: helpers::GeneratedPreviewFontFamilies<'_>,
) -> String {
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
    let markdown = markdown_with_explicit_table_boundaries(markdown);
    let markdown = markdown_with_export_math_blocks(&markdown, options);
    let masked = mask_escaped_custom_syntax(&markdown, options);
    let inline_source = crate::markdown::CjkInlineSource::new(&masked, || {
        Parser::new_ext(&masked, options).into_offset_iter()
    });
    let parser = Parser::new_with_broken_link_callback(
        inline_source.as_str(),
        options,
        Some(|link: pulldown_cmark::BrokenLink<'_>| inline_source.resolve_reference(&link)),
    )
    .into_offset_iter()
    .map(|(event, range)| inline_source.restore_event(event, range));
    let mut html = String::new();
    push_html(
        &mut html,
        transform_export_events(parser, font_families).into_iter(),
    );
    html
}

/// Pulldown handles native dollar math, but not the editor's bracket syntax or
/// dollar blocks interrupted by blank paragraphs. Adapt only editor-owned
/// preview ranges into fenced math, keeping TeX opaque to Markdown and masking.
fn markdown_with_export_math_blocks(markdown: &str, options: Options) -> std::borrow::Cow<'_, str> {
    let decorations = crate::markdown::parse_decorations(markdown);
    let protected = protected_export_source_ranges(markdown, options);
    let mut output = String::new();
    let mut cursor = 0;
    for preview in decorations
        .iter()
        .filter_map(|decoration| decoration.block_preview.as_ref())
    {
        if !preview.is_anchor || preview.kind != crate::markdown::BlockPreviewKind::Math {
            continue;
        }
        let range = &preview.source_range;
        let Some(source) = markdown.get(range.clone()) else {
            continue;
        };
        if range.start < cursor
            || !(source.starts_with("\\[") || source.starts_with("$$"))
            || protected
                .iter()
                .any(|protected| protected.start <= range.start && range.start < protected.end)
        {
            continue;
        }
        let line_start = markdown[..range.start]
            .rfind('\n')
            .map_or(0, |newline| newline + 1);
        let indent = &markdown[line_start..range.start];
        if !indent.chars().all(|ch| matches!(ch, ' ' | '\t')) {
            continue;
        }
        output.push_str(&markdown[cursor..line_start]);
        if !output.is_empty() && !output.ends_with("\n\n") {
            if !output.ends_with('\n') {
                output.push('\n');
            }
            output.push('\n');
        }
        let fence_length = preview
            .source_text
            .split(|ch| ch != '~')
            .map(str::len)
            .max()
            .unwrap_or(0)
            + 1;
        let fence = "~".repeat(fence_length.max(3));
        output.push_str(indent);
        output.push_str(&fence);
        output.push_str("math\n");
        for line in preview.source_text.split('\n') {
            output.push_str(indent);
            output.push_str(line);
            output.push('\n');
        }
        output.push_str(indent);
        output.push_str(&fence);
        output.push('\n');
        cursor = range.end;
    }
    if cursor == 0 {
        std::borrow::Cow::Borrowed(markdown)
    } else {
        output.push_str(&markdown[cursor..]);
        std::borrow::Cow::Owned(output)
    }
}

fn markdown_with_explicit_table_boundaries(markdown: &str) -> std::borrow::Cow<'_, str> {
    let decorations = crate::markdown::parse_decorations(markdown);
    let mut output = String::with_capacity(markdown.len());
    let mut changed = false;
    let mut previous_was_table = false;
    let mut previous_was_blank = true;

    for (line, source) in markdown.split_inclusive('\n').enumerate() {
        let content = source.trim_end_matches(['\r', '\n']);
        let is_blank = content.trim().is_empty();
        let is_table = decorations
            .get(line)
            .and_then(|decoration| decoration.table_info.as_ref())
            .is_some_and(|table| !table.is_front_matter);
        if line > 0 && is_table != previous_was_table && !previous_was_blank && !is_blank {
            output.push('\n');
            changed = true;
        }
        output.push_str(source);
        previous_was_table = is_table;
        previous_was_blank = is_blank;
    }

    if changed {
        std::borrow::Cow::Owned(output)
    } else {
        std::borrow::Cow::Borrowed(markdown)
    }
}

#[cfg(test)]
fn build_html_document(
    title: &str,
    markdown: &str,
    source_file_path: Option<&Path>,
    layout: ExportLayout,
) -> String {
    build_html_document_with_font_families(
        title,
        markdown,
        source_file_path,
        layout,
        export_preview_font_families(),
    )
}

fn build_html_document_with_font_families(
    title: &str,
    markdown: &str,
    source_file_path: Option<&Path>,
    layout: ExportLayout,
    font_families: helpers::GeneratedPreviewFontFamilies<'_>,
) -> String {
    let html = render_markdown_for_export_with_font_families(markdown, font_families);

    let escaped_title = html_escape(title);
    let base_href = html_base_href(source_file_path)
        .map(|href| format!("<base href=\"{}\">", html_escape(&href)))
        .unwrap_or_default();

    let github_markdown_css = concat!(
        "html,body{margin:0;padding:0;}.markdown-body{box-sizing:border-box;min-width:0;width:100%;max-width:var(--export-page-width);margin:0 auto;padding:var(--export-page-margin);color:#1f2328;background:#fff;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI','Noto Sans',Helvetica,Arial,sans-serif,'Apple Color Emoji','Segoe UI Emoji';font-size:16px;line-height:1.5;word-wrap:break-word;}",
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
        ".markdown-body a{color:#0969da;text-decoration:none;overflow-wrap:anywhere;}.markdown-body a:hover{text-decoration:underline;}",
        ".markdown-body blockquote{margin-left:0;margin-right:0;padding:0 1em;color:#57606a;border-left:.25em solid #d0d7de;}",
        ".markdown-body code,.markdown-body tt{padding:.2em .4em;margin:0;font-size:85%;white-space:break-spaces;background-color:rgba(175,184,193,.2);border-radius:6px;}",
        ".markdown-body pre{padding:16px;overflow:auto;font-size:85%;line-height:1.45;background-color:#f6f8fa;border-radius:6px;}",
        ".markdown-body pre code{padding:0;margin:0;font-size:100%;white-space:pre-wrap;word-break:break-word;background:transparent;border:0;}",
        ".markdown-body pre{font-family:ui-monospace,SFMono-Regular,'SF Mono',Menlo,Consolas,'Liberation Mono',monospace;border:1px solid #d0d7de;}",
        ".markdown-body table{display:block;width:max-content;max-width:100%;overflow:auto;border-spacing:0;border-collapse:collapse;}",
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
            ":root{{--export-content-width:{content_width}px;--export-page-width:{page_width}px;--export-page-margin:{page_margin}px;}}",
            "{}",
            "@media(max-width:{responsive_width}px){{.markdown-body{{padding:15px;}}}}",
            "@page{{size:{page_width}px {page_height}px;margin:{page_margin}px;}}",
            "@media print{{html,body{{background:#fff;}}body{{margin:0;}}.markdown-body{{max-width:none;padding:0;font-size:12pt;line-height:1.55;}}.markdown-body a{{color:inherit;text-decoration:none;}}.markdown-body h1,.markdown-body h2,.markdown-body h3,.markdown-body h4,.markdown-body h5,.markdown-body h6{{break-after:avoid-page;page-break-after:avoid;}}.markdown-body pre,.markdown-body blockquote,.markdown-body table,.markdown-body img,.markdown-body svg,.export-preview{{break-inside:avoid;page-break-inside:avoid;}}.markdown-body pre{{white-space:pre-wrap;word-break:break-word;}}.markdown-body table{{display:table;width:100%;overflow:visible;}}.markdown-body table th,.markdown-body table td{{word-break:break-word;}}.markdown-body tr{{break-inside:avoid;page-break-inside:avoid;}}.markdown-body p{{orphans:3;widows:3;}}}}",
            "</style></head><body><article class=\"markdown-body\">{}</article></body></html>"
        ),
        escaped_title,
        base_href,
        github_markdown_css,
        html,
        content_width = layout.content_width_px,
        page_width = layout.page_width_px,
        page_height = layout.page_height_px,
        page_margin = EXPORT_PAGE_MARGIN_PX,
        responsive_width = layout.page_width_px - 1.0,
    )
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
fn build_pdf_document_with_printpdf(
    title: &str,
    markdown: &str,
    layout: ExportLayout,
) -> anyhow::Result<Vec<u8>> {
    let px_to_mm = CSS_PX_TO_PDF_PT as f32 * 25.4 / 72.0;
    let page_width_mm = layout.page_width_px as f32 * px_to_mm;
    let page_height_mm = layout.page_height_px as f32 * px_to_mm;
    let (document, first_page, first_layer) =
        PdfDocument::new(title, Mm(page_width_mm), Mm(page_height_mm), "Layer 1");

    // Existing on disk is not enough: collections and variable fonts parse
    // only on some backends, so keep trying until one actually loads.
    let font = crate::platform::fonts::pdf_font_candidates()
        .into_iter()
        .find_map(|path| {
            fs::File::open(&path)
                .ok()
                .and_then(|file| document.add_external_font(file).ok())
        })
        .ok_or_else(|| anyhow::anyhow!("could not find a usable system font for PDF export"))?;

    let margin_x_mm = EXPORT_PAGE_MARGIN_PX as f32 * px_to_mm;
    let margin_y_mm = margin_x_mm;
    let line_height_mm = 6.0;
    let font_size_pt = 11.0;
    let unit_width_px = font_size_pt / CSS_PX_TO_PDF_PT as f32 * 0.6;
    let max_units = (layout.content_width_px as f32 / unit_width_px)
        .floor()
        .max(1.0) as usize;

    let rendered_text = rendered_plain_text_for_export(markdown);
    let lines = wrap_text_for_pdf(&rendered_text, max_units);
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

#[cfg(any(not(target_os = "macos"), test))]
fn rendered_plain_text_for_export(markdown: &str) -> String {
    let mut out = String::with_capacity(markdown.len());
    for event in Parser::new_ext(markdown, Options::all()) {
        match event {
            Event::Text(text) | Event::Code(text) => out.push_str(&text),
            Event::InlineMath(math) => out.push_str(&math),
            Event::DisplayMath(math) => {
                push_export_newlines(&mut out, 1);
                out.push_str(&math);
                push_export_newlines(&mut out, 2);
            }
            Event::SoftBreak | Event::HardBreak => push_export_newlines(&mut out, 1),
            Event::Rule => {
                push_export_newlines(&mut out, 1);
                out.push_str("--------------------");
                push_export_newlines(&mut out, 2);
            }
            Event::TaskListMarker(checked) => {
                out.push_str(if checked { "[x] " } else { "[ ] " });
            }
            Event::FootnoteReference(label) => out.push_str(&label),
            Event::End(
                TagEnd::Paragraph
                | TagEnd::Heading(_)
                | TagEnd::CodeBlock
                | TagEnd::BlockQuote(_)
                | TagEnd::FootnoteDefinition,
            ) => push_export_newlines(&mut out, 2),
            Event::End(TagEnd::Item | TagEnd::TableRow) => push_export_newlines(&mut out, 1),
            Event::End(TagEnd::TableCell) => out.push('\t'),
            Event::End(TagEnd::List(_) | TagEnd::Table | TagEnd::TableHead) => {
                push_export_newlines(&mut out, 2)
            }
            Event::Start(Tag::CodeBlock(_)) => push_export_newlines(&mut out, 1),
            Event::Html(html) | Event::InlineHtml(html) => {
                let tag = html.trim().to_ascii_lowercase();
                if matches!(tag.as_str(), "<br>" | "<br/>" | "<br />") {
                    push_export_newlines(&mut out, 1);
                }
            }
            Event::Start(_) | Event::End(_) => {}
        }
    }
    out.trim().to_string()
}

#[cfg(any(not(target_os = "macos"), test))]
fn push_export_newlines(out: &mut String, count: usize) {
    let existing = out.chars().rev().take_while(|ch| *ch == '\n').count();
    out.extend(std::iter::repeat_n('\n', count.saturating_sub(existing)));
}

#[cfg(not(target_os = "macos"))]
fn build_pdf_document(
    title: &str,
    markdown: &str,
    _source_file_path: Option<&Path>,
    layout: ExportLayout,
) -> anyhow::Result<Vec<u8>> {
    build_pdf_document_with_printpdf(title, markdown, layout)
}

impl EditorView {
    fn export_default_name_for_path(&self, source_path: Option<&Path>, extension: &str) -> String {
        let title = source_path
            .as_deref()
            .and_then(Path::file_stem)
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .unwrap_or(self.t(crate::i18n::TextKey::Untitled));
        format!("{}.{}", title, extension)
    }

    fn open_export_text_for_path(&self, path: &Path) -> Option<std::io::Result<String>> {
        if self.current_file_path.as_deref() == Some(path) && self.active_tab_supports_text_buffer()
        {
            return Some(Ok(self.buffer.text().to_string()));
        }
        if let Some(tab) = self
            .all_tabs()
            .find(|tab| tab.path.as_deref() == Some(path))
        {
            if tab
                .file_kind
                .as_ref()
                .is_some_and(FileKind::supports_text_buffer)
            {
                return Some(Ok(tab.snapshot.text.clone()));
            }
            return Some(Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "file does not contain exportable text",
            )));
        }
        None
    }

    pub(crate) fn export_path(
        &mut self,
        path: PathBuf,
        format: ExportFormat,
        cx: &mut Context<Self>,
    ) {
        if let Some(result) = self.open_export_text_for_path(&path) {
            self.finish_loading_export_path(path, format, result, cx);
            return;
        }

        cx.spawn(async move |this, cx| {
            let read_path = path.clone();
            let result =
                crate::platform::fs_task::run("read export source", read_path.clone(), move || {
                    fs::read_to_string(read_path)
                })
                .await;
            this.update(cx, |editor, cx| {
                editor.finish_loading_export_path(path, format, result, cx);
            })
            .ok();
        })
        .detach();
    }

    fn finish_loading_export_path(
        &mut self,
        path: PathBuf,
        format: ExportFormat,
        result: std::io::Result<String>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(text) => self.export_document(Some(path), text, format, cx),
            Err(error) => self.notify_error(
                format!("{}: {error}", self.t(crate::i18n::TextKey::ExportFailed)),
                cx,
            ),
        }
    }

    pub(super) fn export_current(&mut self, format: ExportFormat, cx: &mut Context<Self>) {
        self.export_document(
            self.current_file_path.clone(),
            self.buffer.text().to_string(),
            format,
            cx,
        );
    }

    pub(super) fn export_current_markdown(&mut self, cx: &mut Context<Self>) {
        self.export_current(ExportFormat::Markdown, cx);
    }

    pub(super) fn export_current_html(&mut self, cx: &mut Context<Self>) {
        self.export_current(ExportFormat::Html, cx);
    }

    pub(super) fn export_current_pdf(&mut self, cx: &mut Context<Self>) {
        self.export_current(ExportFormat::Pdf, cx);
    }

    fn export_document(
        &mut self,
        source_file_path: Option<PathBuf>,
        text: String,
        format: ExportFormat,
        cx: &mut Context<Self>,
    ) {
        let title = source_file_path
            .as_deref()
            .and_then(Path::file_name)
            .and_then(|name| name.to_str())
            .unwrap_or(self.t(crate::i18n::TextKey::Untitled))
            .to_string();
        let default_name =
            self.export_default_name_for_path(source_file_path.as_deref(), format.extension());
        #[cfg(not(target_os = "macos"))]
        let ui_language = self.ui_language;
        let layout = ExportLayout::from_settings(&self.app_settings);
        let preview_font_families = helpers::GeneratedPreviewFontFamilies {
            monospace: self.styles.editor.editor_monospace_font_family,
            body_cjk: self.styles.editor.editor_body_cjk_font_family,
        };
        cx.spawn(async move |this, cx| {
            #[cfg(target_os = "macos")]
            let maybe_path = match this.update(cx, |_, cx| {
                cx.prompt_for_new_path(
                    &std::env::home_dir().unwrap_or_default(),
                    Some(&default_name),
                )
            }) {
                Ok(receiver) => receiver.await.ok().and_then(Result::ok).flatten(),
                Err(_) => None,
            };
            #[cfg(not(target_os = "macos"))]
            let maybe_path = cx
                .background_executor()
                .spawn(async move {
                    let dialog_title =
                        crate::i18n::export_dialog_title(ui_language, format.label());
                    export_dialog_path(&default_name, &dialog_title)
                })
                .await;
            let Some(path) = maybe_path else {
                return;
            };

            let export_result: anyhow::Result<Vec<u8>> = match format {
                ExportFormat::Markdown => Ok(text.into_bytes()),
                ExportFormat::Html => {
                    render_html_export(title, text, source_file_path, layout, preview_font_families)
                        .await
                        .map(String::into_bytes)
                        .map_err(Into::into)
                }
                ExportFormat::Pdf | ExportFormat::Png => {
                    #[cfg(target_os = "macos")]
                    let export_result = {
                        let html = render_html_export(
                            title,
                            text,
                            source_file_path.clone(),
                            layout,
                            preview_font_families,
                        )
                        .await;
                        match (html, format) {
                            (Err(error), _) => Err(error.into()),
                            (Ok(html), ExportFormat::Png) => {
                                build_png_document_from_html_macos(
                                    &html,
                                    source_file_path.as_deref(),
                                    layout,
                                    cx,
                                )
                                .await
                            }
                            (Ok(html), _) => {
                                build_pdf_document_from_html_macos(
                                    &html,
                                    source_file_path.as_deref(),
                                    layout,
                                    cx,
                                )
                                .await
                            }
                        }
                    };

                    #[cfg(not(target_os = "macos"))]
                    let export_result = if matches!(format, ExportFormat::Png) {
                        Err(anyhow::anyhow!("PNG export is only available on macOS"))
                    } else {
                        crate::platform::fs_task::run_anyhow(
                            "build PDF export",
                            source_file_path.clone().unwrap_or_default(),
                            move || {
                                build_pdf_document(
                                    &title,
                                    &text,
                                    source_file_path.as_deref(),
                                    layout,
                                )
                            },
                        )
                        .await
                    };
                    export_result
                }
            };
            let export_error = match export_result {
                Ok(bytes) => {
                    let write_path = path.clone();
                    crate::platform::fs_task::run("write export", write_path.clone(), move || {
                        file_ops::atomic_write(&write_path, &bytes)
                    })
                    .await
                    .err()
                    .map(|error| error.to_string())
                }
                Err(error) => Some(error.to_string()),
            };
            match export_error {
                Some(error) => {
                    this.update(cx, |editor, cx| {
                        let message =
                            format!("{}: {error}", editor.t(crate::i18n::TextKey::ExportFailed));
                        editor.notify_error(message, cx);
                    })
                    .ok();
                }
                None => {
                    this.update(cx, |editor, cx| {
                        let message = format!(
                            "{}: {}",
                            editor.t(crate::i18n::TextKey::ExportCompleted),
                            path.display()
                        );
                        editor.notify_info(message, cx);
                    })
                    .ok();
                }
            }
        })
        .detach();
    }
}

async fn render_html_export(
    title: String,
    text: String,
    source_file_path: Option<PathBuf>,
    layout: ExportLayout,
    fonts: helpers::GeneratedPreviewFontFamilies<'static>,
) -> std::io::Result<String> {
    crate::platform::fs_task::run_value(
        "render HTML export",
        source_file_path.clone().unwrap_or_default(),
        move || {
            build_html_document_with_font_families(
                &title,
                &text,
                source_file_path.as_deref(),
                layout,
                fonts,
            )
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn export_file_url_encoding_does_not_require_an_existing_path() {
        let path = PathBuf::from("unavailable mount/note #1.md");
        assert_eq!(
            super::percent_encode_file_url_path(&path),
            "unavailable%20mount/note%20%231.md/"
        );
    }

    #[test]
    fn pdf_plain_text_projection_removes_markdown_source_markers() {
        let rendered = rendered_plain_text_for_export(
            "# Heading\n\nThis is **bold** and [linked](https://example.com).\n\n- item\n\n![diagram](chart.png)\n\n```rust\nlet answer = 42;\n```\n\n$$x + y$$\n",
        );

        assert!(rendered.contains("Heading"));
        assert!(rendered.contains("This is bold and linked."));
        assert!(rendered.contains("item"));
        assert!(rendered.contains("diagram"));
        assert!(rendered.contains("let answer = 42;"));
        assert!(rendered.contains("x + y"));
        assert!(!rendered.contains('#'));
        assert!(!rendered.contains("**"));
        assert!(!rendered.contains("```"));
        assert!(!rendered.contains("https://example.com"));
        assert!(!rendered.contains("chart.png"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn pdf_pages_break_at_the_lowest_candidate_that_still_fits() {
        use super::pdf_page_slices;

        // Shorter than a page: one slice covering everything.
        assert_eq!(
            pdf_page_slices(300.0, &[100.0, 200.0], 740.0),
            vec![(0.0, 300.0)]
        );

        // Snaps to the last line box that fits rather than cutting at 740.
        let breaks = vec![200.0, 700.0, 730.0, 900.0, 1400.0, 1500.0];
        assert_eq!(
            pdf_page_slices(1500.0, &breaks, 740.0),
            vec![(0.0, 730.0), (730.0, 670.0), (1400.0, 100.0)]
        );

        // Without any candidate in range the cut falls back to the page height.
        assert_eq!(
            pdf_page_slices(1000.0, &[], 400.0),
            vec![(0.0, 400.0), (400.0, 400.0), (800.0, 200.0)]
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn composed_slices_use_the_configured_content_width() {
        use super::cg_pdf::*;
        use core_graphics::geometry::{CGPoint, CGRect, CGSize};
        use objc::{class, msg_send, sel, sel_impl};

        // A minimal one-page PDF standing in for a rendered slice.
        fn slice_pdf(width: f64, height: f64) -> Vec<u8> {
            unsafe {
                let data: *mut objc::runtime::Object = msg_send![class!(NSMutableData), data];
                let consumer = CGDataConsumerCreateWithCFData(data as CFMutableDataRef);
                let media_box = CGRect::new(&CGPoint::new(0.0, 0.0), &CGSize::new(width, height));
                let context = CGPDFContextCreate(consumer, &media_box, std::ptr::null());
                CGDataConsumerRelease(consumer);
                CGPDFContextBeginPage(context, std::ptr::null());
                CGPDFContextEndPage(context);
                CGPDFContextClose(context);
                CGContextRelease(context);
                let length: usize = msg_send![data, length];
                let ptr: *const u8 = msg_send![data, bytes];
                std::slice::from_raw_parts(ptr, length).to_vec()
            }
        }

        let mut settings = super::AppSettings::default();
        settings.editor_content_width = 1200.0;
        let layout = super::ExportLayout::from_settings(&settings);
        let composed = super::compose_pdf_pages(
            &[
                slice_pdf(layout.content_width_px, 700.0),
                slice_pdf(layout.content_width_px, 200.0),
            ],
            layout,
        )
        .unwrap();

        unsafe {
            let data: *mut objc::runtime::Object = msg_send![
                class!(NSData),
                dataWithBytes: composed.as_ptr() as *const std::ffi::c_void
                length: composed.len()
            ];
            let provider = CGDataProviderCreateWithCFData(data as CFDataRef);
            let document = CGPDFDocumentCreateWithProvider(provider);
            CGDataProviderRelease(provider);
            assert_eq!(CGPDFDocumentGetNumberOfPages(document), 2);
            for index in 1..=2 {
                let bounds = CGPDFPageGetBoxRect(CGPDFDocumentGetPage(document, index), 0);
                assert!(
                    (bounds.size.width - layout.page_width_px * super::CSS_PX_TO_PDF_PT).abs()
                        < 0.01
                );
                assert!(
                    (bounds.size.height - layout.page_height_px * super::CSS_PX_TO_PDF_PT).abs()
                        < 0.01
                );
            }
            CGPDFDocumentRelease(document);
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn pdf_html_drops_the_screen_padding_that_would_double_the_margins() {
        let page =
            super::pdf_page_html("<html><head><style>a{}</style></head><body></body></html>");
        assert!(page.contains(".markdown-body{max-width:none!important;padding:0!important;"));
        assert_eq!(page.matches("</head>").count(), 1);
    }

    #[test]
    fn export_layout_tracks_content_width_and_separates_page_margins() {
        for width in [320.0, 700.0, 1200.0, 1600.0] {
            let mut settings = super::AppSettings::default();
            settings.editor_content_width = width;
            let layout = super::ExportLayout::from_settings(&settings);
            assert_eq!(layout.content_width_px, f64::from(width));
            assert_eq!(
                layout.page_width_px - 2.0 * super::EXPORT_PAGE_MARGIN_PX,
                layout.content_width_px
            );
            assert!(layout.content_height_px() > 0.0);
            let html = build_html_document("Note", "body", None, layout);
            assert!(html.contains(&format!("--export-content-width:{width}px;")));
            assert!(html.contains(&format!("--export-page-width:{}px;", layout.page_width_px)));
            assert!(html.contains("max-width:var(--export-page-width)"));
            assert!(!html.contains("max-width:980px"));
            assert!(!html.contains("size:A4"));
            assert!(html.contains(&format!("@page{{size:{}px ", layout.page_width_px)));
            // PNG captures exactly this outer width, so the responsive rule
            // cannot reduce padding and silently widen the content at small settings.
            assert!(html.contains(&format!(
                "@media(max-width:{}px)",
                layout.page_width_px - 1.0
            )));
        }
        let mut settings = super::AppSettings::default();
        settings.editor_content_width = f32::NAN;
        assert_eq!(
            super::ExportLayout::from_settings(&settings).content_width_px,
            700.0
        );
    }

    #[test]
    fn png_export_pixel_width_is_independent_of_backing_scale() {
        let layout = super::ExportLayout::from_settings(&super::AppSettings::default());
        for scale in [1.0, 1.5, 2.0] {
            assert_eq!(layout.png_snapshot_width_pt(scale) * scale, 1604.0);
        }
    }

    #[test]
    fn export_mermaid_uses_ondine_in_svg_html_and_nested_callouts() {
        let source = "flowchart LR\n A[Start] --> B[Finish]";
        let svg =
            super::render_export_preview_svg(crate::markdown::BlockPreviewKind::Mermaid, source);
        let preview = crate::editor::helpers::build_generated_block_preview_svg(
            crate::markdown::BlockPreviewKind::Mermaid,
            source,
            &crate::theme::ThemeKind::PaperWhite.build(),
            super::export_preview_font_families(),
        );
        assert_eq!(svg, preview, "export must use the same renderer as preview");
        assert!(svg.contains("class=\"ondine ondine-flowchart\""));
        assert!(svg.contains("--ondine-bg:"));
        assert!(svg.contains("--ondine-fg:"));
        crate::editor::helpers::load_generated_preview_image_from_svg(
            &svg,
            crate::markdown::BlockPreviewKind::Mermaid,
        )
        .expect("Ondine SVG should rasterize");

        let fenced = format!("```mermaid\n{source}\n```\n");
        for (prefix, quote) in [
            ("", ""),
            ("", "> "),
            ("> [!NOTE]\n", "> "),
            ("> [!TIP] Custom title\n", "> "),
            ("> > [!NOTE]\n", "> > "),
        ] {
            let markdown = format!(
                "{prefix}{}",
                fenced
                    .lines()
                    .map(|line| format!("{quote}{line}\n"))
                    .collect::<String>()
            );
            let html = super::build_html_document(
                "Diagram",
                &markdown,
                None,
                super::ExportLayout::from_settings(&super::AppSettings::default()),
            );
            assert!(
                html.contains(&svg),
                "Ondine SVG not propagated through {markdown:?}"
            );
            assert_eq!(
                html.matches("class=\"export-preview export-preview-mermaid\"")
                    .count(),
                1
            );
        }

        let html = super::build_html_document_with_font_families(
            "Diagram",
            &fenced,
            None,
            super::ExportLayout::from_settings(&super::AppSettings::default()),
            crate::editor::helpers::GeneratedPreviewFontFamilies {
                monospace: "Moeka Test Code",
                body_cjk: "Moeka Test CJK",
            },
        );
        assert!(html.contains("text,tspan{font-family:'Moeka Test Code'"));
    }

    #[test]
    fn export_math_never_rewrites_multiline_html_literals() {
        for wrapper in [
            ("<pre>", "</pre>"),
            ("<!--", "-->"),
            ("<script>", "</script>"),
        ] {
            for delimiters in [("\\[", "\\]"), ("$$", "$$")] {
                let markdown = format!(
                    "{}\n{}\nx+y\n{}\n{}\n",
                    wrapper.0, delimiters.0, delimiters.1, wrapper.1
                );
                assert_eq!(
                    super::markdown_with_export_math_blocks(&markdown, Options::all()),
                    markdown
                );
                let html = super::render_markdown_for_export(&markdown);
                assert!(!html.contains("export-preview-math"));
                assert!(!html.contains("~~~math"));
                assert!(html.contains("x+y"));
            }
        }
    }

    #[test]
    fn export_renders_editor_block_math_syntax_without_source_or_error_fallback() {
        for markdown in [
            "$$\n\\frac{a}{b}\n$$\n",
            "$$\nx +\n\ny\n$$\n",
            "\\[\\frac{a}{b}\\]\n",
            "\\[\n\\frac{a}{b}\n\\]\n",
            "$$x\\:y$$",
            "```math\n\\frac{a}{b}\n```\n",
            "```latex\n\\frac{a}{b}\n```\n",
            "```tex\n\\frac{a}{b}\n```\n",
            "$$\n\\operatorname*{arg\\,min}_{\\theta \\in \\mathbb{R}^n}\n\\left[\\frac{1}{m}\\sum_{i=1}^{m}\\left\\lVert f_\\theta(x_i)-y_i \\right\\rVert_2^2 + \\lambda \\left\\lVert \\theta \\right\\rVert_2^2\\right]\n$$\n",
        ] {
            let html = super::render_markdown_for_export(markdown);
            assert_eq!(
                html.matches("class=\"export-preview export-preview-math\"")
                    .count(),
                1,
                "{markdown:?}: {html}"
            );
            assert!(html.contains("<svg"), "{markdown:?}: {html}");
            assert!(html.contains("<path"), "{markdown:?}: {html}");
            assert!(
                !html.contains("LaTeX preview failed"),
                "{markdown:?}: {html}"
            );
            assert!(!html.contains("$$"), "{markdown:?}: {html}");
            assert!(
                !html
                    .chars()
                    .any(|ch| ('\u{e000}'..='\u{e004}').contains(&ch)),
                "{markdown:?}: {html}"
            );
        }
    }

    #[test]
    fn export_math_keeps_tex_commands_opaque_to_escape_masking() {
        for markdown in ["$$x\\:y$$", "$x\\:y$", "```math\nx\\:y\n```"] {
            assert_eq!(
                super::mask_escaped_custom_syntax(markdown, Options::all()),
                markdown
            );
        }
    }

    #[test]
    fn export_math_preserves_adjacent_prose_quotes_and_literal_contexts() {
        let html = super::render_markdown_for_export("before\n\\[\\frac{a}{b}\\]\nafter\n");
        assert!(html.contains("<p>before</p>"));
        assert!(html.contains("<p>after</p>"));
        assert_eq!(html.matches("export-preview-math").count(), 1);
        let html = super::render_markdown_for_export("> [!NOTE]\n> $$\n> x+y\n> $$\n");
        assert!(html.contains("callout-note"));
        assert!(html.contains("export-preview-math"));
        for markdown in [
            "`\\[x\\]`",
            "```text\n\\[x\\]\n$$\nx\n$$\n```",
            "[x]",
            "$$\nx + y\n",
            "\\[\nx+y\n",
        ] {
            let html = super::render_markdown_for_export(markdown);
            assert!(
                !html.contains("export-preview-math"),
                "{markdown:?}: {html}"
            );
        }
    }

    #[test]
    fn export_highlights_code_blocks_and_styles_tables() {
        let md = "```rust\nfn main() { let x = 1; }\n```\n";
        let html = super::render_markdown_for_export(md);
        // syntect emits inline-styled spans for highlighted tokens.
        assert!(
            html.contains("<span style=\"color:"),
            "code highlighting: {html}"
        );

        let doc = super::build_html_document(
            "T",
            md,
            None,
            super::ExportLayout::from_settings(&super::AppSettings::default()),
        );
        assert!(
            doc.contains(
                ".markdown-body table{display:block;width:max-content;max-width:100%;overflow:auto"
            ),
            "overflow-safe tables: missing rule"
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
    fn html_export_preserves_task_after_plain_and_before_empty_list_items() {
        let html = super::render_markdown_for_export("- abc\n- [x] abc\n- ");
        assert!(html.contains("type=\"checkbox\""), "{html}");
        assert!(html.contains("checked"), "{html}");
    }

    #[test]
    fn export_keeps_unspaced_text_and_headings_outside_tables() {
        let markdown = "| A | B |\n| --- | --- |\n| 1 | 2 |\nmiddle\n| C | D |\n| --- | --- |\n| 3 | 4 |\n# ad|bc\n";
        let html = super::render_markdown_for_export(markdown);

        assert_eq!(html.matches("<table>").count(), 2);
        assert!(html.contains("<p>middle</p>"));
        assert!(html.contains("<h1>ad|bc</h1>"));
    }

    #[test]
    fn export_preserves_html_breaks_inside_table_cells() {
        let html = super::render_markdown_for_export(
            "| A | B |\n| --- | --- |\n| first<br>second | value |\n",
        );

        assert!(html.contains("first<br>second"));
        assert!(
            super::rendered_plain_text_for_export(
                "| A | B |\n| --- | --- |\n| first<br>second | value |\n"
            )
            .contains("first\nsecond")
        );
    }

    #[test]
    fn export_cjk_reference_labels_unknown_sentinel_keys_stay_unlinked() {
        for (delimiter, tag, masked_label) in [
            ("**", "strong", "前**你好。**⸀"),
            ("*", "em", "前*你好。*⸀"),
            ("__", "strong", "⸀__⸀好。__⸀"),
            ("_", "em", "⸀_⸀好。_⸀"),
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
                    let definitions =
                        format!("[{masked_label}]: /wrong\n[known]: /known\n{original_definition}");
                    let target = if suffix == "[known]" {
                        Some("/known")
                    } else if known {
                        Some("/right")
                    } else {
                        None
                    };
                    for source in [
                        format!("{usage}\n\n{definitions}"),
                        format!("| Value |\n| --- |\n| {usage} |\n\n{definitions}"),
                    ] {
                        let html = super::render_markdown_for_export(&source);
                        assert!(!html.contains("href=\"/wrong\""), "{source}: {html}");
                        assert!(
                            html.contains(&format!("前<{tag}>你好。</{tag}>后")),
                            "{source}: {html}"
                        );
                        if let Some(target) = target {
                            assert!(
                                html.contains(&format!("href=\"{target}\"")),
                                "{source}: {html}"
                            );
                        } else {
                            assert!(!html.contains("<a "), "{source}: {html}");
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn export_cjk_reference_labels_preserve_links_titles_and_visible_characters() {
        for (delimiter, tag) in [("**", "strong"), ("__", "strong"), ("*", "em"), ("_", "em")] {
            for suffix in ["", "[]"] {
                let label = format!("前{delimiter}你好。{delimiter}后");
                let usage = format!("[{label}]{suffix}");
                let definition = format!("[{label}]: /target \"original title\"\n");
                let expected = format!(
                    "<a href=\"/target\" title=\"original title\">前<{tag}>你好。</{tag}>后</a>"
                );
                let source = format!("{usage}\n\n{definition}");
                let html = super::render_markdown_for_export(&source);
                assert_eq!(html.matches(&expected).count(), 1, "{source}: {html}");
                let table = format!("| {usage} |\n| --- |\n| {usage} |\n\n{definition}");
                let html = super::render_markdown_for_export(&table);
                assert_eq!(html.matches(&expected).count(), 2, "{table}: {html}");
            }
        }
    }

    #[test]
    fn export_cjk_inline_punctuation_matches_editor_and_preserves_text() {
        for (delimiter, tag) in [
            ("**", "strong"),
            ("__", "strong"),
            ("*", "em"),
            ("_", "em"),
            ("~~", "del"),
            ("==", "mark"),
            ("++", "u"),
            ("^", "sup"),
            ("~", "sub"),
        ] {
            for inner in ["你好。", "「你好」", "你好!", "你好🙂", "𠮷。"] {
                let source = format!("前{delimiter}{inner}{delimiter}后");
                let expected = format!("前<{tag}>{inner}</{tag}>后");
                let html = super::render_markdown_for_export(&source);
                assert!(html.contains(&expected), "{source}: {html}");
                let table = format!("| {source} |\n| --- |\n| {source} |\n");
                let html = super::render_markdown_for_export(&table);
                assert_eq!(html.matches(&expected).count(), 2, "{table}: {html}");
            }
        }
        for (source, expected) in [
            ("前***你好。***后", "前<em><strong>你好。</strong></em>后"),
            (
                "前**==你好。==**后",
                "前<strong><mark>你好。</mark></strong>后",
            ),
            (
                "甲**你好。**乙*「世界」*丙",
                "甲<strong>你好。</strong>乙<em>「世界」</em>丙",
            ),
            ("𠮷**你好。**𠮷", "𠮷<strong>你好。</strong>𠮷"),
            (
                "&#x2e00; 前**你好。**后 &amp; &#x10100;",
                "⸀ 前<strong>你好。</strong>后 &amp; 𐄀",
            ),
        ] {
            let html = super::render_markdown_for_export(source);
            assert!(html.contains(expected), "{source}: {html}");
        }
    }

    #[test]
    fn export_cjk_inline_punctuation_preserves_protected_source_and_western_rules() {
        for source in [
            r"前\**你好。\**后",
            "`前**你好。**后`",
            "$前**你好。**后$",
            "<code>前**你好。**后</code>",
            "<!-- 前**你好。**后 -->",
            "[label](前**你好。**后)",
            "https://host/前**你好。**后",
            "foo_bar_baz",
            "foo__bar__baz",
            "a__你好__b",
        ] {
            let html = super::render_markdown_for_export(source);
            assert!(!html.contains("<strong>"), "{source}: {html}");
            assert!(!html.contains("<em>"), "{source}: {html}");
        }
        let html = super::render_markdown_for_export(
            "[前**你好。**后](path_你好_字) [[target|前**你好。**后]]",
        );
        assert_eq!(
            html.matches("前<strong>你好。</strong>后").count(),
            2,
            "{html}"
        );
        assert!(
            html.contains("href=\"path_%E4%BD%A0%E5%A5%BD_%E5%AD%97\""),
            "{html}"
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
    fn export_custom_inline_syntax_respects_escapes_and_protected_contexts() {
        let md = r"\==literal== \++literal++ \^literal^ \~literal~ \:rocket: `==code==` [\==escaped==](one) <code>++html++</code>";
        let html = super::render_markdown_for_export(md);

        assert!(!html.contains("<mark>"), "highlight: {html}");
        assert!(!html.contains("<u>"), "underline: {html}");
        assert!(!html.contains("<sup>"), "superscript: {html}");
        assert!(!html.contains("<sub>"), "subscript: {html}");
        assert!(!html.contains('\u{1F680}'), "emoji: {html}");
        assert!(html.contains("==literal=="), "escaped literal: {html}");
        assert!(html.contains("++html++"), "html code literal: {html}");
    }

    #[test]
    fn export_link_labels_allow_custom_syntax_but_targets_and_image_alt_are_opaque() {
        let md = "[==link== ++under++ :rocket:](target==literal==) [[wiki==target==|==wiki== ++alias++ :rocket:]] ![==alt==](image.png)";
        let html = super::render_markdown_for_export(md);

        assert_eq!(html.matches("<mark>").count(), 2, "highlight: {html}");
        assert_eq!(html.matches("<u>").count(), 2, "underline: {html}");
        assert_eq!(html.matches('\u{1F680}').count(), 2, "emoji: {html}");
        assert!(
            html.contains("href=\"target==literal==\""),
            "target: {html}"
        );
        assert!(
            html.contains("href=\"wiki==target==\""),
            "wiki target: {html}"
        );
        assert!(html.contains("alt=\"==alt==\""), "image alt: {html}");
    }

    #[test]
    fn export_super_and_subscript_match_editor_compact_rules() {
        let html = super::render_markdown_for_export("^two words^ ~two words~ ^ok^ ~ok~");

        assert!(
            !html.contains("<sup>two words</sup>"),
            "superscript: {html}"
        );
        assert!(!html.contains("<sub>two words</sub>"), "subscript: {html}");
        assert!(html.contains("<sup>ok</sup>"), "superscript: {html}");
        assert!(html.contains("<sub>ok</sub>"), "subscript: {html}");
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
    fn export_does_not_promote_late_callout_markers() {
        let html = super::render_markdown_for_export("> ordinary\n> [!NOTE] literal");

        assert!(!html.contains("class=\"callout"), "callout: {html}");
        assert!(html.contains("[!NOTE] literal"), "literal marker: {html}");
    }

    #[test]
    fn html_export_includes_print_styles_and_base_href() {
        let html = build_html_document(
            "Note",
            "![img](./assets/pic.png)",
            Some(&PathBuf::from("/tmp/demo/note.md")),
            super::ExportLayout::from_settings(&super::AppSettings::default()),
        );

        assert!(html.contains("@page{size:802px "));
        assert!(html.contains("margin:51px;}"));
        assert!(html.contains("<base href=\"file:///tmp/demo/\">"));
    }
}
