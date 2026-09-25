//! Which cover is published where a folder image and an embedded picture both exist.

use kantele::index::artwork::{Prefer, Source};
use kantele::index::{Library, ScanOptions, Store};
use kantele::service::{self, Indexing, Pass};

mod fixtures;
use fixtures::Tree;

/// One album folder holding a cover.jpg and a FLAC that carries a picture of its own.
/// The name is the caller's: two tests of one binary run at once, and the folder is named after one.
fn both(name: &str) -> Tree {
    let tree = Tree::new(name);
    let folder = tree.path("Sierra");
    std::fs::create_dir_all(&folder).expect("creating the album folder");
    std::fs::write(
        folder.join("01.flac"),
        fixtures::flac(&[("ALBUM", "Dundunbanza"), ("TITLE", "One")], true),
    )
    .expect("writing a flac carrying a picture");
    std::fs::write(folder.join("cover.jpg"), fixtures::jpeg()).expect("writing a folder cover");
    tree
}

fn served_cover(tree: &Tree, cover_art: Prefer) -> Source {
    let library = Library::scan_with(
        &tree.0,
        &ScanOptions {
            cover_art,
            ..ScanOptions::default()
        },
    )
    .expect("the folder is read");
    library
        .tracks()
        .first()
        .expect("the track is in the library")
        .artwork
        .as_ref()
        .expect("it has a cover")
        .source
        .clone()
}

#[test]
fn the_folders_image_wins_by_default() {
    let tree = both("cover-preference-folder");
    assert!(
        matches!(served_cover(&tree, Prefer::Folder), Source::File(path) if path.ends_with("cover.jpg")),
        "which is what this server has always done, so an upgrade changes no artwork"
    );
}

#[test]
fn the_picture_in_the_file_wins_where_the_owner_asked_for_it() {
    let tree = both("cover-preference-embedded");
    assert!(matches!(
        served_cover(&tree, Prefer::Embedded),
        Source::Embedded { .. }
    ));
}

#[test]
fn whichever_is_not_preferred_still_serves_where_the_other_is_missing() {
    let only_folder = Tree::new("cover-only-folder");
    only_folder.album("Sierra", &["01.wav"], true);
    assert!(
        matches!(served_cover(&only_folder, Prefer::Embedded), Source::File(path) if path.ends_with("cover.jpg")),
        "a preference is not a refusal of the other"
    );

    let only_embedded = Tree::new("cover-only-embedded");
    let folder = only_embedded.path("Sierra");
    std::fs::create_dir_all(&folder).expect("creating the album folder");
    std::fs::write(
        folder.join("01.flac"),
        fixtures::flac(&[("ALBUM", "Dundunbanza"), ("TITLE", "One")], true),
    )
    .expect("writing a flac carrying a picture");
    assert!(matches!(
        served_cover(&only_embedded, Prefer::Folder),
        Source::Embedded { .. }
    ));
}

#[test]
fn a_folder_of_several_albums_leaves_each_file_its_own_picture() {
    let tree = both("cover-preference-mixed");
    let mix = tree.path("Exotic Dance Tunes");
    std::fs::create_dir_all(&mix).expect("creating a selection folder");
    for (file, album) in [
        ("01.flac", "Blue Note Trip 10"),
        ("02.flac", "Republicafrobeat"),
    ] {
        let comments = [("ALBUM", album), ("TITLE", file)];
        std::fs::write(mix.join(file), fixtures::flac(&comments, true)).expect("writing a flac");
    }
    std::fs::write(mix.join("Folder.jpg"), fixtures::jpeg())
        .expect("writing the selection's image");

    let library = Library::scan(&tree.0).expect("the folders are read");
    let source = |folder: &str| -> Vec<Source> {
        library
            .tracks()
            .iter()
            .filter(|track| track.relative.starts_with(folder))
            .map(|track| track.artwork.as_ref().expect("a cover").source.clone())
            .collect()
    };
    assert!(
        source("Exotic Dance Tunes")
            .iter()
            .all(|one| matches!(one, Source::Embedded { .. })),
        "the image of a folder of many albums is the folder's, not each album's"
    );
    assert!(
        source("Sierra")
            .iter()
            .all(|one| matches!(one, Source::File(path) if path.ends_with("cover.jpg"))),
        "and an album folder keeps its image"
    );
}

#[test]
fn a_second_pass_keeps_each_file_its_own_picture_without_opening_it() {
    let tree = Tree::new("cover-preference-mixed-again");
    let mix = tree.path("Mix");
    std::fs::create_dir_all(&mix).expect("creating a selection folder");
    for (file, album) in [("01.flac", "One"), ("02.flac", "Two")] {
        let comments = [("ALBUM", album), ("TITLE", file)];
        std::fs::write(mix.join(file), fixtures::flac(&comments, true)).expect("writing a flac");
    }
    std::fs::write(mix.join("Folder.jpg"), fixtures::jpeg())
        .expect("writing the selection's image");
    let mut store = Some(Store::in_memory().expect("a store"));
    let indexing = Indexing::of(&tree.0);
    service::index(&indexing, &mut store, Pass::Whole).expect("the first pass");

    let again = service::index(&indexing, &mut store, Pass::Whole).expect("the second pass");
    assert_eq!(
        again.pass.opened, 0,
        "nothing changed, so nothing is opened"
    );
    assert!(again.library.tracks().iter().all(|track| matches!(
        track.artwork.as_ref().map(|artwork| &artwork.source),
        Some(Source::Embedded { .. })
    )));
}
