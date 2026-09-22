//! A device must not see an empty library for the length of a first read.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use kantele::index::scan::{Scope, Underway};
use kantele::index::{Library, ScanOptions};

mod fixtures;
use fixtures::Tree;

fn read_publishing(tree: &Tree, first: Duration) -> (Vec<usize>, usize) {
    let seen: Arc<Mutex<Vec<usize>>> = Arc::default();
    let underway = Underway::default();
    let recorded = seen.clone();
    underway.partial.arm(
        Arc::new(move |finished| {
            let files = kantele::index::scan::in_walk_order(finished);
            assert!(
                files.windows(2).all(|pair| pair[0].path <= pair[1].path),
                "a publication is built in walk order, whatever order the threads finished in"
            );
            recorded.lock().expect("the record").push(files.len());
            true
        }),
        first,
    );
    let scan = Library::scan_scoped(
        &tree.0,
        &ScanOptions::default(),
        &kantele::index::store::Cache::default(),
        &Scope::whole_tree(),
        &underway,
        &kantele::index::fold::Ignored::default(),
    )
    .expect("a first read");
    let seen = seen.lock().expect("the record").clone();
    (seen, scan.library.len())
}

#[test]
fn what_is_published_while_reading_is_whole_folders_and_grows_to_the_library() {
    let tree = Tree::new("first-read-publishes");
    tree.album(
        "Sierra Maestra",
        &["01.wav", "02.wav", "03.wav", "04.wav", "05.wav"],
        true,
    );
    tree.album("Kremerata", &["01.wav", "02.wav", "03.wav"], false);

    let (seen, total) = read_publishing(&tree, Duration::ZERO);
    assert!(!seen.is_empty(), "a read that is due at once publishes");
    assert!(
        seen.windows(2).all(|pair| pair[0] <= pair[1]),
        "each publication holds at least what the one before did: {seen:?}"
    );
    assert!(
        seen.iter().all(|count| [3, 5, 8].contains(count)),
        "only whole folders are published, five and three files: {seen:?}"
    );
    assert_eq!(
        seen.last(),
        Some(&total),
        "the last publication is the whole library"
    );
}

#[test]
fn a_read_shorter_than_the_first_wait_publishes_nothing_early() {
    let tree = Tree::new("first-read-waits");
    tree.album("Sierra Maestra", &["01.wav"], false);

    let (seen, _) = read_publishing(&tree, Duration::from_secs(600));
    assert!(seen.is_empty(), "nothing is due within the read: {seen:?}");
}

#[test]
fn an_unarmed_read_publishes_nothing() {
    let tree = Tree::new("first-read-unarmed");
    tree.album("Sierra Maestra", &["01.wav"], false);
    let underway = Underway::default();
    assert!(!underway.partial.is_armed());
    let scan = Library::scan_scoped(
        &tree.0,
        &ScanOptions::default(),
        &kantele::index::store::Cache::default(),
        &Scope::whole_tree(),
        &underway,
        &kantele::index::fold::Ignored::default(),
    )
    .expect("a read");
    assert_eq!(scan.library.len(), 1);
}

/// The identifier of the album whose tracks sit in `folder`.
fn album_in(library: &Library, folder: &str) -> String {
    let album = library
        .albums()
        .iter()
        .find(|album| {
            album
                .tracks
                .iter()
                .all(|at| library.tracks()[*at].relative.starts_with(folder))
        })
        .unwrap_or_else(|| panic!("an album of its own in {folder}"));
    album.id.as_str().to_owned()
}

fn read_whole(tree: &Tree, underway: &Underway) -> Library {
    Library::scan_scoped(
        &tree.0,
        &ScanOptions::default(),
        &kantele::index::store::Cache::default(),
        &Scope::whole_tree(),
        underway,
        &kantele::index::fold::Ignored::default(),
    )
    .expect("a read")
    .library
}

#[test]
fn an_album_published_during_a_first_read_keeps_the_identifier_it_was_published_under() {
    let tree = Tree::new("first-read-identity");
    let tagged = |folder: &str| {
        let at = tree.path(folder);
        std::fs::create_dir_all(&at).expect("a folder");
        std::fs::write(
            at.join("01.flac"),
            fixtures::flac(
                &[
                    ("ALBUM", "Dundunbanza"),
                    ("TITLE", "Juana Peña"),
                    ("ARTIST", "Sierra Maestra"),
                ],
                false,
            ),
        )
        .expect("a flac");
    };

    // One folder read so far, and what it published is the whole library at that moment.
    tagged("Second/Dundunbanza");
    let published = read_whole(&tree, &Underway::default());
    let as_published = album_in(&published, "Second");

    // The other folder is read later in the same first read, and derives the same album key.
    tagged("First/Dundunbanza");
    let unaware = read_whole(&tree, &Underway::default());
    assert_ne!(
        album_in(&unaware, "Second"),
        as_published,
        "walk order alone hands the key to the folder read first, and moves the other"
    );

    let underway = Underway::default();
    underway.partial.award(published.claims().clone());
    let ended = read_whole(&tree, &underway);
    assert_eq!(
        album_in(&ended, "Second"),
        as_published,
        "the album a device was handed keeps the identifier it was handed under"
    );
    assert_eq!(
        ended
            .albums()
            .iter()
            .filter(|album| album.rule == kantele::index::Rule::Path)
            .count(),
        1,
        "and the folder that lost the key is the one keyed on its path"
    );
}
