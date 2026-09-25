//! What one folder holds, so a pane that shows a folder can open onto its files and what they
//! belong to.

use serde::{Deserialize, Serialize};

use crate::browse::{Served, albums_of, tracks_in};
use crate::index::{Check, Track};

/// Rows one answer carries. A folder of a thousand files is read as far as this and says so.
const MOST: usize = 500;

#[derive(Debug, Default, Deserialize)]
pub struct Asked {
    /// The folder to list, empty for the top of the library.
    #[serde(default)]
    pub folder: String,
}

/// One file, as a pane lists it before anybody opens it.
#[derive(Debug, Serialize)]
pub struct Row {
    /// The path relative to the music folder, which is what `/api/track` is asked for.
    pub path: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub number: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artwork: Option<String>,
    pub seconds: u64,
}

/// One album the files in this folder belong to. Two of these is a folder holding two albums; one
/// whose `tracks` fall short of `of` is an album this folder holds a part of.
#[derive(Debug, Serialize)]
pub struct Album {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    /// What to ask `/art/` for, wherever the cover sits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artwork: Option<String>,
    /// The folder the cover sits in, named only where that is not this folder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cover_in: Option<String>,
    /// Tracks of it directly in this folder.
    pub tracks: usize,
    /// Tracks it has in all, which is more where it is spread over folders.
    pub of: usize,
    /// Every folder its tracks sit in, this one included, in the library's order.
    pub folders: Vec<String>,
    /// Which disc this is, or how much of the album is elsewhere. Nothing where it is all here.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub says: Option<String>,
    /// What a listener would want fixed in its tags.
    pub checks: Vec<Checked>,
}

#[derive(Debug, Serialize)]
pub struct Checked {
    pub says: String,
    /// The other folder, for an album another one carries as well.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct Listing {
    pub folder: String,
    pub files: Vec<Row>,
    /// What the files here belong to, which no folder tree shows.
    pub albums: Vec<Album>,
    /// Whether files were left out because one answer is bounded.
    pub more: bool,
}

/// The files directly in one folder, or nothing where the library holds no such folder.
/// `failed` says whether a folder the tree lacks holds files that would not read.
pub fn listing(
    served: &Served,
    asked: &Asked,
    failed: impl FnOnce(&str) -> bool,
) -> Option<Listing> {
    if !served.view.folders().holds(&asked.folder) && !failed(&asked.folder) {
        return None;
    }
    let at = tracks_in(&served.view, &asked.folder);
    let files = at
        .iter()
        .take(MOST)
        .filter_map(|at| served.library.tracks().get(*at))
        .map(|track| Row {
            path: track.relative.clone(),
            title: track.title.clone(),
            number: track.track_number,
            artwork: track.artwork.as_ref().map(|_| track.id.as_str().to_owned()),
            seconds: track.duration.as_secs(),
        })
        .collect();
    Some(Listing {
        albums: albums_here(served, &asked.folder, &at),
        folder: asked.folder.clone(),
        files,
        more: at.len() > MOST,
    })
}

/// What the files in a folder belong to. A folder is not an album: one folder holds two of them,
/// and one album lies across the disc folders under it.
fn albums_here(served: &Served, folder: &str, here: &[usize]) -> Vec<Album> {
    let library = &served.library;
    let track_at = |at: &usize| library.tracks().get(*at);
    albums_of(library, here)
        .into_iter()
        .map(|album| {
            let mine: Vec<&Track> = album
                .tracks
                .iter()
                .filter_map(track_at)
                .filter(|track| folder_of(&track.relative) == folder)
                .collect();
            let mut folders: Vec<String> = Vec::new();
            for track in album.tracks.iter().filter_map(track_at) {
                let holding = folder_of(&track.relative);
                if !folders.iter().any(|seen| seen == holding) {
                    folders.push(holding.to_owned());
                }
            }
            let cover = album
                .tracks
                .iter()
                .filter_map(track_at)
                .find(|track| track.artwork.is_some());
            Album {
                id: album.id.as_str().to_owned(),
                title: album.title.clone(),
                credit: crate::report::credited(&album.artists),
                date: album.date.clone(),
                artwork: cover.map(|track| track.id.as_str().to_owned()),
                cover_in: cover
                    .map(|track| folder_of(&track.relative))
                    .filter(|holding| *holding != folder)
                    .map(ToOwned::to_owned),
                says: crate::report::album_part(
                    mine.len(),
                    album.tracks.len(),
                    folders.len(),
                    disc_of(&mine, album.disc_count),
                ),
                tracks: mine.len(),
                of: album.tracks.len(),
                folders,
                checks: served
                    .counts
                    .checks
                    .on(&album.id)
                    .iter()
                    .map(|check| Checked {
                        says: crate::report::check(check),
                        folder: match check {
                            Check::Twin { folder } => Some(folder.clone()),
                            _ => None,
                        },
                    })
                    .collect(),
            }
        })
        .collect()
}

/// The disc these tracks are, where they are one disc of an album that has several.
fn disc_of(mine: &[&Track], discs: u32) -> Option<(u32, u32)> {
    if discs < 2 {
        return None;
    }
    let first = mine.first()?.disc_number?;
    mine.iter()
        .all(|track| track.disc_number == Some(first))
        .then_some((first, discs))
}

/// The folder a track sits in, empty for one at the top of the library.
fn folder_of(relative: &str) -> &str {
    relative.rfind('/').map_or("", |cut| &relative[..cut])
}

/// The same, for a shell with no `jq`.
pub fn lines(listing: &Listing) -> Vec<(String, String)> {
    let mut lines = vec![("folder".to_owned(), listing.folder.clone())];
    for (at, album) in listing.albums.iter().enumerate() {
        lines.push((
            format!("album.{}", at + 1),
            format!(
                "{}\t{} of {} tracks{}",
                album.title,
                album.tracks,
                album.of,
                album
                    .says
                    .as_deref()
                    .map(|says| format!("\t{says}"))
                    .unwrap_or_default()
            ),
        ));
        for (checked, check) in album.checks.iter().enumerate() {
            lines.push((
                format!("album.{}.check.{}", at + 1, checked + 1),
                check.says.clone(),
            ));
        }
    }
    for (at, file) in listing.files.iter().enumerate() {
        lines.push((
            format!("file.{}", at + 1),
            format!("{}\t{}\t{}s", file.path, file.title, file.seconds),
        ));
    }
    lines.push(("more".to_owned(), listing.more.to_string()));
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{Artwork, Library, Scanned, Source};

    fn track(relative: &str, album: &str, disc: Option<u32>, cover: bool) -> Scanned {
        Scanned {
            path: std::path::PathBuf::from(format!("/music/{relative}")),
            relative: std::path::PathBuf::from(relative),
            tags: crate::tags::FileTags {
                title: Some(relative.to_owned()),
                album: Some(album.to_owned()),
                album_artists: vec!["A Tribe Called Quest".to_owned()],
                track_number: Some(1),
                disc_number: disc,
                ..Default::default()
            },
            properties: crate::tags::AudioProperties::default(),
            size: 27,
            artwork: cover.then(|| Artwork {
                source: Source::File(std::path::PathBuf::from(format!("/music/{relative}"))),
                mime: "image/jpeg",
                dimensions: None,
            }),
        }
    }

    fn albums_of_folder(files: &[Scanned], folder: &str) -> Vec<Album> {
        let served = Served::new(
            Library::build("Music".to_owned(), files),
            crate::browse::Settings::default(),
        );
        listing(
            &served,
            &Asked {
                folder: folder.to_owned(),
            },
            |_| false,
        )
        .expect("the folder is in the tree")
        .albums
    }

    #[test]
    fn a_folder_holding_two_albums_answers_two_of_them() {
        let albums = albums_of_folder(
            &[
                track("Compilations/01.flac", "One Night", None, false),
                track("Compilations/02.flac", "One Night", None, false),
                track("Compilations/03.flac", "Another Night", None, false),
            ],
            "Compilations",
        );
        let titles: Vec<&str> = albums.iter().map(|album| album.title.as_str()).collect();
        assert_eq!(titles, ["One Night", "Another Night"]);
        assert!(
            albums.iter().all(|album| album.says.is_none()),
            "both are wholly in this folder"
        );
        assert_eq!(albums[0].tracks, 2);
        assert_eq!(albums[0].of, 2);
    }

    #[test]
    fn one_album_on_two_discs_says_which_disc_a_folder_holds_and_where_its_cover_is() {
        let files = [
            track(
                "Midnight Marauders/Disc 1/01.flac",
                "Midnight Marauders",
                Some(1),
                true,
            ),
            track(
                "Midnight Marauders/Disc 2/01.flac",
                "Midnight Marauders",
                Some(2),
                false,
            ),
        ];
        let second = albums_of_folder(&files, "Midnight Marauders/Disc 2");
        assert_eq!(second.len(), 1, "two folders, one album");
        let album = &second[0];
        assert_eq!((album.tracks, album.of), (1, 2));
        assert_eq!(album.says.as_deref(), Some("Disc 2 of 2, 1 of 2 tracks"));
        assert_eq!(
            album.folders,
            ["Midnight Marauders/Disc 1", "Midnight Marauders/Disc 2"]
        );
        assert_eq!(
            album.cover_in.as_deref(),
            Some("Midnight Marauders/Disc 1"),
            "the second disc is drawn with the first disc's cover"
        );

        let first = albums_of_folder(&files, "Midnight Marauders/Disc 1");
        assert!(
            first[0].cover_in.is_none(),
            "and the folder the cover is in does not name itself"
        );
        assert!(first[0].artwork.is_some());
    }

    #[test]
    fn disc_folders_with_no_disc_tag_are_still_read_as_discs() {
        let albums = albums_of_folder(
            &[
                track("Live/CD1/01.flac", "Live at Leeds", None, false),
                track("Live/CD2/01.flac", "Live at Leeds", None, false),
            ],
            "Live/CD1",
        );
        assert_eq!(
            albums[0].says.as_deref(),
            Some("Disc 1 of 2, 1 of 2 tracks")
        );
    }

    #[test]
    fn an_album_split_over_folders_nothing_numbers_says_how_many_hold_it() {
        // A disc total above one is what puts two folders in one album where their names do not.
        let part = |relative: &str| {
            let mut file = track(relative, "Live at Leeds", None, false);
            file.tags.disc_total = Some(2);
            file
        };
        let albums = albums_of_folder(
            &[
                part("Live at Leeds/Bonus/01.flac"),
                part("Live at Leeds/Main/01.flac"),
            ],
            "Live at Leeds/Bonus",
        );
        assert_eq!(
            albums[0].says.as_deref(),
            Some("1 of 2 tracks, in 2 folders"),
            "one album in two folders no disc marker explains"
        );
    }

    #[test]
    fn a_folder_of_folders_belongs_to_no_album_of_its_own() {
        let albums = albums_of_folder(
            &[track(
                "Blue Note/Sierra/01.flac",
                "Dundunbanza",
                None,
                false,
            )],
            "Blue Note",
        );
        assert!(albums.is_empty(), "nothing is directly in it");
    }
}
