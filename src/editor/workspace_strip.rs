//! With several workspaces open, they are listed as a tab-like strip above the
//! panes' tab bars; the sidebar then shows only the active workspace.

use std::path::PathBuf;

use gpui::{
    AnyElement, Context, DragMoveEvent, MouseButton, MouseDownEvent, SharedString, div, prelude::*,
    px,
};

use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum WorkspaceKey {
    Local(PathBuf),
    /// Connection order of an SSH workspace.
    Remote(u64),
    /// Index into the saved SSH workspaces still connecting.
    Pending(usize),
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
            + usize::from(self.remote_workspace.is_some())
            + self.parked_remote_workspaces.len()
            + self.pending_ssh_workspace_count()
    }

    /// Two or more workspaces move the workspace list out of the sidebar.
    pub(crate) fn workspace_strip_visible(&self) -> bool {
        !self.embedded_editor && self.workspace_count() > 1
    }

    /// Whether the sidebar lists this local workspace (all, unless the strip does).
    pub(super) fn sidebar_lists_local_workspace(&self, root: &std::path::Path) -> bool {
        !self.workspace_strip_visible()
            || (!self.remote_workspace_active && root == self.sidebar_root)
    }

    fn workspace_strip_items(&self) -> Vec<StripItem> {
        let mut items = Vec::new();
        for root in &self.workspaces {
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
        for (serial, parked, workspace) in self.remote_workspaces_in_order() {
            items.push(StripItem {
                key: WorkspaceKey::Remote(serial),
                title: workspace
                    .root
                    .path
                    .file_name()
                    .map(|name| name.to_string())
                    .unwrap_or_else(|| workspace.root.path.to_string())
                    .to_uppercase(),
                active: parked.is_none() && self.remote_workspace_active,
                remote: true,
                status: workspace.reconnecting.then(|| {
                    crate::i18n::remote_text(self.ui_language, "reconnecting").to_string()
                }),
            });
        }
        for (index, (name, error)) in self.pending_ssh_workspace_labels().into_iter().enumerate() {
            let state = crate::i18n::remote_text(
                self.ui_language,
                if error { "offline" } else { "reconnecting" },
            );
            items.push(StripItem {
                key: WorkspaceKey::Pending(index),
                title: name,
                active: false,
                remote: true,
                status: Some(state.to_string()),
            });
        }
        items
    }

    pub(crate) fn render_workspace_strip(&self, cx: &mut Context<Self>) -> AnyElement {
        let ts = &self.styles.tab_bar;
        let surface = self.styles.tab_surface();
        let height = self.tab_bar_height();
        let inner_height = (height - ts.tab_bar_border_width_px).max(0.0);
        let leading = super::render::sidebar_button_tab_bar_inset(
            self.window_controls_leading_inset(),
            height,
            self.sidebar_width() + self.file_list_width() + self.workspace_search_panel_width(),
        );
        let drop_color = ts.tab_drop_indicator_color;
        let drop_width = ts.tab_drop_indicator_width_px;
        let items = self.workspace_strip_items();
        let ghost_style = self.styles.drag_preview(true, self.ui_scale);
        let drop_target = self.workspace_strip_drop_target.clone();
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
            let row =
                crate::ui::chrome::tab_surface(surface, self.ui_scale, inner_height, item.active)
                    .when(item.remote, |this| {
                        this.child(
                            crate::ui::typography::badge(
                                self.styles.sidebar_kind_badge(false),
                                self.ui_scale,
                            )
                            .flex_none()
                            .child("REMOTE"),
                        )
                    })
                    .child(
                        crate::ui::decoration::file_name(
                            self.styles.tab_name(item.active, false, false),
                            self.ui_scale,
                        )
                        .child(label),
                    )
                    .when(!item.active, |this| {
                        this.child(
                            div()
                                .absolute()
                                .left_0()
                                .right_0()
                                .bottom_0()
                                .h(px(ts.tab_bar_border_width_px))
                                .bg(ts.tab_bar_border_color),
                        )
                    });
            div()
                .id(SharedString::from(format!("workspace-strip-{key:?}")))
                .flex_none()
                .h(px(inner_height))
                .min_w(px(self.scaled(ts.tab_item_min_width_px)))
                .max_w(px(self.scaled(ts.tab_item_max_width_px)))
                .when_some(indicator, |this, after| {
                    if after {
                        this.border_r(px(drop_width)).border_color(drop_color)
                    } else {
                        this.border_l(px(drop_width)).border_color(drop_color)
                    }
                })
                .child(row)
                .on_mouse_down(MouseButton::Left, {
                    let key = key.clone();
                    cx.listener(move |this, _, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                        this.activate_workspace_key(key.clone(), cx);
                    })
                })
                .on_mouse_down(MouseButton::Right, {
                    let key = key.clone();
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                        this.open_workspace_key_menu(key.clone(), event, window, cx);
                    })
                })
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
                })
                .into_any_element()
        });
        div()
            .id("workspace-strip")
            .debug_selector(|| "workspace-strip".into())
            .relative()
            .w_full()
            .h(px(height))
            .flex_none()
            .flex()
            .items_start()
            .bg(ts.tab_bar_bg)
            .pl(px(leading))
            .pr(px(self.window_caption_trailing_inset()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(Self::on_tab_bar_drag_space_mouse_down),
            )
            .child(
                div()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .h(px(ts.tab_bar_border_width_px))
                    .bg(ts.tab_bar_border_color),
            )
            .child(
                div()
                    .id("workspace-strip-scroll")
                    .flex_1()
                    .h_full()
                    .min_w(px(0.0))
                    .flex()
                    .items_start()
                    .overflow_x_scroll()
                    .track_scroll(&self.workspace_strip_scroll)
                    .children(children)
                    .child(div().flex_1().min_w(px(0.0)).h_full()),
            )
            .into_any_element()
    }

    fn activate_workspace_key(&mut self, key: WorkspaceKey, cx: &mut Context<Self>) {
        match key {
            WorkspaceKey::Local(root) => self.activate_workspace_row(root, cx),
            WorkspaceKey::Remote(serial) => self.activate_remote_workspace_serial(serial, cx),
            WorkspaceKey::Pending(index) => self.retry_pending_ssh_workspace_at(index, cx),
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
                self.open_context_menu(
                    ContextMenu {
                        position: event.position,
                        kind: ContextMenuKind::WorkspaceRoot { path: root },
                    },
                    true,
                    cx,
                );
            }
            WorkspaceKey::Remote(serial) => {
                self.activate_remote_workspace_serial(serial, cx);
                if let Some(root) = self.remote_workspace.as_ref().map(|w| w.root.clone()) {
                    self.open_file_tree_resource_context_menu(root, true, event, window, cx);
                }
            }
            WorkspaceKey::Pending(_) => {}
        }
    }

    fn reorder_workspace_key(
        &mut self,
        source: &WorkspaceKey,
        target: &WorkspaceKey,
        after: bool,
        cx: &mut Context<Self>,
    ) {
        match (source, target) {
            (WorkspaceKey::Local(source), WorkspaceKey::Local(target)) => {
                self.reorder_workspace(source, target, after, cx);
            }
            (WorkspaceKey::Remote(source), WorkspaceKey::Remote(target)) => {
                self.reorder_remote_workspaces(*source, *target, after);
                session_store::persist_session(self);
            }
            // Local and SSH workspaces keep their own groups.
            _ => {}
        }
        cx.notify();
    }
}

#[cfg(all(test, feature = "gpui-test-support"))]
mod tests {
    use super::*;

    #[gpui::test]
    fn strip_appears_with_two_workspaces_and_sidebar_lists_only_the_active_one(
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
            assert!(!editor.workspace_strip_visible());
            assert!(editor.sidebar_lists_local_workspace(&a));
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
            assert_eq!(editor.workspaces, vec![b, a]);
        });
    }

    #[gpui::test]
    fn strip_renders_for_several_workspaces(cx: &mut gpui::TestAppContext) {
        use gpui::{AppContext as _, Entity, Render, Subscription, Window};
        struct Host(Entity<EditorView>, Subscription);
        impl Render for Host {
            fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
                self.0
                    .update(cx, |editor, cx| editor.render_workspace_strip(cx))
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
    }
}
