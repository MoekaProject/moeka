use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{AnyElement, Context, Entity, Task, div, prelude::*, px};

use super::*;

const FILTER_DELAY: std::time::Duration = std::time::Duration::from_millis(150);

#[derive(Clone)]
struct FilterEntry {
    item: SidebarItem,
    name: String,
}

#[derive(Default)]
pub(super) struct SidebarFilter {
    input: Option<Entity<crate::text_input::TextInput>>,
    query: String,
    generation: usize,
    requested_epoch: Option<usize>,
    index: Option<(usize, Arc<Vec<FilterEntry>>)>,
    debounce: Option<Task<()>>,
    running: bool,
    cancel: Arc<AtomicBool>,
    matches: Vec<SidebarItem>,
    collapsed: HashSet<PathBuf>,
    pub(super) items: Rc<Vec<SidebarItem>>,
}

fn scan_filter_index(
    root: &std::path::Path,
    sort: SidebarSortMode,
    cancel: &AtomicBool,
) -> std::io::Result<Option<Vec<FilterEntry>>> {
    let mut index = Vec::new();
    let mut pending = vec![(root.to_path_buf(), 0, None)];
    while let Some((path, depth, entry)) = pending.pop() {
        if cancel.load(Ordering::Relaxed) {
            return Ok(None);
        }
        crate::platform::fs_task::check_deadline()?;
        let is_dir = entry
            .as_ref()
            .is_none_or(|child: &sidebar_tree::SidebarDirectoryChild| child.is_dir);
        if let Some(child) = entry {
            index.push(FilterEntry {
                name: child.name.to_lowercase(),
                item: SidebarItem {
                    name: child.name,
                    path: child.path,
                    depth,
                    is_dir,
                    is_expanded: is_dir,
                },
            });
        }
        if is_dir {
            let children = sidebar_tree::load_sidebar_directory(&path, sort).map_err(|error| {
                std::io::Error::new(error.kind(), format!("{}: {error}", path.display()))
            })?;
            pending.extend(
                children
                    .into_iter()
                    .rev()
                    .map(|child| (child.path.clone(), depth + 1, Some(child))),
            );
        }
    }
    Ok(Some(index))
}

// Preorder propagates directory matches downwards; reverse preorder retains
// ancestors of matching files in linear time without per-match path walks.
fn filter_index(
    index: &[FilterEntry],
    query: &str,
    cancel: &AtomicBool,
) -> Option<Vec<SidebarItem>> {
    let mut keep = vec![false; index.len()];
    let mut matched_directory_depth = None;
    for (i, entry) in index.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        if matched_directory_depth.is_some_and(|depth| entry.item.depth <= depth) {
            matched_directory_depth = None;
        }
        keep[i] = matched_directory_depth.is_some() || entry.name.contains(query);
        if keep[i] && entry.item.is_dir && matched_directory_depth.is_none() {
            matched_directory_depth = Some(entry.item.depth);
        }
    }
    let mut ancestor_depth = None;
    for (i, entry) in index.iter().enumerate().rev() {
        if cancel.load(Ordering::Relaxed) {
            return None;
        }
        if ancestor_depth.is_some_and(|depth| entry.item.depth < depth) && entry.item.is_dir {
            keep[i] = true;
        }
        if keep[i] {
            ancestor_depth = Some(entry.item.depth);
        }
    }
    Some(
        index
            .iter()
            .zip(keep)
            .filter(|(_, keep)| *keep)
            .map(|(entry, _)| entry.item.clone())
            .collect(),
    )
}

fn visible_filtered_items(
    matches: &[SidebarItem],
    collapsed: &HashSet<PathBuf>,
) -> Vec<SidebarItem> {
    let mut hidden_depth = None;
    let mut items = Vec::new();
    for item in matches {
        if hidden_depth.is_some_and(|depth| item.depth > depth) {
            continue;
        }
        hidden_depth = None;
        let mut item = item.clone();
        item.is_expanded = item.is_dir && !collapsed.contains(&item.path);
        if item.is_dir && !item.is_expanded {
            hidden_depth = Some(item.depth);
        }
        items.push(item);
    }
    items
}

impl SidebarFilter {
    fn rebuild_visible(&mut self) {
        self.items = Rc::new(visible_filtered_items(&self.matches, &self.collapsed));
    }
}

pub(super) fn filter_match_ranges(name: &str, query: &str) -> Vec<Range<usize>> {
    if query.is_empty() {
        return Vec::new();
    }
    let lower = name.to_lowercase();
    let mut source_ranges = Vec::with_capacity(lower.len());
    for (start, ch) in name.char_indices() {
        let range = start..start + ch.len_utf8();
        for lowered in ch.to_lowercase() {
            source_ranges.extend(std::iter::repeat_n(range.clone(), lowered.len_utf8()));
        }
    }
    let mut ranges: Vec<Range<usize>> = Vec::new();
    for (start, matched) in lower.match_indices(query) {
        let range = source_ranges[start].start..source_ranges[start + matched.len() - 1].end;
        if let Some(previous) = ranges.last_mut()
            && previous.end >= range.start
        {
            previous.end = previous.end.max(range.end);
        } else {
            ranges.push(range);
        }
    }
    ranges
}

impl EditorView {
    pub(super) fn sidebar_filter_match_ranges(&self, name: &str) -> Vec<Range<usize>> {
        filter_match_ranges(name, &self.sidebar_filter.query)
    }

    pub(super) fn focus_sidebar_filter(
        &mut self,
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) {
        self.sidebar_visible = true;
        self.sync_sidebar_width_animation(cx);
        self.ensure_sidebar_filter_input(cx);
        self.ensure_sidebar_filter_input(cx);
        if let Some(input) = &self.sidebar_filter.input {
            input.read(cx).focus_handle.focus(window);
        }
        cx.notify();
    }

    pub(super) fn invalidate_sidebar_filter_index(&mut self) {
        self.sidebar_filter.cancel.store(true, Ordering::Relaxed);
        self.sidebar_filter.generation = self.sidebar_filter.generation.wrapping_add(1);
        self.sidebar_filter.index = None;
        self.sidebar_filter.requested_epoch = None;
    }
    pub(super) fn sidebar_filter_active(&self) -> bool {
        !self.sidebar_filter.query.is_empty()
    }

    pub(super) fn sync_sidebar_filter_input(&self, cx: &mut Context<Self>) {
        if let Some(input) = &self.sidebar_filter.input {
            let styles = self
                .styles
                .input_styles(crate::style::InputRole::SidebarFilter, self.ui_scale);
            let placeholder = self.t(crate::i18n::TextKey::SidebarFilterPlaceholder);
            input.update(cx, |input, cx| {
                input.placeholder = placeholder.into();
                input.set_styles(styles, cx);
            });
        }
    }

    pub(super) fn reset_sidebar_filter(&mut self, cx: &mut Context<Self>) {
        self.sidebar_filter.cancel.store(true, Ordering::Relaxed);
        self.sidebar_filter.generation = self.sidebar_filter.generation.wrapping_add(1);
        self.sidebar_filter.query.clear();
        self.sidebar_filter.index = None;
        self.sidebar_filter.requested_epoch = None;
        self.sidebar_filter.debounce = None;
        self.sidebar_filter.matches.clear();
        self.sidebar_filter.collapsed.clear();
        self.sidebar_filter.items = Rc::new(Vec::new());
        if let Some(input) = &self.sidebar_filter.input {
            input.update(cx, |input, cx| input.set_text("", cx));
        }
    }

    pub(super) fn toggle_filtered_directory(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if !self.sidebar_filter.collapsed.insert(path.clone()) {
            self.sidebar_filter.collapsed.remove(&path);
        }
        self.sidebar_filter.rebuild_visible();
        cx.notify();
    }

    pub(super) fn set_filtered_directories_collapsed(
        &mut self,
        collapsed: bool,
        cx: &mut Context<Self>,
    ) {
        self.sidebar_filter.collapsed = if collapsed {
            self.sidebar_filter
                .matches
                .iter()
                .filter(|item| item.is_dir)
                .map(|item| item.path.clone())
                .collect()
        } else {
            HashSet::new()
        };
        self.context_menu = None;
        self.sidebar_filter.rebuild_visible();
        cx.notify();
    }

    pub(super) fn ensure_sidebar_filter_current(&mut self, cx: &mut Context<Self>) {
        if self.sidebar_filter_active()
            && self.sidebar_filter.requested_epoch != Some(self.sidebar_directory_epoch)
        {
            self.schedule_sidebar_filter(cx);
        }
    }

    fn schedule_sidebar_filter(&mut self, cx: &mut Context<Self>) {
        self.sidebar_filter.cancel.store(true, Ordering::Relaxed);
        self.sidebar_filter.generation = self.sidebar_filter.generation.wrapping_add(1);
        self.sidebar_filter.requested_epoch = Some(self.sidebar_directory_epoch);
        self.sidebar_filter.debounce = None;
        if !self.sidebar_filter_active() {
            self.sidebar_filter.collapsed.clear();
            self.sidebar_filter.matches.clear();
            self.sidebar_filter.items = Rc::new(Vec::new());
            cx.notify();
            return;
        }
        self.sidebar_tree_motion = None;
        self.sidebar_filter.debounce = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(FILTER_DELAY).await;
            this.update(cx, |this, cx| this.start_sidebar_filter(cx))
                .ok();
        }));
    }

    fn start_sidebar_filter(&mut self, cx: &mut Context<Self>) {
        // A canceled mount-backed read keeps its slot until it really finishes.
        if self.sidebar_filter.running || !self.sidebar_filter_active() {
            return;
        }
        let root = self.sidebar_root.clone();
        if root.as_os_str().is_empty() {
            return;
        }
        let epoch = self.sidebar_directory_epoch;
        let generation = self.sidebar_filter.generation;
        let query = self.sidebar_filter.query.clone();
        let sort = self.sidebar_sort_mode;
        let collapsed = self.sidebar_filter.collapsed.clone();
        let index = self
            .sidebar_filter
            .index
            .as_ref()
            .filter(|(index_epoch, _)| *index_epoch == epoch)
            .map(|(_, index)| index.clone());
        let cancel = Arc::new(AtomicBool::new(false));
        self.sidebar_filter.cancel = cancel.clone();
        self.sidebar_filter.running = true;
        cx.spawn(async move |this, cx| {
            let scan_root = root.clone();
            let background_collapsed = collapsed.clone();
            let result = crate::platform::fs_task::run_background(
                "Filter sidebar files",
                root.clone(),
                move || {
                    let index = match index {
                        Some(index) => index,
                        None => {
                            let Some(index) = scan_filter_index(&scan_root, sort, &cancel)? else {
                                return Ok(None);
                            };
                            Arc::new(index)
                        }
                    };
                    let Some(items) = filter_index(&index, &query, &cancel) else {
                        return Ok(None);
                    };
                    let visible = visible_filtered_items(&items, &background_collapsed);
                    if cancel.load(Ordering::Relaxed) {
                        return Ok(None);
                    }
                    Ok(Some((index, items, visible)))
                },
            )
            .await;
            this.update(cx, |this, cx| {
                this.sidebar_filter.running = false;
                let current = this.sidebar_root == root
                    && this.sidebar_directory_epoch == epoch
                    && this.sidebar_filter.generation == generation;
                if current {
                    match result {
                        Ok(Some((index, items, visible))) => {
                            this.sidebar_filter.index = Some((epoch, index));
                            this.sidebar_filter.matches = items;
                            if this.sidebar_filter.collapsed == collapsed {
                                this.sidebar_filter.items = Rc::new(visible);
                            } else {
                                this.sidebar_filter.rebuild_visible();
                            }
                            this.pending_sidebar_scroll_anchor = None;
                            this.pending_sidebar_scroll_top.set(None);
                            this.sidebar_scroll_handle
                                .set_offset(gpui::point(px(0.0), px(0.0)));
                            this.clear_sidebar_fs_error("Filter sidebar files", &root);
                        }
                        Ok(None) => {}
                        Err(error) => {
                            this.report_sidebar_fs_error("Filter sidebar files", &root, error, cx);
                        }
                    }
                    cx.notify();
                } else if this.sidebar_filter_active() {
                    this.schedule_sidebar_filter(cx);
                }
            })
            .ok();
        })
        .detach();
    }

    fn sidebar_filter_has_text(&self, cx: &gpui::App) -> bool {
        self.sidebar_filter
            .input
            .as_ref()
            .is_some_and(|input| !input.read(cx).text().is_empty())
    }

    fn update_sidebar_filter_query(&mut self, cx: &mut Context<Self>) {
        let query = self
            .sidebar_filter
            .input
            .as_ref()
            .map(|input| input.read(cx).text().trim().to_lowercase())
            .unwrap_or_default();
        if self.sidebar_filter.query != query {
            self.sidebar_filter.query = query;
            self.schedule_sidebar_filter(cx);
        }
        cx.notify();
    }

    fn clear_sidebar_filter(&mut self, window: &mut gpui::Window, cx: &mut Context<Self>) {
        self.clear_sidebar_filter_text(cx);
        if let Some(input) = &self.sidebar_filter.input {
            input.read(cx).focus_handle.focus(window);
        }
    }

    pub(super) fn clear_sidebar_filter_text(&mut self, cx: &mut Context<Self>) {
        if !self.sidebar_filter_has_text(cx) && self.sidebar_filter.query.is_empty() {
            return;
        }
        if let Some(input) = &self.sidebar_filter.input {
            input.update(cx, |input, cx| input.set_text("", cx));
        }
        self.update_sidebar_filter_query(cx);
    }

    pub(super) fn render_sidebar_filter(&self, cx: &mut Context<Self>) -> AnyElement {
        let ss = &self.styles.sidebar;
        div()
            .w_full()
            .h(px(self.scaled(ss.sidebar_header_height_px)))
            .flex_none()
            .flex()
            .items_center()
            .pl(px(self.scaled(ss.sidebar_header_padding_left_px)))
            .pr(px(self.scaled(ss.sidebar_header_padding_right_px)))
            .gap(px(self.scaled(ss.sidebar_header_gap_px)))
            .child(crate::ui::decoration::icon(
                crate::style::IconStyle::new(
                    self.scaled(ss.sidebar_header_action_icon_size_px),
                    ss.sidebar_filter_muted_color,
                ),
                FILETREE_SEARCH_SVG_PATH,
            ))
            .when_some(self.sidebar_filter.input.as_ref(), |this, input| {
                this.child(div().flex_1().min_w(px(0.0)).child(input.clone()))
            })
            .when(self.sidebar_filter_has_text(cx), |this| {
                this.child(self.render_sidebar_header_action_button(
                    "sidebar-filter-clear",
                    FILETREE_CLOSE_SVG_PATH,
                    crate::i18n::search_option_label(self.ui_language, "clear_filter"),
                    None,
                    cx,
                    |this, _, window, cx| {
                        window.prevent_default();
                        this.clear_sidebar_filter(window, cx);
                    },
                ))
            })
            .into_any_element()
    }

    pub(super) fn ensure_sidebar_filter_input(&mut self, cx: &mut Context<Self>) {
        if self.sidebar_filter.input.is_some() {
            return;
        }
        let styles = self
            .styles
            .input_styles(crate::style::InputRole::SidebarFilter, self.ui_scale);
        let placeholder = self.t(crate::i18n::TextKey::SidebarFilterPlaceholder);
        let input = cx.new(|cx| crate::text_input::TextInput::new(styles, placeholder, cx));
        self.sidebar_filter.input = Some(input.clone());
        cx.subscribe(&input, |this, _, event, cx| match event {
            crate::text_input::TextInputEvent::Changed => this.update_sidebar_filter_query(cx),
            crate::text_input::TextInputEvent::Escape => this.clear_sidebar_filter_text(cx),
            _ => {}
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_highlights_preserve_original_unicode_byte_ranges() {
        assert_eq!(
            filter_match_ranges("NOTE-note.md", "note"),
            vec![0..4, 5..9]
        );
        assert_eq!(filter_match_ranges("中文中文.md", "中文"), vec![0..12]);
        assert_eq!(filter_match_ranges("İstanbul.md", "i"), vec![0..2]);
        assert_eq!(filter_match_ranges("DESPUÉS.md", "después"), vec![0..8]);
        assert_eq!(filter_match_ranges("note.md", "md"), vec![5..7]);
        assert!(filter_match_ranges("ancestor", "note").is_empty());
        assert!(filter_match_ranges("note.md", "").is_empty());
    }

    fn entries() -> Vec<FilterEntry> {
        [
            ("a", true),
            ("a/empty", true),
            ("a/nested", true),
            ("a/nested/NOTE.md", false),
            ("a/other.txt", false),
            ("目录", true),
            ("目录/中文.md", false),
            ("root.md", false),
        ]
        .into_iter()
        .map(|(path, is_dir)| {
            let path = PathBuf::from(path);
            let name = path.file_name().unwrap().to_string_lossy().into_owned();
            FilterEntry {
                name: name.to_lowercase(),
                item: SidebarItem {
                    depth: path.components().count(),
                    path,
                    name,
                    is_dir,
                    is_expanded: is_dir,
                },
            }
        })
        .collect()
    }

    #[test]
    fn sidebar_filter_keeps_matching_names_ancestors_and_directory_subtrees() {
        let mut index = entries();
        let cancel = AtomicBool::new(false);
        for mask in 0..(1 << index.len()) {
            for (i, entry) in index.iter_mut().enumerate() {
                entry.name = if mask & (1 << i) != 0 { "match" } else { "_" }.into();
            }
            let actual = filter_index(&index, "match", &cancel).unwrap();
            let expected = index
                .iter()
                .filter(|entry| {
                    index.iter().any(|matched| {
                        matched.name == "match"
                            && (matched.item.path == entry.item.path
                                || (matched.item.is_dir
                                    && entry.item.path.starts_with(&matched.item.path))
                                || (entry.item.is_dir
                                    && matched.item.path.starts_with(&entry.item.path)))
                    })
                })
                .map(|entry| entry.item.path.clone())
                .collect::<Vec<_>>();
            assert_eq!(
                actual.into_iter().map(|item| item.path).collect::<Vec<_>>(),
                expected,
                "match mask {mask}"
            );
        }
    }

    #[test]
    fn sidebar_filter_matches_case_insensitively_and_handles_unicode_and_no_matches() {
        let index = entries();
        let cancel = AtomicBool::new(false);
        assert_eq!(filter_index(&index, "note", &cancel).unwrap().len(), 3);
        assert_eq!(filter_index(&index, "中文", &cancel).unwrap().len(), 2);
        assert!(filter_index(&index, "absent", &cancel).unwrap().is_empty());
        cancel.store(true, Ordering::Relaxed);
        assert!(filter_index(&index, "note", &cancel).is_none());
    }

    #[test]
    fn sidebar_filter_scan_visits_collapsed_descendants_and_preserves_tree_order() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("folder/nested")).unwrap();
        std::fs::write(root.path().join("folder/nested/NOTE.md"), "").unwrap();
        std::fs::write(root.path().join(".hidden.md"), "").unwrap();
        std::fs::write(root.path().join("z.txt"), "").unwrap();
        let cancel = AtomicBool::new(false);
        let index = scan_filter_index(root.path(), SidebarSortMode::NameAsc, &cancel)
            .unwrap()
            .unwrap();
        assert_eq!(
            index
                .iter()
                .map(|entry| entry.item.name.as_str())
                .collect::<Vec<_>>(),
            ["folder", "nested", "NOTE.md", "z.txt"]
        );
        assert_eq!(filter_index(&index, "note", &cancel).unwrap().len(), 3);
        assert!(
            scan_filter_index(
                &root.path().join("missing"),
                SidebarSortMode::NameAsc,
                &cancel
            )
            .is_err()
        );
        cancel.store(true, Ordering::Relaxed);
        assert!(
            scan_filter_index(root.path(), SidebarSortMode::NameAsc, &cancel)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn sidebar_filter_collapse_only_changes_filtered_presentation() {
        let mut filter = SidebarFilter {
            matches: entries().into_iter().map(|entry| entry.item).collect(),
            collapsed: HashSet::from([PathBuf::from("a")]),
            ..Default::default()
        };
        filter.rebuild_visible();
        assert_eq!(filter.items.len(), 4);
        assert!(!filter.items[0].is_expanded);
        assert!(filter.matches[0].is_expanded);
        filter.collapsed.clear();
        filter.rebuild_visible();
        assert_eq!(filter.items.len(), 8);
    }

    #[cfg(feature = "gpui-test-support")]
    struct FilterHarness(Entity<EditorView>);

    #[cfg(feature = "gpui-test-support")]
    impl gpui::Render for FilterHarness {
        fn render(
            &mut self,
            _window: &mut gpui::Window,
            cx: &mut Context<Self>,
        ) -> impl IntoElement {
            self.0
                .update(cx, |editor, cx| editor.ensure_sidebar_filter_input(cx));
            div()
                .w(px(300.0))
                .track_focus(&self.0.read(cx).focus_handle)
                .key_context("MarkdownEditor")
                .capture_action({
                    let editor = self.0.clone();
                    move |_: &crate::actions::Escape, _, cx| {
                        editor.update(cx, |editor, cx| editor.clear_search_inputs(cx));
                    }
                })
                .on_action(|_: &crate::actions::Escape, _, _| {})
                .capture_key_down({
                    let editor = self.0.clone();
                    move |event: &gpui::KeyDownEvent, _, cx| {
                        if event.keystroke.key == "escape" {
                            editor.update(cx, |editor, cx| editor.clear_search_inputs(cx));
                        }
                    }
                })
                .child(
                    self.0
                        .update(cx, |editor, cx| editor.render_sidebar_filter(cx)),
                )
        }
    }

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn starred_filter_matches_unicode_and_extensions_and_clear_button_restores_lists(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::{AppContext, MouseButton, MouseDownEvent, point};
        cx.update(|cx| {
            cx.bind_keys([gpui::KeyBinding::new(
                "escape",
                crate::actions::Escape,
                Some("MarkdownEditor"),
            )])
        });

        let editor = cx
            .update(|cx| cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some(""), false)));
        let stars: Vec<PathBuf> = [
            "workspace/DESPUÉS.md",
            "workspace/中文.txt",
            "workspace/other.md",
            "another/DESPUÉS.md",
        ]
        .into_iter()
        .map(PathBuf::from)
        .collect();
        let input = editor.update(cx, |editor, cx| {
            editor.sidebar_root = "workspace".into();
            editor.app_settings.starred_files = stars.clone();
            editor.sidebar_items_dirty = false;
            editor.sidebar_items_cache = Rc::new(vec![entries()[7].item.clone()]);
            editor.ensure_sidebar_filter_input(cx);
            editor.sidebar_filter.input.clone().unwrap()
        });
        let (_, window_cx) = cx.add_window_view(|_, _| FilterHarness(editor.clone()));
        window_cx.run_until_parked();
        for (query, expected) in [
            (" DESPUÉS ", vec!["workspace/DESPUÉS.md"]),
            ("中文", vec!["workspace/中文.txt"]),
            (".MD", vec!["workspace/DESPUÉS.md", "workspace/other.md"]),
            ("absent", vec![]),
            (
                "   ",
                vec![
                    "workspace/DESPUÉS.md",
                    "workspace/中文.txt",
                    "workspace/other.md",
                ],
            ),
        ] {
            window_cx.update(|_, cx| {
                input.update(cx, |input, cx| {
                    input.set_text(query, cx);
                    cx.emit(crate::text_input::TextInputEvent::Changed);
                })
            });
            window_cx.run_until_parked();
            window_cx.update(|_, cx| {
                let editor = editor.read(cx);
                assert!(editor.sidebar_filter_has_text(cx));
                assert_eq!(
                    editor.visible_starred_sidebar_files(),
                    expected.into_iter().map(PathBuf::from).collect::<Vec<_>>()
                );
                assert_eq!(
                    editor.sidebar_visible_starred_count(),
                    editor.visible_starred_sidebar_files().len()
                );
                assert_eq!(editor.app_settings.starred_files, stars);
                if query == " DESPUÉS " {
                    assert_eq!(editor.sidebar_filter_match_ranges("DESPUÉS.md"), vec![0..8]);
                }
                if query == ".MD" {
                    assert_eq!(
                        editor.sidebar_filter_match_ranges("DESPUÉS.md"),
                        vec![8..11]
                    );
                }
            });
            let position = window_cx.update(|_, cx| {
                let editor = editor.read(cx);
                let ss = &editor.styles.sidebar;
                point(
                    px(300.0
                        - editor.scaled(
                            ss.sidebar_header_padding_right_px
                                + ss.sidebar_header_action_btn_size_px * 0.5,
                        )),
                    px(editor.scaled(ss.sidebar_header_height_px * 0.5)),
                )
            });
            window_cx.simulate_event(MouseDownEvent {
                position,
                button: MouseButton::Left,
                ..Default::default()
            });
            window_cx.run_until_parked();
            window_cx.update(|window, cx| {
                let editor = editor.read(cx);
                assert_eq!(input.read(cx).text(), "");
                assert!(input.read(cx).focus_handle.is_focused(window));
                assert!(!editor.sidebar_filter_has_text(cx));
                assert!(!editor.sidebar_filter_active());
                assert!(editor.sidebar_filter.debounce.is_none());
                assert_eq!(editor.visible_starred_sidebar_files().len(), 3);
            });
            window_cx.update(|_, cx| {
                editor.update(cx, |editor, _| {
                    assert_eq!(editor.sidebar_items()[0].name, "root.md");
                });
            });
        }
        for blur in [false, true] {
            window_cx.update(|window, cx| {
                window.activate_window();
                input.update(cx, |input, cx| {
                    input.focus_handle.focus(window);
                    input.set_text("note", cx);
                    cx.emit(crate::text_input::TextInputEvent::Changed);
                });
            });
            window_cx.run_until_parked();
            if blur {
                window_cx.update(|window, _| window.blur());
            } else {
                window_cx.simulate_keystrokes("escape");
            }
            window_cx.run_until_parked();
            window_cx.update(|window, cx| {
                let editor = editor.read(cx);
                assert_eq!(
                    input.read(cx).text(),
                    if blur { "note" } else { "" },
                    "blur={blur}"
                );
                assert_eq!(editor.sidebar_filter_active(), blur);
                if !blur {
                    assert!(editor.sidebar_filter.debounce.is_none());
                    assert_eq!(editor.visible_starred_sidebar_files().len(), 3);
                }
                assert_eq!(input.read(cx).focus_handle.is_focused(window), !blur);
            });
        }
        window_cx.update(|window, cx| {
            editor.update(cx, |editor, cx| {
                editor.search_query = "note".into();
                editor.run_search(cx);
                editor.workspace_search_query = "note".into();
                editor.workspace_search_include = "*.md".into();
                editor.workspace_search_exclude = "temp".into();
                window.focus(&editor.focus_handle);
            });
        });
        window_cx.simulate_keystrokes("escape");
        window_cx.run_until_parked();
        window_cx.update(|_, cx| {
            let editor = editor.read(cx);
            assert_eq!(input.read(cx).text(), "");
            assert!(editor.search_query.is_empty());
            assert!(editor.search_matches.is_empty());
            assert!(editor.workspace_search_query.is_empty());
            assert!(editor.workspace_search_include.is_empty());
            assert!(editor.workspace_search_exclude.is_empty());
        });
    }

    #[cfg(feature = "gpui-test-support")]
    #[gpui::test]
    fn sidebar_filter_debounces_typing_and_clearing_restores_the_original_tree(
        cx: &mut gpui::TestAppContext,
    ) {
        use gpui::AppContext;
        let editor = cx
            .update(|cx| cx.new(|cx| EditorView::new_with_mode(cx, true, false, Some(""), false)));
        let input = editor.update(cx, |editor, cx| {
            editor.sidebar_root = "workspace".into();
            editor.expanded_dirs = HashSet::from([editor.sidebar_root.clone()]);
            editor.sidebar_items_dirty = false;
            editor.sidebar_items_cache = Rc::new(vec![entries()[7].item.clone()]);
            editor.ensure_sidebar_filter_input(cx);
            editor.sidebar_filter.input.clone().unwrap()
        });
        for query in ["n", "no", "note"] {
            input.update(cx, |input, cx| {
                input.set_text(query, cx);
                cx.emit(crate::text_input::TextInputEvent::Changed);
            });
        }
        editor.update(cx, |editor, _| {
            assert_eq!(editor.sidebar_filter.query, "note");
            assert!(!editor.sidebar_filter.running);
            assert!(editor.sidebar_filter.debounce.is_some());
        });
        cx.executor().run_until_parked();
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(149));
        cx.executor().run_until_parked();
        editor.update(cx, |editor, cx| {
            assert!(!editor.sidebar_filter.running);
            let original_expanded = editor.expanded_dirs.clone();
            editor.sidebar_filter.matches = entries().into_iter().map(|entry| entry.item).collect();
            editor.toggle_filtered_directory("a".into(), cx);
            assert_eq!(editor.expanded_dirs, original_expanded);
            editor.collapse_all_dirs(cx);
            assert!(editor.sidebar_items().iter().all(|item| item.depth == 1));
            editor.expand_all_dirs(cx);
            assert_eq!(editor.sidebar_items().len(), 8);
            assert_eq!(editor.expanded_dirs, original_expanded);
        });
        input.update(cx, |input, cx| {
            input.set_text("", cx);
            cx.emit(crate::text_input::TextInputEvent::Changed);
        });
        editor.update(cx, |editor, _| {
            assert!(editor.sidebar_filter.debounce.is_none());
            assert!(!editor.sidebar_filter_active());
            assert_eq!(editor.sidebar_items()[0].name, "root.md");
        });
        input.update(cx, |input, cx| {
            input.set_text("note", cx);
            cx.emit(crate::text_input::TextInputEvent::Changed);
        });
        editor.update(cx, |editor, cx| {
            let generation = editor.sidebar_filter.generation;
            editor.invalidate_sidebar_filter_index();
            assert!(editor.sidebar_filter.generation != generation);
            assert!(editor.sidebar_filter.cancel.load(Ordering::Relaxed));
            editor.reset_sidebar_filter(cx);
            assert_eq!(input.read(cx).text(), "");
            assert!(!editor.sidebar_filter_active());
            assert!(editor.sidebar_filter.index.is_none());
            assert!(editor.sidebar_filter.debounce.is_none());
        });
    }
}
