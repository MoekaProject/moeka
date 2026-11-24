use std::ops::Range;
use std::path::Path;

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct SearchPathFilters {
    pub include: String,
    pub exclude: String,
}

#[derive(Clone, Debug, Default)]
pub struct CompiledSearchPathFilters {
    include: Option<regex::Regex>,
    exclude: Option<regex::Regex>,
}

impl SearchPathFilters {
    pub fn compile(&self) -> Result<CompiledSearchPathFilters, regex::Error> {
        let compile = |pattern: &str| {
            let pattern = pattern.trim();
            if pattern.is_empty() {
                Ok(None)
            } else {
                regex::Regex::new(pattern).map(Some)
            }
        };
        Ok(CompiledSearchPathFilters {
            include: compile(&self.include)?,
            exclude: compile(&self.exclude)?,
        })
    }
}

impl CompiledSearchPathFilters {
    pub fn is_excluded(&self, relative: &str) -> bool {
        !relative.is_empty()
            && self.exclude.as_ref().is_some_and(|exclude| {
                exclude.is_match(relative)
                    || relative
                        .match_indices('/')
                        .any(|(index, _)| exclude.is_match(&relative[..index]))
            })
    }

    pub fn accepts_file(&self, relative: &str) -> bool {
        relative.is_empty()
            || (!self.is_excluded(relative)
                && self
                    .include
                    .as_ref()
                    .is_none_or(|include| include.is_match(relative)))
    }
}

/// Comparison text only; resource identities always retain their original native units.
pub fn relative_path_text(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

/// Resumable exact matching without retaining a range or UTF-8 map per match/byte.
#[derive(Clone)]
pub struct SearchCursor {
    matcher: Matcher,
    whole_word: bool,
    offset: usize,
}

#[derive(Clone)]
enum Matcher {
    Literal(String),
    Regex(regex::Regex),
    Folded {
        needle: Vec<u8>,
        prefix: Vec<usize>,
        matched: usize,
        byte_count: usize,
        starts: Vec<usize>,
        mapped: usize,
        bytes: Box<[(u8, usize); 12]>,
        length: usize,
        position: usize,
    },
}

impl SearchCursor {
    pub fn new(
        query: &str,
        case_sensitive: bool,
        whole_word: bool,
        use_regex: bool,
    ) -> Result<Self, regex::Error> {
        let matcher = if query.is_empty() {
            Matcher::Literal(String::new())
        } else if use_regex {
            Matcher::Regex(
                regex::RegexBuilder::new(query)
                    .case_insensitive(!case_sensitive)
                    .build()?,
            )
        } else if case_sensitive {
            Matcher::Literal(query.to_owned())
        } else {
            let needle = query.to_lowercase().into_bytes();
            let mut prefix = vec![0; needle.len()];
            for index in 1..needle.len() {
                let mut count = prefix[index - 1];
                while count > 0 && needle[index] != needle[count] {
                    count = prefix[count - 1];
                }
                if needle[index] == needle[count] {
                    count += 1;
                }
                prefix[index] = count;
            }
            Matcher::Folded {
                starts: vec![0; needle.len()],
                needle,
                prefix,
                matched: 0,
                byte_count: 0,
                mapped: 0,
                bytes: Box::new([(0, 0); 12]),
                length: 0,
                position: 0,
            }
        };
        Ok(Self {
            matcher,
            whole_word,
            offset: 0,
        })
    }

    pub fn next_match(&mut self, text: &str) -> Option<Range<usize>> {
        match &mut self.matcher {
            Matcher::Literal(needle) => {
                if needle.is_empty() {
                    return None;
                }
                while self.offset < text.len() {
                    let start = self.offset + text[self.offset..].find(needle.as_str())?;
                    let end = start + needle.len();
                    if self.whole_word
                        && (!is_word_boundary(text, start) || !is_word_boundary(text, end))
                    {
                        self.offset = next_char_boundary(text, start);
                        continue;
                    }
                    self.offset = end;
                    return Some(start..end);
                }
                None
            }
            Matcher::Regex(regex) => {
                while self.offset <= text.len() {
                    let found = regex.find_at(text, self.offset)?;
                    if found.is_empty() {
                        if found.end() == text.len() {
                            return None;
                        }
                        self.offset = next_char_boundary(text, found.end());
                        continue;
                    }
                    self.offset = found.end();
                    if !self.whole_word
                        || is_word_boundary(text, found.start())
                            && is_word_boundary(text, found.end())
                    {
                        return Some(found.range());
                    }
                }
                None
            }
            Matcher::Folded {
                needle,
                prefix,
                matched,
                byte_count,
                starts,
                mapped,
                bytes,
                length,
                position,
            } => loop {
                if *position == *length {
                    let character = text[self.offset..].chars().next()?;
                    let start = self.offset;
                    self.offset += character.len_utf8();
                    *length = 0;
                    *position = 0;
                    if character.is_ascii() {
                        bytes[0] = (character.to_ascii_lowercase() as u8, self.offset);
                        *length = 1;
                    } else {
                        for lower in character.to_lowercase() {
                            let mut encoded = [0; 4];
                            let encoded = lower.encode_utf8(&mut encoded).as_bytes();
                            for (index, byte) in encoded.iter().enumerate() {
                                bytes[*length] = (
                                    *byte,
                                    if index + 1 == encoded.len() {
                                        self.offset
                                    } else {
                                        start
                                    },
                                );
                                *length += 1;
                            }
                        }
                    }
                }
                let (byte, end) = bytes[*position];
                *position += 1;
                starts[*byte_count % needle.len()] = *mapped;
                *mapped = end;
                *byte_count += 1;
                while *matched > 0 && byte != needle[*matched] {
                    *matched = prefix[*matched - 1];
                }
                if byte == needle[*matched] {
                    *matched += 1;
                }
                if *matched == needle.len() {
                    let start = starts[(*byte_count - needle.len()) % needle.len()];
                    if self.whole_word
                        && (!is_word_boundary(text, start) || !is_word_boundary(text, end))
                    {
                        *matched = prefix[*matched - 1];
                    } else {
                        *matched = 0;
                        return Some(start..end);
                    }
                }
            },
        }
    }
}

/// The editor and workspace search share original UTF-8 byte ranges.
pub fn find_search_matches(
    text: &str,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    use_regex: bool,
) -> Vec<Range<usize>> {
    let Ok(mut cursor) = SearchCursor::new(query, case_sensitive, whole_word, use_regex) else {
        return Vec::new();
    };
    std::iter::from_fn(|| cursor.next_match(text)).collect()
}

#[cfg(test)]
fn reference_search_matches(
    text: &str,
    query: &str,
    case_sensitive: bool,
    whole_word: bool,
    use_regex: bool,
) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }

    let mut matches = Vec::new();

    if use_regex {
        let regex = if case_sensitive {
            regex::Regex::new(query)
        } else {
            regex::RegexBuilder::new(query)
                .case_insensitive(true)
                .build()
        };
        if let Ok(regex) = regex {
            for found in regex.find_iter(text) {
                if found.start() != found.end()
                    && (!whole_word
                        || (is_word_boundary(text, found.start())
                            && is_word_boundary(text, found.end())))
                {
                    matches.push(found.start()..found.end());
                }
            }
        }
        return matches;
    }

    let (haystack, byte_map): (std::borrow::Cow<'_, str>, Option<Vec<usize>>) = if case_sensitive {
        (std::borrow::Cow::Borrowed(text), None)
    } else if text.is_ascii() && query.is_ascii() {
        (std::borrow::Cow::Owned(text.to_ascii_lowercase()), None)
    } else {
        let (folded, map) = fold_with_original_byte_map(text);
        (std::borrow::Cow::Owned(folded), Some(map))
    };
    let needle = if case_sensitive {
        std::borrow::Cow::Borrowed(query)
    } else if query.is_ascii() {
        std::borrow::Cow::Owned(query.to_ascii_lowercase())
    } else {
        std::borrow::Cow::Owned(query.to_lowercase())
    };
    let needle = needle.as_ref();

    let mut start = 0;
    while let Some(position) = haystack[start..].find(needle) {
        let folded_match_start = start + position;
        let folded_match_end = folded_match_start + needle.len();
        let (match_start, match_end) = if let Some(byte_map) = &byte_map {
            (byte_map[folded_match_start], byte_map[folded_match_end])
        } else {
            (folded_match_start, folded_match_end)
        };

        if whole_word
            && (!is_word_boundary(text, match_start) || !is_word_boundary(text, match_end))
        {
            start = next_char_boundary(&haystack, folded_match_start);
            continue;
        }

        matches.push(match_start..match_end);
        start = folded_match_end;
    }

    matches
}

#[cfg(test)]
fn fold_with_original_byte_map(text: &str) -> (String, Vec<usize>) {
    let mut folded = String::new();
    let mut byte_map = Vec::with_capacity(text.len() + 1);
    byte_map.push(0);

    for (original_start, ch) in text.char_indices() {
        let original_end = original_start + ch.len_utf8();
        for folded_ch in ch.to_lowercase() {
            let folded_len = folded_ch.len_utf8();
            folded.push(folded_ch);
            for _ in 1..folded_len {
                byte_map.push(original_start);
            }
            byte_map.push(original_end);
        }
    }

    (folded, byte_map)
}

fn next_char_boundary(text: &str, index: usize) -> usize {
    if index >= text.len() {
        return text.len();
    }
    let next = index + text[index..].chars().next().map_or(1, char::len_utf8);
    next.min(text.len())
}

pub fn is_word_boundary(text: &str, index: usize) -> bool {
    if index == 0 || index >= text.len() {
        return true;
    }

    let before = text[..index].chars().next_back();
    let after = text[index..].chars().next();
    !before.is_some_and(is_word_char) || !after.is_some_and(is_word_char)
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

pub fn should_skip_path(path: &Path, is_dir: bool) -> bool {
    let Some(name) = path.file_name() else {
        return false;
    };
    let name = name.to_string_lossy();
    should_skip_name(&name, is_dir)
}

pub fn should_skip_name(name: &str, is_dir: bool) -> bool {
    if name.starts_with('.') {
        return true;
    }
    is_dir
        && matches!(
            name,
            "node_modules" | "target" | "dist" | "build" | "coverage" | ".git"
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_filters_apply_ancestor_exclusions_without_pruning_for_includes() {
        let filters = SearchPathFilters {
            include: r" \.md$ ".into(),
            exclude: " ^skip$|secret ".into(),
        }
        .compile()
        .unwrap();
        assert!(filters.accepts_file("notes/file.md"));
        assert!(!filters.accepts_file("notes/file.txt"));
        assert!(!filters.is_excluded("notes"));
        for path in ["skip", "skip/file.md", "notes/secret/file.md"] {
            assert!(filters.is_excluded(path));
            assert!(!filters.accepts_file(path));
        }
        assert!(filters.accepts_file("skipfile/file.md"));
        assert!(!filters.accepts_file("notes/FILE.MD"));
        assert!(filters.accepts_file(""));
        assert!(!filters.is_excluded(""));
        assert!(
            SearchPathFilters {
                include: "[".into(),
                exclude: String::new()
            }
            .compile()
            .is_err()
        );
        assert!(
            SearchPathFilters {
                include: String::new(),
                exclude: "[".into()
            }
            .compile()
            .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn path_filter_comparison_keeps_unix_backslashes_and_handles_non_utf8_names() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let relative = Path::new(OsStr::from_bytes(b"notes/draft\\\xff.md"));
        let text = relative_path_text(relative);
        assert_eq!(text, "notes/draft\\\u{fffd}.md");
        let filters = SearchPathFilters {
            include: r"^notes/draft\\.*\.md$".into(),
            exclude: String::new(),
        }
        .compile()
        .unwrap();
        assert!(filters.accepts_file(&text));
        assert!(should_skip_path(
            Path::new(OsStr::from_bytes(b".hidden-\xff")),
            false
        ));
        assert!(!should_skip_path(
            Path::new(OsStr::from_bytes(b"visible-\xff")),
            false
        ));
    }

    #[test]
    fn resumable_matching_agrees_with_existing_editor_semantics() {
        for text in [
            "word word words\nword",
            "Äpfel äPFEL\nİstanbul i\u{307}stanbul",
            "ßSS Kelvin KELVIN\nΣσς",
            "中文 原稿 中",
            "aaa aa a",
            "ab\nba",
            "",
        ] {
            for query in [
                "word", "äpfel", "İ", "i", "\u{307}", "ß", "K", "σ", "中", "aa", "", "a*|b", "a*?",
                "(?m)^.*$", "\\b",
            ] {
                for case_sensitive in [false, true] {
                    for whole_word in [false, true] {
                        for regex in [false, true] {
                            let expected = reference_search_matches(
                                text,
                                query,
                                case_sensitive,
                                whole_word,
                                regex,
                            );
                            let mut cursor =
                                SearchCursor::new(query, case_sensitive, whole_word, regex)
                                    .unwrap();
                            let actual: Vec<_> =
                                std::iter::from_fn(|| cursor.next_match(text)).collect();
                            assert_eq!(
                                actual, expected,
                                "{text:?} {query:?} {case_sensitive} {whole_word} {regex}"
                            );
                        }
                    }
                }
            }
        }
    }
}
