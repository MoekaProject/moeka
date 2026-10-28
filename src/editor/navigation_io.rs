use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};

use anyhow::Context as _;
use gpui::{Context, EntityId, RenderImage};

use crate::platform::fs_task;

use super::{
    EditorView, MarkdownNavigationTarget, PaneId, attachments, helpers, split_markdown_target,
};

/// Classification is deliberately lexical: prepaint calls this for every link.
pub(super) fn classify_target(target: &str) -> Option<MarkdownNavigationTarget> {
    let target = target.trim();
    if target.is_empty() || target.contains('\0') {
        return None;
    }
    if target.starts_with("http://")
        || target.starts_with("https://")
        || target.starts_with("mailto:")
    {
        return Some(MarkdownNavigationTarget::ExternalUrl(target.to_string()));
    }
    let (path, anchor) = split_markdown_target(target);
    let anchor = anchor
        .filter(|anchor| !anchor.is_empty())
        .map(str::to_string);
    if path.trim().is_empty() {
        return anchor.map(MarkdownNavigationTarget::CurrentAnchor);
    }
    // Keep the original spelling for the worker's URL decoding and wiki lookup.
    Some(MarkdownNavigationTarget::File {
        path: PathBuf::from(path.trim()),
        anchor,
    })
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DocumentContext {
    pane_id: PaneId,
    tab_id: Option<String>,
    file: Option<PathBuf>,
    root: PathBuf,
}

impl DocumentContext {
    pub(super) fn capture(editor: &EditorView) -> Self {
        Self {
            pane_id: editor.current_pane_id(),
            tab_id: editor.active_tab_id.clone(),
            file: editor.current_file_path.clone(),
            root: editor.sidebar_root.clone(),
        }
    }

    pub(super) fn pane_id(&self) -> PaneId {
        self.pane_id
    }

    pub(super) fn is_current(&self, editor: &EditorView) -> bool {
        *self == Self::capture(editor)
    }
}

fn navigation_request_is_current(
    source: &DocumentContext,
    current: &DocumentContext,
    epoch: usize,
    current_epoch: usize,
) -> bool {
    source == current && epoch == current_epoch
}

thread_local! {
    static IMAGE_REQUESTS: RefCell<HashMap<(EntityId, String), Weak<()>>> = RefCell::default();
}

pub(super) fn begin_image_request(entity_id: EntityId, path: &str) -> Arc<()> {
    let request = Arc::new(());
    IMAGE_REQUESTS.with_borrow_mut(|requests| {
        requests.retain(|_, request| request.strong_count() > 0);
        requests.insert((entity_id, path.to_string()), Arc::downgrade(&request));
    });
    request
}

pub(super) fn image_request_is_current(entity_id: EntityId, path: &str, request: &Arc<()>) -> bool {
    IMAGE_REQUESTS.with_borrow(|requests| {
        requests
            .get(&(entity_id, path.to_string()))
            .is_some_and(|current| current.ptr_eq(&Arc::downgrade(request)))
    })
}

fn expanded_image_target(target: &str) -> io::Result<String> {
    let decoded = percent_encoding::percent_decode_str(target)
        .decode_utf8()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    if decoded.starts_with("~/") || cfg!(windows) && decoded.starts_with("~\\") {
        return crate::platform::paths::expand_tilde(&decoded)
            .and_then(|path| crate::platform::paths::path_to_file_url(&path))
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "home directory is unavailable")
            });
    }
    Ok(target.to_string())
}

pub(super) fn image_operation_path(target: &str, source: Option<&Path>, root: &Path) -> PathBuf {
    if helpers::is_remote_image_url(target) {
        return PathBuf::from(target);
    }
    let candidates =
        expanded_image_target(target).and_then(|target| direct_candidates(root, source, &target));
    match candidates {
        Ok(paths) => paths
            .into_iter()
            .next()
            .unwrap_or_else(|| root.join(target)),
        Err(_) => root.join(target), // The worker reports the original parsing error.
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                if normalized.file_name().is_some_and(|name| name != "..") {
                    normalized.pop();
                } else if !normalized.has_root() {
                    normalized.push(component.as_os_str());
                }
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

fn direct_candidates(root: &Path, source: Option<&Path>, target: &str) -> io::Result<Vec<PathBuf>> {
    let (raw, _) = split_markdown_target(target);
    let raw = raw.trim();
    if crate::platform::paths::is_file_url(raw) {
        return crate::platform::paths::path_from_file_url(raw)
            .map(|path| vec![path])
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("invalid file URL: {raw}"),
                )
            });
    }
    let decoded = percent_encoding::percent_decode_str(raw)
        .decode_utf8()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
    let path = PathBuf::from(decoded.trim());
    let mut candidates = Vec::new();
    if path.is_absolute() {
        candidates.push(path);
        if let Some(relative) = decoded.strip_prefix('/') {
            candidates.push(root.join(relative));
        }
    } else {
        if let Some(directory) = source.and_then(Path::parent) {
            candidates.push(directory.join(&path));
        }
        if !root.as_os_str().is_empty() {
            candidates.push(root.join(path));
        }
        // Match the existing index resolver's extensionless wiki-link behavior.
        candidates = candidates
            .into_iter()
            .flat_map(|path| {
                let markdown = path
                    .extension()
                    .is_none()
                    .then(|| path.with_extension("md"));
                std::iter::once(path).chain(markdown)
            })
            .collect();
    }
    Ok(candidates
        .into_iter()
        .map(|path| normalize_path(&path))
        .collect())
}

fn candidate_exists(path: &Path) -> anyhow::Result<bool> {
    fs_task::check_deadline()?;
    match fs::metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error).with_context(|| format!("cannot inspect {}", path.display())),
    }
}

/// Worker-only resolution, with direct paths first so a nearby link does not
/// require traversing every directory (or a disconnected unrelated subtree).
fn resolve_local_target(
    root: &Path,
    source: Option<&Path>,
    target: &str,
) -> anyhow::Result<PathBuf> {
    fs_task::check_deadline()?;
    for candidate in direct_candidates(root, source, target)? {
        if candidate_exists(&candidate)? {
            fs_task::check_deadline()?;
            return Ok(candidate);
        }
    }
    let (raw, _) = split_markdown_target(target);
    if !root.as_os_str().is_empty()
        && !crate::platform::paths::is_file_url(raw)
        && !Path::new(raw).is_absolute()
    {
        fs_task::check_deadline()?;
        fs::read_dir(root)
            .with_context(|| format!("cannot search workspace {}", root.display()))?;
        let resolved = super::resolve_markdown_target_path(root, source, target);
        fs_task::check_deadline()?;
        if let Some(path) = resolved {
            // The index may have raced a removal, and its stat errors are optional.
            fs::metadata(&path).with_context(|| format!("cannot inspect {}", path.display()))?;
            return Ok(path);
        }
    }
    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!("link target ‘{target}’ was not found in {}", root.display()),
    )
    .into())
}

pub(super) fn read_document_image_bytes(
    target: &str,
    current_file: Option<&Path>,
    root: &Path,
    workspaces: &[PathBuf],
) -> anyhow::Result<Vec<u8>> {
    fs_task::check_deadline()?;
    if helpers::is_remote_image_url(target) {
        let response = ureq::AgentBuilder::new()
            .timeout(Duration::from_secs(20))
            .build()
            .get(target)
            .call()?;
        let mut bytes = Vec::new();
        response
            .into_reader()
            .take(64 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        anyhow::ensure!(
            bytes.len() <= 64 * 1024 * 1024,
            "image exceeds 64 MiB download limit"
        );
        fs_task::check_deadline()?;
        return Ok(bytes);
    }
    let expanded = expanded_image_target(target)?;
    for candidate_root in std::iter::once(root).chain(workspaces.iter().map(PathBuf::as_path)) {
        for candidate in direct_candidates(candidate_root, current_file, &expanded)? {
            if candidate_exists(&candidate)? {
                fs_task::check_deadline()?;
                return Ok(fs::read(candidate)?);
            }
        }
    }
    let path = resolve_local_target(root, current_file, &expanded)?;
    fs_task::check_deadline()?;
    Ok(fs::read(path)?)
}

pub(super) fn load_document_image(
    target: &str,
    current_file: Option<&Path>,
    root: &Path,
    workspaces: &[PathBuf],
) -> anyhow::Result<Arc<RenderImage>> {
    fs_task::check_deadline()?;
    let directory = current_file.and_then(Path::parent).map(Path::to_path_buf);
    if helpers::is_remote_image_url(target) {
        return helpers::load_image(target, directory.as_ref(), workspaces).map(Arc::new);
    }
    let expanded = expanded_image_target(target)?;
    let target = expanded.as_str();
    // Preserve multiple-workspace image fallback, but do not suppress an actual
    // permission/I/O failure while deciding which mount to use.
    let mut roots = vec![root.to_path_buf()];
    roots.extend(
        workspaces
            .iter()
            .filter(|workspace| workspace.as_path() != root)
            .cloned(),
    );
    for candidate_root in &roots {
        for candidate in direct_candidates(candidate_root, current_file, target)? {
            if candidate_exists(&candidate)? {
                fs_task::check_deadline()?;
                return helpers::load_render_image_from_path(&candidate).map(Arc::new);
            }
        }
    }
    let resolved = resolve_local_target(root, current_file, target)?;
    fs_task::check_deadline()?;
    helpers::load_render_image_from_path(&resolved).map(Arc::new)
}

pub(super) fn unique_import_path(
    directory: &Path,
    stem: &str,
    extension: Option<&str>,
) -> io::Result<PathBuf> {
    let extension = extension
        .map(str::trim)
        .filter(|extension| !extension.is_empty())
        .map(|extension| extension.trim_start_matches('.'));
    for suffix in 0usize.. {
        fs_task::check_deadline()?;
        let name = match (suffix, extension) {
            (0, Some(extension)) => format!("{stem}.{extension}"),
            (0, None) => stem.to_string(),
            (_, Some(extension)) => format!("{stem}-{suffix}.{extension}"),
            (_, None) => format!("{stem}-{suffix}"),
        };
        let candidate = directory.join(name);
        match fs::symlink_metadata(&candidate) {
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(candidate),
            Err(error) => {
                return Err(io::Error::new(
                    error.kind(),
                    format!("{}: {error}", candidate.display()),
                ));
            }
        }
    }
    unreachable!("import filename suffix exhausted")
}

fn write_import_image(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    fs_task::check_deadline()?;
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("cannot create imported image {}", path.display()))?;
    fs_task::check_deadline()?;
    file.write_all(bytes)
        .with_context(|| format!("cannot write imported image {}", path.display()))?;
    fs_task::check_deadline()?;
    file.flush()
        .with_context(|| format!("cannot flush imported image {}", path.display()))?;
    Ok(())
}

pub(super) fn import_resources(
    policy: attachments::AttachmentPolicy,
    bookmark: &str,
    root: &Path,
    current_file: &Path,
    images: Vec<gpui::Image>,
    mut paths: Vec<PathBuf>,
) -> anyhow::Result<Option<String>> {
    fs_task::check_deadline()?;
    if !images.is_empty() {
        paths.retain(|path| !super::is_image_path(path));
    }
    paths.dedup();
    let mut files = Vec::new();
    for path in paths {
        fs_task::check_deadline()?;
        let metadata = fs::metadata(&path)
            .with_context(|| format!("cannot read attachment {}", path.display()))?;
        if metadata.is_file() {
            files.push(path);
        }
    }
    if images.is_empty() && files.is_empty() {
        return Ok(None);
    }
    fs_task::check_deadline()?;
    let access = policy.access(bookmark);
    fs_task::check_deadline()?;
    let policy = access.policy();
    if policy.location == attachments::AttachmentLocation::ExternalFolder {
        fs_task::check_deadline()?;
        fs::metadata(&policy.folder)
            .with_context(|| format!("cannot access attachment directory {}", policy.folder))?;
    }
    let directory = policy.target_directory(root, current_file)?;
    fs_task::check_deadline()?;
    fs::create_dir_all(&directory)
        .with_context(|| format!("cannot create attachment directory {}", directory.display()))?;
    let mut imported = Vec::new();
    for image in images {
        fs_task::check_deadline()?;
        let image = super::normalize_clipboard_image(&image);
        let path = unique_import_path(
            &directory,
            &super::timestamp_file_stem(),
            Some(super::image_extension(image.format)),
        )?;
        write_import_image(&path, &image.bytes)?;
        imported.push((path, true));
    }
    for source in files {
        fs_task::check_deadline()?;
        let extension = source.extension().and_then(|extension| extension.to_str());
        let stem = source
            .file_stem()
            .and_then(|stem| stem.to_str())
            .filter(|stem| !stem.is_empty())
            .unwrap_or("attachment");
        let path = unique_import_path(&directory, stem, extension)?;
        fs_task::check_deadline()?;
        super::file_ops::copy_file_data(&source, &path, false)
            .with_context(|| format!("cannot import {} to {}", source.display(), path.display()))?;
        imported.push((path, super::is_image_path(&source)));
    }
    fs_task::check_deadline()?;
    let index = if policy.path_style == attachments::AttachmentPathStyle::Shortest {
        Some(attachments::WorkspaceLinkIndex::build(root))
    } else {
        None
    };
    fs_task::check_deadline()?;
    let links = imported
        .iter()
        .map(|(path, image)| {
            fs_task::check_deadline()?;
            policy.reference_for(root, current_file, path, *image, index.as_ref())
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(Some(links.join("\n")))
}

impl EditorView {
    pub(super) fn schedule_markdown_navigation(
        &mut self,
        target: String,
        focus_editor: bool,
        cx: &mut Context<Self>,
    ) {
        let source = DocumentContext::capture(self);
        self.preview_open_epoch = self.preview_open_epoch.wrapping_add(1);
        let epoch = self.preview_open_epoch;
        let request = source.clone();
        let anchor = split_markdown_target(&target)
            .1
            .filter(|anchor| !anchor.is_empty())
            .map(str::to_string);
        let operation_path = source.file.clone().unwrap_or_else(|| source.root.clone());
        cx.spawn(async move |this, cx| {
            let result = fs_task::run_anyhow("resolve Markdown link", operation_path, move || {
                resolve_local_target(&request.root, request.file.as_deref(), &target)
            })
            .await;
            let Some((path, pane_id, open_epoch, root, waiting_tab_id)) = this
                .update(cx, |editor, cx| {
                    if editor.focused_pane_id != source.pane_id {
                        return None;
                    }
                    editor
                        .with_pane_if_alive(source.pane_id, |editor| {
                            if !navigation_request_is_current(
                                &source,
                                &DocumentContext::capture(editor),
                                epoch,
                                editor.preview_open_epoch,
                            ) {
                                return None;
                            }
                            match result {
                                Ok(path) => {
                                    editor.open_path_in_permanent_tab(
                                        path.clone(),
                                        focus_editor,
                                        cx,
                                    );
                                    Some((
                                        path,
                                        editor.current_pane_id(),
                                        editor.preview_open_epoch,
                                        editor.sidebar_root.clone(),
                                        editor.active_tab_id.clone(),
                                    ))
                                }
                                Err(error) => {
                                    editor.notify_error(error.to_string(), cx);
                                    None
                                }
                            }
                        })
                        .flatten()
                })
                .ok()
                .flatten()
            else {
                return;
            };
            let Some(anchor) = anchor else { return };
            // Tab loading is separately bounded. Wait only for this open request;
            // never reveal an anchor in the source file or after another click.
            let started = Instant::now();
            while started.elapsed() < Duration::from_secs(5) {
                let pending = this
                    .update(cx, |editor, cx| {
                        if editor.focused_pane_id != pane_id {
                            return false;
                        }
                        editor
                            .with_pane_if_alive(pane_id, |editor| {
                                if editor.preview_open_epoch != open_epoch
                                    || editor.sidebar_root != root
                                {
                                    return false;
                                }
                                if editor.current_file_path.as_ref() != Some(&path)
                                    || !editor.active_tab_supports_text_buffer()
                                {
                                    return editor.active_tab_id == waiting_tab_id;
                                }
                                match editor.find_anchor_line_in_current_buffer(&anchor) {
                                    Some(line) => editor.reveal_line_in_current_buffer(line, cx),
                                    None => editor.notify_error(
                                        format!(
                                            "anchor ‘{anchor}’ was not found in {}",
                                            path.display()
                                        ),
                                        cx,
                                    ),
                                }
                                false
                            })
                            .unwrap_or(false)
                    })
                    .unwrap_or(false);
                if !pending {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
            }
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_classification_is_lexical_even_for_unavailable_mounts() {
        assert_eq!(
            classify_target("/unavailable-mount/note.md#intro"),
            Some(MarkdownNavigationTarget::File {
                path: PathBuf::from("/unavailable-mount/note.md"),
                anchor: Some("intro".into()),
            })
        );
        assert_eq!(
            classify_target("#intro"),
            Some(MarkdownNavigationTarget::CurrentAnchor("intro".into()))
        );
        assert_eq!(
            classify_target("https://example.test/#intro"),
            Some(MarkdownNavigationTarget::ExternalUrl(
                "https://example.test/#intro".into()
            ))
        );
        assert!(classify_target(" ").is_none());
        assert!(classify_target("#").is_none());
        assert!(classify_target("invalid\0path.md").is_none());
    }

    #[test]
    fn direct_paths_keep_current_directory_precedence_and_wiki_extensions() {
        let root = Path::new("workspace");
        let source = root.join("notes/current.md");
        assert_eq!(
            direct_candidates(root, Some(&source), "other#intro").unwrap(),
            vec![
                root.join("notes/other"),
                root.join("notes/other.md"),
                root.join("other"),
                root.join("other.md"),
            ]
        );
    }

    #[test]
    fn worker_resolution_supports_direct_shortest_and_encoded_links() {
        let temp = tempfile::tempdir().unwrap();
        let notes = temp.path().join("notes");
        fs::create_dir_all(&notes).unwrap();
        let source = temp.path().join("current.md");
        let linked = notes.join("other note.md");
        fs::write(&linked, "# Intro").unwrap();
        assert_eq!(
            resolve_local_target(temp.path(), Some(&source), "notes/other%20note.md#intro")
                .unwrap(),
            linked
        );
        assert_eq!(
            resolve_local_target(temp.path(), Some(&source), "other note#intro").unwrap(),
            linked
        );
        let error = resolve_local_target(temp.path(), Some(&source), "missing.md").unwrap_err();
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    fn navigation_completions_reject_changed_root_pane_file_tab_or_epoch() {
        let source = DocumentContext {
            pane_id: 1,
            tab_id: Some("tab".into()),
            file: Some(PathBuf::from("workspace/note.md")),
            root: PathBuf::from("workspace"),
        };
        assert!(navigation_request_is_current(&source, &source, 2, 2));
        assert!(!navigation_request_is_current(&source, &source, 2, 3));
        let mut changed = source.clone();
        changed.root = PathBuf::from("other-workspace");
        assert!(!navigation_request_is_current(&source, &changed, 2, 2));
        changed = source.clone();
        changed.pane_id = 2;
        assert!(!navigation_request_is_current(&source, &changed, 2, 2));
        changed = source.clone();
        changed.file = Some(PathBuf::from("workspace/other.md"));
        assert!(!navigation_request_is_current(&source, &changed, 2, 2));
        changed = source.clone();
        changed.tab_id = Some("another-tab".into());
        assert!(!navigation_request_is_current(&source, &changed, 2, 2));
    }

    #[test]
    fn direct_links_do_not_need_an_available_workspace_root() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("current.md");
        let linked = temp.path().join("linked.md");
        fs::write(&linked, "# Linked").unwrap();
        let missing_root = temp.path().join("unavailable-mount");
        assert_eq!(
            resolve_local_target(&missing_root, Some(&source), "linked.md").unwrap(),
            linked
        );
    }

    #[test]
    fn image_save_reads_original_bytes_without_reencoding() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("note.md");
        let bytes =
            b"<svg xmlns=\"http://www.w3.org/2000/svg\"><rect width=\"10\" height=\"10\"/></svg>";
        fs::write(directory.path().join("original.svg"), bytes).unwrap();
        assert_eq!(
            read_document_image_bytes("original.svg", Some(&source), directory.path(), &[])
                .unwrap(),
            bytes
        );
    }

    #[test]
    fn home_relative_image_paths_keep_encoded_hashes_and_percent_signs() {
        let expanded = expanded_image_target("~/.moeka-image-test/image%23one%25.png").unwrap();
        let candidates = direct_candidates(Path::new("unused"), None, &expanded).unwrap();
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].ends_with(".moeka-image-test/image#one%.png"));
    }

    fn import_policy() -> attachments::AttachmentPolicy {
        attachments::AttachmentPolicy::from_settings(
            "workspace_folder",
            "assets",
            "",
            "markdown",
            "relative",
            false,
        )
    }

    #[test]
    fn missing_import_sources_report_the_original_error_before_creating_a_directory() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("current.md");
        let missing = temp.path().join("missing.pdf");
        let error = import_resources(
            import_policy(),
            "",
            temp.path(),
            &source,
            Vec::new(),
            vec![missing.clone()],
        )
        .unwrap_err();
        assert_eq!(
            error.downcast_ref::<io::Error>().unwrap().kind(),
            io::ErrorKind::NotFound
        );
        assert!(format!("{error:#}").contains(&missing.to_string_lossy().into_owned()));
        assert!(!temp.path().join("assets").exists());
    }

    #[test]
    fn imports_copy_content_and_emit_links_without_a_workspace_index_for_relative_paths() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("current.md");
        let attachment = temp.path().join("report.pdf");
        fs::write(&attachment, "report").unwrap();
        let insertion = import_resources(
            import_policy(),
            "",
            temp.path(),
            &source,
            Vec::new(),
            vec![attachment],
        )
        .unwrap();
        assert_eq!(
            insertion.as_deref(),
            Some("[report.pdf](assets/report.pdf)")
        );
        assert_eq!(
            fs::read_to_string(temp.path().join("assets/report.pdf")).unwrap(),
            "report"
        );
    }

    fn block_on<T>(future: impl std::future::Future<Output = T>) -> T {
        struct WakeThread(std::thread::Thread);
        impl std::task::Wake for WakeThread {
            fn wake(self: Arc<Self>) {
                self.0.unpark();
            }
            fn wake_by_ref(self: &Arc<Self>) {
                self.0.unpark();
            }
        }
        let waker = std::task::Waker::from(Arc::new(WakeThread(std::thread::current())));
        let mut context = std::task::Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        loop {
            if let std::task::Poll::Ready(result) = future.as_mut().poll(&mut context) {
                return result;
            }
            std::thread::park_timeout(Duration::from_millis(10));
        }
    }

    #[test]
    fn expired_workers_stop_traversal_and_import_before_writes() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().to_path_buf();
        fs_task::register_workspace_root(&root);
        let attachment = root.join("report.pdf");
        fs::write(&attachment, "report").unwrap();
        let request_root = root.clone();
        let (release, wait) = std::sync::mpsc::channel();
        let (observed, observations) = std::sync::mpsc::channel();
        let result: io::Result<()> = block_on(fs_task::run_with_timeout(
            "expired import stage regression",
            root.clone(),
            Duration::from_millis(100),
            move || {
                wait.recv().map_err(io::Error::other)?;
                let index = attachments::WorkspaceLinkIndex::build(&request_root);
                let indexed = index.shortest_unique_path(&attachment).is_some();
                let imported = import_resources(
                    import_policy(),
                    "",
                    &request_root,
                    &request_root.join("current.md"),
                    Vec::new(),
                    vec![attachment],
                );
                observed
                    .send((indexed, imported.is_err()))
                    .map_err(io::Error::other)?;
                Ok(())
            },
        ));
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::TimedOut);
        release.send(()).unwrap();
        assert_eq!(
            observations.recv_timeout(Duration::from_secs(2)).unwrap(),
            (false, true)
        );
        assert!(!root.join("assets").exists());
    }

    #[test]
    fn image_loading_preserves_decode_errors_instead_of_caching_silent_failure() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("current.md");
        let image = temp.path().join("image.png");
        image::RgbaImage::new(1, 1).save(&image).unwrap();
        assert!(load_document_image("image.png", Some(&source), temp.path(), &[]).is_ok());
        fs::write(&image, b"not an image").unwrap();
        let error = match load_document_image("image.png", Some(&source), temp.path(), &[]) {
            Err(error) => error,
            Ok(_) => panic!("invalid image should report its decode error"),
        };
        assert!(!error.to_string().is_empty());
    }

    #[test]
    fn clipboard_images_are_written_with_image_references() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("current.md");
        let image = temp.path().join("clipboard.png");
        image::RgbaImage::new(1, 1).save(&image).unwrap();
        let bytes = fs::read(&image).unwrap();
        let image = gpui::Image::from_bytes(gpui::ImageFormat::Png, bytes.clone());
        let insertion = import_resources(
            import_policy(),
            "",
            temp.path(),
            &source,
            vec![image],
            Vec::new(),
        )
        .unwrap()
        .unwrap();
        assert!(insertion.starts_with("![](assets/"));
        let imported = fs::read_dir(temp.path().join("assets"))
            .unwrap()
            .collect::<io::Result<Vec<_>>>()
            .unwrap();
        assert_eq!(imported.len(), 1);
        assert_eq!(fs::read(imported[0].path()).unwrap(), bytes);
    }

    #[test]
    fn import_names_do_not_overwrite_existing_files() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("image.png");
        fs::write(&first, "existing").unwrap();
        let next = unique_import_path(temp.path(), "image", Some("png")).unwrap();
        assert_eq!(next, temp.path().join("image-1.png"));
        assert!(write_import_image(&first, b"replacement").is_err());
        assert_eq!(fs::read(first).unwrap(), b"existing");
    }
}
