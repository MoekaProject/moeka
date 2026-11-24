use super::*;
use gpui::{
    AnyElement, Context, DragMoveEvent, MouseButton, SharedString, div, prelude::*, px, svg,
};

#[derive(Clone)]
struct DraggedWorkspace(PathBuf);

#[derive(Clone, Copy, PartialEq, Eq)]
enum TreeEntryKind {
    Starred,
    Workspace,
    File,
}

impl EditorView {
    fn scroll_workspace_drag(
        &mut self,
        position: gpui::Point<gpui::Pixels>,
        bounds: gpui::Bounds<gpui::Pixels>,
        cx: &mut Context<Self>,
    ) {
        let zone = self.sidebar_item_height_px();
        let delta = if !bounds.contains(&position) {
            0.0
        } else if position.y < bounds.origin.y + px(zone) {
            zone / 4.0
        } else if position.y > bounds.origin.y + bounds.size.height - px(zone) {
            -zone / 4.0
        } else {
            0.0
        };
        if delta == 0.0 {
            self.workspace_drag_scroll = None;
            return;
        }
        let handle = self.workspace_roots_scroll.clone();
        self.workspace_drag_scroll = Some(cx.spawn(async move |this, cx| {
            loop {
                let moving = this
                    .update(cx, |editor, cx| {
                        if !cx.has_active_drag() || editor.workspace_drag_scroll.is_none() {
                            return false;
                        }
                        let offset = handle.offset();
                        let y = (offset.y + px(delta)).clamp(-handle.max_offset().height, px(0.0));
                        if y == offset.y {
                            return false;
                        }
                        handle.set_offset(gpui::point(offset.x, y));
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !moving {
                    break;
                }
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(16))
                    .await;
            }
        }));
    }
    pub(crate) fn reorder_workspace(
        &mut self,
        source: &Path,
        target: &Path,
        after: bool,
        cx: &mut Context<Self>,
    ) {
        if source == target {
            return;
        }
        let Some(from) = self.workspaces.iter().position(|root| root == source) else {
            return;
        };
        if !self.workspaces.iter().any(|root| root == target) {
            return;
        }
        let root = self.workspaces.remove(from);
        let to = self
            .workspaces
            .iter()
            .position(|root| root == target)
            .unwrap()
            + usize::from(after);
        self.workspaces.insert(to, root);
        self.workspace_drop_target = None;
        self.workspace_drag_scroll = None;
        self.sidebar_drag_scroll_task = None;
        session_store::persist_session(self);
        cx.notify();
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;
    use gpui::{Entity, Render, Subscription, Window};

    struct TreeHarness(Entity<EditorView>, Subscription);

    #[gpui::test]
    fn local_and_offline_remote_stars_share_row_geometry_and_remove_action(
        cx: &mut gpui::TestAppContext,
    ) {
        use moeka_fs::path::{FilesystemId, NativePath, ResourcePath};
        let directory = tempfile::tempdir().unwrap();
        let local = directory.path().join("same.md");
        let remote = ResourcePath {
            filesystem: FilesystemId::Ssh([9; 32]),
            path: NativePath::unix_absolute("/workspace/same.md").unwrap(),
        };
        let editor = cx.update(|cx| {
            cx.new(|cx| {
                let mut editor = EditorView::new_with_mode(cx, true, false, Some(""), false);
                editor.sidebar_visible = true;
                editor.set_sidebar_width_immediately(320.0);
                editor.last_window_height = 600.0;
                editor.app_settings.starred_files = vec![local.clone()];
                editor.app_settings.starred_resources = vec![remote.clone()];
                editor.sidebar_starred_collapsed = false;
                editor.styles.ui.spec.animations_enabled = false;
                editor
            })
        });
        let (_, cx) = cx.add_window_view(|_, cx| {
            TreeHarness(editor.clone(), cx.observe(&editor, |_, _, cx| cx.notify()))
        });
        let local_id = format!("sidebar-starred-{}", local.display());
        let remote_id = format!(
            "remote-star-{}",
            super::super::remote::resource_key(&remote)
        );
        for suffix in ["", "-icon", "-label", "-remove"] {
            let local = cx
                .debug_bounds(Box::leak(format!("{local_id}{suffix}").into_boxed_str()))
                .unwrap();
            let remote = cx
                .debug_bounds(Box::leak(format!("{remote_id}{suffix}").into_boxed_str()))
                .unwrap();
            assert_eq!(local.origin.x, remote.origin.x, "{suffix}");
            assert_eq!(local.size, remote.size, "{suffix}");
            assert!(remote.origin.y > local.origin.y);
        }
        let remove = cx
            .debug_bounds(Box::leak(format!("{remote_id}-remove").into_boxed_str()))
            .unwrap();
        cx.simulate_mouse_move(remove.center(), None, Default::default());
        cx.simulate_click(remove.center(), Default::default());
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert!(!editor.app_settings.starred_resources.contains(&remote));
            assert!(editor.app_settings.is_starred_file(&local));
        });
    }

    #[gpui::test]
    fn sidebar_footer_opens_settings_help_and_direct_workspace_actions(
        cx: &mut gpui::TestAppContext,
    ) {
        let editor = cx.update(|cx| {
            cx.new(|cx| {
                let mut editor = EditorView::new_with_mode(cx, true, false, Some(""), false);
                editor.sidebar_visible = true;
                editor.set_sidebar_width_immediately(320.0);
                editor.last_window_height = 600.0;
                editor.app_settings.starred_files.clear();
                editor.ui_language = UiLanguage::English;
                editor.styles.ui.spec.animations_enabled = false;
                editor
            })
        });
        let (_, cx) = cx.add_window_view(|_, cx| {
            TreeHarness(editor.clone(), cx.observe(&editor, |_, _, cx| cx.notify()))
        });
        let settings = cx
            .debug_bounds("sidebar-settings")
            .expect("settings footer");
        assert!((f32::from(settings.bottom()) - 600.0).abs() < 2.0);
        assert!(cx.debug_bounds("sidebar-help").is_none());
        assert!(cx.debug_bounds("sidebar-add-workspace").is_none());
        cx.simulate_click(settings.center(), Default::default());
        cx.update(|_, cx| {
            assert!(matches!(
                editor.read(cx).context_menu.as_ref().map(|menu| &menu.kind),
                Some(ContextMenuKind::SidebarSettings)
            ));
        });
        let menu_settings = cx.debug_bounds("menu-item-Settings").unwrap();
        let help = cx.debug_bounds("menu-item-Help").unwrap();
        let add = cx.debug_bounds("menu-item-Add Local Folder...").unwrap();
        assert!(menu_settings.bottom() <= help.top());
        assert!(help.bottom() <= add.top());
        assert!(add.bottom() <= px(600.0));
        assert!(cx.debug_bounds("menu-item-Add Folder...").is_none());
        if crate::platform::capabilities::CHILD_PROCESSES {
            let ssh = cx
                .debug_bounds("menu-item-Add SSH Remote Folder...")
                .unwrap();
            assert!(
                add.bottom() <= ssh.top() + px(1.0),
                "local={add:?}, ssh={ssh:?}"
            );
            assert!(ssh.bottom() <= px(600.0));
        }
        cx.simulate_click(menu_settings.center(), Default::default());
        cx.update(|_, cx| {
            editor.update(cx, |editor, _| {
                assert!(editor.active_tab_is_settings());
            })
        });
        cx.simulate_click(settings.center(), Default::default());
        let help = cx.debug_bounds("menu-item-Help").unwrap();
        cx.simulate_click(help.center(), Default::default());
        cx.update(|_, cx| {
            editor.update(cx, |editor, _| {
                assert_eq!(editor.active_tab_id.as_deref(), Some(WELCOME_TAB_ID));
                assert!(!editor.active_tab_is_settings());
            })
        });
        if crate::platform::capabilities::CHILD_PROCESSES {
            cx.simulate_click(settings.center(), Default::default());
            let ssh = cx
                .debug_bounds("menu-item-Add SSH Remote Folder...")
                .unwrap();
            cx.simulate_click(ssh.center(), Default::default());
            cx.update(|_, cx| {
                let editor = editor.read(cx);
                assert!(editor.remote_input.is_some());
                assert!(editor.context_menu.is_none());
            });
        }
        for language in UiLanguage::supported() {
            for key in [
                crate::i18n::TextKey::SettingsTitle,
                crate::i18n::TextKey::HelpManual,
                crate::i18n::TextKey::OpenFolder,
            ] {
                assert!(!crate::i18n::translate(*language, key).is_empty());
            }
        }
        assert_eq!(
            crate::i18n::translate(
                UiLanguage::ChineseSimplified,
                crate::i18n::TextKey::OpenFolder
            ),
            "添加目录..."
        );
        assert_eq!(
            crate::i18n::translate(UiLanguage::English, crate::i18n::TextKey::OpenFolder),
            "Add Folder..."
        );
    }

    #[gpui::test]
    fn pending_tree_scroll_is_applied_before_row_geometry(cx: &mut gpui::TestAppContext) {
        let root = PathBuf::from("workspace");
        let editor = cx.update(|cx| {
            cx.new(|cx| {
                let mut editor = EditorView::new_with_mode(cx, true, false, Some(""), false);
                editor.workspaces = vec![root.clone()];
                editor.sidebar_root = root.clone();
                editor.expanded_dirs.insert(root.clone());
                editor.sidebar_visible = true;
                editor.sidebar_custom_width = Some(320.0);
                editor.set_sidebar_width_immediately(320.0);
                editor.last_window_height = 600.0;
                editor.app_settings.simple_mode = true;
                editor.app_settings.starred_files.clear();
                editor.sidebar_items_cache = Rc::new(
                    (0..100)
                        .map(|index| SidebarItem {
                            name: format!("folder-{index}"),
                            path: root.join(format!("folder-{index}")),
                            depth: 1,
                            is_dir: true,
                            is_expanded: false,
                        })
                        .collect(),
                );
                editor.sidebar_items_dirty = false;
                editor
            })
        });
        let (_, cx) = cx.add_window_view(|_, cx| {
            TreeHarness(editor.clone(), cx.observe(&editor, |_, _, cx| cx.notify()))
        });
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.pending_sidebar_scroll_top.set(Some(500.0));
                cx.notify();
            })
        });
        cx.run_until_parked();
        let selector =
            Box::leak(format!("sidebar-{}", root.join("folder-20").display()).into_boxed_str());
        let row = cx.debug_bounds(selector).expect("restored visible row");
        cx.update(|_, cx| {
            editor.update(cx, |editor, _| {
                let expected = editor.sidebar_scroll_handle.bounds().origin.y
                    + px(20.0 * editor.sidebar_item_height_px())
                    + editor.sidebar_scroll_handle.offset().y;
                assert_eq!(editor.sidebar_scroll_handle.offset().y, px(-500.0));
                assert!(
                    (f32::from(row.origin.y - expected)).abs() < 1.0,
                    "row={row:?}, expected={expected:?}"
                );
            })
        });
    }

    #[gpui::test]
    fn workspace_root_click_restores_cached_tree_and_selection_on_first_frame(
        cx: &mut gpui::TestAppContext,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let roots = [directory.path().join("a"), directory.path().join("b")];
        for root in &roots {
            moeka_fs::local::native::create_dir(root).unwrap();
        }
        let snapshots: Vec<Rc<Vec<SidebarItem>>> = roots
            .iter()
            .map(|root| {
                Rc::new(
                    (0..100)
                        .map(|index| SidebarItem {
                            name: format!("folder-{index}"),
                            path: root.join(format!("folder-{index}")),
                            depth: 1,
                            is_dir: true,
                            is_expanded: false,
                        })
                        .collect(),
                )
            })
            .collect();
        let editor = cx.update(|cx| {
            cx.new(|cx| {
                let mut editor = EditorView::new_with_mode(cx, true, false, Some(""), false);
                editor.workspaces = roots.to_vec();
                editor.sidebar_root = roots[0].clone();
                editor.expanded_dirs.insert(roots[0].clone());
                editor.sidebar_visible = true;
                editor.set_sidebar_width_immediately(320.0);
                editor.last_window_height = 600.0;
                editor.app_settings.simple_mode = true;
                editor.app_settings.starred_files.clear();
                editor.sidebar_items_cache = snapshots[0].clone();
                editor.sidebar_items_dirty = false;
                editor
            })
        });
        let (_, cx) = cx.add_window_view(|_, cx| {
            TreeHarness(editor.clone(), cx.observe(&editor, |_, _, cx| cx.notify()))
        });
        let selectors: Vec<&'static str> = roots
            .iter()
            .map(|root| &*Box::leak(format!("workspace-root-{}", root.display()).into_boxed_str()))
            .collect();
        let selected = snapshots[0][20].path.clone();
        let retained_handle = cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.sidebar_select_item(&selected, false, false);
                editor.pending_sidebar_scroll_top.set(None);
                editor
                    .sidebar_scroll_handle
                    .set_offset(gpui::point(px(0.0), px(-500.0)));
                editor
                    .sidebar_directory_children
                    .insert(roots[0].clone(), Vec::new());
                cx.notify();
                editor.sidebar_scroll_handle.clone()
            })
        });
        cx.run_until_parked();
        let b_bounds = cx.debug_bounds(selectors[1]).unwrap();
        cx.simulate_click(b_bounds.center(), Default::default());
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                assert_eq!(editor.sidebar_root, roots[1]);
                assert_eq!(editor.sidebar_layout_width(), 320.0);
                assert!(editor.sidebar_width_task.is_none());
                assert_eq!(retained_handle.offset().y, px(-500.0));
                editor.sidebar_items_cache = snapshots[1].clone();
                editor.sidebar_items_dirty = false;
                editor.pending_sidebar_scroll_top.set(None);
                editor.sidebar_select_item(&snapshots[1][10].path, false, false);
                editor
                    .sidebar_scroll_handle
                    .set_offset(gpui::point(px(0.0), px(-220.0)));
                cx.notify();
            })
        });
        cx.run_until_parked();
        let a_bounds = cx.debug_bounds(selectors[0]).unwrap();
        cx.simulate_click(a_bounds.center(), Default::default());
        cx.update(|_, cx| {
            editor.update(cx, |editor, _| {
                assert_eq!(editor.sidebar_root, roots[0]);
                assert!(Rc::ptr_eq(&editor.sidebar_items_cache, &snapshots[0]));
                assert_eq!(
                    editor.selected_sidebar_paths,
                    HashSet::from([selected.clone()])
                );
                assert_eq!(editor.focused_sidebar_path.as_ref(), Some(&selected));
                assert_eq!(editor.sidebar_selection_anchor.as_ref(), Some(&selected));
                assert_eq!(editor.sidebar_scroll_handle.offset().y, px(-500.0));
                assert!(editor.sidebar_directory_children.contains_key(&roots[0]));
                retained_handle.set_offset(gpui::point(px(0.0), px(-510.0)));
                assert_eq!(
                    editor.sidebar_scroll_handle.offset().y,
                    px(-510.0),
                    "keep the original ScrollHandle"
                );
            })
        });
        let b_bounds = cx.debug_bounds(selectors[1]).unwrap();
        cx.simulate_click(b_bounds.center(), Default::default());
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(20));
        cx.run_until_parked();
        cx.update(|_, cx| {
            editor.update(cx, |editor, _| {
                assert!(Rc::ptr_eq(&editor.sidebar_items_cache, &snapshots[1]));
                assert_eq!(
                    editor.selected_sidebar_paths,
                    HashSet::from([snapshots[1][10].path.clone()])
                );
                assert_eq!(editor.sidebar_scroll_handle.offset().y, px(-220.0));
                assert_eq!(
                    editor.focused_sidebar_path.as_ref(),
                    Some(&snapshots[1][10].path)
                );
                assert_eq!(editor.focused_workspace_root.as_ref(), Some(&roots[1]));
            })
        });
    }

    impl Render for TreeHarness {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            let _ = &self.1;
            self.0.update(cx, |editor, cx| {
                div()
                    .w(px(320.0))
                    .h(px(600.0))
                    .relative()
                    .child(editor.render_sidebar(false, cx))
                    .child(editor.render_context_menu(cx))
            })
        }
    }

    #[gpui::test]
    fn global_starred_files_precede_every_workspace_and_open_in_their_owner(
        cx: &mut gpui::TestAppContext,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let roots = [directory.path().join("a"), directory.path().join("b")];
        for root in &roots {
            moeka_fs::local::native::create_dir(root).unwrap();
        }
        let stars = [roots[0].join("first.md"), roots[1].join("second.md")];
        for path in &stars {
            moeka_fs::local::native::write(path, "# Starred").unwrap();
        }
        let editor = cx.update(|cx| {
            cx.new(|cx| {
                let mut editor = EditorView::new_with_mode(cx, true, false, Some(""), false);
                editor.workspaces = roots.to_vec();
                editor.sidebar_root = roots[0].clone();
                editor.expanded_dirs.insert(roots[0].clone());
                editor.sidebar_visible = true;
                editor.set_sidebar_width_immediately(320.0);
                editor.last_window_height = 600.0;
                editor.app_settings.simple_mode = true;
                editor.app_settings.starred_files = stars.to_vec();
                editor.sidebar_starred_collapsed = false;
                editor
            })
        });
        let (_, cx) = cx.add_window_view(|_, cx| {
            TreeHarness(editor.clone(), cx.observe(&editor, |_, _, cx| cx.notify()))
        });
        let star_selectors: Vec<&'static str> = stars
            .iter()
            .map(|path| &*Box::leak(format!("sidebar-starred-{}", path.display()).into_boxed_str()))
            .collect();
        let root_selectors: Vec<&'static str> = roots
            .iter()
            .map(|path| &*Box::leak(format!("workspace-root-{}", path.display()).into_boxed_str()))
            .collect();
        let starred_bounds = cx.debug_bounds("sidebar-starred-section").unwrap();
        for selector in &root_selectors {
            assert!(
                cx.debug_bounds(selector).unwrap().origin.y
                    >= starred_bounds.origin.y + starred_bounds.size.height
            );
        }
        let second = cx
            .debug_bounds(star_selectors[1])
            .expect("cross-workspace star");
        let first = cx.debug_bounds(star_selectors[0]).unwrap();
        cx.simulate_click(second.center(), Default::default());
        cx.update(|_, cx| {
            editor.update(cx, |editor, _| {
                assert_eq!(editor.sidebar_root, roots[1]);
                assert_eq!(editor.visible_starred_sidebar_files(), stars);
            })
        });
        for (selector, before) in star_selectors.iter().zip([first, second]) {
            let after = cx.debug_bounds(selector).unwrap();
            assert_eq!(after.origin.y, before.origin.y);
            assert_eq!(after.size.height, before.size.height);
        }
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.focus_workspace_row(roots[0].clone(), cx);
                editor.sidebar_nav_up(cx);
                assert_eq!(editor.focused_starred_path.as_ref(), Some(&stars[1]));
                editor.sidebar_nav_up(cx);
                assert_eq!(editor.focused_starred_path.as_ref(), Some(&stars[0]));
                editor.sidebar_confirm_focused(cx);
                assert_eq!(editor.sidebar_root, roots[0]);
            })
        });
    }

    #[gpui::test]
    fn workspace_roots_are_grouped_above_the_tree_and_keep_their_order_on_activation(
        cx: &mut gpui::TestAppContext,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let roots: Vec<_> = ["a", "b", "c", "d"]
            .iter()
            .map(|name| directory.path().join(name))
            .collect();
        for root in &roots {
            moeka_fs::local::native::create_dir(root).unwrap();
        }
        let editor = cx.update(|cx| {
            cx.new(|cx| {
                let mut editor = EditorView::new_with_mode(cx, true, false, Some(""), false);
                editor.workspaces = roots.clone();
                editor.sidebar_root = roots[1].clone();
                editor.expanded_dirs.insert(roots[1].clone());
                editor.styles.sidebar.sidebar_root_region_max_fraction = 0.5;
                editor.sidebar_visible = true;
                editor.set_sidebar_width_immediately(320.0);
                editor.last_window_height = 600.0;
                editor.app_settings.simple_mode = true;
                editor.app_settings.starred_files.clear();
                editor.sidebar_items_cache = Rc::new(
                    (0..100)
                        .map(|index| SidebarItem {
                            path: roots[1].join(format!("file-{index}.md")),
                            name: format!("file-{index}.md"),
                            depth: 1,
                            is_dir: false,
                            is_expanded: false,
                        })
                        .collect(),
                );
                editor.sidebar_items_dirty = false;
                editor
            })
        });
        let (_, cx) = cx.add_window_view(|_, cx| {
            TreeHarness(editor.clone(), cx.observe(&editor, |_, _, cx| cx.notify()))
        });
        let selectors: Vec<&'static str> = roots
            .iter()
            .map(|path| &*Box::leak(format!("workspace-root-{}", path.display()).into_boxed_str()))
            .collect();
        let before: Vec<_> = selectors
            .iter()
            .map(|name| cx.debug_bounds(name).unwrap())
            .collect();
        for rows in before.windows(2) {
            assert_eq!(rows[0].bottom(), rows[1].top());
        }
        let subtree = cx.debug_bounds("sidebar-tree-region").unwrap();
        assert!(before[3].bottom() <= subtree.top());
        for action in ["sidebar-header-sort", "sidebar-header-create"] {
            assert!(before[1].contains(&cx.debug_bounds(action).unwrap().center()));
        }
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.pending_sidebar_scroll_top.set(None);
                editor
                    .sidebar_scroll_handle
                    .set_offset(gpui::point(px(0.0), px(-500.0)));
                cx.notify();
            })
        });
        cx.run_until_parked();
        for (index, selector) in selectors.iter().enumerate() {
            assert_eq!(cx.debug_bounds(selector).unwrap(), before[index]);
        }
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.reorder_workspace(&roots[3], &roots[0], false, cx);
                assert_eq!(
                    editor.workspaces,
                    vec![
                        roots[3].clone(),
                        roots[0].clone(),
                        roots[1].clone(),
                        roots[2].clone()
                    ]
                );
                assert_eq!(editor.sidebar_root, roots[1]);
                assert_eq!(editor.sidebar_scroll_handle.offset().y, px(-500.0));
                editor.focus_workspace_row(roots[0].clone(), cx);
                editor.sidebar_nav_right(cx);
                assert_eq!(editor.sidebar_root, roots[0]);
                editor.sidebar_nav_left(cx);
                assert_eq!(editor.sidebar_root, roots[0]);
            })
        });
        cx.run_until_parked();
        let target = cx.debug_bounds(selectors[2]).unwrap();
        cx.simulate_mouse_down(target.center(), MouseButton::Right, Default::default());
        cx.simulate_mouse_up(target.center(), MouseButton::Right, Default::default());
        cx.update(|_, cx| editor.update(cx, |editor, cx| {
            assert_eq!(editor.sidebar_root, roots[0]);
            assert!(matches!(&editor.context_menu, Some(menu) if matches!(&menu.kind, ContextMenuKind::WorkspaceRoot { path } if path == &roots[2])));
            editor.close_context_menu_immediately(cx);
        }));
        cx.simulate_click(target.center(), Default::default());
        cx.update(|_, cx| {
            editor.update(cx, |editor, _| {
                assert_eq!(editor.sidebar_root, roots[2]);
            })
        });
        for action in ["sidebar-header-sort", "sidebar-header-create"] {
            assert!(
                cx.debug_bounds(selectors[2])
                    .unwrap()
                    .contains(&cx.debug_bounds(action).unwrap().center())
            );
        }
        let from = cx.debug_bounds(selectors[1]).unwrap().center();
        let target = cx.debug_bounds(selectors[3]).unwrap();
        let to = gpui::point(target.center().x, target.origin.y + px(2.0));
        cx.simulate_mouse_down(from, MouseButton::Left, Default::default());
        cx.simulate_mouse_move(
            from + gpui::point(px(8.0), px(0.0)),
            MouseButton::Left,
            Default::default(),
        );
        cx.simulate_mouse_move(to, MouseButton::Left, Default::default());
        cx.simulate_mouse_up(to, MouseButton::Left, Default::default());
        cx.update(|_, cx| {
            editor.update(cx, |editor, _| {
                assert_eq!(
                    editor.sidebar_root, roots[2],
                    "reordering must not activate a root"
                );
                assert_eq!(editor.workspaces[0], roots[1]);
            })
        });
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.styles.sidebar.sidebar_root_region_max_fraction = 0.12;
                editor.styles.sidebar.sidebar_min_tree_rows = 4.0;
                editor
                    .workspaces
                    .extend((0..20).map(|index| directory.path().join(format!("extra-{index}"))));
                cx.notify();
            })
        });
        cx.run_until_parked();
        let subtree = cx.debug_bounds("sidebar-tree-region").unwrap();
        let roots_region = cx.debug_bounds("workspace-roots").unwrap();
        cx.update(|_, cx| {
            editor.update(cx, |editor, _| {
                let available = editor.last_window_height
                    - editor.tab_bar_height()
                    - editor.scaled(editor.styles.sidebar.sidebar_header_height_px) * 2.0
                    - editor.sidebar_item_height_px() * 4.0;
                assert!(f32::from(roots_region.size.height) <= available * 0.12 + 1.0);
                assert!(f32::from(subtree.size.height) >= editor.sidebar_item_height_px() * 4.0);
                assert!(editor.workspace_roots_scroll.max_offset().height > px(0.0));
            })
        });
    }

    #[gpui::test]
    fn workspace_and_child_selection_highlights_are_mutually_exclusive(
        cx: &mut gpui::TestAppContext,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().to_path_buf();
        let child = root.join("folder");
        moeka_fs::local::native::create_dir(&child).unwrap();
        let root_resource = moeka_fs::path::ResourcePath::local(&root);
        let child_resource = moeka_fs::path::ResourcePath::local(&child);
        let editor = cx.update(|cx| {
            cx.new(|cx| {
                let mut editor = EditorView::new_with_mode(cx, true, false, Some(""), false);
                editor.workspaces = vec![root.clone()];
                editor.sidebar_root = root.clone();
                editor.sidebar_visible = true;
                editor.set_sidebar_width_immediately(320.0);
                editor.last_window_height = 600.0;
                editor.app_settings.starred_files.clear();
                editor.sidebar_items_cache = Rc::new(vec![SidebarItem {
                    name: "folder".into(),
                    path: child.clone(),
                    depth: 1,
                    is_dir: true,
                    is_expanded: false,
                }]);
                editor.sidebar_items_dirty = false;
                editor.sidebar_select_item(&child, false, false);
                editor
            })
        });
        let (_, cx) = cx.add_window_view(|_, cx| {
            TreeHarness(editor.clone(), cx.observe(&editor, |_, _, cx| cx.notify()))
        });
        let root_selector =
            &*Box::leak(format!("workspace-root-{}", root.display()).into_boxed_str());
        let child_selector = &*Box::leak(format!("sidebar-{}", child.display()).into_boxed_str());
        cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert!(!editor.file_tree_resource_selected(&root_resource));
            assert!(editor.file_tree_resource_selected(&child_resource));
        });
        let bounds = cx.debug_bounds(root_selector).unwrap();
        cx.simulate_click(bounds.center(), Default::default());
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                assert!(editor.file_tree_resource_selected(&root_resource));
                assert!(!editor.file_tree_resource_selected(&child_resource));
                assert!(!editor.file_tree_resource_focused(&child_resource));
                assert!(editor.selected_sidebar_paths.contains(&child));
                editor.sidebar_nav_right(cx);
                assert!(!editor.file_tree_resource_selected(&root_resource));
                assert!(editor.file_tree_resource_selected(&child_resource));
                assert!(editor.file_tree_resource_focused(&child_resource));
                editor.focus_workspace_row(root.clone(), cx);
            })
        });
        let bounds = cx.debug_bounds(child_selector).unwrap();
        cx.simulate_click(bounds.center(), Default::default());
        cx.update(|_, cx| {
            editor.update(cx, |editor, _| {
                assert!(!editor.file_tree_resource_selected(&root_resource));
                assert!(editor.file_tree_resource_selected(&child_resource));
                assert!(editor.file_tree_resource_focused(&child_resource));
                assert_eq!(editor.sidebar_root, root);
            })
        });
    }

    #[gpui::test]
    fn workspace_row_click_resets_only_the_visible_file_list_to_its_root(
        cx: &mut gpui::TestAppContext,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let roots = [directory.path().join("a"), directory.path().join("b")];
        for root in &roots {
            moeka_fs::local::native::create_dir(root).unwrap();
        }
        let editor = cx.update(|cx| {
            cx.new(|cx| {
                let mut editor = EditorView::new_with_mode(cx, true, false, Some(""), false);
                editor.workspaces = roots.to_vec();
                editor.sidebar_root = roots[0].clone();
                editor.sidebar_visible = true;
                editor.set_sidebar_width_immediately(320.0);
                editor.last_window_height = 600.0;
                editor.app_settings.starred_files.clear();
                editor.file_list_visible = true;
                editor.file_list_dir = roots[0].join("nested");
                editor.file_list_pending_reveal_path = Some(roots[0].join("nested/file.md"));
                editor
            })
        });
        let (_, cx) = cx.add_window_view(|_, cx| {
            TreeHarness(editor.clone(), cx.observe(&editor, |_, _, cx| cx.notify()))
        });
        let selectors: Vec<&'static str> = roots
            .iter()
            .map(|root| &*Box::leak(format!("workspace-root-{}", root.display()).into_boxed_str()))
            .collect();
        for index in [0, 1, 0] {
            let bounds = cx.debug_bounds(selectors[index]).unwrap();
            cx.simulate_click(bounds.center(), Default::default());
            cx.update(|_, cx| {
                editor.update(cx, |editor, _| {
                    assert_eq!(editor.sidebar_root, roots[index]);
                    assert_eq!(editor.file_list_dir, roots[index]);
                    assert!(editor.file_list_pending_reveal_path.is_none());
                    editor.file_list_dir = roots[index].join("nested");
                })
            });
        }
        cx.update(|_, cx| {
            editor.update(cx, |editor, cx| {
                editor.file_list_visible = false;
                editor.activate_workspace_row(roots[0].clone(), cx);
                assert!(!editor.file_list_visible);
                assert_eq!(editor.file_list_dir, roots[0].join("nested"));
                editor.focus_workspace_row(roots[0].clone(), cx);
                editor.sidebar_nav_down(cx);
                assert_eq!(editor.focused_workspace_root.as_ref(), Some(&roots[1]));
            })
        });
    }
}

impl EditorView {
    pub(super) fn activate_workspace_row(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.switch_workspace(root.clone(), cx);
        if self.file_list_visible {
            self.file_list_pending_reveal_path = None;
            self.set_file_list_dir_for_entry(&root, true, cx);
        }
        self.focus_workspace_row(root, cx);
    }

    pub(super) fn focus_workspace_row(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.pending_editor_focus = false;
        self.focused_starred_path = None;
        // Root focus is separate from the workspace's retained item selection.
        self.focused_workspace_root = Some(root.clone());
        if let Some(index) = self.workspaces.iter().position(|path| path == &root) {
            self.workspace_roots_scroll.scroll_to_item(index);
        }
        cx.notify();
    }

    pub(super) fn navigate_workspace_tree(&mut self, delta: isize, cx: &mut Context<Self>) {
        let children = self.sidebar_items();
        let mut rows = Vec::new();
        if !self.sidebar_starred_collapsed {
            rows.extend(
                self.visible_starred_sidebar_files()
                    .into_iter()
                    .map(|path| (TreeEntryKind::Starred, path)),
            );
        }
        for root in &self.workspaces {
            rows.push((TreeEntryKind::Workspace, root.clone()));
        }
        rows.extend(
            children
                .iter()
                .map(|item| (TreeEntryKind::File, item.path.clone())),
        );
        if rows.is_empty() {
            return;
        }
        let current = if let Some(root) = &self.focused_workspace_root {
            rows.iter()
                .position(|(kind, path)| *kind == TreeEntryKind::Workspace && path == root)
        } else {
            self.focused_sidebar_path.as_ref().and_then(|path| {
                let target_kind = if self.focused_starred_path.as_ref() == Some(path) {
                    TreeEntryKind::Starred
                } else {
                    TreeEntryKind::File
                };
                rows.iter()
                    .position(|(kind, item)| *kind == target_kind && path == item)
            })
        };
        let next = current.map_or(if delta < 0 { rows.len() - 1 } else { 0 }, |index| {
            index.saturating_add_signed(delta).min(rows.len() - 1)
        });
        let (kind, path) = rows[next].clone();
        if kind == TreeEntryKind::Workspace {
            self.focus_workspace_row(path, cx);
        } else if kind == TreeEntryKind::Starred {
            self.select_starred_sidebar_item(&path, false, false);
            self.ensure_starred_path_visible(&path);
            cx.notify();
        } else {
            self.focused_starred_path = None;
            self.focused_workspace_root = None;
            self.focused_sidebar_path = Some(path.clone());
            if let Some(index) = children.iter().position(|item| item.path == path) {
                self.ensure_sidebar_item_minimally_visible(index, children.len());
            }
            cx.notify();
        }
    }

    pub(super) fn render_workspace_root(
        &self,
        root: PathBuf,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = !self.remote_workspace_active && root == self.sidebar_root;
        let ss = &self.styles.sidebar;
        let name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string());
        let selected =
            self.file_tree_resource_selected(&moeka_fs::path::ResourcePath::local(&root));
        let label_style = self.styles.workspace_root_label(active);
        let icon_color = if active {
            ss.sidebar_item_selected_icon_color
        } else {
            ss.sidebar_item_icon_color
        };
        let mut row = crate::ui::list::tree_row(
            self.styles.sidebar_tree_row(),
            crate::style::TreeRowState {
                selected,
                focused: false,
                drag_over: false,
                cut: false,
                resizing: self.sidebar_resizing,
            },
            self.ui_scale,
        )
        .opacity(if active { 1.0 } else { 0.5 })
        .id(SharedString::from(format!(
            "workspace-root-{}",
            root.display()
        )))
        .debug_selector(|| format!("workspace-root-{}", root.display()))
        .pl(px(self.scaled(ss.sidebar_root_padding_x_px)))
        .pr(px(self.scaled(ss.sidebar_item_padding_right_px)))
        .flex_none()
        .flex()
        .items_center()
        .gap(px(self.scaled(ss.sidebar_item_gap_px)))
        .child(
            svg()
                .path(ROOT_SVG_PATH)
                .size(px(self.scaled(ss.sidebar_header_root_icon_size_px)))
                .flex_none()
                .text_color(icon_color),
        )
        .child(
            crate::ui::typography::label(label_style, self.ui_scale)
                .min_w(px(0.0))
                .flex_1()
                .truncate()
                .child(name.clone()),
        )
        .when(active, |this| this.child(self.render_workspace_actions(cx)))
        .on_mouse_down(MouseButton::Left, {
            let root = root.clone();
            cx.listener(move |this, _, window, cx| {
                cx.stop_propagation();
                window.prevent_default();
                this.focus_workspace_row(root.clone(), cx);
                window.focus(&this.focus_handle);
            })
        })
        .on_click({
            let root = root.clone();
            cx.listener(move |this, _, _, cx| {
                this.activate_workspace_row(root.clone(), cx);
                cx.stop_propagation();
            })
        })
        .on_mouse_down(MouseButton::Right, {
            let root = root.clone();
            cx.listener(move |this, event: &gpui::MouseDownEvent, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
                this.focus_workspace_row(root.clone(), cx);
                this.open_context_menu(
                    ContextMenu {
                        position: event.position,
                        kind: ContextMenuKind::WorkspaceRoot { path: root.clone() },
                    },
                    true,
                    cx,
                );
            })
        })
        .on_drag(DraggedWorkspace(root.clone()), {
            let style = self.styles.drag_preview(false, self.ui_scale);
            move |_, offset, _, cx| {
                cx.new(|_| DraggedSidebarItemsView {
                    label: name.clone(),
                    count: 1,
                    style: style.clone(),
                    cursor_offset: offset,
                })
            }
        })
        .on_drag_move({
            let root = root.clone();
            cx.listener(
                move |this, event: &DragMoveEvent<DraggedWorkspace>, _, cx| {
                    if !event.bounds.contains(&event.event.position) {
                        return;
                    }
                    let after = event.event.position.y > event.bounds.center().y;
                    let target = Some((root.clone(), after));
                    if this.workspace_drop_target != target {
                        this.workspace_drop_target = target;
                        cx.notify();
                    }
                },
            )
        })
        .on_drop({
            let root = root.clone();
            cx.listener(move |this, dragged: &DraggedWorkspace, _, cx| {
                let after = this
                    .workspace_drop_target
                    .as_ref()
                    .is_some_and(|(path, after)| path == &root && *after);
                this.reorder_workspace(&dragged.0, &root, after, cx);
            })
        });
        if let Some((target, after)) = &self.workspace_drop_target
            && target == &root
        {
            row = row
                .border_color(ss.sidebar_item_selected_text_color)
                .when(*after, |this| this.border_b(px(1.0)))
                .when(!*after, |this| this.border_t(px(1.0)));
        }
        if active {
            row = row
                .on_drop({
                    let root = root.clone();
                    cx.listener(move |this, dragged: &DraggedSidebarItems, _, cx| {
                        this.clear_sidebar_drag_state(cx);
                        this.move_sidebar_items(dragged.paths.clone(), root.clone(), cx);
                    })
                })
                .on_drop({
                    let root = root.clone();
                    cx.listener(move |this, dragged: &gpui::ExternalPaths, _, cx| {
                        this.copy_external_paths_into_dir(dragged.paths(), root.clone(), cx);
                    })
                });
        }
        row.into_any_element()
    }

    pub(super) fn render_workspace_roots(&self, cx: &mut Context<Self>) -> AnyElement {
        let available = (self.last_window_height
            - self.tab_bar_height()
            - self.scaled(self.styles.sidebar.sidebar_header_height_px)
            - self.sidebar_item_height_px()
            - self.styles.workspace_roots_separator().thickness_px
            - self.sidebar_starred_section_height(!self.sidebar_root.as_os_str().is_empty())
            - self.sidebar_item_height_px() * self.styles.sidebar.sidebar_min_tree_rows)
            .max(0.0);
        div()
            .id("workspace-roots")
            .debug_selector(|| "workspace-roots".into())
            .w_full()
            .min_h(px(0.0))
            .flex_none()
            .max_h(px(available
                * self
                    .styles
                    .sidebar
                    .sidebar_root_region_max_fraction
                    .clamp(0.0, 0.5)))
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .track_scroll(&self.workspace_roots_scroll)
            .on_drag_move(cx.listener(
                move |this, event: &DragMoveEvent<DraggedWorkspace>, _, cx| {
                    this.scroll_workspace_drag(event.event.position, event.bounds, cx);
                },
            ))
            .children(
                self.workspaces
                    .iter()
                    .map(|root| self.render_workspace_root(root.clone(), cx)),
            )
            .when(self.remote_workspace.is_some(), |this| {
                this.child(self.render_remote_workspace_root(cx))
            })
            .into_any_element()
    }
}
