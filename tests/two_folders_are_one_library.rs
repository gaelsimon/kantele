//! Several music folders, told apart by their names, served as one library from one store.

mod fixtures;

use std::path::{Path, PathBuf};

use fixtures::Tree;
use kantele::browse;
use kantele::index::store::Cache;
use kantele::index::{Library, Roots, ScanOptions, Store, watch};
use kantele::service::{self, Indexing, Pass};

fn label(tree: &Tree) -> String {
    tree.0
        .file_name()
        .expect("a fixture has a name")
        .to_string_lossy()
        .into_owned()
}

/// Named per test, since the tests run together and a fixture removes its namesake.
fn two(test: &str) -> (Tree, Tree, Roots) {
    let left = Tree::new(&format!("{test}-left"));
    left.album("Bach/Cantatas", &["01.wav", "02.wav"], false);
    let right = Tree::new(&format!("{test}-right"));
    right.album("Bach/Cantatas", &["01.wav"], false);
    let roots = Roots::new([left.0.clone(), right.0.clone()]).expect("two folders");
    (left, right, roots)
}

#[test]
fn two_folders_are_one_library_and_the_same_path_in_each_is_two_things() {
    let (left, right, roots) = two("shape");
    let library = Library::scan_using(&roots, &ScanOptions::default(), &Cache::default())
        .expect("scanning")
        .library;
    assert_eq!(library.len(), 3);
    let (left, right) = (label(&left), label(&right));
    for track in library.tracks() {
        let relative = Path::new(&track.relative);
        assert!(
            relative.starts_with(&left) || relative.starts_with(&right),
            "{} starts with neither folder's name",
            track.relative
        );
        assert_eq!(
            roots.absolute(relative).as_ref(),
            Some(&track.path),
            "the relative path says where the file is"
        );
    }

    let view = browse::View::build(&library);
    let (folders, loose) = browse::folder(&view, "");
    let mut names: Vec<&str> = folders.iter().map(|folder| folder.name.as_str()).collect();
    names.sort_unstable();
    let mut expected = vec![left.as_str(), right.as_str()];
    expected.sort_unstable();
    assert_eq!(
        names, expected,
        "the folder view opens onto one entry per folder"
    );
    assert!(loose.is_empty());
    let (_, in_left) = browse::folder(&view, &format!("{left}/Bach/Cantatas"));
    let (_, in_right) = browse::folder(&view, &format!("{right}/Bach/Cantatas"));
    assert_eq!(
        (in_left.len(), in_right.len()),
        (2, 1),
        "Bach/Cantatas under each folder is a folder of its own"
    );
}

#[test]
fn a_store_remembers_two_folders_and_serves_them_back_where_they_are() {
    let (_left, _right, roots) = two("store");
    let mut store = Some(Store::in_memory().expect("a store"));
    let indexing = Indexing::of(&roots);
    let first = service::index(&indexing, &mut store, Pass::Whole).expect("the first pass");
    assert_eq!(first.library.len(), 3);

    let remembered = service::remembered(&indexing, &mut store).expect("rows to serve");
    let paths = |library: &Library| -> Vec<PathBuf> {
        let mut paths: Vec<PathBuf> = library.tracks().iter().map(|t| t.path.clone()).collect();
        paths.sort();
        paths
    };
    assert_eq!(paths(&remembered), paths(&first.library));
    assert_eq!(
        fixtures::shape(&remembered),
        fixtures::shape(&first.library)
    );

    let second = service::index(&indexing, &mut store, Pass::Whole).expect("the second pass");
    assert_eq!(
        second.pass.opened, 0,
        "every file was answered from the store"
    );
    assert!(!second.changed);
}

#[test]
fn a_change_in_one_folder_rescans_that_folder_and_the_other_comes_from_the_store() {
    let (_left, right, roots) = two("change");
    let mut store = Some(Store::in_memory().expect("a store"));
    let indexing = Indexing::of(&roots);
    service::index(&indexing, &mut store, Pass::Whole).expect("the first pass");

    right.write("Bach/Cantatas/03.wav", 9);
    let change = watch::Change {
        paths: vec![right.path("Bach/Cantatas/03.wav")],
        whole_tree: false,
    };
    let pass = Pass::for_change(&roots, &change);
    let Pass::Within(scope) = &pass else {
        panic!("a file in one folder asks for that folder, got {pass:?}");
    };
    assert_eq!(
        scope.regions()[0].folder(),
        Path::new(&label(&right)).join("Bach/Cantatas"),
        "the region is named under the folder's label"
    );
    let indexed = service::index(&indexing, &mut store, pass).expect("the scoped pass");
    assert_eq!(indexed.library.len(), 4);
    assert_eq!(indexed.pass.opened, 1, "one file was read");
    assert!(indexed.changed);
}

#[test]
fn a_store_written_for_one_folder_walks_before_it_answers_for_two_and_forgets_the_old_rows() {
    let (left, _right, roots) = two("grown");
    let mut store = Some(Store::in_memory().expect("a store"));
    let alone = Indexing::of(&left.0);
    let first = service::index(&alone, &mut store, Pass::Whole).expect("one folder indexed");
    assert_eq!(first.library.len(), 2);

    // The owner adds a second folder and restarts.
    let both = Indexing::of(&roots);
    store
        .as_mut()
        .expect("a store")
        .serving(&roots)
        .expect("told which library it serves");
    assert!(
        service::remembered(&both, &mut store).is_none(),
        "rows written for one folder name nothing under two, so nothing is served from them"
    );
    let grown = service::index(&both, &mut store, Pass::Whole).expect("both folders indexed");
    assert_eq!(grown.library.len(), 3);
    assert!(
        grown.library.tracks().iter().all(|track| {
            let relative = Path::new(&track.relative);
            relative.starts_with(label(&left)) || !relative.starts_with("Bach")
        }),
        "no track comes from an unlabelled row"
    );

    // The next pass holds the rows for both folders, and forgets the ones for one.
    let again = service::index(&both, &mut store, Pass::Whole).expect("the settling pass");
    assert_eq!(again.library.len(), 3);
    let remembered = service::remembered(&both, &mut store).expect("rows to serve");
    assert_eq!(
        remembered.len(),
        3,
        "the old rows are gone or ignored, not served twice"
    );
    assert_eq!(
        service::index(&both, &mut store, Pass::Whole)
            .expect("a quiet pass")
            .pass
            .opened,
        0
    );
}

#[test]
fn folders_the_names_cannot_tell_apart_are_refused_before_anything_is_read() {
    let one = Tree::new("same");
    let other = std::env::temp_dir()
        .join("kantele-elsewhere")
        .join(label(&one));
    assert!(Roots::new([one.0.clone(), other]).is_err());
    assert!(Roots::new([one.0.clone(), one.0.join("inside")]).is_err());
}
