//! Two folders can derive one album identity. Which of them keeps it must not depend on walk
//! order, or a folder copied in renames an album nobody touched and every client loses its place.

use kantele::index::{Library, ScanOptions, Store};

mod fixtures;
use fixtures::Tree;

/// Same album tags in each folder, so they contend for one key.
fn same_album(tree: &Tree, folder: &str) {
    let path = tree.path(folder);
    std::fs::create_dir_all(&path).expect("creating a folder");
    std::fs::write(
        path.join("01.flac"),
        fixtures::flac(
            &[
                ("ALBUM", "Greatest Hits"),
                ("ALBUMARTIST", "Ana"),
                ("TITLE", "One"),
            ],
            false,
        ),
    )
    .expect("writing a flac");
}

fn album_ids(tree: &Tree, store: &Store) -> Vec<(String, String)> {
    let cache = store.cache(&tree.0).expect("a cache");
    let scan = Library::scan_using(&tree.0, &ScanOptions::default(), &cache).expect("a pass");
    let mut named: Vec<(String, String)> = scan
        .library
        .albums()
        .iter()
        .map(|album| {
            let relative = &scan.library.tracks()[album.tracks[0]].relative;
            let folder = std::path::Path::new(relative)
                .parent()
                .expect("a track sits in a folder")
                .to_string_lossy()
                .into_owned();
            (folder, album.id.to_string())
        })
        .collect();
    named.sort();
    named
}

#[test]
fn a_folder_that_sorts_earlier_does_not_take_the_identity_of_one_already_served() {
    let tree = Tree::new("claims-walk-order");
    let mut store = Store::open(&tree.path("state/kantele.sqlite")).expect("a store");
    same_album(&tree, "Melody");
    same_album(&tree, "Zebra");

    let first = album_ids(&tree, &store);
    assert_eq!(first.len(), 2, "one key each, the second keyed on its path");
    let melody = first[0].1.clone();
    let zebra = first[1].1.clone();
    assert_ne!(melody, zebra);

    // What the pass awarded, as a real pass would have written it.
    let cache = store.cache(&tree.0).expect("a cache");
    let library = Library::build_holding(
        "music".to_owned(),
        &Library::scan_using(&tree.0, &ScanOptions::default(), &cache)
            .expect("a pass")
            .files,
        &[],
        &Default::default(),
        cache.claims(),
    );
    store
        .remember_claims(library.claims())
        .expect("the awards are written");

    same_album(&tree, "Alpha");
    let after = album_ids(&tree, &store);

    assert_eq!(after.len(), 3, "the newcomer is an album of its own");
    assert_eq!(
        after
            .iter()
            .find(|(folder, _)| folder == "Melody")
            .map(|(_, id)| id.clone()),
        Some(melody),
        "Melody held the shared key and keeps it, though Alpha now sorts first"
    );
    assert_eq!(
        after
            .iter()
            .find(|(folder, _)| folder == "Zebra")
            .map(|(_, id)| id.clone()),
        Some(zebra),
        "and the one keyed on its path was never in question"
    );
}

#[test]
fn a_store_that_remembers_nothing_falls_back_to_walk_order() {
    let tree = Tree::new("claims-cold");
    let store = Store::open(&tree.path("state/kantele.sqlite")).expect("a store");
    same_album(&tree, "Melody");
    same_album(&tree, "Zebra");

    assert!(
        store.claims().expect("it answers").is_empty(),
        "nothing is remembered before a pass writes it, so a first start behaves as it always did"
    );
    assert_eq!(album_ids(&tree, &store).len(), 2);
}

/// The defect itself, so the test above cannot pass by accident.
#[test]
fn with_nothing_remembered_the_newcomer_takes_it_and_the_album_is_renamed() {
    let tree = Tree::new("claims-unremembered");
    let store = Store::open(&tree.path("state/kantele.sqlite")).expect("a store");
    same_album(&tree, "Melody");
    same_album(&tree, "Zebra");
    let melody = album_ids(&tree, &store)[0].1.clone();

    same_album(&tree, "Alpha");
    let after = album_ids(&tree, &store);
    assert_ne!(
        after
            .iter()
            .find(|(folder, _)| folder == "Melody")
            .map(|(_, id)| id.clone()),
        Some(melody),
        "walk order alone hands Alpha the shared key and moves Melody onto its path"
    );
}

#[test]
fn a_key_nothing_derives_any_more_is_released() {
    let tree = Tree::new("claims-released");
    let mut store = Store::open(&tree.path("state/kantele.sqlite")).expect("a store");
    same_album(&tree, "Melody");

    let cache = store.cache(&tree.0).expect("a cache");
    let library = Library::build_holding(
        "music".to_owned(),
        &Library::scan_using(&tree.0, &ScanOptions::default(), &cache)
            .expect("a pass")
            .files,
        &[],
        &Default::default(),
        cache.claims(),
    );
    store.remember_claims(library.claims()).expect("written");
    assert_eq!(store.claims().expect("read back").len(), 1);

    std::fs::remove_dir_all(tree.path("Melody")).expect("the folder goes");
    let cache = store.cache(&tree.0).expect("a cache");
    let empty = Library::build_holding(
        "music".to_owned(),
        &Library::scan_using(&tree.0, &ScanOptions::default(), &cache)
            .expect("a pass")
            .files,
        &[],
        &Default::default(),
        cache.claims(),
    );
    store.remember_claims(empty.claims()).expect("written");
    assert!(
        store.claims().expect("read back").is_empty(),
        "or the table grows for the life of the library"
    );
}
