use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context as _, Result, anyhow, bail};
use percent_encoding::{AsciiSet, CONTROLS, percent_decode_str, utf8_percent_encode};

use crate::markdown::{LocalLinkSyntax, parse_local_link_targets};

use super::{file_ops, should_skip_path};

const MARKDOWN_PATH_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'%')
    .add(b'(')
    .add(b')')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'[')
    .add(b'\\')
    .add(b']')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'|')
    .add(b'}');

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AttachmentLocation {
    CurrentFile,
    WorkspaceRoot,
    WorkspaceFolder,
    CurrentFileSubfolder,
}

impl AttachmentLocation {
    fn from_setting(value: &str) -> Self {
        match value {
            "workspace_root" => Self::WorkspaceRoot,
            "workspace_folder" => Self::WorkspaceFolder,
            "current_file_subfolder" => Self::CurrentFileSubfolder,
            _ => Self::CurrentFile,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AttachmentLinkSyntax {
    Markdown,
    WikiLink,
}

impl AttachmentLinkSyntax {
    fn from_setting(value: &str) -> Self {
        match value {
            "wikilink" => Self::WikiLink,
            _ => Self::Markdown,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AttachmentPathStyle {
    Relative,
    WorkspaceAbsolute,
    Shortest,
}

impl AttachmentPathStyle {
    fn from_setting(value: &str) -> Self {
        match value {
            "workspace_absolute" => Self::WorkspaceAbsolute,
            "shortest" => Self::Shortest,
            _ => Self::Relative,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AttachmentPolicy {
    pub location: AttachmentLocation,
    pub folder: String,
    pub link_syntax: AttachmentLinkSyntax,
    pub path_style: AttachmentPathStyle,
    pub automatically_update_links: bool,
}

impl AttachmentPolicy {
    pub(crate) fn from_settings(
        location: &str,
        folder: &str,
        link_syntax: &str,
        path_style: &str,
        automatically_update_links: bool,
    ) -> Self {
        Self {
            location: AttachmentLocation::from_setting(location),
            folder: folder.to_string(),
            link_syntax: AttachmentLinkSyntax::from_setting(link_syntax),
            path_style: AttachmentPathStyle::from_setting(path_style),
            automatically_update_links,
        }
    }

    pub(crate) fn target_directory(
        &self,
        workspace_root: &Path,
        current_file: &Path,
    ) -> Result<PathBuf> {
        if self.path_style != AttachmentPathStyle::Relative {
            ensure_file_in_workspace(workspace_root, current_file)?;
        }
        let current_dir = current_file
            .parent()
            .ok_or_else(|| anyhow!("the current file has no parent directory"))?;
        let target = match self.location {
            AttachmentLocation::CurrentFile => current_dir.to_path_buf(),
            AttachmentLocation::WorkspaceRoot => {
                ensure_file_in_workspace(workspace_root, current_file)?;
                workspace_root.to_path_buf()
            }
            AttachmentLocation::WorkspaceFolder => {
                ensure_file_in_workspace(workspace_root, current_file)?;
                workspace_root.join(validate_relative_folder(&self.folder)?)
            }
            AttachmentLocation::CurrentFileSubfolder => {
                ensure_file_in_workspace(workspace_root, current_file)?;
                current_dir.join(validate_relative_folder(&self.folder)?)
            }
        };
        if self.location != AttachmentLocation::CurrentFile && !target.starts_with(workspace_root) {
            bail!("attachment directory must stay inside the workspace");
        }
        Ok(target)
    }

    pub(crate) fn reference_for(
        &self,
        workspace_root: &Path,
        source_file: &Path,
        target_file: &Path,
        is_image: bool,
        index: Option<&WorkspaceLinkIndex>,
    ) -> Result<String> {
        let target = self.target_for(workspace_root, source_file, target_file, index)?;
        Ok(match self.link_syntax {
            AttachmentLinkSyntax::Markdown => {
                if is_image {
                    format!("![]({target})")
                } else {
                    let label = target_file
                        .file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or(&target);
                    format!("[{label}]({target})")
                }
            }
            AttachmentLinkSyntax::WikiLink => {
                if is_image {
                    format!("![[{target}]]")
                } else {
                    format!("[[{target}]]")
                }
            }
        })
    }

    fn target_for(
        &self,
        workspace_root: &Path,
        source_file: &Path,
        target_file: &Path,
        index: Option<&WorkspaceLinkIndex>,
    ) -> Result<String> {
        let target = match self.path_style {
            AttachmentPathStyle::Relative => {
                let source_dir = source_file
                    .parent()
                    .ok_or_else(|| anyhow!("the source file has no parent directory"))?;
                relative_path(source_dir, target_file)
                    .ok_or_else(|| anyhow!("could not create a relative attachment path"))?
            }
            AttachmentPathStyle::WorkspaceAbsolute => target_file
                .strip_prefix(workspace_root)
                .context("attachment is outside the workspace")?
                .to_path_buf(),
            AttachmentPathStyle::Shortest => index
                .and_then(|index| index.shortest_unique_path(target_file))
                .or_else(|| {
                    target_file
                        .strip_prefix(workspace_root)
                        .ok()
                        .map(Path::to_path_buf)
                })
                .ok_or_else(|| anyhow!("attachment is outside the workspace"))?,
        };
        let mut target = path_to_slashes(&target);
        if self.link_syntax == AttachmentLinkSyntax::WikiLink
            && target_file
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
        {
            target.truncate(target.len().saturating_sub(3));
        }
        if self.link_syntax == AttachmentLinkSyntax::Markdown {
            target = encode_markdown_path(&target);
        }
        Ok(target)
    }
}

#[derive(Clone, Debug, Default)]
pub(crate) struct WorkspaceLinkIndex {
    root: PathBuf,
    files: Vec<PathBuf>,
    relative_paths: HashMap<PathBuf, PathBuf>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PathMapping {
    pub old_path: PathBuf,
    pub new_path: PathBuf,
}

#[derive(Clone, Debug)]
pub(crate) struct WorkspaceRewriteContext {
    root: PathBuf,
    index: WorkspaceLinkIndex,
    documents: Vec<(PathBuf, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct WorkspaceLinkRewrite {
    pub old_path: PathBuf,
    pub new_path: PathBuf,
    pub text: String,
}

impl PathMapping {
    pub(crate) fn new(old_path: PathBuf, new_path: PathBuf) -> Self {
        Self { old_path, new_path }
    }
}

impl WorkspaceRewriteContext {
    pub(crate) fn plan(
        &self,
        mappings: &[PathMapping],
        policy: &AttachmentPolicy,
    ) -> Vec<WorkspaceLinkRewrite> {
        self.documents
            .iter()
            .filter_map(|(old_path, text)| {
                let new_path = remap_path(old_path, mappings);
                let rewritten = rewrite_local_links_for_move(
                    text,
                    &self.root,
                    old_path,
                    &new_path,
                    mappings,
                    &self.index,
                    policy,
                );
                (rewritten != *text).then_some(WorkspaceLinkRewrite {
                    old_path: old_path.clone(),
                    new_path,
                    text: rewritten,
                })
            })
            .collect()
    }
}

pub(crate) fn write_text_atomically(path: &Path, text: &str) -> Result<()> {
    file_ops::atomic_write(path, text.as_bytes())
        .with_context(|| format!("failed to replace {}", path.display()))
}

impl WorkspaceLinkIndex {
    pub(crate) fn build(root: &Path) -> Self {
        let mut files = Vec::new();
        collect_workspace_files(root, &mut files);
        let relative_paths = files
            .iter()
            .filter_map(|path| {
                path.strip_prefix(root)
                    .ok()
                    .map(|relative| (path.clone(), relative.to_path_buf()))
            })
            .collect();
        Self {
            root: root.to_path_buf(),
            files,
            relative_paths,
        }
    }

    pub(crate) fn shortest_unique_path(&self, target: &Path) -> Option<PathBuf> {
        let relative = self.relative_paths.get(target)?;
        let components: Vec<_> = relative.components().collect();
        for component_count in 1..=components.len() {
            let suffix = components[components.len() - component_count..]
                .iter()
                .collect::<PathBuf>();
            let matches = self
                .relative_paths
                .values()
                .filter(|candidate| candidate.ends_with(&suffix))
                .take(2)
                .count();
            if matches == 1 {
                return Some(suffix);
            }
        }
        Some(relative.clone())
    }

    pub(crate) fn resolve(&self, source_file: Option<&Path>, raw_target: &str) -> Option<PathBuf> {
        let raw_path = raw_target
            .split_once('#')
            .map_or(raw_target, |(path, _)| path);
        if crate::platform::paths::is_file_url(raw_path) {
            let path = crate::platform::paths::path_from_file_url(raw_path)?;
            return path.exists().then_some(path);
        }
        let decoded = percent_decode_str(raw_path).decode_utf8().ok()?;
        let decoded = decoded.trim();
        if decoded.is_empty() {
            return source_file.map(Path::to_path_buf);
        }

        let path = PathBuf::from(decoded.as_ref() as &str);
        if path.is_absolute() {
            let workspace_path = decoded
                .strip_prefix('/')
                .map(|relative| self.root.join(relative));
            return path
                .exists()
                .then_some(path)
                .or_else(|| workspace_path.filter(|candidate| candidate.exists()));
        }

        let mut direct_candidates = Vec::new();
        if let Some(source_dir) = source_file.and_then(Path::parent) {
            direct_candidates.push(source_dir.join(&path));
        }
        direct_candidates.push(self.root.join(&path));
        for candidate in direct_candidates
            .into_iter()
            .map(|path| normalize_path(&path))
        {
            if candidate.exists() {
                return Some(candidate);
            }
            if candidate.extension().is_none() {
                let markdown = candidate.with_extension("md");
                if markdown.exists() {
                    return Some(markdown);
                }
            }
        }

        let mut suffix_matches = self.files.iter().filter(|candidate| {
            self.relative_paths.get(*candidate).is_some_and(|relative| {
                relative.ends_with(&path)
                    || (path.extension().is_none() && relative.with_extension("").ends_with(&path))
            })
        });
        let match_path = suffix_matches.next()?.clone();
        suffix_matches.next().is_none().then_some(match_path)
    }

    fn remapped(&self, mappings: &[PathMapping]) -> Self {
        let files: Vec<_> = self
            .files
            .iter()
            .map(|path| remap_path(path, mappings))
            .collect();
        let relative_paths = files
            .iter()
            .filter_map(|path| {
                path.strip_prefix(&self.root)
                    .ok()
                    .map(|relative| (path.clone(), relative.to_path_buf()))
            })
            .collect();
        Self {
            root: self.root.clone(),
            files,
            relative_paths,
        }
    }
}

pub(crate) fn rewrite_local_links_for_move(
    text: &str,
    workspace_root: &Path,
    source_before: &Path,
    source_after: &Path,
    mappings: &[PathMapping],
    old_index: &WorkspaceLinkIndex,
    policy: &AttachmentPolicy,
) -> String {
    if !policy.automatically_update_links || mappings.is_empty() {
        return text.to_string();
    }

    let new_index = old_index.remapped(mappings);
    let source_moved = source_before != source_after;
    let mut replacements = Vec::new();
    for link in parse_local_link_targets(text) {
        let (path_target, fragment) = split_target_fragment(&link.target);
        if path_target.is_empty() || is_non_local_target(path_target) {
            continue;
        }
        let Some(resolved_before) = old_index.resolve(Some(source_before), path_target) else {
            continue;
        };
        let resolved_after = remap_path(&resolved_before, mappings);
        if resolved_after == resolved_before && !source_moved {
            continue;
        }

        let mut link_policy = policy.clone();
        link_policy.link_syntax = match link.syntax {
            LocalLinkSyntax::Markdown => AttachmentLinkSyntax::Markdown,
            LocalLinkSyntax::WikiLink => AttachmentLinkSyntax::WikiLink,
        };
        let Ok(mut replacement) = link_policy.target_for(
            workspace_root,
            source_after,
            &resolved_after,
            Some(&new_index),
        ) else {
            continue;
        };
        replacement.push_str(fragment);
        if replacement != link.target {
            replacements.push((link.target_range, replacement));
        }
    }

    let mut rewritten = text.to_string();
    for (range, replacement) in replacements.into_iter().rev() {
        rewritten.replace_range(range, &replacement);
    }
    rewritten
}

fn remap_path(path: &Path, mappings: &[PathMapping]) -> PathBuf {
    mappings
        .iter()
        .filter(|mapping| path.starts_with(&mapping.old_path))
        .max_by_key(|mapping| mapping.old_path.components().count())
        .map(|mapping| {
            let relative = path
                .strip_prefix(&mapping.old_path)
                .unwrap_or_else(|_| Path::new(""));
            mapping.new_path.join(relative)
        })
        .unwrap_or_else(|| path.to_path_buf())
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir | Component::Normal(_) => {
                normalized.push(component.as_os_str());
            }
            Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push("..");
                }
            }
        }
    }
    normalized
}

fn split_target_fragment(target: &str) -> (&str, &str) {
    target
        .find('#')
        .map_or((target, ""), |index| (&target[..index], &target[index..]))
}

fn is_non_local_target(target: &str) -> bool {
    let lower = target.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
        || lower.starts_with("data:")
        || lower.starts_with("obsidian:")
}

fn ensure_file_in_workspace(workspace_root: &Path, current_file: &Path) -> Result<()> {
    if current_file.starts_with(workspace_root) {
        Ok(())
    } else {
        bail!("the current file is outside the workspace")
    }
}

fn validate_relative_folder(folder: &str) -> Result<PathBuf> {
    let trimmed = folder.trim().trim_matches('/');
    if trimmed.is_empty() {
        bail!("attachment folder cannot be empty");
    }
    let path = Path::new(trimmed);
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(component) => normalized.push(component),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!("attachment folder must be a relative path without '..'")
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        bail!("attachment folder cannot be empty");
    }
    Ok(normalized)
}

pub(crate) fn validated_attachment_folder(folder: &str) -> Option<String> {
    validate_relative_folder(folder)
        .ok()
        .map(|path| path_to_slashes(&path))
}

fn collect_workspace_files(directory: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_directory = path.is_dir();
        if should_skip_path(&path, is_directory) {
            continue;
        }
        if is_directory {
            collect_workspace_files(&path, files);
        } else {
            files.push(path);
        }
    }
}

fn relative_path(from_directory: &Path, to: &Path) -> Option<PathBuf> {
    let from_components: Vec<_> = from_directory.components().collect();
    let to_components: Vec<_> = to.components().collect();
    let shared = from_components
        .iter()
        .zip(&to_components)
        .take_while(|(left, right)| left == right)
        .count();
    if shared == 0 {
        return None;
    }
    let mut relative = PathBuf::new();
    for _ in shared..from_components.len() {
        relative.push("..");
    }
    for component in &to_components[shared..] {
        relative.push(component.as_os_str());
    }
    Some(relative)
}

fn path_to_slashes(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn encode_markdown_path(path: &str) -> String {
    utf8_percent_encode(path, MARKDOWN_PATH_ENCODE_SET).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(location: &str, syntax: &str, style: &str) -> AttachmentPolicy {
        AttachmentPolicy::from_settings(location, "assets/images", syntax, style, true)
    }

    #[test]
    fn attachment_policy_defaults_match_existing_behavior() {
        let policy = AttachmentPolicy::from_settings("", "", "", "", true);

        assert_eq!(policy.location, AttachmentLocation::CurrentFile);
        assert_eq!(policy.link_syntax, AttachmentLinkSyntax::Markdown);
        assert_eq!(policy.path_style, AttachmentPathStyle::Relative);
    }

    #[test]
    fn attachment_directory_rejects_parent_traversal() {
        let temp = tempfile::tempdir().expect("tempdir");
        let note = temp.path().join("notes/note.md");
        let policy = AttachmentPolicy::from_settings(
            "workspace_folder",
            "../outside",
            "markdown",
            "relative",
            true,
        );

        assert!(policy.target_directory(temp.path(), &note).is_err());
    }

    #[test]
    fn workspace_path_styles_reject_documents_outside_workspace_before_import() {
        let workspace = tempfile::tempdir().expect("workspace");
        let external = tempfile::tempdir().expect("external");
        let note = external.path().join("note.md");
        let policy = policy("current_file", "markdown", "workspace_absolute");

        assert!(policy.target_directory(workspace.path(), &note).is_err());
    }

    #[test]
    fn attachment_reference_encodes_unicode_and_spaces_for_markdown() {
        let root = Path::new("/vault");
        let reference = policy("current_file", "markdown", "relative")
            .reference_for(
                root,
                Path::new("/vault/notes/note.md"),
                Path::new("/vault/assets/示例 image.png"),
                true,
                None,
            )
            .expect("reference");

        assert_eq!(reference, "![](../assets/%E7%A4%BA%E4%BE%8B%20image.png)");
    }

    #[test]
    fn attachment_reference_uses_readable_wiki_paths() {
        let reference = policy("workspace_root", "wikilink", "workspace_absolute")
            .reference_for(
                Path::new("/vault"),
                Path::new("/vault/notes/note.md"),
                Path::new("/vault/assets/示例 image.png"),
                true,
                None,
            )
            .expect("reference");

        assert_eq!(reference, "![[assets/示例 image.png]]");
    }

    #[test]
    fn workspace_index_resolves_encoded_and_shortest_unique_paths() {
        let temp = tempfile::tempdir().expect("tempdir");
        let assets = temp.path().join("assets");
        let notes = temp.path().join("notes");
        std::fs::create_dir_all(&assets).expect("assets");
        std::fs::create_dir_all(&notes).expect("notes");
        let image = assets.join("示例 image.png");
        std::fs::write(&image, b"image").expect("image");
        let note = notes.join("note.md");
        std::fs::write(&note, b"note").expect("note");
        let index = WorkspaceLinkIndex::build(temp.path());

        assert_eq!(
            index.resolve(Some(&note), "assets/%E7%A4%BA%E4%BE%8B%20image.png"),
            Some(image.clone())
        );
        assert_eq!(
            index.shortest_unique_path(&image),
            Some(PathBuf::from("示例 image.png"))
        );
    }

    #[test]
    fn rewrite_move_preserves_markdown_title_and_encodes_new_target() {
        let temp = tempfile::tempdir().expect("tempdir");
        let notes = temp.path().join("notes");
        let assets = temp.path().join("assets");
        std::fs::create_dir_all(&notes).expect("notes");
        std::fs::create_dir_all(&assets).expect("assets");
        let note = notes.join("note.md");
        let old_image = assets.join("old image.png");
        let new_image = assets.join("新 image.png");
        std::fs::write(&note, b"note").expect("note");
        std::fs::write(&old_image, b"image").expect("image");
        let index = WorkspaceLinkIndex::build(temp.path());
        let mappings = [PathMapping::new(old_image, new_image)];

        let rewritten = rewrite_local_links_for_move(
            "![alt](../assets/old%20image.png \"Preview\")",
            temp.path(),
            &note,
            &note,
            &mappings,
            &index,
            &policy("current_file", "markdown", "relative"),
        );

        assert_eq!(
            rewritten,
            "![alt](../assets/%E6%96%B0%20image.png \"Preview\")"
        );
    }

    #[test]
    fn rewrite_directory_move_preserves_wiki_fragment_and_alias() {
        let temp = tempfile::tempdir().expect("tempdir");
        let notes = temp.path().join("notes");
        let assets = temp.path().join("assets");
        std::fs::create_dir_all(&notes).expect("notes");
        std::fs::create_dir_all(&assets).expect("assets");
        let note = notes.join("note.md");
        std::fs::write(&note, b"note").expect("note");
        std::fs::write(assets.join("old.png"), b"image").expect("image");
        let index = WorkspaceLinkIndex::build(temp.path());
        let mappings = [PathMapping::new(assets.clone(), temp.path().join("media"))];

        let rewritten = rewrite_local_links_for_move(
            "![[assets/old.png#preview|320]]",
            temp.path(),
            &note,
            &note,
            &mappings,
            &index,
            &policy("workspace_root", "wikilink", "workspace_absolute"),
        );

        assert_eq!(rewritten, "![[media/old.png#preview|320]]");
    }

    #[test]
    fn rewrite_source_move_recalculates_relative_links_and_skips_remote_urls() {
        let temp = tempfile::tempdir().expect("tempdir");
        let notes = temp.path().join("notes");
        let assets = temp.path().join("assets");
        std::fs::create_dir_all(&notes).expect("notes");
        std::fs::create_dir_all(&assets).expect("assets");
        let old_note = notes.join("note.md");
        let new_note = temp.path().join("archive/notes/note.md");
        std::fs::write(&old_note, b"note").expect("note");
        std::fs::write(assets.join("file.pdf"), b"pdf").expect("pdf");
        let index = WorkspaceLinkIndex::build(temp.path());
        let mappings = [PathMapping::new(old_note.clone(), new_note.clone())];

        let rewritten = rewrite_local_links_for_move(
            "[file](../assets/file.pdf) ![](https://example.com/image.png)",
            temp.path(),
            &old_note,
            &new_note,
            &mappings,
            &index,
            &policy("current_file", "markdown", "relative"),
        );

        assert_eq!(
            rewritten,
            "[file](../../assets/file.pdf) ![](https://example.com/image.png)"
        );
    }
}
