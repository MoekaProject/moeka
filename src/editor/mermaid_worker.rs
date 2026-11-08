//! One CPU preview job per window, including across pane swaps and invalidation.
//! `EditorView::mermaid_preview_worker` is root-only: never put it in `PaneState`.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;

use gpui::{Context, RenderImage, Task};

use super::{EditorView, PaneState, helpers};
use crate::markdown::{BlockPreviewKind, LineDecoration};
use crate::theme::{Theme, ThemeColorField};

use preview_cache::{CacheUsage, select_svg};
use scheduling::{DirtyKeys, JobIdentity, Schedule};

// This scheduler deliberately has no GPUI dependencies, so its concurrency and
// invalidation invariants can be exercised without creating a window/executor.
mod scheduling {
    use std::collections::{HashMap, VecDeque};
    use std::path::{Path, PathBuf};

    pub(super) const QUEUED_LIMIT: usize = 8;

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub(super) struct JobIdentity {
        pub epoch: u64,
        pub pane_id: usize,
        pub tab_id: String,
        pub cache_key: String,
        pub workspace_root: PathBuf,
        pub document_path: Option<PathBuf>,
        pub dirty_generation: Option<u64>,
    }

    #[derive(Default)]
    pub(super) struct DirtyKeys {
        next_generation: u64,
        keys: HashMap<String, DirtyEntry>,
    }

    struct DirtyEntry {
        generation: u64,
        failed: bool,
    }

    impl DirtyKeys {
        pub fn mark(&mut self, key: &str) {
            self.next_generation = self.next_generation.wrapping_add(1);
            self.keys.insert(
                key.to_owned(),
                DirtyEntry {
                    generation: self.next_generation,
                    failed: false,
                },
            );
        }

        pub fn generation(&self, key: &str) -> Option<u64> {
            self.keys.get(key).map(|entry| entry.generation)
        }

        pub fn needs_refresh(&self, key: &str) -> bool {
            self.keys.get(key).is_some_and(|entry| !entry.failed)
        }

        pub fn matches(&self, id: &JobIdentity) -> bool {
            self.generation(&id.cache_key) == id.dirty_generation
                && !self
                    .keys
                    .get(&id.cache_key)
                    .is_some_and(|entry| entry.failed)
        }

        pub fn finish(
            &mut self,
            id: &JobIdentity,
            current_epoch: u64,
            accepted: bool,
            successful: bool,
        ) {
            if !accepted || id.epoch != current_epoch || !self.matches(id) {
                return;
            }
            if successful {
                self.keys.remove(&id.cache_key);
            } else if let Some(entry) = self.keys.get_mut(&id.cache_key) {
                // Keep failed intent, but do not automatically retry a broken
                // fallback raster forever. An explicit force mark enables a
                // new attempt; normal renderer errors with a raster are cached
                // terminal previews and count as successful publications.
                entry.failed = true;
            }
        }

        pub fn retain(&mut self, mut live_consumer: impl FnMut(&str) -> bool) {
            self.keys.retain(|key, _| live_consumer(key));
        }

        pub fn invalidate(&mut self) {
            self.keys.clear();
        }
    }

    /// A borrowed snapshot of the document/presentation a result would affect.
    #[derive(Clone, Copy)]
    pub(super) struct Relevance<'a> {
        pub epoch: u64,
        pub pane_id: usize,
        pub tab_id: Option<&'a str>,
        pub workspace_root: &'a Path,
        pub document_path: Option<&'a Path>,
        pub configuration_matches: bool,
        pub key_is_current: bool,
        pub visible: bool,
        pub frozen: bool,
    }

    impl JobIdentity {
        pub fn is_relevant(&self, current: &Relevance<'_>) -> bool {
            self.epoch == current.epoch
                && self.pane_id == current.pane_id
                && Some(self.tab_id.as_str()) == current.tab_id
                && self.workspace_root == current.workspace_root
                && self.document_path.as_deref() == current.document_path
                && current.configuration_matches
                && current.key_is_current
                && current.visible
                && !current.frozen
        }
    }

    pub(super) fn preview_is_frozen(
        key: &str,
        code_mode_key: Option<&str>,
        rerender_pending: bool,
        debounce_pending: bool,
    ) -> bool {
        code_mode_key == Some(key) && (rerender_pending || debounce_pending)
    }

    pub(super) struct Schedule<J> {
        pub epoch: u64,
        pub active: Option<(JobIdentity, J)>,
        pub queued: VecDeque<(JobIdentity, J)>,
    }

    impl<J> Default for Schedule<J> {
        fn default() -> Self {
            Self {
                epoch: 0,
                active: None,
                queued: VecDeque::new(),
            }
        }
    }

    impl<J> Schedule<J> {
        pub fn any_matching(&self, mut relevant: impl FnMut(&JobIdentity, &J) -> bool) -> bool {
            self.active
                .iter()
                .chain(self.queued.iter())
                .any(|(id, job)| relevant(id, job))
        }

        pub fn contains(&self, identity: &JobIdentity) -> bool {
            self.any_matching(|id, _| id == identity)
        }

        pub fn enqueue(&mut self, identity: JobIdentity, job: J) -> bool {
            if identity.epoch != self.epoch
                || self.contains(&identity)
                || self.queued.len() >= QUEUED_LIMIT
            {
                return false;
            }
            self.queued.push_back((identity, job));
            true
        }

        pub fn start_next(&mut self) -> Option<(JobIdentity, J)>
        where
            J: Clone,
        {
            if self.active.is_some() {
                return None;
            }
            let next = self.queued.pop_front()?;
            self.active = Some(next.clone());
            Some(next)
        }

        pub fn complete(&mut self, identity: &JobIdentity) -> bool {
            if !self.active.as_ref().is_some_and(|(id, _)| id == identity) {
                return false;
            }
            self.active = None;
            true
        }

        pub fn invalidate(&mut self) {
            self.epoch = self.epoch.wrapping_add(1);
            self.queued.clear();
            // The running CPU job still owns the only slot. Its completion,
            // not invalidation, releases it, even though its result is obsolete.
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        fn id(epoch: u64, pane_id: usize, tab: &str, key: &str) -> JobIdentity {
            JobIdentity {
                epoch,
                pane_id,
                tab_id: tab.into(),
                cache_key: key.into(),
                workspace_root: PathBuf::from("workspace"),
                document_path: Some(PathBuf::from("workspace/document.md")),
                dirty_generation: None,
            }
        }

        fn relevant_document(identity: &JobIdentity) -> Relevance<'_> {
            Relevance {
                epoch: identity.epoch,
                pane_id: identity.pane_id,
                tab_id: Some(&identity.tab_id),
                workspace_root: &identity.workspace_root,
                document_path: identity.document_path.as_deref(),
                configuration_matches: true,
                key_is_current: true,
                visible: true,
                frozen: false,
            }
        }

        #[test]
        fn pending_excludes_every_obsolete_or_unrelated_context() {
            let request = id(0, 1, "tab", "key");
            let current = relevant_document(&request);
            assert!(request.is_relevant(&current));
            for unrelated in [
                Relevance {
                    epoch: 1,
                    ..current
                },
                Relevance {
                    pane_id: 2,
                    ..current
                },
                Relevance {
                    tab_id: None,
                    ..current
                },
                Relevance {
                    tab_id: Some("other-tab"),
                    ..current
                },
                Relevance {
                    workspace_root: Path::new("another-workspace"),
                    ..current
                },
                Relevance {
                    document_path: Some(Path::new("replacement.md")),
                    ..current
                },
                Relevance {
                    configuration_matches: false,
                    ..current
                },
                Relevance {
                    key_is_current: false,
                    ..current
                },
                Relevance {
                    visible: false,
                    ..current
                },
                Relevance {
                    frozen: true,
                    ..current
                },
            ] {
                assert!(!request.is_relevant(&unrelated));
            }
        }

        #[test]
        fn visible_warm_refresh_is_pending_while_queued_and_running() {
            let request = id(0, 1, "tab", "key");
            let current = relevant_document(&request);
            let mut queue = Schedule::default();
            // The bool represents an existing image/SVG. Presence of an old
            // cache entry cannot hide an explicitly requested async refresh.
            assert!(queue.enqueue(request.clone(), true));
            assert!(queue.any_matching(|id, _warm| id.is_relevant(&current)));
            assert!(queue.start_next().is_some());
            assert!(queue.any_matching(|id, _warm| id.is_relevant(&current)));
            assert!(queue.complete(&request));
            assert!(!queue.any_matching(|id, _warm| id.is_relevant(&current)));
        }

        #[test]
        fn irrelevant_active_job_holds_slot_without_delaying_current_document() {
            let old = id(0, 1, "tab", "old-key");
            let fresh = id(1, 1, "tab", "new-key");
            let current = relevant_document(&fresh);
            let mut queue = Schedule::default();
            assert!(queue.enqueue(old.clone(), ()));
            assert!(queue.start_next().is_some());
            queue.invalidate();
            assert!(queue.active.is_some());
            assert!(!queue.any_matching(|id, _| id.is_relevant(&current)));
            assert!(queue.enqueue(id(1, 2, "tab", "new-key"), ()));
            assert!(!queue.any_matching(|id, _| id.is_relevant(&current)));
            assert!(queue.enqueue(fresh.clone(), ()));
            assert!(queue.any_matching(|id, _| id.is_relevant(&current)));
            assert!(queue.start_next().is_none());
            assert!(queue.complete(&old));
            assert!(queue.any_matching(|id, _| id.is_relevant(&current)));
        }

        #[test]
        fn typing_freeze_applies_only_to_the_edited_key_until_debounce() {
            assert!(!preview_is_frozen("key", None, true, true));
            assert!(!preview_is_frozen("key", Some("other-key"), true, true));
            assert!(preview_is_frozen("key", Some("key"), true, false));
            assert!(preview_is_frozen("key", Some("key"), false, true));
            assert!(!preview_is_frozen("key", Some("key"), false, false));
        }

        #[test]
        fn rejected_warm_refresh_is_not_async_pending() {
            let mut queue = Schedule::default();
            for pane in 0..QUEUED_LIMIT {
                assert!(queue.enqueue(id(0, pane, "tab", "key"), true));
            }
            let rejected = id(0, 999, "tab", "warm-key");
            let current = relevant_document(&rejected);
            assert!(!queue.enqueue(rejected.clone(), true));
            assert!(!queue.any_matching(|id, _| id.is_relevant(&current)));
        }

        fn marked_request(dirty: &mut DirtyKeys, epoch: u64, key: &str) -> JobIdentity {
            dirty.mark(key);
            JobIdentity {
                dirty_generation: dirty.generation(key),
                ..id(epoch, 1, "tab", key)
            }
        }

        #[test]
        fn warm_overflow_dedup_and_offscreen_pruning_keep_dirty_intent() {
            let mut dirty = DirtyKeys::default();
            let request = marked_request(&mut dirty, 0, "warm-key");
            let mut queue = Schedule::default();
            for pane in 0..QUEUED_LIMIT {
                assert!(queue.enqueue(id(0, pane, "tab", "other-key"), ()));
            }
            assert!(!queue.enqueue(request.clone(), ()));
            assert!(dirty.needs_refresh("warm-key"));
            let (active, ()) = queue.start_next().unwrap();
            assert!(queue.enqueue(request.clone(), ()));
            assert!(!queue.enqueue(request.clone(), ()));
            assert!(dirty.needs_refresh("warm-key"));
            // Leaving the viewport removes queued work, not refresh intent.
            queue.queued.retain(|(id, _)| id != &request);
            assert!(dirty.needs_refresh("warm-key"));
            assert!(queue.enqueue(request, ()));
            assert!(queue.complete(&active));
        }

        #[test]
        fn only_accepted_success_clears_the_matching_dirty_generation() {
            let mut dirty = DirtyKeys::default();
            let request = marked_request(&mut dirty, 0, "key");
            let current = relevant_document(&request);
            let frozen = Relevance {
                frozen: true,
                ..current
            };
            dirty.finish(&request, 0, request.is_relevant(&frozen), true);
            assert!(dirty.needs_refresh("key"));
            for stale in [
                Relevance {
                    tab_id: Some("other-tab"),
                    ..current
                },
                Relevance {
                    configuration_matches: false,
                    ..current
                },
                Relevance {
                    key_is_current: false,
                    ..current
                },
            ] {
                dirty.finish(&request, 0, request.is_relevant(&stale), true);
                assert!(dirty.needs_refresh("key"));
            }
            dirty.finish(&request, 0, request.is_relevant(&current), true);
            assert!(!dirty.needs_refresh("key"));
            assert_eq!(dirty.generation("key"), None);
        }

        #[test]
        fn remarking_an_active_key_queues_a_distinct_generation() {
            let mut dirty = DirtyKeys::default();
            let old = marked_request(&mut dirty, 0, "key");
            let mut queue = Schedule::default();
            assert!(queue.enqueue(old.clone(), ()));
            assert!(queue.start_next().is_some());
            let fresh = marked_request(&mut dirty, 0, "key");
            assert_ne!(old, fresh);
            assert!(!dirty.matches(&old));
            assert!(queue.enqueue(fresh.clone(), ()));
            assert!(queue.start_next().is_none());
            // Neither a successful nor failed old completion may consume or
            // suppress a newer explicit force request for this same key.
            dirty.finish(&old, 0, true, true);
            dirty.finish(&old, 0, true, false);
            assert!(dirty.needs_refresh("key"));
            assert!(queue.complete(&old));
            assert_eq!(queue.start_next(), Some((fresh.clone(), ())));
            assert!(queue.complete(&fresh));
            dirty.finish(&fresh, 0, true, true);
            assert!(!dirty.needs_refresh("key"));
        }

        #[test]
        fn newer_generation_replaces_queued_work_without_clearing_intent() {
            let mut dirty = DirtyKeys::default();
            let old = marked_request(&mut dirty, 0, "key");
            let mut queue = Schedule::default();
            assert!(queue.enqueue(old, ()));
            let fresh = marked_request(&mut dirty, 0, "key");
            queue.queued.retain(|(id, _)| dirty.matches(id));
            assert!(queue.queued.is_empty());
            assert!(dirty.needs_refresh("key"));
            assert!(queue.enqueue(fresh.clone(), ()));
            assert!(queue.contains(&fresh));
            assert!(dirty.needs_refresh("key"));
        }

        #[test]
        fn epoch_invalidation_clears_dirty_without_releasing_the_active_slot() {
            let mut dirty = DirtyKeys::default();
            let old = marked_request(&mut dirty, 0, "key");
            let mut queue = Schedule::default();
            assert!(queue.enqueue(old.clone(), ()));
            assert!(queue.start_next().is_some());
            queue.invalidate();
            dirty.invalidate();
            assert_eq!(dirty.generation("key"), None);
            assert!(queue.active.is_some());
            let fresh = marked_request(&mut dirty, queue.epoch, "key");
            dirty.finish(&old, queue.epoch, true, true);
            assert!(dirty.needs_refresh("key"));
            // Epoch rejection is independent of generation rejection.
            let stale_epoch = JobIdentity {
                epoch: old.epoch,
                ..fresh.clone()
            };
            dirty.finish(&stale_epoch, queue.epoch, true, true);
            assert!(dirty.needs_refresh("key"));
            assert!(queue.complete(&old));
            dirty.finish(&fresh, queue.epoch, true, true);
            assert!(!dirty.needs_refresh("key"));
        }

        #[test]
        fn failed_fallback_keeps_intent_without_automatic_retry() {
            let mut dirty = DirtyKeys::default();
            let failed = marked_request(&mut dirty, 0, "key");
            dirty.finish(&failed, 0, true, false);
            assert_eq!(dirty.generation("key"), failed.dirty_generation);
            assert!(!dirty.needs_refresh("key"));
            assert!(!dirty.matches(&failed));
            let retry = marked_request(&mut dirty, 0, "key");
            assert!(dirty.needs_refresh("key"));
            assert!(dirty.matches(&retry));
            dirty.finish(&failed, 0, true, false);
            assert!(dirty.needs_refresh("key"));
            dirty.finish(&retry, 0, true, true);
            assert_eq!(dirty.generation("key"), None);
        }

        #[test]
        fn dirty_cleanup_keeps_live_and_stashed_consumers_only() {
            let mut dirty = DirtyKeys::default();
            let live = marked_request(&mut dirty, 0, "live");
            let stashed = marked_request(&mut dirty, 0, "stashed-tab");
            marked_request(&mut dirty, 0, "closed");
            dirty.retain(|key| ["live", "stashed-tab"].contains(&key));
            assert!(dirty.matches(&live));
            assert!(dirty.matches(&stashed));
            assert_eq!(dirty.generation("closed"), None);
        }

        #[test]
        fn repeated_requests_are_stable_and_fifo() {
            let mut queue = Schedule::default();
            let first = id(0, 1, "tab", "a");
            let second = id(0, 1, "tab", "b");
            assert!(queue.enqueue(first.clone(), 10));
            assert!(queue.enqueue(second.clone(), 20));
            assert!(!queue.enqueue(first.clone(), 99));
            assert_eq!(queue.start_next(), Some((first.clone(), 10)));
            assert!(!queue.enqueue(first.clone(), 99));
            assert!(queue.start_next().is_none());
            assert!(queue.complete(&first));
            assert_eq!(queue.start_next(), Some((second, 20)));
        }

        #[test]
        fn identity_separates_panes_tabs_workspaces_documents_and_epochs() {
            let base = id(0, 1, "tab", "key");
            let mut identities = vec![
                base.clone(),
                id(0, 2, "tab", "key"),
                id(0, 1, "other-tab", "key"),
                id(1, 1, "tab", "key"),
                id(0, 1, "tab", "other-key"),
            ];
            let mut other_workspace = base.clone();
            other_workspace.workspace_root = PathBuf::from("other-workspace");
            identities.push(other_workspace);
            let mut other_document = base;
            other_document.document_path = Some(PathBuf::from("replacement.md"));
            identities.push(other_document);
            for (index, identity) in identities.iter().enumerate() {
                for other in &identities[index + 1..] {
                    assert_ne!(identity, other);
                }
            }
        }

        #[test]
        fn invalidation_does_not_release_the_running_slot() {
            let mut queue = Schedule::default();
            let old = id(0, 1, "tab", "key");
            assert!(queue.enqueue(old.clone(), ()));
            assert_eq!(queue.start_next(), Some((old.clone(), ())));
            assert!(queue.enqueue(id(0, 2, "tab", "discard"), ()));
            queue.invalidate();
            assert_eq!(queue.epoch, 1);
            assert!(queue.queued.is_empty());
            assert!(queue.active.is_some());
            assert!(!queue.enqueue(old.clone(), ()));
            let fresh = id(1, 1, "tab", "key");
            assert!(queue.enqueue(fresh.clone(), ()));
            assert!(queue.start_next().is_none());
            assert!(!queue.complete(&fresh));
            assert!(queue.active.is_some());
            assert!(queue.complete(&old));
            assert_eq!(queue.start_next(), Some((fresh.clone(), ())));
            // A late/duplicate old completion cannot release the new job.
            assert!(!queue.complete(&old));
            assert_eq!(queue.active.as_ref().map(|(id, _)| id), Some(&fresh));
        }

        #[test]
        fn cap_is_global_and_rejected_jobs_can_be_added_after_completion() {
            let mut queue = Schedule::default();
            for pane in 0..QUEUED_LIMIT {
                assert!(queue.enqueue(id(0, pane, "tab", "key"), ()));
            }
            let deferred = id(0, 999, "tab", "key");
            assert!(!queue.enqueue(deferred.clone(), ()));
            let (active, ()) = queue.start_next().unwrap();
            assert!(queue.enqueue(deferred, ()));
            assert_eq!(queue.queued.len(), QUEUED_LIMIT);
            assert!(queue.start_next().is_none());
            assert!(queue.complete(&active));
            assert!(queue.start_next().is_some());
        }

        #[test]
        fn pruning_stale_work_keeps_order_and_frees_capacity() {
            let mut queue = Schedule::default();
            for pane in 0..QUEUED_LIMIT {
                assert!(queue.enqueue(id(0, pane, "tab", "key"), pane));
            }
            queue.queued.retain(|(id, _)| id.pane_id % 2 == 0);
            assert!(queue.enqueue(id(0, 999, "tab", "key"), 999));
            let (first, _) = queue.start_next().unwrap();
            assert_eq!(first.pane_id, 0);
            assert!(queue.complete(&first));
            assert_eq!(queue.start_next().unwrap().0.pane_id, 2);
        }
    }
}

mod preview_cache {
    use std::sync::Arc;

    pub(super) fn select_svg(
        dirty_generation: Option<u64>,
        previous: Option<&Arc<str>>,
        build: impl FnOnce() -> String,
    ) -> Arc<str> {
        if dirty_generation.is_none()
            && let Some(previous) = previous
        {
            return previous.clone();
        }
        let svg = build();
        if let Some(previous) = previous
            && previous.as_ref() == svg.as_str()
        {
            return previous.clone();
        }
        Arc::from(svg)
    }

    #[derive(Default)]
    pub(super) struct CacheUsage {
        pub raster_entries: usize,
        pub raster_bytes: usize,
        pub svg_entries: usize,
        pub svg_bytes: usize,
    }

    impl CacheUsage {
        pub fn fits(&self, limits: &Self) -> bool {
            self.raster_entries <= limits.raster_entries
                && self.raster_bytes <= limits.raster_bytes
                && self.svg_entries <= limits.svg_entries
                && self.svg_bytes <= limits.svg_bytes
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::cell::Cell;

        #[test]
        fn ordinary_raster_miss_reuses_svg_without_calling_builder() {
            let previous: Arc<str> = Arc::from("<svg>cached layout</svg>");
            let svg = select_svg(None, Some(&previous), || {
                panic!("an ordinary raster miss must not lay out Mermaid")
            });
            assert!(Arc::ptr_eq(&svg, &previous));
        }

        #[test]
        fn dirty_refresh_builds_even_when_svg_is_cached() {
            let previous: Arc<str> = Arc::from("<svg>old</svg>");
            let builds = Cell::new(0);
            let svg = select_svg(Some(1), Some(&previous), || {
                builds.set(builds.get() + 1);
                "<svg>new</svg>".into()
            });
            assert_eq!(builds.get(), 1);
            assert_eq!(svg.as_ref(), "<svg>new</svg>");
            assert!(!Arc::ptr_eq(&svg, &previous));
        }

        #[test]
        fn unchanged_dirty_svg_preserves_identity_for_image_reuse() {
            let previous: Arc<str> = Arc::from("<svg>same</svg>");
            let builds = Cell::new(0);
            let svg = select_svg(Some(1), Some(&previous), || {
                builds.set(builds.get() + 1);
                previous.to_string()
            });
            assert_eq!(builds.get(), 1);
            assert!(Arc::ptr_eq(&svg, &previous));
        }

        #[test]
        fn missing_svg_builds_for_both_ordinary_and_dirty_requests() {
            for generation in [None, Some(1)] {
                let builds = Cell::new(0);
                let svg = select_svg(generation, None, || {
                    builds.set(builds.get() + 1);
                    "<svg/>".into()
                });
                assert_eq!(builds.get(), 1);
                assert_eq!(svg.as_ref(), "<svg/>");
            }
        }

        #[test]
        fn trim_guard_requires_every_visible_resource_to_fit() {
            let limits = CacheUsage {
                raster_entries: 2,
                raster_bytes: 100,
                svg_entries: 3,
                svg_bytes: 200,
            };
            let mut visible = CacheUsage { ..limits };
            assert!(visible.fits(&limits));
            visible.raster_entries += 1;
            assert!(!visible.fits(&limits));
            visible.raster_entries -= 1;
            visible.raster_bytes += 1;
            assert!(!visible.fits(&limits));
            visible.raster_bytes -= 1;
            visible.svg_entries += 1;
            assert!(!visible.fits(&limits));
            visible.svg_entries -= 1;
            visible.svg_bytes += 1;
            assert!(!visible.fits(&limits));
        }
    }
}

#[derive(Clone)]
struct MermaidJob {
    source: Arc<str>,
    theme: Theme,
    monospace: String,
    body_cjk: String,
    priority_range: (usize, usize),
    scroll_offset: f32,
}

impl MermaidJob {
    fn configuration_matches(&self, editor: &EditorView) -> bool {
        self.monospace == editor.styles.editor.editor_monospace_font_family
            && self.body_cjk == editor.styles.editor.editor_body_cjk_font_family
            && ThemeColorField::UI_FIELDS
                .into_iter()
                .chain(ThemeColorField::EDITOR_FIELDS)
                .all(|field| self.theme.color(field) == editor.theme.color(field))
    }

    fn is_visible_in(
        &self,
        identity: &JobIdentity,
        decorations: &[LineDecoration],
        (visible_start, visible_end): (usize, usize),
    ) -> bool {
        let matches = |decoration: &LineDecoration| {
            decoration.block_preview.as_ref().is_some_and(|preview| {
                preview.kind == BlockPreviewKind::Mermaid
                    && preview.cache_key == identity.cache_key
                    && preview.source_text.as_str() == self.source.as_ref()
            })
        };
        let start = visible_start.min(decorations.len());
        let end = visible_end
            .saturating_add(1)
            .min(decorations.len())
            .max(start);
        decorations[start..end].iter().any(matches)
    }
}

struct MermaidResult {
    svg: Arc<str>,
    image: Option<Arc<RenderImage>>,
}

// No document-controlled text or fonts in the panic fallback. Cache even a
// failed fallback raster as None, so an unrecoverable renderer cannot hot-loop.
const ERROR_SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="520" height="96" viewBox="0 0 520 96"><rect width="520" height="96" rx="8" fill="#fff1f0"/><text x="24" y="42" font-family="sans-serif" font-size="18" fill="#9b2525">Mermaid preview failed</text><text x="24" y="70" font-family="sans-serif" font-size="14" fill="#663333">Unable to render this diagram.</text></svg>"##;

fn render_job(
    job: &MermaidJob,
    dirty_generation: Option<u64>,
    previous_svg: Option<Arc<str>>,
    previous_image: Option<Arc<RenderImage>>,
) -> MermaidResult {
    let rendered = catch_unwind(AssertUnwindSafe(|| {
        let svg = select_svg(dirty_generation, previous_svg.as_ref(), || {
            helpers::build_generated_block_preview_svg(
                BlockPreviewKind::Mermaid,
                &job.source,
                &job.theme,
                helpers::GeneratedPreviewFontFamilies {
                    monospace: &job.monospace,
                    body_cjk: &job.body_cjk,
                },
            )
        });
        if let (Some(previous_svg), Some(previous_image)) = (previous_svg.as_ref(), previous_image)
            && Arc::ptr_eq(previous_svg, &svg)
        {
            return Some(MermaidResult {
                svg,
                image: Some(previous_image),
            });
        }
        let image = helpers::load_generated_preview_image_from_svg_for_editor(
            &svg,
            BlockPreviewKind::Mermaid,
        )
        .ok()?;
        Some(MermaidResult {
            svg,
            image: Some(Arc::new(image)),
        })
    }));
    if let Ok(Some(result)) = rendered {
        return result;
    }
    let image = catch_unwind(AssertUnwindSafe(|| {
        helpers::load_generated_preview_image_from_svg_for_editor(
            ERROR_SVG,
            BlockPreviewKind::Mermaid,
        )
        .ok()
        .map(Arc::new)
    }))
    .ok()
    .flatten();
    MermaidResult {
        svg: Arc::from(ERROR_SVG),
        image,
    }
}

#[derive(Default)]
pub(crate) struct MermaidPreviewWorker {
    schedule: Schedule<MermaidJob>,
    dirty: DirtyKeys,
    task: Option<Task<()>>,
}

impl EditorView {
    /// Call once per explicit forced request, before queueing it. Ordinary
    /// missing/dirty retries must not mark again and supersede their own job.
    /// Does not evict the last-good SVG/image used by typing freeze.
    pub(super) fn mark_mermaid_preview_dirty(&mut self, key: &str) {
        self.mermaid_preview_worker.dirty.mark(key);
    }

    /// Root-shared refresh intent, independent of cache presence, visibility,
    /// or admission to the bounded queue. Reading it never schedules/notifies.
    /// A terminal fallback-raster failure suppresses automatic retry until a
    /// new force mark, without consuming that failed generation's dirty intent.
    pub(super) fn mermaid_preview_needs_refresh(&self, key: &str) -> bool {
        self.mermaid_preview_worker.dirty.needs_refresh(key)
    }

    /// Returns whether this visible request is queued or already in flight.
    /// Does not start work or mutate cached SVGs/images. Forced requests marked
    /// dirty remain retryable after overflow or offscreen queue pruning.
    pub(super) fn queue_mermaid_preview(&mut self, cache_key: &str, source: &str) -> bool {
        // During refresh, generated_preview_keys is not replaced until the end
        // of the pass. Use live decorations, not that old key set, for pruning.
        self.prune_mermaid_preview_queue(false);
        let Some(tab_id) = self.active_tab_id.clone() else {
            return false;
        };
        let identity = JobIdentity {
            epoch: self.mermaid_preview_worker.schedule.epoch,
            pane_id: self.current_pane_id(),
            tab_id,
            cache_key: cache_key.to_owned(),
            workspace_root: self.sidebar_root.clone(),
            document_path: self.current_file_path.clone(),
            dirty_generation: self.mermaid_preview_worker.dirty.generation(cache_key),
        };
        if self.mermaid_preview_worker.schedule.contains(&identity) {
            return self
                .mermaid_preview_worker
                .schedule
                .any_matching(|id, job| {
                    id == &identity && self.mermaid_job_is_relevant(id, job, false, true)
                });
        }
        if self.mermaid_preview_worker.schedule.queued.len() >= scheduling::QUEUED_LIMIT {
            return false;
        }
        let job = MermaidJob {
            source: Arc::from(source),
            theme: self.theme.clone(),
            monospace: self.styles.editor.editor_monospace_font_family.to_owned(),
            body_cjk: self.styles.editor.editor_body_cjk_font_family.to_owned(),
            priority_range: self.generated_preview_priority_line_range(),
            scroll_offset: self.scroll_offset,
        };
        if self.mermaid_job_is_relevant(&identity, &job, false, true) {
            return self.mermaid_preview_worker.schedule.enqueue(identity, job);
        }
        false
    }

    /// Start one background build/raster, or leave the physical active slot
    /// untouched. The root task owns the background task until it has finished.
    pub(super) fn start_mermaid_preview_worker(&mut self, cx: &mut Context<Self>) {
        // Refresh has committed generated_preview_keys by this point. Never
        // clean dirty intent during queueing, while that set is still old.
        self.prune_mermaid_preview_dirty_keys();
        self.prune_mermaid_preview_queue(true);
        let Some((identity, job)) = self.mermaid_preview_worker.schedule.start_next() else {
            return;
        };
        let previous_svg = self
            .generated_preview_svg_cache
            .get(&identity.cache_key)
            .cloned();
        let previous_image = self.image_cache.get(&identity.cache_key).cloned().flatten();
        let build_job = job.clone();
        let dirty_generation = identity.dirty_generation;
        let build = cx.background_executor().spawn(async move {
            render_job(&build_job, dirty_generation, previous_svg, previous_image)
        });
        self.mermaid_preview_worker.task = Some(cx.spawn(async move |this, cx| {
            let result = build.await;
            let _ = this.update(cx, |this, cx| {
                if !this.mermaid_preview_worker.schedule.complete(&identity) {
                    return;
                }
                this.mermaid_preview_worker.task = None;
                if this.mermaid_job_is_relevant(&identity, &job, true, false) {
                    // Validation above targets the originating pane, but the
                    // caches are root-shared. Every live consumer of this key
                    // must remeasure after a replacement, not just its owner.
                    let successful = result.image.is_some();
                    let applied = this.with_pane_if_alive(identity.pane_id, |this| {
                        let same_svg = this
                            .generated_preview_svg_cache
                            .get(&identity.cache_key)
                            .is_some_and(|svg| Arc::ptr_eq(svg, &result.svg));
                        let same_image =
                            match (this.image_cache.get(&identity.cache_key), &result.image) {
                                (Some(Some(previous)), Some(image)) => Arc::ptr_eq(previous, image),
                                (Some(None), None) => true,
                                _ => false,
                            };
                        if !same_image {
                            if let Some(Some(previous)) = this
                                .image_cache
                                .insert(identity.cache_key.clone(), result.image)
                            {
                                this.retired_images.push(previous);
                            }
                        }
                        this.generated_preview_svg_cache
                            .insert(identity.cache_key.clone(), result.svg);
                        if !same_svg || !same_image {
                            if let Some((_, previous)) = this
                                .generated_preview_overlay_cache
                                .remove(&identity.cache_key)
                            {
                                this.retired_images.push(previous);
                            }
                            this.trim_mermaid_preview_caches_after_publish();
                        }
                        !same_svg || !same_image
                    });
                    if let Some(changed) = applied {
                        let worker = &mut this.mermaid_preview_worker;
                        worker
                            .dirty
                            .finish(&identity, worker.schedule.epoch, true, successful);
                        if changed {
                            this.invalidate_mermaid_preview_layouts(&identity.cache_key);
                        }
                    }
                }
                this.start_mermaid_preview_worker(cx);
                cx.notify();
            });
        }));
    }

    /// Current-pane, active-tab, visible work for this key, including a forced
    /// refresh that still displays an old cached image. Never a cache-miss test.
    pub(super) fn mermaid_preview_is_pending(&self, key: &str) -> bool {
        self.mermaid_preview_worker
            .schedule
            .any_matching(|id, job| {
                id.cache_key == key && self.current_mermaid_preview_is_relevant(id, job)
            })
    }

    /// Async work that can still change the CURRENT pane's visible layout.
    /// Includes queued warm-cache refreshes; excludes obsolete physical work,
    /// other panes/tabs, offscreen keys, old configuration and typing freezes.
    /// Completions notify the view, so scroll confirmation can wait without
    /// polling or spending a layout frame. This is not global worker occupancy.
    pub(crate) fn mermaid_previews_in_flight(&self) -> bool {
        self.mermaid_preview_worker
            .schedule
            .any_matching(|id, job| self.current_mermaid_preview_is_relevant(id, job))
    }

    fn current_mermaid_preview_is_relevant(&self, id: &JobIdentity, job: &MermaidJob) -> bool {
        id.pane_id == self.current_pane_id() && self.mermaid_job_is_relevant(id, job, true, true)
    }

    fn mermaid_preview_cache_usage<'a>(&self, keys: impl Iterator<Item = &'a str>) -> CacheUsage {
        let mut usage = CacheUsage::default();
        for key in keys {
            if self.image_cache.contains_key(key)
                || self.generated_preview_overlay_cache.contains_key(key)
                || self.generated_preview_views.contains_key(key)
            {
                usage.raster_entries += 1;
            }
            usage.raster_bytes += self.generated_preview_raster_cache_bytes(key);
            if let Some(svg) = self.generated_preview_svg_cache.get(key) {
                usage.svg_entries += 1;
                usage.svg_bytes += svg.len();
            }
        }
        usage
    }

    fn trim_mermaid_preview_caches_after_publish(&mut self) {
        // Called inside the originating pane, after installing its result. Keep
        // the existing trim policy, but only invoke it when ALL visible keys fit
        // its budgets: keeping merely the newest key can evict another visible
        // diagram and create an endless completion -> missing -> render loop.
        let limits = CacheUsage {
            raster_entries: super::GENERATED_PREVIEW_CACHE_RETAIN,
            raster_bytes: super::GENERATED_PREVIEW_CACHE_BYTE_BUDGET,
            svg_entries: super::GENERATED_PREVIEW_SVG_CACHE_RETAIN,
            svg_bytes: super::GENERATED_PREVIEW_SVG_CACHE_BYTE_BUDGET,
        };
        if self
            .mermaid_preview_cache_usage(self.generated_preview_keys.iter().map(String::as_str))
            .fits(&limits)
        {
            return;
        }
        let (start, end) = self.generated_preview_priority_line_range();
        let mut visible_keys = std::collections::HashSet::new();
        let slice_start = start.min(self.decorations.len());
        let slice_end = end
            .saturating_add(1)
            .min(self.decorations.len())
            .max(slice_start);
        for (relative_line, decoration) in
            self.decorations[slice_start..slice_end].iter().enumerate()
        {
            if let Some(preview) = &decoration.block_preview {
                visible_keys.insert(preview.cache_key.clone());
            }
            let text = self.buffer.line_content(slice_start + relative_line);
            for range in &decoration.inline_math_spans {
                if let Some(source) = text.get(range.clone()) {
                    visible_keys.insert(helpers::inline_math_cache_key(source));
                }
            }
        }
        let mut visible = self.mermaid_preview_cache_usage(visible_keys.iter().map(String::as_str));
        // SVG ranking takes its entry limit BEFORE filtering missing SVGs, so
        // conservatively count all visible keys against that limit as well.
        visible.svg_entries = visible_keys.len();
        if visible.fits(&limits) {
            self.trim_generated_preview_render_caches();
        }
    }

    fn invalidate_mermaid_preview_layouts(&mut self, key: &str) {
        // Block previews are always remeasured by prepaint, even with a valid
        // shaping cache. Keep the OLD measured heights until that pass: clearing
        // them loses the compensation baseline, shrinks the scroll range and
        // disables windowed Markdown prepaint on every async completion.
        // Do not capture a scroll offset here: dispatch may predate user input.
        if self.generated_preview_keys.contains(key) {
            self.layout_dirty = true;
        }
        for pane in &mut self.panes {
            pane.state.invalidate_mermaid_preview_layout(key);
        }
    }

    pub(super) fn invalidate_mermaid_preview_jobs(&mut self) {
        self.mermaid_preview_worker.schedule.invalidate();
        // The caller invalidates SVG resources globally, making missing-cache
        // detection responsible for the new epoch's initial requests.
        self.mermaid_preview_worker.dirty.invalidate();
    }

    fn prune_mermaid_preview_dirty_keys(&mut self) {
        let mut dirty = std::mem::take(&mut self.mermaid_preview_worker.dirty);
        dirty.retain(|key| {
            self.generated_preview_keys.contains(key)
                || self
                    .tab_generated_preview_cache
                    .values()
                    .any(|cache| cache.keys.contains(key))
                || self.panes.iter().any(|pane| {
                    pane.state.generated_preview_keys.contains(key)
                        || pane
                            .state
                            .tab_generated_preview_cache
                            .values()
                            .any(|cache| cache.keys.contains(key))
                })
        });
        self.mermaid_preview_worker.dirty = dirty;
    }

    fn prune_mermaid_preview_queue(&mut self, check_membership: bool) {
        let mut queued = std::mem::take(&mut self.mermaid_preview_worker.schedule.queued);
        queued.retain(|(id, job)| self.mermaid_job_is_relevant(id, job, check_membership, true));
        self.mermaid_preview_worker.schedule.queued = queued;
    }

    fn mermaid_job_is_relevant(
        &self,
        id: &JobIdentity,
        job: &MermaidJob,
        check_membership: bool,
        check_visibility: bool,
    ) -> bool {
        if id.epoch != self.mermaid_preview_worker.schedule.epoch
            || !self.mermaid_preview_worker.dirty.matches(id)
            || id.workspace_root != self.sidebar_root
            || !job.configuration_matches(self)
            || self.pane_index(id.pane_id).is_none()
        {
            return false;
        }
        if id.pane_id == self.current_pane_id() {
            return id.is_relevant(&scheduling::Relevance {
                epoch: self.mermaid_preview_worker.schedule.epoch,
                pane_id: self.current_pane_id(),
                tab_id: self.active_tab_id.as_deref(),
                workspace_root: &self.sidebar_root,
                document_path: self.current_file_path.as_deref(),
                configuration_matches: true,
                key_is_current: !check_membership
                    || self.generated_preview_keys.contains(&id.cache_key),
                visible: !self.source_mode
                    && (!check_visibility
                        || job.is_visible_in(
                            id,
                            &self.decorations,
                            self.generated_preview_priority_line_range(),
                        )),
                frozen: scheduling::preview_is_frozen(
                    &id.cache_key,
                    self.code_mode_preview_key.as_deref(),
                    self.generated_preview_rerender_pending,
                    self.generated_preview_rerender_task.is_some(),
                ),
            });
        }
        // Do not nest with_pane while prepainting another pane. During a swap,
        // the focused pane's real state is parked in the swapped pane's slot.
        let slot_id = if id.pane_id == self.focused_pane_id {
            self.swapped_pane_id.unwrap_or(id.pane_id)
        } else {
            id.pane_id
        };
        let Some(index) = self.pane_index(slot_id) else {
            return false;
        };
        let state = &self.panes[index].state;
        let visible_range = state
            .visible_line_geometry
            .first()
            .zip(state.visible_line_geometry.last())
            .map(|(first, last)| (first.0, last.0))
            .unwrap_or(job.priority_range);
        id.is_relevant(&scheduling::Relevance {
            epoch: self.mermaid_preview_worker.schedule.epoch,
            pane_id: id.pane_id,
            tab_id: state.active_tab_id.as_deref(),
            workspace_root: &self.sidebar_root,
            document_path: state.current_file_path.as_deref(),
            configuration_matches: true,
            key_is_current: !check_membership
                || state.generated_preview_keys.contains(&id.cache_key),
            // A parked pane cannot compute fresh geometry with root methods.
            // Conservatively discard if it scrolled since enqueue; its next
            // prepaint can enqueue again using the new viewport.
            visible: !state.source_mode
                && (!check_visibility
                    || (state.scroll_offset == job.scroll_offset
                        && job.is_visible_in(id, &state.decorations, visible_range))),
            frozen: scheduling::preview_is_frozen(
                &id.cache_key,
                state.code_mode_preview_key.as_deref(),
                state.generated_preview_rerender_pending,
                state.generated_preview_rerender_task.is_some(),
            ),
        })
    }
}

impl PaneState {
    fn invalidate_mermaid_preview_layout(&mut self, key: &str) {
        if self.generated_preview_keys.contains(key) {
            // The next pane prepaint remeasures previews. A full deferred
            // invalidation on swap would discard its anchoring baseline too.
            self.layout_dirty = true;
        }
    }
}

#[cfg(test)]
mod preview_layout_tests {
    use super::*;
    use crate::editor::tests::test_line_display_info;

    #[test]
    fn mermaid_completions_keep_measured_geometry_and_do_not_restore_old_scroll() {
        let mut state = PaneState::detached();
        // The gallery has 42 examples. Keep both tall preview anchors and the
        // measured zero-height hidden source rows across every completion.
        let count = 42;
        state.line_display_info = (0..count)
            .flat_map(|index| {
                [
                    Some(Box::new(test_line_display_info(
                        &[24.0],
                        200.0 + index as f32,
                    ))),
                    Some(Box::new(test_line_display_info(&[], 0.0))),
                ]
            })
            .collect();
        let measured: Vec<_> = state
            .line_display_info
            .iter()
            .map(|info| {
                let info = info.as_deref().unwrap();
                (info as *const _, info.total_height())
            })
            .collect();
        state
            .line_geometry_cache
            .borrow_mut()
            .sparse
            .reset(count * 2, 24.0);
        for (line, (_, height)) in measured.iter().enumerate() {
            state
                .line_geometry_cache
                .borrow_mut()
                .sparse
                .set_measured_height(line, *height as f64);
        }
        let before_y = state
            .line_geometry_cache
            .borrow()
            .sparse
            .y_offset(count * 2, 0.0);
        for index in 0..count {
            let key = format!("mermaid-{index}");
            state.generated_preview_keys.insert(key.clone());
            // Wheel/drag input after dispatch, before this result arrives.
            state.scroll_offset = 1_000.0 + index as f32 * 50.0;
            state.layout_dirty = false;
            state.invalidate_mermaid_preview_layout(&key);
            assert!(state.layout_dirty);
            assert_eq!(state.scroll_offset, 1_000.0 + index as f32 * 50.0);
            assert!(state.pending_preview_reveal.is_none());
            assert!(!state.pending_scroll_to_cursor);
            assert!(!state.take_layout_cache_invalidation());
            assert_eq!(state.line_display_info.len(), count * 2);
            for (info, (pointer, height)) in state.line_display_info.iter().zip(&measured) {
                let info = info.as_deref().unwrap();
                assert_eq!(info as *const _, *pointer);
                assert_eq!(info.total_height(), *height);
            }
            assert_eq!(
                state
                    .line_geometry_cache
                    .borrow()
                    .sparse
                    .y_offset(count * 2, 0.0),
                before_y
            );
        }
    }

    #[test]
    fn mermaid_completion_does_not_dirty_an_unrelated_pane() {
        let mut state = PaneState::detached();
        state.generated_preview_keys.insert("other-diagram".into());
        state.invalidate_mermaid_preview_layout("completed-diagram");
        assert!(!state.layout_dirty);
        assert!(!state.take_layout_cache_invalidation());
    }
}
