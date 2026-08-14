use std::ffi::OsString;
use std::path::{Path, PathBuf};

use gpui::{AppContext, Context, Window};

use super::*;

impl EditorView {
    fn reveal_sidebar_create_row(&mut self, parent: PathBuf, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            for _ in 0..6 {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;

                let should_continue = this
                    .update(cx, |editor, cx| {
                        let items = editor.sidebar_items();
                        if parent == editor.sidebar_root {
                            editor.pending_sidebar_scroll_top.set(None);
                            editor.pending_sidebar_scroll_anchor = None;
                            let offset = editor.sidebar_scroll_handle.offset();
                            if f32::from(offset.y).abs() >= 0.5 {
                                editor
                                    .sidebar_scroll_handle
                                    .set_offset(gpui::point(offset.x, gpui::px(0.0)));
                                cx.notify();
                            }
                            return editor.sidebar_edit.is_some();
                        }
                        let create_index = items
                            .iter()
                            .position(|item| item.is_dir && item.path == parent)
                            .map(|index| index + 1);
                        let Some(create_index) = create_index else {
                            return true;
                        };
                        if editor
                            .ensure_sidebar_item_minimally_visible(create_index, items.len() + 1)
                        {
                            cx.notify();
                        }
                        editor.sidebar_edit.is_some()
                    })
                    .unwrap_or(false);

                if !should_continue {
                    break;
                }
            }
        })
        .detach();
    }

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
            this.commit_sidebar_edit(cx);
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

    fn remove_copied_path(path: &Path) -> std::io::Result<()> {
        if path.is_dir() {
            std::fs::remove_dir_all(path)
        } else if path.exists() {
            std::fs::remove_file(path)
        } else {
            Ok(())
        }
    }

    fn move_path_with_fallback(
        src: &Path,
        dst: &Path,
        rename: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
        trash: impl FnOnce(&Path) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        let rename_error = match rename(src, dst) {
            Ok(()) => return Ok(()),
            Err(error) => error,
        };
        if !src.exists() || dst.exists() {
            return Err(rename_error.into());
        }

        if let Err(copy_error) = Self::copy_path_recursively(src, dst) {
            let cleanup_error = Self::remove_copied_path(dst).err();
            return Err(anyhow::anyhow!(
                "rename failed ({rename_error}); copy fallback failed ({copy_error}){}",
                cleanup_error
                    .map(|error| format!("; destination cleanup failed ({error})"))
                    .unwrap_or_default()
            ));
        }

        if let Err(trash_error) = trash(src) {
            let cleanup_error = Self::remove_copied_path(dst).err();
            return Err(anyhow::anyhow!(
                "rename failed ({rename_error}); copied source but could not move it to Trash ({trash_error}){}",
                cleanup_error
                    .map(|error| format!("; destination cleanup failed ({error})"))
                    .unwrap_or_default()
            ));
        }

        Ok(())
    }

    pub(super) fn move_path_compat(src: &Path, dst: &Path) -> anyhow::Result<()> {
        Self::move_path_with_fallback(
            src,
            dst,
            |from, to| std::fs::rename(from, to),
            crate::platform::trash::move_to_trash,
        )
    }

    pub(crate) fn delete_paths_to_trash(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let mut deleted_any = false;
        for path in paths {
            match crate::platform::trash::move_to_trash(path) {
                Ok(()) => deleted_any = true,
                Err(error) => {
                    let message = format!(
                        "{}: {error}",
                        self.t(crate::i18n::TextKey::FileOperationFailed)
                    );
                    self.notify_error(message, cx);
                }
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
        for pane_id in self.pane_ids() {
            self.with_pane(pane_id, |this| {
                this.tabs.retain(|tab| {
                    tab.path.as_ref().is_none_or(|candidate| {
                        !paths.iter().any(|path| candidate.starts_with(path))
                    })
                });
                this.active_tab_index_cache.set(None);
                if !this.tabs.is_empty() {
                    let active_still_exists = this
                        .active_tab_id
                        .as_ref()
                        .is_some_and(|id| this.tabs.iter().any(|tab| &tab.id == id));
                    if !active_still_exists {
                        this.activate_tab_by_index_without_persist(0, cx);
                    }
                }
            });
        }
        for pane_id in self.pane_ids() {
            self.remove_pane_if_empty(pane_id, cx);
        }
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
        let create_in_file_list = self.file_list_menu_origin;
        if create_in_file_list {
            self.file_list_pending_reveal_path = None;
            // The filename input row renders at the top of the FileList.
            self.pending_file_list_scroll_top.set(None);
            let offset = self.file_list_scroll_handle.offset();
            self.file_list_scroll_handle
                .set_offset(gpui::point(offset.x, gpui::px(0.0)));
        } else {
            let expanded_changed = self.expanded_dirs.insert(parent.clone());
            self.schedule_sidebar_directory_load(parent.clone(), cx);
            if expanded_changed {
                self.invalidate_sidebar_flattened_items();
            }
        }
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
            this.commit_sidebar_edit(cx);
        });
        window.focus(&focus_handle);
        self.sidebar_edit = Some(SidebarEditMode::CreateFile {
            parent: parent.clone(),
            editor,
        });
        self.sidebar_edit_in_file_list = create_in_file_list;
        self.file_list_menu_origin = false;
        self.sidebar_edit_blur_sub = Some(blur_sub);
        self.context_menu = None;
        if !create_in_file_list {
            self.reveal_sidebar_create_row(parent, cx);
        }
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
            this.commit_sidebar_edit(cx);
        });
        window.focus(&focus_handle);
        self.sidebar_edit = Some(SidebarEditMode::CreateDir {
            parent: parent.clone(),
            editor,
        });
        self.sidebar_edit_in_file_list = false;
        self.sidebar_edit_blur_sub = Some(blur_sub);
        self.context_menu = None;
        self.reveal_sidebar_create_row(parent, cx);
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
                let reveal_in_file_tree = matches!(&edit, SidebarEditMode::Rename { .. });
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
                    if let Err(error) = Self::move_path_compat(path, &new_path) {
                        let message = format!(
                            "{}: {error}",
                            self.t(crate::i18n::TextKey::FileOperationFailed)
                        );
                        self.notify_error(message, cx);
                    } else {
                        let starred_changed =
                            self.app_settings.rename_starred_prefix(path, &new_path);
                        if starred_changed {
                            self.app_settings.save();
                        }
                        self.invalidate_sidebar_items_cache();
                        self.rewrite_open_tab_paths_after_move(path, &new_path);
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
                        if reveal_in_file_tree {
                            self.reveal_path_in_sidebar(new_path, cx);
                        }
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
                    if let Err(error) = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&new_path)
                    {
                        let message = format!(
                            "{}: {error}",
                            self.t(crate::i18n::TextKey::FileOperationFailed)
                        );
                        self.notify_error(message, cx);
                    } else {
                        self.schedule_sidebar_directory_refresh(parent.clone(), cx);
                        self.quick_open_index_root = None;
                        self.quick_open_file_index.clear();
                        if self.file_list_effective_dir() == *parent {
                            self.file_list_dirty = true;
                        }
                        self.set_file_list_dir_for_path(&new_path, cx);
                        self.selected_sidebar_paths.clear();
                        self.selected_sidebar_paths.insert(new_path.clone());
                        self.sidebar_selection_anchor = Some(new_path.clone());
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
                        let message = format!(
                            "{}: {error}",
                            self.t(crate::i18n::TextKey::FileOperationFailed)
                        );
                        self.notify_error(message, cx);
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
        // `symlink_metadata` keeps a symlinked directory from being walked into.
        if std::fs::symlink_metadata(src)?.is_dir() {
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
        let mut source = std::fs::File::open(src)?;
        let mut destination = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dst)?;
        std::io::copy(&mut source, &mut destination)?;
        if let Ok(metadata) = source.metadata() {
            let _ = destination.set_permissions(metadata.permissions());
        }
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
                if let Err(error) = Self::move_path_compat(&src, &dst) {
                    let message = format!(
                        "{}: {error}",
                        self.t(crate::i18n::TextKey::FileOperationFailed)
                    );
                    self.notify_error(message, cx);
                } else {
                    pasted_paths.push(dst.clone());
                    starred_changed |= self.app_settings.rename_starred_prefix(&src, &dst);
                    self.rewrite_open_tab_paths_after_move(&src, &dst);

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
                let message = format!(
                    "{}: {error}",
                    self.t(crate::i18n::TextKey::FileOperationFailed)
                );
                self.notify_error(message, cx);
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
            if self.file_list_effective_dir() == target_dir {
                self.file_list_pending_reveal_path = Some(reveal_path.clone());
                self.file_list_dirty = true;
            }
            self.selected_sidebar_paths = pasted_paths.into_iter().collect();
            self.focused_sidebar_path = Some(reveal_path.clone());
            self.sidebar_selection_anchor = Some(reveal_path);
        }
        cx.notify();
    }

    pub(crate) fn reveal_in_file_manager(&self, path: &Path) {
        if let Err(error) = crate::platform::shell::reveal(path) {
            eprintln!("failed to reveal {}: {error}", path.display());
        }
    }

    pub(crate) fn reveal_paths_in_file_manager(&self, paths: &[PathBuf]) {
        for path in paths {
            self.reveal_in_file_manager(path);
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
        self.workspace_search_epoch = self.workspace_search_epoch.wrapping_add(1);
        self.workspace_search_pending_paths.clear();
        self.workspace_search_has_more = false;
        self.workspace_search_input_focused = false;
        self.workspace_search_render_limit = super::list_viewport::SEARCH_LIST_POLICY.initial_count;
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
        // This is intentional navigation to a different root: unlike a
        // refresh, old directory children must not remain visible.
        self.sidebar_directory_children.clear();
        self.sidebar_items_cache.clear();
        self.sidebar_items_dirty = true;
        self.invalidate_sidebar_items_cache();
        self.pending_sidebar_scroll_top.set(None);
        self.pending_sidebar_scroll_anchor = None;
        self.pending_file_list_scroll_top.set(None);
        self.sidebar_scroll_handle
            .set_offset(gpui::point(gpui::px(0.0), gpui::px(0.0)));
        self.file_list_scroll_handle
            .set_offset(gpui::point(gpui::px(0.0), gpui::px(0.0)));
        self.sidebar_git_statuses.clear();
        self.sidebar_git_statuses_task = None;
        self.invalidate_sidebar_git_statuses();
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

    #[test]
    fn recursive_copy_does_not_overwrite_existing_destination() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.md");
        let destination = temp.path().join("destination.md");
        fs::write(&source, "source").unwrap();
        fs::write(&destination, "existing").unwrap();

        assert!(EditorView::copy_path_recursively(&source, &destination).is_err());
        assert_eq!(fs::read_to_string(destination).unwrap(), "existing");
    }

    #[test]
    fn move_falls_back_to_copy_then_trash_when_rename_is_unsupported() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.md");
        let destination = temp.path().join("destination.md");
        fs::write(&source, "network content").unwrap();

        EditorView::move_path_with_fallback(
            &source,
            &destination,
            |_, _| {
                Err(std::io::Error::new(
                    std::io::ErrorKind::Unsupported,
                    "cross-volume rename unsupported",
                ))
            },
            |path| fs::remove_file(path).map_err(anyhow::Error::from),
        )
        .unwrap();

        assert!(!source.exists());
        assert_eq!(fs::read_to_string(destination).unwrap(), "network content");
    }

    #[test]
    fn move_fallback_keeps_source_and_removes_copy_when_trash_fails() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.md");
        let destination = temp.path().join("destination.md");
        fs::write(&source, "network content").unwrap();

        let result = EditorView::move_path_with_fallback(
            &source,
            &destination,
            |_, _| Err(std::io::Error::other("rename failed")),
            |_| Err(anyhow::anyhow!("Trash unavailable")),
        );

        assert!(result.is_err());
        assert_eq!(fs::read_to_string(source).unwrap(), "network content");
        assert!(!destination.exists());
    }

    #[test]
    fn directory_move_fallback_copies_tree_before_removing_source() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::write(source.join("root.md"), "root").unwrap();
        fs::write(source.join("nested/child.md"), "child").unwrap();

        EditorView::move_path_with_fallback(
            &source,
            &destination,
            |_, _| Err(std::io::Error::other("rename unsupported")),
            |path| fs::remove_dir_all(path).map_err(anyhow::Error::from),
        )
        .unwrap();

        assert!(!source.exists());
        assert_eq!(
            fs::read_to_string(destination.join("root.md")).unwrap(),
            "root"
        );
        assert_eq!(
            fs::read_to_string(destination.join("nested/child.md")).unwrap(),
            "child"
        );
    }
}
