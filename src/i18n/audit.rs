//! A guard against reintroducing literal prose at direct UI/error boundaries.
//! Indirect formatted messages still require review; this is not a Rust parser.
use std::path::{Path, PathBuf};

fn rust_sources(directory: &Path, files: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        if entry.file_type().unwrap().is_dir() {
            rust_sources(&entry.path(), files);
        } else if entry.path().extension().is_some_and(|ext| ext == "rs") {
            files.push(entry.path());
        }
    }
}

fn reviewed_fixture(file: &str, text: &str) -> bool {
    match file {
        "editor/config_store.rs" => text == "write failed",
        "editor/file_watch.rs" => text == "filesystem unavailable",
        "editor/notifications.rs" => text == "OS error 13: Permission denied",
        // Assertions in the command-line native export smoke test, not UI copy.
        "editor/export.rs" => matches!(
            text,
            "Clipboard must contain one file reference"
                | "Wrong clipboard file name"
                | "Clipboard file contents changed"
        ),
        // Injected filesystem/native errors must remain verbatim in regressions.
        "editor/sidebar_files.rs" => matches!(
            text,
            "source removal"
                | "late cross-volume rename failure"
                | "rename failed after deadline"
                | "rename unsupported"
                | "cross-volume rename unsupported"
                | "rename failed"
                | "Permission denied while moving to Trash"
                | "trash unavailable: permission denied"
                | "unknown Trash failure"
                | "locked child"
        ),
        "platform/trash.rs" => matches!(
            text,
            "no trash"
                | "trashItemAtURL: volume doesn't have one"
                | "废纸篓不可用 / recycle bin unavailable / .Trash: No such file"
                | "probe failed"
                | "COM failure"
                | "unknown local Trash failure"
        ),
        _ => false,
    }
}

#[test]
fn direct_ui_and_runtime_errors_do_not_embed_untranslated_prose() {
    let direct = regex::Regex::new(
        r#"(?s)(?:\.(?:context|tooltip|placeholder)|\b(?:notify_error|notify_info|notify_diagnostic|anyhow!|bail!))\(\s*(?:format!\(\s*)?"((?:\\.|[^"\\])*)""#,
    ).unwrap();
    let errors = regex::Regex::new(
        r#"(?s)(?:\bensure!|\b(?:io|std::io)::Error::(?:new|other))\([^;]*?"((?:\\.|[^"\\])*)""#,
    )
    .unwrap();
    let prose = regex::Regex::new(r"[A-Za-z]{2,}[ \n]+[A-Za-z]{2,}").unwrap();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_sources(&root, &mut files);
    files.sort();
    let mut violations = Vec::new();
    for path in files {
        let relative = path
            .strip_prefix(&root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        // Catalogs and standalone developer reports are not application UI.
        if relative == "i18n.rs" || relative.starts_with("i18n/") || relative.starts_with("bin/") {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        for pattern in [&direct, &errors] {
            for captures in pattern.captures_iter(&source) {
                let literal = &captures[1];
                if prose.is_match(literal) && !reviewed_fixture(&relative, literal) {
                    let line = source[..captures.get(0).unwrap().start()]
                        .bytes()
                        .filter(|byte| *byte == b'\n')
                        .count()
                        + 1;
                    violations.push(format!("{relative}:{line}: {literal}"));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "Use a catalog key for first-party UI/runtime prose:\n{}",
        violations.join("\n")
    );
}
