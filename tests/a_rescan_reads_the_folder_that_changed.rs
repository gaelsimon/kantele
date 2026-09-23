//! A new album must not cost a walk of the tree.

use kantele::index::scan::{Region, Scope};
use kantele::index::{Store, watch};
use kantele::service::{self, Indexing, Pass};

mod fixtures;
use fixtures::{Tree, shape};

fn remembering(tree: &Tree) -> Option<Store> {
    let mut store = Some(Store::in_memory().expect("a store"));
    service::index(&Indexing::of(&tree.0), &mut store, Pass::Whole).expect("the first pass");
    store
}

fn within(folders: &[&str]) -> Pass {
    Pass::Within(Scope::of(folders.iter().map(std::path::PathBuf::from)))
}

fn rows_held(store: &Option<Store>, tree: &Tree) -> usize {
    store
        .as_ref()
        .expect("a store")
        .cache(&tree.0)
        .expect("a cache")
        .len()
}

fn library(tree: &Tree, store: &mut Option<Store>, pass: Pass) -> kantele::index::Library {
    service::index(&Indexing::of(&tree.0), store, pass)
        .expect("a pass over the test tree")
        .library
}

#[test]
fn an_incremental_pass_produces_the_library_a_full_walk_would() {
    let tree = Tree::new("incremental-agrees");
    tree.album("Sierra Maestra", &["01.wav", "02.wav"], true);
    tree.album("Kremerata", &["01.wav"], false);
    let mut store = remembering(&tree);

    tree.album("New Arrival", &["01.wav", "02.wav"], true);
    let incremental = library(&tree, &mut store, within(&["New Arrival"]));

    let mut fresh = Some(Store::in_memory().expect("a store"));
    let whole = library(&tree, &mut fresh, Pass::Whole);

    assert_eq!(shape(&incremental), shape(&whole));
    assert_eq!(incremental.len(), 5);
}

#[test]
fn a_pass_over_one_folder_with_no_rows_to_join_it_to_walks_the_whole_tree() {
    let tree = Tree::new("incremental-without-rows");
    tree.album("Sierra Maestra", &["01.wav", "02.wav"], true);
    tree.album("Kremerata", &["01.wav"], false);

    for mut store in [None, Some(Store::in_memory().expect("a store"))] {
        let indexed = service::index(&Indexing::of(&tree.0), &mut store, within(&["Kremerata"]))
            .expect("a pass over the test tree");
        assert_eq!(
            indexed.library.len(),
            3,
            "the album nobody touched is walked rather than dropped"
        );
        assert!(indexed.pass.whole_tree, "and the pass says what it covered");
    }
}

#[test]
fn a_pass_says_whether_the_store_kept_what_it_learned() {
    let tree = Tree::new("incremental-stored");
    tree.album("Sierra Maestra", &["01.wav"], false);

    let without = service::index(&Indexing::of(&tree.0), &mut None, Pass::Whole).expect("a pass");
    assert!(
        !without.pass.stored,
        "no store holds what a pass without one learned"
    );

    let mut store = Some(Store::in_memory().expect("a store"));
    let with = service::index(&Indexing::of(&tree.0), &mut store, Pass::Whole).expect("a pass");
    assert!(with.pass.stored);
}

#[test]
fn a_look_finds_nothing_where_nothing_changed_and_the_folder_where_something_did() {
    let tree = Tree::new("sweep-finds");
    tree.album("Sierra Maestra", &["01.wav", "02.wav"], true);
    tree.album("Kremerata", &["01.wav"], false);
    let store = remembering(&tree);
    let indexing = Indexing::of(&tree.0);
    let held = store.as_ref().expect("a store");

    assert_eq!(
        service::sweep(&indexing, held).expect("a look"),
        None,
        "nothing changed, so there is nothing to pass over"
    );

    tree.album("New Arrival", &["01.wav"], true);
    let Some(Pass::Within(scope)) = service::sweep(&indexing, held).expect("a look") else {
        panic!("an album arrived");
    };
    assert!(scope.covers(std::path::Path::new("New Arrival")));
    assert!(
        !scope.covers(std::path::Path::new("Sierra Maestra")),
        "the album nobody touched is left to the store"
    );
}

#[test]
fn a_look_notices_a_file_that_went_away_and_one_rewritten_in_place() {
    let tree = Tree::new("sweep-notices");
    tree.album("Sierra Maestra", &["01.wav", "02.wav"], false);
    tree.album("Kremerata", &["01.wav"], false);
    let store = remembering(&tree);
    let indexing = Indexing::of(&tree.0);
    let held = store.as_ref().expect("a store");

    tree.remove("Kremerata/01.wav");
    let Some(Pass::Within(gone)) = service::sweep(&indexing, held).expect("a look") else {
        panic!("a file went away");
    };
    assert!(gone.covers(std::path::Path::new("Kremerata")));
    assert!(!gone.covers(std::path::Path::new("Sierra Maestra")));

    // Retagged in place: same size, another mtime.
    std::fs::File::options()
        .write(true)
        .open(tree.path("Sierra Maestra/02.wav"))
        .expect("opening it")
        .set_modified(std::time::SystemTime::now() - std::time::Duration::from_secs(3600))
        .expect("moving its clock");
    let Some(Pass::Within(both)) = service::sweep(&indexing, held).expect("a look") else {
        panic!("a file changed");
    };
    assert!(both.covers(std::path::Path::new("Sierra Maestra")));
    assert!(both.covers(std::path::Path::new("Kremerata")));
}

#[test]
fn the_pass_a_look_asks_for_produces_the_library_a_full_walk_would() {
    let tree = Tree::new("sweep-agrees");
    tree.album("Sierra Maestra", &["01.wav", "02.wav"], true);
    let mut store = remembering(&tree);

    tree.album("New Arrival", &["01.wav", "02.wav"], true);
    let pass = service::sweep(&Indexing::of(&tree.0), store.as_ref().expect("a store"))
        .expect("a look")
        .expect("something changed");
    let swept = library(&tree, &mut store, pass);

    let mut fresh = Some(Store::in_memory().expect("a store"));
    assert_eq!(
        shape(&swept),
        shape(&library(&tree, &mut fresh, Pass::Whole))
    );
    assert_eq!(swept.len(), 4);
}

#[test]
fn only_the_folder_that_changed_is_read() {
    let tree = Tree::new("incremental-reads-one-folder");
    tree.album("Sierra Maestra", &["01.wav", "02.wav", "03.wav"], true);
    tree.album("Kremerata", &["01.wav", "02.wav"], false);
    let store = remembering(&tree);

    tree.write("Kremerata/03.wav", 9);
    let scan = kantele::index::Library::scan_scoped(
        &tree.0,
        &kantele::index::ScanOptions::default(),
        &store
            .as_ref()
            .expect("a store")
            .cache(&tree.0)
            .expect("a cache"),
        &Scope::of([std::path::PathBuf::from("Kremerata/03.wav")]),
        &kantele::index::scan::Underway::default(),
        &kantele::index::fold::Ignored::default(),
    )
    .expect("an incremental pass");

    assert_eq!(
        fixtures::walked_paths(&tree.0, &scan.walked),
        vec![
            "Kremerata/01.wav".to_owned(),
            "Kremerata/02.wav".to_owned(),
            "Kremerata/03.wav".to_owned(),
        ],
        "the other album was answered from the store and never listed"
    );
    assert_eq!(scan.library.len(), 6, "the whole library is still served");
}

#[test]
fn a_file_that_went_away_is_forgotten_and_the_rest_of_the_library_is_not() {
    let tree = Tree::new("incremental-forgets-one");
    tree.album("Sierra Maestra", &["01.wav", "02.wav"], true);
    tree.album("Kremerata", &["01.wav", "02.wav"], false);
    let mut store = remembering(&tree);

    tree.remove("Kremerata/02.wav");
    let after = library(&tree, &mut store, within(&["Kremerata/02.wav"]));
    assert_eq!(after.len(), 3);

    let cache = store
        .as_ref()
        .expect("a store")
        .cache(&tree.0)
        .expect("a cache");
    assert_eq!(cache.len(), 3, "one row went, and only one");
}

#[test]
fn a_folder_that_went_away_takes_its_rows_with_it() {
    let tree = Tree::new("incremental-forgets-a-folder");
    tree.album("Sierra Maestra", &["01.wav", "02.wav"], true);
    tree.album("Kremerata", &["01.wav", "02.wav"], false);
    let mut store = remembering(&tree);

    tree.remove("Kremerata");
    let after = library(&tree, &mut store, within(&["Kremerata"]));
    assert_eq!(after.len(), 2);
    assert_eq!(
        store
            .as_ref()
            .expect("a store")
            .cache(&tree.0)
            .expect("a cache")
            .len(),
        2
    );
}

#[test]
fn a_change_that_cannot_be_placed_walks_the_whole_tree() {
    let root = std::path::Path::new("/music");
    let outside = watch::Change {
        paths: vec![std::path::PathBuf::from("/elsewhere/1.flac")],
        whole_tree: false,
    };
    assert_eq!(Pass::for_change(root, &outside), Pass::Whole);

    let lost_events = watch::Change {
        paths: vec![std::path::PathBuf::from("/music/a/1.flac")],
        whole_tree: true,
    };
    assert_eq!(Pass::for_change(root, &lost_events), Pass::Whole);

    assert_eq!(
        Pass::for_change(root, &watch::Change::default()),
        Pass::Whole
    );
}

#[test]
fn a_changed_file_asks_for_its_folder_and_a_changed_folder_for_its_subtree() {
    let root = std::path::Path::new("/music");
    let change = watch::Change {
        paths: vec![
            std::path::PathBuf::from("/music/Kremerata/01.flac"),
            std::path::PathBuf::from("/music/New Arrival"),
        ],
        whole_tree: false,
    };
    let Pass::Within(scope) = Pass::for_change(root, &change) else {
        panic!("a placeable change is an incremental pass");
    };
    let mut regions = scope.regions().to_vec();
    regions.sort();
    assert_eq!(
        regions,
        vec![
            Region::Folder(std::path::PathBuf::from("Kremerata")),
            Region::Subtree(std::path::PathBuf::from("New Arrival")),
        ]
    );
}

#[test]
fn a_pass_that_learned_nothing_says_so_rather_than_telling_every_client_to_reload() {
    let tree = Tree::new("incremental-unchanged");
    tree.album("Sierra Maestra", &["01.wav", "02.wav"], true);
    let mut store = remembering(&tree);

    let second = service::index(&Indexing::of(&tree.0), &mut store, Pass::Whole)
        .expect("a second pass over an unchanged tree");
    assert!(
        !second.changed,
        "the tree agrees with the rows, so nothing moved"
    );
    assert!(second.complete);

    tree.write("Sierra Maestra/03.wav", 3);
    let third = service::index(
        &Indexing::of(&tree.0),
        &mut store,
        within(&["Sierra Maestra"]),
    )
    .expect("a pass over the folder that changed");
    assert!(
        third.changed,
        "a new file is something the rows did not hold"
    );
}

#[test]
fn a_pass_asked_to_stop_leaves_at_once_and_the_cache_survives_it() {
    let tree = Tree::new("stop-mid-pass");
    tree.album("Sierra Maestra", &["01.wav", "02.wav"], true);
    tree.album("Kremerata", &["01.wav"], false);
    tree.album("Autechre", &["01.wav", "02.wav"], false);
    let mut store = remembering(&tree);
    assert_eq!(rows_held(&store, &tree), 5);

    let underway = std::sync::Arc::new(kantele::index::scan::Underway::default());
    underway.stopping.stop();
    let indexing = Indexing {
        underway,
        ..Indexing::of(&tree.0)
    };
    let stopped =
        service::index(&indexing, &mut store, Pass::Whole).expect("a stop is not a failure");

    assert!(
        !stopped.complete,
        "a pass that left the tree unfinished is not an answer about it"
    );
    assert!(
        stopped.library.is_empty(),
        "it read nothing after the stop: {}",
        stopped.library.len()
    );
    assert_eq!(
        rows_held(&store, &tree),
        5,
        "and it forgot nothing, which is the property that makes leaving safe"
    );
    assert_eq!(
        stopped.pass.opened, 0,
        "the report counts what the pass opened, not what the walk found before it left"
    );

    let after = library(&tree, &mut store, Pass::Whole);
    assert_eq!(after.len(), 5);

    let mut fresh = Some(Store::in_memory().expect("a store"));
    assert_eq!(
        shape(&after),
        shape(&library(&tree, &mut fresh, Pass::Whole)),
        "a stop left no mark on what the next pass produces"
    );
}

#[test]
fn a_pass_with_no_store_at_all_still_serves_the_library() {
    let tree = Tree::new("incremental-no-store");
    tree.album("Sierra Maestra", &["01.wav"], false);
    let mut none = None;
    let indexed = service::index(&Indexing::of(&tree.0), &mut none, Pass::Whole)
        .expect("a scan without persistence");
    assert_eq!(indexed.library.len(), 1);
    assert!(
        indexed.changed,
        "with nothing to compare against, a pass has learned everything"
    );
    assert!(
        indexed.library.tracks()[0].date_added.is_none(),
        "no store answered, so there is no date to claim"
    );
}

#[test]
fn a_file_with_no_extension_beside_a_new_album_does_not_cost_the_album_its_pass() {
    let tree = Tree::new("incremental-extensionless");
    tree.album("Sierra Maestra", &["01.wav", "02.wav"], true);
    tree.album("Kremerata", &["01.wav"], false);
    let mut store = remembering(&tree);

    tree.album("New Arrival", &["01.wav", "02.wav"], true);
    tree.text("Sierra Maestra/Credits", "liner notes");
    let pass = within(&["New Arrival", "Sierra Maestra/Credits"]);
    let outcome = service::index(&Indexing::of(&tree.0), &mut store, pass);

    assert_eq!(
        outcome
            .expect("a name with no dot is taken for a folder, and a file named so is none")
            .library
            .len(),
        5
    );
}
