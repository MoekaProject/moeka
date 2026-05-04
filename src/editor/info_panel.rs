use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use gpui::{AnyElement, Context, CursorStyle, MouseButton, SharedString, div, img, prelude::*, px};
use printpdf::{Mm, PdfDocument};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd, html::push_html};
use similar::{ChangeTag, TextDiff};
use unicode_segmentation::UnicodeSegmentation;

use super::*;

struct InfoPanelRow {
    label: String,
    value: String,
}

struct InfoPanelData {
    primary_rows: Vec<InfoPanelRow>,
    front_matter_rows: Vec<InfoPanelRow>,
    attachment_items: Vec<InfoPanelResourceItem>,
    image_items: Vec<InfoPanelResourceItem>,
    external_link_items: Vec<InfoPanelExternalLinkItem>,
    supports_text_export: bool,
}

#[derive(Clone)]
struct InfoPanelResourceItem {
    label: String,
    path: Option<PathBuf>,
    action: Option<InfoPanelResourceAction>,
}

#[derive(Clone)]
struct InfoPanelExternalLinkItem {
    label: String,
    url: String,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum InfoPanelResourceAction {
    RevealInFinder,
    OpenPreviewTab,
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
}

#[derive(Clone)]
struct GitHistoryItem {
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

fn format_relative_time(time: SystemTime) -> String {
    let Ok(delta) = SystemTime::now().duration_since(time) else {
        return "刚刚".to_string();
    };

    let seconds = delta.as_secs();
    if seconds < 60 {
        "刚刚".to_string()
    } else if seconds < 3600 {
        format!("{} 分钟前", seconds / 60)
    } else if seconds < 86_400 {
        format!("{} 小时前", seconds / 3600)
    } else if seconds < 86_400 * 365 {
        format!("{} 天前", seconds / 86_400)
    } else {
        format!("{} 年前", seconds / (86_400 * 365))
    }
}

fn format_unix_timestamp_relative(timestamp: i64) -> String {
    if timestamp <= 0 {
        return "刚刚".to_string();
    }

    let Some(time) = UNIX_EPOCH.checked_add(std::time::Duration::from_secs(timestamp as u64))
    else {
        return "刚刚".to_string();
    };
    format_relative_time(time)
}

fn format_file_kind(file_kind: Option<&FileKind>) -> &'static str {
    match file_kind {
        Some(FileKind::Markdown) => "Markdown",
        Some(FileKind::Code { .. }) => "Code",
        Some(FileKind::Text) => "Text",
        Some(FileKind::Image) => "Image",
        Some(FileKind::Binary) => "Binary",
        None => "Settings",
    }
}

fn export_dialog_path(default_name: &str, title: &str) -> Option<std::path::PathBuf> {
    rfd::FileDialog::new()
        .set_title(title)
        .set_file_name(default_name)
        .save_file()
}

fn build_html_document(title: &str, markdown: &str) -> String {
    let mut html = String::new();
    let parser = Parser::new_ext(markdown, Options::all());
    push_html(&mut html, parser);

    format!(
        concat!(
            "<!doctype html><html lang=\"zh-CN\"><head><meta charset=\"utf-8\">",
            "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">",
            "<title>{}</title>",
            "<style>",
            ":root{{color-scheme:light dark;}}",
            "body{{margin:40px auto;max-width:840px;padding:0 24px;font:16px/1.7 -apple-system,BlinkMacSystemFont,'PingFang SC','Segoe UI',sans-serif;}}",
            "img{{max-width:100%;height:auto;}}",
            "pre{{overflow:auto;padding:12px;border-radius:8px;background:rgba(127,127,127,.08);}}",
            "code{{font-family:Menlo,Consolas,monospace;}}",
            "table{{border-collapse:collapse;width:100%;}}",
            "th,td{{border:1px solid rgba(127,127,127,.25);padding:8px 10px;text-align:left;}}",
            "blockquote{{margin:0;padding-left:16px;border-left:3px solid rgba(127,127,127,.35);color:rgba(127,127,127,.9);}}",
            "</style></head><body>{}</body></html>"
        ),
        title, html
    )
}

fn is_external_reference(target: &str) -> bool {
    let lower = target.trim().to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
        || lower.starts_with("tel:")
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

fn normalize_panel_label(label: &str, target: &str) -> String {
    let compact = label.split_whitespace().collect::<Vec<_>>().join(" ");
    if !compact.is_empty() {
        return compact;
    }

    Path::new(target)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
        .unwrap_or_else(|| target.to_string())
}

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

fn find_pdf_font_path() -> Option<PathBuf> {
    pdf_font_candidates().into_iter().find(|path| path.exists())
}

fn build_pdf_document(title: &str, markdown: &str) -> anyhow::Result<Vec<u8>> {
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

fn build_git_diff_rows(base_text: &str, target_text: &str) -> Vec<GitDiffRow> {
    let diff = TextDiff::from_lines(base_text, target_text);
    let mut old_line_number = 1usize;
    let mut new_line_number = 1usize;
    let mut rows = Vec::new();

    for change in diff.iter_all_changes() {
        let text = change.to_string().trim_end_matches('\n').to_string();
        match change.tag() {
            ChangeTag::Equal => {
                rows.push(GitDiffRow {
                    kind: GitDiffRowKind::Context,
                    old_line_number: Some(old_line_number),
                    new_line_number: Some(new_line_number),
                    text,
                });
                old_line_number += 1;
                new_line_number += 1;
            }
            ChangeTag::Delete => {
                rows.push(GitDiffRow {
                    kind: GitDiffRowKind::Removed,
                    old_line_number: Some(old_line_number),
                    new_line_number: None,
                    text,
                });
                old_line_number += 1;
            }
            ChangeTag::Insert => {
                rows.push(GitDiffRow {
                    kind: GitDiffRowKind::Added,
                    old_line_number: None,
                    new_line_number: Some(new_line_number),
                    text,
                });
                new_line_number += 1;
            }
        }
    }

    if rows.is_empty() {
        rows.push(GitDiffRow {
            kind: GitDiffRowKind::Context,
            old_line_number: None,
            new_line_number: None,
            text: "没有文本差异。".to_string(),
        });
    }

    rows
}

impl EditorView {
    fn git_repo_context(&self) -> Option<GitRepoContext> {
        let file_path = self.current_file_path.clone()?;
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
            file_path,
            relative_path,
        })
    }

    fn collect_git_history_items(&self) -> Option<Vec<GitHistoryItem>> {
        let context = self.git_repo_context()?;
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
                short_hash: "WORKING".to_string(),
                message: "未提交更改".to_string(),
                time_label: "当前".to_string(),
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
                .map(format_unix_timestamp_relative)
                .unwrap_or_else(|_| "刚刚".to_string());
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
        if let Some(index) = self.tabs.iter().position(|tab| tab.kind == TabKind::Diff) {
            self.tabs[index].title = diff_title;
            self.tabs[index].path = Some(path);
            self.tabs[index].git_diff = Some(diff);
            self.activate_tab_by_index(index, cx);
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
            file_kind: None,
            snapshot: EditorSnapshot {
                text: String::new(),
                cursor_offset: 0,
                cursor_anchor: 0,
                scroll_offset: 0.0,
                extra_cursors: Vec::new(),
            },
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
        let diff_title = format!("Diff {}", Self::title_for_path(&context.file_path));

        let (source_label, target_label, base_text, target_text) = if item.is_working_tree {
            let base_text =
                git_show_file_at_revision(&context.repo_root, "HEAD", &context.relative_path);
            let target_text = self.current_file_text_for_git_diff(&context.file_path);
            (
                if base_text.is_empty() {
                    "空文件".to_string()
                } else {
                    "HEAD".to_string()
                },
                "工作区".to_string(),
                base_text,
                target_text,
            )
        } else {
            let commit_hash = item.commit_hash.clone().unwrap_or_default();
            let parent_revision = format!("{commit_hash}^");
            let base_text = git_show_file_at_revision(
                &context.repo_root,
                &parent_revision,
                &context.relative_path,
            );
            let target_text =
                git_show_file_at_revision(&context.repo_root, &commit_hash, &context.relative_path);
            (
                format!("{}^", item.short_hash),
                item.short_hash.clone(),
                base_text,
                target_text,
            )
        };

        let diff = GitDiffState {
            source_label,
            target_label,
            rows: build_git_diff_rows(&base_text, &target_text),
        };

        self.open_git_diff_tab(diff_title, context.file_path, diff, cx);
    }

    fn resolve_info_panel_reference_path(&self, raw: &str) -> Option<PathBuf> {
        let decoded = decode_file_url_path(raw);
        let raw = decoded.trim();
        if raw.is_empty() || raw.starts_with('#') || is_external_reference(raw) {
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

    fn collect_info_panel_resources(
        &self,
        text: &str,
    ) -> (
        Vec<InfoPanelResourceItem>,
        Vec<InfoPanelResourceItem>,
        Vec<InfoPanelExternalLinkItem>,
    ) {
        let mut attachment_items = Vec::new();
        let mut image_items = Vec::new();
        let mut external_link_items = Vec::new();
        let mut attachment_seen = HashSet::new();
        let mut image_seen = HashSet::new();
        let mut external_seen = HashSet::new();
        let mut pending_references = Vec::new();

        let push_resource_item = |section: &mut Vec<InfoPanelResourceItem>,
                                  seen: &mut HashSet<String>,
                                  item: InfoPanelResourceItem,
                                  key: String| {
            if seen.insert(key) {
                section.push(item);
            }
        };

        let mut commit_reference = |pending: PendingPanelReference| {
            if is_external_reference(&pending.target) {
                if external_seen.insert(pending.target.clone()) {
                    external_link_items.push(InfoPanelExternalLinkItem {
                        label: normalize_panel_label(&pending.label, &pending.target),
                        url: pending.target,
                    });
                }
                return;
            }

            let Some(resolved_path) = self.resolve_info_panel_reference_path(&pending.target)
            else {
                return;
            };
            let key = resolved_path.to_string_lossy().into_owned();
            let path = resolved_path.exists().then_some(resolved_path.clone());
            let resolved_kind = resolved_path
                .exists()
                .then(|| classify_file_kind(&resolved_path));
            let is_image = pending.kind == PendingPanelReferenceKind::Image
                || resolved_kind.as_ref().is_some_and(FileKind::is_image);
            let item = InfoPanelResourceItem {
                label: if is_image {
                    resolved_path
                        .file_name()
                        .and_then(|name| name.to_str())
                        .filter(|name| !name.is_empty())
                        .map(ToString::to_string)
                        .unwrap_or_else(|| normalize_panel_label(&pending.label, &pending.target))
                } else {
                    normalize_panel_label(&pending.label, &pending.target)
                },
                path,
                action: Some(if is_image {
                    InfoPanelResourceAction::OpenPreviewTab
                } else {
                    InfoPanelResourceAction::RevealInFinder
                }),
            };

            if is_image {
                push_resource_item(&mut image_items, &mut image_seen, item, key);
            } else {
                push_resource_item(&mut attachment_items, &mut attachment_seen, item, key);
            }
        };

        for event in Parser::new_ext(text, Options::all()) {
            match event {
                Event::Start(Tag::Link { dest_url, .. }) => {
                    pending_references.push(PendingPanelReference {
                        kind: PendingPanelReferenceKind::Link,
                        target: dest_url.to_string(),
                        label: String::new(),
                    });
                }
                Event::Start(Tag::Image { dest_url, .. }) => {
                    pending_references.push(PendingPanelReference {
                        kind: PendingPanelReferenceKind::Image,
                        target: dest_url.to_string(),
                        label: String::new(),
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

        for decoration in &self.decorations {
            for wiki_link in &decoration.wiki_links {
                let resolved_path =
                    resolve_wiki_link_target_path(&self.sidebar_root, &wiki_link.target_path)
                        .or_else(|| Some(self.sidebar_root.join(&wiki_link.target_path)));
                let Some(resolved_path) = resolved_path else {
                    continue;
                };

                let key = format!("wiki:{}", resolved_path.to_string_lossy());
                let path = resolved_path.exists().then_some(resolved_path.clone());
                let is_image =
                    resolved_path.exists() && classify_file_kind(&resolved_path).is_image();
                let item = InfoPanelResourceItem {
                    label: if is_image {
                        resolved_path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .filter(|name| !name.is_empty())
                            .map(ToString::to_string)
                            .unwrap_or_else(|| normalize_panel_label("", &wiki_link.target_path))
                    } else {
                        normalize_panel_label("", &wiki_link.target_path)
                    },
                    path,
                    action: Some(if is_image {
                        InfoPanelResourceAction::OpenPreviewTab
                    } else {
                        InfoPanelResourceAction::RevealInFinder
                    }),
                };

                if is_image {
                    push_resource_item(&mut image_items, &mut image_seen, item, key);
                } else {
                    push_resource_item(&mut attachment_items, &mut attachment_seen, item, key);
                }
            }
        }

        (attachment_items, image_items, external_link_items)
    }

    fn collect_info_panel_data(&self) -> Option<InfoPanelData> {
        let active_index = self.active_tab_index()?;
        let tab = self.tabs.get(active_index)?;
        let path = tab.path.as_deref();
        let relative_path = path
            .and_then(|path| self.relative_path_for_path(path))
            .or_else(|| path.map(|path| path.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "未保存".to_string());
        let metadata = path.and_then(|path| fs::metadata(path).ok());
        let created_at = metadata
            .as_ref()
            .and_then(|meta| meta.created().or_else(|_| meta.modified()).ok())
            .map(format_relative_time)
            .unwrap_or_else(|| "不可用".to_string());
        let modified_at = metadata
            .as_ref()
            .and_then(|meta| meta.modified().ok())
            .map(format_relative_time)
            .unwrap_or_else(|| "不可用".to_string());

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

        let front_matter_rows = self
            .decorations
            .iter()
            .filter_map(|decoration| {
                let info = decoration.table_info.as_ref()?;
                if !info.is_front_matter || info.is_separator {
                    return None;
                }
                let key = info.cells.first()?.trim().to_string();
                if key.is_empty() {
                    return None;
                }
                Some(InfoPanelRow {
                    label: key,
                    value: info
                        .cells
                        .get(1)
                        .map(|cell| cell.trim().to_string())
                        .unwrap_or_default(),
                })
            })
            .collect();

        let (attachment_items, image_items, external_link_items) = if supports_text_export {
            self.collect_info_panel_resources(text)
        } else {
            (Vec::new(), Vec::new(), Vec::new())
        };

        Some(InfoPanelData {
            primary_rows: vec![
                InfoPanelRow {
                    label: "文件".to_string(),
                    value: relative_path,
                },
                InfoPanelRow {
                    label: "类型".to_string(),
                    value: format_file_kind(tab.file_kind.as_ref()).to_string(),
                },
                InfoPanelRow {
                    label: "创建".to_string(),
                    value: created_at,
                },
                InfoPanelRow {
                    label: "修改".to_string(),
                    value: modified_at,
                },
                InfoPanelRow {
                    label: "大小".to_string(),
                    value: format_file_size(file_size),
                },
            ],
            front_matter_rows,
            attachment_items,
            image_items,
            external_link_items,
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
            .unwrap_or("untitled");
        format!("{}.{}", title, extension)
    }

    fn export_current_markdown(&mut self, cx: &mut Context<Self>) {
        let default_name = self.export_default_name("md");
        let text = self.buffer.text().to_string();

        cx.spawn(async move |_this, cx| {
            let maybe_path = cx
                .background_executor()
                .spawn(async move { export_dialog_path(&default_name, "导出 Markdown") })
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
            .unwrap_or("Untitled")
            .to_string();
        let default_name = self.export_default_name("html");
        let document = build_html_document(&title, self.buffer.text());

        cx.spawn(async move |_this, cx| {
            let maybe_path = cx
                .background_executor()
                .spawn(async move { export_dialog_path(&default_name, "导出 HTML") })
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
            .unwrap_or("Untitled")
            .to_string();
        let default_name = self.export_default_name("pdf");
        let markdown = self.buffer.text().to_string();

        cx.spawn(async move |_this, cx| {
            let maybe_path = cx
                .background_executor()
                .spawn(async move { export_dialog_path(&default_name, "导出 PDF") })
                .await;
            let Some(path) = maybe_path else {
                return;
            };

            let export_result = cx
                .background_executor()
                .spawn(async move { build_pdf_document(&title, &markdown) })
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

    fn render_info_section_title(&self, title: &'static str) -> AnyElement {
        let ups = &self.styles.utility_panel;
        div()
            .px(px(self.scaled(ups.utility_panel_section_padding_x_px)))
            .text_size(px(self.scaled(ups.utility_panel_section_title_font_size_px)))
            .text_color(ups.utility_panel_section_title_text_color)
            .child(title)
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
                    .max_w(px(self.utility_panel_width() * 0.6))
                    .child(SharedString::from(row.value.clone())),
            )
            .into_any_element()
    }

    fn render_export_button(
        &self,
        label: &'static str,
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
            .child(label);

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

    fn render_resource_item(
        &self,
        item: &InfoPanelResourceItem,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ups = &self.styles.utility_panel;
        let base = div()
            .px(px(self.scaled(ups.utility_panel_section_padding_x_px)))
            .min_h(px(self.scaled(ups.utility_panel_item_height_px)))
            .w_full()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(self.scaled(ups.utility_panel_item_gap_px)))
            .text_size(px(self.scaled(ups.utility_panel_item_font_size_px)))
            .child(
                div()
                    .min_w(px(0.0))
                    .flex_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(ups.utility_panel_item_value_text_color)
                    .child(SharedString::from(item.label.clone())),
            )
            .child(
                img(helpers::resolve_asset_image_path(SHARE_SVG_PATH))
                    .w(px(self.scaled(ups.utility_panel_item_font_size_px)))
                    .h(px(self.scaled(ups.utility_panel_item_font_size_px)))
                    .flex_none(),
            );

        match (item.action, item.path.clone()) {
            (Some(action), Some(path)) => base
                .cursor(CursorStyle::Arrow)
                .hover(move |style| style.bg(ups.utility_panel_button_hover_bg))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        match action {
                            InfoPanelResourceAction::RevealInFinder => this.reveal_in_finder(&path),
                            InfoPanelResourceAction::OpenPreviewTab => {
                                this.open_path_in_preview_tab(path.clone(), true, cx);
                            }
                        }
                    }),
                )
                .into_any_element(),
            _ => base.opacity(0.55).into_any_element(),
        }
    }

    fn render_external_link_item(
        &self,
        item: &InfoPanelExternalLinkItem,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ups = &self.styles.utility_panel;
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
                cx.listener({
                    let url = item.url.clone();
                    move |_this, _, _, cx| {
                        cx.stop_propagation();
                        if let Err(error) = Command::new("open").arg(&url).spawn() {
                            eprintln!("failed to open URL {url}: {error}");
                        }
                    }
                }),
            )
            .child(
                div()
                    .min_w(px(0.0))
                    .flex_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_color(ups.utility_panel_item_value_text_color)
                    .child(SharedString::from(item.label.clone())),
            )
            .child(
                img(helpers::resolve_asset_image_path(SHARE_SVG_PATH))
                    .w(px(self.scaled(ups.utility_panel_item_font_size_px)))
                    .h(px(self.scaled(ups.utility_panel_item_font_size_px)))
                    .flex_none(),
            )
            .into_any_element()
    }

    fn render_git_history_item(&self, item: &GitHistoryItem, cx: &mut Context<Self>) -> AnyElement {
        let ups = &self.styles.utility_panel;
        let history_item = item.clone();
        div()
            .px(px(self.scaled(ups.utility_panel_section_padding_x_px)))
            .py(px(self.scaled(ups.utility_panel_item_gap_px)))
            .w_full()
            .flex()
            .flex_col()
            .gap(px(self.scaled(ups.utility_panel_item_gap_px) * 0.5))
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
                    .text_size(px(self.scaled(ups.utility_panel_item_font_size_px)))
                    .child(
                        div()
                            .text_color(ups.utility_panel_item_value_text_color)
                            .child(SharedString::from(item.short_hash.clone())),
                    )
                    .child(
                        div()
                            .text_color(ups.utility_panel_item_label_text_color)
                            .child(SharedString::from(item.time_label.clone())),
                    ),
            )
            .child(
                div()
                    .text_size(px(self.scaled(ups.utility_panel_item_font_size_px)))
                    .text_color(ups.utility_panel_item_value_text_color)
                    .child(SharedString::from(item.message.clone())),
            )
            .into_any_element()
    }

    pub(crate) fn render_git_diff_view(&mut self, _cx: &mut Context<Self>) -> AnyElement {
        let Some(diff) = self.active_git_diff_state() else {
            return div().into_any_element();
        };

        let line_number_color = self
            .styles
            .utility_panel
            .utility_panel_item_label_text_color;
        let text_color = self.styles.editor.editor_text_color;
        let added = gpui::rgb(0x2da44e);
        let removed = gpui::rgb(0xcf222e);
        let added_color: gpui::Hsla = added.into();
        let removed_color: gpui::Hsla = removed.into();
        let added_bg = gpui::Hsla {
            a: 0.12,
            ..added_color
        };
        let removed_bg = gpui::Hsla {
            a: 0.12,
            ..removed_color
        };

        let mut children = vec![
            div()
                .px(px(16.0))
                .py(px(12.0))
                .flex()
                .flex_col()
                .gap(px(4.0))
                .border_b(px(1.0))
                .border_color(self.styles.utility_panel.utility_panel_border_color)
                .child(
                    div()
                        .text_size(px(self.scaled(
                            self.styles.utility_panel.utility_panel_header_font_size_px,
                        )))
                        .text_color(self.styles.utility_panel.utility_panel_header_text_color)
                        .child(format!("{} -> {}", diff.source_label, diff.target_label)),
                )
                .into_any_element(),
        ];

        for row in &diff.rows {
            let background = match row.kind {
                GitDiffRowKind::Context => gpui::transparent_black(),
                GitDiffRowKind::Added => added_bg,
                GitDiffRowKind::Removed => removed_bg,
            };
            let prefix = match row.kind {
                GitDiffRowKind::Context => " ",
                GitDiffRowKind::Added => "+",
                GitDiffRowKind::Removed => "-",
            };
            let old_number = row
                .old_line_number
                .map(|value| value.to_string())
                .unwrap_or_default();
            let new_number = row
                .new_line_number
                .map(|value| value.to_string())
                .unwrap_or_default();

            children.push(
                div()
                    .px(px(16.0))
                    .py(px(4.0))
                    .w_full()
                    .flex()
                    .items_start()
                    .gap(px(10.0))
                    .bg(background)
                    .font_family("Menlo")
                    .text_size(px(12.0))
                    .child(
                        div()
                            .w(px(34.0))
                            .text_color(line_number_color)
                            .child(old_number),
                    )
                    .child(
                        div()
                            .w(px(34.0))
                            .text_color(line_number_color)
                            .child(new_number),
                    )
                    .child(
                        div()
                            .w(px(12.0))
                            .text_color(match row.kind {
                                GitDiffRowKind::Added => added_color,
                                GitDiffRowKind::Removed => removed_color,
                                GitDiffRowKind::Context => line_number_color,
                            })
                            .child(prefix),
                    )
                    .child(
                        div()
                            .flex_1()
                            .text_color(text_color)
                            .child(if row.text.is_empty() {
                                SharedString::from(" ")
                            } else {
                                SharedString::from(row.text.clone())
                            }),
                    )
                    .into_any_element(),
            );
        }

        div()
            .id("git-diff-view")
            .h_full()
            .w_full()
            .bg(self.styles.editor.editor_bg)
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .children(children)
            .into_any_element()
    }

    pub(crate) fn render_utility_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        match self.utility_panel {
            Some(RightUtilityPanel::Outline) => self.render_outline(cx),
            Some(RightUtilityPanel::Info) => self.render_info_panel(cx),
            Some(RightUtilityPanel::GitHistory) => self.render_git_history_panel(cx),
            None => div().into_any_element(),
        }
    }

    pub(crate) fn render_info_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let data = self.collect_info_panel_data();
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
                .child("当前文件信息")
                .into_any_element(),
        ];

        if let Some(data) = data {
            children.push(div().h(px(section_gap)).flex_shrink_0().into_any_element());
            children.push(self.render_info_section_title("文件"));
            for row in &data.primary_rows {
                children.push(self.render_info_row(row));
            }

            children.push(div().h(px(section_gap)).flex_shrink_0().into_any_element());
            children.push(self.render_info_section_title("导出"));
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
                        self.render_export_button("图片", false, cx, |_, _| {}),
                    ])
                    .into_any_element(),
            );

            if data.supports_text_export {
                children.push(div().h(px(section_gap)).flex_shrink_0().into_any_element());
                children.push(self.render_info_section_title("附件"));
                if data.attachment_items.is_empty() {
                    children.push(self.render_info_empty_line("未发现本地附件。"));
                } else {
                    for item in &data.attachment_items {
                        children.push(self.render_resource_item(item, cx));
                    }
                }

                children.push(div().h(px(section_gap)).flex_shrink_0().into_any_element());
                children.push(self.render_info_section_title("图片"));
                if data.image_items.is_empty() {
                    children.push(self.render_info_empty_line("未发现可预览图片。"));
                } else {
                    for item in &data.image_items {
                        children.push(self.render_resource_item(item, cx));
                    }
                }

                children.push(div().h(px(section_gap)).flex_shrink_0().into_any_element());
                children.push(self.render_info_section_title("链接"));
                if data.external_link_items.is_empty() {
                    children.push(self.render_info_empty_line("未发现链接。"));
                } else {
                    for item in &data.external_link_items {
                        children.push(self.render_external_link_item(item, cx));
                    }
                }
            }

            if !data.front_matter_rows.is_empty() {
                children.push(div().h(px(section_gap)).flex_shrink_0().into_any_element());
                children.push(self.render_info_section_title("属性"));
                for row in &data.front_matter_rows {
                    children.push(self.render_info_row(row));
                }
            }
        } else {
            children.push(
                div()
                    .px(px(section_padding_x))
                    .pt(px(section_gap))
                    .text_size(px(empty_font_size))
                    .text_color(ups.utility_panel_empty_text_color)
                    .child(SharedString::from("当前标签没有可展示的文件信息。"))
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
        let ups = &self.styles.utility_panel;
        let top_spacer = self.scaled(ups.utility_panel_top_spacer_height_px);
        let bottom_spacer = self.scaled(ups.utility_panel_bottom_spacer_height_px);
        let header_height = self.scaled(ups.utility_panel_header_height_px);
        let header_padding_x = self.scaled(ups.utility_panel_header_padding_x_px);
        let header_font_size = self.scaled(ups.utility_panel_header_font_size_px);
        let section_gap = self.scaled(ups.utility_panel_section_gap_px);
        let section_padding_x = self.scaled(ups.utility_panel_section_padding_x_px);
        let empty_font_size = self.scaled(ups.utility_panel_empty_font_size_px);

        let history_items = self.collect_git_history_items();
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
                .child("Git 历史")
                .into_any_element(),
        ];

        match history_items {
            Some(items) if !items.is_empty() => {
                children.push(div().h(px(section_gap)).flex_shrink_0().into_any_element());
                for item in &items {
                    children.push(self.render_git_history_item(item, cx));
                }
            }
            Some(_) => {
                children.push(
                    div()
                        .px(px(section_padding_x))
                        .pt(px(section_gap))
                        .text_size(px(empty_font_size))
                        .text_color(ups.utility_panel_empty_text_color)
                        .child(SharedString::from("当前文件没有 Git 历史。"))
                        .into_any_element(),
                );
            }
            None => {
                children.push(
                    div()
                        .px(px(section_padding_x))
                        .pt(px(section_gap))
                        .text_size(px(empty_font_size))
                        .text_color(ups.utility_panel_empty_text_color)
                        .child(SharedString::from("当前文件不在 Git 仓库中。"))
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
            .border_l(px(ups.utility_panel_border_width_px))
            .border_color(ups.utility_panel_border_color)
            .flex()
            .flex_col()
            .children(children)
            .into_any_element()
    }
}
