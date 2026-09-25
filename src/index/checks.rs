//! What a listener would want fixed in an album's tags, where no pass refused anything.

use std::collections::{BTreeSet, HashMap};

use crate::index::{Album, Library, Track, fold};
use crate::object::ObjectId;

/// Below this on its longer side a cover shows blurred on a tablet or a television.
pub const SMALL_COVER: u32 = 500;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Check {
    /// The numbers a disc skips, where most of the disc is there.
    Incomplete {
        disc: Option<u32>,
        missing: Vec<u32>,
        total: u32,
    },
    /// Another album carries the same title and credit, so a player lists the two alike.
    Twin {
        folder: String,
    },
    /// No album artist and no compilation flag, over tracks by this many artists.
    Unmarked {
        artists: usize,
    },
    SmallCover {
        width: u32,
        height: u32,
    },
}

#[derive(Clone, Debug, Default)]
pub struct Checks {
    by_album: HashMap<ObjectId, Vec<Check>>,
}

impl Checks {
    pub fn of(library: &Library) -> Self {
        let twins = twins(library);
        let mut by_album: HashMap<ObjectId, Vec<Check>> = HashMap::new();
        for album in library.albums() {
            let tracks: Vec<&Track> = album
                .tracks
                .iter()
                .filter_map(|at| library.tracks().get(*at))
                .collect();
            let mut found = incomplete(library, album);
            found.extend(twins.get(&album.id).map(|folder| Check::Twin {
                folder: folder.clone(),
            }));
            found.extend(unmarked(album, &tracks));
            found.extend(small_cover(album));
            if !found.is_empty() {
                by_album.insert(album.id.clone(), found);
            }
        }
        Self { by_album }
    }

    pub fn on(&self, album: &ObjectId) -> &[Check] {
        self.by_album.get(album).map_or(&[], Vec::as_slice)
    }
}

fn incomplete(library: &Library, album: &Album) -> Vec<Check> {
    album
        .discs
        .iter()
        .filter_map(|disc| {
            let tracks: Vec<&Track> = disc
                .tracks
                .iter()
                .filter_map(|at| library.tracks().get(*at))
                .collect();
            let total = agreed_total(&tracks)?;
            // As many files as the total means the numbering is off rather than a track gone.
            if tracks.len() >= total as usize {
                return None;
            }
            let present: BTreeSet<u32> = tracks
                .iter()
                .filter_map(|track| track.track_number)
                .filter(|number| (1..=total).contains(number))
                .collect();
            // A track or two kept from an album is a choice, so most of the disc has to be here.
            if present.len() < 2 || present.len() * 2 <= total as usize {
                return None;
            }
            let missing: Vec<u32> = (1..=total).filter(|n| !present.contains(n)).collect();
            (!missing.is_empty()).then(|| Check::Incomplete {
                disc: disc.number.filter(|_| album.discs.len() > 1),
                missing,
                total,
            })
        })
        .collect()
}

/// The total every track of a disc gives, or nothing where one gives none or another.
fn agreed_total(tracks: &[&Track]) -> Option<u32> {
    let (first, rest) = tracks.split_first()?;
    let total = first.track_total.filter(|total| *total > 0)?;
    rest.iter()
        .all(|track| track.track_total == Some(total))
        .then_some(total)
}

/// Each album whose title and credit another album carries, with the folder of one such other.
fn twins(library: &Library) -> HashMap<ObjectId, String> {
    let mut alike: HashMap<(String, Vec<String>), Vec<&Album>> = HashMap::new();
    for album in library.albums() {
        let credit = album.artists.iter().map(|one| fold(&one.name)).collect();
        alike
            .entry((fold(&album.title), credit))
            .or_default()
            .push(album);
    }
    let mut twins = HashMap::new();
    for group in alike.values().filter(|group| group.len() > 1) {
        let folders: Vec<String> = group
            .iter()
            .map(|album| folder_of(library, album))
            .collect();
        for (at, album) in group.iter().enumerate() {
            let other = group.iter().enumerate().find(|(elsewhere, other)| {
                *elsewhere != at
                    && folders[*elsewhere] != folders[at]
                    && copies(library, album, other)
            });
            if let Some((elsewhere, _)) = other {
                twins.insert(album.id.clone(), folders[elsewhere].clone());
            }
        }
    }
    twins
}

/// Two copies of one album: each holds most of it, and neither is a few tracks of the other.
fn copies(library: &Library, one: &Album, other: &Album) -> bool {
    let (small, large) = if one.tracks.len() <= other.tracks.len() {
        (one.tracks.len(), other.tracks.len())
    } else {
        (other.tracks.len(), one.tracks.len())
    };
    whole(library, one) && whole(library, other) && small * 2 >= large
}

/// Whether an album holds most of the tracks its files say it has, where they say it.
fn whole(library: &Library, album: &Album) -> bool {
    let totals: Option<Vec<u32>> = album
        .discs
        .iter()
        .map(|disc| {
            let tracks: Vec<&Track> = disc
                .tracks
                .iter()
                .filter_map(|at| library.tracks().get(*at))
                .collect();
            agreed_total(&tracks)
        })
        .collect();
    totals.is_none_or(|totals| album.tracks.len() * 2 > totals.iter().sum::<u32>() as usize)
}

/// The deepest folder holding every track of an album, which is the album's own for one on discs.
fn folder_of(library: &Library, album: &Album) -> String {
    let mut folders = album
        .tracks
        .iter()
        .filter_map(|at| library.tracks().get(*at))
        .map(|track| {
            track
                .relative
                .rfind('/')
                .map_or("", |cut| &track.relative[..cut])
        });
    let Some(first) = folders.next() else {
        return String::new();
    };
    let mut shared: Vec<&str> = first.split('/').collect();
    for folder in folders {
        let parts: Vec<&str> = folder.split('/').collect();
        let same = shared
            .iter()
            .zip(&parts)
            .take_while(|(left, right)| left == right)
            .count();
        shared.truncate(same);
    }
    shared.join("/")
}

fn unmarked(album: &Album, tracks: &[&Track]) -> Option<Check> {
    if album.credited || tracks.iter().any(|track| track.compilation) {
        return None;
    }
    let mut carried: HashMap<String, usize> = HashMap::new();
    for track in tracks {
        let names: BTreeSet<String> = track.artists.iter().map(|one| fold(&one.name)).collect();
        for name in names {
            *carried.entry(name).or_default() += 1;
        }
    }
    let most = carried.values().copied().max()?;
    // An artist on most of the tracks makes it that artist's album, with guests.
    (carried.len() >= 3 && most * 2 <= tracks.len()).then_some(Check::Unmarked {
        artists: carried.len(),
    })
}

fn small_cover(album: &Album) -> Option<Check> {
    let (width, height) = album.artwork.as_ref()?.dimensions?;
    (width.max(height) < SMALL_COVER).then_some(Check::SmallCover { width, height })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use super::*;
    use crate::index::{Artwork, Scanned, Source};
    use crate::tags::{AudioProperties, FileTags};

    fn file(relative: &str, album: &str, artist: &str, track: Option<u32>) -> Scanned {
        Scanned {
            path: Path::new("/music").join(relative),
            relative: PathBuf::from(relative),
            tags: FileTags {
                title: Some(relative.to_owned()),
                album: Some(album.to_owned()),
                artists: vec![artist.to_owned()],
                track_number: track,
                ..FileTags::default()
            },
            properties: AudioProperties {
                duration: Duration::from_secs(180),
                ..AudioProperties::default()
            },
            size: 27,
            artwork: None,
        }
    }

    fn of_twelve(folder: &str, numbers: &[u32]) -> Vec<Scanned> {
        numbers
            .iter()
            .map(|n| {
                let mut one = file(
                    &format!("{folder}/{n:02}.flac"),
                    "Harvest",
                    "Neil Young",
                    Some(*n),
                );
                one.tags.track_total = Some(12);
                one
            })
            .collect()
    }

    fn checks_of(files: &[Scanned]) -> Vec<Vec<Check>> {
        let library = Library::build("Music".to_owned(), files);
        let checks = Checks::of(&library);
        library
            .albums()
            .iter()
            .map(|album| checks.on(&album.id).to_vec())
            .collect()
    }

    #[test]
    fn an_album_missing_a_few_of_its_tracks_says_which() {
        let numbers: Vec<u32> = (1..=12).filter(|n| ![7, 9].contains(n)).collect();
        assert_eq!(
            checks_of(&of_twelve("Harvest", &numbers)),
            vec![vec![Check::Incomplete {
                disc: None,
                missing: vec![7, 9],
                total: 12
            }]]
        );
    }

    #[test]
    fn a_track_or_two_kept_from_an_album_is_a_choice_rather_than_a_hole() {
        for kept in [&[5][..], &[3, 8]] {
            assert_eq!(
                checks_of(&of_twelve("Harvest", kept)),
                vec![Vec::<Check>::new()],
                "{kept:?} of 12"
            );
        }
    }

    #[test]
    fn an_album_whose_files_disagree_on_the_total_or_lack_numbers_is_not_judged() {
        let mut disagreeing = of_twelve("Harvest", &(1..=10).collect::<Vec<_>>());
        disagreeing[0].tags.track_total = Some(10);
        assert_eq!(checks_of(&disagreeing), vec![Vec::<Check>::new()]);

        let mut unnumbered = of_twelve("Harvest", &(1..=11).collect::<Vec<_>>());
        let mut last = file("Harvest/bonus.flac", "Harvest", "Neil Young", None);
        last.tags.track_total = Some(12);
        unnumbered.push(last);
        assert_eq!(
            checks_of(&unnumbered),
            vec![Vec::<Check>::new()],
            "twelve files for twelve tracks: the numbering is off, nothing is missing"
        );
    }

    #[test]
    fn the_same_album_in_two_folders_names_the_other_folder() {
        let mut files = of_twelve("_FLAC/Harvest", &(1..=12).collect::<Vec<_>>());
        files.extend(of_twelve("_ITUNES/Harvest", &(1..=12).collect::<Vec<_>>()));
        let checks = checks_of(&files);
        assert_eq!(checks.len(), 2, "two albums");
        let others: Vec<&Check> = checks.iter().flatten().collect();
        assert!(others.contains(&&Check::Twin {
            folder: "_ITUNES/Harvest".to_owned()
        }));
        assert!(others.contains(&&Check::Twin {
            folder: "_FLAC/Harvest".to_owned()
        }));
    }

    #[test]
    fn a_few_tracks_copied_out_of_an_album_do_not_make_a_twin_of_it() {
        let mut files = of_twelve("_FLAC/Harvest", &(1..=12).collect::<Vec<_>>());
        files.extend(of_twelve("_mariage/selection", &[3]));
        files.extend(of_twelve("_best/Harvest", &[1, 2, 3]));
        assert!(
            checks_of(&files)
                .iter()
                .flatten()
                .all(|check| !matches!(check, Check::Twin { .. })),
            "a selection folder is not a second copy"
        );
    }

    #[test]
    fn a_two_track_ep_in_two_folders_is_a_twin_with_or_without_a_total() {
        let ep = |folder: &str, total: Option<u32>| -> Vec<Scanned> {
            (1..=2)
                .map(|n| {
                    let mut one = file(&format!("{folder}/{n}.flac"), "Fauna", "Motin", Some(n));
                    one.tags.track_total = total;
                    one
                })
                .collect()
        };
        for total in [Some(2), None] {
            let mut files = ep("_FLAC/Fauna", total);
            files.extend(ep("_ITUNES/Fauna", total));
            let twins = checks_of(&files)
                .iter()
                .flatten()
                .filter(|check| matches!(check, Check::Twin { .. }))
                .count();
            assert_eq!(twins, 2, "total {total:?}");
        }
    }

    #[test]
    fn the_discs_of_one_album_are_not_twins() {
        let mut files = Vec::new();
        for disc in [1, 2] {
            for n in 1..=3 {
                let mut one = file(
                    &format!("The Wall/CD{disc}/{n:02}.flac"),
                    "The Wall",
                    "Pink Floyd",
                    Some(n),
                );
                one.tags.disc_number = Some(disc);
                files.push(one);
            }
        }
        assert_eq!(checks_of(&files), vec![Vec::<Check>::new()]);
    }

    #[test]
    fn tracks_by_many_artists_with_no_album_artist_and_no_flag_are_an_unmarked_compilation() {
        let files: Vec<Scanned> = ["Ana", "Bo", "Cy", "Di"]
            .iter()
            .enumerate()
            .map(|(at, artist)| {
                file(
                    &format!("Mix/{at}.flac"),
                    "Mix",
                    artist,
                    Some(at as u32 + 1),
                )
            })
            .collect();
        assert_eq!(
            checks_of(&files),
            vec![vec![Check::Unmarked { artists: 4 }]]
        );

        let mut flagged = files.clone();
        for one in &mut flagged {
            one.tags.compilation = true;
        }
        assert_eq!(
            checks_of(&flagged),
            vec![Vec::<Check>::new()],
            "the flag says it"
        );

        let mut credited = files;
        for one in &mut credited {
            one.tags.album_artists = vec!["DJ Cam".to_owned()];
        }
        assert_eq!(
            checks_of(&credited),
            vec![Vec::<Check>::new()],
            "so does an album artist"
        );
    }

    #[test]
    fn an_artist_on_most_tracks_with_guests_on_the_rest_is_that_artist_s_album() {
        let files: Vec<Scanned> = ["Ana", "Ana", "Ana", "Bo", "Cy"]
            .iter()
            .enumerate()
            .map(|(at, artist)| {
                file(
                    &format!("Solo/{at}.flac"),
                    "Solo",
                    artist,
                    Some(at as u32 + 1),
                )
            })
            .collect();
        assert_eq!(checks_of(&files), vec![Vec::<Check>::new()]);
    }

    #[test]
    fn a_cover_smaller_than_a_screen_wants_is_said_with_its_size() {
        let cover = |width, height| Artwork {
            source: Source::File(PathBuf::from("/music/Harvest/cover.jpg")),
            mime: "image/jpeg",
            dimensions: Some((width, height)),
        };
        let mut small = of_twelve("Harvest", &(1..=12).collect::<Vec<_>>());
        for one in &mut small {
            one.artwork = Some(cover(300, 300));
        }
        assert_eq!(
            checks_of(&small),
            vec![vec![Check::SmallCover {
                width: 300,
                height: 300
            }]]
        );

        let mut large = small;
        for one in &mut large {
            one.artwork = Some(cover(SMALL_COVER, SMALL_COVER));
        }
        assert_eq!(checks_of(&large), vec![Vec::<Check>::new()]);
    }
}
