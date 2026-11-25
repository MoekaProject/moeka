//! Workspaces are listed in the persistent editor status bar and in the
//! FileTree, with both surfaces sharing activation and menu behavior.

use std::path::PathBuf;

use gpui::{
    AnyElement, Context, DragMoveEvent, MouseButton, MouseDownEvent, SharedString, canvas, div,
    prelude::*, px,
};

use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub(crate) enum WorkspaceKey {
    Local(PathBuf),
    Remote {
        destination: String,
        root: moeka_fs::path::ResourcePath,
    },
}

#[derive(Clone)]
pub(crate) struct DraggedWorkspaceTab {
    pub key: WorkspaceKey,
    pub title: String,
}

struct StripItem {
    key: WorkspaceKey,
    title: String,
    active: bool,
    remote: bool,
    status: Option<String>,
}

impl EditorView {
    fn workspace_count(&self) -> usize {
        self.workspaces.len()
            + usize::from(
                !self.sidebar_root.as_os_str().is_empty()
                    && !self.workspaces.contains(&self.sidebar_root),
            )
            + usize::from(self.remote_workspace.is_some())
            + self.parked_remote_workspaces.len()
            + self.pending_ssh_workspace_count()
    }

    /// Every workspace is represented in the status bar, including the only
    /// workspace.
    pub(crate) fn workspace_strip_visible(&self) -> bool {
        !self.embedded_editor && self.workspace_count() > 0
    }

    pub(super) fn sidebar_lists_local_workspace(&self, root: &std::path::Path) -> bool {
        !self.remote_workspace_active && root == self.sidebar_root
    }

    fn workspace_strip_items(&self) -> Vec<StripItem> {
        let mut items = Vec::new();
        let local_roots = self.workspaces.iter().chain(
            (!self.sidebar_root.as_os_str().is_empty()
                && !self.workspaces.contains(&self.sidebar_root))
            .then_some(&self.sidebar_root),
        );
        for root in local_roots {
            items.push(StripItem {
                key: WorkspaceKey::Local(root.clone()),
                title: root
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| root.display().to_string())
                    .to_uppercase(),
                active: !self.remote_workspace_active && *root == self.sidebar_root,
                remote: false,
                status: None,
            });
        }
        for (_, parked, workspace) in self.remote_workspaces_in_order() {
            let active = parked.is_none()
                && self.live_view.as_ref().is_some_and(|key| {
                    *key == workspace::remote_view_key(&workspace.destination, &workspace.root)
                });
            items.push(StripItem {
                key: WorkspaceKey::Remote {
                    destination: workspace.destination.clone(),
                    root: workspace.root.clone(),
                },
                title: workspace
                    .root
                    .path
                    .file_name()
                    .map(|name| name.to_string())
                    .unwrap_or_else(|| workspace.root.path.to_string())
                    .to_uppercase(),
                active,
                remote: true,
                status: workspace.reconnecting.then(|| {
                    crate::i18n::remote_text(self.ui_language, "reconnecting").to_string()
                }),
            });
        }
        for (destination, root, name, error, active) in self.pending_ssh_workspace_labels() {
            let state = crate::i18n::remote_text(
                self.ui_language,
                if error { "offline" } else { "reconnecting" },
            );
            items.push(StripItem {
                key: WorkspaceKey::Remote { destination, root },
                title: name,
                active,
                remote: true,
                status: Some(state.to_string()),
            });
        }
        let fallback = items
            .iter()
            .enumerate()
            .map(|(index, item)| (item.key.clone(), index))
            .collect::<std::collections::HashMap<_, _>>();
        let positions = self
            .workspace_strip_order
            .iter()
            .enumerate()
            .map(|(index, key)| (key, index))
            .collect::<std::collections::HashMap<_, _>>();
        items.sort_by_key(|item| {
            positions
                .get(&item.key)
                .copied()
                .unwrap_or(positions.len() + fallback[&item.key])
        });
        items
    }

    pub(crate) fn render_workspace_strip(&self, cx: &mut Context<Self>) -> AnyElement {
        let ss = self.styles.status_bar();
        let height = self.status_bar_height();
        let active_background = ss.active_background;
        let mut remote_badge = self.styles.sidebar_kind_badge(false);
        let font_size = ss.font_size_px - 1.0;
        remote_badge.text.size_px = font_size * 0.8;
        remote_badge.padding_x_px = (ss.padding_x_px * 0.5).max(2.0);
        remote_badge.padding_y_px = 0.0;
        remote_badge.radius_px = (ss.height_px * 0.2).min(remote_badge.radius_px);
        let drop_color = self.styles.tab_bar.tab_drop_indicator_color;
        let drop_width = self.styles.tab_bar.tab_drop_indicator_width_px;
        let items = self.workspace_strip_items();
        let item_bounds = Rc::new(RefCell::new(Vec::<(
            WorkspaceKey,
            gpui::Bounds<gpui::Pixels>,
        )>::new()));
        let ghost_style = self.styles.drag_preview(true, self.ui_scale);
        let drop_target = self.workspace_strip_drop_target.clone();
        let captured_item_bounds = item_bounds.clone();
        let children = items.into_iter().map(|item| {
            let key = item.key.clone();
            let title = item.title.clone();
            let label = match &item.status {
                Some(status) => format!("[{status}] {}", item.title),
                None => item.title.clone(),
            };
            let indicator = drop_target
                .as_ref()
                .filter(|(target, _)| *target == item.key)
                .map(|(_, after)| *after);
            let recorded_bounds = item_bounds.clone();
            let bounds_key = key.clone();
            let row = div()
                .id(SharedString::from(format!(
                    "workspace-strip-surface-{key:?}"
                )))
                .debug_selector({
                    let key = key.clone();
                    move || format!("workspace-strip-surface-{key:?}")
                })
                .relative()
                .h_full()
                .px(px(self.scaled(ss.padding_x_px)))
                .flex()
                .items_center()
                .gap(px(self.scaled(ss.item_gap_px)))
                .bg(if item.active {
                    active_background
                } else {
                    gpui::transparent_black()
                })
                .text_color(if item.active { ss.value } else { ss.title })
                .when(!item.active, |this| {
                    this.opacity(0.5)
                        .hover(move |style| style.opacity(1.0).bg(ss.hover))
                })
                .child(
                    crate::ui::decoration::icon(
                        crate::style::IconStyle::new(
                            self.scaled(ss.icon_size_px),
                            if item.active { ss.value } else { ss.title },
                        ),
                        ROOT_SVG_PATH,
                    )
                    .debug_selector({
                        let key = key.clone();
                        move || format!("workspace-strip-icon-{key:?}")
                    }),
                )
                .when(item.remote, |this| {
                    this.child(
                        crate::ui::typography::badge(remote_badge, self.ui_scale)
                            .flex_none()
                            .child("REMOTE"),
                    )
                })
                .child(
                    div()
                        .min_w(px(0.0))
                        .overflow_hidden()
                        .truncate()
                        .whitespace_nowrap()
                        .child(label),
                )
                .on_drag(
                    DraggedWorkspaceTab {
                        key: key.clone(),
                        title: title.clone(),
                    },
                    {
                        let ghost_style = ghost_style.clone();
                        move |dragged, offset, _, cx| {
                            cx.new(|_| DraggedTabView {
                                title: dragged.title.clone(),
                                style: ghost_style.clone(),
                                cursor_offset: offset,
                            })
                        }
                    },
                )
                .on_drag_move({
                    let key = key.clone();
                    cx.listener(
                        move |this, event: &DragMoveEvent<DraggedWorkspaceTab>, _, cx| {
                            if !event.bounds.contains(&event.event.position) {
                                return;
                            }
                            let after = event.event.position.x > event.bounds.center().x;
                            let target = Some((key.clone(), after));
                            if this.workspace_strip_drop_target != target {
                                this.workspace_strip_drop_target = target;
                                cx.notify();
                            }
                        },
                    )
                })
                .on_drop({
                    let key = key.clone();
                    cx.listener(move |this, dragged: &DraggedWorkspaceTab, _, cx| {
                        let after = this
                            .workspace_strip_drop_target
                            .as_ref()
                            .is_some_and(|(target, after)| *target == key && *after);
                        this.workspace_strip_drop_target = None;
                        this.reorder_workspace_key(&dragged.key, &key, after, cx);
                    })
                });
            div()
                .flex_none()
                .relative()
                .h(px(height))
                .min_w(px(self.scaled(ss.height_px * 3.0)))
                .max_w(px(
                    self.scaled(self.styles.tab_bar.tab_item_max_width_px * 0.75)
                ))
                .border_r(px(self.scaled(ss.border_width_px)))
                .border_color(ss.border)
                .on_children_prepainted(move |bounds, _, _| {
                    if let Some(bounds) = bounds.first().copied() {
                        recorded_bounds
                            .borrow_mut()
                            .push((bounds_key.clone(), bounds));
                    }
                })
                .id(SharedString::from(format!("workspace-strip-{key:?}")))
                .debug_selector({
                    let key = key.clone();
                    move || format!("workspace-strip-{key:?}")
                })
                .when_some(indicator, |this, after| {
                    this.child(
                        div()
                            .absolute()
                            .top_0()
                            .bottom_0()
                            .w(px(drop_width))
                            .bg(drop_color)
                            .when(after, |line| line.right_0())
                            .when(!after, |line| line.left_0()),
                    )
                })
                .child(row)
                .into_any_element()
        });
        div()
            .id("workspace-strip")
            .debug_selector(|| "workspace-strip".into())
            .relative()
            .flex_1()
            .min_w(px(0.0))
            .h(px(height))
            .flex()
            .items_center()
            .text_size(px(self.scaled(font_size)))
            .child({
                let editor = cx.entity().clone();
                canvas(
                    |_, _, _| (),
                    move |_, _, window, _| {
                        window.on_mouse_event({
                            let editor = editor.clone();
                            let item_bounds = captured_item_bounds.clone();
                            move |event: &MouseDownEvent, phase, window, cx| {
                                if !phase.capture() {
                                    return;
                                }
                                let key = item_bounds.borrow().iter().find_map(|(key, bounds)| {
                                    bounds.contains(&event.position).then(|| key.clone())
                                });
                                let Some(key) = key else { return };
                                editor.update(cx, |this, cx| match event.button {
                                    MouseButton::Left => {
                                        if this.sidebar_edit.is_some()
                                            || this.remote_sidebar_edit.is_some()
                                        {
                                            window.focus(&this.focus_handle);
                                        }
                                        this.commit_sidebar_edit(cx);
                                        this.commit_remote_sidebar_edit(true, cx);
                                        this.activate_workspace_key(key, cx);
                                    }
                                    MouseButton::Right => {
                                        window.prevent_default();
                                        cx.stop_propagation();
                                        this.open_workspace_key_menu(key, event, window, cx);
                                    }
                                    _ => {}
                                });
                            }
                        });
                    },
                )
                .absolute()
                .inset_0()
            })
            .child(
                div()
                    .id("workspace-strip-scroll")
                    .flex_1()
                    .h_full()
                    .min_w(px(0.0))
                    .flex()
                    .items_center()
                    .overflow_x_scroll()
                    .track_scroll(&self.workspace_strip_scroll)
                    .children(children)
                    .child(
                        div()
                            .id("workspace-strip-drag-space")
                            .flex_1()
                            .min_w(px(0.0))
                            .h_full()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(Self::on_tab_bar_drag_space_mouse_down),
                            ),
                    ),
            )
            .into_any_element()
    }

    fn activate_workspace_key(&mut self, key: WorkspaceKey, cx: &mut Context<Self>) {
        match key {
            WorkspaceKey::Local(root) => self.activate_workspace_row(root, cx),
            WorkspaceKey::Remote { destination, root } => {
                if let Some((serial, ..)) =
                    self.remote_workspaces_in_order()
                        .into_iter()
                        .find(|(_, _, workspace)| {
                            workspace.destination == destination && workspace.root == root
                        })
                {
                    self.activate_remote_workspace_serial(serial, cx);
                } else if let Some(index) = self.pending_ssh_workspace_index(&destination, &root) {
                    self.activate_pending_ssh_workspace_at(index, cx);
                }
            }
        }
    }

    fn open_workspace_key_menu(
        &mut self,
        key: WorkspaceKey,
        event: &MouseDownEvent,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        match key {
            WorkspaceKey::Local(root) => {
                self.open_workspace_resource_context_menu(
                    moeka_fs::path::ResourcePath::local(&root),
                    event,
                    window,
                    cx,
                );
            }
            WorkspaceKey::Remote { destination, root } => {
                if self
                    .remote_workspaces_in_order()
                    .into_iter()
                    .any(|(_, _, workspace)| {
                        workspace.destination == destination && workspace.root == root
                    })
                    || self
                        .pending_ssh_workspace_index(&destination, &root)
                        .is_some()
                {
                    self.open_workspace_resource_context_menu(root, event, window, cx);
                }
            }
        }
    }

    fn reorder_workspace_key(
        &mut self,
        source: &WorkspaceKey,
        target: &WorkspaceKey,
        after: bool,
        cx: &mut Context<Self>,
    ) {
        if source == target {
            return;
        }
        let mut order = self
            .workspace_strip_items()
            .into_iter()
            .map(|item| item.key)
            .collect::<Vec<_>>();
        let (Some(from), Some(target_index)) = (
            order.iter().position(|key| key == source),
            order.iter().position(|key| key == target),
        ) else {
            return;
        };
        let moved = order.remove(from);
        let adjusted_target = target_index.saturating_sub(usize::from(from < target_index));
        order.insert(adjusted_target + usize::from(after), moved);
        self.workspace_strip_order = order;
        session_store::persist_session(self);
        cx.notify();
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;

    #[gpui::test]
    fn strip_lists_every_workspace_and_sidebar_lists_only_the_active_workspace(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::AppContext as _;
        let editor = cx
            .update(|cx| cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some(""), false)));
        editor.update(cx, |editor, cx| {
            editor.embedded_editor = false;
            let (a, b) = (PathBuf::from("/tmp/a"), PathBuf::from("/tmp/b"));
            editor.workspaces = vec![a.clone()];
            editor.sidebar_root = a.clone();
            assert!(editor.workspace_strip_visible());
            assert!(editor.sidebar_lists_local_workspace(&a));
            editor.workspaces.clear();
            assert!(editor.workspace_strip_visible());
            assert_eq!(
                editor
                    .workspace_strip_items()
                    .into_iter()
                    .map(|item| item.key)
                    .collect::<Vec<_>>(),
                vec![WorkspaceKey::Local(a.clone())]
            );
            editor.workspaces.push(a.clone());
            editor.workspaces.push(b.clone());
            assert!(editor.workspace_strip_visible());
            assert!(editor.sidebar_lists_local_workspace(&a));
            assert!(!editor.sidebar_lists_local_workspace(&b));
            editor.reorder_workspace_key(
                &WorkspaceKey::Local(b.clone()),
                &WorkspaceKey::Local(a.clone()),
                false,
                cx,
            );
            assert_eq!(editor.workspaces, vec![a.clone(), b.clone()]);
            assert_eq!(
                editor.workspace_strip_order,
                vec![WorkspaceKey::Local(b), WorkspaceKey::Local(a)]
            );
        });
    }

    #[gpui::test]
    fn strip_renders_for_several_workspaces(cx: &mut gpui::TestAppContext) {
        use gpui::{AppContext as _, Entity, Render, Subscription, Window};
        struct Host(Entity<EditorView>, Subscription);
        impl Render for Host {
            fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                self.0.update(cx, |editor, cx| {
                    div()
                        .size_full()
                        .relative()
                        .child(editor.render_workspace_strip(cx))
                        .child(editor.render_context_menu(cx))
                })
            }
        }
        let editor = cx.update(|cx| {
            cx.new(|cx| {
                let mut editor = EditorView::new_with_mode(cx, true, false, Some(""), false);
                editor.embedded_editor = false;
                editor.workspaces = (0..3)
                    .map(|i| PathBuf::from(format!("/tmp/ws-{i}")))
                    .collect();
                editor.sidebar_root = editor.workspaces[1].clone();
                editor
            })
        });
        let (_, cx) = cx.add_window_view(|_, cx| {
            Host(editor.clone(), cx.observe(&editor, |_, _, cx| cx.notify()))
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("workspace-strip").is_some());
        let keys = cx.update(|_, cx| {
            editor
                .read(cx)
                .workspaces
                .iter()
                .cloned()
                .map(WorkspaceKey::Local)
                .collect::<Vec<_>>()
        });
        for key in &keys {
            let item = Box::leak(format!("workspace-strip-{key:?}").into_boxed_str());
            let icon = Box::leak(format!("workspace-strip-icon-{key:?}").into_boxed_str());
            let item_bounds = cx.debug_bounds(item).expect("workspace strip item");
            let icon_bounds = cx.debug_bounds(icon).expect("workspace icon");
            assert!(item_bounds.contains(&icon_bounds.center()));
        }
        let before = keys
            .iter()
            .map(|key| {
                let selector = Box::leak(format!("workspace-strip-{key:?}").into_boxed_str());
                cx.debug_bounds(selector).unwrap()
            })
            .collect::<Vec<_>>();
        let from = before[0].center();
        let to = gpui::point(before[2].right() - px(2.0), before[2].center().y);
        cx.simulate_mouse_down(from, MouseButton::Left, Default::default());
        cx.simulate_mouse_move(
            from + gpui::point(px(8.0), px(0.0)),
            MouseButton::Left,
            Default::default(),
        );
        cx.simulate_mouse_move(to, MouseButton::Left, Default::default());
        for (index, key) in keys.iter().enumerate() {
            let selector = Box::leak(format!("workspace-strip-{key:?}").into_boxed_str());
            assert_eq!(
                cx.debug_bounds(selector).unwrap().size,
                before[index].size,
                "drop indicator must not change item geometry"
            );
        }
        cx.simulate_mouse_up(to, MouseButton::Left, Default::default());
        cx.update(|_, cx| {
            assert_eq!(
                editor.read(cx).workspace_strip_order,
                vec![keys[1].clone(), keys[2].clone(), keys[0].clone()]
            );
        });
        let third = Box::leak(format!("workspace-strip-{:?}", keys[2]).into_boxed_str());
        let third_bounds = cx.debug_bounds(third).unwrap();
        cx.simulate_click(third_bounds.center(), Default::default());
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).sidebar_root, PathBuf::from("/tmp/ws-2"));
            assert!(!editor.read(cx).remote_workspace_active);
        });
        let first = Box::leak(format!("workspace-strip-{:?}", keys[0]).into_boxed_str());
        let first_bounds = cx.debug_bounds(first).unwrap();
        cx.simulate_mouse_down(
            first_bounds.center(),
            MouseButton::Right,
            Default::default(),
        );
        cx.simulate_mouse_up(
            first_bounds.center(),
            MouseButton::Right,
            Default::default(),
        );
        cx.update(|_, cx| {
            assert!(matches!(
                editor.read(cx).context_menu.as_ref().map(|menu| &menu.kind),
                Some(ContextMenuKind::FileTreeResource { resource, is_dir: true, is_root: true, .. })
                    if resource.local_path().ok().as_deref() == Some(std::path::Path::new("/tmp/ws-0"))
            ));
        });
    }

    #[gpui::test]
    fn empty_workspace_menu_create_is_removed_on_blur(cx: &mut gpui::TestAppContext) {
        use gpui::{AppContext as _, Render, Window};
        struct Host(gpui::Entity<EditorView>);
        impl Render for Host {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().child(self.0.clone())
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_path_buf();
        let other = root.join("other");
        moeka_fs::local::native::create_dir(&other).unwrap();
        let editor = cx.update(|cx| {
            cx.new(|cx| {
                let mut editor = EditorView::new_with_mode(cx, true, false, Some(""), false);
                editor.workspaces.push(root.clone());
                editor.workspaces.push(other.clone());
                editor.sidebar_root = root.clone();
                editor.sidebar_visible = true;
                editor.embedded_editor = false;
                editor.last_window_height = 700.0;
                editor
            })
        });
        let (_, cx) = cx.add_window_view(|_, _| Host(editor.clone()));
        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.perform_resource_menu_action(
                    super::file_tree::ResourceMenuAction::NewDirectory,
                    &moeka_fs::path::ResourcePath::local(&root),
                    &[],
                    true,
                    window,
                    cx,
                );
                assert!(editor.sidebar_edit.is_some());
            });
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("sidebar-edit-row").is_some());
        let item = Box::leak(
            format!("workspace-strip-{:?}", WorkspaceKey::Local(other.clone())).into_boxed_str(),
        );
        let target = cx.debug_bounds(item).unwrap().center();
        cx.simulate_click(target, Default::default());
        cx.run_until_parked();
        cx.update(|_, cx| assert!(editor.read(cx).sidebar_edit.is_none()));

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.switch_workspace(root.clone(), cx);
                editor.perform_resource_menu_action(
                    super::file_tree::ResourceMenuAction::NewDirectory,
                    &moeka_fs::path::ResourcePath::local(&root),
                    &[],
                    true,
                    window,
                    cx,
                );
                if let Some(super::SidebarEditMode::CreateDir { editor: input, .. }) =
                    &editor.sidebar_edit
                {
                    input.update(cx, |input, cx| input.set_text("created", cx));
                    assert_eq!(input.read(cx).text(), "created");
                } else {
                    panic!("expected directory input");
                }
            });
        });
        cx.run_until_parked();
        cx.simulate_click(target, Default::default());
        cx.run_until_parked();
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).sidebar_root, other);
            assert!(editor.read(cx).sidebar_edit.is_none());
        });
        for _ in 0..100 {
            if moeka_fs::local::native::metadata(root.join("created")).is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
            cx.run_until_parked();
        }
        assert!(moeka_fs::local::native::metadata(root.join("created")).is_ok());
        cx.update(|_, cx| assert_eq!(editor.read(cx).sidebar_root, other));

        cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.switch_workspace(root.clone(), cx);
                editor.perform_resource_menu_action(
                    super::file_tree::ResourceMenuAction::NewFile,
                    &moeka_fs::path::ResourcePath::local(&root),
                    &[],
                    true,
                    window,
                    cx,
                );
                if let Some(super::SidebarEditMode::CreateFile { editor: input, .. }) =
                    &editor.sidebar_edit
                {
                    input.update(cx, |input, cx| input.set_text("created-file", cx));
                } else {
                    panic!("expected file input");
                }
            });
        });
        cx.run_until_parked();
        cx.simulate_click(target, Default::default());
        for _ in 0..100 {
            cx.run_until_parked();
            if moeka_fs::local::native::metadata(root.join("created-file.md")).is_ok() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert!(moeka_fs::local::native::metadata(root.join("created-file.md")).is_ok());
        cx.update(|_, cx| assert_eq!(editor.read(cx).sidebar_root, other));
    }

    #[gpui::test]
    fn empty_remote_create_blur_cancels_but_submit_keeps_input(cx: &mut gpui::TestAppContext) {
        use gpui::{AppContext as _, Render, Window};
        use moeka_fs::path::{FilesystemId, NativePath, ResourcePath};
        struct Host(gpui::Entity<EditorView>);
        impl Render for Host {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div().child(self.0.clone())
            }
        }
        let root = ResourcePath {
            filesystem: FilesystemId::Ssh([4; 32]),
            path: NativePath::unix_absolute("/work").unwrap(),
        };
        let editor = cx
            .update(|cx| cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some(""), false)));
        let (_, cx) = cx.add_window_view(|_, _| Host(editor.clone()));
        for directory in [true, false] {
            cx.update(|window, cx| {
                editor.update(cx, |editor, cx| {
                    editor.start_remote_sidebar_create(root.clone(), directory, window, cx);
                    editor.commit_remote_sidebar_edit(false, cx);
                    assert!(editor.remote_sidebar_edit.is_some());
                    editor.commit_remote_sidebar_edit(true, cx);
                    assert!(editor.remote_sidebar_edit.is_none());
                });
            });
        }
    }

    #[gpui::test]
    fn empty_root_view_does_not_cover_workspace_strip_items(cx: &mut gpui::TestAppContext) {
        use gpui::{AppContext as _, Render, Window};
        struct RootHost(gpui::Entity<EditorView>);
        impl Render for RootHost {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                self.0.clone()
            }
        }
        let directory = tempfile::tempdir().unwrap();
        let roots = [
            directory.path().join("empty-a"),
            directory.path().join("empty-b"),
        ];
        for root in &roots {
            moeka_fs::local::native::create_dir(root).unwrap();
        }
        let expected = roots[1].clone();
        let editor = cx.update(|cx| {
            cx.new(|cx| {
                let mut editor = EditorView::new_with_mode(cx, true, false, None, false);
                editor.embedded_editor = false;
                editor.styles.ui.spec.animations_enabled = false;
                editor.workspaces = vec![roots[0].clone()];
                editor.sidebar_root = editor.workspaces[0].clone();
                editor.tabs.clear();
                editor.active_tab_id = None;
                editor.sidebar_visible = true;
                editor.set_sidebar_width_immediately(320.0);
                editor
            })
        });
        let (_, cx) = cx.add_window_view(|_, _| RootHost(editor.clone()));
        cx.update(|_, cx| assert!(editor.read(cx).tabs.is_empty()));
        let status = cx
            .debug_bounds("editor-status-bar")
            .expect("persistent status bar");
        let strip = cx
            .debug_bounds("workspace-strip")
            .expect("status workspace strip");
        assert!(
            strip.size.width > px(0.0),
            "workspace strip must retain width inside the status bar: {strip:?}"
        );
        assert!(status.contains(&strip.center()));
        let first = Box::leak(
            format!(
                "workspace-strip-surface-{:?}",
                WorkspaceKey::Local(roots[0].clone())
            )
            .into_boxed_str(),
        );
        let first = cx.debug_bounds(first).expect("first workspace item");
        assert!(
            strip.contains(&first.center()),
            "workspace item must be inside the status strip: strip={strip:?}, item={first:?}"
        );
        let sidebar_item =
            Box::leak(format!("workspace-root-{}", roots[0].display()).into_boxed_str());
        assert!(
            cx.debug_bounds(sidebar_item).is_some(),
            "FileTree must retain its workspace item"
        );
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.workspaces.push(expected.clone());
                cx.notify();
            })
        });
        cx.run_until_parked();
        let empty = cx
            .debug_bounds("empty-workspace-content")
            .expect("empty workspace content");
        assert!(
            empty.bottom() <= status.top(),
            "empty={empty:?}, status={status:?}"
        );
        let second = cx
            .debug_bounds(Box::leak(
                format!(
                    "workspace-strip-surface-{:?}",
                    WorkspaceKey::Local(expected.clone())
                )
                .into_boxed_str(),
            ))
            .expect("second empty workspace");
        cx.simulate_mouse_down(second.center(), MouseButton::Left, Default::default());
        cx.simulate_mouse_up(second.center(), MouseButton::Left, Default::default());
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).sidebar_root, expected);
        });
    }

    #[gpui::test]
    fn offline_workspace_switches_to_its_view_and_uses_the_shared_workspace_menu(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::{AppContext as _, Entity, Render, Subscription, Window};
        use moeka_fs::path::{FilesystemId, NativePath, ResourcePath};
        struct Host(Entity<EditorView>, Subscription);
        impl Render for Host {
            fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                self.0.update(cx, |editor, cx| {
                    div()
                        .size_full()
                        .relative()
                        .child(editor.render_workspace_strip(cx))
                        .child(editor.render_context_menu(cx))
                })
            }
        }
        let root = ResourcePath {
            filesystem: FilesystemId::Ssh([3; 32]),
            path: NativePath::unix_absolute("/offline").unwrap(),
        };
        let destination = "offline-host".to_string();
        let key = workspace::remote_view_key(&destination, &root);
        let editor = cx.update(|cx| {
            cx.new(|cx| {
                let mut editor = EditorView::new_with_mode(cx, true, false, Some(""), false);
                editor.embedded_editor = false;
                editor.ui_language = UiLanguage::English;
                editor.styles.ui.spec.animations_enabled = false;
                editor.workspaces = vec![PathBuf::from("/tmp/local")];
                editor.sidebar_root = editor.workspaces[0].clone();
                editor.ssh_restore = Some(config_store::SshRestore::Connected {
                    destination: destination.clone(),
                    config_file: None,
                    root: root.clone(),
                    active: false,
                });
                editor.ssh_restore_error = Some("offline".into());
                editor
            })
        });
        let (_, cx) = cx.add_window_view(|_, cx| {
            Host(editor.clone(), cx.observe(&editor, |_, _, cx| cx.notify()))
        });
        let remote_key = WorkspaceKey::Remote {
            destination: destination.clone(),
            root: root.clone(),
        };
        let local_key = WorkspaceKey::Local(PathBuf::from("/tmp/local"));
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.reorder_workspace_key(&remote_key, &local_key, false, cx);
            })
        });
        cx.run_until_parked();
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                assert!(editor.render_remote_workspace_rows(cx).is_empty());
            })
        });
        let item = Box::leak(format!("workspace-strip-{remote_key:?}").into_boxed_str());
        let local = Box::leak(format!("workspace-strip-{local_key:?}").into_boxed_str());
        assert!(cx.debug_bounds(item).unwrap().left() < cx.debug_bounds(local).unwrap().left());
        let bounds = cx.debug_bounds(item).expect("offline workspace item");
        cx.simulate_click(bounds.center(), Default::default());
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                assert_eq!(editor.live_view.as_ref(), Some(&key));
                assert!(editor.remote_workspace_active);
                assert_eq!(editor.render_remote_workspace_rows(cx).len(), 1);
            })
        });
        let local_bounds = cx.debug_bounds(local).expect("local workspace item");
        cx.simulate_click(local_bounds.center(), Default::default());
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert!(editor.live_view.is_none());
            assert!(!editor.remote_workspace_active);
            assert_eq!(editor.sidebar_root, PathBuf::from("/tmp/local"));
        });
        let bounds = cx.debug_bounds(item).unwrap();
        cx.simulate_click(bounds.center(), Default::default());
        cx.update(|_, cx| {
            assert_eq!(editor.read(cx).live_view.as_ref(), Some(&key));
        });
        let bounds = cx.debug_bounds(item).unwrap();
        cx.simulate_mouse_down(bounds.center(), MouseButton::Right, Default::default());
        cx.simulate_mouse_up(bounds.center(), MouseButton::Right, Default::default());
        cx.update(|_, cx| {
            assert!(matches!(
                editor.read(cx).context_menu.as_ref().map(|menu| &menu.kind),
                Some(ContextMenuKind::FileTreeResource {
                    resource,
                    is_dir: true,
                    is_root: true,
                    ..
                }) if resource == &root
            ));
        });
        assert!(cx.debug_bounds("menu-item-New File").is_none());
        assert!(cx.debug_bounds("menu-item-Copy Path").is_some());
        assert!(cx.debug_bounds("menu-item-Refresh").is_some());
        let close = cx.debug_bounds("menu-item-Close Folder").unwrap();
        cx.simulate_click(close.center(), Default::default());
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert!(editor.ssh_restore.is_none());
            assert!(editor.live_view.is_none());
            assert!(!editor.remote_workspace_active);
        });
    }
}
