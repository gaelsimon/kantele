//! Watching the library for changes.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify_debouncer_full::notify::event::{CreateKind, EventKind, ModifyKind, RemoveKind};
use notify_debouncer_full::notify::{RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, NoCache, new_debouncer_opt};
use tokio::sync::mpsc;

use crate::index::roots::Roots;
use crate::index::{playlist, scan};

pub const QUIET: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, Default)]
pub struct Change {
    /// Absolute, one per distinct path.
    pub paths: Vec<PathBuf>,
    pub whole_tree: bool,
}

#[derive(Debug, Default)]
struct Pending {
    paths: HashSet<PathBuf>,
    whole_tree: bool,
}

/// Dropping it stops the watch.
pub struct Watcher {
    _debouncer: Debouncer<RecommendedWatcher, NoCache>,
    changes: mpsc::Receiver<()>,
    pending: Arc<Mutex<Pending>>,
}

impl Watcher {
    /// Blocks while the watch lists the tree.
    pub fn start(
        roots: impl Into<Roots>,
        quiet: Duration,
        exclude: scan::Exclusions,
    ) -> notify_debouncer_full::notify::Result<Self> {
        let roots = roots.into();
        // One slot: the channel carries a signal, the paths sit in `pending`.
        let (sender, changes) = mpsc::channel(1);
        let pending: Arc<Mutex<Pending>> = Arc::default();
        let collected = pending.clone();
        let under = roots.clone();
        // `NoCache`: the file-id cache would stat the whole tree first.
        let mut debouncer = new_debouncer_opt::<_, RecommendedWatcher, NoCache>(
            quiet,
            None,
            move |result: DebounceEventResult| {
                if collect(&collected, result, &under, &exclude) {
                    // `try_send`: blocking here stalls the watcher thread.
                    let _ = sender.try_send(());
                }
            },
            NoCache::new(),
            notify_debouncer_full::notify::Config::default(),
        )?;
        for root in roots.paths() {
            debouncer.watch(root, RecursiveMode::Recursive)?;
        }
        tracing::info!(
            folder = %roots.describe(),
            quiet_seconds = quiet.as_secs(),
            "watching for changes"
        );
        Ok(Self {
            _debouncer: debouncer,
            changes,
            pending,
        })
    }

    pub async fn changed(&mut self) -> Option<Change> {
        loop {
            self.changes.recv().await?;
            if let Some(change) = taken(&self.pending) {
                return Some(change);
            }
        }
    }
}

/// What has gathered since the last look, or nothing where a wake outlived what it pointed at:
/// a change naming no path is read as the whole tree, which is a walk of every folder for nothing.
fn taken(pending: &Mutex<Pending>) -> Option<Change> {
    let held = std::mem::take(&mut *crate::held(pending));
    if held.paths.is_empty() && !held.whole_tree {
        return None;
    }
    Some(Change {
        paths: held.paths.into_iter().collect(),
        whole_tree: held.whole_tree,
    })
}

/// True when a waiter needs waking.
fn collect(
    pending: &Mutex<Pending>,
    result: DebounceEventResult,
    roots: &Roots,
    exclude: &scan::Exclusions,
) -> bool {
    let mut pending = crate::held(pending);
    let before = pending.paths.len();
    match result {
        Ok(events) => {
            for event in &events {
                // `Any` and `Other`: the backend lost track, so the whole tree is stale.
                if matches!(event.kind, EventKind::Any | EventKind::Other) {
                    pending.whole_tree = true;
                }
                for path in &event.paths {
                    if matters(path, &event.kind, roots, exclude) {
                        pending.paths.insert(path.clone());
                    }
                }
            }
        }
        Err(errors) => {
            for error in errors {
                tracing::warn!(%error, "watching the library");
            }
            pending.whole_tree = true;
        }
    }
    if pending.paths.len() == before && !pending.whole_tree {
        return false;
    }
    tracing::info!(
        changes = pending.paths.len(),
        whole_tree = pending.whole_tree,
        "the library changed"
    );
    true
}

fn matters(path: &Path, kind: &EventKind, roots: &Roots, exclude: &scan::Exclusions) -> bool {
    // A folder said to be modified is a folder whose contents or attributes moved, and the content
    // arrives as its own event on every backend. Windows says it for the parent of every change,
    // and a pass over the parent is a pass over every album beside the one that changed.
    if matches!(kind, EventKind::Modify(modified) if !matches!(modified, ModifyKind::Name(_)))
        && path.is_dir()
    {
        return false;
    }
    let skipped = path
        .components()
        .filter_map(|part| part.as_os_str().to_str())
        .any(scan::is_skipped);
    if skipped || exclude.excludes(&roots.relative_or_self(path)) {
        return false;
    }
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    scan::mime_for(path).is_some()
        || scan::is_image(name)
        || playlist::is_playlist(name)
        || path.extension().is_none()
        || names_a_folder(kind)
        || path.is_dir()
}

// `St. Vincent` has an extension: the kind says folder where the name cannot, and a rename never says.
fn names_a_folder(kind: &EventKind) -> bool {
    matches!(
        kind,
        EventKind::Create(CreateKind::Folder)
            | EventKind::Remove(RemoveKind::Folder)
            | EventKind::Modify(ModifyKind::Name(_))
    )
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;
    use crate::index::scan::Exclusions;
    use notify_debouncer_full::DebouncedEvent;
    use notify_debouncer_full::notify::Event;
    use notify_debouncer_full::notify::event::RenameMode;

    fn batch(events: Vec<(EventKind, &str)>) -> DebounceEventResult {
        Ok(events
            .into_iter()
            .map(|(kind, path)| {
                DebouncedEvent::new(
                    Event::new(kind).add_path(PathBuf::from(path)),
                    Instant::now(),
                )
            })
            .collect())
    }

    fn pending() -> Mutex<Pending> {
        Mutex::default()
    }

    fn collected(held: &Mutex<Pending>, result: DebounceEventResult) -> bool {
        collect(held, result, &Roots::one("/music"), &Exclusions::default())
    }

    fn counts(path: &str, kind: &EventKind) -> bool {
        matters(
            Path::new(path),
            kind,
            &Roots::one("/music"),
            &Exclusions::default(),
        )
    }

    #[test]
    fn a_wake_with_nothing_behind_it_is_not_a_change_over_the_whole_tree() {
        let held = pending();
        assert!(collected(
            &held,
            batch(vec![(
                EventKind::Create(CreateKind::File),
                "/music/a/1.flac"
            )])
        ));
        let first = taken(&held).expect("the file that was written");
        assert_eq!(first.paths, [PathBuf::from("/music/a/1.flac")]);
        assert!(
            taken(&held).is_none(),
            "a second wake over a drained set names no path, and a change naming none walks \
             every folder"
        );

        // The backend losing track is still a change, though it names nothing.
        assert!(collected(&held, Err(Vec::new())));
        assert!(taken(&held).is_some_and(|change| change.whole_tree));
    }

    #[test]
    fn an_excluded_path_is_not_a_change() {
        let exclude = Exclusions::new(["*.iso", "Podcasts"]);
        let root = Roots::one("/music");
        let file = EventKind::Create(CreateKind::File);
        assert!(!matters(
            Path::new("/music/Rips/disc.iso"),
            &file,
            &root,
            &exclude
        ));
        assert!(!matters(
            Path::new("/music/Podcasts/ep.mp3"),
            &file,
            &root,
            &exclude
        ));
        assert!(matters(
            Path::new("/music/a/1.flac"),
            &file,
            &root,
            &exclude
        ));
    }

    #[test]
    fn a_batch_of_events_keeps_the_paths_that_could_change_what_is_served() {
        let held = pending();
        assert!(collected(
            &held,
            batch(vec![
                (EventKind::Create(CreateKind::File), "/music/a/1.flac"),
                (EventKind::Modify(ModifyKind::Any), "/music/a/.DS_Store"),
                (EventKind::Create(CreateKind::Folder), "/music/b"),
            ])
        ));
        let taken = held.lock().expect("not poisoned");
        assert_eq!(taken.paths.len(), 2, "{:?}", taken.paths);
        assert!(taken.paths.contains(Path::new("/music/a/1.flac")));
        assert!(taken.paths.contains(Path::new("/music/b")));
        assert!(!taken.whole_tree);
    }

    #[test]
    fn a_batch_of_nothing_relevant_wakes_nobody() {
        let held = pending();
        assert!(!collected(
            &held,
            batch(vec![(
                EventKind::Modify(ModifyKind::Any),
                "/music/a/notes.txt"
            )])
        ));
        assert!(held.lock().expect("not poisoned").paths.is_empty());
    }

    #[test]
    fn a_backend_that_lost_events_asks_for_the_whole_tree() {
        let held = pending();
        assert!(collected(
            &held,
            batch(vec![(EventKind::Other, "/music/a/1.flac")])
        ));
        assert!(held.lock().expect("not poisoned").whole_tree);

        let failed = pending();
        assert!(collected(
            &failed,
            Err(vec![notify_debouncer_full::notify::Error::generic(
                "the watch died"
            )])
        ));
        let taken = failed.lock().expect("not poisoned");
        assert!(
            taken.whole_tree,
            "a watch that failed is no answer about one folder"
        );
        assert!(taken.paths.is_empty());
    }

    #[test]
    fn what_the_server_serves_is_a_change_and_what_it_never_walks_is_not() {
        let file = EventKind::Modify(ModifyKind::Any);
        assert!(counts("/music/a/01 Juana Peña.flac", &file));
        assert!(counts("/music/a/cover.jpg", &file));
        assert!(counts("/music/a new album", &file));
        assert!(counts("/music/a/playlist.m3u", &file));

        assert!(!counts("/music/a/.DS_Store", &file));
        assert!(!counts("/music/a/@eaDir/1.flac/THUMB.jpg", &file));
        assert!(!counts("/music/#recycle/deleted.flac", &file));
        assert!(!counts("/music/a/notes.txt", &file));
    }

    #[test]
    fn a_folder_that_is_there_and_merely_modified_is_not_a_change_of_its_own() {
        let dir = std::env::temp_dir().join(format!("kantele-watch-dir-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a folder on disk");
        let roots = Roots::one(std::env::temp_dir());
        let none = Exclusions::default();
        assert!(
            !matters(&dir, &EventKind::Modify(ModifyKind::Any), &roots, &none),
            "what changed inside it is an event of its own"
        );
        assert!(
            !matters(
                &dir,
                &EventKind::Modify(ModifyKind::Metadata(
                    notify_debouncer_full::notify::event::MetadataKind::Any
                )),
                &roots,
                &none
            ),
            "a chmod on a folder changes nothing served"
        );
        assert!(
            matters(
                &dir,
                &EventKind::Modify(ModifyKind::Name(RenameMode::To)),
                &roots,
                &none
            ),
            "a folder renamed is a folder that arrived"
        );
        assert!(matters(
            &dir,
            &EventKind::Create(CreateKind::Folder),
            &roots,
            &none
        ));
        let _ = std::fs::remove_dir(&dir);
    }

    #[test]
    fn a_folder_with_a_dot_in_its_name_is_a_change_whatever_the_name_looks_like() {
        let folder = EventKind::Create(CreateKind::Folder);
        assert!(counts("/music/St. Vincent", &folder));
        assert!(counts(
            "/music/Artist - Live 2024.06",
            &EventKind::Remove(RemoveKind::Folder)
        ));
        assert!(
            counts(
                "/music/R.E.M.",
                &EventKind::Modify(ModifyKind::Name(RenameMode::To))
            ),
            "a rename is how a whole album arrives, and its event never says folder"
        );
        assert!(
            !counts("/music/@eaDir/St. Vincent", &folder),
            "skipped stays skipped"
        );
        assert!(!counts(
            "/music/a/notes.txt",
            &EventKind::Create(CreateKind::File)
        ));
    }
}
