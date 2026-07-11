use std::ffi::OsString;
use std::path::{Path, PathBuf};

use gpui::{AppContext, Context, Window};

use super::*;

impl EditorView {
    fn begin_sidebar_rename(
        &mut self,
        path: PathBuf,
        starred_surface: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let current_name = path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default();
        let styles = self.sidebar_edit_text_input_styles();
        let placeholder = self
            .t(crate::i18n::TextKey::RenameNamePlaceholder)
            .to_string();
        let editor = cx.new(move |cx| {
            let mut text_input = crate::text_input::TextInput::new(styles, &placeholder, cx);
            text_input.set_text(&current_name, cx);
            let dot_pos = current_name.rfind('.').unwrap_or(current_name.len());
            text_input.set_selection(0, dot_pos, cx);
            text_input
        });
        cx.subscribe(
            &editor,
            |this, _editor, event: &crate::text_input::TextInputEvent, cx| match event {
                crate::text_input::TextInputEvent::Submit => this.commit_sidebar_edit(cx),
                crate::text_input::TextInputEvent::Escape => this.cancel_sidebar_edit(cx),
                crate::text_input::TextInputEvent::Changed => {}
                _ => {}
            },
        )
        .detach();
        let focus_handle = editor.read(cx).focus_handle.clone();
        let blur_sub = cx.on_blur(&focus_handle, window, |this, _window, cx| {
            this.cancel_sidebar_edit(cx);
        });
        window.focus(&focus_handle);
        self.sidebar_edit = Some(if starred_surface {
            SidebarEditMode::RenameStarred { path, editor }
        } else {
            SidebarEditMode::Rename { path, editor }
        });
        self.sidebar_edit_in_file_list = !starred_surface && self.file_list_menu_origin;
        self.sidebar_edit_blur_sub = Some(blur_sub);
        self.context_menu = None;
        cx.notify();
    }

    fn move_path_to_trash(path: &Path) -> anyhow::Result<()> {
        #[cfg(target_os = "macos")]
        {
            use trash::TrashContext;
            use trash::macos::{DeleteMethod, TrashContextExtMacos};

            let mut trash_context = TrashContext::new();
            trash_context.set_delete_method(DeleteMethod::NsFileManager);
            trash_context.delete(path).map_err(anyhow::Error::from)
        }

        #[cfg(not(target_os = "macos"))]
        {
            trash::delete(path).map_err(anyhow::Error::from)
        }
    }

    pub(crate) fn delete_paths_to_trash(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let mut deleted_any = false;
        for path in paths {
            match Self::move_path_to_trash(path) {
                Ok(()) => deleted_any = true,
                Err(error) => eprintln!("Delete failed for {:?}: {error}", path),
            }
        }

        if !deleted_any {
            self.context_menu = None;
            cx.notify();
            return;
        }

        let mut starred_changed = false;
        for path in paths {
            starred_changed |= self.app_settings.remove_starred_prefix(path);
        }
        if starred_changed {
            self.app_settings.save();
        }

        self.invalidate_sidebar_items_cache();
        self.tabs.retain(|tab| {
            tab.path
                .as_ref()
                .is_none_or(|candidate| !paths.iter().any(|path| candidate.starts_with(path)))
        });
        self.selected_sidebar_paths
            .retain(|selected| !paths.iter().any(|path| selected.starts_with(path)));

        if self
            .focused_sidebar_path
            .as_ref()
            .is_some_and(|focused| paths.iter().any(|path| focused.starts_with(path)))
        {
            self.focused_sidebar_path = None;
        }
        if self
            .sidebar_selection_anchor
            .as_ref()
            .is_some_and(|anchor| paths.iter().any(|path| anchor.starts_with(path)))
        {
            self.sidebar_selection_anchor = None;
        }

        if self.tabs.is_empty() {
            self.create_scratch_tab(cx);
            return;
        }

        let active_still_exists = self
            .active_tab_id
            .as_ref()
            .is_some_and(|id| self.tabs.iter().any(|tab| &tab.id == id));
        if !active_still_exists {
            self.activate_tab_by_index(0, cx);
        }

        self.context_menu = None;
        cx.notify();
    }

    pub(crate) fn start_rename(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_sidebar_rename(path, false, window, cx);
    }

    pub(crate) fn start_starred_rename(
        &mut self,
        path: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_sidebar_rename(path, true, window, cx);
    }

    /// Create a new empty `.moeka` whiteboard in `parent`, then open it. Falls
    /// back to the current directory when no workspace folder is set.
    pub(crate) fn create_new_whiteboard(&mut self, parent: PathBuf, cx: &mut Context<Self>) {
        let parent = if parent.as_os_str().is_empty() {
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
        } else {
            parent
        };
        let json = WhiteboardDocument::default().to_json_string();
        let mut name = "Whiteboard.moeka".to_string();
        let mut index = 1;
        while parent.join(&name).exists() {
            index += 1;
            name = format!("Whiteboard {index}.moeka");
        }
        let path = parent.join(&name);
        if std::fs::write(&path, json).is_ok() {
            self.expanded_dirs.insert(parent);
            self.invalidate_sidebar_items_cache();
            self.open_path_in_permanent_tab(path, true, cx);
            cx.notify();
        }
    }

    pub(crate) fn start_create_file(
        &mut self,
        parent: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.expanded_dirs.insert(parent.clone());
        self.invalidate_sidebar_items_cache();
        let styles = self.sidebar_edit_text_input_styles();
        let placeholder = self
            .t(crate::i18n::TextKey::FileNamePlaceholder)
            .to_string();
        let editor = cx.new(move |cx| crate::text_input::TextInput::new(styles, &placeholder, cx));
        cx.subscribe(
            &editor,
            |this, _editor, event: &crate::text_input::TextInputEvent, cx| match event {
                crate::text_input::TextInputEvent::Submit => this.commit_sidebar_edit(cx),
                crate::text_input::TextInputEvent::Escape => this.cancel_sidebar_edit(cx),
                crate::text_input::TextInputEvent::Changed => {}
                _ => {}
            },
        )
        .detach();
        let focus_handle = editor.read(cx).focus_handle.clone();
        let blur_sub = cx.on_blur(&focus_handle, window, |this, _window, cx| {
            this.cancel_sidebar_edit(cx);
        });
        window.focus(&focus_handle);
        self.sidebar_edit = Some(SidebarEditMode::CreateFile { parent, editor });
        self.sidebar_edit_in_file_list = self.file_list_menu_origin;
        self.sidebar_edit_blur_sub = Some(blur_sub);
        self.context_menu = None;
        cx.notify();
    }

    pub(crate) fn start_create_dir(
        &mut self,
        parent: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.expanded_dirs.insert(parent.clone());
        self.invalidate_sidebar_items_cache();
        let styles = self.sidebar_edit_text_input_styles();
        let placeholder = self
            .t(crate::i18n::TextKey::FolderNamePlaceholder)
            .to_string();
        let editor = cx.new(move |cx| crate::text_input::TextInput::new(styles, &placeholder, cx));
        cx.subscribe(
            &editor,
            |this, _editor, event: &crate::text_input::TextInputEvent, cx| match event {
                crate::text_input::TextInputEvent::Submit => this.commit_sidebar_edit(cx),
                crate::text_input::TextInputEvent::Escape => this.cancel_sidebar_edit(cx),
                crate::text_input::TextInputEvent::Changed => {}
                _ => {}
            },
        )
        .detach();
        let focus_handle = editor.read(cx).focus_handle.clone();
        let blur_sub = cx.on_blur(&focus_handle, window, |this, _window, cx| {
            this.cancel_sidebar_edit(cx);
        });
        window.focus(&focus_handle);
        self.sidebar_edit = Some(SidebarEditMode::CreateDir { parent, editor });
        self.sidebar_edit_in_file_list = false;
        self.sidebar_edit_blur_sub = Some(blur_sub);
        self.context_menu = None;
        cx.notify();
    }

    pub(crate) fn commit_sidebar_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.sidebar_edit.take() else {
            return;
        };
        self.sidebar_edit_blur_sub = None;
        let value = edit.text(cx);
        match &edit {
            SidebarEditMode::Rename { path, .. } | SidebarEditMode::RenameStarred { path, .. } => {
                let trimmed = value.trim();
                if !trimmed.is_empty()
                    && trimmed
                        != path
                            .file_name()
                            .and_then(|name| name.to_str())
                            .unwrap_or("")
                {
                    let new_path = path.with_file_name(trimmed);
                    let rewrite_context = self.capture_workspace_rewrite_context();
                    if let Err(error) = std::fs::rename(path, &new_path) {
                        eprintln!("Rename failed: {error}");
                    } else {
                        let starred_changed =
                            self.app_settings.rename_starred_prefix(path, &new_path);
                        if starred_changed {
                            self.app_settings.save();
                        }
                        self.invalidate_sidebar_items_cache();
                        for tab in &mut self.tabs {
                            if let Some(tab_path) = tab.path.as_ref()
                                && tab_path.starts_with(path)
                            {
                                let relative = tab_path
                                    .strip_prefix(path)
                                    .unwrap_or_else(|_| Path::new(""));
                                let renamed_tab_path = new_path.join(relative);
                                tab.title = Self::title_for_path(&renamed_tab_path);
                                tab.path = Some(renamed_tab_path);
                            }
                        }
                        if self.selected_sidebar_paths.contains(path) {
                            self.selected_sidebar_paths.remove(path);
                            self.selected_sidebar_paths.insert(new_path.clone());
                        }
                        if let Some(current_path) = self.current_file_path.as_ref()
                            && current_path.starts_with(path)
                        {
                            let relative = current_path
                                .strip_prefix(path)
                                .unwrap_or_else(|_| Path::new(""));
                            self.current_file_path = Some(new_path.join(relative));
                        }
                        let mapping = attachments::PathMapping::new(path.clone(), new_path.clone());
                        self.apply_workspace_link_rewrites(rewrite_context, &[mapping], cx);
                    }
                }
            }
            SidebarEditMode::CreateFile { parent, .. } => {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    let file_name = if Path::new(trimmed)
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .is_some_and(|ext| !ext.is_empty())
                    {
                        trimmed.to_string()
                    } else {
                        format!("{trimmed}.md")
                    };
                    let new_path = parent.join(file_name);
                    if let Err(error) = std::fs::File::create(&new_path) {
                        eprintln!("Create file failed: {error}");
                    } else {
                        self.invalidate_sidebar_items_cache();
                        self.reveal_path_in_sidebar(new_path.clone(), cx);
                        self.open_sidebar_file(new_path, cx);
                        return;
                    }
                }
            }
            SidebarEditMode::CreateDir { parent, .. } => {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    let new_path = parent.join(trimmed);
                    if let Err(error) = std::fs::create_dir(&new_path) {
                        eprintln!("Create folder failed: {error}");
                    } else {
                        self.expanded_dirs.insert(new_path.clone());
                        self.invalidate_sidebar_items_cache();
                        self.reveal_path_in_sidebar(new_path, cx);
                    }
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn cancel_sidebar_edit(&mut self, cx: &mut Context<Self>) {
        if self.sidebar_edit.take().is_some() {
            self.sidebar_edit_blur_sub = None;
            cx.notify();
        }
    }

    pub(crate) fn delete_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.delete_paths_to_trash(&[path], cx);
    }

    fn next_available_paste_destination(src: &Path, target_dir: &Path) -> Option<PathBuf> {
        let name = src.file_name()?;
        let initial = target_dir.join(name);
        if !initial.exists() {
            return Some(initial);
        }

        if src.is_file() {
            let stem = src.file_stem().unwrap_or(name);
            let extension = src.extension().filter(|ext| !ext.is_empty());
            for suffix in 1.. {
                let mut candidate = OsString::from(stem);
                candidate.push(format!("-{suffix}"));
                if let Some(extension) = extension {
                    candidate.push(".");
                    candidate.push(extension);
                }
                let candidate_path = target_dir.join(candidate);
                if !candidate_path.exists() {
                    return Some(candidate_path);
                }
            }
        }

        for suffix in 1.. {
            let mut candidate = OsString::from(name);
            candidate.push(format!("-{suffix}"));
            let candidate_path = target_dir.join(candidate);
            if !candidate_path.exists() {
                return Some(candidate_path);
            }
        }

        None
    }

    fn copy_path_recursively(src: &Path, dst: &Path) -> std::io::Result<()> {
        if src.is_dir() {
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::create_dir(dst)?;
            for entry in std::fs::read_dir(src)? {
                let entry = entry?;
                let child_src = entry.path();
                let child_dst = dst.join(entry.file_name());
                Self::copy_path_recursively(&child_src, &child_dst)?;
            }
            return Ok(());
        }

        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(src, dst)?;
        Ok(())
    }

    pub(crate) fn paste_into_dir(&mut self, target_dir: PathBuf, cx: &mut Context<Self>) {
        let Some(clipboard) = self.file_clipboard.clone() else {
            return;
        };
        let target_dir = Self::normalize_sidebar_path(&target_dir);
        let mut pasted_paths = Vec::new();
        let mut starred_changed = false;
        for src in &clipboard.paths {
            let src = Self::normalize_sidebar_path(src);
            let Some(dst) = Self::next_available_paste_destination(&src, &target_dir) else {
                continue;
            };
            if clipboard.is_cut {
                if let Err(error) = std::fs::rename(&src, &dst) {
                    eprintln!("Move {:?} -> {:?} failed: {error}", src, dst);
                } else {
                    pasted_paths.push(dst.clone());
                    starred_changed |= self.app_settings.rename_starred_prefix(&src, &dst);
                    for tab in &mut self.tabs {
                        if let Some(ref tab_path) = tab.path {
                            let normalized_tab_path = Self::normalize_sidebar_path(tab_path);
                            if normalized_tab_path.starts_with(&src) {
                                let relative = normalized_tab_path
                                    .strip_prefix(&src)
                                    .unwrap_or(normalized_tab_path.as_path());
                                let new_tab_path = dst.join(relative);
                                tab.title = Self::title_for_path(&new_tab_path);
                                tab.path = Some(new_tab_path);
                            }
                        }
                    }

                    let old_selected: Vec<PathBuf> = self
                        .selected_sidebar_paths
                        .iter()
                        .filter(|path| Self::normalize_sidebar_path(path).starts_with(&src))
                        .cloned()
                        .collect();
                    for old in old_selected {
                        self.selected_sidebar_paths.remove(&old);
                        let normalized_old = Self::normalize_sidebar_path(&old);
                        let relative = normalized_old
                            .strip_prefix(&src)
                            .unwrap_or(normalized_old.as_path());
                        self.selected_sidebar_paths.insert(dst.join(relative));
                    }

                    if self
                        .current_file_path
                        .as_ref()
                        .is_some_and(|path| Self::normalize_sidebar_path(path).starts_with(&src))
                        && let Some(ref path) = self.current_file_path
                    {
                        let normalized_path = Self::normalize_sidebar_path(path);
                        let relative = normalized_path
                            .strip_prefix(&src)
                            .unwrap_or(normalized_path.as_path());
                        self.current_file_path = Some(dst.join(relative));
                    }

                    if self
                        .sidebar_selection_anchor
                        .as_ref()
                        .is_some_and(|path| Self::normalize_sidebar_path(path).starts_with(&src))
                        && let Some(anchor) = self.sidebar_selection_anchor.as_ref()
                    {
                        let normalized_anchor = Self::normalize_sidebar_path(anchor);
                        let relative = normalized_anchor
                            .strip_prefix(&src)
                            .unwrap_or(normalized_anchor.as_path());
                        self.sidebar_selection_anchor = Some(dst.join(relative));
                    }
                }
            } else if let Err(error) = Self::copy_path_recursively(&src, &dst) {
                eprintln!("Copy {:?} -> {:?} failed: {error}", src, dst);
            } else {
                pasted_paths.push(dst);
            }
        }
        if clipboard.is_cut {
            self.file_clipboard = None;
        }
        if starred_changed {
            self.app_settings.save();
        }
        self.invalidate_sidebar_items_cache();
        self.context_menu = None;
        if let Some(reveal_path) = pasted_paths.first().cloned() {
            self.reveal_path_in_sidebar(reveal_path.clone(), cx);
            self.selected_sidebar_paths = pasted_paths.into_iter().collect();
            self.focused_sidebar_path = Some(reveal_path.clone());
            self.sidebar_selection_anchor = Some(reveal_path);
        }
        cx.notify();
    }

    pub(crate) fn reveal_in_finder(&self, path: &Path) {
        if let Err(error) = std::process::Command::new("open")
            .arg("--reveal")
            .arg(path)
            .spawn()
        {
            eprintln!("failed to reveal in Finder: {error}");
        }
    }

    pub(crate) fn reveal_paths_in_finder(&self, paths: &[PathBuf]) {
        for path in paths {
            self.reveal_in_finder(path);
        }
    }

    pub(crate) fn pick_workspace_folder(&mut self, cx: &mut Context<Self>) {
        let title = self
            .t(crate::i18n::TextKey::ChooseWorkspaceFolder)
            .to_string();
        cx.spawn(async move |this, cx| {
            let maybe_path = cx
                .background_executor()
                .spawn(async move { helpers::pick_folder_blocking(&title) })
                .await;
            if let Some(path) = maybe_path {
                this.update(cx, |editor, cx| {
                    if !editor.workspaces.contains(&path) {
                        editor.workspaces.push(path.clone());
                    }
                    editor.switch_workspace(path, cx);
                    session_store::persist_session(editor);
                })
                .ok();
            }
        })
        .detach();
    }

    pub(crate) fn switch_workspace(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.sidebar_root = path.clone();
        self.file_list_dir = path.clone();
        self.file_list_loaded_dir = None;
        self.sidebar_visible = true;
        self.sidebar_resizing = false;
        self.workspace_search_visible = false;
        self.workspace_search_query.clear();
        self.workspace_search_results.clear();
        self.workspace_search_focus_sub = None;
        self.workspace_search_blur_sub = None;
        self.workspace_search_task = None;
        self.workspace_search_input_focused = false;
        self.workspace_search_render_limit = 100;
        self.workspace_search_selected_match = None;
        self.workspace_search_highlight_path = None;
        self.workspace_search_highlight_ranges.clear();
        self.workspace_search_highlight_current_index = None;
        if let Some(editor) = &self.workspace_search_editor {
            editor.update(cx, |input, cx| {
                input.set_text("", cx);
            });
        }
        self.expanded_dirs.clear();
        self.expanded_dirs.insert(path);
        self.selected_sidebar_paths.clear();
        self.focused_sidebar_path = None;
        self.sidebar_selection_anchor = None;
        self.sidebar_error = None;
        self.invalidate_sidebar_items_cache();
        self.sidebar_git_statuses.clear();
        self.sidebar_git_statuses_task = None;
        self.invalidate_sidebar_git_statuses();
        self.reset_sidebar_render_limit();
        self.context_menu = None;
        self.start_watching(cx);
        self.sync_sidebar_width_animation(cx);
        cx.notify();
        session_store::persist_session(self);
    }

    pub(crate) fn close_workspace(&mut self, path: &Path, cx: &mut Context<Self>) {
        let closing_idx = self
            .workspaces
            .iter()
            .position(|workspace| workspace == path);
        self.workspaces.retain(|workspace| workspace != path);
        if self.workspaces.is_empty() {
            self.sidebar_root = PathBuf::new();
            self.sidebar_visible = false;
            self.sidebar_resizing = false;
            self.expanded_dirs.clear();
            self.selected_sidebar_paths.clear();
            self.focused_sidebar_path = None;
            self.sidebar_selection_anchor = None;
            self.sidebar_error = None;
            self.invalidate_sidebar_items_cache();
            self.sidebar_git_statuses.clear();
            self.sidebar_git_statuses_dirty = false;
            self.sidebar_git_statuses_scanned_at = None;
            self.sidebar_git_statuses_task = None;
            self.context_menu = None;
            self._watcher_task = None;
            // Collapse the panel to width 0; without this the sidebar keeps
            // rendering (empty) because `should_render_sidebar_panel` still sees
            // a non-zero display width.
            self.sync_sidebar_width_animation(cx);
            cx.notify();
            session_store::persist_session(self);
            return;
        }
        if self.sidebar_root == path {
            let next_idx = closing_idx
                .unwrap_or(0)
                .min(self.workspaces.len().saturating_sub(1));
            let next = self.workspaces[next_idx].clone();
            self.switch_workspace(next, cx);
        } else {
            self.context_menu = None;
            cx.notify();
        }
        session_store::persist_session(self);
    }
}

#[cfg(test)]
mod tests {
    use super::EditorView;
    use std::fs;

    #[test]
    fn paste_destination_adds_incrementing_suffix_before_extension() {
        let temp = tempfile::tempdir().unwrap();
        let target_dir = temp.path().join("target");
        let source_dir = temp.path().join("source");
        fs::create_dir_all(&target_dir).unwrap();
        fs::create_dir_all(&source_dir).unwrap();

        let source = source_dir.join("file.ext");
        fs::write(&source, "src").unwrap();
        fs::write(target_dir.join("file.ext"), "existing").unwrap();
        fs::write(target_dir.join("file-1.ext"), "existing").unwrap();

        let destination =
            EditorView::next_available_paste_destination(&source, &target_dir).unwrap();

        assert_eq!(
            destination.file_name().unwrap().to_string_lossy(),
            "file-2.ext"
        );
    }

    #[test]
    fn paste_destination_adds_incrementing_suffix_for_directories() {
        let temp = tempfile::tempdir().unwrap();
        let target_dir = temp.path().join("target");
        let source_dir = temp.path().join("source").join("notes");
        fs::create_dir_all(&target_dir).unwrap();
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir(target_dir.join("notes")).unwrap();
        fs::create_dir(target_dir.join("notes-1")).unwrap();

        let destination =
            EditorView::next_available_paste_destination(&source_dir, &target_dir).unwrap();

        assert_eq!(
            destination.file_name().unwrap().to_string_lossy(),
            "notes-2"
        );
    }

    #[test]
    fn recursive_copy_preserves_directory_contents() {
        let temp = tempfile::tempdir().unwrap();
        let source_dir = temp.path().join("source").join("docs");
        let nested_dir = source_dir.join("nested");
        let destination_dir = temp.path().join("target").join("docs-copy");

        fs::create_dir_all(&nested_dir).unwrap();
        fs::write(source_dir.join("root.md"), "root").unwrap();
        fs::write(nested_dir.join("child.md"), "child").unwrap();

        EditorView::copy_path_recursively(&source_dir, &destination_dir).unwrap();

        assert_eq!(
            fs::read_to_string(destination_dir.join("root.md")).unwrap(),
            "root"
        );
        assert_eq!(
            fs::read_to_string(destination_dir.join("nested").join("child.md")).unwrap(),
            "child"
        );
    }
}
