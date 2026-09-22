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
            let files: usize = finished.iter().map(|read| read.0.len()).sum();
            recorded.lock().expect("the record").push(files);
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
