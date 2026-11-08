use std::collections::{HashMap, HashSet};
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
    ExternalFolder,
}

impl AttachmentLocation {
    fn from_setting(value: &str) -> Self {
        match value {
            "workspace_root" => Self::WorkspaceRoot,
            "workspace_folder" => Self::WorkspaceFolder,
            "current_file_subfolder" => Self::CurrentFileSubfolder,
            "external_folder" => Self::ExternalFolder,
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
    FileUrl,
}

impl AttachmentPathStyle {
    fn from_setting(value: &str) -> Self {
        match value {
            "workspace_absolute" => Self::WorkspaceAbsolute,
            "shortest" => Self::Shortest,
            "file_url" => Self::FileUrl,
            _ => Self::Relative,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AttachmentPolicy {
    pub location: AttachmentLocation,
    pub folder: String,
    pub folder_template: Option<String>,
    pub link_syntax: AttachmentLinkSyntax,
    pub path_style: AttachmentPathStyle,
    pub automatically_update_links: bool,
}

pub(crate) struct AttachmentPolicyAccess {
    policy: AttachmentPolicy,
    scoped_path: Option<PathBuf>,
}

impl AttachmentPolicyAccess {
    pub(crate) fn policy(&self) -> &AttachmentPolicy {
        &self.policy
    }
}

impl Drop for AttachmentPolicyAccess {
    fn drop(&mut self) {
        if let Some(path) = self.scoped_path.as_deref() {
            crate::platform::security_scope::stop_access(path);
        }
    }
}

impl AttachmentPolicy {
    pub(crate) fn from_settings(
        location: &str,
        folder: &str,
        external_folder: &str,
        link_syntax: &str,
        path_style: &str,
        automatically_update_links: bool,
    ) -> Self {
        let location = AttachmentLocation::from_setting(location);
        Self {
            location,
            folder: if location == AttachmentLocation::ExternalFolder {
                external_folder.to_string()
            } else {
                folder.to_string()
            },
            folder_template: None,
            link_syntax: if location == AttachmentLocation::ExternalFolder {
                AttachmentLinkSyntax::Markdown
            } else {
                AttachmentLinkSyntax::from_setting(link_syntax)
            },
            path_style: if location == AttachmentLocation::ExternalFolder {
                AttachmentPathStyle::FileUrl
            } else {
                AttachmentPathStyle::from_setting(path_style)
            },
            automatically_update_links,
        }
    }

    pub(crate) fn access(mut self, bookmark: &str) -> AttachmentPolicyAccess {
        let scoped_path = (self.location == AttachmentLocation::ExternalFolder)
            .then(|| crate::platform::security_scope::start_access(bookmark))
            .flatten();
        if let Some(path) = scoped_path.as_ref() {
            self.folder = path.to_string_lossy().into_owned();
        }
        AttachmentPolicyAccess {
            policy: self,
            scoped_path,
        }
    }

    pub(crate) fn target_directory(
        &self,
        workspace_root: &Path,
        current_file: &Path,
    ) -> Result<PathBuf> {
        let target = self.preview_directory(workspace_root, current_file)?;
        if self.location == AttachmentLocation::ExternalFolder {
            let metadata = std::fs::metadata(&self.folder)
                .with_context(|| crate::i18n::runtime::rt!(InspectPath, self.folder))?;
            if !metadata.is_dir() {
                bail!(crate::i18n::runtime::RuntimeText::NotDirectory.to_string());
            }
        }
        self.validate_directory_boundary(workspace_root, current_file, &target)?;
        Ok(target)
    }

    pub(crate) fn preview_directory(
        &self,
        workspace_root: &Path,
        current_file: &Path,
    ) -> Result<PathBuf> {
        let base = self.base_directory(workspace_root, current_file)?;
        let folder = if let Some(template) = self.folder_template.as_deref() {
            expand_attachment_folder_template(template, current_file)?
        } else if matches!(
            self.location,
            AttachmentLocation::WorkspaceFolder | AttachmentLocation::CurrentFileSubfolder
        ) {
            validate_relative_folder(&self.folder)?
        } else {
            PathBuf::new()
        };
        Ok(base.join(folder))
    }

    fn base_directory(&self, workspace_root: &Path, current_file: &Path) -> Result<PathBuf> {
        if self.location != AttachmentLocation::ExternalFolder
            && self.path_style != AttachmentPathStyle::Relative
        {
            ensure_file_in_workspace(workspace_root, current_file)?;
        }
        let current_dir = current_file
            .parent()
            .ok_or_else(|| anyhow!(crate::i18n::runtime::RuntimeText::ParentMissing.to_string()))?;
        Ok(match self.location {
            AttachmentLocation::CurrentFile | AttachmentLocation::CurrentFileSubfolder => {
                if self.location == AttachmentLocation::CurrentFileSubfolder
                    && self.folder_template.is_none()
                {
                    ensure_file_in_workspace(workspace_root, current_file)?;
                }
                current_dir.to_path_buf()
            }
            AttachmentLocation::WorkspaceRoot | AttachmentLocation::WorkspaceFolder => {
                ensure_file_in_workspace(workspace_root, current_file)?;
                workspace_root.to_path_buf()
            }
            AttachmentLocation::ExternalFolder => {
                let target = PathBuf::from(&self.folder);
                if !target.is_absolute() {
                    bail!(crate::i18n::runtime::RuntimeText::AbsolutePathRequired.to_string());
                }
                target
            }
        })
    }

    pub(crate) fn validate_directory_boundary(
        &self,
        workspace_root: &Path,
        current_file: &Path,
        target: &Path,
    ) -> Result<()> {
        if self.folder_template.is_none() {
            return Ok(());
        }
        let base = self.base_directory(workspace_root, current_file)?;
        let canonical_base = std::fs::canonicalize(&base)
            .with_context(|| crate::i18n::runtime::rt!(InspectPath, base.display()))?;
        let relative = target
            .strip_prefix(&base)
            .context(crate::i18n::runtime::RuntimeText::AttachmentOutsideBase.to_string())?;
        let mut ancestor = base;
        for component in relative.components() {
            crate::platform::fs_task::check_deadline()?;
            ancestor.push(component);
            match std::fs::symlink_metadata(&ancestor) {
                Ok(_) => {
                    let resolved = std::fs::canonicalize(&ancestor).with_context(|| {
                        crate::i18n::runtime::rt!(InspectPath, ancestor.display())
                    })?;
                    if !resolved.starts_with(&canonical_base) {
                        bail!(crate::i18n::runtime::rt!(
                            AttachmentOutsideBase,
                            ancestor.display()
                        ));
                    }
                    if !std::fs::metadata(&resolved)
                        .with_context(|| {
                            crate::i18n::runtime::rt!(InspectPath, ancestor.display())
                        })?
                        .is_dir()
                    {
                        bail!(crate::i18n::runtime::rt!(NotDirectory, ancestor.display()));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(error) => {
                    return Err(error).with_context(|| {
                        crate::i18n::runtime::rt!(InspectPath, ancestor.display())
                    });
                }
            }
        }
        Ok(())
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

    pub(crate) fn target_for(
        &self,
        workspace_root: &Path,
        source_file: &Path,
        target_file: &Path,
        index: Option<&WorkspaceLinkIndex>,
    ) -> Result<String> {
        let target = match self.path_style {
            AttachmentPathStyle::Relative => {
                let source_dir = source_file.parent().ok_or_else(|| {
                    anyhow!(crate::i18n::runtime::RuntimeText::ParentMissing.to_string())
                })?;
                relative_path(source_dir, target_file)
                    .ok_or_else(|| anyhow!(crate::i18n::runtime::RuntimeText::InvalidPath))?
            }
            AttachmentPathStyle::WorkspaceAbsolute => target_file
                .strip_prefix(workspace_root)
                .context(crate::i18n::runtime::RuntimeText::OutsideWorkspace.to_string())?
                .to_path_buf(),
            AttachmentPathStyle::Shortest => index
                .and_then(|index| index.shortest_unique_path(target_file))
                .or_else(|| {
                    target_file
                        .strip_prefix(workspace_root)
                        .ok()
                        .map(Path::to_path_buf)
                })
                .ok_or_else(|| {
                    anyhow!(crate::i18n::runtime::RuntimeText::OutsideWorkspace.to_string())
                })?,
            AttachmentPathStyle::FileUrl => {
                return crate::platform::paths::path_to_file_url(target_file).ok_or_else(|| {
                    anyhow!(crate::i18n::runtime::RuntimeText::AbsolutePathRequired.to_string())
                });
            }
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
        .with_context(|| crate::i18n::runtime::rt!(WriteFile, path.display()))
}

impl WorkspaceLinkIndex {
    pub(crate) fn build(root: &Path) -> Self {
        let mut files = Vec::new();
        if let Err(error) =
            collect_workspace_files(root, &mut files, &mut HashSet::new(), &mut |error| {
                eprintln!("Index workspace links '{}': {error}", root.display());
                Ok(())
            })
        {
            eprintln!("Index workspace links '{}': {error}", root.display());
        }
        Self::from_files(root, files)
    }

    pub(crate) fn build_checked(root: &Path) -> std::io::Result<Self> {
        let mut files = Vec::new();
        collect_workspace_files(root, &mut files, &mut HashSet::new(), &mut Err)?;
        Ok(Self::from_files(root, files))
    }

    pub(crate) fn resolve_direct(root: &Path, source: &Path, target: &str) -> Option<PathBuf> {
        Self {
            root: root.to_path_buf(),
            ..Self::default()
        }
        .resolve(Some(source), target)
    }

    fn from_files(root: &Path, files: Vec<PathBuf>) -> Self {
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
        bail!(crate::i18n::runtime::RuntimeText::OutsideWorkspace)
    }
}

fn validate_relative_folder(folder: &str) -> Result<PathBuf> {
    let trimmed = folder.trim().trim_matches('/');
    if trimmed.is_empty() {
        bail!(crate::i18n::runtime::RuntimeText::FolderEmpty.to_string());
    }
    let path = Path::new(trimmed);
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(component) => normalized.push(component),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                bail!(crate::i18n::runtime::RuntimeText::RelativeFolderRequired.to_string())
            }
        }
    }
    if normalized.as_os_str().is_empty() {
        bail!(crate::i18n::runtime::RuntimeText::FolderEmpty.to_string());
    }
    Ok(normalized)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AttachmentTemplateError {
    Variable,
    Path,
    FileName,
    FolderName,
}

impl AttachmentTemplateError {
    pub(crate) fn label_key(self) -> &'static str {
        match self {
            Self::Variable => "template_error_variable",
            Self::Path => "template_error_path",
            Self::FileName => "template_error_file_name",
            Self::FolderName => "template_error_folder_name",
        }
    }
}

impl std::fmt::Display for AttachmentTemplateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Variable => "only ${fileName} and ${folderName} are supported; use $$ for a literal $",
            Self::Path => "attachment template must be a relative directory without '..' or invalid filename characters",
            Self::FileName => "the document name cannot be used as an attachment directory",
            Self::FolderName => "the document folder name cannot be used as an attachment directory",
        })
    }
}

impl std::error::Error for AttachmentTemplateError {}

fn valid_template_component(value: &str) -> bool {
    if value.is_empty()
        || matches!(value, "." | "..")
        || value.ends_with(['.', ' '])
        || value
            .chars()
            .any(|ch| ch.is_control() || r#"\/:*?"<>|"#.contains(ch))
    {
        return false;
    }
    let stem = value
        .split('.')
        .next()
        .unwrap_or(value)
        .to_ascii_uppercase();
    !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        && !["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|suffix| {
                matches!(
                    suffix,
                    "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
                )
            })
        })
}

fn expand_folder_template(
    template: &str,
    mut variable: impl FnMut(&str) -> Result<String, AttachmentTemplateError>,
) -> Result<PathBuf, AttachmentTemplateError> {
    let mut remaining = template.trim();
    let mut expanded = String::new();
    while let Some(index) = remaining.find('$') {
        expanded.push_str(&remaining[..index]);
        remaining = &remaining[index + 1..];
        if let Some(rest) = remaining.strip_prefix('$') {
            expanded.push('$');
            remaining = rest;
        } else if let Some(rest) = remaining.strip_prefix('{') {
            let end = rest.find('}').ok_or(AttachmentTemplateError::Variable)?;
            let name = &rest[..end];
            if !matches!(name, "fileName" | "folderName") {
                return Err(AttachmentTemplateError::Variable);
            }
            expanded.push_str(&variable(name)?);
            remaining = &rest[end + 1..];
        } else {
            expanded.push('$');
        }
    }
    expanded.push_str(remaining);
    if expanded.starts_with('/') {
        return Err(AttachmentTemplateError::Path);
    }
    let mut path = PathBuf::new();
    for component in expanded.split('/').filter(|part| !part.is_empty()) {
        if !valid_template_component(component) {
            return Err(AttachmentTemplateError::Path);
        }
        path.push(component);
    }
    Ok(path)
}

pub(crate) fn validate_attachment_folder_template(
    template: &str,
) -> Result<(), AttachmentTemplateError> {
    expand_folder_template(template, |_| Ok("example".to_string())).map(|_| ())
}

pub(crate) fn expand_attachment_folder_template(
    template: &str,
    current_file: &Path,
) -> Result<PathBuf, AttachmentTemplateError> {
    expand_folder_template(template, |variable| {
        let (name, error) = match variable {
            "fileName" => (current_file.file_stem(), AttachmentTemplateError::FileName),
            _ => (
                current_file.parent().and_then(Path::file_name),
                AttachmentTemplateError::FolderName,
            ),
        };
        name.and_then(|name| name.to_str())
            .filter(|name| valid_template_component(name))
            .map(str::to_string)
            .ok_or(error)
    })
}

fn collect_workspace_files(
    directory: &Path,
    files: &mut Vec<PathBuf>,
    ancestors: &mut HashSet<PathBuf>,
    on_error: &mut impl FnMut(std::io::Error) -> std::io::Result<()>,
) -> std::io::Result<()> {
    crate::platform::fs_task::check_deadline()?;
    let entries = match std::fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) => return on_error(error),
    };
    let canonical = match std::fs::canonicalize(directory) {
        Ok(path) => path,
        Err(error) => return on_error(error),
    };
    if !ancestors.insert(canonical.clone()) {
        return Ok(());
    }
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                on_error(error)?;
                continue;
            }
        };
        crate::platform::fs_task::check_deadline()?;
        let path = entry.path();
        let kind = match entry.file_type() {
            Ok(kind) => kind,
            Err(error) => {
                on_error(error)?;
                continue;
            }
        };
        let is_directory = kind.is_dir()
            || (kind.is_symlink()
                && match std::fs::metadata(&path) {
                    Ok(metadata) => metadata.is_dir(),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
                    Err(error) => {
                        on_error(error)?;
                        continue;
                    }
                });
        crate::platform::fs_task::check_deadline()?;
        if should_skip_path(&path, is_directory) {
            continue;
        }
        if is_directory {
            collect_workspace_files(&path, files, ancestors, on_error)?;
        } else {
            files.push(path);
        }
    }
    ancestors.remove(&canonical);
    Ok(())
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
        AttachmentPolicy::from_settings(location, "assets/images", "", syntax, style, true)
    }

    #[test]
    fn attachment_policy_defaults_match_existing_behavior() {
        let policy = AttachmentPolicy::from_settings("", "", "", "", "", true);

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
            "",
            "markdown",
            "relative",
            true,
        );

        assert!(policy.target_directory(temp.path(), &note).is_err());
    }

    #[test]
    fn attachment_templates_expand_document_names_without_reparsing_values() {
        let note = Path::new("notes/旅行/旅行 笔记.v2.md");
        for (template, expected) in [
            ("photo/${folderName}/${fileName}", "photo/旅行/旅行 笔记.v2"),
            ("${fileName}.assets", "旅行 笔记.v2.assets"),
            (
                "${folderName}-${fileName}-${folderName}",
                "旅行-旅行 笔记.v2-旅行",
            ),
            ("fixed/nested", "fixed/nested"),
            ("", ""),
            ("  ", ""),
            ("photo//${fileName}/", "photo/旅行 笔记.v2"),
            ("$${fileName}/$$/${fileName}", "${fileName}/$/旅行 笔记.v2"),
        ] {
            assert!(
                validate_attachment_folder_template(template).is_ok(),
                "{template}"
            );
            assert_eq!(
                expand_attachment_folder_template(template, note).unwrap(),
                Path::new(expected),
                "{template}",
            );
        }
        assert_eq!(
            expand_attachment_folder_template("${fileName}", Path::new("notes/${folderName}.md"))
                .unwrap(),
            Path::new("${folderName}"),
        );
        assert_eq!(
            expand_attachment_folder_template("${fileName}", Path::new("notes/README")).unwrap(),
            Path::new("README"),
        );
    }

    #[test]
    fn attachment_templates_reject_unknown_variables_and_nonportable_paths() {
        for template in [
            "${filename}",
            "${year}",
            "${}",
            "${fileName",
            "photo/${folderName",
        ] {
            assert_eq!(
                validate_attachment_folder_template(template),
                Err(AttachmentTemplateError::Variable),
                "{template}",
            );
        }
        for template in [
            "../photo",
            "photo/../other",
            "/absolute",
            "C:/photo",
            r"C:\photo",
            r"\\server\photo",
            "photo/.",
            "photo/CON",
            "photo/aux.png",
            "photo/LPT1",
            "photo/name.",
            "photo/name /child",
            "photo/na:me",
            "photo/a?b",
            "photo/a\nb",
            "photo/a\0b",
        ] {
            assert_eq!(
                validate_attachment_folder_template(template),
                Err(AttachmentTemplateError::Path),
                "{template:?}",
            );
        }
        assert_eq!(
            expand_attachment_folder_template("${folderName}", Path::new("note.md")),
            Err(AttachmentTemplateError::FolderName),
        );
        for note in ["notes/..md", "notes/CON.md", "notes/a:b.md"] {
            assert_eq!(
                expand_attachment_folder_template("${fileName}", Path::new(note)),
                Err(AttachmentTemplateError::FileName),
                "{note}",
            );
        }
        #[cfg(not(windows))]
        assert_eq!(
            expand_attachment_folder_template("${fileName}", Path::new(r"notes/a\b.md")),
            Err(AttachmentTemplateError::FileName),
        );
    }

    #[test]
    fn attachment_templates_apply_to_all_bases_and_allow_empty_subfolders() {
        let workspace = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let notes = workspace.path().join("notes");
        std::fs::create_dir(&notes).unwrap();
        let note = notes.join("document.md");
        for (location, base) in [
            ("current_file", notes.as_path()),
            ("current_file_subfolder", notes.as_path()),
            ("workspace_root", workspace.path()),
            ("workspace_folder", workspace.path()),
            ("external_folder", external.path()),
        ] {
            let mut policy = AttachmentPolicy::from_settings(
                location,
                "legacy",
                external.path().to_str().unwrap(),
                "markdown",
                "relative",
                false,
            );
            for template in ["photo/${folderName}/${fileName}", ""] {
                policy.folder_template = Some(template.to_string());
                let expected = if template.is_empty() {
                    base.to_path_buf()
                } else {
                    base.join("photo/notes/document")
                };
                assert_eq!(
                    policy.preview_directory(workspace.path(), &note).unwrap(),
                    expected,
                );
                assert_eq!(
                    policy.target_directory(workspace.path(), &note).unwrap(),
                    expected,
                );
                assert!(
                    !base.join("photo").exists(),
                    "resolution must not create folders"
                );
            }
        }
        let mut policy = policy("current_file", "markdown", "relative");
        policy.folder_template = Some("${fileName}.assets".into());
        assert_eq!(
            policy
                .target_directory(workspace.path(), &external.path().join("note.md"))
                .unwrap(),
            external.path().join("note.assets"),
        );
    }

    #[cfg(unix)]
    #[test]
    fn attachment_templates_reject_symlinks_outside_each_base_before_creation() {
        let workspace = tempfile::tempdir().unwrap();
        let documents = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        for (location, base, note) in [
            (
                "workspace_root",
                workspace.path(),
                workspace.path().join("note.md"),
            ),
            (
                "current_file",
                documents.path(),
                documents.path().join("note.md"),
            ),
            (
                "external_folder",
                external.path(),
                documents.path().join("note.md"),
            ),
        ] {
            std::os::unix::fs::symlink(outside.path(), base.join("photo")).unwrap();
            let mut policy = AttachmentPolicy::from_settings(
                location,
                "",
                external.path().to_str().unwrap(),
                "markdown",
                "relative",
                false,
            );
            policy.folder_template = Some("photo/${fileName}".into());
            let error = policy
                .target_directory(workspace.path(), &note)
                .unwrap_err();
            assert!(
                error.to_string().contains(
                    crate::i18n::runtime::RuntimeText::AttachmentOutsideBase
                        .text(crate::i18n::UiLanguage::English)
                ),
                "{error:#}"
            );
            assert!(!outside.path().join("note").exists());
        }
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
    fn external_attachment_directory_uses_file_url_references() {
        let workspace = tempfile::tempdir().expect("workspace");
        let documents = tempfile::tempdir().expect("documents");
        let external = tempfile::tempdir().expect("external");
        let note = documents.path().join("note.md");
        let image = external.path().join("示例 image.png");
        std::fs::write(&image, b"image").expect("image");
        let policy = AttachmentPolicy::from_settings(
            "external_folder",
            "attachments",
            external.path().to_str().unwrap(),
            "markdown",
            "relative",
            true,
        );

        assert_eq!(
            policy.target_directory(workspace.path(), &note).unwrap(),
            external.path()
        );
        assert_eq!(
            policy
                .reference_for(workspace.path(), &note, &image, true, None)
                .unwrap(),
            format!(
                "![]({})",
                crate::platform::paths::path_to_file_url(&image).unwrap()
            )
        );
        let wiki_setting = AttachmentPolicy::from_settings(
            "external_folder",
            "",
            external.path().to_str().unwrap(),
            "wikilink",
            "shortest",
            false,
        );
        assert_eq!(wiki_setting.link_syntax, AttachmentLinkSyntax::Markdown);
        assert_eq!(
            wiki_setting
                .reference_for(workspace.path(), &note, &image, true, None)
                .unwrap(),
            format!(
                "![]({})",
                crate::platform::paths::path_to_file_url(&image).unwrap()
            )
        );
    }

    #[test]
    fn external_attachment_directory_must_already_exist() {
        let workspace = tempfile::tempdir().expect("workspace");
        let missing = workspace.path().join("missing");
        let policy = AttachmentPolicy::from_settings(
            "external_folder",
            "attachments",
            missing.to_str().unwrap(),
            "markdown",
            "relative",
            true,
        );

        assert!(
            policy
                .target_directory(workspace.path(), &workspace.path().join("note.md"))
                .is_err()
        );
        assert!(!missing.exists());
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
    fn shortest_link_uses_filename_until_a_duplicate_needs_a_directory() {
        let temp = tempfile::tempdir().expect("tempdir");
        let assets = temp.path().join("assets");
        let other = temp.path().join("other");
        std::fs::create_dir_all(&assets).unwrap();
        std::fs::create_dir_all(&other).unwrap();
        let image = assets.join("image.png");
        std::fs::write(&image, b"image").unwrap();
        let source = temp.path().join("note.md");
        let link_policy = policy("workspace_folder", "wikilink", "shortest");
        let index = WorkspaceLinkIndex::build(temp.path());
        assert_eq!(
            link_policy
                .reference_for(temp.path(), &source, &image, true, Some(&index))
                .unwrap(),
            "![[image.png]]"
        );
        std::fs::write(other.join("image.png"), b"other").unwrap();
        let index = WorkspaceLinkIndex::build(temp.path());
        assert_eq!(
            link_policy
                .reference_for(temp.path(), &source, &image, true, Some(&index))
                .unwrap(),
            "![[assets/image.png]]"
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
