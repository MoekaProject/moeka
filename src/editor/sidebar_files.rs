use std::ffi::OsString;
use std::path::{Path, PathBuf};

use gpui::{AppContext, Context, Window};

use crate::platform::fs_task::{self, check_deadline};

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
        check_deadline()?;
        match crate::platform::trash::remove_permanently(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }

    fn move_path_with_fallback(
        src: &Path,
        dst: &Path,
        rename: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
        trash: impl FnOnce(&Path) -> anyhow::Result<()>,
    ) -> anyhow::Result<()> {
        Self::move_path_with_fallback_and_check(src, dst, rename, trash, check_deadline)
    }

    fn move_path_with_fallback_and_check(
        src: &Path,
        dst: &Path,
        rename: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
        trash: impl FnOnce(&Path) -> anyhow::Result<()>,
        check: impl Fn() -> std::io::Result<()>,
    ) -> anyhow::Result<()> {
        check()?;
        let rename_error = match rename(src, dst) {
            Ok(()) => return check().map_err(anyhow::Error::from),
            Err(error) => error,
        };
        check()?;
        if matches!(
            rename_error.kind(),
            std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
        ) {
            return Err(rename_error.into());
        }
        let destination_missing = dst
            .symlink_metadata()
            .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound);
        if !src.exists() || !destination_missing {
            return Err(rename_error.into());
        }

        check()?;
        let mut destination_created = false;
        if let Err(copy_error) = Self::copy_path_recursively_with_creation(src, dst, || {
            destination_created = true;
        }) {
            check()?;
            if matches!(
                copy_error.kind(),
                std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
            ) {
                return Err(copy_error.into());
            }
            // A failed create_new/create_dir may belong to another operation.
            let cleanup_error = destination_created
                .then(|| Self::remove_copied_path(dst).err())
                .flatten();
            return Err(anyhow::Error::from(copy_error).context(format!(
                "rename failed ({rename_error}); copy fallback failed{}",
                cleanup_error
                    .map(|error| format!("; destination cleanup failed ({error})"))
                    .unwrap_or_default()
            )));
        }

        check().map_err(|error| {
            anyhow::Error::from(error).context(format!(
                "source removal was not started; destination retained for recovery: {}",
                dst.display()
            ))
        })?;
        if let Err(trash_error) = trash(src) {
            // Recursive network deletion or a native Trash operation may have
            // partially removed the source before failing. Keep the complete copy.
            return Err(trash_error.context(format!(
                "rename failed ({rename_error}); source removal failed; destination retained for recovery: {}",
                dst.display()
            )));
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
        let paths = paths.to_vec();
        self.context_menu = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let mut results = Vec::new();
            for path in paths {
                let operation_path = path.clone();
                let result = Self::run_sidebar_mutation("delete", path.clone(), move || {
                    check_deadline()?;
                    crate::platform::trash::move_to_trash(&operation_path)
                })
                .await;
                let uncertain = Self::sidebar_mutation_outcome_uncertain(&result);
                results.push((path, result));
                if uncertain {
                    break;
                }
            }
            this.update(cx, |editor, cx| editor.finish_delete_paths(results, cx))
                .ok();
        })
        .detach();
    }

    fn finish_delete_paths(
        &mut self,
        results: Vec<(PathBuf, anyhow::Result<()>)>,
        cx: &mut Context<Self>,
    ) {
        let mut deleted = Vec::new();
        for (path, result) in results {
            match result {
                Ok(()) => deleted.push(path),
                Err(error) => {
                    let message = format!(
                        "{} ({}): {error:#}",
                        self.t(crate::i18n::TextKey::FileOperationFailed),
                        path.display()
                    );
                    self.notify_error(message, cx);
                }
            }
        }

        if deleted.is_empty() {
            cx.notify();
            return;
        }

        let mut starred_changed = false;
        for path in &deleted {
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
                        !deleted.iter().any(|path| candidate.starts_with(path))
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
            .retain(|selected| !deleted.iter().any(|path| selected.starts_with(path)));

        if self
            .focused_sidebar_path
            .as_ref()
            .is_some_and(|focused| deleted.iter().any(|path| focused.starts_with(path)))
        {
            self.focused_sidebar_path = None;
        }
        if self
            .sidebar_selection_anchor
            .as_ref()
            .is_some_and(|anchor| deleted.iter().any(|path| anchor.starts_with(path)))
        {
            self.sidebar_selection_anchor = None;
        }

        if self.tabs.is_empty() {
            self.create_scratch_tab(cx);
            return;
        }

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
        let json = WhiteboardDocument::default().to_json_string();
        cx.spawn(async move |this, cx| {
            let error_path = if parent.as_os_str().is_empty() {
                PathBuf::from(".")
            } else {
                parent.clone()
            };
            let result = fs_task::run("create whiteboard", error_path.clone(), move || {
                let parent = if parent.as_os_str().is_empty() {
                    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
                } else {
                    parent
                };
                let mut name = "Whiteboard.moeka".to_string();
                let mut index = 1;
                while !Self::sidebar_destination_missing(&parent.join(&name))? {
                    index += 1;
                    name = format!("Whiteboard {index}.moeka");
                }
                let path = parent.join(name);
                file_ops::atomic_write(&path, json.as_bytes()).map(|()| path)
            })
            .await;
            this.update(cx, |editor, cx| {
                let path = match result {
                    Ok(path) => path,
                    Err(error) => {
                        let message = format!(
                            "{} ({}): {error}",
                            editor.t(crate::i18n::TextKey::FileOperationFailed),
                            error_path.display()
                        );
                        editor.notify_error(message, cx);
                        return;
                    }
                };
                if let Some(parent) = path.parent() {
                    editor.expanded_dirs.insert(parent.to_path_buf());
                }
                editor.invalidate_sidebar_items_cache();
                editor.open_path_in_permanent_tab(path, true, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
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
                    let old_path = path.clone();
                    cx.spawn(async move |this, cx| {
                        let move_old_path = old_path.clone();
                        let move_new_path = new_path.clone();
                        let result =
                            Self::run_sidebar_mutation("rename", old_path.clone(), move || {
                                Self::move_path_compat(&move_old_path, &move_new_path)
                            })
                            .await;
                        this.update(cx, |editor, cx| {
                            editor.finish_sidebar_rename(
                                old_path,
                                new_path,
                                reveal_in_file_tree,
                                rewrite_context,
                                result,
                                cx,
                            );
                        })
                        .ok();
                    })
                    .detach();
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
                    let parent = parent.clone();
                    cx.spawn(async move |this, cx| {
                        let create_path = new_path.clone();
                        let result = fs_task::run("create file", new_path.clone(), move || {
                            check_deadline()?;
                            std::fs::OpenOptions::new()
                                .write(true)
                                .create_new(true)
                                .open(create_path)
                                .map(drop)
                        })
                        .await;
                        this.update(cx, |editor, cx| {
                            editor.finish_create_file(parent, new_path, result, cx)
                        })
                        .ok();
                    })
                    .detach();
                }
            }
            SidebarEditMode::CreateDir { parent, .. } => {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    let new_path = parent.join(trimmed);
                    cx.spawn(async move |this, cx| {
                        let create_path = new_path.clone();
                        let result = fs_task::run("create folder", new_path.clone(), move || {
                            check_deadline()?;
                            std::fs::create_dir(create_path)
                        })
                        .await;
                        this.update(cx, |editor, cx| {
                            if let Err(error) = result {
                                let message = format!(
                                    "{} ({}): {error}",
                                    editor.t(crate::i18n::TextKey::FileOperationFailed),
                                    new_path.display()
                                );
                                editor.notify_error(message, cx);
                                return;
                            }
                            editor.expanded_dirs.insert(new_path.clone());
                            editor.invalidate_sidebar_items_cache();
                            editor.reveal_path_in_sidebar(new_path, cx);
                        })
                        .ok();
                    })
                    .detach();
                }
            }
        }
        cx.notify();
    }

    fn finish_create_file(
        &mut self,
        parent: PathBuf,
        new_path: PathBuf,
        result: std::io::Result<()>,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = result {
            let message = format!(
                "{} ({}): {error}",
                self.t(crate::i18n::TextKey::FileOperationFailed),
                new_path.display()
            );
            self.notify_error(message, cx);
            return;
        }
        self.schedule_sidebar_directory_refresh(parent.clone(), cx);
        self.quick_open_index_root = None;
        self.quick_open_file_index.clear();
        if self.file_list_effective_dir() == parent {
            self.file_list_dirty = true;
        }
        self.set_file_list_dir_for_path(&new_path, cx);
        self.selected_sidebar_paths.clear();
        self.selected_sidebar_paths.insert(new_path.clone());
        self.sidebar_selection_anchor = Some(new_path.clone());
        self.reveal_path_in_sidebar(new_path.clone(), cx);
        self.open_sidebar_file(new_path, cx);
    }

    fn finish_sidebar_rename(
        &mut self,
        old_path: PathBuf,
        new_path: PathBuf,
        reveal_in_file_tree: bool,
        rewrite_context: Option<attachments::WorkspaceRewriteContext>,
        result: anyhow::Result<()>,
        cx: &mut Context<Self>,
    ) {
        if let Err(error) = result {
            let message = format!(
                "{} ({} → {}): {error:#}",
                self.t(crate::i18n::TextKey::FileOperationFailed),
                old_path.display(),
                new_path.display()
            );
            self.notify_error(message, cx);
            return;
        }
        let starred_changed = self
            .app_settings
            .rename_starred_prefix(&old_path, &new_path);
        if starred_changed {
            self.app_settings.save();
        }
        self.invalidate_sidebar_items_cache();
        self.rewrite_open_tab_paths_after_move(&old_path, &new_path);
        if self.selected_sidebar_paths.remove(&old_path) {
            self.selected_sidebar_paths.insert(new_path.clone());
        }
        if let Some(current_path) = self.current_file_path.as_ref()
            && current_path.starts_with(&old_path)
        {
            let relative = current_path
                .strip_prefix(&old_path)
                .unwrap_or_else(|_| Path::new(""));
            self.current_file_path = Some(new_path.join(relative));
        }
        let mapping = attachments::PathMapping::new(old_path, new_path.clone());
        self.apply_workspace_link_rewrites(rewrite_context, &[mapping], cx);
        if reveal_in_file_tree {
            self.reveal_path_in_sidebar(new_path, cx);
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

    pub(super) async fn run_sidebar_mutation<T: Send + 'static>(
        operation: &'static str,
        path: PathBuf,
        work: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
    ) -> anyhow::Result<T> {
        // Keep inner I/O error kinds intact so batches stop on uncertain mutations.
        fs_task::run_value(operation, path, work).await?
    }

    pub(super) fn sidebar_mutation_outcome_uncertain<T>(result: &anyhow::Result<T>) -> bool {
        result.as_ref().is_err_and(|error| {
            error.downcast_ref::<std::io::Error>().is_some_and(|error| {
                matches!(
                    error.kind(),
                    std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                )
            })
        })
    }

    fn sidebar_destination_missing(path: &Path) -> std::io::Result<bool> {
        check_deadline()?;
        match std::fs::symlink_metadata(path) {
            Ok(_) => Ok(false),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(true),
            Err(error) => Err(error),
        }
    }

    fn next_available_paste_destination(
        src: &Path,
        target_dir: &Path,
    ) -> std::io::Result<Option<PathBuf>> {
        let Some(name) = src.file_name() else {
            return Ok(None);
        };
        let initial = target_dir.join(name);
        if Self::sidebar_destination_missing(&initial)? {
            return Ok(Some(initial));
        }

        check_deadline()?;
        if std::fs::metadata(src)?.is_file() {
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
                if Self::sidebar_destination_missing(&candidate_path)? {
                    return Ok(Some(candidate_path));
                }
            }
        }

        for suffix in 1.. {
            let mut candidate = OsString::from(name);
            candidate.push(format!("-{suffix}"));
            let candidate_path = target_dir.join(candidate);
            if Self::sidebar_destination_missing(&candidate_path)? {
                return Ok(Some(candidate_path));
            }
        }

        Ok(None)
    }

    fn copy_path_recursively(src: &Path, dst: &Path) -> std::io::Result<()> {
        Self::copy_path_recursively_with_creation(src, dst, || {})
    }

    fn copy_path_recursively_with_creation(
        src: &Path,
        dst: &Path,
        on_destination_created: impl FnOnce(),
    ) -> std::io::Result<()> {
        check_deadline()?;
        // `symlink_metadata` keeps a symlinked directory from being walked into.
        if std::fs::symlink_metadata(src)?.is_dir() {
            check_deadline()?;
            if let Some(parent) = dst.parent() {
                std::fs::create_dir_all(parent)?;
            }
            check_deadline()?;
            std::fs::create_dir(dst)?;
            on_destination_created();
            check_deadline()?;
            for entry in std::fs::read_dir(src)? {
                check_deadline()?;
                let entry = entry?;
                if entry.file_name().to_string_lossy().starts_with("._") {
                    continue;
                }
                let child_src = entry.path();
                let child_dst = dst.join(entry.file_name());
                Self::copy_path_recursively(&child_src, &child_dst)?;
            }
            return Ok(());
        }

        check_deadline()?;
        if let Some(parent) = dst.parent() {
            std::fs::create_dir_all(parent)?;
        }
        check_deadline()?;
        let mut source = std::fs::File::open(src)?;
        check_deadline()?;
        let mut destination = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(dst)?;
        on_destination_created();
        check_deadline()?;
        std::io::copy(&mut source, &mut destination)?;
        check_deadline()?;
        if let Ok(metadata) = source.metadata() {
            check_deadline()?;
            let _ = destination.set_permissions(metadata.permissions());
        }
        Ok(())
    }

    fn external_drop_destination(
        src: &Path,
        target_dir: &Path,
    ) -> std::io::Result<Option<PathBuf>> {
        let src = Self::normalize_sidebar_path(src);
        let target_dir = Self::normalize_sidebar_path(target_dir);
        if src.file_name().is_none() || target_dir.starts_with(&src) {
            return Ok(None);
        }
        Self::next_available_paste_destination(&src, &target_dir)
    }

    pub(super) fn sidebar_drop_would_copy(paths: &[PathBuf], target_dir: &Path) -> bool {
        let target_dir = Self::normalize_sidebar_path(target_dir);
        paths.iter().any(|src| {
            let src = Self::normalize_sidebar_path(src);
            src.file_name().is_some() && !target_dir.starts_with(&src)
        })
    }

    fn copy_external_path_into_dir(
        src: &Path,
        target_dir: &Path,
    ) -> std::io::Result<Option<PathBuf>> {
        let Some(destination) = Self::external_drop_destination(src, target_dir)? else {
            return Ok(None);
        };
        Self::copy_path_recursively(src, &destination)?;
        Ok(Some(destination))
    }

    pub(crate) fn copy_external_paths_into_dir(
        &mut self,
        paths: &[PathBuf],
        target_dir: PathBuf,
        cx: &mut Context<Self>,
    ) {
        let target_dir = Self::normalize_sidebar_path(&target_dir);
        let paths = paths.to_vec();
        self.context_menu = None;
        cx.notify();
        cx.spawn(async move |this, cx| {
            let mut results = Vec::new();
            for source in paths {
                let operation_source = source.clone();
                let operation_target = target_dir.clone();
                let result = fs_task::run("copy", source.clone(), move || {
                    Self::copy_external_path_into_dir(&operation_source, &operation_target)
                })
                .await;
                let uncertain = result.as_ref().is_err_and(|error| {
                    matches!(
                        error.kind(),
                        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock
                    )
                });
                results.push((source, result));
                if uncertain {
                    break;
                }
            }
            this.update(cx, |editor, cx| {
                editor.finish_copy_external_paths(target_dir, results, cx)
            })
            .ok();
        })
        .detach();
    }

    fn finish_copy_external_paths(
        &mut self,
        target_dir: PathBuf,
        results: Vec<(PathBuf, std::io::Result<Option<PathBuf>>)>,
        cx: &mut Context<Self>,
    ) {
        let mut copied_paths = Vec::new();
        for (source, result) in results {
            match result {
                Ok(Some(destination)) => copied_paths.push(destination),
                Ok(None) => {}
                Err(error) => {
                    let message = format!(
                        "{} ({} → {}): {error}",
                        self.t(crate::i18n::TextKey::FileOperationFailed),
                        source.display(),
                        target_dir.display()
                    );
                    self.notify_error(message, cx);
                }
            }
        }

        self.invalidate_sidebar_items_cache();
        if let Some(reveal_path) = copied_paths.first().cloned() {
            self.reveal_path_in_sidebar(reveal_path.clone(), cx);
            if self.file_list_effective_dir() == target_dir {
                self.file_list_pending_reveal_path = Some(reveal_path.clone());
                self.file_list_dirty = true;
            }
            self.selected_sidebar_paths = copied_paths.into_iter().collect();
            self.focused_sidebar_path = Some(reveal_path.clone());
            self.sidebar_selection_anchor = Some(reveal_path);
        }
        cx.notify();
    }

    pub(crate) fn paste_into_dir(&mut self, target_dir: PathBuf, cx: &mut Context<Self>) {
        let Some(clipboard) = self.file_clipboard.clone() else {
            return;
        };
        let target_dir = Self::normalize_sidebar_path(&target_dir);
        let rewrite_context = clipboard
            .is_cut
            .then(|| self.capture_workspace_rewrite_context())
            .flatten();
        self.context_menu = None;
        if clipboard.is_cut {
            self.file_clipboard = None;
        }
        cx.notify();
        cx.spawn(async move |this, cx| {
            let is_cut = clipboard.is_cut;
            let mut results = Vec::new();
            for source in clipboard.paths {
                let source = Self::normalize_sidebar_path(&source);
                let operation_source = source.clone();
                let operation_target = target_dir.clone();
                let outcome = Self::run_sidebar_mutation("paste", source.clone(), move || {
                    let Some(destination) = Self::next_available_paste_destination(
                        &operation_source,
                        &operation_target,
                    )?
                    else {
                        return Ok(None);
                    };
                    check_deadline()?;
                    if is_cut {
                        Self::move_path_compat(&operation_source, &destination)?;
                    } else {
                        Self::copy_path_recursively(&operation_source, &destination)?;
                    }
                    Ok(Some(destination))
                })
                .await;
                let uncertain = Self::sidebar_mutation_outcome_uncertain(&outcome);
                match outcome {
                    Ok(Some(destination)) => results.push((source, destination, Ok(()))),
                    Ok(None) => {}
                    Err(error) => results.push((source, target_dir.clone(), Err(error))),
                }
                if uncertain {
                    break;
                }
            }
            this.update(cx, |editor, cx| {
                editor.finish_paste_into_dir(target_dir, is_cut, rewrite_context, results, cx)
            })
            .ok();
        })
        .detach();
    }

    fn finish_paste_into_dir(
        &mut self,
        target_dir: PathBuf,
        is_cut: bool,
        rewrite_context: Option<attachments::WorkspaceRewriteContext>,
        results: Vec<(PathBuf, PathBuf, anyhow::Result<()>)>,
        cx: &mut Context<Self>,
    ) {
        let mut pasted_paths = Vec::new();
        let mut starred_changed = false;
        let mut mappings = Vec::new();
        for (src, dst, result) in results {
            match result {
                Ok(()) if is_cut => {
                    pasted_paths.push(dst.clone());
                    starred_changed |= self.app_settings.rename_starred_prefix(&src, &dst);
                    self.rewrite_open_tab_paths_after_move(&src, &dst);
                    mappings.push(attachments::PathMapping::new(src.clone(), dst.clone()));

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
                Ok(()) => pasted_paths.push(dst),
                Err(error) => {
                    let message = format!(
                        "{} ({} → {}): {error:#}",
                        self.t(crate::i18n::TextKey::FileOperationFailed),
                        src.display(),
                        dst.display()
                    );
                    self.notify_error(message, cx);
                }
            }
        }
        if is_cut {
            self.apply_workspace_link_rewrites(rewrite_context, &mappings, cx);
        }
        if starred_changed {
            self.app_settings.save();
        }
        self.invalidate_sidebar_items_cache();
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

    pub(crate) fn reveal_in_file_manager(&self, path: &Path, cx: &mut Context<Self>) {
        self.reveal_paths_in_file_manager(&[path.to_path_buf()], cx);
    }

    pub(crate) fn reveal_paths_in_file_manager(&self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let paths = paths.to_vec();
        cx.spawn(async move |this, cx| {
            for path in paths {
                if let Err(error) = crate::platform::shell::reveal_async(path.clone()).await {
                    let _ = this.update(cx, |this, cx| {
                        this.notify_error(
                            format!(
                                "{} — {}: {error}",
                                this.t(crate::i18n::TextKey::RevealInFinder),
                                path.display(),
                            ),
                            cx,
                        );
                    });
                }
            }
        })
        .detach();
    }

    pub(crate) fn pick_workspace_folder(&mut self, cx: &mut Context<Self>) {
        #[cfg(not(target_os = "macos"))]
        let title = self
            .t(crate::i18n::TextKey::ChooseWorkspaceFolder)
            .to_string();
        cx.spawn(async move |this, cx| {
            #[cfg(target_os = "macos")]
            let maybe_path = match this.update(cx, |_, cx| {
                cx.prompt_for_paths(gpui::PathPromptOptions {
                    files: false,
                    directories: true,
                    multiple: false,
                    prompt: None,
                })
            }) {
                Ok(receiver) => receiver
                    .await
                    .ok()
                    .and_then(Result::ok)
                    .flatten()
                    .and_then(|mut paths| paths.pop()),
                Err(_) => None,
            };
            #[cfg(not(target_os = "macos"))]
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
        fs_task::register_workspace_root(&path);
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
        self.sidebar_items_cache = Rc::new(Vec::new());
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
        crate::platform::security_scope::stop_access(path);
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
            self.start_watching(cx);
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
    use std::{
        fs,
        future::Future,
        sync::Arc,
        task::{Context, Poll, Wake, Waker},
    };

    struct TestWaker(std::thread::Thread);

    impl Wake for TestWaker {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }

        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    fn block_on<T>(future: impl Future<Output = T>) -> T {
        let waker = Waker::from(Arc::new(TestWaker(std::thread::current())));
        let mut cx = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        loop {
            if let Poll::Ready(result) = future.as_mut().poll(&mut cx) {
                return result;
            }
            std::thread::park_timeout(std::time::Duration::from_millis(10));
        }
    }

    #[test]
    fn runner_preserves_uncertain_mutation_error_kinds() {
        let temp = tempfile::tempdir().unwrap();
        for kind in [std::io::ErrorKind::TimedOut, std::io::ErrorKind::WouldBlock] {
            let result: anyhow::Result<()> = block_on(EditorView::run_sidebar_mutation(
                "test uncertain mutation",
                temp.path().join(format!("{kind:?}")),
                move || {
                    Err(anyhow::Error::from(std::io::Error::from(kind)).context("source removal"))
                },
            ));
            assert!(EditorView::sidebar_mutation_outcome_uncertain(&result));
            assert_eq!(
                result
                    .unwrap_err()
                    .downcast_ref::<std::io::Error>()
                    .unwrap()
                    .kind(),
                kind
            );
        }
    }

    #[test]
    fn external_drop_hover_only_checks_path_relationships() {
        let missing = std::path::PathBuf::from("/unavailable-moeka-hover-test/note.md");
        assert!(EditorView::sidebar_drop_would_copy(
            std::slice::from_ref(&missing),
            std::path::Path::new("/unavailable-moeka-hover-test/target"),
        ));
        assert!(!EditorView::sidebar_drop_would_copy(
            std::slice::from_ref(&missing),
            &missing.join("nested"),
        ));
    }

    #[test]
    fn runner_deadline_stops_late_rename_fallback_and_source_deletion() {
        let temp = tempfile::tempdir().unwrap();
        crate::platform::fs_task::register_workspace_root(temp.path());
        let source = temp.path().join("source.md");
        let destination = temp.path().join("destination.md");
        fs::write(&source, "keep").unwrap();
        let worker_source = source.clone();
        let worker_destination = destination.clone();
        let (release, wait) = std::sync::mpsc::channel();
        let (finished, done) = std::sync::mpsc::channel();
        let result = block_on(crate::platform::fs_task::run_with_timeout(
            "test expired move",
            source.clone(),
            std::time::Duration::from_millis(100),
            move || {
                let result = EditorView::move_path_with_fallback(
                    &worker_source,
                    &worker_destination,
                    |_, _| {
                        wait.recv_timeout(std::time::Duration::from_secs(2))
                            .unwrap();
                        Err(std::io::Error::other("late cross-volume rename failure"))
                    },
                    |_| panic!("late rename must not delete its source"),
                );
                finished
                    .send(EditorView::sidebar_mutation_outcome_uncertain(&result))
                    .unwrap();
                Ok(result)
            },
        ));
        assert_eq!(result.unwrap_err().kind(), std::io::ErrorKind::TimedOut);
        release.send(()).unwrap();
        assert!(
            done.recv_timeout(std::time::Duration::from_secs(2))
                .unwrap()
        );
        assert_eq!(fs::read_to_string(source).unwrap(), "keep");
        assert!(!destination.exists());
    }

    #[test]
    fn expired_rename_does_not_start_a_copy_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.md");
        let destination = temp.path().join("destination.md");
        fs::write(&source, "keep").unwrap();
        let expired = std::cell::Cell::new(false);
        let result = EditorView::move_path_with_fallback_and_check(
            &source,
            &destination,
            |_, _| {
                expired.set(true);
                Err(std::io::Error::other("rename failed after deadline"))
            },
            |_| panic!("expired move must not delete its source"),
            || {
                if expired.get() {
                    Err(std::io::ErrorKind::TimedOut.into())
                } else {
                    Ok(())
                }
            },
        );
        assert!(EditorView::sidebar_mutation_outcome_uncertain(&result));
        assert_eq!(fs::read_to_string(source).unwrap(), "keep");
        assert!(!destination.exists());
    }

    #[test]
    fn expired_copy_keeps_source_and_destination_without_deletion() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.md");
        let destination = temp.path().join("destination.md");
        fs::write(&source, "keep").unwrap();
        let result = EditorView::move_path_with_fallback_and_check(
            &source,
            &destination,
            |_, _| Err(std::io::Error::other("rename unsupported")),
            |_| panic!("expired copy must not delete its source"),
            || {
                if destination.exists() {
                    Err(std::io::ErrorKind::TimedOut.into())
                } else {
                    Ok(())
                }
            },
        );
        assert!(EditorView::sidebar_mutation_outcome_uncertain(&result));
        assert!(format!("{:#}", result.unwrap_err()).contains("retained for recovery"));
        assert_eq!(fs::read_to_string(source).unwrap(), "keep");
        assert_eq!(fs::read_to_string(destination).unwrap(), "keep");
    }

    #[test]
    fn timed_out_source_removal_remains_an_uncertain_mutation() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.md");
        let destination = temp.path().join("destination.md");
        fs::write(&source, "keep").unwrap();
        let result = EditorView::move_path_with_fallback(
            &source,
            &destination,
            |_, _| Err(std::io::Error::other("rename unsupported")),
            |_| Err(std::io::Error::from(std::io::ErrorKind::TimedOut).into()),
        );
        assert!(EditorView::sidebar_mutation_outcome_uncertain(&result));
        assert_eq!(fs::read_to_string(source).unwrap(), "keep");
        assert_eq!(fs::read_to_string(destination).unwrap(), "keep");
    }

    #[test]
    fn timed_out_rename_does_not_retry_through_copy() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.md");
        let destination = temp.path().join("destination.md");
        fs::write(&source, "keep").unwrap();
        let result = EditorView::move_path_with_fallback(
            &source,
            &destination,
            |_, _| Err(std::io::ErrorKind::TimedOut.into()),
            |_| panic!("uncertain rename must not delete its source"),
        );
        assert!(EditorView::sidebar_mutation_outcome_uncertain(&result));
        assert_eq!(fs::read_to_string(source).unwrap(), "keep");
        assert!(!destination.exists());
    }

    #[test]
    fn busy_rename_does_not_retry_through_copy() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.md");
        let destination = temp.path().join("destination.md");
        fs::write(&source, "keep").unwrap();
        let result = EditorView::move_path_with_fallback(
            &source,
            &destination,
            |_, _| Err(std::io::ErrorKind::WouldBlock.into()),
            |_| panic!("uncertain rename must not delete its source"),
        );
        assert!(EditorView::sidebar_mutation_outcome_uncertain(&result));
        assert_eq!(fs::read_to_string(source).unwrap(), "keep");
        assert!(!destination.exists());
    }

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

        let destination = EditorView::next_available_paste_destination(&source, &target_dir)
            .unwrap()
            .unwrap();

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

        let destination = EditorView::next_available_paste_destination(&source_dir, &target_dir)
            .unwrap()
            .unwrap();

        assert_eq!(
            destination.file_name().unwrap().to_string_lossy(),
            "notes-2"
        );
    }

    #[cfg(unix)]
    #[test]
    fn paste_destination_does_not_reuse_a_dangling_symlink() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source/note.md");
        let target_dir = temp.path().join("target");
        fs::create_dir_all(source.parent().unwrap()).unwrap();
        fs::create_dir(&target_dir).unwrap();
        fs::write(&source, "keep").unwrap();
        std::os::unix::fs::symlink(temp.path().join("missing"), target_dir.join("note.md"))
            .unwrap();
        let destination = EditorView::next_available_paste_destination(&source, &target_dir)
            .unwrap()
            .unwrap();
        assert_eq!(destination, target_dir.join("note-1.md"));
    }

    #[test]
    fn recursive_copy_preserves_directory_contents() {
        let temp = tempfile::tempdir().unwrap();
        let source_dir = temp.path().join("source").join("docs");
        let nested_dir = source_dir.join("nested");
        let destination_dir = temp.path().join("target").join("docs-copy");

        fs::create_dir_all(&nested_dir).unwrap();
        fs::write(source_dir.join("root.md"), "root").unwrap();
        fs::write(source_dir.join("._root.md"), "AppleDouble").unwrap();
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
        assert!(!destination_dir.join("._root.md").exists());
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
    fn external_drop_copies_files_and_directories_with_collision_suffixes() {
        let temp = tempfile::tempdir().unwrap();
        let source_root = temp.path().join("source");
        let source_dir = source_root.join("docs");
        let target_dir = temp.path().join("target");
        fs::create_dir_all(&source_dir).unwrap();
        fs::create_dir_all(&target_dir).unwrap();
        fs::write(source_root.join("note.md"), "note").unwrap();
        fs::write(source_dir.join("guide.md"), "guide").unwrap();
        fs::write(target_dir.join("note.md"), "existing").unwrap();

        let copied_file =
            EditorView::copy_external_path_into_dir(&source_root.join("note.md"), &target_dir)
                .unwrap()
                .unwrap();
        let copied_dir = EditorView::copy_external_path_into_dir(&source_dir, &target_dir)
            .unwrap()
            .unwrap();

        assert_eq!(copied_file.file_name().unwrap(), "note-1.md");
        assert_eq!(fs::read_to_string(copied_file).unwrap(), "note");
        assert_eq!(
            fs::read_to_string(copied_dir.join("guide.md")).unwrap(),
            "guide"
        );
        assert!(
            EditorView::copy_external_path_into_dir(&source_dir, &source_dir.join("nested"))
                .unwrap()
                .is_none()
        );
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
    fn move_fallback_keeps_source_and_recovery_copy_when_trash_fails() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.md");
        let destination = temp.path().join("destination.md");
        fs::write(&source, "network content").unwrap();

        let result = EditorView::move_path_with_fallback(
            &source,
            &destination,
            |_, _| Err(std::io::Error::other("rename failed")),
            |_| Err(anyhow::anyhow!("Permission denied while moving to Trash")),
        );

        assert!(result.is_err());
        assert_eq!(fs::read_to_string(source).unwrap(), "network content");
        assert_eq!(fs::read_to_string(destination).unwrap(), "network content");
        assert!(format!("{:#}", result.unwrap_err()).contains("retained for recovery"));
    }

    #[test]
    fn move_fallback_does_not_interpret_unavailable_messages_or_os_codes() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.md");
        let destination = temp.path().join("destination.md");
        fs::write(&source, "keep").unwrap();
        for error in [
            anyhow::anyhow!("trash unavailable: permission denied"),
            trash::Error::Os {
                code: 45,
                description: "unsupported".into(),
            }
            .into(),
            trash::Error::Unknown {
                description: "trashItemAtURL: volume doesn't have one".into(),
            }
            .into(),
        ] {
            let result = EditorView::move_path_with_fallback(
                &source,
                &destination,
                |_, _| Err(std::io::Error::other("rename failed")),
                |_| Err(error),
            );
            assert!(result.is_err());
            assert_eq!(fs::read_to_string(&source).unwrap(), "keep");
            assert_eq!(fs::read_to_string(&destination).unwrap(), "keep");
            fs::remove_file(&destination).unwrap();
        }
    }

    #[cfg(unix)]
    #[test]
    fn copy_cleanup_unlinks_dangling_and_directory_symlinks() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let target = temp.path().join("target");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep.md"), "keep").unwrap();
        for destination in [&target, &temp.path().join("missing")] {
            let link = temp.path().join("copy");
            symlink(destination, &link).unwrap();
            EditorView::remove_copied_path(&link).unwrap();
            assert!(fs::symlink_metadata(&link).is_err());
            assert_eq!(fs::read_to_string(target.join("keep.md")).unwrap(), "keep");
        }
    }

    #[cfg(unix)]
    #[test]
    fn move_fallback_keeps_an_existing_dangling_destination_link() {
        use std::os::unix::fs::symlink;
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.md");
        let destination = temp.path().join("destination.md");
        let target = temp.path().join("missing.md");
        fs::write(&source, "keep").unwrap();
        symlink(&target, &destination).unwrap();

        assert!(
            EditorView::move_path_with_fallback(
                &source,
                &destination,
                |_, _| Err(std::io::Error::other("rename failed")),
                |_| panic!("existing destination must stop the move"),
            )
            .is_err()
        );
        assert_eq!(fs::read_link(destination).unwrap(), target);
        assert_eq!(fs::read_to_string(source).unwrap(), "keep");
    }

    #[test]
    fn copy_failure_does_not_claim_an_existing_destination() {
        let temp = tempfile::tempdir().unwrap();
        for is_dir in [false, true] {
            let source = temp
                .path()
                .join(if is_dir { "source-dir" } else { "source.md" });
            let destination = temp.path().join(if is_dir {
                "existing-dir"
            } else {
                "existing.md"
            });
            if is_dir {
                fs::create_dir(&source).unwrap();
                fs::create_dir(&destination).unwrap();
            } else {
                fs::write(&source, "source").unwrap();
                fs::write(&destination, "existing").unwrap();
            }
            let mut created = false;
            assert!(
                EditorView::copy_path_recursively_with_creation(&source, &destination, || {
                    created = true;
                })
                .is_err()
            );
            assert!(!created);
            assert!(source.exists());
            assert!(destination.exists());
            if !is_dir {
                assert_eq!(fs::read_to_string(destination).unwrap(), "existing");
            }
        }
    }

    #[test]
    fn directory_move_fallback_keeps_source_when_removal_fails() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        fs::create_dir_all(source.join("nested")).unwrap();
        fs::write(source.join("nested/keep.md"), "keep").unwrap();

        assert!(
            EditorView::move_path_with_fallback(
                &source,
                &destination,
                |_, _| Err(std::io::Error::other("rename failed")),
                |_| Err(anyhow::anyhow!("unknown Trash failure")),
            )
            .is_err()
        );
        assert_eq!(
            fs::read_to_string(source.join("nested/keep.md")).unwrap(),
            "keep"
        );
        assert_eq!(
            fs::read_to_string(destination.join("nested/keep.md")).unwrap(),
            "keep"
        );
    }

    #[test]
    fn move_fallback_retains_complete_copy_after_partial_source_removal() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source");
        let destination = temp.path().join("destination");
        fs::create_dir(&source).unwrap();
        fs::write(source.join("removed.md"), "recover me").unwrap();
        fs::write(source.join("locked.md"), "keep").unwrap();

        let result = EditorView::move_path_with_fallback(
            &source,
            &destination,
            |_, _| Err(std::io::Error::other("rename failed")),
            |path| {
                fs::remove_file(path.join("removed.md"))?;
                Err(
                    std::io::Error::new(std::io::ErrorKind::PermissionDenied, "locked child")
                        .into(),
                )
            },
        );
        assert!(format!("{:#}", result.unwrap_err()).contains("retained for recovery"));
        assert!(!source.join("removed.md").exists());
        assert_eq!(
            fs::read_to_string(source.join("locked.md")).unwrap(),
            "keep"
        );
        assert_eq!(
            fs::read_to_string(destination.join("removed.md")).unwrap(),
            "recover me"
        );
        assert_eq!(
            fs::read_to_string(destination.join("locked.md")).unwrap(),
            "keep"
        );
    }

    #[test]
    fn move_fallback_accepts_platform_network_deletion() {
        let temp = tempfile::tempdir().unwrap();
        let source = temp.path().join("source.md");
        let destination = temp.path().join("destination.md");
        fs::write(&source, "network content").unwrap();

        EditorView::move_path_with_fallback(
            &source,
            &destination,
            |_, _| Err(std::io::Error::other("rename unsupported")),
            |path| crate::platform::trash::remove_permanently(path).map_err(anyhow::Error::from),
        )
        .unwrap();

        assert!(!source.exists());
        assert_eq!(fs::read_to_string(destination).unwrap(), "network content");
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
