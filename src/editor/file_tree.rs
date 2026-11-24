use gpui::{ClickEvent, Context, MouseDownEvent, Window};
use moeka_fs::path::{FilesystemId, ResourcePath};

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ResourceMenuAction {
    NewFile,
    NewDirectory,
    Export,
    Star,
    Chat,
    Rename,
    Delete,
    Cut,
    Copy,
    Paste,
    CopyPath,
    CopyRelativePath,
    Collapse,
    Expand,
    Search,
    Reveal,
    CloseWorkspace,
    Refresh,
    Disconnect,
    Separator,
}

pub(super) fn resource_menu_actions(
    root: bool,
    directory: bool,
    markdown: bool,
    count: usize,
    paste: bool,
    remote: bool,
) -> Vec<ResourceMenuAction> {
    use ResourceMenuAction::*;
    let mut actions = vec![NewFile, NewDirectory, Separator];
    if root {
        if paste {
            actions.extend([Paste, Separator]);
        }
        actions.extend([
            Collapse,
            Expand,
            Separator,
            Search,
            CopyPath,
            CopyRelativePath,
        ]);
        actions.extend([Separator, Chat]);
    } else {
        if !directory {
            if markdown {
                actions.extend([Export, Separator]);
            }
            actions.extend([Star, Separator]);
        }
        actions.extend([Chat, Separator]);
        if count == 1 {
            actions.push(Rename);
        }
        actions.extend([Delete, Separator, Cut, Copy]);
        if paste {
            actions.push(Paste);
        }
        actions.extend([
            Separator,
            CopyPath,
            CopyRelativePath,
            Separator,
            Collapse,
            Expand,
        ]);
    }
    if !remote {
        actions.extend([Separator, Reveal]);
    }
    if root {
        actions.extend([Separator, CloseWorkspace]);
        if remote {
            actions.extend([Separator, Refresh, Disconnect]);
        }
    }
    actions
}

pub(super) fn resource_is_within(path: &ResourcePath, root: &ResourcePath) -> bool {
    let mut current = Some(path.clone());
    while let Some(path) = current {
        if &path == root {
            return true;
        }
        current = path.parent();
    }
    false
}

pub(super) fn filter_resource_roots(resources: Vec<ResourcePath>) -> Vec<ResourcePath> {
    resources
        .iter()
        .enumerate()
        .filter(|(index, resource)| {
            !resources.iter().enumerate().any(|(other_index, other)| {
                if *resource == other {
                    other_index < *index
                } else {
                    resource_is_within(resource, other)
                }
            })
        })
        .map(|(_, resource)| resource.clone())
        .collect()
}

pub(super) fn rebase_transferred_resource(
    resource: &ResourcePath,
    source: &ResourcePath,
    destination: &ResourcePath,
) -> Option<ResourcePath> {
    if !resource_is_within(resource, source) {
        return None;
    }
    let mut parts = Vec::new();
    let mut current = resource.clone();
    while &current != source {
        parts.push(current.path.file_name()?);
        current = current.parent()?;
    }
    let mut rebased = destination.clone();
    for part in parts.into_iter().rev() {
        rebased = rebased.join_native_child(&part).ok()?;
    }
    Some(rebased)
}

#[cfg(test)]
mod resource_menu_tests {
    use super::*;

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn file_list_shift_selection_uses_list_rows_when_tree_hides_files(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::AppContext as _;
        let editor = cx
            .update(|cx| cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some(""), false)));
        editor.update(cx, |editor, _| {
            editor.file_list_visible = true;
            editor.sidebar_items_cache = Rc::new(Vec::new());
            editor.sidebar_items_dirty = false;
            editor.file_list_items = ["first.md", "second.md", "third.md"]
                .into_iter()
                .map(|name| FileListItem {
                    path: PathBuf::from(name),
                    name: name.into(),
                    modified: std::time::UNIX_EPOCH,
                    created: std::time::UNIX_EPOCH,
                    details: None,
                })
                .collect();
            editor.select_file_list_resource(
                &ResourcePath::local(Path::new("first.md")),
                false,
                false,
            );
            editor.select_file_list_resource(
                &ResourcePath::local(Path::new("third.md")),
                true,
                false,
            );
            assert_eq!(
                editor.selected_sidebar_paths,
                HashSet::from([
                    PathBuf::from("first.md"),
                    PathBuf::from("second.md"),
                    PathBuf::from("third.md"),
                ])
            );
        });
    }

    #[test]
    fn local_and_remote_menu_order_only_differs_for_platform_actions() {
        use ResourceMenuAction::*;
        fn common(actions: Vec<ResourceMenuAction>) -> Vec<ResourceMenuAction> {
            actions
                .into_iter()
                .filter(|action| {
                    !matches!(
                        action,
                        ResourceMenuAction::Reveal
                            | ResourceMenuAction::Refresh
                            | ResourceMenuAction::Disconnect
                            | ResourceMenuAction::Separator
                    )
                })
                .collect()
        }
        for (root, directory, markdown, count, paste) in [
            (true, true, false, 1, true),
            (false, true, false, 2, true),
            (false, false, true, 1, false),
            (false, false, true, 3, true),
        ] {
            let local = resource_menu_actions(root, directory, markdown, count, paste, false);
            let remote = resource_menu_actions(root, directory, markdown, count, paste, true);
            assert_eq!(common(local), common(remote.clone()));
            assert_eq!(remote.contains(&Rename), !root && count == 1);
            assert_eq!(remote.contains(&Paste), paste);
            assert_eq!(remote.contains(&Export), !root && !directory && markdown);
            assert_eq!(remote.contains(&CloseWorkspace), root);
            assert!(
                remote.contains(&Chat) && remote.contains(&Collapse) && remote.contains(&Expand)
            );
            assert!(!root || !remote.contains(&Delete));
        }
        assert_eq!(
            common(resource_menu_actions(false, false, true, 2, true, true)),
            vec![
                NewFile,
                NewDirectory,
                Export,
                Star,
                Chat,
                Delete,
                Cut,
                Copy,
                Paste,
                CopyPath,
                CopyRelativePath,
                Collapse,
                Expand
            ],
        );
    }

    #[test]
    fn resource_selection_and_rebasing_use_identity_and_path_components() {
        let root = ResourcePath {
            filesystem: FilesystemId::Ssh([1; 32]),
            path: moeka_fs::path::NativePath::unix_absolute("/notes").unwrap(),
        };
        let child = root.join_child("日本語.md").unwrap();
        let sibling =
            root.with_path(moeka_fs::path::NativePath::unix_absolute("/notes-old/a.md").unwrap());
        let foreign = ResourcePath {
            filesystem: FilesystemId::Ssh([2; 32]),
            path: child.path.clone(),
        };
        assert_eq!(
            filter_resource_roots(vec![
                child.clone(),
                root.clone(),
                root.clone(),
                sibling.clone(),
                foreign.clone()
            ]),
            vec![root.clone(), sibling.clone(), foreign.clone()]
        );
        assert!(!resource_is_within(&sibling, &root));
        assert!(!resource_is_within(&foreign, &root));
        let directory = tempfile::tempdir().unwrap();
        let destination = ResourcePath::local(directory.path());
        assert_eq!(
            rebase_transferred_resource(&child, &root, &destination)
                .unwrap()
                .local_path()
                .unwrap(),
            directory.path().join("日本語.md")
        );
    }

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn dropping_resources_moves_or_copies_between_workspaces_without_using_the_clipboard(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::AppContext as _;
        let directory = tempfile::tempdir().unwrap();
        let (a, b) = (directory.path().join("a"), directory.path().join("b"));
        moeka_fs::local::native::create_dir_all(a.join("nested")).unwrap();
        moeka_fs::local::native::create_dir(&b).unwrap();
        moeka_fs::local::native::write(a.join("move.md"), "move").unwrap();
        moeka_fs::local::native::write(a.join("nested/copy.md"), "copy").unwrap();
        let editor = cx
            .update(|cx| cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some(""), false)));
        editor.update(cx, |editor, cx| {
            editor.workspaces = vec![a.clone(), b.clone()];
            editor.sidebar_root = a.clone();
            let kept = vec![ResourcePath::local(Path::new("kept.md"))];
            editor.set_resource_clipboard(kept.clone(), false);
            let target = ResourcePath::local(&b);
            editor.drop_resources_into(
                vec![ResourcePath::local(&a.join("move.md"))],
                target.clone(),
                false,
                cx,
            );
            editor.drop_resources_into(
                vec![ResourcePath::local(&a.join("nested/copy.md"))],
                target,
                true,
                cx,
            );
            assert_eq!(
                editor.resource_clipboard_contents().unwrap().resources,
                kept
            );
        });
        cx.run_until_parked();
        std::thread::sleep(std::time::Duration::from_millis(300));
        cx.run_until_parked();
        assert!(b.join("move.md").exists() && !a.join("move.md").exists());
        assert!(b.join("copy.md").exists() && a.join("nested/copy.md").exists());
    }

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn clipboard_and_batch_stars_share_resource_state(cx: &mut gpui::TestAppContext) {
        use gpui::AppContext as _;
        let editor = cx
            .update(|cx| cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some(""), false)));
        editor.update(cx, |editor, cx| {
            let local = ResourcePath::local(Path::new("local.md"));
            let remote = ResourcePath {
                filesystem: FilesystemId::Ssh([1; 32]),
                path: moeka_fs::path::NativePath::unix_absolute("/note.md").unwrap(),
            };
            editor.set_local_file_tree_clipboard(FileClipboard {
                paths: vec![PathBuf::from("local.md")],
                is_cut: false,
            });
            assert_eq!(
                editor.resource_clipboard_contents().unwrap().resources,
                vec![local.clone()]
            );
            editor.set_resource_clipboard(vec![remote.clone()], true);
            assert!(editor.file_clipboard.is_none());
            assert_eq!(
                editor.resource_clipboard_contents().unwrap().resources,
                vec![remote.clone()]
            );
            editor.set_resources_starred(&[local.clone(), remote.clone()], true, cx);
            assert!(editor.resource_is_starred(&local) && editor.resource_is_starred(&remote));
            editor.set_resources_starred(&[local.clone(), remote.clone()], false, cx);
            assert!(!editor.resource_is_starred(&local) && !editor.resource_is_starred(&remote));
            let remote_second = remote.with_path(moeka_fs::path::NativePath::unix_absolute("/second.md").unwrap());
            editor.confirm_resource_trash(vec![remote.clone(), remote_second], cx);
            let remote_dialog = editor.dialog.as_ref().unwrap();
            let remote_text = (remote_dialog.title.clone(), remote_dialog.message.clone(), remote_dialog.confirm_label.clone());
            assert!(matches!(&remote_dialog.action, DialogAction::DeleteResources { resources, .. } if resources.len() == 2));
            editor.confirm_resource_trash(vec![local, ResourcePath::local(Path::new("second.md"))], cx);
            let local_dialog = editor.dialog.as_ref().unwrap();
            assert_eq!(remote_text, (local_dialog.title.clone(), local_dialog.message.clone(), local_dialog.confirm_label.clone()));
        });
    }

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn moved_remote_document_keeps_its_live_draft_and_becomes_a_local_resource(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::AppContext as _;
        let editor = cx.update(|cx| {
            cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some("unsaved draft"), false))
        });
        let directory = tempfile::tempdir().unwrap();
        editor.update(cx, |editor, cx| {
            let root = ResourcePath {
                filesystem: FilesystemId::Ssh([1; 32]),
                path: moeka_fs::path::NativePath::unix_absolute("/notes").unwrap(),
            };
            let source = root.join_child("draft.md").unwrap();
            let target = ResourcePath::local(&directory.path().join("draft.md"));
            let id = editor.tabs[0].id.clone();
            editor.tabs[0].is_remote = true;
            editor.tabs[0].is_dirty = true;
            editor.tabs[0].remote = Some(config_store::RemoteTabState {
                peer_scope: None,
                destination: "example.invalid".into(),
                config_file: None,
                root,
                resource: Some(source.clone()),
                revision: None,
                save_uncertain: false,
                pending_resource: None,
                line_ending_crlf: false,
            });
            editor.remote_documents.insert(
                id.clone(),
                super::super::remote::RemoteDocument {
                    resource: Some(source.clone()),
                    revision: None,
                    save_uncertain: false,
                },
            );
            editor.retarget_transferred_resources(&source, &target, &HashMap::new(), cx);
            assert_eq!(editor.buffer.text(), "unsaved draft");
            assert_eq!(editor.active_tab_id.as_deref(), Some(id.as_str()));
            assert_eq!(
                editor.current_file_path,
                Some(directory.path().join("draft.md"))
            );
            assert!(editor.tabs[0].is_dirty);
            assert!(!editor.tabs[0].is_remote);
            assert!(editor.tabs[0].remote.is_none());
            assert!(!editor.remote_documents.contains_key(&id));
        });
    }
}

#[cfg(test)]
#[test]
fn additive_range_preserves_prior_selection_and_skips_hidden_items() {
    let mut selection = HashSet::from([0, 2]);
    let mut anchor = Some(2);
    select_tree_item(&mut selection, &mut anchor, &5, &[0, 2, 4, 5], true, true);
    assert_eq!(selection, HashSet::from([0, 2, 4, 5]));
    select_tree_item(&mut selection, &mut anchor, &4, &[0, 2, 4, 5], true, false);
    assert_eq!(selection, HashSet::from([2, 4]));
}

pub(super) fn select_tree_item<T: Clone + Eq + std::hash::Hash>(
    selected: &mut HashSet<T>,
    anchor: &mut Option<T>,
    item: &T,
    visible: &[T],
    shift: bool,
    additive: bool,
) {
    if !additive {
        selected.clear();
    }
    if shift {
        if let Some(start) = anchor
            .as_ref()
            .and_then(|key| visible.iter().position(|v| v == key))
            && let Some(end) = visible.iter().position(|v| v == item)
        {
            selected.extend(visible[start.min(end)..=start.max(end)].iter().cloned());
            return;
        }
        selected.insert(item.clone());
    } else if !additive || !selected.remove(item) {
        selected.insert(item.clone());
    }
    *anchor = Some(item.clone());
}

impl EditorView {
    pub(super) fn select_file_list_resource(
        &mut self,
        resource: &ResourcePath,
        shift: bool,
        secondary: bool,
    ) {
        if let Ok(path) = resource.local_path() {
            let visible = self
                .file_list_items
                .iter()
                .map(|item| item.path.clone())
                .collect::<Vec<_>>();
            select_tree_item(
                &mut self.selected_sidebar_paths,
                &mut self.sidebar_selection_anchor,
                &path,
                &visible,
                shift,
                secondary,
            );
            self.focused_sidebar_path = Some(path);
            self.focused_workspace_root = None;
            self.focused_starred_path = None;
        } else {
            self.select_remote_file_list_item(resource, shift, secondary);
        }
    }

    pub(super) fn select_starred_resource(
        &mut self,
        resource: &ResourcePath,
        shift: bool,
        secondary: bool,
    ) {
        if let Ok(path) = resource.local_path() {
            self.select_starred_sidebar_item(&path, shift, secondary);
            return;
        }
        let visible = self.visible_starred_resources();
        self.focused_workspace_root = None;
        if let Some(workspace) = &mut self.remote_workspace {
            select_tree_item(
                &mut workspace.selected_paths,
                &mut workspace.selection_anchor,
                resource,
                &visible,
                shift,
                secondary,
            );
            workspace.selected = Some(resource.clone());
            workspace.focused = Some(resource.clone());
        }
    }
    pub(super) fn resource_filesystem(
        &self,
        resource: &ResourcePath,
    ) -> std::io::Result<super::resource_transfer::ResourceFilesystem> {
        use super::resource_transfer::ResourceFilesystem;
        if resource.filesystem == FilesystemId::Local {
            return Ok(ResourceFilesystem::Local);
        }
        self.remote_workspace
            .iter()
            .chain(self.parked_remote_workspaces.iter())
            .find(|workspace| workspace.filesystem.identity() == &resource.filesystem)
            .map(|workspace| ResourceFilesystem::Ssh(workspace.filesystem.clone()))
            .ok_or_else(|| {
                std::io::Error::other(crate::i18n::remote_text(
                    self.ui_language,
                    "wrong_connection",
                ))
            })
    }

    pub(super) fn paste_resource_clipboard(
        &mut self,
        target: ResourcePath,
        cx: &mut Context<Self>,
    ) {
        let Some(clipboard) = self.resource_clipboard_contents() else {
            return;
        };
        self.paste_resources(clipboard, true, target, cx);
    }

    /// Moves (or with `copy`, copies) dragged resources into `target`, which may
    /// live in another workspace or on another filesystem. Never touches the
    /// user's clipboard.
    pub(super) fn drop_resources_into(
        &mut self,
        resources: Vec<ResourcePath>,
        target: ResourcePath,
        copy: bool,
        cx: &mut Context<Self>,
    ) {
        self.clear_sidebar_drag_state(cx);
        let resources = filter_resource_roots(resources)
            .into_iter()
            .filter(|resource| copy || resource.parent().as_ref() != Some(&target))
            .collect::<Vec<_>>();
        if resources.is_empty() {
            return;
        }
        if target.filesystem != FilesystemId::Local
            && !self.focus_remote_workspace_for_resource(&target, cx)
        {
            self.notify_error(
                crate::i18n::remote_text(self.ui_language, "not_connected"),
                cx,
            );
            return;
        }
        self.paste_resources(
            ResourceClipboard {
                resources,
                is_cut: !copy,
            },
            false,
            target,
            cx,
        );
    }

    fn paste_resources(
        &mut self,
        clipboard: ResourceClipboard,
        from_clipboard: bool,
        target: ResourcePath,
        cx: &mut Context<Self>,
    ) {
        if clipboard.is_cut
            && clipboard.resources.iter().any(|resource| {
                self.workspaces
                    .iter()
                    .any(|root| resource_is_within(&ResourcePath::local(root), resource))
                    || self
                        .remote_workspace
                        .as_ref()
                        .is_some_and(|workspace| resource_is_within(&workspace.root, resource))
            })
        {
            self.notify_error(
                crate::i18n::runtime::RuntimeText::WorkspaceRootProtected.to_string(),
                cx,
            );
            return;
        }
        if target.filesystem == FilesystemId::Local
            && clipboard
                .resources
                .iter()
                .all(|resource| resource.filesystem == FilesystemId::Local)
        {
            let paths = clipboard
                .resources
                .iter()
                .map(ResourcePath::local_path)
                .collect::<std::io::Result<Vec<_>>>();
            match (paths, target.local_path()) {
                (Ok(paths), Ok(target)) => {
                    // A drop borrows the local clipboard slot and gives it back.
                    let saved = (!from_clipboard)
                        .then(|| (self.resource_clipboard.take(), self.file_clipboard.take()));
                    if from_clipboard {
                        self.resource_clipboard = None;
                    }
                    self.file_clipboard = Some(FileClipboard {
                        paths,
                        is_cut: clipboard.is_cut,
                    });
                    self.paste_into_dir(target, cx);
                    if let Some((resources, files)) = saved {
                        self.resource_clipboard = resources;
                        self.file_clipboard = files;
                    }
                }
                (Err(error), _) | (_, Err(error)) => self.notify_error(error.to_string(), cx),
            }
            return;
        }
        let sources = clipboard
            .resources
            .iter()
            .map(|resource| {
                self.resource_filesystem(resource)
                    .map(|filesystem| (resource.clone(), filesystem))
            })
            .collect::<std::io::Result<Vec<_>>>();
        let (sources, destination_fs) = match (sources, self.resource_filesystem(&target)) {
            (Ok(sources), Ok(destination)) => (sources, destination),
            (Err(error), _) | (_, Err(error)) => {
                self.notify_error(error.to_string(), cx);
                return;
            }
        };
        if clipboard.is_cut
            && self.all_tabs().any(|tab| {
                let resource = tab
                    .remote
                    .as_ref()
                    .and_then(|remote| remote.resource.clone())
                    .or_else(|| tab.path.as_deref().map(ResourcePath::local));
                self.remote_saves_inflight.contains(&tab.id)
                    && resource.is_some_and(|path| {
                        clipboard
                            .resources
                            .iter()
                            .any(|source| resource_is_within(&path, source))
                    })
            })
        {
            self.notify_error(
                crate::i18n::runtime::RuntimeText::AutoSavePaused.to_string(),
                cx,
            );
            return;
        }
        let generation = self.remote_generation;
        let open_resources = self
            .all_tabs()
            .filter_map(|tab| {
                tab.remote
                    .as_ref()
                    .and_then(|remote| remote.resource.clone())
                    .or_else(|| tab.path.as_deref().map(ResourcePath::local))
            })
            .collect::<Vec<_>>();
        if clipboard.is_cut && from_clipboard {
            self.resource_clipboard = None;
            self.file_clipboard = None;
        }
        cx.spawn(async move |this, cx| {
            let destination = target.clone();
            let cut = clipboard.is_cut;
            let result =
                crate::platform::fs_task::run_write("SSH paste", PathBuf::from("ssh"), move || {
                    Ok(sources
                        .into_iter()
                        .map(|(source, filesystem)| {
                            let result = super::resource_transfer::transfer(
                                &filesystem,
                                &destination_fs,
                                &source,
                                &destination,
                                cut,
                            )
                            .and_then(|destination| {
                                let mut revisions = HashMap::new();
                                if cut
                                    && let super::resource_transfer::ResourceFilesystem::Ssh(
                                        filesystem,
                                    ) = &destination_fs
                                {
                                    for resource in &open_resources {
                                        if let Some(rebased) = rebase_transferred_resource(
                                            resource,
                                            &source,
                                            &destination,
                                        ) {
                                            let (_, revision) = filesystem
                                                .read_file(&rebased.path, 128 * 1024 * 1024)
                                                .map_err(|error| {
                                                    std::io::Error::other(format!(
                                                        "{} -> {}: {error}",
                                                        source.path, rebased.path
                                                    ))
                                                })?;
                                            revisions.insert(rebased, revision);
                                        }
                                    }
                                }
                                Ok((destination, revisions))
                            });
                            (source, result)
                        })
                        .collect::<Vec<_>>())
                })
                .await;
            let _ = this.update(cx, |editor, cx| {
                match result {
                    Ok(outcomes) => {
                        let mut pasted = Vec::new();
                        for (source, result) in outcomes {
                            match result {
                                Ok((destination, revisions)) => {
                                    if editor.remote_generation != generation {
                                        editor.notify_error(
                                            format!(
                                                "{}: {} -> {}",
                                                crate::i18n::remote_text(
                                                    editor.ui_language,
                                                    "wrong_connection"
                                                ),
                                                source.path,
                                                destination.path
                                            ),
                                            cx,
                                        );
                                        continue;
                                    }
                                    if cut {
                                        editor.retarget_transferred_resources(
                                            &source,
                                            &destination,
                                            &revisions,
                                            cx,
                                        );
                                    }
                                    pasted.push(destination);
                                }
                                Err(error) => {
                                    editor.notify_error(format!("{}: {error}", source.path), cx)
                                }
                            }
                        }
                        if let Ok(path) = target.local_path() {
                            if editor.sidebar_root.starts_with(&path)
                                || path.starts_with(&editor.sidebar_root)
                            {
                                editor.invalidate_sidebar_items_cache();
                            }
                        } else if editor.remote_generation == generation {
                            if let Some(workspace) = editor.remote_workspace.as_mut() {
                                workspace.selected_paths = pasted.iter().cloned().collect();
                                workspace.selected = pasted.first().cloned();
                                workspace.focused = pasted.first().cloned();
                                workspace.file_list_dir = target.clone();
                            }
                        }
                        if editor.remote_generation == generation {
                            editor.refresh_remote_tree(cx);
                        }
                        editor.save_settings_in_background(cx);
                        session_store::persist_session(editor);
                    }
                    Err(error) => editor.notify_error(error.to_string(), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    fn retarget_transferred_resources(
        &mut self,
        source: &ResourcePath,
        destination: &ResourcePath,
        revisions: &HashMap<ResourcePath, moeka_fs::backend::Revision>,
        cx: &mut Context<Self>,
    ) {
        let affected = self
            .all_tabs()
            .filter_map(|tab| {
                let resource = tab
                    .remote
                    .as_ref()
                    .and_then(|remote| remote.resource.clone())
                    .or_else(|| tab.path.as_deref().map(ResourcePath::local))?;
                resource_is_within(&resource, source).then_some((tab.id.clone(), resource))
            })
            .collect::<Vec<_>>();
        for (id, resource) in affected {
            let Some(rebased) = rebase_transferred_resource(&resource, source, destination) else {
                continue;
            };
            let local = rebased.local_path().ok();
            let remote_state = if local.is_none() {
                self.remote_workspace
                    .as_ref()
                    .map(|workspace| config_store::RemoteTabState {
                        peer_scope: workspace.peer_scope,
                        destination: workspace.destination.clone(),
                        config_file: workspace.config_file.clone(),
                        root: workspace.root.clone(),
                        resource: Some(rebased.clone()),
                        revision: revisions.get(&rebased).cloned(),
                        save_uncertain: false,
                        pending_resource: None,
                        line_ending_crlf: self
                            .all_tabs()
                            .find(|tab| tab.id == id)
                            .and_then(|tab| tab.remote.as_ref())
                            .is_some_and(|remote| remote.line_ending_crlf),
                    })
            } else {
                None
            };
            if let Some(owner) = self.pane_id_for_tab(&id) {
                self.with_pane(owner, |editor| {
                    if let Some(tab) = editor.tabs.iter_mut().find(|tab| tab.id == id) {
                        tab.path = local.clone();
                        tab.is_remote = remote_state.is_some();
                        tab.remote = remote_state;
                        tab.remote_binding_lost = false;
                        tab.is_deleted = false;
                        if let Some(name) = rebased.path.file_name() {
                            tab.title = name.to_string();
                        }
                        if editor.active_tab_id.as_deref() == Some(&id) {
                            editor.current_file_path = local.clone();
                        }
                    }
                });
            }
            if local.is_some() {
                self.remote_documents.remove(&id);
            } else {
                self.remote_documents.insert(
                    id,
                    super::remote::RemoteDocument {
                        resource: Some(rebased.clone()),
                        revision: revisions.get(&rebased).cloned(),
                        save_uncertain: false,
                    },
                );
            }
        }
        let stars = self.app_settings.starred_resources.clone();
        for resource in stars {
            if let Some(rebased) = rebase_transferred_resource(&resource, source, destination) {
                self.set_resources_starred(&[resource], false, cx);
                self.set_resources_starred(&[rebased], true, cx);
            }
        }
        let stars = self.app_settings.starred_files.clone();
        for path in stars {
            let resource = ResourcePath::local(&path);
            if let Some(rebased) = rebase_transferred_resource(&resource, source, destination) {
                self.set_resources_starred(&[resource], false, cx);
                self.set_resources_starred(&[rebased], true, cx);
            }
        }
    }

    pub(super) fn resource_text_snapshot(
        &mut self,
        resource: &ResourcePath,
    ) -> Option<(String, String)> {
        let tab_id = self
            .all_tabs()
            .find(|tab| {
                tab.remote
                    .as_ref()
                    .and_then(|remote| remote.resource.as_ref())
                    == Some(resource)
                    || tab
                        .path
                        .as_ref()
                        .is_some_and(|path| ResourcePath::local(path) == *resource)
            })?
            .id
            .clone();
        let owner = self.pane_id_for_tab(&tab_id)?;
        self.with_pane(owner, |editor| {
            let tab = editor.tabs.iter().find(|tab| tab.id == tab_id)?;
            let text = if editor.active_tab_id.as_deref() == Some(&tab_id) {
                editor.buffer.text().to_string()
            } else {
                tab.snapshot.text.clone()
            };
            Some((tab_id, text))
        })
    }

    pub(super) fn confirm_resource_trash(
        &mut self,
        resources: Vec<ResourcePath>,
        cx: &mut Context<Self>,
    ) {
        let resources = filter_resource_roots(resources);
        if resources
            .iter()
            .all(|resource| resource.filesystem == FilesystemId::Local)
        {
            match resources
                .iter()
                .map(ResourcePath::local_path)
                .collect::<std::io::Result<Vec<_>>>()
            {
                Ok(paths) => self.show_sidebar_delete_dialog(paths, cx),
                Err(error) => self.notify_error(error.to_string(), cx),
            }
            return;
        }
        let count = resources.len();
        let message = if count == 1 {
            crate::i18n::delete_path_message(
                self.ui_language,
                &resources[0]
                    .path
                    .file_name()
                    .map(|name| name.to_string())
                    .unwrap_or_default(),
            )
        } else {
            crate::i18n::delete_items_message(self.ui_language, count)
        };
        self.show_dialog(
            Dialog {
                title: self.t(crate::i18n::TextKey::Delete).to_string(),
                message,
                save_label: None,
                confirm_label: if count == 1 {
                    self.t(crate::i18n::TextKey::Delete).to_string()
                } else {
                    crate::i18n::delete_items_label(self.ui_language, count)
                },
                cancel_label: self.t(crate::i18n::TextKey::Cancel).to_string(),
                is_danger: true,
                action: DialogAction::DeleteResources {
                    resources,
                    generation: self.remote_generation,
                },
            },
            cx,
        );
    }

    pub(super) fn trash_resource_paths(
        &mut self,
        resources: Vec<ResourcePath>,
        cx: &mut Context<Self>,
    ) {
        use moeka_fs::backend::Filesystem;
        let Some(workspace) = &self.remote_workspace else {
            return;
        };
        if resources.iter().any(|resource| resource == &workspace.root) {
            self.notify_error(
                crate::i18n::runtime::RuntimeText::WorkspaceRootProtected.to_string(),
                cx,
            );
            return;
        }
        if self.all_tabs().any(|tab| {
            self.remote_saves_inflight.contains(&tab.id)
                && tab
                    .remote
                    .as_ref()
                    .and_then(|remote| remote.resource.as_ref())
                    .is_some_and(|path| {
                        resources
                            .iter()
                            .any(|resource| resource_is_within(path, resource))
                    })
        }) {
            self.notify_error(
                crate::i18n::runtime::RuntimeText::AutoSavePaused.to_string(),
                cx,
            );
            return;
        }
        let filesystem = workspace.filesystem.clone();
        let generation = self.remote_generation;
        cx.spawn(async move |this, cx| {
            let result =
                crate::platform::fs_task::run_write("SSH trash", PathBuf::from("ssh"), move || {
                    Ok(resources
                        .into_iter()
                        .map(|resource| {
                            let result =
                                crate::platform::fs_task::check_deadline().and_then(|()| {
                                    filesystem
                                        .trash(&resource.path)
                                        .map_err(std::io::Error::other)
                                });
                            (resource, result)
                        })
                        .collect::<Vec<_>>())
                })
                .await;
            let _ = this.update(cx, |editor, cx| {
                match result {
                    Ok(outcomes) => {
                        for (resource, result) in outcomes {
                            if let Err(error) = result {
                                editor.notify_error(format!("{}: {error}", resource.path), cx);
                                continue;
                            }
                            if editor.remote_generation != generation {
                                editor.notify_error(
                                    format!(
                                        "{}: {}",
                                        crate::i18n::remote_text(
                                            editor.ui_language,
                                            "wrong_connection"
                                        ),
                                        resource.path
                                    ),
                                    cx,
                                );
                                continue;
                            }
                            editor
                                .app_settings
                                .starred_resources
                                .retain(|path| !resource_is_within(path, &resource));
                            let closed = editor
                                .all_tabs()
                                .filter(|tab| {
                                    tab.remote
                                        .as_ref()
                                        .and_then(|remote| remote.resource.as_ref())
                                        .is_some_and(|path| resource_is_within(path, &resource))
                                })
                                .map(|tab| tab.id.clone())
                                .collect::<Vec<_>>();
                            for id in closed {
                                editor.close_tab(&id, cx);
                            }
                            if editor.remote_generation == generation
                                && let Some(workspace) = editor.remote_workspace.as_mut()
                            {
                                workspace
                                    .selected_paths
                                    .retain(|path| !resource_is_within(path, &resource));
                                workspace
                                    .expanded
                                    .retain(|path| !resource_is_within(path, &resource));
                                workspace
                                    .directories
                                    .retain(|path, _| !resource_is_within(path, &resource));
                                for entries in workspace.directories.values_mut() {
                                    entries.retain(|entry| {
                                        !resource_is_within(&entry.resource, &resource)
                                    });
                                }
                                if workspace
                                    .focused
                                    .as_ref()
                                    .is_some_and(|path| resource_is_within(path, &resource))
                                {
                                    workspace.focused = None;
                                    workspace.selected = None;
                                    workspace.selection_anchor = None;
                                }
                            }
                        }
                        editor.save_settings_in_background(cx);
                        if editor.remote_generation == generation {
                            editor.refresh_remote_tree(cx);
                        }
                    }
                    Err(error) => editor.notify_error(error.to_string(), cx),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn reveal_resource_in_tree(
        &mut self,
        resource: ResourcePath,
        cx: &mut Context<Self>,
    ) {
        self.activate_resource_workspace(&resource, cx);
        if let Ok(path) = resource.local_path() {
            self.locate_path_in_sidebar(path, cx);
            return;
        }
        let Some(root) = self
            .remote_workspace
            .as_ref()
            .map(|workspace| workspace.root.clone())
        else {
            return;
        };
        let mut parent = resource.parent();
        while let Some(path) = parent {
            if !resource_is_within(&path, &root) {
                break;
            }
            if let Some(workspace) = self.remote_workspace.as_mut() {
                workspace.expanded.insert(path.clone());
            }
            self.load_remote_directory(path.clone(), cx);
            parent = path.parent();
        }
        self.select_remote_sidebar_item(&resource, false, false);
        cx.notify();
    }

    pub(super) fn perform_resource_menu_action(
        &mut self,
        action: ResourceMenuAction,
        resource: &ResourcePath,
        resources: &[ResourcePath],
        directory: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use ResourceMenuAction::*;
        let local = resource.local_path().ok();
        match action {
            CopyPath | CopyRelativePath => {
                let text = resources
                    .iter()
                    .map(|path| self.resource_path_text(path, action == CopyRelativePath))
                    .collect::<Vec<_>>()
                    .join("\n");
                self.copy_text_to_clipboard(text, cx);
            }
            Star => {
                let starred = !resources.iter().all(|path| self.resource_is_starred(path));
                self.set_resources_starred(resources, starred, cx);
            }
            Copy | Cut => self.set_resource_clipboard(resources.to_vec(), action == Cut),
            Delete => self.confirm_resource_trash(resources.to_vec(), cx),
            Chat => {
                self.activate_resource_workspace(resource, cx);
                self.add_resources_to_chat(resources.to_vec(), cx);
            }
            CloseWorkspace => {
                if let Some(path) = local {
                    self.close_workspace(&path, cx);
                } else {
                    self.request_close_remote_workspace(cx);
                }
            }
            Reveal => {
                if let Some(path) = local {
                    self.reveal_in_file_manager(&path, cx);
                }
            }
            Refresh => self.refresh_remote_tree(cx),
            Disconnect => self.disconnect_remote(cx),
            NewFile | NewDirectory | Paste => {
                let parent = if directory {
                    Some(resource.clone())
                } else {
                    resource.parent()
                };
                let Some(parent) = parent else {
                    self.notify_error(
                        crate::i18n::runtime::RuntimeText::FileAction.to_string(),
                        cx,
                    );
                    return;
                };
                self.activate_resource_workspace(resource, cx);
                if action == Paste {
                    self.paste_resource_clipboard(parent, cx);
                } else if let Ok(path) = parent.local_path() {
                    if action == NewDirectory {
                        self.start_create_dir(path, window, cx);
                    } else {
                        self.start_create_file(path, window, cx);
                    }
                } else {
                    self.start_remote_sidebar_create(parent, action == NewDirectory, window, cx);
                }
            }
            Rename => {
                self.activate_resource_workspace(resource, cx);
                if let Some(path) = local {
                    self.start_rename(path, window, cx);
                } else {
                    self.start_remote_sidebar_rename(resource.clone(), window, cx);
                }
            }
            Collapse | Expand | Search => {
                self.activate_resource_workspace(resource, cx);
                match action {
                    Collapse if local.is_some() => self.collapse_all_dirs(cx),
                    Collapse => self.collapse_remote_directories(cx),
                    Expand if local.is_some() => self.expand_all_dirs(cx),
                    Expand => self.expand_remote_directories(cx),
                    _ => {
                        if !self.workspace_search_visible {
                            self.toggle_workspace_search(window, cx);
                        }
                    }
                }
            }
            Export | Separator => unreachable!("non-action menu entries"),
        }
        cx.notify();
    }

    pub(super) fn resource_menu_paths(
        &self,
        resource: &ResourcePath,
        root: bool,
    ) -> Vec<ResourcePath> {
        if root {
            return vec![resource.clone()];
        }
        let paths = match resource.local_path() {
            Ok(path) => self
                .sidebar_context_paths(&path)
                .iter()
                .map(|path| ResourcePath::local(path))
                .collect(),
            Err(_) => self.remote_file_tree_context_resources(resource),
        };
        filter_resource_roots(paths)
    }

    pub(super) fn validate_resource_menu(
        &mut self,
        resource: &ResourcePath,
        generation: u64,
        cx: &mut Context<Self>,
    ) -> bool {
        if resource.filesystem == FilesystemId::Local {
            return true;
        }
        if self.remote_generation == generation
            && self.remote_workspace.as_ref().is_some_and(|workspace| {
                workspace.filesystem.identity() == &resource.filesystem
                    && resource_is_within(resource, &workspace.root)
            })
        {
            return true;
        }
        self.notify_error(
            crate::i18n::remote_text(self.ui_language, "wrong_connection"),
            cx,
        );
        false
    }

    pub(super) fn activate_resource_workspace(
        &mut self,
        resource: &ResourcePath,
        cx: &mut Context<Self>,
    ) {
        if let Ok(path) = resource.local_path() {
            if let Some(root) = self.workspace_root_for_path(&path) {
                self.switch_workspace(root, cx);
            }
        } else if !self.remote_session_active() {
            self.activate_remote_workspace(cx);
        }
    }

    pub(super) fn resource_path_text(&self, resource: &ResourcePath, relative: bool) -> String {
        if let Ok(path) = resource.local_path() {
            return self
                .path_copy_text(&path, relative)
                .unwrap_or_else(|| path.display().to_string());
        }
        if relative
            && let Some(workspace) = &self.remote_workspace
            && resource_is_within(resource, &workspace.root)
        {
            let mut parts = Vec::new();
            let mut current = resource.clone();
            while current != workspace.root {
                if let Some(name) = current.path.file_name() {
                    parts.push(name.to_string());
                }
                let Some(parent) = current.parent() else {
                    break;
                };
                current = parent;
            }
            parts.reverse();
            return if parts.is_empty() {
                ".".into()
            } else {
                parts.join("/")
            };
        }
        resource.path.to_string()
    }

    pub(super) fn resource_is_starred(&self, resource: &ResourcePath) -> bool {
        match resource.local_path() {
            Ok(path) => self.app_settings.is_starred_file(&path),
            Err(_) => self.app_settings.starred_resources.contains(resource),
        }
    }

    pub(super) fn resource_is_known_file(&self, resource: &ResourcePath) -> bool {
        if let Ok(path) = resource.local_path() {
            return self.sidebar_cached_path_is_dir(&path) == Some(false);
        }
        self.remote_workspace.as_ref().is_some_and(|workspace| {
            workspace
                .directories
                .values()
                .flatten()
                .any(|entry| entry.resource == *resource && !entry.metadata.is_dir)
        })
    }

    pub(super) fn set_resources_starred(
        &mut self,
        resources: &[ResourcePath],
        starred: bool,
        cx: &mut Context<Self>,
    ) {
        for resource in resources {
            if let Ok(path) = resource.local_path() {
                if starred {
                    self.app_settings
                        .add_starred_file(Self::normalize_sidebar_path(&path));
                } else {
                    self.app_settings
                        .remove_starred_file(&Self::normalize_sidebar_path(&path));
                }
            } else if starred {
                if !self.app_settings.starred_resources.contains(resource) {
                    self.app_settings.starred_resources.push(resource.clone());
                }
            } else {
                self.app_settings
                    .starred_resources
                    .retain(|path| path != resource);
            }
        }
        if starred {
            self.sidebar_starred_collapsed = false;
        }
        self.save_settings_in_background(cx);
        cx.notify();
    }

    pub(super) fn resource_clipboard_contents(&self) -> Option<ResourceClipboard> {
        self.resource_clipboard.clone().or_else(|| {
            self.file_clipboard
                .as_ref()
                .map(|clipboard| ResourceClipboard {
                    resources: clipboard
                        .paths
                        .iter()
                        .map(|path| ResourcePath::local(path))
                        .collect(),
                    is_cut: clipboard.is_cut,
                })
        })
    }

    pub(super) fn set_resource_clipboard(&mut self, resources: Vec<ResourcePath>, cut: bool) {
        self.file_clipboard = None;
        self.resource_clipboard = Some(ResourceClipboard {
            resources: filter_resource_roots(resources),
            is_cut: cut,
        });
    }

    pub(super) fn set_local_file_tree_clipboard(&mut self, clipboard: FileClipboard) {
        self.resource_clipboard = None;
        self.file_clipboard = Some(clipboard);
    }

    pub(super) fn file_tree_resource_selected(&self, resource: &ResourcePath) -> bool {
        match &resource.filesystem {
            FilesystemId::Local => resource.local_path().ok().is_some_and(|path| {
                if self.workspaces.contains(&path) {
                    self.focused_workspace_root.as_ref() == Some(&path)
                } else {
                    self.focused_workspace_root.is_none()
                        && self.selected_sidebar_paths.contains(&path)
                }
            }),
            FilesystemId::Ssh(_) => self.remote_workspace.as_ref().is_some_and(|workspace| {
                self.focused_workspace_root.is_none()
                    && if resource == &workspace.root {
                        workspace.focused.as_ref() == Some(resource)
                    } else {
                        workspace.focused.as_ref() != Some(&workspace.root)
                            && workspace.selected_paths.contains(resource)
                    }
            }),
        }
    }

    pub(super) fn file_tree_resource_focused(&self, resource: &ResourcePath) -> bool {
        match &resource.filesystem {
            FilesystemId::Local => resource.local_path().ok().is_some_and(|path| {
                self.focused_workspace_root.is_none()
                    && self.focused_sidebar_path.as_ref() == Some(&path)
                    && self.focused_starred_path.is_none()
            }),
            FilesystemId::Ssh(_) => self.remote_workspace.as_ref().is_some_and(|workspace| {
                self.focused_workspace_root.is_none()
                    && workspace.focused.as_ref() == Some(resource)
            }),
        }
    }

    pub(super) fn select_file_tree_resource(
        &mut self,
        resource: &ResourcePath,
        shift: bool,
        secondary: bool,
    ) {
        match &resource.filesystem {
            FilesystemId::Local => {
                if let Ok(path) = resource.local_path() {
                    self.sidebar_select_item(&path, shift, secondary);
                }
            }
            FilesystemId::Ssh(_) => {
                self.select_remote_sidebar_item(resource, shift, secondary);
            }
        }
    }

    pub(super) fn activate_file_tree_resource(
        &mut self,
        resource: ResourcePath,
        is_dir: bool,
        event: &ClickEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let modifiers = event.modifiers();
        let secondary = modifiers.secondary();
        let modified = secondary || modifiers.shift;
        self.select_file_tree_resource(&resource, modifiers.shift, secondary);
        if !modified {
            match &resource.filesystem {
                FilesystemId::Local => {
                    let Ok(path) = resource.local_path() else {
                        return;
                    };
                    self.set_file_list_dir_for_entry(&path, is_dir, cx);
                    if is_dir {
                        self.toggle_sidebar_directory(path, cx);
                    } else {
                        self.suppress_next_sidebar_sync_path = Some(path.clone());
                        if event.click_count() >= 2 {
                            self.clear_sidebar_keyboard_focus();
                            self.open_path_in_permanent_tab(path, true, cx);
                            self.move_cursor_to_first_line_end();
                            self.request_editor_focus(cx);
                        } else if !self.switch_to_existing_editor_tab_for_path(&path, false, cx) {
                            self.open_path_in_preview_tab(path, false, cx);
                        }
                    }
                }
                FilesystemId::Ssh(_) => {
                    if is_dir {
                        self.toggle_remote_sidebar_directory(resource, cx);
                    } else {
                        if let Some(workspace) = self.remote_workspace.as_mut()
                            && let Some(parent) = resource.parent()
                        {
                            workspace.file_list_dir = parent;
                        }
                        self.open_remote_file(resource, cx);
                    }
                }
            }
        }
        window.focus(&self.focus_handle);
        window.prevent_default();
        cx.notify();
    }

    pub(super) fn open_file_tree_resource_context_menu(
        &mut self,
        resource: ResourcePath,
        is_dir: bool,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.prevent_default();
        cx.stop_propagation();
        if !self.file_tree_resource_selected(&resource) {
            self.select_file_tree_resource(&resource, false, false);
        }
        let is_root = match &resource.filesystem {
            FilesystemId::Local => resource
                .local_path()
                .ok()
                .is_some_and(|path| path == self.sidebar_root),
            FilesystemId::Ssh(_) => self.remote_workspace.as_mut().is_some_and(|workspace| {
                workspace.focused = Some(resource.clone());
                workspace.root == resource
            }),
        };
        if let Ok(path) = resource.local_path() {
            self.focused_sidebar_path = Some(path);
        }
        self.open_context_menu(
            ContextMenu {
                position: event.position,
                kind: ContextMenuKind::FileTreeResource {
                    resource,
                    is_dir,
                    is_root,
                    generation: self.remote_generation,
                },
            },
            true,
            cx,
        );
        window.focus(&self.focus_handle);
    }
}
