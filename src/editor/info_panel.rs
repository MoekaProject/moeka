use std::fs;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::SystemTime;

use gpui::{
    AnyElement, Context, CursorStyle, FontWeight, MouseButton, SharedString, div, prelude::*, px,
    svg,
};
#[cfg(not(target_os = "macos"))]
use printpdf::{Mm, PdfDocument};
use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd, html::push_html};
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

struct GitRepoContext {
    repo_root: PathBuf,
    file_path: PathBuf,
    relative_path: String,
}

fn git_repo_context_for_file(file_path: &Path) -> Option<GitRepoContext> {
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

fn build_html_document(title: &str, markdown: &str, source_file_path: Option<&Path>) -> String {
    let mut html = String::new();
    let parser = Parser::new_ext(markdown, Options::all());
    push_html(&mut html, parser);

    let escaped_title = html_escape(title);
    let base_href = html_base_href(source_file_path)
        .map(|href| format!("<base href=\"{}\">", html_escape(&href)))
        .unwrap_or_default();

    format!(
        concat!(
            "<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\">",
            "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">",
            "<title>{}</title>",
            "{}",
            "<style>",
            ":root{{color-scheme:light dark;}}",
            "@page{{size:A4;margin:18mm;}}",
            "body{{margin:40px auto;max-width:840px;padding:0 24px;font:16px/1.7 -apple-system,BlinkMacSystemFont,'PingFang SC','Segoe UI',sans-serif;word-break:break-word;overflow-wrap:anywhere;}}",
            "img{{max-width:100%;height:auto;}}",
            "pre{{overflow:auto;padding:12px;border-radius:8px;background:rgba(127,127,127,.08);}}",
            "code{{font-family:Menlo,Consolas,monospace;}}",
            "table{{border-collapse:collapse;width:100%;}}",
            "th,td{{border:1px solid rgba(127,127,127,.25);padding:8px 10px;text-align:left;}}",
            "blockquote{{margin:0;padding-left:16px;border-left:3px solid rgba(127,127,127,.35);color:rgba(127,127,127,.9);}}",
            "@media print{{body{{margin:0;max-width:none;padding:0;font-size:12pt;line-height:1.65;}}a{{color:inherit;text-decoration:none;}}pre{{white-space:pre-wrap;word-break:break-word;}}}}",
            "</style></head><body>{}</body></html>"
        ),
        escaped_title, base_href, html
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
fn run_current_run_loop_for(seconds: f64) {
    unsafe {
        let run_loop: *mut objc::runtime::Object = msg_send![class!(NSRunLoop), currentRunLoop];
        let until_date: *mut objc::runtime::Object =
            msg_send![class!(NSDate), dateWithTimeIntervalSinceNow: seconds];
        let () = msg_send![run_loop, runUntilDate: until_date];
    }
}

#[cfg(target_os = "macos")]
fn build_pdf_document_from_html_macos(
    html: &str,
    source_file_path: Option<&Path>,
) -> anyhow::Result<Vec<u8>> {
    unsafe {
        let autorelease_pool: *mut objc::runtime::Object =
            msg_send![class!(NSAutoreleasePool), new];

        let page_width = 595.0_f64;
        let page_height = 842.0_f64;
        let margin = 51.0_f64;
        let content_width = page_width - margin * 2.0;
        let frame = NSRect {
            origin: NSPoint { x: 0.0, y: 0.0 },
            size: NSSize {
                width: content_width,
                height: page_height,
            },
        };
        let nil = std::ptr::null_mut::<objc::runtime::Object>();
        let web_view: *mut objc::runtime::Object = msg_send![class!(WebView), alloc];
        let web_view: *mut objc::runtime::Object = msg_send![
            web_view,
            initWithFrame: frame
            frameName: nil
            groupName: nil
        ];
        let () = msg_send![web_view, setMediaStyle: nsstring("print")];

        // Host the WebView inside an offscreen, never-shown window. The legacy
        // WebHTMLView only flows and draws its content when it is attached to a
        // window; without one the print operation snapshots a blank page.
        let host_window: *mut objc::runtime::Object = msg_send![class!(NSWindow), alloc];
        let host_window: *mut objc::runtime::Object = msg_send![
            host_window,
            initWithContentRect: frame
            styleMask: 0u64
            backing: 2u64
            defer: false
        ];
        let () = msg_send![host_window, setReleasedWhenClosed: false];
        let () = msg_send![host_window, setContentView: web_view];

        let main_frame: *mut objc::runtime::Object = msg_send![web_view, mainFrame];
        let base_url = source_file_path
            .and_then(Path::parent)
            .map(nsurl_from_directory)
            .unwrap_or(nil);
        let () = msg_send![main_frame, loadHTMLString: nsstring(html) baseURL: base_url];

        let start = std::time::Instant::now();
        loop {
            let is_loading: bool = msg_send![web_view, isLoading];
            if !is_loading {
                break;
            }
            if start.elapsed() > std::time::Duration::from_secs(10) {
                let () = msg_send![autorelease_pool, drain];
                anyhow::bail!("timed out waiting for WebKit HTML rendering");
            }
            run_current_run_loop_for(0.01);
        }

        run_current_run_loop_for(0.05);

        let frame_view: *mut objc::runtime::Object = msg_send![main_frame, frameView];
        if frame_view.is_null() {
            let () = msg_send![autorelease_pool, drain];
            anyhow::bail!("WebKit frame view unavailable after HTML load");
        }

        let document_view: *mut objc::runtime::Object = msg_send![frame_view, documentView];
        if document_view.is_null() {
            let () = msg_send![autorelease_pool, drain];
            anyhow::bail!("WebKit document view unavailable after HTML load");
        }

        let document_bounds: NSRect = msg_send![document_view, bounds];
        let total_height = document_bounds.size.height.max(page_height);
        let full_rect = NSRect {
            origin: NSPoint { x: 0.0, y: 0.0 },
            size: NSSize {
                width: document_bounds.size.width.max(content_width),
                height: total_height,
            },
        };
        let () = msg_send![web_view, setFrame: full_rect];

        // Force the freshly-resized document view to lay out before we snapshot
        // it, otherwise WebKit may report stale geometry and emit a blank page.
        let () = msg_send![document_view, setNeedsDisplay: true];
        run_current_run_loop_for(0.05);

        let shared_print_info: *mut objc::runtime::Object =
            msg_send![class!(NSPrintInfo), sharedPrintInfo];
        let print_info: *mut objc::runtime::Object = msg_send![shared_print_info, copy];
        let () =
            msg_send![print_info, setPaperSize: NSSize { width: page_width, height: page_height }];
        let () = msg_send![print_info, setTopMargin: margin];
        let () = msg_send![print_info, setBottomMargin: margin];
        let () = msg_send![print_info, setLeftMargin: margin];
        let () = msg_send![print_info, setRightMargin: margin];
        let () = msg_send![print_info, setHorizontallyCentered: false];
        let () = msg_send![print_info, setVerticallyCentered: false];

        let pdf_data: *mut objc::runtime::Object = msg_send![class!(NSMutableData), data];
        // Print the frame's document view rather than the top-level WebView.
        // The legacy `WebView` host view draws nothing itself, so printing it
        // produces a blank PDF; the rendered content lives in the document view.
        let document_print_bounds: NSRect = msg_send![document_view, bounds];
        let print_rect = NSRect {
            origin: NSPoint { x: 0.0, y: 0.0 },
            size: NSSize {
                width: document_print_bounds.size.width.max(content_width),
                height: document_print_bounds.size.height.max(page_height),
            },
        };
        let print_operation: *mut objc::runtime::Object = msg_send![
            class!(NSPrintOperation),
            PDFOperationWithView: document_view
            insideRect: print_rect
            toData: pdf_data
            printInfo: print_info
        ];
        let () = msg_send![print_operation, setShowsPrintPanel: false];
        let () = msg_send![print_operation, setShowsProgressPanel: false];
        let did_run: bool = msg_send![print_operation, runOperation];
        if !did_run {
            let () = msg_send![autorelease_pool, drain];
            anyhow::bail!("failed to run AppKit PDF print operation");
        }

        let length: usize = msg_send![pdf_data, length];
        let bytes_ptr: *const u8 = msg_send![pdf_data, bytes];
        let bytes = std::slice::from_raw_parts(bytes_ptr, length).to_vec();

        let () = msg_send![host_window, close];
        let () = msg_send![autorelease_pool, drain];
        Ok(bytes)
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

fn build_pdf_document(
    title: &str,
    markdown: &str,
    source_file_path: Option<&Path>,
) -> anyhow::Result<Vec<u8>> {
    #[cfg(target_os = "macos")]
    {
        let html = build_html_document(title, markdown, source_file_path);
        return build_pdf_document_from_html_macos(&html, source_file_path);
    }

    #[cfg(not(target_os = "macos"))]
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

fn git_show_file_at_revision(repo_root: &Path, revision: &str, relative_path: &str) -> String {
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

fn build_inline_change_text_and_highlights(
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

fn merge_adjacent_inline_ranges(mut ranges: Vec<Range<usize>>) -> Vec<Range<usize>> {
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

impl EditorView {
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

    fn cached_info_panel_data(&mut self) -> Option<InfoPanelData> {
        let current_tab_id = self.active_tab_id.clone();
        if self.info_panel_cache_tab_id == current_tab_id {
            return self.info_panel_cache_result.clone().flatten();
        }

        let mut profile_scope = profile::ProfileScope::new("utility_panel.info_panel_refresh");
        if let Some(tab_id) = current_tab_id.as_deref() {
            profile_scope.set_note(format!("tab_id={tab_id}"));
        }

        self.info_panel_cache_tab_id = current_tab_id;
        self.info_panel_cache_result = Some(self.collect_info_panel_data());
        self.info_panel_cache_result.clone().flatten()
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
            if let Some(path) = current_path.as_ref() {
                let mut profile_scope =
                    profile::ProfileScope::new("utility_panel.git_history_refresh");
                profile_scope.set_note(format!("path={}", path.display()));
            }
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
            git_diff: Some(diff),
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
        let diff_title = Self::title_for_path(&context.file_path);

        let source = if item.is_working_tree {
            GitDiffSource::WorkingTree
        } else {
            GitDiffSource::Commit {
                commit_hash: item.commit_hash.clone().unwrap_or_default(),
                short_hash: item.short_hash.clone(),
            }
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
            let export_result = build_pdf_document(&title, &markdown, source_file_path.as_deref());

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
            .child(
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
            )
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

    pub(crate) fn render_utility_panel(
        &mut self,
        is_chat_focused: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match self.displayed_utility_panel {
            Some(RightUtilityPanel::Outline) => self.render_outline(cx),
            Some(RightUtilityPanel::Chat) => self.render_chat_panel(is_chat_focused, cx),
            Some(RightUtilityPanel::Info) => self.render_info_panel(cx),
            Some(RightUtilityPanel::GitHistory) => self.render_git_history_panel(cx),
            None => div().into_any_element(),
        }
    }

    pub(crate) fn render_info_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
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
            children.push(self.render_info_section_title(
                self.t(crate::i18n::TextKey::Statistics),
                FILE_INFO_SECTION_INFO_SVG_PATH,
            ));
            for row in &data.primary_rows {
                children.push(self.render_info_row(row));
            }

            children.push(div().h(px(section_gap)).flex_shrink_0().into_any_element());
            children.push(self.render_info_section_title(
                self.t(crate::i18n::TextKey::Export),
                FILE_INFO_SECTION_EXPORT_SVG_PATH,
            ));
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

            if data.supports_text_export {
                children.push(div().h(px(section_gap)).flex_shrink_0().into_any_element());
                children.push(self.render_info_section_title(
                    self.t(crate::i18n::TextKey::Attachments),
                    FILE_INFO_SECTION_ATTACHMENT_SVG_PATH,
                ));
                if data.attachment_items.is_empty() {
                    children.push(
                        self.render_info_empty_line(
                            self.t(crate::i18n::TextKey::NoLocalAttachments),
                        ),
                    );
                } else {
                    for item in &data.attachment_items {
                        children.push(self.render_reference_item(item, cx));
                    }
                }

                children.push(div().h(px(section_gap)).flex_shrink_0().into_any_element());
                children.push(self.render_info_section_title(
                    self.t(crate::i18n::TextKey::Images),
                    FMT_IMAGE_SVG,
                ));
                if data.image_items.is_empty() {
                    children.push(
                        self.render_info_empty_line(self.t(crate::i18n::TextKey::NoImageLinks)),
                    );
                } else {
                    for item in &data.image_items {
                        children.push(self.render_reference_item(item, cx));
                    }
                }

                children.push(div().h(px(section_gap)).flex_shrink_0().into_any_element());
                children.push(
                    self.render_info_section_title(
                        self.t(crate::i18n::TextKey::Links),
                        FMT_LINK_SVG,
                    ),
                );
                if data.external_link_items.is_empty() {
                    children.push(
                        self.render_info_empty_line(self.t(crate::i18n::TextKey::NoRemoteLinks)),
                    );
                } else {
                    for item in &data.external_link_items {
                        children.push(self.render_reference_item(item, cx));
                    }
                }

                children.push(div().h(px(section_gap)).flex_shrink_0().into_any_element());
                children.push(self.render_info_section_title(
                    self.t(crate::i18n::TextKey::Backlinks),
                    FMT_WIKI_LINK_SVG,
                ));
                if data.wiki_link_items.is_empty() {
                    children.push(
                        self.render_info_empty_line(self.t(crate::i18n::TextKey::NoBacklinks)),
                    );
                } else {
                    for item in &data.wiki_link_items {
                        children.push(self.render_reference_item(item, cx));
                    }
                }
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

        div()
            .id("info-panel")
            .h_full()
            .w_full()
            .bg(ups.utility_panel_bg)
            .overflow_y_scroll()
            .border_l(px(ups.utility_panel_border_width_px))
            .border_color(ups.utility_panel_border_color)
            .flex()
            .flex_col()
            .children(children)
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

        div()
            .id("git-history-panel")
            .h_full()
            .w_full()
            .bg(ups.utility_panel_bg)
            .overflow_y_scroll()
            .track_scroll(&self.git_history_scroll_handle)
            .border_l(px(ups.utility_panel_border_width_px))
            .border_color(ups.utility_panel_border_color)
            .flex()
            .flex_col()
            .children(children)
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
        let rows = build_git_diff_rows(crate::i18n::UiLanguage::English, "", "**new**\n", true, None);

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
