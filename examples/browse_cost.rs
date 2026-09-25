//! Times the pieces of a root browse.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use kantele::browse;
use kantele::index::{Library, Scanned};
use kantele::tags::{AudioProperties, FileTags};
use kantele::upnp::contentdirectory::{self, BrowseFlag, BrowseRequest};
use kantele::upnp::didl;

fn main() {
    let tracks: usize = std::env::args()
        .nth(1)
        .and_then(|n| n.parse().ok())
        .unwrap_or(9365);
    let library = synthetic(tracks);
    println!(
        "{} tracks, {} albums, {} artists",
        library.len(),
        library.albums().len(),
        library.artists().len()
    );

    let time = |label: &str, mut work: Box<dyn FnMut()>| {
        work();
        let started = Instant::now();
        for _ in 0..5 {
            work();
        }
        let each = started.elapsed() / 5;
        println!("  {label:<38} {:>7.1} ms", each.as_secs_f64() * 1000.0);
        each
    };

    let view = browse::View::build(&library);

    let served = kantele::browse::Served::new(library.clone(), view.settings.clone());

    let root = browse::Position::default();
    time(
        "selection of everything",
        Box::new(|| {
            std::hint::black_box(browse::selection(&library, &view, &root));
        }),
    );
    let selected = browse::selection(&library, &view, &root).expect("the root names every track");
    time(
        "albums_in over the whole selection",
        Box::new(|| {
            std::hint::black_box(browse::albums_in(&library, &selected));
        }),
    );
    time(
        "menu: the five axes",
        Box::new(|| {
            std::hint::black_box(browse::menu(&library, &view, &root));
        }),
    );
    let listing = browse::Position::default().listing(browse::Facet::AllArtists);
    time(
        "listing the All Artists axis",
        Box::new(|| {
            std::hint::black_box(browse::menu(&library, &view, &listing));
        }),
    );

    let request = |object_id: String, count: usize| BrowseRequest {
        object_id,
        browse_flag: BrowseFlag::DirectChildren,
        starting_index: 0,
        requested_count: count,
    };
    let answer = |request: &BrowseRequest| {
        contentdirectory::browse(&served, request, didl::To::plain("http://host"), 1)
            .expect("a container the library holds")
    };
    for facet in [
        browse::Facet::AllArtists,
        browse::Facet::Artist,
        browse::Facet::Genre,
    ] {
        let listing = browse::Position::default().listing(facet);
        for (label, count) in [("the whole axis", 0), ("one page of a hundred", 100)] {
            let request = request(listing.id(), count);
            let response = answer(&request);
            let title = facet.title();
            println!(
                "  Browse of {title}, {label}: {} of {}",
                response.number_returned, response.total_matches
            );
            time(
                &format!("Browse of {title}, {label}"),
                Box::new(move || {
                    std::hint::black_box(answer(&request));
                }),
            );
        }
    }
    let root = request(String::from("0"), 0);
    time(
        "Browse of the root",
        Box::new(move || {
            std::hint::black_box(answer(&root));
        }),
    );
    time(
        "untagged",
        Box::new(|| {
            std::hint::black_box(browse::untagged(&library, &view));
        }),
    );
    time(
        "folder counts of the root",
        Box::new(|| {
            std::hint::black_box(browse::folder_size(&view, ""));
        }),
    );
    time(
        "folder listing of the root",
        Box::new(|| {
            std::hint::black_box(browse::folder(&view, ""));
        }),
    );
}

fn synthetic(count: usize) -> Library {
    let per = |every: usize| (count / every).max(1);
    let (album_artists, composers, genres, dates) = (per(11), per(17), per(32), per(55));
    let artists = per(4);
    let files: Vec<Scanned> = (0..count)
        .map(|n| {
            let album = n / 5;
            let artist = n % artists;
            Scanned {
                path: PathBuf::from(format!("/music/album{album}/{n}.flac")),
                relative: PathBuf::from(format!("album{album}/{n}.flac")),
                tags: FileTags {
                    title: Some(format!("Track {n}")),
                    album: Some(format!("Album {album}")),
                    album_artists: vec![format!("Artist {}", album % album_artists)],
                    artists: vec![format!("Artist {artist}")],
                    composers: vec![format!("Composer {}", n % composers)],
                    genres: vec![format!("Genre {}", n % genres)],
                    date: Some(format!("{}", 1900 + n % dates)),
                    track_number: Some((n % 12) as u32 + 1),
                    ..FileTags::default()
                },
                properties: AudioProperties {
                    duration: Duration::from_secs(180),
                    sample_rate: Some(44_100),
                    bit_depth: Some(16),
                    channels: Some(2),
                    bitrate_bps: Some(1_411_000),
                },
                size: 1,
                artwork: None,
            }
        })
        .collect();
    Library::build("Music".to_owned(), &files)
}
