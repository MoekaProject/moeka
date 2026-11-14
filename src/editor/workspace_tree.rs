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
        before: bool,
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
            if self
                .workspace_drag_scroll
                .as_ref()
                .is_some_and(|(region, _)| *region == before)
            {
                self.workspace_drag_scroll = None;
            }
            return;
        }
        let handle = if before {
            self.workspace_top_scroll.clone()
        } else {
            self.workspace_bottom_scroll.clone()
        };
        self.workspace_drag_scroll = Some((
            before,
            cx.spawn(async move |this, cx| {
                loop {
                    let moving = this
                        .update(cx, |editor, cx| {
                            if !cx.has_active_drag() || editor.workspace_drag_scroll.is_none() {
                                return false;
                            }
                            let offset = handle.offset();
                            let y =
                                (offset.y + px(delta)).clamp(-handle.max_offset().height, px(0.0));
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
            }),
        ));
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
    fn sidebar_settings_and_help_precede_add_folder_and_open_their_tabs(
        cx: &mut gpui::TestAppContext,
    ) {
        let editor = cx.update(|cx| {
            cx.new(|cx| {
                let mut editor = EditorView::new_with_mode(cx, true, false, Some(""), false);
                editor.sidebar_visible = true;
                editor.set_sidebar_width_immediately(320.0);
                editor.last_window_height = 600.0;
                editor.app_settings.starred_files.clear();
                editor
            })
        });
        let (_, cx) = cx.add_window_view(|_, cx| {
            TreeHarness(editor.clone(), cx.observe(&editor, |_, _, cx| cx.notify()))
        });
        let settings = cx
            .debug_bounds("sidebar-settings")
            .expect("settings button");
        let help = cx.debug_bounds("sidebar-help").expect("help button");
        let add = cx
            .debug_bounds("sidebar-add-workspace")
            .expect("add folder button");
        assert_eq!(settings.origin.y, help.origin.y);
        assert_eq!(help.origin.y, add.origin.y);
        assert!(settings.origin.x + settings.size.width <= help.origin.x);
        assert!(help.origin.x + help.size.width <= add.origin.x);
        cx.simulate_click(settings.center(), Default::default());
        cx.update(|_, cx| {
            editor.update(cx, |editor, _| {
                assert!(editor.active_tab_is_settings());
            })
        });
        cx.simulate_click(help.center(), Default::default());
        cx.update(|_, cx| {
            editor.update(cx, |editor, _| {
                assert_eq!(editor.active_tab_id.as_deref(), Some(WELCOME_TAB_ID));
                assert!(!editor.active_tab_is_settings());
            })
        });
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
        let row = cx
            .debug_bounds("sidebar-workspace/folder-20")
            .expect("restored visible row");
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
            std::fs::create_dir(root).unwrap();
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
                    .child(editor.render_sidebar(false, cx))
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
            std::fs::create_dir(root).unwrap();
        }
        let stars = [roots[0].join("first.md"), roots[1].join("second.md")];
        for path in &stars {
            std::fs::write(path, "# Starred").unwrap();
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
    fn workspace_roots_stay_fixed_while_subtree_scrolls(cx: &mut gpui::TestAppContext) {
        let directory = tempfile::tempdir().unwrap();
        let roots: Vec<_> = ["a", "b", "c", "d"]
            .iter()
            .map(|name| directory.path().join(name))
            .collect();
        for root in &roots {
            std::fs::create_dir(root).unwrap();
        }
        let editor = cx.update(|cx| {
            cx.new(|cx| {
                let mut editor = EditorView::new_with_mode(cx, true, false, Some(""), false);
                editor.workspaces = roots.clone();
                editor.sidebar_root = roots[1].clone();
                editor.expanded_dirs.insert(roots[1].clone());
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
        assert!(before[0].origin.y < before[1].origin.y);
        assert!(before[1].origin.y + before[1].size.height < before[2].origin.y);
        let bottom_region = cx.debug_bounds("workspace-roots-bottom").unwrap();
        cx.update(|_, cx| {
            editor.update(cx, |editor, _| {
                assert_eq!(
                    before[2].origin.y - bottom_region.origin.y,
                    px(editor.styles.workspace_roots_separator().thickness_px),
                );
            })
        });
        assert!((f32::from(before[3].origin.y + before[3].size.height) - 600.0).abs() < 2.0);
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
        let bottom = cx.debug_bounds("workspace-roots-bottom").unwrap();
        cx.update(|_, cx| {
            editor.update(cx, |editor, _| {
                let available = editor.last_window_height
                    - editor.tab_bar_height()
                    - editor.scaled(editor.styles.sidebar.sidebar_header_height_px) * 2.0
                    - editor.sidebar_item_height_px() * 5.0;
                assert!(f32::from(bottom.size.height) <= available * 0.12 + 1.0);
                assert!(f32::from(subtree.size.height) >= editor.sidebar_item_height_px() * 4.0);
                assert!(editor.workspace_bottom_scroll.max_offset().height > px(0.0));
            })
        });
    }
}

impl EditorView {
    pub(super) fn focus_workspace_row(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        self.pending_editor_focus = false;
        self.focused_starred_path = None;
        // Root focus is separate from the workspace's retained item selection.
        self.focused_workspace_root = Some(root.clone());
        if let (Some(index), Some(active)) = (
            self.workspaces.iter().position(|path| path == &root),
            self.workspaces
                .iter()
                .position(|path| path == &self.sidebar_root),
        ) {
            if index < active {
                self.workspace_top_scroll.scroll_to_item(index);
            }
            if index > active {
                self.workspace_bottom_scroll
                    .scroll_to_item(index - active - 1);
            }
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
            if root == &self.sidebar_root {
                rows.extend(
                    children
                        .iter()
                        .map(|item| (TreeEntryKind::File, item.path.clone())),
                );
            }
        }
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
        let active = root == self.sidebar_root;
        let ss = &self.styles.sidebar;
        let name = root
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| root.display().to_string());
        let focused = self.focused_workspace_root.as_ref() == Some(&root);
        let label_style = self.styles.workspace_root_label(active);
        let icon_color = if active {
            ss.sidebar_item_selected_icon_color
        } else {
            ss.sidebar_item_icon_color
        };
        let mut row = crate::ui::list::tree_row(
            self.styles.sidebar_tree_row(),
            crate::style::TreeRowState {
                selected: false,
                focused: focused && !active,
                drag_over: false,
                cut: false,
                resizing: self.sidebar_resizing,
            },
            self.ui_scale,
        )
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
                this.switch_workspace(root.clone(), cx);
                this.focus_workspace_row(root.clone(), cx);
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

    pub(super) fn render_workspace_roots(
        &self,
        before: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let active = self
            .workspaces
            .iter()
            .position(|root| root == &self.sidebar_root)
            .unwrap_or(0);
        let roots = if before {
            &self.workspaces[..active]
        } else {
            &self.workspaces[(active + 1).min(self.workspaces.len())..]
        };
        let scroll = if before {
            &self.workspace_top_scroll
        } else {
            &self.workspace_bottom_scroll
        };
        let available = (self.last_window_height
            - self.tab_bar_height()
            - self.scaled(self.styles.sidebar.sidebar_header_height_px)
            - self.sidebar_starred_section_height(!self.sidebar_root.as_os_str().is_empty())
            - self.sidebar_item_height_px() * (1.0 + self.styles.sidebar.sidebar_min_tree_rows))
            .max(0.0);
        let separator = self.styles.workspace_roots_separator();
        div()
            .id(if before {
                "workspace-roots-top"
            } else {
                "workspace-roots-bottom"
            })
            .debug_selector(|| {
                if before {
                    "workspace-roots-top"
                } else {
                    "workspace-roots-bottom"
                }
                .into()
            })
            .w_full()
            .min_h(px(0.0))
            .flex_none()
            .when(!before && !roots.is_empty(), |this| {
                this.border_t(px(separator.thickness_px))
                    .border_color(separator.color)
            })
            .max_h(px(available
                * self
                    .styles
                    .sidebar
                    .sidebar_root_region_max_fraction
                    .clamp(0.0, 0.5)))
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .track_scroll(scroll)
            .on_drag_move(cx.listener(
                move |this, event: &DragMoveEvent<DraggedWorkspace>, _, cx| {
                    this.scroll_workspace_drag(before, event.event.position, event.bounds, cx);
                },
            ))
            .children(
                roots
                    .iter()
                    .map(|root| self.render_workspace_root(root.clone(), cx)),
            )
            .into_any_element()
    }
}
