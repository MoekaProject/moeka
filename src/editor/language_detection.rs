use super::{FileKind, helpers};

fn code_file_kind_for_syntax_name(name: &str) -> Option<FileKind> {
    (!matches!(name, "Plain Text" | "Markdown")).then(|| FileKind::Code {
        language: name.to_string(),
    })
}

fn code_file_kind_for_language(language: &str) -> Option<FileKind> {
    if matches!(language, "Plain Text" | "Markdown") {
        return None;
    }
    Some(FileKind::Code {
        language: helpers::find_syntax_for_language(language)
            .map(|syntax| syntax.name.to_string())
            .unwrap_or_else(|| language.to_string()),
    })
}

fn first_non_empty_line(text: &str) -> Option<&str> {
    text.lines().find(|line| !line.trim().is_empty())
}

fn pasted_text_has_markdown_features(text: &str) -> bool {
    let mut table_separator_lines = 0usize;
    let mut list_lines = 0usize;
    let mut link_like = false;

    for line in text.lines().take(120) {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```")
            || trimmed.starts_with("~~~")
            || trimmed.starts_with("# ")
            || trimmed.starts_with("## ")
            || trimmed.starts_with("### ")
            || trimmed.starts_with("> ")
            || trimmed.starts_with("- [ ] ")
            || trimmed.starts_with("- [x] ")
            || trimmed.starts_with("- [X] ")
            || trimmed.starts_with("![")
            || trimmed.starts_with("[[")
        {
            return true;
        }

        if trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ ") {
            list_lines += 1;
        }

        if trimmed.contains('|')
            && trimmed.contains("---")
            && trimmed
                .chars()
                .all(|ch| matches!(ch, ' ' | '\t' | '|' | '-' | ':'))
        {
            table_separator_lines += 1;
        }

        link_like |= (trimmed.contains("](") && trimmed.contains('['))
            || (trimmed.contains("]:") && trimmed.contains('['))
            || (trimmed.contains("[[") && trimmed.contains("]]"));
    }

    if text.trim_start().starts_with("---") {
        let mut non_empty_lines = text.lines().filter(|line| !line.trim().is_empty());
        let _ = non_empty_lines.next();
        if non_empty_lines.any(|line| line.trim() == "---") && link_like {
            return true;
        }
    }

    table_separator_lines > 0 || list_lines >= 2 || link_like
}

fn pasted_text_shebang_language(text: &str) -> Option<FileKind> {
    let first_line = text.lines().next()?.trim();
    let shebang = first_line.strip_prefix("#!")?.to_ascii_lowercase();
    for (needle, language) in [
        ("python", "Python"),
        ("node", "JavaScript"),
        ("deno", "TypeScript"),
        ("bun", "JavaScript"),
        ("bash", "Shell-Unix-Generic"),
        ("zsh", "Shell-Unix-Generic"),
        ("fish", "Shell-Unix-Generic"),
        ("sh", "Shell-Unix-Generic"),
        ("ruby", "Ruby"),
        ("perl", "Perl"),
        ("php", "PHP"),
        ("lua", "Lua"),
        ("swift", "Swift"),
        ("awk", "Awk"),
        ("rscript", "R"),
    ] {
        if shebang.contains(needle) {
            return code_file_kind_for_language(language);
        }
    }
    None
}

fn pasted_text_first_line_language(text: &str) -> Option<FileKind> {
    let first_line = first_non_empty_line(text)?;
    let syntax = helpers::syntax_set().find_syntax_by_first_line(first_line)?;
    code_file_kind_for_syntax_name(&syntax.name)
}

fn pasted_text_structured_language(text: &str) -> Option<FileKind> {
    let trimmed = text.trim();
    let lower = trimmed.to_ascii_lowercase();

    if matches!(trimmed.as_bytes().first(), Some(b'{') | Some(b'['))
        && serde_json::from_str::<serde_json::Value>(trimmed).is_ok()
    {
        return code_file_kind_for_language("JSON");
    }

    if lower.starts_with("<!doctype html")
        || lower.starts_with("<html")
        || (lower.contains("<head") && lower.contains("<body"))
    {
        return code_file_kind_for_language("HTML");
    }

    if lower.starts_with("<?xml") {
        return code_file_kind_for_language("XML");
    }

    if lower.starts_with("<?php") {
        return code_file_kind_for_language("PHP");
    }

    let yaml_like_lines = text
        .lines()
        .take(80)
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with('#')
                && trimmed
                    .split_once(':')
                    .is_some_and(|(key, value)| !key.trim().is_empty() && !value.contains("//"))
        })
        .count();
    if yaml_like_lines >= 3 && !trimmed.contains('{') && !trimmed.contains(';') {
        return code_file_kind_for_language("YAML");
    }

    let toml_like_lines = text
        .lines()
        .take(80)
        .filter(|line| {
            let trimmed = line.trim();
            (trimmed.starts_with('[') && trimmed.ends_with(']'))
                || trimmed
                    .split_once('=')
                    .is_some_and(|(key, value)| !key.trim().is_empty() && !value.trim().is_empty())
        })
        .count();
    if toml_like_lines >= 3 && !trimmed.contains('{') {
        return code_file_kind_for_language("TOML");
    }

    None
}

fn pasted_text_common_code_language(text: &str) -> Option<FileKind> {
    let lower = text.to_ascii_lowercase();
    let trimmed = text.trim_start();
    let first_original = first_non_empty_line(text).unwrap_or_default().trim_start();
    let first = first_non_empty_line(text)
        .unwrap_or_default()
        .trim_start()
        .to_ascii_lowercase();
    let has = |needle: &str| lower.contains(needle);
    let starts = |needle: &str| first.starts_with(needle);
    let docker_directive_count = text
        .lines()
        .take(20)
        .filter(|line| {
            let upper = line.trim_start().to_ascii_uppercase();
            ["FROM ", "RUN ", "COPY ", "ADD ", "CMD ", "ENTRYPOINT "]
                .iter()
                .any(|prefix| upper.starts_with(prefix))
        })
        .count();

    let language = if first_original.starts_with("FROM ") || docker_directive_count >= 2 {
        Some("Dockerfile")
    } else if has("#include <") || has("#include \"") || has("std::") {
        Some("C++")
    } else if has("using system;") || has("namespace ") && has("public class ") {
        Some("C#")
    } else if has("public class ") || has("public static void main") {
        Some("Java")
    } else if starts("package ") && has("func ") {
        Some("Go")
    } else if has("fn main") || has("use std::") || has("println!(") || has("impl ") {
        Some("Rust")
    } else if has("interface ")
        || has("type ") && has(" = ") && (has(": string") || has(": number") || has(": boolean"))
    {
        Some("TypeScript")
    } else if has("import ") && has(" from ")
        || has("export default")
        || has("console.log")
        || has("function ") && has("{")
        || has("const ") && (has("=>") || has(";"))
        || has("let ") && (has("=>") || has(";"))
    {
        Some("JavaScript")
    } else if starts("def ")
        || starts("class ") && has(":")
        || has("if __name__ ==")
        || has("print(") && !has(";")
    {
        Some("Python")
    } else if has("select ") && has(" from ") || has("create table") || has("insert into") {
        Some("SQL")
    } else if has("function ") && has("end") && has("local ") {
        Some("Lua")
    } else if starts("def ") && has("\nend") || has("puts ") && has("\nend") {
        Some("Ruby")
    } else if has("import foundation") || has("func ") && has("let ") && has("var ") {
        Some("Swift")
    } else if has("fun main") || has("val ") && has("println(") {
        Some("Kotlin")
    } else if has("void main()") || has("import 'package:") {
        Some("Dart")
    } else if has("defmodule ") || has("iex>") {
        Some("Elixir")
    } else if has("-module(") || has("-export(") {
        Some("Erlang")
    } else if starts("module ") && has(" where") {
        Some("Haskell")
    } else if has("library(") || has(" <- ") {
        Some("R")
    } else if has("<?php") {
        Some("PHP")
    } else if has("{") && has("}") && has(":") && has(";") && !has("function ") {
        Some("CSS")
    } else if text
        .lines()
        .collect::<Vec<_>>()
        .windows(2)
        .any(|pair| pair[0].contains(':') && pair[1].starts_with('\t'))
    {
        Some("Makefile")
    } else if trimmed.starts_with('(') && (has("defn ") || has("lambda ")) {
        Some("Clojure")
    } else {
        None
    }?;

    code_file_kind_for_language(language)
}

pub(crate) fn classify_pasted_text_kind(text: &str) -> Option<FileKind> {
    if text.trim().is_empty() {
        return None;
    }

    if pasted_text_has_markdown_features(text) {
        return Some(FileKind::Markdown);
    }

    pasted_text_shebang_language(text)
        .or_else(|| pasted_text_first_line_language(text))
        .or_else(|| pasted_text_structured_language(text))
        .or_else(|| pasted_text_common_code_language(text))
        .or(Some(FileKind::Markdown))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pasted_text_classifier_detects_markdown() {
        assert_eq!(
            classify_pasted_text_kind("# Title\n\n- [ ] task\n\n[link](https://example.com)"),
            Some(FileKind::Markdown)
        );
        assert_eq!(
            classify_pasted_text_kind("```rust\nfn main() {}\n```"),
            Some(FileKind::Markdown)
        );
    }

    #[test]
    fn pasted_text_classifier_detects_common_code_languages() {
        assert_eq!(
            classify_pasted_text_kind("fn main() {\n    println!(\"hi\");\n}\n"),
            Some(FileKind::Code {
                language: "Rust".to_string()
            })
        );
        assert_eq!(
            classify_pasted_text_kind("{\"name\": \"moeka\", \"version\": 1}"),
            Some(FileKind::Code {
                language: "JSON".to_string()
            })
        );
        assert_eq!(
            classify_pasted_text_kind("#!/usr/bin/env python3\nprint('hi')\n"),
            Some(FileKind::Code {
                language: "Python".to_string()
            })
        );
        assert_eq!(
            classify_pasted_text_kind("<!doctype html>\n<html><body>Hello</body></html>"),
            Some(FileKind::Code {
                language: "HTML".to_string()
            })
        );
        assert_eq!(
            classify_pasted_text_kind("name: moeka\nversion: 1\nenabled: true\n"),
            Some(FileKind::Code {
                language: "YAML".to_string()
            })
        );
        assert_eq!(
            classify_pasted_text_kind("[package]\nname = \"moeka\"\nversion = \"0.1.0\"\n"),
            Some(FileKind::Code {
                language: "TOML".to_string()
            })
        );
        assert_eq!(
            classify_pasted_text_kind(
                "interface User {\n  name: string\n}\nconst user: User = { name: 'Moeka' };\n"
            ),
            Some(FileKind::Code {
                language: "TypeScript".to_string()
            })
        );
        assert_eq!(
            classify_pasted_text_kind("FROM alpine:3\nRUN echo hi\n"),
            Some(FileKind::Code {
                language: "Dockerfile".to_string()
            })
        );
    }

    #[test]
    fn pasted_text_classifier_keeps_plain_prose_as_markdown() {
        assert_eq!(
            classify_pasted_text_kind("这是一段普通文本，不应该被识别成代码。"),
            Some(FileKind::Markdown)
        );
        assert_eq!(classify_pasted_text_kind("   \n\t"), None);
    }
}
