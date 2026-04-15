use std::path::PathBuf;

use gpui::{AnyElement, Context, CursorStyle, MouseButton, Pixels, Point, SharedString, div, prelude::*, px, svg};

use super::*;

impl EditorView {
    pub(crate) fn open_sidebar_context_menu(
        &mut self,
        path: PathBuf,
        is_dir: bool,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = Some(ContextMenu {
            position,
            kind: ContextMenuKind::Sidebar { path, is_dir },
        });
        self.sidebar_edit = None;
        cx.notify();
    }

    pub(crate) fn open_format_context_menu(
        &mut self,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = Some(ContextMenu {
            position,
            kind: ContextMenuKind::Format,
        });
        self.sidebar_edit = None;
        cx.notify();
    }

    pub(crate) fn cursor_screen_position(&self) -> Point<Pixels> {
        let (line, col) = self.buffer.offset_to_line_col(self.cursor.offset);
        let origin_y: f32 = self.content_origin.y.into();
        let y = origin_y + self.line_y_offset(line) - self.scroll_offset;
        let line_height = self.visual_height_for_line(line);
        let x = if let Some(Some(info)) = self.line_display_info.get(line) {
            let disp_col = helpers::buffer_to_display_offset(&info.buffer_to_display, col);
            let pos = info
                .wrapped_line
                .position_for_index(disp_col, gpui::px(info.line_height))
                .unwrap_or_default();
            info.content_x + f32::from(pos.x)
        } else {
            self.last_content_left + col as f32 * self.base_font_size() * 0.6
        };
        gpui::point(px(x), px(y + line_height))
    }

    pub(crate) fn open_slash_menu(&mut self, cx: &mut Context<Self>) {
        self.slash_menu_start = Some(self.cursor.offset - 1);
        self.slash_filter = String::new();
        self.menu_selected_index = 0;
        self.heading_submenu_open = false;
        let pos = self.cursor_screen_position();
        self.open_format_context_menu(pos, cx);
    }

    pub(crate) fn update_slash_filter(&mut self, cx: &mut Context<Self>) {
        if let Some(start) = self.slash_menu_start {
            self.menu_selected_index = 0;
            let end = self.cursor.offset;
            if end > start + 1 {
                let text = self.buffer.text();
                let filter_start = start + 1;
                if filter_start <= end && end <= text.len() {
                    self.slash_filter = text[filter_start..end].to_string();
                }
            } else {
                self.slash_filter.clear();
            }
            let pos = self.cursor_screen_position();
            if let Some(menu) = &mut self.context_menu {
                menu.position = pos;
            }
            cx.notify();
        }
    }

    pub(crate) fn open_tab_context_menu(
        &mut self,
        tab_id: String,
        position: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.context_menu = Some(ContextMenu {
            position,
            kind: ContextMenuKind::Tab { tab_id },
        });
        self.sidebar_edit = None;
        cx.notify();
    }

    pub(crate) fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.context_menu.take().is_some() {
            if let Some(start) = self.slash_menu_start.take() {
                let end = self.cursor.offset;
                if end > start {
                    self.buffer.replace(start..end, "");
                    self.cursor.move_to(start, &self.buffer);
                    self.reparse();
                }
                self.slash_filter.clear();
            }
            cx.notify();
        }
    }

    pub(crate) fn execute_format_menu_action(
        &mut self,
        action: FormatMenuAction,
        cx: &mut Context<Self>,
    ) {
        if let Some(start) = self.slash_menu_start.take() {
            let end = self.cursor.offset;
            if end > start {
                self.buffer.replace(start..end, "");
                self.cursor.move_to(start, &self.buffer);
                self.reparse();
            }
            self.slash_filter.clear();
        }
        self.context_menu = None;
        match action {
            FormatMenuAction::Cut => self.cut_selection_to_clipboard(cx),
            FormatMenuAction::Copy => self.copy_selection_to_clipboard(cx),
            FormatMenuAction::Paste => self.paste_from_clipboard(cx),
            FormatMenuAction::Bold => {
                self.checkpoint();
                self.apply_wrap("**", "**");
                self.pause_blinking(cx);
            }
            FormatMenuAction::Italic => {
                self.checkpoint();
                self.apply_wrap("*", "*");
                self.pause_blinking(cx);
            }
            FormatMenuAction::Strikethrough => {
                self.checkpoint();
                self.apply_wrap("~~", "~~");
                self.pause_blinking(cx);
            }
            FormatMenuAction::InlineCode => {
                self.checkpoint();
                self.apply_wrap("`", "`");
                self.pause_blinking(cx);
            }
            FormatMenuAction::CodeBlock => {
                self.checkpoint();
                self.apply_wrap("```\n", "\n```");
                self.pause_blinking(cx);
            }
            FormatMenuAction::HorizontalRule => {
                self.checkpoint();
                self.toggle_horizontal_rule_at_cursor();
                self.pause_blinking(cx);
            }
            FormatMenuAction::Quote => {
                self.checkpoint();
                self.toggle_current_line_prefix("> ");
                self.pause_blinking(cx);
            }
            FormatMenuAction::UnorderedList => {
                self.checkpoint();
                self.toggle_current_line_prefix("- ");
                self.pause_blinking(cx);
            }
            FormatMenuAction::OrderedList => {
                self.checkpoint();
                self.toggle_current_line_prefix("1. ");
                self.pause_blinking(cx);
            }
            FormatMenuAction::Todo => {
                self.checkpoint();
                self.toggle_current_line_prefix("- [ ] ");
                self.pause_blinking(cx);
            }
            FormatMenuAction::Link => {
                self.checkpoint();
                self.insert_link_or_image("[", "](https://)", "title");
                self.pause_blinking(cx);
            }
            FormatMenuAction::Image => {
                self.checkpoint();
                self.insert_link_or_image("![", "](image.png)", "alt");
                self.pause_blinking(cx);
            }
        }
        self.close_context_menu(cx);
    }

    pub(crate) fn slash_menu_matched_actions(&self) -> Vec<FormatMenuAction> {
        let filter = self.slash_filter.to_lowercase();
        let matches_filter = |label: &str, filter_text: &str| -> bool {
            if filter.is_empty() {
                return true;
            }
            label.to_lowercase().contains(&filter) || filter_text.to_lowercase().contains(&filter)
        };

        let mut actions = Vec::new();
        let all_items: &[(&str, &str, FormatMenuAction)] = &[
            ("粗体", "bold", FormatMenuAction::Bold),
            ("斜体", "italic", FormatMenuAction::Italic),
            ("删除线", "strikethrough", FormatMenuAction::Strikethrough),
            ("行内代码", "inline code", FormatMenuAction::InlineCode),
            ("代码块", "code block", FormatMenuAction::CodeBlock),
            (
                "分割线",
                "horizontal rule divider",
                FormatMenuAction::HorizontalRule,
            ),
            ("引用", "quote blockquote", FormatMenuAction::Quote),
            (
                "无序列表",
                "unordered list bullet",
                FormatMenuAction::UnorderedList,
            ),
            (
                "有序列表",
                "ordered list numbered",
                FormatMenuAction::OrderedList,
            ),
            ("待办事项", "todo task checkbox", FormatMenuAction::Todo),
            ("链接", "link url", FormatMenuAction::Link),
            ("图片", "image photo", FormatMenuAction::Image),
        ];
        for &(label, ft, action) in all_items {
            if matches_filter(label, ft) {
                actions.push(action);
            }
        }
        actions
    }

    pub(crate) fn execute_slash_menu_selected(&mut self, cx: &mut Context<Self>) {
        let actions = self.slash_menu_matched_actions();
        let index = self
            .menu_selected_index
            .min(actions.len().saturating_sub(1));
        if let Some(&action) = actions.get(index) {
            self.execute_format_menu_action(action, cx);
        }
    }

    // ── Context menu rendering ──────────────────────────────────────

    fn render_context_menu_item(
        &self,
        label: &str,
        is_destructive: bool,
        cx: &mut Context<Self>,
        handler: impl Fn(&mut Self, &mut gpui::Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let ms = &self.styles.context_menu;
        let text_color = if is_destructive {
            ms.menu_item_danger_text_color
        } else {
            ms.menu_item_text_color
        };
        let hover_bg = ms.menu_item_hover_bg;
        let item_id = SharedString::from(format!("menu-item-{}", label));
        div()
            .id(item_id)
            .w_full()
            .h(px(ms.menu_item_height_px))
            .px(px(ms.menu_item_padding_x_px))
            .flex()
            .items_center()
            .text_size(px(ms.menu_item_font_size_px))
            .text_color(text_color)
            .hover(move |s| s.bg(hover_bg))
            .cursor(CursorStyle::Arrow)
            .child(label.to_string())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _event, window, cx| {
                    handler(this, window, cx);
                }),
            )
            .into_any_element()
    }

    fn render_format_context_menu_item(
        &self,
        label: &str,
        shortcut: Option<&str>,
        is_selected: bool,
        cx: &mut Context<Self>,
        handler: impl Fn(&mut Self, &mut gpui::Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let ms = &self.styles.context_menu;
        let bg = if is_selected {
            ms.menu_item_selected_bg
        } else {
            gpui::transparent_black()
        };
        let hover_bg = ms.menu_item_hover_bg;
        div()
            .w_full()
            .h(px(self.scaled(ms.menu_item_height_px)))
            .bg(bg)
            .px(px(self.scaled(ms.menu_item_padding_x_px)))
            .flex()
            .items_center()
            .justify_between()
            .text_size(px(self.scaled(ms.menu_item_font_size_px)))
            .text_color(ms.menu_item_text_color)
            .hover(move |s| s.bg(hover_bg))
            .cursor(CursorStyle::Arrow)
            .child(label.to_string())
            .when_some(shortcut, |this, shortcut| {
                this.child(
                    div()
                        .text_size(px(self.scaled(ms.menu_shortcut_font_size_px)))
                        .text_color(ms.menu_shortcut_text_color)
                        .child(shortcut.to_string()),
                )
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _event, window, cx| {
                    handler(this, window, cx);
                }),
            )
            .into_any_element()
    }

    fn render_context_menu_separator(&self) -> AnyElement {
        let ms = &self.styles.context_menu;
        div()
            .w_full()
            .h(px(ms.menu_separator_height_px))
            .my(px(ms.menu_separator_margin_y_px))
            .bg(ms.menu_separator_color)
            .into_any_element()
    }

    fn render_sidebar_context_menu(
        &self,
        path: PathBuf,
        is_dir: bool,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let has_clipboard = self.file_clipboard.is_some();
        let parent_dir = if is_dir {
            path.clone()
        } else {
            path.parent()
                .map(PathBuf::from)
                .unwrap_or_else(|| path.clone())
        };

        let mut items: Vec<AnyElement> = Vec::new();

        {
            let pd = parent_dir.clone();
            items.push(
                self.render_context_menu_item("新建文件", false, cx, move |this, _, cx| {
                    this.start_create_file(pd.clone(), cx);
                }),
            );
        }
        {
            let pd = parent_dir.clone();
            items.push(self.render_context_menu_item(
                "新建文件夹",
                false,
                cx,
                move |this, _, cx| {
                    this.start_create_dir(pd.clone(), cx);
                },
            ));
        }
        items.push(self.render_context_menu_separator());
        {
            let p = path.clone();
            items.push(
                self.render_context_menu_item("重命名", false, cx, move |this, _, cx| {
                    this.start_rename(p.clone(), cx);
                }),
            );
        }
        {
            let p = path.clone();
            items.push(
                self.render_context_menu_item("删除", true, cx, move |this, _, cx| {
                    this.delete_path(p.clone(), cx);
                }),
            );
        }
        items.push(self.render_context_menu_separator());
        {
            let p = path.clone();
            items.push(
                self.render_context_menu_item("剪切", false, cx, move |this, _, cx| {
                    this.cut_path(p.clone(), cx);
                }),
            );
        }
        {
            let p = path.clone();
            items.push(
                self.render_context_menu_item("复制", false, cx, move |this, _, cx| {
                    this.copy_path(p.clone(), cx);
                }),
            );
        }
        if has_clipboard {
            let pd = parent_dir.clone();
            items.push(
                self.render_context_menu_item("粘贴", false, cx, move |this, _, cx| {
                    this.paste_into_dir(pd.clone(), cx);
                }),
            );
        }
        items.push(self.render_context_menu_separator());
        {
            let p = path.clone();
            items.push(self.render_context_menu_item(
                "在访达中显示",
                false,
                cx,
                move |this, _, _cx| {
                    this.reveal_in_finder(&p);
                },
            ));
        }

        items
    }

    fn render_format_context_menu(&self, cx: &mut Context<Self>) -> Vec<AnyElement> {
        let mut items: Vec<AnyElement> = Vec::new();
        let is_slash_menu = self.slash_menu_start.is_some();
        let filter = self.slash_filter.to_lowercase();
        let selected_index = self.menu_selected_index;
        let mut actionable_index: usize = 0;

        let matches_filter = |label: &str, filter_text: &str| -> bool {
            if filter.is_empty() {
                return true;
            }
            label.to_lowercase().contains(&filter) || filter_text.to_lowercase().contains(&filter)
        };

        if !is_slash_menu {
            let clipboard_items: &[(&str, &str, Option<&str>, FormatMenuAction)] = &[
                ("剪切", "cut", Some("⌘X"), FormatMenuAction::Cut),
                ("复制", "copy", Some("⌘C"), FormatMenuAction::Copy),
                ("粘贴", "paste", Some("⌘V"), FormatMenuAction::Paste),
            ];
            let mut clip_matched = Vec::new();
            for &(label, ft, shortcut, action) in clipboard_items {
                if matches_filter(label, ft) {
                    clip_matched.push(self.render_format_context_menu_item(
                        label,
                        shortcut,
                        false,
                        cx,
                        move |this, _, cx| {
                            this.execute_format_menu_action(action, cx);
                        },
                    ));
                }
            }
            if !clip_matched.is_empty() {
                items.extend(clip_matched);
            }
        }

        let all_menu_items: &[(&str, &str, Option<&str>, FormatMenuAction)] = &[
            ("粗体", "bold", Some("⌘B"), FormatMenuAction::Bold),
            ("斜体", "italic", Some("⌘I"), FormatMenuAction::Italic),
            (
                "删除线",
                "strikethrough",
                Some("⌘⇧S"),
                FormatMenuAction::Strikethrough,
            ),
            (
                "行内代码",
                "inline code",
                Some("⌘E"),
                FormatMenuAction::InlineCode,
            ),
            (
                "代码块",
                "code block",
                Some("⌘⌥C"),
                FormatMenuAction::CodeBlock,
            ),
            (
                "分割线",
                "horizontal rule divider",
                Some("---"),
                FormatMenuAction::HorizontalRule,
            ),
            (
                "引用",
                "quote blockquote",
                Some("⌘⌥Q"),
                FormatMenuAction::Quote,
            ),
            (
                "无序列表",
                "unordered list bullet",
                Some("⌘⌥U"),
                FormatMenuAction::UnorderedList,
            ),
            (
                "有序列表",
                "ordered list numbered",
                Some("⌘⌥O"),
                FormatMenuAction::OrderedList,
            ),
            (
                "待办事项",
                "todo task checkbox",
                Some("⌘⌥T"),
                FormatMenuAction::Todo,
            ),
            ("链接", "link url", Some("⌘K"), FormatMenuAction::Link),
            ("图片", "image photo", Some("⌘⇧I"), FormatMenuAction::Image),
        ];

        let group_boundaries = [0usize, 4, 10, 12];
        let mut group_start_flags = [false; 3];

        for (idx, &(label, ft, _, _)) in all_menu_items.iter().enumerate() {
            if matches_filter(label, ft) {
                for g in 0..3 {
                    if idx >= group_boundaries[g] && idx < group_boundaries[g + 1] {
                        group_start_flags[g] = true;
                    }
                }
            }
        }

        let mut prev_group_had_items = !items.is_empty();
        for g in 0..3 {
            if !group_start_flags[g] {
                continue;
            }
            if prev_group_had_items {
                items.push(self.render_context_menu_separator());
            }
            for idx in group_boundaries[g]..group_boundaries[g + 1] {
                let (label, ft, shortcut, action) = all_menu_items[idx];
                if matches_filter(label, ft) {
                    let is_selected = is_slash_menu && actionable_index == selected_index;
                    items.push(self.render_format_context_menu_item(
                        label,
                        shortcut,
                        is_selected,
                        cx,
                        move |this, _, cx| {
                            this.execute_format_menu_action(action, cx);
                        },
                    ));
                    actionable_index += 1;
                }
            }
            prev_group_had_items = true;
        }

        items
    }

    pub(crate) fn render_context_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(menu) = &self.context_menu else {
            return div().into_any_element();
        };
        let pos_x: f32 = menu.position.x.into();
        let pos_y: f32 = menu.position.y.into();

        let (items, width) = match &menu.kind {
            ContextMenuKind::Sidebar { path, is_dir } => (
                self.render_sidebar_context_menu(path.clone(), *is_dir, cx),
                self.scaled(self.styles.context_menu.menu_sidebar_width_px),
            ),
            ContextMenuKind::Format => (
                self.render_format_context_menu(cx),
                self.scaled(self.styles.context_menu.menu_format_width_px),
            ),
            ContextMenuKind::Tab { tab_id } => {
                let mut items = Vec::new();
                {
                    let id = tab_id.clone();
                    items.push(self.render_context_menu_item(
                        "关闭",
                        false,
                        cx,
                        move |this, _, cx| {
                            this.close_tab(&id, cx);
                        },
                    ));
                }
                {
                    let id = tab_id.clone();
                    items.push(self.render_context_menu_item(
                        "关闭其他",
                        false,
                        cx,
                        move |this, _, cx| {
                            this.close_other_tabs(&id, cx);
                        },
                    ));
                }
                {
                    let id = tab_id.clone();
                    items.push(self.render_context_menu_item(
                        "关闭右侧",
                        false,
                        cx,
                        move |this, _, cx| {
                            this.close_right_tabs(&id, cx);
                        },
                    ));
                }
                items.push(self.render_context_menu_separator());
                items.push(self.render_context_menu_item(
                    "关闭已保存",
                    false,
                    cx,
                    move |this, _, cx| {
                        this.close_saved_tabs(cx);
                    },
                ));
                (
                    items,
                    self.scaled(self.styles.context_menu.menu_tab_width_px),
                )
            }
            ContextMenuKind::Workspace => {
                let mut items = Vec::new();
                let wss = &self.styles.workspace_switcher;
                let current_root = self.sidebar_root.clone();
                let check_color = wss.ws_menu_item_check_color;
                let check_size = self.scaled(wss.ws_menu_item_check_size_px);
                let close_color = wss.ws_menu_item_close_color;
                let close_hover_color = wss.ws_menu_item_close_hover_color;
                let close_size = self.scaled(wss.ws_menu_item_close_size_px);
                let ms = &self.styles.context_menu;
                let hover_bg = ms.menu_item_hover_bg;
                let text_color = ms.menu_item_text_color;

                for workspace in self.workspaces.clone() {
                    let ws_name = workspace
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_else(|| workspace.display().to_string())
                        .to_uppercase();
                    let is_current = workspace == current_root;
                    let ws_switch = workspace.clone();
                    let ws_close = workspace.clone();
                    let item_id = SharedString::from(format!("ws-{}", ws_name));
                    let close_btn_id = SharedString::from(format!("ws-close-{}", ws_name));

                    items.push(
                        div()
                            .id(item_id)
                            .w_full()
                            .h(px(self.scaled(ms.menu_item_height_px)))
                            .px(px(self.scaled(ms.menu_item_padding_x_px)))
                            .flex()
                            .items_center()
                            .gap(px(self.scaled(4.0)))
                            .text_size(px(self.scaled(ms.menu_item_font_size_px)))
                            .text_color(text_color)
                            .hover(move |s| s.bg(hover_bg))
                            .cursor(CursorStyle::Arrow)
                            .child(
                                div()
                                    .w(px(check_size))
                                    .h(px(check_size))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .flex_none()
                                    .when(is_current, |this| {
                                        this.child(
                                            svg()
                                                .path(CHECK_SVG_PATH)
                                                .size(px(check_size))
                                                .text_color(check_color),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .overflow_hidden()
                                    .truncate()
                                    .child(ws_name),
                            )
                            .child(
                                div()
                                    .id(close_btn_id)
                                    .w(px(close_size))
                                    .h(px(close_size))
                                    .flex()
                                    .flex_none()
                                    .items_center()
                                    .justify_center()
                                    .rounded_full()
                                    .text_size(px(self.scaled(10.0)))
                                    .text_color(close_color)
                                    .hover(move |s| {
                                        s.text_color(close_hover_color).bg(hover_bg)
                                    })
                                    .child("✕")
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, _, cx| {
                                            this.close_workspace(&ws_close, cx);
                                        }),
                                    ),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, _, cx| {
                                    this.switch_workspace(ws_switch.clone(), cx);
                                }),
                            )
                            .into_any_element(),
                    );
                }
                items.push(self.render_context_menu_separator());
                items.push(self.render_context_menu_item(
                    "打开 Workspace ...",
                    false,
                    cx,
                    move |this, _, cx| {
                        this.pick_workspace_folder(cx);
                    },
                ));
                (
                    items,
                    self.scaled(wss.ws_menu_width_px),
                )
            }
        };

        let ms = &self.styles.context_menu;

        // Estimate total menu height from items
        let item_count = items.len();
        let estimated_item_height = self.scaled(ms.menu_item_height_px);
        let menu_padding = self.scaled(ms.menu_padding_y_px) * 2.0;
        let estimated_menu_height = item_count as f32 * estimated_item_height + menu_padding;

        let viewport_w = self.last_viewport_width;
        let viewport_h = self.last_viewport_height;
        let margin = self.scaled(ms.menu_max_height_margin_px).max(8.0);

        // Adjust horizontal: prefer right of click, flip left if overflows
        let mut final_x = pos_x;
        if final_x + width > viewport_w - margin {
            final_x = (pos_x - width).max(margin);
        }

        // Adjust vertical: prefer below click, flip above if overflows
        let mut final_y = pos_y;
        let mut max_menu_height = (viewport_h - final_y - margin).max(self.scaled(100.0));
        if estimated_menu_height > max_menu_height {
            // Try flipping above the click position
            let above_y = pos_y - estimated_menu_height;
            if above_y >= margin {
                final_y = above_y;
                max_menu_height = estimated_menu_height;
            } else {
                // Can't fit above either — pick the side with more room and scroll
                let space_below = viewport_h - pos_y - margin;
                let space_above = pos_y - margin;
                if space_above > space_below {
                    final_y = margin;
                    max_menu_height = space_above;
                }
                // else keep original position (below) with scroll
            }
        }

        let mut menu_div = div()
            .id("context-menu")
            .absolute()
            .left(px(final_x))
            .top(px(final_y))
            .w(px(width))
            .max_h(px(max_menu_height))
            .overflow_y_scroll()
            .py(px(self.scaled(ms.menu_padding_y_px)))
            .bg(ms.menu_bg)
            .border(px(ms.menu_border_width_px))
            .border_color(ms.menu_border_color)
            .rounded(px(self.scaled(ms.menu_radius_px)));
        if ms.menu_shadow {
            menu_div = menu_div.shadow_xs();
        }
        menu_div
            .children(items)
            .cursor(CursorStyle::Arrow)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|_this, _event, _window, _cx| {}),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|_this, _event, _window, _cx| {}),
            )
            .on_mouse_move(cx.listener(|_this, _event, _window, _cx| {}))
            .on_scroll_wheel(cx.listener(|_this, _event, _window, _cx| {}))
            .into_any_element()
    }
}
