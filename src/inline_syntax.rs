//! Source-preserving CJK delimiter flanking extension shared by display and export.
//!
//! CommonMark treats CJK letters as word characters: `前**你好。**后` cannot
//! close strong emphasis. Treat CJK neighbours of punctuation-ended delimiter
//! runs as punctuation for flanking only, then let pulldown handle pairing,
//! nesting and the rule of three. Underscores additionally allow CJK word
//! boundaries; ASCII/Western intraword underscores are unchanged. Equal-width
//! sentinels keep every source byte offset stable and are restored in text events.

use pulldown_cmark::{BrokenLink, CowStr, Event, LinkType, Tag, TagEnd};
use regex::Regex;
use std::borrow::Cow;
use std::collections::HashSet;
use std::ops::Range;
use std::sync::OnceLock;

use super::{is_escaped_at, merge_byte_ranges, protected_inline_html_tag, raw_wiki_link_ranges};

pub(crate) struct CjkInlineSource<'a> {
    source: Cow<'a, str>,
    replacements: Vec<(usize, char)>,
    markers: [Option<char>; 2],
    references: Vec<ResolvedReference>,
}

struct ResolvedReference {
    range: Range<usize>,
    destination: CowStr<'static>,
    title: CowStr<'static>,
}

fn is_cjk_letter(ch: char) -> bool {
    ch.is_alphabetic()
        && matches!(ch as u32,
            0x2E80..=0xA4CF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF
            | 0xFF66..=0xFFDC | 0x20000..=0x323AF)
}

fn is_punctuation(ch: char) -> bool {
    static PUNCTUATION: OnceLock<Regex> = OnceLock::new();
    let mut bytes = [0; 4];
    PUNCTUATION
        .get_or_init(|| Regex::new(r"^[\p{P}\p{S}]$").unwrap())
        .is_match(ch.encode_utf8(&mut bytes))
}

impl<'a> CjkInlineSource<'a> {
    pub(crate) fn new<'e, I>(source: &'a str, events: impl FnOnce() -> I) -> Self
    where
        I: Iterator<Item = (Event<'e>, Range<usize>)>,
    {
        let mut candidates = Vec::new();
        let mut index = 0;
        while index < source.len() {
            let delimiter = source.as_bytes()[index];
            if !matches!(delimiter, b'*' | b'_' | b'~') {
                index += source[index..].chars().next().unwrap().len_utf8();
                continue;
            }
            let start = index;
            while source.as_bytes().get(index) == Some(&delimiter) {
                index += 1;
            }
            if is_escaped_at(source, start) || (delimiter == b'~' && index - start < 2) {
                continue;
            }
            let before = source[..start].chars().next_back();
            let after = source[index..].chars().next();
            if let Some(ch) = before.filter(|ch| is_cjk_letter(*ch))
                && (delimiter == b'_' || after.is_some_and(is_punctuation))
            {
                candidates.push((start - ch.len_utf8(), ch));
            }
            if let Some(ch) = after.filter(|ch| is_cjk_letter(*ch))
                && (delimiter == b'_' || before.is_some_and(is_punctuation))
            {
                candidates.push((index, ch));
            }
        }
        let mut result = Self {
            source: Cow::Borrowed(source),
            replacements: Vec::new(),
            markers: [None, None],
            references: Vec::new(),
        };
        if candidates.is_empty() {
            return result;
        }

        // Only text emitted by the real parser may be adapted. This excludes
        // destinations, reference identifiers, attributes, escapes, code and math.
        let mut text_ranges = Vec::new();
        let mut html_stack = Vec::new();
        let mut opaque_depth = 0usize;
        let mut autolinks = Vec::new();
        let mut used_characters = source.chars().collect::<HashSet<_>>();
        for (event, range) in events() {
            match event {
                Event::Start(Tag::CodeBlock(_) | Tag::HtmlBlock | Tag::Image { .. }) => {
                    opaque_depth += 1;
                }
                Event::Start(Tag::Link {
                    link_type,
                    dest_url,
                    title,
                    ..
                }) => {
                    if matches!(
                        link_type,
                        LinkType::Shortcut
                            | LinkType::ShortcutUnknown
                            | LinkType::Collapsed
                            | LinkType::CollapsedUnknown
                    ) {
                        // These labels are also reference lookup keys. The
                        // adapted spelling must not sever their original link,
                        // including links resolved from inherited cell context.
                        result.references.push(ResolvedReference {
                            range,
                            destination: dest_url.into_static(),
                            title: title.into_static(),
                        });
                    }
                    let opaque = matches!(link_type, LinkType::Autolink | LinkType::Email);
                    autolinks.push(opaque);
                    opaque_depth += usize::from(opaque);
                }
                Event::End(TagEnd::CodeBlock | TagEnd::HtmlBlock | TagEnd::Image) => {
                    opaque_depth = opaque_depth.saturating_sub(1);
                }
                Event::End(TagEnd::Link) => {
                    if autolinks.pop() == Some(true) {
                        opaque_depth = opaque_depth.saturating_sub(1);
                    }
                }
                Event::InlineHtml(ref html) => {
                    if let Some((tag, closing, self_closing)) = protected_inline_html_tag(html) {
                        if closing {
                            if let Some(position) = html_stack.iter().rposition(|open| *open == tag)
                            {
                                html_stack.truncate(position);
                            }
                        } else if !self_closing {
                            html_stack.push(tag);
                        }
                    }
                }
                Event::Text(ref text) => {
                    // A sentinel must also be absent from decoded entities, not
                    // just literal source, so restoration can never consume one.
                    used_characters.extend(text.chars());
                    if opaque_depth == 0 && html_stack.is_empty() {
                        text_ranges.push(range);
                    }
                }
                _ => {}
            }
        }
        result
            .references
            .sort_by_key(|reference| reference.range.start);
        text_ranges.sort_by_key(|range| range.start);
        let is_text = |offset: usize, length: usize| {
            let position = text_ranges.partition_point(|range| range.end <= offset);
            text_ranges
                .get(position)
                .is_some_and(|range| range.start <= offset && offset + length <= range.end)
        };
        let mut excluded = Vec::new();
        let mut line_offset = 0;
        let mut math_open: Option<(&str, usize)> = None;
        for line in source.split_inclusive('\n') {
            let trimmed = line.trim();
            if let Some((closer, start)) = math_open {
                if trimmed == closer {
                    excluded.push(start..line_offset + line.len());
                    math_open = None;
                }
            } else if trimmed.starts_with("\\[") && trimmed.ends_with("\\]") {
                excluded.push(line_offset..line_offset + line.len());
            } else if matches!(trimmed, "\\[" | "$$")
                && is_text(
                    line_offset + line.find(trimmed).unwrap() + usize::from(trimmed == "\\["),
                    1,
                )
            {
                math_open = Some((if trimmed == "\\[" { "\\]" } else { "$$" }, line_offset));
            }
            for range in raw_wiki_link_ranges(line, None) {
                let target_end = line[range.start + 2..range.end - 2]
                    .find('|')
                    .map_or(range.end, |separator| range.start + 2 + separator + 1);
                excluded.push(line_offset + range.start..line_offset + target_end);
            }
            line_offset += line.len();
        }
        // Bare URIs are not native links in pulldown. Never change their
        // punctuation (including non-HTTP schemes and table-cell destinations).
        static URI: OnceLock<Regex> = OnceLock::new();
        excluded.extend(
            URI.get_or_init(|| Regex::new(r"[A-Za-z][A-Za-z0-9+.-]*:[^\s<>|]+").unwrap())
                .find_iter(source)
                .map(|matched| matched.range()),
        );
        let excluded = merge_byte_ranges(excluded);
        candidates.sort_unstable();
        candidates.dedup();
        candidates.retain(|(offset, ch)| {
            let position = excluded.partition_point(|range| range.end <= *offset);
            is_text(*offset, ch.len_utf8())
                && !excluded
                    .get(position)
                    .is_some_and(|range| range.contains(offset))
        });
        if candidates.is_empty() {
            return result;
        }
        for width in [3, 4] {
            if candidates.iter().any(|(_, ch)| ch.len_utf8() == width) {
                let start = if width == 3 { 0x2E00 } else { 0x10100 };
                let end = if width == 3 { 0xFFFF } else { 0x10FFFF };
                result.markers[width - 3] = (start..=end)
                    .filter_map(char::from_u32)
                    .find(|ch| !used_characters.contains(ch) && is_punctuation(*ch));
            }
        }
        let mut adapted = source.to_string();
        for (offset, ch) in candidates {
            if let Some(marker) = result.markers[ch.len_utf8() - 3] {
                adapted.replace_range(offset..offset + ch.len_utf8(), &marker.to_string());
                result.replacements.push((offset, ch));
            }
        }
        result.source = Cow::Owned(adapted);
        result
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.source
    }

    /// Recover self-labelled references by unchanged source coordinates, not by
    /// a masked or re-normalized key. External definitions must go through
    /// `reference_allows_fallback` so changed unknown keys stay unresolved.
    pub(crate) fn resolve_reference(
        &self,
        link: &BrokenLink<'_>,
    ) -> Option<(CowStr<'static>, CowStr<'static>)> {
        if !matches!(link.link_type, LinkType::Shortcut | LinkType::Collapsed) {
            return None;
        }
        let index = self
            .references
            .binary_search_by_key(&link.span.start, |reference| reference.range.start)
            .ok()?;
        let reference = &self.references[index];
        (reference.range == link.span)
            .then(|| (reference.destination.clone(), reference.title.clone()))
    }

    pub(crate) fn reference_allows_fallback(&self, link: &BrokenLink<'_>) -> bool {
        if !matches!(link.link_type, LinkType::Shortcut | LinkType::Collapsed) {
            return true;
        }
        let index = self
            .replacements
            .partition_point(|(offset, _)| *offset < link.span.start);
        // After the original resolution failed, an adapted self-labelled key
        // must not create a new inherited link from its sentinel spelling.
        !self
            .replacements
            .get(index)
            .is_some_and(|(offset, _)| *offset < link.span.end)
    }

    pub(crate) fn restore_event<'e>(&self, event: Event<'e>, range: Range<usize>) -> Event<'e> {
        let Event::Text(text) = event else {
            return event;
        };
        let start = self
            .replacements
            .partition_point(|(offset, _)| *offset < range.start);
        let mut replacements = self.replacements[start..]
            .iter()
            .take_while(|(offset, _)| *offset < range.end);
        let mut next = replacements.next();
        if next.is_none() {
            return Event::Text(text);
        }
        let mut restored = String::with_capacity(text.len());
        for ch in text.chars() {
            if self.markers.contains(&Some(ch))
                && let Some((_, original)) = next
            {
                restored.push(*original);
                next = replacements.next();
            } else {
                restored.push(ch);
            }
        }
        Event::Text(restored.into())
    }
}

#[cfg(test)]
mod tests {
    use super::CjkInlineSource;
    use pulldown_cmark::{Options, Parser, html::push_html};

    #[test]
    fn cjk_reference_labels_unknown_fallback_is_limited_to_unchanged_keys() {
        use pulldown_cmark::{BrokenLink, LinkType};
        for (source, span, link_type, allowed) in [
            ("[前**你好。**后]", 0..21, LinkType::Shortcut, false),
            ("[前**你好。**后][]", 0..23, LinkType::Collapsed, false),
            ("[前__你好。__后]", 0..21, LinkType::Shortcut, false),
            ("[前__你好。__后][]", 0..23, LinkType::Collapsed, false),
            ("[前**你好。**后][key]", 0..26, LinkType::Reference, true),
            ("[plain] 前**你好。**后", 0..7, LinkType::Shortcut, true),
        ] {
            let adapted = CjkInlineSource::new(source, || Parser::new(source).into_offset_iter());
            assert_ne!(adapted.as_str(), source);
            let link = BrokenLink {
                span,
                link_type,
                reference: "key".into(),
            };
            assert_eq!(
                adapted.reference_allows_fallback(&link),
                allowed,
                "{source}"
            );
        }
    }

    #[test]
    fn cjk_reference_labels_preserve_resolved_links_and_visible_text() {
        for (label, tag, visible) in [
            ("前**你好。**后", "strong", "前<strong>你好。</strong>后"),
            ("前__你好。__后", "strong", "前<strong>你好。</strong>后"),
            ("前_你好。_后", "em", "前<em>你好。</em>后"),
        ] {
            for suffix in ["", "[]"] {
                let source =
                    format!("[{label}]{suffix}\n\n[{label}]: /target \"original title\"\n");
                let adapted = CjkInlineSource::new(&source, || {
                    Parser::new_ext(&source, Options::all()).into_offset_iter()
                });
                let mut html = String::new();
                let parser = Parser::new_with_broken_link_callback(
                    adapted.as_str(),
                    Options::all(),
                    Some(|link: pulldown_cmark::BrokenLink<'_>| adapted.resolve_reference(&link)),
                )
                .into_offset_iter()
                .map(|(event, range)| adapted.restore_event(event, range));
                push_html(&mut html, parser);
                assert!(
                    html.contains("href=\"/target\" title=\"original title\""),
                    "{source}: {html}"
                );
                assert!(html.contains(visible), "{source}: {html}");
                assert!(html.contains(&format!("<{tag}>")), "{html}");
            }
        }
    }

    #[test]
    fn cjk_reference_labels_keep_unicode_matching_and_reference_kinds_distinct() {
        for (label, definition_label) in [
            ("前**你好。**后 MIXED   WORDS", "前**你好。**后 mixed words"),
            ("前__你好。__后\nMIXED", "前__你好。__后 mixed"),
            ("前**你好。**后 ẞ", "前**你好。**后 ss"),
        ] {
            for suffix in ["", "[]", "[explicit]"] {
                let source = format!(
                    "[{label}]{suffix}\n\n[{definition_label}]: /implicit \"implicit title\"\n[explicit]: /explicit \"explicit title\"\n"
                );
                let adapted = CjkInlineSource::new(&source, || {
                    Parser::new_ext(&source, Options::all()).into_offset_iter()
                });
                let mut html = String::new();
                push_html(
                    &mut html,
                    Parser::new_with_broken_link_callback(
                        adapted.as_str(),
                        Options::all(),
                        Some(|link: pulldown_cmark::BrokenLink<'_>| {
                            adapted.resolve_reference(&link)
                        }),
                    )
                    .into_offset_iter()
                    .map(|(event, range)| {
                        assert!(source.get(range.clone()).is_some());
                        adapted.restore_event(event, range)
                    }),
                );
                let target = if suffix == "[explicit]" {
                    "explicit"
                } else {
                    "implicit"
                };
                assert!(
                    html.contains(&format!("href=\"/{target}\" title=\"{target} title\"")),
                    "{source}: {html}"
                );
                assert!(html.contains("前<strong>你好。</strong>后"), "{html}");
                assert_eq!(html.matches("<a ").count(), 1, "{html}");
            }
        }
        let source = "[missing前**你好。**后]\n\n[前**你好。**后]: /target\n";
        let adapted = CjkInlineSource::new(source, || Parser::new(source).into_offset_iter());
        let mut html = String::new();
        push_html(
            &mut html,
            Parser::new_with_broken_link_callback(
                adapted.as_str(),
                Options::empty(),
                Some(|link: pulldown_cmark::BrokenLink<'_>| adapted.resolve_reference(&link)),
            )
            .into_offset_iter()
            .map(|(event, range)| adapted.restore_event(event, range)),
        );
        assert!(!html.contains("<a "), "{html}");
        assert!(
            html.contains("[missing前<strong>你好。</strong>后]"),
            "{html}"
        );
    }

    #[test]
    fn cjk_inline_punctuation_keeps_opaque_islands_unchanged() {
        for opaque in [
            "`前**你好。**后`",
            "```\n$$\n前**你好。**后\n```",
            "```\n\\[\n前**你好。**后\n```",
            "$前**你好。**后$",
            "\\[\n前**你好。**后\n\\]",
            "$$\n\n前**你好。**后\n\n$$",
            "<code>前**你好。**后</code>",
            "<pre>前**你好。**后</pre>",
            "[label](前**你好。**后)",
            "![前**你好。**后](image.png)",
            "https://host/前**你好。**后",
            "[[前**你好。**后|label]]",
            r"前\**你好。\**后",
        ] {
            let source = format!("{opaque}\n\n前**你好。**后");
            let adapted = CjkInlineSource::new(&source, || {
                Parser::new_ext(&source, Options::all()).into_offset_iter()
            });
            assert!(
                adapted.as_str().starts_with(opaque),
                "{source}: {}",
                adapted.as_str()
            );
            assert_ne!(adapted.as_str(), source, "tail must still adapt: {source}");
            assert_eq!(adapted.as_str().len(), source.len());
        }
    }

    #[test]
    fn cjk_inline_punctuation_restores_entities_escapes_and_all_character_widths() {
        for source in [
            "⸀ &#x2e00; 𐄀 &#x10100; 甲**你好。**乙*「世界」*丙 &amp;",
            "𠮷**你好。**𠮷",
            r"\< 前**你好。**后 \\ 前**「世界」**后",
            "前_你好_后",
            "前**&#x3002;**后",
        ] {
            let adapted = CjkInlineSource::new(source, || {
                Parser::new_ext(source, Options::all()).into_offset_iter()
            });
            assert_eq!(adapted.as_str().len(), source.len());
            let mut html = String::new();
            let parser = Parser::new_ext(adapted.as_str(), Options::all())
                .into_offset_iter()
                .map(|(event, range)| {
                    assert!(source.get(range.clone()).is_some(), "{source}: {range:?}");
                    adapted.restore_event(event, range)
                });
            push_html(&mut html, parser);
            assert!(!html.contains('⸁'), "sentinel leaked: {html}");
            assert!(
                html.contains("<strong>") || html.contains("<em>"),
                "{source}: {html}"
            );
            if source.contains("甲") {
                assert!(
                    html.contains("⸀ ⸀ 𐄀 𐄀 甲<strong>你好。</strong>乙<em>「世界」</em>丙 &amp;"),
                    "{html}"
                );
            }
            if source.starts_with('𠮷') {
                assert!(html.contains("𠮷<strong>你好。</strong>𠮷"), "{html}");
            }
        }
    }
}
