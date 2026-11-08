use std::{
    collections::{HashMap, HashSet},
    ops::Range,
    path::PathBuf,
    rc::Rc,
    time::Instant,
};

use gpui::{AnyElement, Context, Render, Subscription, WeakEntity, Window, div, prelude::*, px};

use super::{EditorView, SidebarItem};
use crate::ui::motion::Transition;

pub(super) struct SidebarMotionView {
    editor: WeakEntity<EditorView>,
    refresh: bool,
    _subscription: Subscription,
}

pub(super) struct StarredMotion {
    paths: Vec<PathBuf>,
    progress: Transition,
    frame_progress: f32,
}

impl Render for SidebarMotionView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(editor) = self.editor.upgrade() else {
            return div().into_any_element();
        };
        let refresh = std::mem::replace(&mut self.refresh, false);
        let (content, running) = editor.update(cx, |editor, cx| {
            if !window.is_window_active() {
                editor.sidebar_tree_motion = None;
                editor.sidebar_starred_motion = None;
            }
            let finished = editor
                .sidebar_tree_motion
                .as_ref()
                .is_some_and(|motion| !motion.progress.running(Instant::now()))
                || editor
                    .sidebar_starred_motion
                    .as_ref()
                    .is_some_and(|motion| !motion.progress.running(Instant::now()));
            let content = editor.render_sidebar(
                refresh
                    || finished
                    || (editor.sidebar_tree_motion.is_none()
                        && editor.sidebar_starred_motion.is_none()),
                cx,
            );
            let running = editor
                .sidebar_tree_motion
                .as_ref()
                .is_some_and(|motion| motion.progress.running(Instant::now()))
                || editor
                    .sidebar_starred_motion
                    .as_ref()
                    .is_some_and(|motion| motion.progress.running(Instant::now()));
            (content, running)
        });
        if running {
            window.request_presentation_frame();
        }
        content
    }
}

pub(super) struct TreeMotion {
    path: PathBuf,
    root: PathBuf,
    canonical: Rc<Vec<SidebarItem>>,
    expanded: Rc<Vec<SidebarItem>>,
    children: Range<usize>,
    expanding: bool,
    progress: Transition,
    guides: Option<Rc<Vec<HashSet<usize>>>>,
}

#[derive(Clone)]
pub(super) struct TreeFrame {
    children: Range<usize>,
    progress: f32,
}

fn descendants(items: &[SidebarItem], path: &std::path::Path) -> Range<usize> {
    let Some(parent) = items.iter().position(|item| item.path == path) else {
        return 0..0;
    };
    let start = parent + 1;
    let count = items[start..]
        .iter()
        .take_while(|item| item.depth > items[parent].depth)
        .count();
    start..start + count
}

impl TreeFrame {
    pub(super) fn new(children: Range<usize>, progress: f32) -> Self {
        Self { children, progress }
    }
    pub(super) fn offset(&self, item_height: f32) -> f32 {
        -(1.0 - self.progress) * self.children.len() as f32 * item_height
    }

    pub(super) fn ranges(&self, normal: Range<usize>, shifted: Range<usize>) -> Vec<Range<usize>> {
        let before = normal.start..normal.end.min(self.children.end);
        let after = shifted.start.max(self.children.end)..shifted.end;
        [before, after]
            .into_iter()
            .filter(|range| !range.is_empty())
            .collect()
    }

    pub(super) fn render_row(&self, index: usize, item_height: f32, row: AnyElement) -> AnyElement {
        if index < self.children.start {
            return row;
        }
        let child = self.children.contains(&index);
        let row = div()
            .w_full()
            .flex_none()
            .opacity(if child { self.progress } else { 1.0 })
            .child(row);
        crate::ui::motion::slide(
            row,
            if child { 0.0 } else { self.offset(item_height) },
            child.then(|| {
                let top = (self.children.start as f32 - index as f32) * item_height;
                top..top + self.progress * self.children.len() as f32 * item_height
            }),
        )
        .into_any_element()
    }
}

impl EditorView {
    pub(super) fn toggle_starred_section(&mut self, cx: &mut Context<Self>) {
        self.sidebar_starred_collapsed = !self.sidebar_starred_collapsed;
        let expanding = !self.sidebar_starred_collapsed;
        let paths = self.visible_starred_sidebar_files();
        let now = Instant::now();
        let mut motion = self
            .sidebar_starred_motion
            .take()
            .filter(|motion| motion.paths == paths)
            .unwrap_or_else(|| StarredMotion {
                paths,
                frame_progress: if expanding { 0.0 } else { 1.0 },
                progress: Transition::settled(if expanding { 0.0 } else { 1.0 }, now),
            });
        motion.progress.set_easing(self.styles.ui.spec.easing, now);
        motion.progress.retarget(
            if expanding { 1.0 } else { 0.0 },
            self.styles.ui.spec.tree_transition_duration(expanding),
            now,
        );
        self.sidebar_starred_motion =
            (self.animations_enabled() && !motion.paths.is_empty()).then_some(motion);
        cx.notify();
    }

    pub(super) fn prepare_starred_motion(&mut self, paths: &[PathBuf]) {
        let now = Instant::now();
        if self.sidebar_starred_motion.as_ref().is_some_and(|motion| {
            motion.paths != paths || !motion.progress.running(now) || !self.animations_enabled()
        }) {
            self.sidebar_starred_motion = None;
        }
        if let Some(motion) = &mut self.sidebar_starred_motion {
            motion.frame_progress = motion.progress.value(now);
        }
    }

    pub(super) fn starred_reveal(&self) -> Option<TreeFrame> {
        self.sidebar_starred_motion
            .as_ref()
            .map(|motion| TreeFrame::new(0..motion.paths.len(), motion.frame_progress))
    }

    pub(super) fn render_animated_sidebar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let view = self
            .sidebar_motion_view
            .get_or_insert_with(|| {
                let editor = cx.entity();
                cx.new(|cx: &mut Context<SidebarMotionView>| SidebarMotionView {
                    editor: editor.downgrade(),
                    refresh: true,
                    _subscription: cx.observe(&editor, |view, editor, cx| {
                        view.refresh = true;
                        if editor.read(cx).should_render_sidebar_panel() {
                            cx.notify();
                        }
                    }),
                })
            })
            .clone();
        gpui::AnyView::from(view)
            .cached(
                gpui::StyleRefinement::default()
                    .w(px(self.sidebar_layout_width()))
                    .h_full()
                    .flex_none(),
            )
            .into_any_element()
    }

    pub(super) fn begin_sidebar_tree_motion(
        &mut self,
        path: PathBuf,
        before: Rc<Vec<SidebarItem>>,
    ) {
        let previous = self.sidebar_tree_motion.take();
        if !self.animations_enabled() || self.sidebar_edit.is_some() {
            return;
        }
        let canonical = self.sidebar_items();
        let expanding = self.expanded_dirs.contains(&path);
        let now = Instant::now();
        let spec = &self.styles.ui.spec;
        let mut motion = if let Some(previous) = previous.filter(|previous| {
            previous.path == path
                && previous.root == self.sidebar_root
                && Rc::ptr_eq(&previous.canonical, &before)
        }) {
            previous
        } else {
            let expanded = if expanding { canonical.clone() } else { before };
            TreeMotion {
                children: descendants(&expanded, &path),
                path,
                root: self.sidebar_root.clone(),
                expanded,
                canonical: canonical.clone(),
                expanding,
                progress: Transition::settled(if expanding { 0.0 } else { 1.0 }, now),
                guides: None,
            }
        };
        motion.canonical = canonical;
        motion.expanding = expanding;
        motion.progress.set_easing(spec.easing, now);
        motion.progress.retarget(
            if expanding { 1.0 } else { 0.0 },
            spec.tree_transition_duration(expanding),
            now,
        );
        self.sidebar_tree_motion = Some(motion);
    }

    pub(super) fn sidebar_tree_frame(
        &mut self,
        canonical: Rc<Vec<SidebarItem>>,
    ) -> (Rc<Vec<SidebarItem>>, Option<TreeFrame>) {
        let Some(mut motion) = self.sidebar_tree_motion.take() else {
            return (canonical, None);
        };
        if !self.animations_enabled()
            || self.sidebar_edit.is_some()
            || motion.root != self.sidebar_root
        {
            return (canonical, None);
        }
        let now = Instant::now();
        if !Rc::ptr_eq(&canonical, &motion.canonical) {
            if !motion.expanding
                || !motion.children.is_empty()
                || !self.expanded_dirs.contains(&motion.path)
            {
                return (canonical, None);
            }
            motion.children = descendants(&canonical, &motion.path);
            motion.expanded = canonical.clone();
            motion.canonical = canonical.clone();
            motion.guides = None;
            motion.progress = Transition::settled(0.0, now);
            motion.progress.set_easing(self.styles.ui.spec.easing, now);
            motion
                .progress
                .retarget(1.0, self.styles.ui.spec.tree_transition_duration(true), now);
        }
        let waiting = motion.expanding
            && motion.children.is_empty()
            && self.sidebar_directory_loading.contains(&motion.path);
        if !waiting && !motion.progress.running(now) {
            return (canonical, None);
        }
        let frame = TreeFrame {
            children: motion.children.clone(),
            progress: motion.progress.value(now).clamp(0.0, 1.0),
        };
        let items = if motion.expanding {
            canonical
        } else {
            motion.expanded.clone()
        };
        self.sidebar_tree_motion = Some(motion);
        (items, Some(frame))
    }

    pub(super) fn sidebar_row_is_closing(&self, path: &std::path::Path) -> bool {
        self.sidebar_tree_motion.as_ref().is_some_and(|motion| {
            !motion.expanding && path != motion.path && path.starts_with(&motion.path)
        })
    }

    pub(super) fn sidebar_motion_guide_levels(
        &mut self,
        items: &Rc<Vec<SidebarItem>>,
    ) -> Rc<Vec<HashSet<usize>>> {
        if let Some((cached_items, guides)) = &self.sidebar_guide_cache
            && Rc::ptr_eq(cached_items, items)
        {
            return guides.clone();
        }
        if let Some(guides) = self
            .sidebar_tree_motion
            .as_ref()
            .and_then(|motion| motion.guides.as_ref())
        {
            return guides.clone();
        }
        let mut next_at_depth = HashMap::new();
        let mut guides = vec![HashSet::new(); items.len()];
        for i in (0..items.len()).rev() {
            let depth = items[i].depth;
            next_at_depth.insert(depth, i);
            for level in 1..=depth {
                if next_at_depth.get(&level).is_some_and(|next| *next > i) {
                    guides[i].insert(level);
                }
            }
        }
        let guides = Rc::new(guides);
        self.sidebar_guide_cache = Some((items.clone(), guides.clone()));
        if let Some(motion) = &mut self.sidebar_tree_motion {
            motion.guides = Some(guides.clone());
        }
        guides
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn large_tree_motion_keeps_two_bounded_render_ranges() {
        let frame = TreeFrame {
            children: 4..10_004,
            progress: 0.5,
        };
        assert_eq!(frame.offset(24.0), -120_000.0);
        let ranges = frame.ranges(4990..5030, 9990..10_030);
        assert_eq!(ranges, vec![4990..5030, 10_004..10_030]);
        assert_eq!(ranges.iter().map(Range::len).sum::<usize>(), 66);
    }

    #[test]
    fn completed_reveal_does_not_move_following_rows() {
        let frame = TreeFrame {
            children: 4..12,
            progress: 1.0,
        };
        assert_eq!(frame.offset(32.0), 0.0);
        assert_eq!(frame.ranges(0..30, 0..30), vec![0..12, 12..30]);
        assert_eq!(frame.ranges(20..30, 20..30), vec![20..30]);
    }
}
