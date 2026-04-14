use std::path::{Path, PathBuf};

use gpui::{
    AnyElement, Context, CursorStyle, FontWeight, MouseButton, MouseDownEvent, SharedString,
    Transformation, div, img, prelude::*, px, radians, svg,
};

use super::*;

impl EditorView {
    pub(crate) fn toggle_sidebar_directory(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.selected_sidebar_path = Some(path.clone());
        if !self.expanded_dirs.insert(path.clone()) {
            self.expanded_dirs.remove(&path);
        }
        cx.notify();
    }

    pub(crate) fn open_sidebar_file(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !is_markdown_file(&path) {
            self.selected_sidebar_path = Some(path);
            cx.notify();
            return;
        }

        if let Some(index) = self.tabs.iter().position(|tab| {
            tab.path
                .as_ref()
                .is_some_and(|open_path| open_path == &path)
        }) {
            self.selected_sidebar_path = Some(path);
            self.activate_tab_by_index(index, cx);
            return;
        }

        match std::fs::read(&path) {
            Ok(bytes) => {
                let text = if bytes.contains(&0) {
                    format!(
                        "# Binary File\n\n{}\n\nThis file could not be opened as plain text.",
                        path.display()
                    )
                } else {
                    String::from_utf8_lossy(&bytes).into_owned()
                };
                let tab = OpenTab {
                    id: self.next_tab_identity(),
                    title: Self::title_for_path(&path),
                    path: Some(path.clone()),
                    snapshot: EditorSnapshot {
                        text,
                        cursor_offset: 0,
                        cursor_anchor: 0,
                    },
                    is_dirty: false,
                };
                self.persist_active_tab_state();
                self.tabs.push(tab);
                self.selected_sidebar_path = Some(path.clone());
                self.sidebar_error = None;
                if let Some(index) = self.tabs.len().checked_sub(1) {
                    self.activate_tab_by_index(index, cx);
                }
            }
            Err(error) => {
                self.sidebar_error = Some(format!("Failed to open file: {error}"));
                self.selected_sidebar_path = Some(path);
                cx.notify();
            }
        }
    }

    pub(crate) fn sorted_dir_entries(path: &Path) -> std::io::Result<Vec<std::fs::DirEntry>> {
        let mut entries: Vec<_> = std::fs::read_dir(path)?.filter_map(Result::ok).collect();
        entries.sort_by(|left, right| {
            let left_is_dir = left.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            let right_is_dir = right.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            right_is_dir.cmp(&left_is_dir).then_with(|| {
                left.file_name()
                    .to_string_lossy()
                    .to_lowercase()
                    .cmp(&right.file_name().to_string_lossy().to_lowercase())
            })
        });
        Ok(entries)
    }

    pub(crate) fn collect_sidebar_items(
        &self,
        path: &Path,
        depth: usize,
        items: &mut Vec<SidebarItem>,
    ) {
        let Ok(entries) = Self::sorted_dir_entries(path) else {
            return;
        };

        for entry in entries {
            let entry_path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let is_dir = file_type.is_dir();
            if !is_dir && !is_markdown_file(&entry_path) {
                continue;
            }
            let is_expanded = is_dir && self.expanded_dirs.contains(&entry_path);
            items.push(SidebarItem {
                name: entry.file_name().to_string_lossy().into_owned(),
                path: entry_path.clone(),
                depth,
                is_dir,
                is_expanded,
            });
            if is_expanded {
                self.collect_sidebar_items(&entry_path, depth + 1, items);
            }
        }
    }

    pub(crate) fn sidebar_items(&self) -> Vec<SidebarItem> {
        let mut items = Vec::new();
        if self.sidebar_error.is_none() && self.sidebar_root.exists() {
            self.collect_sidebar_items(&self.sidebar_root, 1, &mut items);
        }
        items
    }

    // ── Sidebar edit operations ─────────────────────────────────────

    pub(crate) fn start_rename(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let current_name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.sidebar_edit = Some(SidebarEditMode::Rename {
            path,
            value: current_name,
        });
        self.context_menu = None;
        cx.notify();
    }

    pub(crate) fn start_create_file(&mut self, parent: PathBuf, cx: &mut Context<Self>) {
        self.expanded_dirs.insert(parent.clone());
        self.sidebar_edit = Some(SidebarEditMode::CreateFile {
            parent,
            value: String::new(),
        });
        self.context_menu = None;
        cx.notify();
    }

    pub(crate) fn start_create_dir(&mut self, parent: PathBuf, cx: &mut Context<Self>) {
        self.expanded_dirs.insert(parent.clone());
        self.sidebar_edit = Some(SidebarEditMode::CreateDir {
            parent,
            value: String::new(),
        });
        self.context_menu = None;
        cx.notify();
    }

    pub(crate) fn commit_sidebar_edit(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = self.sidebar_edit.take() else {
            return;
        };
        match edit {
            SidebarEditMode::Rename {
                ref path,
                ref value,
            } => {
                let trimmed = value.trim();
                if !trimmed.is_empty()
                    && trimmed != path.file_name().and_then(|n| n.to_str()).unwrap_or("")
                {
                    let new_path = path.with_file_name(trimmed);
                    if let Err(e) = std::fs::rename(path, &new_path) {
                        self.sidebar_error = Some(format!("Rename failed: {e}"));
                    } else {
                        for tab in &mut self.tabs {
                            if tab.path.as_deref() == Some(path.as_path()) {
                                tab.path = Some(new_path.clone());
                                tab.title = Self::title_for_path(&new_path);
                            }
                        }
                        if self.selected_sidebar_path.as_deref() == Some(path.as_path()) {
                            self.selected_sidebar_path = Some(new_path.clone());
                        }
                        if self.current_file_path.as_deref() == Some(path.as_path()) {
                            self.current_file_path = Some(new_path);
                        }
                    }
                }
            }
            SidebarEditMode::CreateFile {
                ref parent,
                ref value,
            } => {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    let new_path = parent.join(trimmed);
                    if let Err(e) = std::fs::File::create(&new_path) {
                        self.sidebar_error = Some(format!("Create file failed: {e}"));
                    } else if is_markdown_file(&new_path) {
                        self.open_sidebar_file(new_path, cx);
                        return;
                    }
                }
            }
            SidebarEditMode::CreateDir {
                ref parent,
                ref value,
            } => {
                let trimmed = value.trim();
                if !trimmed.is_empty() {
                    let new_path = parent.join(trimmed);
                    if let Err(e) = std::fs::create_dir(&new_path) {
                        self.sidebar_error = Some(format!("Create folder failed: {e}"));
                    } else {
                        self.expanded_dirs.insert(new_path);
                    }
                }
            }
        }
        cx.notify();
    }

    pub(crate) fn cancel_sidebar_edit(&mut self, cx: &mut Context<Self>) {
        if self.sidebar_edit.take().is_some() {
            cx.notify();
        }
    }

    pub(crate) fn delete_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        let result = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
        if let Err(e) = result {
            self.sidebar_error = Some(format!("Delete failed: {e}"));
        } else {
            self.tabs
                .retain(|tab| tab.path.as_ref().is_none_or(|p| !p.starts_with(&path)));
            if self.tabs.is_empty() {
                self.create_scratch_tab(cx);
                return;
            }
            let active_still_exists = self
                .active_tab_id
                .as_ref()
                .is_some_and(|id| self.tabs.iter().any(|t| &t.id == id));
            if !active_still_exists {
                self.activate_tab_by_index(0, cx);
            }
        }
        self.context_menu = None;
        cx.notify();
    }

    pub(crate) fn cut_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.file_clipboard = Some(FileClipboard {
            paths: vec![path],
            is_cut: true,
        });
        self.context_menu = None;
        cx.notify();
    }

    pub(crate) fn copy_path(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        self.file_clipboard = Some(FileClipboard {
            paths: vec![path],
            is_cut: false,
        });
        self.context_menu = None;
        cx.notify();
    }

    pub(crate) fn paste_into_dir(&mut self, target_dir: PathBuf, cx: &mut Context<Self>) {
        let Some(clipboard) = self.file_clipboard.take() else {
            return;
        };
        for src in &clipboard.paths {
            let Some(name) = src.file_name() else {
                continue;
            };
            let dst = target_dir.join(name);
            if clipboard.is_cut {
                if let Err(e) = std::fs::rename(src, &dst) {
                    self.sidebar_error = Some(format!("Move failed: {e}"));
                } else {
                    for tab in &mut self.tabs {
                        if tab.path.as_deref() == Some(src.as_path()) {
                            tab.path = Some(dst.clone());
                            tab.title = Self::title_for_path(&dst);
                        }
                    }
                }
            } else {
                if src.is_file() {
                    if let Err(e) = std::fs::copy(src, &dst) {
                        self.sidebar_error = Some(format!("Copy failed: {e}"));
                    }
                }
            }
        }
        self.context_menu = None;
        cx.notify();
    }

    pub(crate) fn reveal_in_finder(&self, path: &Path) {
        let _ = std::process::Command::new("open")
            .arg("--reveal")
            .arg(path)
            .spawn();
    }

    pub(crate) fn pick_workspace_folder(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let maybe_path = cx
                .background_executor()
                .spawn(async move { helpers::pick_folder_blocking() })
                .await;
            if let Some(path) = maybe_path {
                this.update(cx, |editor, cx| {
                    editor.sidebar_root = path.clone();
                    editor.expanded_dirs.clear();
                    editor.expanded_dirs.insert(path.clone());
                    editor.sidebar_error = None;
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
    }

    pub(crate) fn sidebar_edit_append(&mut self, text: &str, cx: &mut Context<Self>) {
        if let Some(ref mut edit) = self.sidebar_edit {
            edit.value_mut().push_str(text);
            cx.notify();
        }
    }

    pub(crate) fn sidebar_edit_backspace(&mut self, cx: &mut Context<Self>) {
        if let Some(ref mut edit) = self.sidebar_edit {
            let val = edit.value_mut();
            if let Some((last_char_start, _)) = val.char_indices().next_back() {
                val.truncate(last_char_start);
            }
            cx.notify();
        }
    }

    // ── Sidebar rendering ───────────────────────────────────────────

    pub(crate) fn render_sidebar_edit_row(
        &self,
        value: &str,
        depth: usize,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let ss = &self.styles.sidebar;
        let left_padding = self.scaled(ss.sidebar_item_indent_base_px + depth as f32 * ss.sidebar_item_indent_step_px + 10.0 + 6.0 + 13.0 + 6.0);
        div()
            .w_full()
            .h(px(self.sidebar_row_height()))
            .pl(px(self.scaled(8.0)))
            .pr(px(self.scaled(10.0)))
            .flex()
            .items_center()
            .bg(ss.sidebar_item_selected_bg)
            .child(
                div()
                    .flex_1()
                    .pl(px(left_padding))
                    .pr(px(self.scaled(2.0)))
                    .h_full()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .h(px(ss.sidebar_edit_input_height_px))
                            .px(px(ss.sidebar_edit_input_padding_x_px))
                            .flex()
                            .items_center()
                            .bg(ss.sidebar_edit_input_bg)
                            .border(px(ss.sidebar_edit_input_border_width_px))
                            .border_color(ss.sidebar_edit_input_border_color)
                            .rounded(px(ss.sidebar_edit_input_radius_px))
                            .text_size(px(ss.sidebar_edit_input_font_size_px))
                            .text_color(ss.sidebar_edit_input_text_color)
                            .child(div().flex_1().text_ellipsis().child(if value.is_empty() {
                                div().text_color(ss.sidebar_edit_input_placeholder_color).child("Name…")
                            } else {
                                div().child(format!("{value}|"))
                            })),
                    ),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_this, _event, _window, _cx| {}),
            )
            .into_any_element()
    }

    pub(crate) fn render_sidebar_item(
        &self,
        item: SidebarItem,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if let Some(SidebarEditMode::Rename {
            path: ref rp,
            ref value,
        }) = self.sidebar_edit
        {
            if *rp == item.path {
                return self.render_sidebar_edit_row(value, item.depth, cx);
            }
        }

        let is_selected = self
            .selected_sidebar_path
            .as_ref()
            .is_some_and(|selected| selected == &item.path)
            || self
                .current_file_path
                .as_ref()
                .is_some_and(|current| current == &item.path);
        let is_active_file = self
            .current_file_path
            .as_ref()
            .is_some_and(|current| current == &item.path);
        let path = item.path.clone();
        let is_dir = item.is_dir;
        let ss = &self.styles.sidebar;
        let left_padding = self.scaled(ss.sidebar_item_indent_base_px + item.depth as f32 * ss.sidebar_item_indent_step_px);
        let row_bg = if is_selected {
            ss.sidebar_item_selected_bg
        } else {
            gpui::transparent_black()
        };
        let icon_color = if is_selected {
            ss.sidebar_item_selected_icon_color
        } else {
            ss.sidebar_item_icon_color
        };
        let label_color = if is_selected {
            ss.sidebar_item_selected_text_color
        } else {
            ss.sidebar_item_text_color
        };
        let folder_icon = if item.is_expanded {
            FOLDER_OPEN_SVG_PATH
        } else {
            FOLDER_CLOSED_SVG_PATH
        };
        let item_hover_bg = ss.sidebar_item_hover_bg;

        div()
            .id(SharedString::from(format!("sidebar-{}", path.display())))
            .w_full()
            .h(px(self.scaled(ss.sidebar_item_height_px)))
            .flex()
            .items_center()
            .bg(row_bg)
            .when(!self.sidebar_resizing, |this| {
                this.hover(move |style| style.bg(item_hover_bg))
            })
            .child(
                div()
                    .w_full()
                    .h_full()
                    .pl(px(left_padding))
                    .pr(px(self.scaled(ss.sidebar_item_padding_right_px)))
                    .flex()
                    .items_center()
                    .gap(px(self.scaled(ss.sidebar_item_gap_px)))
                    .child(
                        div()
                            .w(px(self.scaled(ss.sidebar_item_fold_container_width_px)))
                            .h(px(self.scaled(ss.sidebar_item_fold_container_height_px)))
                            .flex()
                            .flex_none()
                            .items_center()
                            .justify_center()
                            .child(if item.is_dir {
                                svg()
                                    .path(FILETREE_FOLD_SVG_PATH)
                                    .size(px(self.scaled(ss.sidebar_item_fold_icon_size_px)))
                                    .text_color(ss.sidebar_item_fold_icon_color)
                                    .when(item.is_expanded, |s| {
                                        s.with_transformation(Transformation::rotate(radians(
                                            std::f32::consts::FRAC_PI_2,
                                        )))
                                    })
                                    .into_any_element()
                            } else {
                                div().w(px(self.scaled(ss.sidebar_item_fold_icon_size_px))).into_any_element()
                            }),
                    )
                    .child(
                        svg()
                            .path(if item.is_dir {
                                folder_icon
                            } else {
                                MARKDOWN_SVG_PATH
                            })
                            .size(px(self.scaled(ss.sidebar_item_icon_size_px)))
                            .text_color(icon_color),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .text_size(px(self.scaled(ss.sidebar_item_font_size_px)))
                            .text_color(label_color)
                            .truncate()
                            .child(item.name),
                    )
                    .when(is_active_file, |this| {
                        this.child(
                            div()
                                .w(px(self.scaled(ss.sidebar_item_active_dot_size_px)))
                                .h(px(self.scaled(ss.sidebar_item_active_dot_size_px)))
                                .rounded_full()
                                .flex_none()
                                .bg(ss.sidebar_item_active_dot_color),
                        )
                    }),
            )
            .on_click({
                let path = path.clone();
                cx.listener(move |this, _, _, cx| {
                    if path.is_dir() {
                        this.toggle_sidebar_directory(path.clone(), cx);
                    } else {
                        this.open_sidebar_file(path.clone(), cx);
                    }
                })
            })
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                    this.open_sidebar_context_menu(path.clone(), is_dir, event.position, cx);
                }),
            )
            .into_any_element()
    }

    pub(crate) fn render_sidebar(&self, cx: &mut Context<Self>) -> AnyElement {
        let has_root = self.sidebar_root.exists() && self.sidebar_error.is_none();
        let ss = &self.styles.sidebar;
        let mut content: Vec<AnyElement> = Vec::new();

        content.push(
            div()
                .w_full()
                .h(px(self.scaled(ss.sidebar_top_spacer_height_px)))
                .flex_none()
                .into_any_element(),
        );

        content.push(
            div()
                .w_full()
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .py(px(self.scaled(ss.sidebar_logo_padding_y_px)))
                .child(
                    img(PathBuf::from(LOGO_SVG_PATH))
                        .w(px(self.scaled(ss.sidebar_logo_size_px)))
                        .h(px(self.scaled(ss.sidebar_logo_size_px))),
                )
                .into_any_element(),
        );

        let root_name = self.sidebar_root_name().to_uppercase();
        let header_hover_bg = ss.sidebar_header_hover_bg;
        let header = div()
            .w_full()
            .h(px(self.scaled(ss.sidebar_header_height_px)))
            .bg(ss.sidebar_header_bg)
            .px(px(self.scaled(ss.sidebar_header_padding_right_px)))
            .pl(px(self.scaled(ss.sidebar_header_padding_left_px)))
            .flex()
            .items_center()
            .gap(px(self.scaled(ss.sidebar_header_gap_px)))
            .flex_none()
            .hover(move |s| s.bg(header_hover_bg))
            .child(
                svg()
                    .path(ROOT_SVG_PATH)
                    .size(px(self.scaled(ss.sidebar_header_icon_size_px)))
                    .text_color(ss.sidebar_header_icon_color),
            )
            .child(
                div()
                    .flex_1()
                    .text_size(px(self.scaled(ss.sidebar_header_font_size_px)))
                    .font_weight(FontWeight::BOLD)
                    .text_color(ss.sidebar_header_text_color)
                    .overflow_hidden()
                    .child(root_name),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.pick_workspace_folder(cx);
                }),
            )
            .into_any_element();
        content.push(header);

        if let Some(error) = &self.sidebar_error {
            content.push(
                div()
                    .w_full()
                    .px(px(self.scaled(ss.sidebar_error_padding_x_px)))
                    .py(px(self.scaled(ss.sidebar_error_padding_y_px)))
                    .text_size(px(self.scaled(ss.sidebar_error_font_size_px)))
                    .text_color(ss.sidebar_error_text_color)
                    .child(error.clone())
                    .into_any_element(),
            );
        } else if !has_root {
            let empty_hover_bg = ss.sidebar_empty_btn_hover_bg;
            content.push(
                div()
                    .w_full()
                    .flex_1()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(self.scaled(ss.sidebar_empty_gap_px)))
                    .px(px(self.scaled(ss.sidebar_empty_padding_x_px)))
                    .child(
                        div()
                            .text_size(px(self.scaled(ss.sidebar_empty_font_size_px)))
                            .text_color(ss.sidebar_empty_text_color)
                            .text_align(gpui::TextAlign::Center)
                            .child("单击或右键\n打开目录"),
                    )
                    .child(
                        div()
                            .px(px(self.scaled(ss.sidebar_empty_btn_padding_x_px)))
                            .py(px(self.scaled(ss.sidebar_empty_btn_padding_y_px)))
                            .rounded_full()
                            .border(px(ss.sidebar_empty_btn_border_width_px))
                            .border_dashed()
                            .border_color(ss.sidebar_empty_btn_border_color)
                            .text_size(px(self.scaled(ss.sidebar_empty_btn_font_size_px)))
                            .text_color(ss.sidebar_empty_btn_text_color)
                            .hover(move |s| s.bg(empty_hover_bg))
                            .cursor(CursorStyle::Arrow)
                            .child("打开目录")
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.pick_workspace_folder(cx);
                                }),
                            ),
                    )
                    .into_any_element(),
            );
        } else {
            let create_parent = match &self.sidebar_edit {
                Some(SidebarEditMode::CreateFile { parent, .. })
                | Some(SidebarEditMode::CreateDir { parent, .. }) => Some(parent.clone()),
                _ => None,
            };
            let create_depth = create_parent.as_ref().map(|p| {
                p.strip_prefix(&self.sidebar_root)
                    .map(|rel| rel.components().count() + 1)
                    .unwrap_or(1)
            });
            let create_value = match &self.sidebar_edit {
                Some(SidebarEditMode::CreateFile { value, .. })
                | Some(SidebarEditMode::CreateDir { value, .. }) => Some(value.as_str()),
                _ => None,
            };

            let items = self.sidebar_items();
            let mut injected = false;
            for item in items {
                content.push(self.render_sidebar_item(item.clone(), cx));
                if let Some(ref parent) = create_parent {
                    if !injected && item.is_dir && &item.path == parent {
                        let depth = create_depth.unwrap_or(1);
                        let value = create_value.unwrap_or("");
                        content.push(self.render_sidebar_edit_row(value, depth, cx));
                        injected = true;
                    }
                }
            }
            if !injected {
                if let Some(depth) = create_depth {
                    let value = create_value.unwrap_or("");
                    content.push(self.render_sidebar_edit_row(value, depth, cx));
                }
            }
        }

        content.push(
            div()
                .w_full()
                .h(px(self.scaled(ss.sidebar_bottom_spacer_height_px)))
                .flex_none()
                .into_any_element(),
        );

        div()
            .id("sidebar")
            .w(px(self.sidebar_width()))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(ss.sidebar_bg)
            .overflow_y_scroll()
            .children(content)
            .into_any_element()
    }
}
