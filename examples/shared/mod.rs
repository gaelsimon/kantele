#![allow(dead_code)]

use std::path::PathBuf;
use std::time::{Duration, Instant};

use kantele::index::{Library, Scanned};
use kantele::tags::{AudioProperties, FileTags};

pub fn synthetic(count: usize) -> Library {
    Library::build("Music".to_owned(), &synthetic_files(count))
}

pub fn synthetic_files(count: usize) -> Vec<Scanned> {
    let per = |every: usize| (count / every).max(1);
    let (album_artists, composers, genres, dates) = (per(11), per(17), per(32), per(55));
    let artists = per(4);
    (0..count)
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
        .collect()
}

pub fn requested_tracks(default: usize) -> usize {
    std::env::args()
        .nth(1)
        .and_then(|n| n.parse().ok())
        .unwrap_or(default)
}

pub fn time(label: &str, mut work: impl FnMut()) -> Duration {
    work();
    let started = Instant::now();
    for _ in 0..5 {
        work();
    }
    let each = started.elapsed() / 5;
    println!("  {label:<38} {:>7.1} ms", each.as_secs_f64() * 1000.0);
    each
}
