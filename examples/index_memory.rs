//! Where the index's memory goes, field by field, on a real library.

use std::path::PathBuf;

use kantele::index::{Library, Store, library};

fn main() {
    let root = PathBuf::from(
        std::env::args()
            .nth(1)
            .expect("usage: index_memory <music folder> (KANTELE_STATE points at its store)"),
    );
    let store = Store::open(&kantele::config::store_path(&kantele::config::state_dir()))
        .expect("opening the store");
    let cache = store.cache(&root).expect("reading the store");
    let strings = |values: &[String]| values.iter().map(String::len).sum::<usize>();
    let files = cache.remembered();
    assert!(
        !files.is_empty(),
        "the store remembers nothing about {root:?}"
    );
    let transient = files.len() * std::mem::size_of::<kantele::index::Scanned>()
        + files
            .iter()
            .map(|file| {
                file.path.as_os_str().len()
                    + file.relative.as_os_str().len()
                    + file.tags.title.as_ref().map_or(0, String::len)
                    + file.tags.album.as_ref().map_or(0, String::len)
                    + file.tags.date.as_ref().map_or(0, String::len)
                    + strings(&file.tags.artists)
                    + strings(&file.tags.album_artists)
                    + strings(&file.tags.composers)
                    + strings(&file.tags.genres)
            })
            .sum::<usize>();
    let library = Library::build(library::library_name(&root), &files);

    let view = kantele::browse::View::build(&library);
    drop(cache);
    drop(files);

    let tracks = library.tracks();
    let n = tracks.len();
    const PER_ALLOCATION: usize = 16;

    let mut report: Vec<(&str, usize, usize)> = Vec::new();
    let mut note = |what: &'static str, bytes: usize, allocations: usize| {
        report.push((what, bytes, allocations));
    };

    note(
        "the track records themselves",
        std::mem::size_of_val(tracks),
        0,
    );
    note(
        "absolute paths",
        tracks.iter().map(|t| t.path.as_os_str().len()).sum(),
        n,
    );
    note(
        "paths relative to the root",
        tracks.iter().map(|t| t.relative.len()).sum(),
        n,
    );
    note(
        "identifiers, and the album identifier beside each",
        tracks
            .iter()
            .map(|t| t.id.as_str().len() + t.album_id.as_ref().map_or(0, |id| id.as_str().len()))
            .sum(),
        n + tracks.iter().filter(|t| t.album_id.is_some()).count(),
    );
    note("titles", tracks.iter().map(|t| t.title.len()).sum(), n);
    let list_fields = [
        (
            "artists",
            tracks.iter().map(|t| credited(&t.artists)).sum::<usize>(),
            tracks.iter().map(|t| t.artists.len()).sum::<usize>(),
        ),
        (
            "album artists",
            tracks.iter().map(|t| credited(&t.album_artists)).sum(),
            tracks.iter().map(|t| t.album_artists.len()).sum(),
        ),
        (
            "composers",
            tracks.iter().map(|t| credited(&t.composers)).sum(),
            tracks.iter().map(|t| t.composers.len()).sum(),
        ),
        (
            "genres",
            tracks.iter().map(|t| strings(&t.genres)).sum(),
            tracks.iter().map(|t| t.genres.len()).sum(),
        ),
    ];
    for (what, bytes, values) in list_fields {
        note(what, bytes, values + n);
    }
    note(
        "album titles and dates, per track",
        tracks
            .iter()
            .map(|t| {
                t.album.as_ref().map_or(0, String::len) + t.date.as_ref().map_or(0, String::len)
            })
            .sum(),
        tracks.iter().filter(|t| t.album.is_some()).count()
            + tracks.iter().filter(|t| t.date.is_some()).count(),
    );
    note(
        "the cover path, held once per track",
        tracks
            .iter()
            .filter_map(|t| t.artwork.as_ref())
            .map(|art| match &art.source {
                kantele::index::artwork::Source::File(path) => path.as_os_str().len(),
                kantele::index::artwork::Source::Embedded { path, .. } => path.as_os_str().len(),
            })
            .sum(),
        tracks.iter().filter(|t| t.artwork.is_some()).count(),
    );
    let albums = library.albums();
    note(
        "the album records themselves",
        std::mem::size_of_val(albums),
        0,
    );
    note(
        "what an album holds: title, credits, dates, cover, track list",
        albums
            .iter()
            .map(|album| {
                album.title.len()
                    + credited(&album.artists)
                    + strings(&album.genres)
                    + album.date.as_ref().map_or(0, String::len)
                    + album.id.as_str().len()
                    + album.musicbrainz_id.as_ref().map_or(0, String::len)
                    + album.tracks.len() * std::mem::size_of::<usize>()
            })
            .sum(),
        albums.len() * 4,
    );
    let artists = library.artists();
    note(
        "the artist records, and what they hold",
        std::mem::size_of_val(artists)
            + artists
                .iter()
                .map(|artist| {
                    artist.name.len()
                        + artist.id.as_str().len()
                        + (artist.albums.len() + artist.tracks.len()) * std::mem::size_of::<usize>()
                })
                .sum::<usize>(),
        artists.len() * 3,
    );
    note(
        "the interned axes: values, digests, and a number per track per axis",
        view.axes().footprint(),
        0,
    );
    note(
        "the folder tree: a path and two lists per folder",
        view.folders().footprint(),
        view.folders().len() * 3,
    );
    note(
        "the folded searchable text a `Search` compares against",
        library.searchable().footprint(),
        0,
    );
    note(
        "the three identifier maps",
        (library.tracks().len() + library.albums().len() + library.artists().len())
            * (std::mem::size_of::<String>() + 24 + std::mem::size_of::<usize>()),
        0,
    );

    println!(
        "{n} tracks, {} albums, {} artists\n",
        library.albums().len(),
        library.artists().len()
    );
    println!("{:<52} {:>10} {:>10}", "", "megabytes", "per track");
    let mut total = 0;
    for (what, bytes, allocations) in &report {
        let cost = bytes + allocations * PER_ALLOCATION;
        total += cost;
        println!(
            "{what:<52} {:>10.1} {:>9} B",
            cost as f64 / 1_048_576.0,
            cost / n
        );
    }
    println!(
        "{:<52} {:>10.1} {:>9} B",
        "counted here",
        total as f64 / 1_048_576.0,
        total / n
    );
    println!(
        "\nallocations held per track: {:.1}",
        report.iter().map(|(_, _, a)| *a).sum::<usize>() as f64 / n as f64
    );
    println!(
        "what building it held and then freed: {:.1} MB, {} B per track",
        transient as f64 / 1_048_576.0,
        transient / n
    );
    println!(
        "so a process serving this index sits near {:.0} MB rather than {:.0}",
        (total + transient) as f64 / 1_048_576.0,
        total as f64 / 1_048_576.0
    );
}

fn credited(values: &[kantele::index::credits::Credit]) -> usize {
    values
        .iter()
        .map(|credit| {
            credit.name.len()
                + credit.sort.as_ref().map_or(0, String::len)
                + std::mem::size_of::<kantele::index::credits::Credit>()
        })
        .sum()
}
