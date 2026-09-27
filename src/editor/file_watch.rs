use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    sync::mpsc::{self, Receiver},
};

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

pub(super) fn refresh_clean_snapshot(
    snapshot: &mut super::EditorSnapshot,
    is_dirty: bool,
    new_text: String,
) -> bool {
    if is_dirty || snapshot.text == new_text {
        return false;
    }
    snapshot.cursor_offset = snapshot.cursor_offset.min(new_text.len());
    snapshot.cursor_anchor = snapshot.cursor_anchor.min(new_text.len());
    snapshot.text = new_text;
    true
}

#[derive(Default)]
pub(super) struct WatchRequest {
    pub root: PathBuf,
    pub documents: HashSet<PathBuf>,
}

pub(super) struct WatchBatch {
    pub events: Vec<Event>,
    pub documents_to_check: Vec<PathBuf>,
}

pub(super) struct FileWatch {
    watcher: RecommendedWatcher,
    events: Receiver<notify::Result<Event>>,
    watched: HashMap<PathBuf, RecursiveMode>,
    documents: HashSet<PathBuf>,
    fingerprints: HashMap<PathBuf, Option<u64>>,
    polls_since_scan: u8,
}

fn file_fingerprint(path: &Path) -> Option<u64> {
    use std::hash::{Hash, Hasher};
    let bytes = std::fs::read(path).ok()?;
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    Some(hasher.finish())
}

fn resolved_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| {
        path.parent()
            .and_then(|parent| std::fs::canonicalize(parent).ok())
            .zip(path.file_name())
            .map(|(parent, name)| parent.join(name))
            .unwrap_or_else(|| path.to_path_buf())
    })
}

fn watch_targets(request: &WatchRequest) -> HashMap<PathBuf, RecursiveMode> {
    let mut targets = HashMap::new();
    let root = (!request.root.as_os_str().is_empty() && request.root.is_dir())
        .then(|| resolved_path(&request.root));
    if let Some(root) = &root {
        targets.insert(root.clone(), RecursiveMode::Recursive);
    }
    for document in &request.documents {
        for path in [document.clone(), resolved_path(document)] {
            if let Some(parent) = path.parent().filter(|parent| parent.is_dir()) {
                let parent = resolved_path(parent);
                if root.as_ref().is_none_or(|root| !parent.starts_with(root)) {
                    targets.entry(parent).or_insert(RecursiveMode::NonRecursive);
                }
            }
        }
    }
    targets
}

impl FileWatch {
    pub fn new() -> notify::Result<Self> {
        let (sender, events) = mpsc::channel();
        let watcher = notify::recommended_watcher(move |event| {
            let _ = sender.send(event);
        })?;
        Ok(Self {
            watcher,
            events,
            watched: HashMap::new(),
            documents: HashSet::new(),
            fingerprints: HashMap::new(),
            polls_since_scan: 0,
        })
    }

    pub fn poll(&mut self, request: WatchRequest) -> WatchBatch {
        let targets = watch_targets(&request);
        let mut checks = request
            .documents
            .difference(&self.documents)
            .cloned()
            .collect::<HashSet<_>>();
        let obsolete = self
            .watched
            .iter()
            .filter(|(path, mode)| targets.get(*path) != Some(*mode))
            .map(|(path, _)| path.clone())
            .collect::<Vec<_>>();
        for path in obsolete {
            let _ = self.watcher.unwatch(&path);
            self.watched.remove(&path);
        }
        for (path, mode) in targets {
            if self.watched.contains_key(&path) {
                continue;
            }
            match self.watcher.watch(&path, mode) {
                Ok(()) => {
                    self.watched.insert(path, mode);
                    checks.extend(request.documents.iter().cloned());
                }
                Err(error) => eprintln!("File watcher registration failed: {error}"),
            }
        }

        let resolved_documents = request
            .documents
            .iter()
            .map(|path| (path.clone(), resolved_path(path)))
            .collect::<Vec<_>>();
        let root = (!request.root.as_os_str().is_empty()).then(|| resolved_path(&request.root));
        let mut events = Vec::new();
        for result in self.events.try_iter() {
            match result {
                Ok(mut event) => {
                    if event.need_rescan() || event.paths.is_empty() {
                        checks.extend(request.documents.iter().cloned());
                    }
                    for path in &mut event.paths {
                        let resolved = resolved_path(path);
                        for (original, document) in &resolved_documents {
                            if document == &resolved || document.starts_with(&resolved) {
                                checks.insert(original.clone());
                            }
                        }
                        if let Some(root) = &root
                            && let Ok(relative) = resolved.strip_prefix(root)
                        {
                            *path = request.root.join(relative);
                        }
                    }
                    events.push(event);
                }
                Err(error) => {
                    eprintln!("File watcher event failed: {error}");
                    checks.extend(request.documents.iter().cloned());
                }
            }
        }
        self.polls_since_scan += 1;
        if self.polls_since_scan >= 6 {
            self.polls_since_scan = 0;
            self.fingerprints
                .retain(|path, _| request.documents.contains(path));
            for path in &request.documents {
                let fingerprint = file_fingerprint(path);
                if self
                    .fingerprints
                    .insert(path.clone(), fingerprint)
                    .is_some_and(|previous| previous != fingerprint)
                {
                    checks.insert(path.clone());
                }
            }
        }
        self.documents = request.documents;
        WatchBatch {
            events,
            documents_to_check: checks.into_iter().collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn request(root: &Path, paths: &[PathBuf]) -> WatchRequest {
        WatchRequest {
            root: root.to_path_buf(),
            documents: paths.iter().cloned().collect(),
        }
    }

    #[test]
    fn standalone_tabs_watch_deduplicated_parents_without_a_workspace() {
        let directory = tempfile::tempdir().unwrap();
        let parent = std::fs::canonicalize(directory.path()).unwrap();
        let documents = [parent.join("one.md"), parent.join("two.md")];
        let targets = watch_targets(&request(Path::new(""), &documents));
        assert_eq!(targets.len(), 1);
        assert_eq!(targets.get(&parent), Some(&RecursiveMode::NonRecursive));
    }

    #[test]
    fn workspace_and_external_parents_survive_workspace_switch_and_close() {
        let directory = tempfile::tempdir().unwrap();
        let parent = std::fs::canonicalize(directory.path()).unwrap();
        let first = parent.join("first");
        let second = parent.join("second");
        std::fs::create_dir_all(first.join("nested")).unwrap();
        std::fs::create_dir(&second).unwrap();
        let documents = [first.join("nested/doc.md"), second.join("doc.md")];
        let mut watcher = FileWatch::new().unwrap();
        for root in [&first, &second, &PathBuf::new()] {
            watcher.poll(request(root, &documents));
            assert_eq!(watcher.watched, watch_targets(&request(root, &documents)));
            assert_eq!(watcher.watched.len(), 2);
        }
        watcher.poll(request(Path::new(""), &[]));
        assert!(watcher.watched.is_empty());
    }

    #[test]
    fn opening_another_document_in_an_existing_watch_requests_a_reload() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("one.md");
        let second = directory.path().join("two.md");
        let mut watcher = FileWatch::new().unwrap();
        watcher.poll(request(Path::new(""), std::slice::from_ref(&first)));
        let batch = watcher.poll(request(Path::new(""), &[first, second.clone()]));
        assert!(batch.documents_to_check.contains(&second));
    }

    #[test]
    fn polling_detects_remote_style_content_changes_without_events() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("note.md");
        std::fs::write(&path, "old").unwrap();
        let mut watcher = FileWatch::new().unwrap();
        for _ in 0..6 {
            watcher.poll(request(Path::new(""), std::slice::from_ref(&path)));
        }
        std::fs::write(&path, "new text").unwrap();
        let mut changed = false;
        for _ in 0..6 {
            changed |= watcher
                .poll(request(Path::new(""), std::slice::from_ref(&path)))
                .documents_to_check
                .contains(&path);
        }
        assert!(changed);
    }

    #[test]
    fn clean_tabs_reload_but_dirty_tabs_and_unchanged_snapshots_are_preserved() {
        let mut snapshot = super::super::EditorSnapshot {
            text: "local draft".into(),
            cursor_offset: 11,
            cursor_anchor: 7,
            scroll_offset: 80.0,
            ..Default::default()
        };
        let before = snapshot.clone();
        assert!(!refresh_clean_snapshot(&mut snapshot, true, "disk".into()));
        assert_eq!(snapshot, before);
        assert!(!refresh_clean_snapshot(
            &mut snapshot,
            false,
            "local draft".into()
        ));
        assert_eq!(snapshot, before);
        assert!(refresh_clean_snapshot(&mut snapshot, false, "disk".into()));
        assert_eq!(snapshot.text, "disk");
        assert_eq!(snapshot.cursor_offset, 4);
        assert_eq!(snapshot.cursor_anchor, 4);
        assert_eq!(snapshot.scroll_offset, 80.0);
    }

    #[test]
    fn registration_catches_changes_before_the_first_native_notification() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("note.md");
        std::fs::write(&path, "old").unwrap();
        let mut snapshot = super::super::EditorSnapshot {
            text: "old".into(),
            ..Default::default()
        };
        let mut watcher = FileWatch::new().unwrap();
        std::fs::write(&path, "new before registration").unwrap();
        let batch = watcher.poll(request(Path::new(""), std::slice::from_ref(&path)));
        for path in batch.documents_to_check {
            assert!(refresh_clean_snapshot(
                &mut snapshot,
                false,
                std::fs::read_to_string(path).unwrap()
            ));
        }
        assert_eq!(snapshot.text, "new before registration");
    }

    #[cfg(unix)]
    #[test]
    fn canonical_events_still_target_tabs_opened_through_a_directory_alias() {
        let directory = tempfile::tempdir().unwrap();
        let real = directory.path().join("real");
        let alias = directory.path().join("alias");
        std::fs::create_dir(&real).unwrap();
        std::os::unix::fs::symlink(&real, &alias).unwrap();
        let path = alias.join("note.md");
        std::fs::write(&path, "content").unwrap();
        let mut watcher = FileWatch::new().unwrap();
        watcher.poll(request(Path::new(""), std::slice::from_ref(&path)));
        let (sender, events) = mpsc::channel();
        watcher.events = events;
        sender
            .send(Ok(Event::new(notify::EventKind::Modify(
                notify::event::ModifyKind::Data(notify::event::DataChange::Content),
            ))
            .add_path(std::fs::canonicalize(&path).unwrap())))
            .unwrap();
        let batch = watcher.poll(request(Path::new(""), std::slice::from_ref(&path)));
        assert_eq!(batch.events.len(), 1);
        assert_eq!(batch.documents_to_check, vec![path]);
    }

    #[test]
    fn closing_one_tab_retains_its_shared_parent_until_the_last_tab_closes() {
        let directory = tempfile::tempdir().unwrap();
        let first = directory.path().join("one.md");
        let second = directory.path().join("two.md");
        let mut watcher = FileWatch::new().unwrap();
        watcher.poll(request(Path::new(""), &[first.clone(), second]));
        watcher.poll(request(Path::new(""), &[first]));
        assert_eq!(watcher.watched.len(), 1);
        watcher.poll(request(Path::new(""), &[]));
        assert!(watcher.watched.is_empty());
    }

    #[test]
    fn native_parent_watch_reports_writes_atomic_replacement_and_followup_writes() {
        let directory = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(directory.path())
            .unwrap()
            .join("note.md");
        std::fs::write(&path, "before").unwrap();
        let (sender, receiver) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |event| {
            let _ = sender.send(event);
        })
        .unwrap();
        watcher
            .watch(path.parent().unwrap(), RecursiveMode::NonRecursive)
            .unwrap();
        let wait_for_path = || {
            let deadline = Instant::now() + Duration::from_secs(10);
            loop {
                let event: notify::Result<Event> = receiver
                    .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                    .expect("file change notification");
                if event
                    .unwrap()
                    .paths
                    .iter()
                    .any(|changed| resolved_path(changed) == path)
                {
                    break;
                }
            }
        };
        std::fs::write(&path, "modified").unwrap();
        wait_for_path();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "modified");
        for _ in receiver.try_iter() {}
        let replacement = path.with_file_name("replacement.tmp");
        std::fs::write(&replacement, "replacement").unwrap();
        std::fs::rename(&replacement, &path).unwrap();
        wait_for_path();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "replacement");
        for _ in receiver.try_iter() {}
        std::fs::write(&path, "after replacement").unwrap();
        wait_for_path();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "after replacement");
    }
}
