use std::path::PathBuf;

use gpui::{
    AnyElement, Context, CursorStyle, MouseButton, Pixels, Point, SharedString, div, prelude::*,
    px, svg,
};

use crate::markdown::{BlockType, InlineKind};

use super::*;

impl EditorView {
    /// Detect which format action is "active" at the cursor position.
    /// Uses the markdown parser's decoration data for accurate detection.
    /// Inline syntax has priority over block-level syntax.
    /// Returns at most one active action.
    fn active_format_actions(&self) -> Vec<FormatMenuAction> {
        let cursor_line = self.cursor_line();
        let decoration = match self.decorations.get(cursor_line) {
            Some(d) => d,
            None => return Vec::new(),
        };

        let line_start = self.buffer.line_start(cursor_line);
        let cursor_col = self.cursor.offset.saturating_sub(line_start);

        // Check inline spans (including markers) — pick the smallest (innermost) match
        let mut best_inline: Option<(usize, FormatMenuAction)> = None;
        for span in &decoration.inline_spans {
            let marker_len = match &span.kind {
                InlineKind::Bold => 2,
                InlineKind::Italic => 1,
                InlineKind::Strikethrough => 2,
                InlineKind::InlineCode => 1,
                InlineKind::Link => 0,
                InlineKind::Image => 0,
            };
            let full_start = span.range.start.saturating_sub(marker_len);
            let full_end = span.range.end + marker_len;
            if cursor_col >= full_start && cursor_col <= full_end {
                let action = match &span.kind {
                    InlineKind::Bold => Some(FormatMenuAction::Bold),
                    InlineKind::Italic => Some(FormatMenuAction::Italic),
                    InlineKind::Strikethrough => Some(FormatMenuAction::Strikethrough),
                    InlineKind::InlineCode => Some(FormatMenuAction::InlineCode),
                    InlineKind::Link => Some(FormatMenuAction::Link),
                    InlineKind::Image => Some(FormatMenuAction::Image),
                };
                if let Some(action) = action {
                    let span_len = full_end - full_start;
                    if best_inline
                        .as_ref()
                        .is_none_or(|(prev_len, _)| span_len < *prev_len)
                    {
                        best_inline = Some((span_len, action));
                    }
                }
            }
        }

        if let Some((_, action)) = best_inline {
            return vec![action];
        }

        // Fallback to block-level syntax
        let block_action = match &decoration.block_type {
            BlockType::Blockquote => Some(FormatMenuAction::Quote),
            BlockType::UnorderedList => Some(FormatMenuAction::UnorderedList),
            BlockType::OrderedList => Some(FormatMenuAction::OrderedList),
            BlockType::TaskList(_) => Some(FormatMenuAction::Todo),
            BlockType::CodeBlock | BlockType::CodeBlockFence => Some(FormatMenuAction::CodeBlock),
            BlockType::HorizontalRule => Some(FormatMenuAction::HorizontalRule),
            _ => None,
        };

        block_action.into_iter().collect()
    }
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
            ("待办事项", "todo task checkbox", FormatMenuAction::Todo),
            ("代码块", "code block", FormatMenuAction::CodeBlock),
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

    fn render_context_menu_item_with_icon(
        &self,
        label: &str,
        icon_path: &'static str,
        is_destructive: bool,
        cx: &mut Context<Self>,
        handler: impl Fn(&mut Self, &mut gpui::Window, &mut Context<Self>) + 'static,
    ) -> AnyElement {
        let ms = &self.styles.context_menu;
        let wss = &self.styles.workspace_switcher;
        let text_color = if is_destructive {
            ms.menu_item_danger_text_color
        } else {
            ms.menu_item_text_color
        };
        let hover_bg = ms.menu_item_hover_bg;
        let item_id = SharedString::from(format!("menu-item-{}", label));
        let icon_size = self.scaled(wss.ws_menu_item_action_icon_size_px);
        let check_size = self.scaled(wss.ws_menu_item_check_size_px);
        let icon_col_w = check_size.max(icon_size);
        div()
            .id(item_id)
            .w_full()
            .h(px(ms.menu_item_height_px))
            .px(px(ms.menu_item_padding_x_px))
            .flex()
            .items_center()
            .gap(px(self.scaled(4.0)))
            .text_size(px(ms.menu_item_font_size_px))
            .text_color(text_color)
            .hover(move |s| s.bg(hover_bg))
            .cursor(CursorStyle::Arrow)
            .child(
                div()
                    .w(px(icon_col_w))
                    .h(px(icon_col_w))
                    .flex()
                    .flex_none()
                    .items_center()
                    .justify_center()
                    .child(
                        svg()
                            .path(icon_path)
                            .size(px(icon_size))
                            .text_color(wss.ws_menu_item_action_icon_color),
                    ),
            )
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
        icon_path: Option<&str>,
        shortcut: Option<&str>,
        is_selected: bool,
        is_active: bool,
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
        let text_color = if is_active {
            ms.menu_format_active_text_color
        } else {
            ms.menu_item_text_color
        };
        let icon_color = if is_active {
            ms.menu_format_active_text_color
        } else {
            ms.menu_format_icon_color
        };
        let icon_size = self.scaled(ms.menu_format_icon_size_px);
        let icon_gap = self.scaled(ms.menu_format_icon_gap_px);
        let icon_shared: Option<SharedString> =
            icon_path.map(|p| SharedString::from(p.to_string()));
        div()
            .w_full()
            .h(px(self.scaled(ms.menu_item_height_px)))
            .bg(bg)
            .px(px(self.scaled(ms.menu_item_padding_x_px)))
            .flex()
            .items_center()
            .justify_between()
            .text_size(px(self.scaled(ms.menu_item_font_size_px)))
            .text_color(text_color)
            .hover(move |s| s.bg(hover_bg))
            .cursor(CursorStyle::Arrow)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(icon_gap))
                    .child(
                        div()
                            .w(px(icon_size))
                            .h(px(icon_size))
                            .flex()
                            .items_center()
                            .justify_center()
                            .flex_none()
                            .when_some(icon_shared, |this, path| {
                                this.child(
                                    svg().path(path).size(px(icon_size)).text_color(icon_color),
                                )
                            }),
                    )
                    .child(label.to_string()),
            )
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
            items.push(self.render_context_menu_item(
                "新建文件",
                false,
                cx,
                move |this, window, cx| {
                    this.start_create_file(pd.clone(), window, cx);
                },
            ));
        }
        {
            let pd = parent_dir.clone();
            items.push(self.render_context_menu_item(
                "新建文件夹",
                false,
                cx,
                move |this, window, cx| {
                    this.start_create_dir(pd.clone(), window, cx);
                },
            ));
        }
        items.push(self.render_context_menu_separator());
        // Rename: only for single selection
        if self.selected_sidebar_paths.len() <= 1 {
            let p = path.clone();
            items.push(self.render_context_menu_item(
                "重命名",
                false,
                cx,
                move |this, window, cx| {
                    this.start_rename(p.clone(), window, cx);
                },
            ));
        }
        {
            let selected: Vec<std::path::PathBuf> =
                self.selected_sidebar_paths.iter().cloned().collect();
            let count = selected.len();
            let delete_label = if count > 1 {
                format!("删除 ({count} 项)")
            } else {
                "删除".to_string()
            };
            items.push(self.render_context_menu_item(
                &delete_label,
                true,
                cx,
                move |this, _, cx| {
                    if selected.len() == 1 {
                        this.delete_path(selected[0].clone(), cx);
                    } else {
                        // Delete all selected paths
                        for p in &selected {
                            let result = if p.is_dir() {
                                std::fs::remove_dir_all(p)
                            } else {
                                std::fs::remove_file(p)
                            };
                            if let Err(e) = result {
                                eprintln!("Delete failed: {e}");
                            }
                        }
                        this.tabs.retain(|tab| {
                            tab.path
                                .as_ref()
                                .is_none_or(|tp| !selected.iter().any(|sp| tp.starts_with(sp)))
                        });
                        if this.tabs.is_empty() {
                            this.create_scratch_tab(cx);
                            return;
                        }
                        let active_still_exists = this
                            .active_tab_id
                            .as_ref()
                            .is_some_and(|id| this.tabs.iter().any(|t| &t.id == id));
                        if !active_still_exists {
                            this.activate_tab_by_index(0, cx);
                        }
                        this.selected_sidebar_paths.clear();
                        this.context_menu = None;
                        cx.notify();
                    }
                },
            ));
        }
        items.push(self.render_context_menu_separator());
        {
            let selected: Vec<std::path::PathBuf> =
                self.selected_sidebar_paths.iter().cloned().collect();
            let paths = if selected.is_empty() {
                vec![path.clone()]
            } else {
                selected
            };
            let cut_paths = paths.clone();
            items.push(
                self.render_context_menu_item("剪切", false, cx, move |this, _, cx| {
                    this.file_clipboard = Some(FileClipboard {
                        paths: cut_paths.clone(),
                        is_cut: true,
                    });
                    this.context_menu = None;
                    cx.notify();
                }),
            );
            items.push(
                self.render_context_menu_item("复制", false, cx, move |this, _, cx| {
                    this.file_clipboard = Some(FileClipboard {
                        paths: paths.clone(),
                        is_cut: false,
                    });
                    this.context_menu = None;
                    cx.notify();
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
        // Collapse / Expand all
        if !self.expanded_dirs.is_empty() {
            items.push(self.render_context_menu_item(
                "折叠所有文件夹",
                false,
                cx,
                move |this, _, cx| {
                    this.collapse_all_dirs(cx);
                },
            ));
        }
        {
            items.push(self.render_context_menu_item(
                "展开所有文件夹",
                false,
                cx,
                move |this, _, cx| {
                    this.expand_all_dirs(cx);
                },
            ));
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
        let active_actions = self.active_format_actions();

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
                        None,
                        shortcut,
                        false,
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

        // (label, filter_text, icon_path, shortcut, action)
        let all_menu_items: &[(&str, &str, Option<&str>, Option<&str>, FormatMenuAction)] = &[
            (
                "粗体",
                "bold",
                Some(FMT_BOLD_SVG),
                Some("⌘B"),
                FormatMenuAction::Bold,
            ),
            (
                "斜体",
                "italic",
                Some(FMT_ITALIC_SVG),
                Some("⌘I"),
                FormatMenuAction::Italic,
            ),
            (
                "删除线",
                "strikethrough",
                Some(FMT_STRIKETHROUGH_SVG),
                Some("⌘⇧S"),
                FormatMenuAction::Strikethrough,
            ),
            (
                "行内代码",
                "inline code",
                Some(FMT_INLINE_CODE_SVG),
                Some("⌘E"),
                FormatMenuAction::InlineCode,
            ),
            (
                "代码块",
                "code block",
                Some(FMT_CODE_SVG),
                Some("⌘⌥C"),
                FormatMenuAction::CodeBlock,
            ),
            (
                "分割线",
                "horizontal rule divider",
                None,
                Some("---"),
                FormatMenuAction::HorizontalRule,
            ),
            (
                "引用",
                "quote blockquote",
                Some(FMT_QUOTE_SVG),
                Some("⌘⌥Q"),
                FormatMenuAction::Quote,
            ),
            (
                "无序列表",
                "unordered list bullet",
                Some(FMT_LIST_SVG),
                Some("⌘⌥U"),
                FormatMenuAction::UnorderedList,
            ),
            (
                "有序列表",
                "ordered list numbered",
                Some(FMT_ORDER_LIST_SVG),
                Some("⌘⌥O"),
                FormatMenuAction::OrderedList,
            ),
            (
                "待办事项",
                "todo task checkbox",
                Some(FMT_TODO_SVG),
                Some("⌘⌥T"),
                FormatMenuAction::Todo,
            ),
            (
                "链接",
                "link url",
                Some(FMT_LINK_SVG),
                Some("⌘K"),
                FormatMenuAction::Link,
            ),
            (
                "图片",
                "image photo",
                Some(FMT_IMAGE_SVG),
                Some("⌘⇧I"),
                FormatMenuAction::Image,
            ),
        ];

        let group_boundaries = [0usize, 4, 10, 12];
        let mut group_start_flags = [false; 3];

        let slash_only_actions: &[FormatMenuAction] = &[
            FormatMenuAction::Todo,
            FormatMenuAction::CodeBlock,
            FormatMenuAction::Link,
            FormatMenuAction::Image,
        ];

        if is_slash_menu {
            // Render slash menu items in the fixed order defined by slash_only_actions
            for &slash_action in slash_only_actions.iter() {
                // Find the matching item in all_menu_items
                if let Some(&(label, ft, icon, shortcut, action)) = all_menu_items
                    .iter()
                    .find(|&&(_, _, _, _, a)| a == slash_action)
                {
                    if matches_filter(label, ft) {
                        let is_selected = actionable_index == selected_index;
                        let is_active = active_actions.contains(&action);
                        items.push(self.render_format_context_menu_item(
                            label,
                            icon,
                            shortcut,
                            is_selected,
                            is_active,
                            cx,
                            move |this, _, cx| {
                                this.execute_format_menu_action(action, cx);
                            },
                        ));
                        actionable_index += 1;
                    }
                }
            }
        } else {
            let group_boundaries = [0usize, 4, 10, 12];
            let mut group_start_flags = [false; 3];

            for (idx, &(label, ft, _, _, _)) in all_menu_items.iter().enumerate() {
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
                    let (label, ft, icon, shortcut, action) = all_menu_items[idx];
                    if matches_filter(label, ft) {
                        let is_selected = false;
                        let is_active = active_actions.contains(&action);
                        items.push(self.render_format_context_menu_item(
                            label,
                            icon,
                            shortcut,
                            is_selected,
                            is_active,
                            cx,
                            move |this, _, cx| {
                                this.execute_format_menu_action(action, cx);
                            },
                        ));
                    }
                }
                prev_group_had_items = true;
            }
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
                let is_pinned = self
                    .tabs
                    .iter()
                    .find(|t| t.id == *tab_id)
                    .is_some_and(|t| t.is_pinned);
                {
                    let id = tab_id.clone();
                    items.push(self.render_context_menu_item(
                        "关闭",
                        false,
                        cx,
                        move |this, _, cx| {
                            this.request_close_tab(&id, cx);
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
                        "关闭左侧",
                        false,
                        cx,
                        move |this, _, cx| {
                            this.close_left_tabs(&id, cx);
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
                items.push(self.render_context_menu_separator());
                {
                    let id = tab_id.clone();
                    let label = if is_pinned {
                        "取消固定"
                    } else {
                        "固定标签"
                    };
                    items.push(self.render_context_menu_item(
                        label,
                        false,
                        cx,
                        move |this, _, cx| {
                            this.toggle_pin_tab(&id, cx);
                        },
                    ));
                }
                // Copy path if tab has a file
                if let Some(tab) = self.tabs.iter().find(|t| t.id == *tab_id) {
                    if let Some(path) = &tab.path {
                        let path_str = path.display().to_string();
                        items.push(self.render_context_menu_item(
                            "复制路径",
                            false,
                            cx,
                            move |this, _, _cx| {
                                this.context_menu = None;
                                let _ = std::process::Command::new("pbcopy")
                                    .stdin(std::process::Stdio::piped())
                                    .spawn()
                                    .and_then(|mut child| {
                                        use std::io::Write;
                                        if let Some(stdin) = child.stdin.as_mut() {
                                            stdin.write_all(path_str.as_bytes())?;
                                        }
                                        child.wait()
                                    });
                            },
                        ));
                    }
                }
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
                let close_hover_bg = wss.ws_menu_item_close_hover_bg;
                let close_size = self.scaled(wss.ws_menu_item_close_size_px);
                let ms = &self.styles.context_menu;
                let row_hover_bg = ms.menu_item_hover_bg;
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
                            .hover(move |s| s.bg(row_hover_bg))
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
                                    .cursor(CursorStyle::Arrow)
                                    .hover(move |s| {
                                        s.text_color(close_hover_color).bg(close_hover_bg)
                                    })
                                    .child("✕")
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |this, _, window, cx| {
                                            window.prevent_default();
                                            cx.stop_propagation();
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
                items.push(self.render_context_menu_item_with_icon(
                    "打开目录 ...",
                    "assets/icons/filetree/all.svg",
                    false,
                    cx,
                    move |this, _, cx| {
                        this.pick_workspace_folder(cx);
                    },
                ));
                items.push(self.render_context_menu_separator());
                items.push(self.render_context_menu_item_with_icon(
                    "打开设置...",
                    "assets/icons/setting.svg",
                    false,
                    cx,
                    move |this, _, cx| {
                        this.open_settings_tab(cx);
                    },
                ));
                (items, self.scaled(wss.ws_menu_width_px))
            }
        };

        let ms = &self.styles.context_menu;

        // Estimate total menu height from items
        let item_count = items.len();
        let estimated_item_height = self.scaled(ms.menu_item_height_px);
        let menu_padding = self.scaled(ms.menu_padding_y_px) * 2.0;
        let estimated_menu_height = item_count as f32 * estimated_item_height + menu_padding;

        // Use window bounds for positioning, adjusted for editor-root border
        let border_w = self.styles.window.window_border_width_px;
        let overlay_w = self.last_window_width - 2.0 * border_w;
        let overlay_h = self.last_window_height - 2.0 * border_w;
        // Convert window-coordinate mouse position to overlay-local coordinates
        let pos_x = (pos_x - border_w).clamp(0.0, overlay_w);
        let pos_y = (pos_y - border_w).clamp(0.0, overlay_h);
        let margin = self.scaled(ms.menu_max_height_margin_px).max(4.0);

        let space_right = overlay_w - pos_x - margin;
        let space_left = pos_x - margin;
        let space_below = overlay_h - pos_y - margin;
        let space_above = pos_y - margin;

        // Decide direction
        let open_right = space_right >= width || space_right >= space_left;
        let open_down = space_below >= estimated_menu_height || space_below >= space_above;

        let final_x = if open_right {
            pos_x
        } else {
            (pos_x - width).max(margin)
        };

        let max_menu_height = if open_down {
            (overlay_h - pos_y - margin).max(self.scaled(100.0))
        } else {
            (pos_y - margin).max(self.scaled(100.0))
        };

        // Use .top() for downward, .bottom() for upward to anchor the correct edge
        let mut menu_div = div()
            .id("context-menu")
            .absolute()
            .left(px(final_x))
            .w(px(width))
            .max_h(px(max_menu_height))
            .overflow_y_scroll()
            .py(px(self.scaled(ms.menu_padding_y_px)))
            .bg(ms.menu_bg)
            .border(px(ms.menu_border_width_px))
            .border_color(ms.menu_border_color)
            .rounded(px(self.scaled(ms.menu_radius_px)));
        if open_down {
            menu_div = menu_div.top(px(pos_y));
        } else {
            // Anchor bottom edge of menu at cursor y position
            menu_div = menu_div.bottom(px(overlay_h - pos_y));
        }
        if ms.menu_shadow {
            menu_div = menu_div.shadow_sm();
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
