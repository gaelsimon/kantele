//! What a `Search` costs, at any library size.

use kantele::upnp::didl;
use kantele::upnp::{contentdirectory, search};

#[path = "shared/mod.rs"]
mod shared;

fn main() {
    let library = shared::synthetic(shared::requested_tracks(42_516));
    println!(
        "{} tracks, {} albums, {} artists, {} KB of folded searchable text",
        library.len(),
        library.albums().len(),
        library.artists().len(),
        library.searchable().footprint() / 1024,
    );

    let served = kantele::browse::Served::new(library, Default::default());

    let queries = [
        (
            "an amplifier asking for albums",
            r#"upnp:class derivedfrom "object.container.album" and @refID exists false"#,
        ),
        (
            "a title that matches nothing",
            r#"dc:title contains "zzzz""#,
        ),
        (
            "a title that matches one album",
            r#"dc:title contains "track 41""#,
        ),
        (
            "an artist, unqualified",
            r#"upnp:artist contains "artist 7""#,
        ),
        (
            "a composer, role qualified",
            r#"upnp:artist[@role="Composer"] contains "composer 3""#,
        ),
        ("everything", "*"),
        (
            "a class no object carries",
            r#"upnp:class derivedfrom "object.container.playlistContainer""#,
        ),
    ];

    for (label, criteria) in queries {
        let request = contentdirectory::SearchRequest {
            container_id: "0".to_owned(),
            criteria: criteria.to_owned(),
            starting_index: 0,
            requested_count: 20,
        };
        let whole = contentdirectory::SearchRequest {
            requested_count: 0,
            ..request.clone()
        };
        let total = contentdirectory::search(&served, &request, didl::To::plain("http://host"), 1)
            .expect("the criteria parse")
            .total_matches;
        shared::time(&format!("{label} ({total} hits)"), || {
            std::hint::black_box(
                contentdirectory::search(&served, &request, didl::To::plain("http://host"), 1)
                    .expect("the criteria parse"),
            );
        });
        shared::time("  the same, every hit rendered", || {
            std::hint::black_box(
                contentdirectory::search(&served, &whole, didl::To::plain("http://host"), 1)
                    .expect("the criteria parse"),
            );
        });
    }

    shared::time("parsing one criteria string", || {
        std::hint::black_box(search::parse(
            r#"upnp:class derivedfrom "object.container.album" and @refID exists false"#,
        ))
        .ok();
    });
}
