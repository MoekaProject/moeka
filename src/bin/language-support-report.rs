use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

const LINGUIST_COMMIT: &str = "e0c78d62c42abae6122235d8e68a7aa43eef89da";

#[derive(Debug, Deserialize)]
struct LinguistLanguage {
    #[serde(rename = "type")]
    kind: String,
    #[serde(default)]
    aliases: Vec<String>,
    #[serde(default)]
    extensions: Vec<String>,
}

fn normalized(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn normalized_extension(value: &str) -> String {
    value
        .trim_start_matches("*.")
        .trim_start_matches('.')
        .to_ascii_lowercase()
}

fn linguist_yaml(source: Option<&Path>) -> Result<String> {
    if let Some(path) = source {
        return fs::read_to_string(path)
            .with_context(|| format!("failed to read {}", path.display()));
    }

    let url = format!(
        "https://raw.githubusercontent.com/github-linguist/linguist/{LINGUIST_COMMIT}/lib/linguist/languages.yml"
    );
    ureq::get(&url)
        .call()
        .with_context(|| format!("failed to download {url}"))?
        .into_string()
        .context("failed to read Linguist response")
}

fn main() -> Result<()> {
    let mut arguments = std::env::args_os().skip(1);
    let output = arguments
        .next()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/language-support-audit.md"));
    let source = arguments.next().map(PathBuf::from);
    anyhow::ensure!(
        arguments.next().is_none(),
        "usage: language-support-report [OUTPUT] [LANGUAGES_YML]"
    );

    let languages: BTreeMap<String, LinguistLanguage> =
        serde_yaml::from_str(&linguist_yaml(source.as_deref())?)
            .context("failed to parse Linguist languages.yml")?;
    let syntax_set = two_face::syntax::extra_newlines();
    let mut syntax_names = BTreeSet::new();
    let mut syntax_extensions = BTreeSet::new();
    for syntax in syntax_set.syntaxes() {
        syntax_names.insert(normalized(&syntax.name));
        for extension in &syntax.file_extensions {
            syntax_extensions.insert(normalized_extension(extension));
        }
    }

    let included_types = ["programming", "markup", "data", "prose"];
    let mut totals = BTreeMap::<String, usize>::new();
    let mut supported = BTreeMap::<String, usize>::new();
    let mut unsupported = BTreeMap::<String, Vec<String>>::new();
    for (name, language) in languages {
        if !included_types.contains(&language.kind.as_str()) {
            continue;
        }
        *totals.entry(language.kind.clone()).or_default() += 1;
        let name_match = std::iter::once(name.as_str())
            .chain(language.aliases.iter().map(String::as_str))
            .map(normalized)
            .any(|candidate| syntax_names.contains(&candidate));
        let extension_match = language
            .extensions
            .iter()
            .map(|extension| normalized_extension(extension))
            .filter(|extension| !extension.is_empty())
            .any(|extension| syntax_extensions.contains(&extension));
        if name_match || extension_match {
            *supported.entry(language.kind).or_default() += 1;
        } else {
            unsupported.entry(language.kind).or_default().push(name);
        }
    }

    let mut markdown = String::new();
    markdown.push_str("# GitHub Linguist syntax coverage audit\n\n");
    markdown.push_str(&format!(
        "Baseline: `github-linguist/linguist@{LINGUIST_COMMIT}`. Registry: `two-face::syntax::extra_newlines()` with the pure-Rust fancy-regex backend.\n\n"
    ));
    markdown.push_str("A language is counted as supported when its normalized Linguist name/alias or one of its extensions matches the syntax registry. This is a conservative static audit: shared extensions can produce false positives, while fence aliases maintained by Moeka can produce false negatives. Runtime lookup tests remain authoritative.\n\n");
    markdown.push_str("| Type | Supported | Total | Coverage |\n| --- | ---: | ---: | ---: |\n");
    for kind in included_types {
        let total = totals.get(kind).copied().unwrap_or(0);
        let covered = supported.get(kind).copied().unwrap_or(0);
        let percent = if total == 0 {
            0.0
        } else {
            covered as f64 * 100.0 / total as f64
        };
        markdown.push_str(&format!(
            "| {kind} | {covered} | {total} | {percent:.1}% |\n"
        ));
    }
    markdown.push_str("\n## Not matched\n\n");
    for kind in included_types {
        let entries = unsupported.get(kind).cloned().unwrap_or_default();
        markdown.push_str(&format!("### {kind} ({})\n\n", entries.len()));
        if entries.is_empty() {
            markdown.push_str("None.\n\n");
        } else {
            for name in entries {
                markdown.push_str(&format!("- {name}\n"));
            }
            markdown.push('\n');
        }
    }

    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    fs::write(&output, markdown)
        .with_context(|| format!("failed to write {}", output.display()))?;
    println!("wrote {}", output.display());
    Ok(())
}
