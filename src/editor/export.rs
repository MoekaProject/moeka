//! Document export orchestration and portable Markdown/HTML/PDF conversion.
use super::*;
use gpui::Context;
use pulldown_cmark::{
    BlockQuoteKind, CodeBlockKind, CowStr, Event, LinkType, Options, Parser, Tag, TagEnd,
    html::push_html,
};
use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
mod html;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
use macos::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExportFormat {
    Markdown,
    Html,
    Pdf,
    Png,
}

impl ExportFormat {
    pub(super) fn label(self) -> &'static str {
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

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExportTarget {
    File,
    Clipboard,
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct CopiedExport {
    pane: pane::PaneId,
    tab: Option<String>,
    path: Option<PathBuf>,
    format: ExportFormat,
}

#[cfg(any(target_os = "macos", test))]
const CSS_PX_TO_PDF_PT: f64 = 72.0 / 96.0;

struct ExportPermit;
static EXPORT_RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub(super) fn export_in_progress() -> bool {
    EXPORT_RUNNING.load(std::sync::atomic::Ordering::Acquire)
}
impl ExportPermit {
    fn acquire() -> anyhow::Result<Self> {
        use std::sync::atomic::Ordering;
        EXPORT_RUNNING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| {
                anyhow::anyhow!(crate::i18n::runtime::RuntimeText::ExportBusy.to_string())
            })?;
        Ok(Self)
    }
}
impl Drop for ExportPermit {
    fn drop(&mut self) {
        EXPORT_RUNNING.store(false, std::sync::atomic::Ordering::Release);
    }
}

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

fn html_base_href(source_file_path: Option<&Path>) -> Option<String> {
    let directory = source_file_path?.parent()?;
    let mut url = crate::platform::paths::path_to_file_url(directory)?;
    if !url.ends_with('/') {
        url.push('/');
    }
    Some(url)
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

    let theme = helpers::syntect_theme_for_palette(
        crate::theme::ThemeKind::PaperWhite.build().code_palette,
    );

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
        title_html.push_str(title);
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
    let mut first_line_html = String::new();
    push_html(
        &mut first_line_html,
        transform_export_events(inner[1..first_line_end].iter().cloned(), font_families)
            .into_iter(),
    );
    let title = first_line_html.trim_start().strip_prefix("[!")?;
    let title = title
        .get(title.find(']')? + 1..)?
        .trim_start_matches(['+', '-'])
        .trim();

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
            Event::FootnoteReference(label) => {
                let label = html_escape(&label);
                events.push(Event::Html(
                    format!(
                        "<sup class=\"footnote-reference\"><a href=\"#{label}\">{label}</a></sup>"
                    )
                    .into(),
                ));
            }
            Event::Start(Tag::FootnoteDefinition(label)) => {
                let label = html_escape(&label);
                events.push(Event::Html(format!(
                    "<div class=\"footnote-definition\" id=\"{label}\"><sup class=\"footnote-definition-label\">{label}</sup>\n"
                ).into()));
            }
            Event::End(TagEnd::FootnoteDefinition) => {
                events.push(Event::Html("</div>\n".into()));
            }
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
    let markdown = html::adapt_editor_syntax(markdown);
    let markdown = markdown_with_explicit_table_boundaries(&markdown);
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
    let mut events = transform_export_events(parser, font_families);
    let mut heading_ids = HashMap::<String, usize>::new();
    for index in 0..events.len() {
        if !matches!(&events[index], Event::Start(Tag::Heading { id: None, .. })) {
            continue;
        }
        let title = events[index + 1..]
            .iter()
            .take_while(|event| !matches!(event, Event::End(TagEnd::Heading(_))))
            .filter_map(|event| match event {
                Event::Text(text) | Event::Code(text) | Event::InlineMath(text) => {
                    Some(text.as_ref())
                }
                _ => None,
            })
            .collect::<String>();
        let slug = title
            .to_lowercase()
            .chars()
            .filter(|ch| ch.is_alphanumeric() || ch.is_whitespace() || matches!(ch, '-' | '_'))
            .map(|ch| if ch.is_whitespace() { '-' } else { ch })
            .collect::<String>();
        let count = heading_ids.entry(slug.clone()).or_default();
        let id = if *count == 0 {
            slug
        } else {
            format!("{slug}-{count}")
        };
        *count += 1;
        if let Event::Start(Tag::Heading { id: heading_id, .. }) = &mut events[index] {
            *heading_id = Some(id.into());
        }
    }
    push_html(&mut html, events.into_iter());
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
        &html::default_typography_css(),
    )
}

fn build_html_document_with_font_families(
    title: &str,
    markdown: &str,
    source_file_path: Option<&Path>,
    layout: ExportLayout,
    font_families: helpers::GeneratedPreviewFontFamilies<'_>,
    typography_css: &str,
) -> String {
    let html = render_markdown_for_export_with_font_families(markdown, font_families);

    let escaped_title = html_escape(title);
    let base_href = html_base_href(source_file_path)
        .map(|href| format!("<base href=\"{}\">", html_escape(&href)))
        .unwrap_or_default();

    let github_markdown_css = concat!(
        "html,body{margin:0;padding:0;}html{scrollbar-width:none;}html::-webkit-scrollbar{display:none;}.markdown-body{box-sizing:border-box;min-width:0;width:100%;max-width:var(--export-page-width);margin:0 auto;padding:var(--export-page-margin);color:#1f2328;background:#fff;font-family:-apple-system,BlinkMacSystemFont,'Segoe UI','Noto Sans',Helvetica,Arial,sans-serif,'Apple Color Emoji','Segoe UI Emoji';font-size:16px;line-height:1.5;word-wrap:break-word;}",
        ".markdown-body h1,.markdown-body h2{padding-bottom:.3em;border-bottom:1px solid #d8dee4;}",
        ".markdown-body h1{margin:.67em 0 16px;font-size:2em;}",
        ".markdown-body h2{margin:24px 0 16px;font-size:1.5em;}",
        ".markdown-body h3{margin:24px 0 16px;font-size:1.25em;}",
        ".markdown-body h4{margin:24px 0 16px;font-size:1em;}",
        ".markdown-body h5{margin:24px 0 16px;font-size:.875em;}",
        ".markdown-body h6{margin:24px 0 16px;color:#57606a;font-size:.85em;}",
        ".markdown-body p,.markdown-body blockquote,.markdown-body ul,.markdown-body ol,.markdown-body dl,.markdown-body table,.markdown-body pre,.markdown-body details{margin-top:0;margin-bottom:16px;}",
        ".markdown-body .footnote-definition{margin-bottom:16px;}.markdown-body .footnote-definition>sup+p{display:inline;margin:0;}.markdown-body .footnote-definition-label{margin-right:.35em;}",
        ".markdown-body ul,.markdown-body ol{padding-left:1.5em;}",
        ".markdown-body>ul:has(>li>input[type=checkbox]){padding-left:0;}",
        ".markdown-body li:has(>input[type=checkbox]){list-style:none;}.markdown-body li>input[type=checkbox]{-webkit-appearance:none;appearance:none;box-sizing:border-box;display:inline-block;width:1.14em;height:1.14em;margin:0 .5em 0 0;border:1.5px solid #8c959f;border-radius:.25em;background:#fff;vertical-align:-.05em;}",
        ".markdown-body li>input[type=checkbox]:checked{border-color:#0969da;background:#0969da url(\"data:image/svg+xml,%3Csvg%20xmlns='http://www.w3.org/2000/svg'%20viewBox='0%200%2016%2016'%3E%3Cpath%20fill='none'%20stroke='white'%20stroke-width='2.2'%20stroke-linecap='round'%20stroke-linejoin='round'%20d='M3.5%208.5l3%203%206-7'/%3E%3C/svg%3E\") center/85% 85% no-repeat;}",
        ".markdown-body a{color:#0969da;text-decoration:none;overflow-wrap:anywhere;}.markdown-body a:hover{text-decoration:underline;}",
        ".markdown-body blockquote{margin-left:0;margin-right:0;padding:0 1em;color:#57606a;border-left:.25em solid #d0d7de;}",
        ".markdown-body code,.markdown-body tt{padding:.2em .4em;margin:0;font-size:85%;white-space:break-spaces;background-color:rgba(175,184,193,.2);border-radius:6px;}",
        ".markdown-body pre{padding:16px;white-space:pre-wrap;overflow-wrap:anywhere;font-size:85%;line-height:1.45;background-color:#f6f8fa;border-radius:6px;}",
        ".markdown-body pre code{padding:0;margin:0;font-size:100%;white-space:pre-wrap;word-break:break-word;background:transparent;border:0;}",
        ".markdown-body pre{font-family:ui-monospace,SFMono-Regular,'SF Mono',Menlo,Consolas,'Liberation Mono',monospace;border:1px solid #d0d7de;}",
        ".markdown-body table{display:table;width:100%;max-width:100%;border-spacing:0;border-collapse:collapse;}.markdown-body th,.markdown-body td{overflow-wrap:anywhere;}",
        ".markdown-body table th{font-weight:600;}.markdown-body table th,.markdown-body table td{padding:6px 13px;border:1px solid #d0d7de;}",
        ".markdown-body table tr{background-color:#fff;border-top:1px solid #d8dee4;}.markdown-body table tr:nth-child(2n){background-color:#f6f8fa;}",
        ".markdown-body img{box-sizing:content-box;max-width:100%;height:auto;background-color:#fff;}",
        ".markdown-body hr{height:.25em;padding:0;margin:24px 0;background-color:#d0d7de;border:0;}",
        ".markdown-body kbd{display:inline-block;padding:3px 5px;font:11px ui-monospace,SFMono-Regular,SF Mono,Menlo,Consolas,Liberation Mono,monospace;line-height:10px;color:#24292f;vertical-align:middle;background-color:#f6f8fa;border:solid 1px rgba(175,184,193,.2);border-bottom-color:rgba(175,184,193,.2);border-radius:6px;box-shadow:inset 0 -1px 0 rgba(175,184,193,.2);}",
        ".export-preview{margin:16px 0;padding:12px;overflow:auto;text-align:center;background:#fff;border:1px solid #d0d7de;border-radius:6px;}",
        ".export-preview svg{max-width:100%;height:auto;}.export-math-inline{display:inline-flex;max-width:100%;vertical-align:middle;}.export-math-inline svg{height:auto;max-width:100%;}",
        ".markdown-body .front-matter{font-size:.9em;}.front-matter th{text-align:left;width:1%;white-space:nowrap;}.front-matter td{white-space:pre-wrap;}",
        ".markdown-body strong{font-weight:700;}.markdown-body em{font-style:italic;}.markdown-body mark{background:#fff0a6;color:inherit;}.markdown-body{print-color-adjust:exact;-webkit-print-color-adjust:exact;}",
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
            "<meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src data:; style-src 'unsafe-inline'; font-src data:;\">",
            "<title>{}</title>",
            "{}",
            "<style>",
            ":root{{--export-content-width:{content_width}px;--export-page-width:{page_width}px;--export-page-margin:{page_margin}px;}}",
            "{}",
            "{typography_css}",
            "@media(max-width:{responsive_width}px){{.markdown-body{{padding:15px;}}}}",
            "@page{{size:{page_width}px {page_height}px;margin:{page_margin}px;}}",
            "@media print{{html,body{{background:#fff;}}body{{margin:0;}}.markdown-body{{max-width:none;padding:0;}}.markdown-body h1,.markdown-body h2,.markdown-body h3,.markdown-body h4,.markdown-body h5,.markdown-body h6{{break-after:avoid-page;page-break-after:avoid;}}.markdown-body pre,.markdown-body blockquote,.markdown-body table,.markdown-body img,.markdown-body svg,.export-preview{{break-inside:avoid;page-break-inside:avoid;}}.markdown-body tr{{break-inside:avoid;page-break-inside:avoid;}}.markdown-body p{{orphans:3;widows:3;}}}}",
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
        typography_css = typography_css,
        responsive_width = layout.page_width_px - 1.0,
    )
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
                crate::i18n::menu_command_label(self.ui_language, "requires_text"),
            )));
        }
        None
    }

    pub(crate) fn export_path(
        &mut self,
        path: PathBuf,
        format: ExportFormat,
        target: ExportTarget,
        cx: &mut Context<Self>,
    ) {
        if let Some(result) = self.open_export_text_for_path(&path) {
            self.finish_loading_export_path(path, format, target, result, cx);
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
                editor.finish_loading_export_path(path, format, target, result, cx);
            })
            .ok();
        })
        .detach();
    }

    fn finish_loading_export_path(
        &mut self,
        path: PathBuf,
        format: ExportFormat,
        target: ExportTarget,
        result: std::io::Result<String>,
        cx: &mut Context<Self>,
    ) {
        match result {
            Ok(text) => self.export_document(Some(path), text, format, target, cx),
            Err(error) => self.notify_error(
                format!("{}: {error}", self.t(crate::i18n::TextKey::ExportFailed)),
                cx,
            ),
        }
    }

    pub(super) fn export_current(
        &mut self,
        format: ExportFormat,
        target: ExportTarget,
        cx: &mut Context<Self>,
    ) {
        self.export_document(
            self.current_file_path.clone(),
            self.buffer.text().to_string(),
            format,
            target,
            cx,
        );
    }

    fn export_copy_identity(&self, format: ExportFormat) -> CopiedExport {
        CopiedExport {
            pane: self.current_pane_id(),
            tab: self.active_tab_id.clone(),
            path: self.current_file_path.clone(),
            format,
        }
    }

    pub(super) fn export_is_copied(&self, format: ExportFormat) -> bool {
        self.copied_export.as_ref() == Some(&self.export_copy_identity(format))
    }

    fn export_document(
        &mut self,
        source_file_path: Option<PathBuf>,
        text: String,
        format: ExportFormat,
        target: ExportTarget,
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
        let feedback = (target == ExportTarget::Clipboard
            && self.current_file_path == source_file_path
            && self.buffer.text() == text)
            .then(|| self.export_copy_identity(format));
        #[cfg(not(target_os = "macos"))]
        let ui_language = self.ui_language;
        let layout = ExportLayout::from_settings(&self.app_settings);
        let workspace_root = self.sidebar_root.clone();
        let workspace_roots = self.workspaces.clone();
        let typography_css = html::editor_typography_css(&self.styles.editor);
        let preview_font_families = helpers::GeneratedPreviewFontFamilies {
            monospace: self.styles.editor.editor_monospace_font_family,
            body_cjk: self.styles.editor.editor_body_cjk_font_family,
        };
        cx.spawn(async move |this, cx| {
            let _permit = match ExportPermit::acquire() {
                Ok(permit) => permit,
                Err(error) => {
                    this.update(cx, |editor, cx| editor.notify_diagnostic(error, cx))
                        .ok();
                    return;
                }
            };
            let path = if target == ExportTarget::File {
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
                let dialog_default_name = default_name.clone();
                #[cfg(not(target_os = "macos"))]
                let maybe_path = cx
                    .background_executor()
                    .spawn(async move {
                        let dialog_title =
                            crate::i18n::export_dialog_title(ui_language, format.label());
                        export_dialog_path(&dialog_default_name, &dialog_title)
                    })
                    .await;
                let Some(path) = maybe_path else {
                    return;
                };
                Some(path)
            } else {
                None
            };

            let export_result: anyhow::Result<Vec<u8>> = match format {
                ExportFormat::Markdown => Ok(text.into_bytes()),
                ExportFormat::Html => render_html_export(
                    title,
                    text,
                    source_file_path,
                    layout,
                    preview_font_families,
                    workspace_root,
                    workspace_roots,
                    typography_css,
                )
                .await
                .map(String::into_bytes),
                ExportFormat::Pdf | ExportFormat::Png => {
                    let html = render_html_export(
                        title,
                        text,
                        source_file_path.clone(),
                        layout,
                        preview_font_families,
                        workspace_root,
                        workspace_roots,
                        typography_css,
                    )
                    .await;
                    #[cfg(target_os = "macos")]
                    let export_result = match (html, format) {
                        (Err(error), _) => Err(error),
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
                    };

                    #[cfg(not(target_os = "macos"))]
                    let export_result = match html {
                        Err(error) => Err(error),
                        Ok(html) => crate::platform::fs_task::run_with_timeout(
                            "Render native export",
                            PathBuf::new(),
                            std::time::Duration::from_secs(130),
                            move || {
                                let format = if matches!(format, ExportFormat::Png) {
                                    crate::platform::export::NativeFormat::Png
                                } else {
                                    crate::platform::export::NativeFormat::Pdf
                                };
                                crate::platform::export::run(
                                    &html,
                                    format,
                                    layout.page_width_px,
                                    layout.page_height_px,
                                    EXPORT_PAGE_MARGIN_PX,
                                )
                                .map_err(|error| std::io::Error::other(format!("{error:#}")))
                            },
                        )
                        .await
                        .map_err(Into::into),
                    };
                    export_result
                }
            };
            let export_error = match export_result {
                Ok(bytes) => {
                    if let Some(path) = path.as_ref() {
                        let write_path = path.clone();
                        crate::platform::fs_task::run_write(
                            "write export",
                            write_path.clone(),
                            move || file_ops::atomic_write(&write_path, &bytes),
                        )
                        .await
                        .err()
                        .map(|error| error.to_string())
                    } else {
                        let prepared = crate::platform::fs_task::run_write(
                            "prepare clipboard export",
                            PathBuf::from(&default_name),
                            move || {
                                crate::platform::clipboard::ExportFile::prepare(
                                    &default_name,
                                    &bytes,
                                )
                            },
                        )
                        .await;
                        match prepared {
                            Err(error) => Some(error.to_string()),
                            Ok(file) => {
                                #[cfg(target_os = "macos")]
                                let result = file.publish();
                                #[cfg(not(target_os = "macos"))]
                                let result = crate::platform::fs_task::run_anyhow(
                                    "publish clipboard export",
                                    PathBuf::new(),
                                    move || file.publish(),
                                )
                                .await;
                                result.err().map(|error| error.to_string())
                            }
                        }
                    }
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
                        let message = if let Some(path) = path {
                            format!(
                                "{}: {}",
                                editor.t(crate::i18n::TextKey::ExportCompleted),
                                path.display()
                            )
                        } else {
                            if let Some(feedback) = feedback {
                                editor.show_export_copy_feedback(feedback, cx);
                            }
                            editor
                                .t(crate::i18n::TextKey::CopiedToClipboard)
                                .to_string()
                        };
                        editor.notify_info(message, cx);
                    })
                    .ok();
                }
            }
        })
        .detach();
    }

    fn show_export_copy_feedback(&mut self, feedback: CopiedExport, cx: &mut Context<Self>) {
        self.export_copy_feedback_epoch = self.export_copy_feedback_epoch.wrapping_add(1);
        let epoch = self.export_copy_feedback_epoch;
        self.copied_export = Some(feedback.clone());
        cx.notify();
        cx.spawn(async move |this, cx| {
            cx.background_executor().timer(Duration::from_secs(2)).await;
            this.update(cx, |editor, cx| {
                if editor.export_copy_feedback_epoch == epoch
                    && editor.copied_export.as_ref() == Some(&feedback)
                {
                    editor.copied_export = None;
                    cx.notify();
                }
            })
            .ok();
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
    workspace_root: PathBuf,
    workspace_roots: Vec<PathBuf>,
    typography_css: String,
) -> anyhow::Result<String> {
    crate::platform::fs_task::run_anyhow(
        "render HTML export",
        source_file_path.clone().unwrap_or_default(),
        move || {
            let document = build_html_document_with_font_families(
                &title,
                &text,
                source_file_path.as_deref(),
                layout,
                fonts,
                &typography_css,
            );
            html::embed_images(
                &document,
                source_file_path.as_deref(),
                &workspace_root,
                &workspace_roots,
            )
        },
    )
    .await
}

/// Explicit diagnostic mode; never opens a workspace or restores user sessions.
pub(crate) fn run_native_smoke_test(directory: PathBuf, cx: &mut gpui::App) {
    cx.spawn(async move |cx| {
        let result = async {
            let layout = ExportLayout::from_settings(&AppSettings::default());
            let prepared_directory = directory.clone();
            let html = cx
                .background_executor()
                .spawn(async move {
                    fs::create_dir_all(&prepared_directory)?;
                    fs::write(
                        prepared_directory.join("export-sample.svg"),
                        include_str!("../../tests/fixtures/export-sample.svg"),
                    )?;
                    let html = build_html_document_with_font_families(
                        "Export gallery",
                        include_str!("../../tests/fixtures/export-gallery.md"),
                        Some(&prepared_directory.join("gallery.md")),
                        layout,
                        export_preview_font_families(),
                        &html::default_typography_css(),
                    );
                    let html = html::embed_images(
                        &html,
                        Some(&prepared_directory.join("gallery.md")),
                        &prepared_directory,
                        &[],
                    )?;
                    fs::write(prepared_directory.join("gallery.html"), &html)?;
                    Ok::<_, anyhow::Error>(html)
                })
                .await?;
            for format in [ExportFormat::Pdf, ExportFormat::Png] {
                #[cfg(target_os = "macos")]
                let bytes = match format {
                    ExportFormat::Pdf => {
                        build_pdf_document_from_html_macos(&html, None, layout, cx).await?
                    }
                    _ => build_png_document_from_html_macos(&html, None, layout, cx).await?,
                };
                #[cfg(not(target_os = "macos"))]
                let bytes = {
                    let html = html.clone();
                    cx.background_executor()
                        .spawn(async move {
                            crate::platform::export::run(
                                &html,
                                if matches!(format, ExportFormat::Pdf) {
                                    crate::platform::export::NativeFormat::Pdf
                                } else {
                                    crate::platform::export::NativeFormat::Png
                                },
                                layout.page_width_px,
                                layout.page_height_px,
                                EXPORT_PAGE_MARGIN_PX,
                            )
                        })
                        .await?
                };
                let path = directory.join(format!("gallery.{}", format.extension()));
                cx.background_executor()
                    .spawn(async move {
                        crate::platform::export::validate_output(
                            &bytes,
                            if matches!(format, ExportFormat::Pdf) {
                                crate::platform::export::NativeFormat::Pdf
                            } else {
                                crate::platform::export::NativeFormat::Png
                            },
                        )?;
                        file_ops::atomic_write(&path, &bytes)?;
                        Ok::<_, anyhow::Error>(())
                    })
                    .await?;
            }
            if std::env::var_os("MOEKA_EXPORT_CLIPBOARD_SMOKE").is_some() {
                for format in [
                    ExportFormat::Markdown,
                    ExportFormat::Html,
                    ExportFormat::Pdf,
                    ExportFormat::Png,
                ] {
                    let name = format!("gallery.{}", format.extension());
                    let source = directory.join(&name);
                    let prepare_name = name.clone();
                    let (file, expected) = cx
                        .background_executor()
                        .spawn(async move {
                            let bytes = if format == ExportFormat::Markdown {
                                include_bytes!("../../tests/fixtures/export-gallery.md").to_vec()
                            } else {
                                fs::read(&source)?
                            };
                            let file = crate::platform::clipboard::ExportFile::prepare(
                                &prepare_name,
                                &bytes,
                            )?;
                            Ok::<_, anyhow::Error>((file, bytes))
                        })
                        .await?;
                    #[cfg(target_os = "macos")]
                    file.publish()?;
                    #[cfg(not(target_os = "macos"))]
                    cx.background_executor()
                        .spawn(async move { file.publish() })
                        .await?;
                    #[cfg(target_os = "macos")]
                    let paths = crate::platform::clipboard::file_paths();
                    #[cfg(not(target_os = "macos"))]
                    let paths = cx
                        .background_executor()
                        .spawn(async { crate::platform::clipboard::file_paths() })
                        .await;
                    cx.background_executor()
                        .spawn(async move {
                            anyhow::ensure!(
                                paths.len() == 1,
                                "Clipboard must contain one file reference"
                            );
                            anyhow::ensure!(
                                paths[0].file_name() == Some(std::ffi::OsStr::new(&name)),
                                "Wrong clipboard file name"
                            );
                            anyhow::ensure!(
                                fs::read(&paths[0])? == expected,
                                "Clipboard file contents changed"
                            );
                            Ok::<_, anyhow::Error>(())
                        })
                        .await?;
                    println!("Native file clipboard verified: {}", format.label());
                }
            }
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if let Err(error) = result {
            eprintln!("Native export smoke test failed: {error:#}");
            std::process::exit(1);
        }
        println!("Native export smoke test passed: HTML, PDF, PNG");
        cx.update(|cx| cx.quit()).ok();
    })
    .detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn busy_export_error_uses_the_interface_language_and_releases_the_permit() {
        use crate::i18n::{UiLanguage, runtime};
        runtime::set_language(UiLanguage::ChineseSimplified);
        {
            let _permit = ExportPermit::acquire().unwrap();
            let error = ExportPermit::acquire().err().unwrap();
            assert_eq!(error.to_string(), "另一个导出任务仍在进行中，请等待完成");
        }
        assert!(ExportPermit::acquire().is_ok());
        runtime::set_language(UiLanguage::English);
    }

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn export_copy_feedback_is_scoped_and_repeated_copies_keep_their_full_timeout(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::AppContext;
        let editor = cx.update(|cx| {
            cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some("draft"), false))
        });
        editor.update(cx, |editor, cx| {
            let style = editor.styles.utility_export_copy_button();
            assert!(style.selected.foreground.a > style.normal.foreground.a);
            assert_eq!(style.selected_hover.foreground, style.selected.foreground);
            let feedback = editor.export_copy_identity(ExportFormat::Markdown);
            editor.show_export_copy_feedback(feedback.clone(), cx);
            assert!(editor.export_is_copied(ExportFormat::Markdown));
            assert!(!editor.export_is_copied(ExportFormat::Html));
            editor.copied_export.as_mut().unwrap().pane += 1;
            assert!(!editor.export_is_copied(ExportFormat::Markdown));
            editor.copied_export = Some(feedback);
            let tab = editor.active_tab_id.replace("different-tab".into());
            assert!(!editor.export_is_copied(ExportFormat::Markdown));
            editor.active_tab_id = tab;
            editor.current_file_path = Some("different.md".into());
            assert!(!editor.export_is_copied(ExportFormat::Markdown));
            editor.current_file_path = None;
            assert!(editor.export_is_copied(ExportFormat::Markdown));
        });
        cx.executor().run_until_parked();
        cx.executor().advance_clock(Duration::from_secs(1));
        editor.update(cx, |editor, cx| {
            editor
                .show_export_copy_feedback(editor.export_copy_identity(ExportFormat::Markdown), cx);
        });
        cx.executor().run_until_parked();
        cx.executor().advance_clock(Duration::from_millis(1100));
        cx.executor().run_until_parked();
        editor.update(cx, |editor, _| {
            assert!(editor.export_is_copied(ExportFormat::Markdown))
        });
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.executor().run_until_parked();
        editor.update(cx, |editor, _| {
            assert!(!editor.export_is_copied(ExportFormat::Markdown))
        });
    }

    #[test]
    fn export_gallery_preserves_editor_syntax_and_nesting() {
        let source = include_str!("../../tests/fixtures/export-gallery.md");
        let html = render_markdown_for_export(source);
        for expected in [
            "<table class=\"front-matter\">",
            "<strong>Bold</strong>",
            "<em>italic</em>",
            "<del>deleted</del>",
            "<u>underlined</u>",
            "<mark>highlighted</mark>",
            "<sub>2</sub>",
            "<sup>2</sup>",
            "🚀",
            "<mark><strong>nested bold</strong> and <em>italic</em></mark>",
            "<u><strong>nested underline</strong></u>",
            "id=\"lists\"",
            "href=\"#lists\"",
            "href=\"note.md\"",
            "callout callout-tip",
            "first<br>second",
            "export-math-inline",
            "export-preview-math",
            "ondine-flowchart",
            "This final paragraph must be present",
        ] {
            assert!(html.contains(expected), "Missing {expected}: {html}");
        }
        assert_eq!(html.matches("<img ").count(), 2);
        assert!(!html.contains("[!TIP]"));
        assert!(!html.contains("![["));
        assert!(html.contains("<code>**literal code** ==not highlighted==</code>"));
        let title = render_markdown_for_export("> [!NOTE] **Bold** and *italic*\n> Body");
        assert!(title.contains("<strong>Bold</strong> and <em>italic</em>"));
    }

    #[test]
    fn exported_footnote_label_stays_inline_with_its_first_paragraph() {
        let markdown =
            "Text[^note].\n\n[^note]: First **paragraph**.\n\nSecond paragraph.\n\n- List item\n";
        let fragment = render_markdown_for_export(markdown);
        assert!(
            fragment.contains("class=\"footnote-reference\""),
            "{fragment}"
        );
        assert!(
            fragment.contains("class=\"footnote-definition\" id=\"note\""),
            "{fragment}"
        );
        assert!(
            fragment.contains("class=\"footnote-definition-label\">note</sup>\n<p>First <strong>paragraph</strong>."),
            "{fragment}"
        );
        assert!(fragment.contains("<p>Second paragraph.</p>"), "{fragment}");
        assert!(fragment.contains("<li>List item</li>"), "{fragment}");
        let document = build_html_document(
            "Footnotes",
            markdown,
            None,
            ExportLayout::from_settings(&AppSettings::default()),
        );
        assert!(document.contains(".footnote-definition>sup+p{display:inline;margin:0;}"));
        assert!(fragment.contains("<a href=\"#note\">note</a>"));
        assert!(!fragment.contains(">1</"));
        let multiple = render_markdown_for_export(
            "A[^中文] B[^42] C[^a&b].\n\n[^中文]: 中文\n[^42]: Number\n[^a&b]: Escaped\n",
        );
        for label in ["中文", "42", "a&amp;b"] {
            assert!(
                multiple.contains(&format!("<a href=\"#{label}\">{label}</a>")),
                "{multiple}"
            );
            assert!(
                multiple.contains(&format!(
                    "class=\"footnote-definition-label\">{label}</sup>"
                )),
                "{multiple}"
            );
        }
    }

    #[test]
    fn help_mindmap_export_uses_colored_branches_and_original_footnote_labels() {
        let help = include_str!("../../docs/help-zh-cn.md");
        let source = help
            .split_once("```mermaid\n")
            .unwrap()
            .1
            .split_once("```")
            .unwrap()
            .0;
        let fragment = render_markdown_for_export(&format!("```mermaid\n{source}```\n"));
        let theme = crate::theme::ThemeKind::PaperWhite.build();
        for color in [
            helpers::mermaid_accent_color(&theme),
            theme.status_success,
            theme.text_warning,
            theme.text_danger,
        ] {
            assert!(
                fragment.contains(&format!("stroke=\"{}\"", helpers::svg_color(color))),
                "{fragment}"
            );
        }
        let footnote = render_markdown_for_export("Reference[^note]\n\n- [^note]: Target\n");
        assert!(
            footnote.contains("<a href=\"#note\">note</a>"),
            "{footnote}"
        );
        assert!(
            footnote.contains("class=\"footnote-definition-label\">note</sup>"),
            "{footnote}"
        );
    }

    #[test]
    fn additional_syntaxes_highlight_exported_code_blocks() {
        for (language, source) in [
            ("bicep", "param name string = 'world'"),
            ("meson", "project('hello', 'c')"),
            ("wasm", "(module (func (result i32) i32.const 42))"),
            ("webidl", "interface Example { attribute DOMString name; };"),
            ("thrift", "struct User { 1: string name }"),
            ("flatbuffers", "table User { name:string; }"),
            ("mermaid", "flowchart TD\nA-->B"),
            ("plantuml", "@startuml\nactor User\n@enduml"),
            ("d2", "a -> b: hello"),
            ("haxe", "class Main { static function main() {} }"),
            (
                "cuda",
                "__global__ void add(float *x) { x[threadIdx.x] += 1; }",
            ),
            (
                "hlsl",
                "float4 main() : SV_Target { return float4(1,0,0,1); }",
            ),
            ("metal", "kernel void add(device float *x [[buffer(0)]]) {}"),
            (
                "gdshader",
                "shader_type spatial;\nvoid fragment() { ALBEDO = vec3(1.0); }",
            ),
            ("luau", "export type Name = string\nlocal name = \"hello\""),
            ("mojo", "fn main():\n    print(\"hello\")"),
            (
                "gherkin",
                "Feature: Greeting\n  Given a user named \"hello\"",
            ),
            (
                "yara",
                "rule Example { strings: $text = \"hello\" condition: $text }",
            ),
            ("pkl", "name: String = \"hello\""),
            ("dhall", "let name : Text = \"hello\" in name"),
            ("nickel", "let name = \"hello\" in { name }"),
            ("rescript", "let name: string = \"hello\""),
            ("abap", "DATA name TYPE string VALUE 'hello'."),
            ("ahk", "MsgBox \"hello\""),
            ("openscad", "module box() { cube([1, 2, 3]); }"),
            ("stan", "parameters { real mu; }"),
            ("typespec", "model User { name: string; }"),
            ("gcode", "G1 X10.5 Y20 F1200"),
            ("ron", "Config(name: \"hello\", enabled: true)"),
            ("smithy", "structure User { name: String }"),
            ("kcl", "schema User:\n    name: str"),
            ("hurl", "GET https://example.invalid\nHTTP 200"),
            ("procfile", "web: echo \"hello\""),
            ("codeowners", "/src/ @example/team"),
        ] {
            let kind = CodeBlockKind::Fenced(language.into());
            let html = highlight_code_block_html(&kind, source).expect(language);
            assert!(html.contains("<span style="), "{language}: {html}");
        }
        assert!(
            highlight_code_block_html(
                &CodeBlockKind::Fenced("not-a-real-language".into()),
                "param name string"
            )
            .is_none()
        );
    }

    #[test]
    fn export_file_url_encoding_does_not_require_an_existing_path() {
        let path = PathBuf::from(if cfg!(windows) {
            "C:/unavailable mount #1/note.md"
        } else {
            "/unavailable mount #1/note.md"
        });
        assert_eq!(
            super::html_base_href(Some(&path)).unwrap(),
            if cfg!(windows) {
                "file:///C:/unavailable%20mount%20%231/"
            } else {
                "file:///unavailable%20mount%20%231/"
            }
        );
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
            &html::default_typography_css(),
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
    fn bundled_languages_use_the_shared_registry_for_export_highlighting() {
        let palette = crate::theme::ThemeKind::PaperWhite.build().code_palette;
        let foreground = format!("color:#{:06x}", palette.foreground);
        for (language, example) in crate::syntax::bundled_syntax_samples() {
            let fence_language = match language {
                "DNS Zone" => "dns-zone",
                "Visual Basic .NET" => "vbnet",
                "Cap'n Proto" => "capnp",
                language => language,
            };
            let kind = CodeBlockKind::Fenced(fence_language.into());
            let html = highlight_code_block_html(&kind, example)
                .unwrap_or_else(|| panic!("{language} did not resolve for export"));
            let colors = html
                .split("style=\"")
                .skip(1)
                .filter_map(|part| part.split('"').next())
                .filter(|style| style.contains("color:"))
                .collect::<Vec<_>>();
            assert!(
                colors
                    .iter()
                    .any(|style| !style.to_ascii_lowercase().contains(&foreground)),
                "{language}: {html}"
            );
            if language != "Mermaid" {
                let markdown = format!("```{fence_language}\n{example}```\n");
                let exported = super::render_markdown_for_export(&markdown);
                assert!(exported.contains(&html), "{language}: {exported}");
            }
        }
        assert!(
            highlight_code_block_html(
                &CodeBlockKind::Fenced("not-a-real-language".into()),
                "hello"
            )
            .is_none()
        );
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
            doc.contains(".markdown-body table{display:table;width:100%;max-width:100%"),
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
        assert!(html.contains("<h1 id=\"adbc\">ad|bc</h1>"));
    }

    #[test]
    fn export_preserves_html_breaks_inside_table_cells() {
        let html = super::render_markdown_for_export(
            "| A | B |\n| --- | --- |\n| first<br>second | value |\n",
        );

        assert!(html.contains("first<br>second"));
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
            Some(Path::new(if cfg!(windows) {
                "C:/tmp/demo/note.md"
            } else {
                "/tmp/demo/note.md"
            })),
            super::ExportLayout::from_settings(&super::AppSettings::default()),
        );

        assert!(html.contains("@page{size:802px "));
        assert!(html.contains("margin:51px;}"));
        assert!(html.contains(if cfg!(windows) {
            "<base href=\"file:///C:/tmp/demo/\">"
        } else {
            "<base href=\"file:///tmp/demo/\">"
        }));
    }
}
