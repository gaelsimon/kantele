//! The store answers what the files answer.

use std::path::Path;

use kantele::index::scan::Scope;
use kantele::index::store::Cache;
use kantele::index::{Library, ScanOptions, Store};

mod fixtures;
use fixtures::{Tree, jpeg, shape};

fn scan(root: &Path, cache: &Cache) -> kantele::index::Scan {
    Library::scan_using(root, &ScanOptions::default(), cache).expect("scanning the test tree")
}

#[test]
fn a_library_read_from_the_store_is_the_library_read_from_the_files() {
    let tree = Tree::new("agrees");
    tree.album("Cover Album", &["01 One.wav", "02 Two.wav"], true);
    tree.album("Bare Album", &["01 Alone.wav"], false);

    let mut store = Store::in_memory().expect("a store");
    let cold = scan(&tree.0, &Cache::default());
    store
        .save(
            &tree.0,
            cold.reading(),
            &Cache::default(),
            &Scope::whole_tree(),
        )
        .expect("writing the store");

    let cache = store.cache(&tree.0).expect("a cache");
    let warm = scan(&tree.0, &cache);

    assert_eq!(cache.hits(), 3, "every file was answered by the store");
    assert_eq!(cache.unreadable(), 0);
    assert_eq!(
        shape(&warm.library),
        shape(&cold.library),
        "the cache answered something the files do not say"
    );
    assert!(
        !warm.library.is_empty(),
        "a test that compares two empty libraries proves nothing"
    );
}

#[test]
fn the_index_the_store_remembers_is_the_index_the_files_produce() {
    let tree = Tree::new("remembers");
    tree.album("Cover Album", &["01 One.wav", "02 Two.wav"], true);
    tree.album("Bare Album", &["01 Alone.wav"], false);
    tree.text(
        "Cover Album/set.m3u",
        "#EXTM3U\n#EXTINF:1,One\n01 One.wav\n",
    );

    let mut store = Store::in_memory().expect("a store");
    let walked = scan(&tree.0, &Cache::default());
    store
        .save(
            &tree.0,
            walked.reading(),
            &Cache::default(),
            &Scope::whole_tree(),
        )
        .expect("writing the store");

    let name = kantele::index::library::library_name(&tree.0);
    let cache = store.cache(&tree.0).expect("a cache");
    let remembered = Library::build_with(
        name.clone(),
        &cache.remembered(),
        &cache.remembered_playlists_outside(&Scope::default()),
    );

    assert_eq!(
        shape(&remembered),
        shape(&walked.library),
        "the store remembers a different library from the one on disk"
    );
    assert_eq!(remembered.name(), walked.library.name());
    assert_eq!(
        remembered.playlists().len(),
        1,
        "a comparison that holds no playlist says nothing about the table that keeps them"
    );

    let streamed = from_the_store(&store, &tree.0);
    assert_eq!(
        shape(&streamed),
        shape(&walked.library),
        "the streamed rows describe a different library from the cache's own"
    );
}

#[test]
fn the_refusals_the_store_derives_are_the_refusals_the_files_derive() {
    let tree = Tree::new("agrees-on-refusals");
    tree.album("Sierra Maestra", &["01 One.wav", "02 Two.wav"], false);
    tree.text(
        "Sierra Maestra/set.m3u",
        "#EXTM3U\n\
         #EXTINF:1,Present\n\
         01 One.wav\n\
         #EXTINF:1,Moved away\n\
         03 Missing.wav\n\
         #EXTINF:1,Present again\n\
         01 One.wav\n",
    );
    tree.text("Sierra Maestra/gone.m3u", "#EXTM3U\nnowhere.wav\n");

    let mut store = Store::in_memory().expect("a store");
    let cold = scan(&tree.0, &Cache::default());
    store
        .save(
            &tree.0,
            cold.reading(),
            &Cache::default(),
            &Scope::whole_tree(),
        )
        .expect("writing the store");

    use kantele::index::refusals::Cause;
    for (cause, expected) in [
        (Cause::MissingEntry, 2),
        (Cause::UnpublishedPlaylist, 1),
        (Cause::RepeatedEntry, 1),
    ] {
        assert_eq!(
            cold.library.refusals().total_of(cause),
            expected,
            "{} is not in the record, so comparing it proves nothing",
            cause.as_str()
        );
    }

    let remembered = from_the_store(&store, &tree.0);
    assert_eq!(
        shape(&remembered),
        shape(&cold.library),
        "a start from the store answers a different record from the one the files produce"
    );
}

#[test]
fn the_rows_of_another_library_are_not_served_as_this_one() {
    let one = Tree::new("holds-one");
    one.album("Alpha", &["01.wav", "02.wav"], false);
    let two = Tree::new("holds-two");
    two.album("Beta", &["01.wav"], false);
    // One path in common, which is not the same library.
    two.album("Alpha", &["01.wav"], false);

    let mut store = Store::in_memory().expect("a store");
    let cold = scan(&one.0, &Cache::default());
    store
        .save(
            &one.0,
            cold.reading(),
            &Cache::default(),
            &Scope::whole_tree(),
        )
        .expect("writing the store");
    assert_eq!(from_the_store(&store, &one.0).len(), 2);

    assert!(
        from_the_store(&store, &two.0).is_empty(),
        "one path those rows name is under this folder, and one is not the library"
    );

    // The same library under another mount point.
    let moved = two.path("moved");
    std::fs::create_dir_all(&moved).expect("creating the mount point");
    let renamed = moved.join("Alpha");
    std::fs::rename(one.path("Alpha"), &renamed).expect("moving the library");
    assert_eq!(
        from_the_store(&store, &moved).len(),
        2,
        "the paths the rows name are all still there, one level up"
    );

    store.serving(&moved).expect("adopting the new path");
    assert_eq!(
        store.cache(&moved).expect("a cache").hits(),
        0,
        "a fresh cache has answered nothing yet"
    );
    let warm = scan(&moved, &store.cache(&moved).expect("a cache"));
    assert_eq!(warm.library.len(), 2);
}

/// The index a start builds from the store; `Library::build` alone would drop the playlists.
fn from_the_store(store: &Store, root: &Path) -> Library {
    let name = kantele::index::library::library_name(root);
    if !store.describes_roots(root).expect("asking after the rows") {
        return Library::build_with(name, &[], &[]);
    }
    let cache = store.cache(root).expect("a cache");
    Library::build_with(name, &cache.remembered(), &cache.remembered_playlists())
}

#[test]
fn a_cover_added_later_reaches_an_album_whose_music_nobody_touched() {
    let tree = Tree::new("late-cover");
    tree.album("Bare Album", &["01 Alone.wav"], false);

    let mut store = Store::in_memory().expect("a store");
    let cold = scan(&tree.0, &Cache::default());
    store
        .save(
            &tree.0,
            cold.reading(),
            &Cache::default(),
            &Scope::whole_tree(),
        )
        .expect("writing the store");
    assert!(
        cold.library.tracks()[0].artwork.is_none(),
        "nothing to show yet"
    );

    std::fs::write(tree.0.join("Bare Album/cover.jpg"), jpeg()).expect("writing a cover");
    let cache = store.cache(&tree.0).expect("a cache");
    let warm = scan(&tree.0, &cache);

    assert_eq!(cache.hits(), 1, "the audio was still answered by the store");
    let shown = warm.library.tracks()[0]
        .artwork
        .as_ref()
        .expect("the cover the user just dropped in");
    assert_eq!(
        shown.source,
        kantele::index::artwork::Source::File(tree.0.join("Bare Album/cover.jpg"))
    );
}
