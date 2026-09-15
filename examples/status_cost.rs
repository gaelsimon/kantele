//! Times answering `/api/status`.

use kantele::browse;
use kantele::index::refusals::{Cause, Refusals};

mod shared;

fn main() {
    let tracks = shared::requested_tracks(42_549);
    let mut files = shared::synthetic_files(tracks);
    for file in files.iter_mut().step_by(16) {
        file.tags = kantele::tags::FileTags {
            track_number: file.tags.track_number,
            ..Default::default()
        };
    }
    let library = kantele::index::Library::build("Music".to_owned(), &files);
    let served = browse::Served::new(library, browse::Settings::default());
    let (library, view) = (&served.library, &served.view);
    println!(
        "{} tracks, {} albums, {} artists, {} untagged",
        library.len(),
        library.albums().len(),
        library.artists().len(),
        served.counts.untagged
    );

    shared::time(
        "the counts, settled once when a library is published",
        || {
            std::hint::black_box((
                kantele::index::Coverage::of(library),
                browse::untagged_count(library, view),
            ));
        },
    );
    shared::time("the counts, as the route reads them", || {
        std::hint::black_box((served.counts.untagged, served.counts.coverage.clone()));
    });
    shared::time("the counters beside it", || {
        std::hint::black_box((
            library.len(),
            library.albums().len(),
            library.artists().len(),
            library.playlists().len(),
        ));
    });
    let full = at_the_bound();
    println!(
        "  a record at its bound: {} causes, {} refusals",
        full.reported().len(),
        full.total()
    );
    shared::time("the record, as the route renders it", || {
        std::hint::black_box(full.reported().len());
    });
    shared::time("the record, as the route clones it first", || {
        let mut both = full.clone();
        both.absorb(full.clone());
        std::hint::black_box(both.reported().len());
    });
    shared::time("the whole answer as JSON, at the bound", || {
        std::hint::black_box(serde_json::to_string_pretty(&full.reported()).expect("json"));
    });
    for each in [100, 1_000, 10_000] {
        shared::time(&format!("recording {each} of each cause"), || {
            std::hint::black_box(filled(each).total());
        });
    }
}

/// More refusals of each cause than the record keeps.
fn at_the_bound() -> Refusals {
    filled(250)
}

fn filled(each: usize) -> Refusals {
    let mut refusals = Refusals::default();
    for cause in Cause::ALL {
        for at in 0..each {
            refusals.refuse(
                *cause,
                format!("_FLAC/Some Long Folder Name (2014) [FLAC]/{at:02} A Track Title.flac"),
                Some(format!(
                    "failed to parse Mpeg file: file contains an invalid frame {at}"
                )),
            );
        }
    }
    refusals
}
