//! The watch on the music folder, started for real on a folder of this machine. It is the one
//! component that does not run on the NAS, so this is what guards it elsewhere.

use std::path::{Path, PathBuf};
use std::time::Duration;

use kantele::index::scan::Exclusions;
use kantele::index::watch::{Change, Watcher};
use kantele::service::Pass;
use tokio::time::timeout;

mod fixtures;
use fixtures::Tree;

/// Short, so a test waits a moment and not the five seconds a library gets.
const QUIET: Duration = Duration::from_millis(300);
/// FSEvents on a loaded runner can take seconds to say anything.
const PATIENCE: Duration = Duration::from_secs(15);

/// The root as the backend reports it: a temp folder on macOS is a link, and the events name the
/// real path.
fn root_of(tree: &Tree) -> PathBuf {
    tree.0.canonicalize().expect("the test tree is there")
}

fn watching(root: &Path, exclude: Exclusions) -> Watcher {
    Watcher::start(root, QUIET, exclude).expect("a watch on a folder that exists")
}

/// Whatever a backend says about the folder it was just pointed at, before the test acts.
async fn settled(watcher: &mut Watcher) {
    let _ = timeout(QUIET * 4, watcher.changed()).await;
}

async fn next(watcher: &mut Watcher) -> Change {
    timeout(PATIENCE, watcher.changed())
        .await
        .expect("a change within patience")
        .expect("the watch is alive")
}

async fn nothing(watcher: &mut Watcher) -> bool {
    timeout(QUIET * 6, watcher.changed()).await.is_err()
}

/// The first change and whatever follows it inside a settle window, as one. The debouncer
/// flushes each event when its own quiet period is up, so a folder made a few milliseconds
/// before its files can wake the loop one tick ahead of them; the loop runs a pass per wake,
/// and the second finds the folder already read.
async fn all_of(watcher: &mut Watcher) -> Change {
    let mut change = next(watcher).await;
    while let Ok(Some(more)) = timeout(QUIET * 6, watcher.changed()).await {
        change.paths.extend(more.paths);
        change.whole_tree |= more.whole_tree;
    }
    change
}

fn within(root: &Path, change: &Change) -> kantele::service::Scope {
    match Pass::for_change(root, change) {
        Pass::Within(scope) => scope,
        other => panic!("a change inside the tree is a scoped pass, not {other:?}: {change:?}"),
    }
}

#[tokio::test]
async fn a_file_written_under_the_root_is_a_change_naming_its_folder() {
    let tree = Tree::new("watch-file");
    tree.album("Blue Note/Kremerata", &["01.wav"], false);
    let root = root_of(&tree);
    let mut watcher = watching(&root, Exclusions::default());
    settled(&mut watcher).await;

    tree.write("Blue Note/Kremerata/02.wav", 2);

    let change = next(&mut watcher).await;
    assert!(!change.whole_tree, "{change:?}");
    assert!(
        change
            .paths
            .iter()
            .any(|path| path.ends_with("Blue Note/Kremerata/02.wav")),
        "the file itself is named: {change:?}"
    );
    let scope = within(&root, &change);
    assert!(
        scope.covers(Path::new("Blue Note/Kremerata")),
        "the pass that follows walks the album folder: {scope:?}"
    );
    assert!(
        !scope.covers(Path::new("Elsewhere")),
        "and nothing else: {scope:?}"
    );
}

#[tokio::test]
async fn an_album_copied_in_is_a_change_over_that_album_alone() {
    let tree = Tree::new("watch-burst");
    tree.album("Jazz/Existing", &["01.wav"], false);
    let root = root_of(&tree);
    let mut watcher = watching(&root, Exclusions::default());
    settled(&mut watcher).await;

    for track in 1..=6 {
        tree.write(&format!("Jazz/Arriving/0{track}.wav"), track);
    }

    let change = all_of(&mut watcher).await;
    assert!(
        change
            .paths
            .iter()
            .filter(|path| path.starts_with(root.join("Jazz/Arriving")))
            .count()
            >= 2,
        "the burst names the album's files, not one at a time: {change:?}"
    );
    assert!(!change.whole_tree, "{change:?}");
    let scope = within(&root, &change);
    assert!(scope.covers(Path::new("Jazz/Arriving")), "{scope:?}");
    assert!(
        !scope.covers(Path::new("Jazz/Existing")),
        "an album nobody touched is not walked again: {scope:?}"
    );
}

#[tokio::test]
async fn what_the_platform_writes_for_itself_wakes_nobody() {
    let tree = Tree::new("watch-skipped");
    tree.album("Jazz/Album", &["01.wav"], false);
    let root = root_of(&tree);
    let mut watcher = watching(&root, Exclusions::default());
    settled(&mut watcher).await;

    tree.write("Jazz/Album/@eaDir/01.wav/SYNOPHOTO_THUMB_M.wav", 3);
    tree.text("Jazz/Album/.DS_Store", "finder");
    tree.text("Jazz/Album/notes.txt", "a text file plays nothing");

    assert!(
        nothing(&mut watcher).await,
        "thumbnails, Finder files and text are not changes to what is served"
    );

    tree.write("Jazz/Album/02.wav", 2);
    let change = next(&mut watcher).await;
    assert!(
        change
            .paths
            .iter()
            .all(|path| !path.components().any(|part| part.as_os_str() == "@eaDir")),
        "and what was skipped stays out of the change that follows: {change:?}"
    );
}

#[tokio::test]
async fn an_excluded_folder_is_not_a_change_and_the_watch_goes_on() {
    let tree = Tree::new("watch-excluded");
    tree.album("Jazz/Album", &["01.wav"], false);
    let root = root_of(&tree);
    let mut watcher = watching(&root, Exclusions::new(["Podcasts"]));
    settled(&mut watcher).await;

    tree.write("Podcasts/episode.wav", 4);
    assert!(nothing(&mut watcher).await, "an excluded folder is silent");

    tree.write("Jazz/Album/02.wav", 2);
    let change = next(&mut watcher).await;
    assert!(
        change
            .paths
            .iter()
            .any(|path| path.ends_with("Jazz/Album/02.wav")),
        "{change:?}"
    );
}

#[tokio::test]
async fn an_album_arriving_by_rename_names_the_folder_it_now_is() {
    let tree = Tree::new("watch-rename");
    tree.album("Jazz/Album", &["01.wav"], false);
    let staging = Tree::new("watch-rename-staging");
    // A dot inside the name, not at its end: Windows drops a trailing dot from any file name.
    staging.album("St. Vincent", &["01.wav", "02.wav"], true);
    let root = root_of(&tree);
    let mut watcher = watching(&root, Exclusions::default());
    settled(&mut watcher).await;

    std::fs::rename(staging.path("St. Vincent"), tree.path("St. Vincent"))
        .expect("moving the album in");

    let change = next(&mut watcher).await;
    assert!(
        change
            .paths
            .iter()
            .any(|path| path.ends_with("St. Vincent")),
        "the folder is named, dot in its name and all: {change:?}"
    );
    let scope = within(&root, &change);
    assert!(scope.covers(Path::new("St. Vincent")), "{scope:?}");
}

#[tokio::test]
async fn a_folder_that_is_not_there_is_refused_rather_than_watched() {
    let tree = Tree::new("watch-missing");
    let missing = tree.path("not-yet-mounted");
    assert!(
        Watcher::start(missing.as_path(), QUIET, Exclusions::default()).is_err(),
        "a watch on nothing would report nothing for ever"
    );
}
