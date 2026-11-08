use std::fs;
use std::mem::size_of;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use gpui::{AnyElement, Context, MouseButton, SharedString, div, prelude::*, px};
use pulldown_cmark::{Event, LinkType, Options, Parser, Tag, TagEnd};
use unicode_segmentation::UnicodeSegmentation;

use super::export::{ExportFormat, ExportTarget};
use super::*;

#[derive(Clone)]
struct InfoPanelRow {
    label: String,
    value: String,
    tooltip: Option<String>,
}

fn export_buttons_height(bounds: &[Bounds<Pixels>]) -> Option<f32> {
    let first = bounds.first()?;
    let top = bounds
        .iter()
        .fold(first.top(), |top, bound| top.min(bound.top()));
    let bottom = bounds
        .iter()
        .fold(first.bottom(), |bottom, bound| bottom.max(bound.bottom()));
    Some(f32::from(bottom - top))
}

fn export_buttons_row(padding: f32, gap: f32) -> gpui::Div {
    div()
        .px(px(padding))
        .flex()
        .flex_wrap()
        .flex_shrink_0()
        .items_center()
        .gap(px(gap))
}

fn export_copy_icon(
    style: crate::style::ControlStyle,
    state: crate::style::ControlState,
    scale: f32,
) -> gpui::Svg {
    let color = style.resolve(state).foreground;
    let hover_color = style
        .resolve(crate::style::ControlState {
            hovered: true,
            ..state
        })
        .foreground;
    crate::ui::button::icon(style, MENU_CLIP_SVG_PATH, color, scale)
        .flex_none()
        .group_hover("export-copy-button", move |icon| {
            icon.text_color(hover_color)
        })
}

#[cfg(test)]
#[test]
fn export_copy_icons_have_explicit_visible_colors_in_each_state() {
    for kind in [
        crate::theme::ThemeKind::OneDark,
        crate::theme::ThemeKind::PaperWhite,
    ] {
        let theme = kind.build();
        let styles = crate::style::Styles::from_theme(&theme, &theme);
        let style = styles.utility_export_copy_button();
        for selected in [false, true] {
            for disabled in [false, true] {
                let state = crate::style::ControlState {
                    selected,
                    disabled,
                    ..Default::default()
                };
                let mut icon = export_copy_icon(style, state, 1.5);
                let color = icon.style().text.as_ref().unwrap().color;
                assert_eq!(color, Some(style.resolve(state).foreground));
                assert!(color.unwrap().a > 0.0);
                assert_eq!(
                    icon.style().size.width,
                    Some(px(style.icon_size_px * 1.5).into())
                );
            }
        }
    }
}

#[cfg(test)]
#[test]
fn export_buttons_height_tracks_actual_wrapped_rows() {
    let bounds = [
        Bounds::new(
            gpui::point(px(0.0), px(100.0)),
            gpui::size(px(80.0), px(24.0)),
        ),
        Bounds::new(
            gpui::point(px(0.0), px(128.0)),
            gpui::size(px(50.0), px(24.0)),
        ),
    ];
    assert_eq!(export_buttons_height(&bounds), Some(52.0));
    assert_eq!(export_buttons_height(&bounds[..1]), Some(24.0));
    assert_eq!(export_buttons_height(&[]), None);
    let mut row = export_buttons_row(12.0, 4.0);
    assert_eq!(row.style().flex_wrap, Some(gpui::FlexWrap::Wrap));
    assert_eq!(row.style().padding.left, Some(px(12.0).into()));
}

#[derive(Clone)]
pub(crate) struct InfoPanelData {
    primary_rows: Vec<InfoPanelRow>,
    attachment_items: Vec<InfoPanelReferenceItem>,
    image_items: Vec<InfoPanelReferenceItem>,
    external_link_items: Vec<InfoPanelReferenceItem>,
    wiki_link_items: Vec<InfoPanelReferenceItem>,
    supports_text_export: bool,
    resources_indexed: bool,
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

impl InfoPanelShareAction {
    fn tooltip_key(&self) -> crate::i18n::TextKey {
        match self {
            Self::RevealPath(_) => crate::i18n::TextKey::RevealInFinder,
            Self::OpenUrl(_) => crate::i18n::TextKey::OpenLink,
        }
    }
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

fn should_defer_info_panel_expensive_work(supports_text_export: bool, line_count: usize) -> bool {
    supports_text_export && line_count >= LARGE_MARKDOWN_MIN_LINES
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
            mouse_move_events: self.runtime_profile_mouse_move_events,
            render_frames: self.runtime_profile_render_frames,
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
            open_tabs: self.all_tabs().count(),
        }
    }

    pub(crate) fn invalidate_info_panel_cache(&mut self) {
        self.info_panel_cache_tab_id = None;
        self.info_panel_cache_result = None;
        self.info_panel_refresh_epoch = self.info_panel_refresh_epoch.wrapping_add(1);
        self.info_panel_refresh_task = None;
    }

    pub(crate) fn invalidate_active_file_panel_caches(&mut self) {
        self.invalidate_info_panel_cache();
    }

    fn cached_info_panel_data(&mut self, cx: &mut Context<Self>) -> Option<Arc<InfoPanelData>> {
        let current_tab_id = self.active_tab_id.clone();
        if self.info_panel_cache_tab_id != current_tab_id {
            self.info_panel_refresh_epoch = self.info_panel_refresh_epoch.wrapping_add(1);
            self.info_panel_refresh_task = None;
            self.info_panel_cache_tab_id = current_tab_id.clone();
            let defer_expensive = should_defer_info_panel_expensive_work(
                self.active_tab_supports_text_buffer(),
                self.buffer.line_count(),
            );
            self.info_panel_cache_result =
                Some(self.collect_info_panel_data(!defer_expensive).map(Arc::new));
            self.schedule_info_panel_refresh(current_tab_id, defer_expensive, cx);
        }

        self.info_panel_cache_result
            .as_ref()
            .and_then(|result| result.as_ref().cloned())
    }

    fn schedule_info_panel_refresh(
        &mut self,
        tab_id: Option<String>,
        count_words: bool,
        cx: &mut Context<Self>,
    ) {
        let text = count_words.then(|| self.buffer.text().to_string());
        let path = self.current_file_path.clone();
        let epoch = self.info_panel_refresh_epoch;
        self.info_panel_refresh_task = Some(cx.spawn(async move |this, cx| {
            let word_count = match text {
                Some(text) => Some(
                    cx.background_executor()
                        .spawn(async move { count_document_words(&text) })
                        .await,
                ),
                None => None,
            };
            let metadata = match path {
                Some(path) => crate::platform::fs_task::run(
                    "read file information",
                    path.clone(),
                    move || fs::metadata(path),
                )
                .await
                .map(Some),
                None => Ok(None),
            };
            this.update(cx, |editor, cx| {
                if editor.info_panel_refresh_epoch != epoch
                    || editor.info_panel_cache_tab_id != tab_id
                {
                    return;
                }
                let metadata_rows = metadata
                    .as_ref()
                    .ok()
                    .and_then(|metadata| metadata.as_ref())
                    .map(|metadata| {
                        let created = metadata.created().or_else(|_| metadata.modified()).ok();
                        let modified = metadata.modified().ok();
                        let time_row = |time: Option<SystemTime>, key| {
                            (
                                time.map(|time| {
                                    crate::i18n::format_relative_time(editor.ui_language, time)
                                })
                                .unwrap_or_else(|| {
                                    editor.t(crate::i18n::TextKey::Unavailable).to_string()
                                }),
                                time.map(|time| editor.detailed_time_label(key, time)),
                            )
                        };
                        (
                            time_row(created, crate::i18n::TextKey::InfoCreated),
                            time_row(modified, crate::i18n::TextKey::InfoModified),
                            format_file_size(metadata.len()),
                        )
                    });
                if let Some(Some(data)) = editor.info_panel_cache_result.as_mut() {
                    let data = Arc::make_mut(data);
                    if let Some(word_count) = word_count {
                        data.primary_rows[4].value = word_count.to_string();
                    }
                    if let Some((created, modified, size)) = metadata_rows {
                        (data.primary_rows[1].value, data.primary_rows[1].tooltip) = created;
                        (data.primary_rows[2].value, data.primary_rows[2].tooltip) = modified;
                        data.primary_rows[3].value = size;
                    }
                }
                if let Err(error) = metadata {
                    editor.notify_diagnostic(error, cx);
                }
                editor.info_panel_refresh_task = None;
                cx.notify();
            })
            .ok();
        }));
    }

    fn resolve_info_panel_reference_path(&self, raw: &str) -> Option<PathBuf> {
        let raw = raw.trim();
        if raw.is_empty() || raw.starts_with('#') {
            return None;
        }

        if crate::platform::paths::is_file_url(raw) {
            return crate::platform::paths::path_from_file_url(raw);
        }

        let expanded = crate::platform::paths::expand_tilde(raw)?;

        if expanded.is_absolute() {
            return Some(expanded);
        }

        if let Some(file_dir) = self
            .current_file_path
            .as_ref()
            .and_then(|path| path.parent())
        {
            return Some(file_dir.join(expanded));
        }

        Some(
            self.workspaces
                .first()
                .map_or(expanded.clone(), |root| root.join(&expanded)),
        )
    }

    fn classify_info_panel_reference_target(&self, raw: &str) -> Option<InfoPanelReferenceTarget> {
        let trimmed = raw.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return None;
        }

        if crate::platform::paths::is_file_url(trimmed) {
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

    fn reveal_panel_reference_in_file_manager(&mut self, path: &Path, cx: &mut Context<Self>) {
        let path = path.to_path_buf();
        cx.spawn(async move |this, cx| {
            let result = crate::platform::fs_task::run_anyhow(
                "reveal referenced file",
                path.clone(),
                move || {
                    match fs::metadata(&path) {
                        Ok(_) => crate::platform::shell::reveal(&path)?,
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                            let parent = path.parent().ok_or(error)?;
                            fs::metadata(parent)?;
                            crate::platform::shell::open_path(parent)?;
                        }
                        Err(error) => return Err(error.into()),
                    }
                    Ok(())
                },
            )
            .await;
            if let Err(error) = result {
                this.update(cx, |editor, cx| editor.notify_diagnostic(error, cx))
                    .ok();
            }
        })
        .detach();
    }

    fn trigger_info_panel_share_action(
        &mut self,
        action: &InfoPanelShareAction,
        cx: &mut Context<Self>,
    ) {
        match action {
            InfoPanelShareAction::RevealPath(path) => {
                self.reveal_panel_reference_in_file_manager(path, cx)
            }
            InfoPanelShareAction::OpenUrl(url) => {
                if let Err(error) = crate::platform::shell::open_url(url) {
                    log_error!("failed to open URL {url}: {error}");
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
                // Display the lexical target; resolving a wiki name must not walk
                // an unavailable workspace from the render path.
                let (path, _) = split_markdown_target(&wiki_link.target_path);
                let resolved_path = self.sidebar_root.join(path);

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

    fn collect_info_panel_data(&self, include_expensive: bool) -> Option<InfoPanelData> {
        let active_index = self.active_tab_index()?;
        let tab = self.tabs.get(active_index)?;
        let path = tab.path.as_deref();
        let relative_path = path
            .and_then(|path| self.relative_path_for_path(path))
            .or_else(|| path.map(|path| path.to_string_lossy().into_owned()))
            .unwrap_or_else(|| self.t(crate::i18n::TextKey::Unsaved).to_string());
        let unavailable = self.t(crate::i18n::TextKey::Unavailable).to_string();

        let supports_text_export = tab
            .file_kind
            .as_ref()
            .is_some_and(FileKind::supports_text_buffer);
        let text = if supports_text_export {
            self.buffer.text()
        } else {
            ""
        };

        let file_size = text.len() as u64;

        let word_count = if !supports_text_export {
            Some(0)
        } else if include_expensive {
            Some(count_document_words(text))
        } else {
            None
        };
        let line_count = if supports_text_export {
            self.buffer.line_count()
        } else {
            0
        };

        let (attachment_items, image_items, external_link_items, wiki_link_items) =
            if supports_text_export && include_expensive {
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
                    tooltip: None,
                },
                InfoPanelRow {
                    label: self.t(crate::i18n::TextKey::InfoCreated).to_string(),
                    value: unavailable.clone(),
                    tooltip: None,
                },
                InfoPanelRow {
                    label: self.t(crate::i18n::TextKey::InfoModified).to_string(),
                    value: unavailable,
                    tooltip: None,
                },
                InfoPanelRow {
                    label: self.t(crate::i18n::TextKey::InfoSize).to_string(),
                    value: format_file_size(file_size),
                    tooltip: None,
                },
                InfoPanelRow {
                    label: self.t(crate::i18n::TextKey::InfoWords).to_string(),
                    value: word_count
                        .map(|count| count.to_string())
                        .unwrap_or_else(|| "…".to_string()),
                    tooltip: None,
                },
                InfoPanelRow {
                    label: self.t(crate::i18n::TextKey::InfoLines).to_string(),
                    value: line_count.to_string(),
                    tooltip: None,
                },
            ],
            attachment_items,
            image_items,
            external_link_items,
            wiki_link_items,
            supports_text_export,
            resources_indexed: !supports_text_export || include_expensive,
        })
    }

    fn render_info_section_title(&self, title: &str, icon_path: &'static str) -> AnyElement {
        crate::ui::decoration::info_section(
            self.styles.info_section(),
            self.ui_scale,
            icon_path,
            title.to_string(),
        )
        .into_any_element()
    }

    fn render_info_row(&self, row: &InfoPanelRow, cx: &mut Context<Self>) -> AnyElement {
        let ups = &self.styles.utility_panel;
        let value = crate::ui::typography::fragment(ups.utility_panel_item_value_text_color)
            .flex_none()
            .ml_auto()
            .text_right()
            .max_w(px(self.utility_panel_layout_width() * 0.6))
            .overflow_hidden()
            .truncate()
            .whitespace_nowrap()
            .child(SharedString::from(row.value.clone()));
        let value = if let Some(tooltip) = row.tooltip.as_ref() {
            tooltip::with_tooltip(
                value,
                cx.entity().clone(),
                tooltip.clone(),
                None::<&'static str>,
            )
        } else {
            value.into_any_element()
        };
        crate::ui::panel::detail_row(ups.detail_row(false, true), self.ui_scale)
            .child(
                crate::ui::typography::fragment(ups.utility_panel_item_label_text_color)
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .truncate()
                    .whitespace_nowrap()
                    .child(SharedString::from(row.label.clone())),
            )
            .child(value)
            .into_any_element()
    }

    fn render_export_button(
        &self,
        format: ExportFormat,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let state = crate::style::ControlState {
            disabled: !enabled,
            ..Default::default()
        };
        let style = self.styles.utility_button();
        let button = crate::ui::button::button_frame(style, state, self.ui_scale)
            .rounded_r(px(0.0))
            .border_r(px(0.0))
            .child(format.label())
            .when(enabled, |button| {
                button.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.export_current(format, ExportTarget::File, cx);
                    }),
                )
            });
        let copied = self.export_is_copied(format);
        let copy_style = self.styles.utility_export_copy_button();
        let copy_state = crate::style::ControlState {
            selected: copied,
            ..state
        };
        let copy = crate::ui::button::button_frame(copy_style, copy_state, self.ui_scale)
            .group("export-copy-button")
            .rounded_l(px(0.0))
            .child(export_copy_icon(copy_style, copy_state, self.ui_scale))
            .when(enabled, |button| {
                button.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.export_current(format, ExportTarget::Clipboard, cx);
                    }),
                )
            });
        div()
            .flex()
            .flex_none()
            .child(tooltip::with_tooltip(
                button,
                cx.entity(),
                self.t(crate::i18n::TextKey::ExportToFile),
                None::<SharedString>,
            ))
            .child(tooltip::with_tooltip(
                copy,
                cx.entity(),
                self.t(if copied {
                    crate::i18n::TextKey::CopiedToClipboard
                } else {
                    crate::i18n::TextKey::CopyToClipboard
                }),
                None::<SharedString>,
            ))
            .into_any_element()
    }

    fn render_info_empty_line(&self, text: &str) -> AnyElement {
        let ups = &self.styles.utility_panel;
        crate::ui::panel::detail_row(ups.detail_row(false, false), self.ui_scale)
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
        let share_tooltip = self.t(item.share_action.tooltip_key());
        let source_line = item.source_line;
        let icon_size = self.scaled(ups.utility_panel_item_font_size_px);
        let share_icon_color = ups.utility_panel_button_text_color;
        crate::ui::panel::detail_row(ups.detail_row(true, false), self.ui_scale)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.scroll_to_line(source_line, cx);
                    this.request_editor_focus(cx);
                }),
            )
            .child(
                crate::ui::typography::fragment(ups.utility_panel_item_value_text_color)
                    .min_w(px(0.0))
                    .flex_1()
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .truncate()
                    .child(SharedString::from(item.label.clone())),
            )
            .child(tooltip::with_tooltip(
                crate::ui::button::button_frame(
                    self.styles.info_share_action(),
                    Default::default(),
                    self.ui_scale,
                )
                .w(px(self.scaled(self.styles.info_share_action().height_px)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.trigger_info_panel_share_action(&share_action, cx);
                    }),
                )
                .child(crate::ui::decoration::icon(
                    crate::style::IconStyle::new(f32::from(px(icon_size)), share_icon_color),
                    SHARE_SVG_PATH,
                )),
                cx.entity().clone(),
                share_tooltip,
                None::<&'static str>,
            ))
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

        let list_window = super::list_viewport::fixed_list_window(
            &self.info_panel_scroll_handle,
            self.last_window_height,
            *content_y,
            item_height,
            items.len(),
            10,
            20,
        );
        if list_window.top_spacer_px > 0.0 {
            children.push(
                div()
                    .h(px(list_window.top_spacer_px))
                    .flex_shrink_0()
                    .into_any_element(),
            );
        }
        for item in &items[list_window.render] {
            children.push(self.render_reference_item(item, cx));
        }
        if list_window.bottom_spacer_px > 0.0 {
            children.push(
                div()
                    .h(px(list_window.bottom_spacer_px))
                    .flex_shrink_0()
                    .into_any_element(),
            );
        }
        *content_y += items.len() as f32 * item_height;
    }

    pub(crate) fn render_utility_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        match self.displayed_utility_panel {
            Some(RightUtilityPanel::Info) => self.render_info_panel(cx),
            Some(RightUtilityPanel::Outline) => {
                self.render_cached_panel(super::motion::PanelKind::Outline, cx)
            }
            None => div().into_any_element(),
        }
    }

    pub(crate) fn render_info_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let data = self.cached_info_panel_data(cx);
        let ups = &self.styles.utility_panel;
        let top_spacer = self.scaled(ups.utility_panel_top_spacer_height_px);
        let bottom_spacer = self.scaled(ups.utility_panel_bottom_spacer_height_px);
        let header_height = self.scaled(ups.utility_panel_header_height_px);
        let section_gap = self.scaled(ups.utility_panel_section_gap_px);
        let item_gap = self.scaled(ups.utility_panel_item_gap_px);
        let section_padding_x = self.scaled(ups.utility_panel_section_padding_x_px);
        let empty_font_size = self.scaled(ups.utility_panel_empty_font_size_px);
        let item_height = self.scaled(ups.utility_panel_item_height_px).max(1.0);
        let section_title_height = self
            .scaled(ups.utility_panel_section_title_font_size_px + 8.0)
            .max(1.0);
        let export_height = self.info_panel_export_height.get().unwrap_or_else(|| {
            self.scaled(ups.utility_panel_button_height_px)
                .max(item_height)
        });
        let mut content_y = top_spacer + header_height;

        let mut children: Vec<AnyElement> = vec![
            div().h(px(top_spacer)).flex_shrink_0().into_any_element(),
            crate::ui::panel::header(ups.header(), self.ui_scale)
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
                children.push(self.render_info_row(row, cx));
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
                export_buttons_row(
                    section_padding_x,
                    self.scaled(ups.utility_panel_button_gap_px),
                )
                .on_children_prepainted({
                    let height = self.info_panel_export_height.clone();
                    let entity_id = cx.entity_id();
                    move |bounds: Vec<Bounds<Pixels>>, _, cx| {
                        if let Some(measured) = export_buttons_height(&bounds)
                            && height.get().is_none_or(|old| (old - measured).abs() > 0.5)
                        {
                            height.set(Some(measured));
                            cx.notify(entity_id);
                        }
                    }
                })
                .children(
                    [
                        ExportFormat::Markdown,
                        ExportFormat::Html,
                        ExportFormat::Pdf,
                        ExportFormat::Png,
                    ]
                    .into_iter()
                    .map(|format| self.render_export_button(format, data.supports_text_export, cx)),
                )
                .into_any_element(),
            );
            content_y += export_height;

            if data.supports_text_export && data.resources_indexed {
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
                crate::ui::typography::label(
                    crate::style::TextStyle::new(
                        f32::from(px(empty_font_size)),
                        ups.utility_panel_empty_text_color,
                    ),
                    1.0,
                )
                .px(px(section_padding_x))
                .pt(px(section_gap))
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
            self.scaled(self.styles.self_drawn.scrollbar.right_margin_px),
            cx,
        );

        crate::ui::panel::panel(ups.panel())
            .id("info-panel")
            .min_w(px(0.0))
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
}

#[cfg(test)]
mod tests {
    use super::{
        InfoPanelReferenceTarget, InfoPanelShareAction, external_reference_host,
        fallback_reference_label, path_reference_name, reference_scheme,
        should_defer_info_panel_expensive_work,
    };
    use pulldown_cmark::{Event, LinkType, Options, Parser, Tag};
    use std::path::PathBuf;

    #[test]
    fn large_text_documents_defer_expensive_info_collection() {
        assert!(!should_defer_info_panel_expensive_work(true, 99_999));
        assert!(should_defer_info_panel_expensive_work(true, 100_000));
        assert!(!should_defer_info_panel_expensive_work(false, 2_000_000));
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
    fn reference_action_tooltips_match_target_type() {
        assert_eq!(
            InfoPanelShareAction::RevealPath(PathBuf::from("/tmp/file.md")).tooltip_key(),
            crate::i18n::TextKey::RevealInFinder
        );
        assert_eq!(
            InfoPanelShareAction::OpenUrl("https://example.com".to_string()).tooltip_key(),
            crate::i18n::TextKey::OpenLink
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
}
