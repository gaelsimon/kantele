//! Times deriving the in-memory index from scanned files.

use kantele::index::Library;

#[path = "shared/mod.rs"]
mod shared;

fn main() {
    let tracks = shared::requested_tracks(42_516);
    let library = shared::synthetic(tracks);
    let files = shared::synthetic_files(tracks);
    println!(
        "{} tracks, {} albums, {} artists",
        library.len(),
        library.albums().len(),
        library.artists().len()
    );

    shared::time("Library::build, the whole index", || {
        std::hint::black_box(Library::build("Music".to_owned(), &files));
    });
    shared::time("the interned browse axes alone", || {
        std::hint::black_box(kantele::browse::Axes::build(
            library.tracks(),
            &kantele::index::fold::Ignored::default(),
        ));
    });
    shared::time("the folded searchable text alone", || {
        std::hint::black_box(kantele::index::Searchables::build(
            library.tracks(),
            library.albums(),
            library.artists(),
            library.playlists(),
            &[],
        ));
    });
    shared::time("grouping files into releases", || {
        std::hint::black_box(kantele::index::identity::group_releases(
            &files
                .iter()
                .map(|file| kantele::index::identity::FileFacts {
                    relative_path: &file.relative,
                    tags: &file.tags,
                })
                .collect::<Vec<_>>(),
        ));
    });
}
