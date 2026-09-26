//! What a listener would want fixed in an album's tags, where no pass refused anything.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;

use crate::index::{Album, Library, Spelling, Track, fold};
use crate::object::ObjectId;

/// Below this on its longer side a cover shows blurred on a tablet or a television.
pub const SMALL_COVER: u32 = 500;

/// Two tracks of one title and one artist this close in length are taken for one recording.
pub const SAME_LENGTH: Duration = Duration::from_secs(2);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Check {
    /// The numbers a disc skips, where most of the disc is there.
    Incomplete {
        disc: Option<u32>,
        missing: Vec<u32>,
        total: u32,
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
    /// The tracks holding one recording, by their index in the library.
    copies: Vec<Vec<usize>>,
    /// Which of `copies` each track that has one is in.
    copied: HashMap<usize, usize>,
    genres: Spelling,
    /// Artists and album artists together, as one name is either.
    artists: Spelling,
}

impl Checks {
    pub fn of(library: &Library) -> Self {
        let mut by_album: HashMap<ObjectId, Vec<Check>> = HashMap::new();
        for album in library.albums() {
            let tracks: Vec<&Track> = album
                .tracks
                .iter()
                .filter_map(|at| library.tracks().get(*at))
                .collect();
            let mut found = incomplete(library, album);
            found.extend(unmarked(album, &tracks));
            found.extend(small_cover(album));
            if !found.is_empty() {
                by_album.insert(album.id.clone(), found);
            }
        }
        let copies = recordings(library.tracks());
        let copied = copies
            .iter()
            .enumerate()
            .flat_map(|(group, tracks)| tracks.iter().map(move |at| (*at, group)))
            .collect();
        let tracks = library.tracks();
        Self {
            by_album,
            copies,
            copied,
            genres: Spelling::of(tracks, |track| track.genres.iter().map(String::as_str)),
            artists: Spelling::of(tracks, |track| {
                track
                    .artists
                    .iter()
                    .chain(&track.album_artists)
                    .map(|one| one.name.as_str())
            }),
        }
    }

    pub fn on(&self, album: &ObjectId) -> &[Check] {
        self.by_album.get(album).map_or(&[], Vec::as_slice)
    }

    pub fn copied(&self, track: usize) -> bool {
        self.copied.contains_key(&track)
    }

    pub fn genres(&self) -> &Spelling {
        &self.genres
    }

    pub fn artists(&self) -> &Spelling {
        &self.artists
    }

    /// The other tracks holding the recording this one holds.
    pub fn copies_of(&self, track: usize) -> impl Iterator<Item = usize> + '_ {
        self.copied
            .get(&track)
            .into_iter()
            .flat_map(|group| self.copies[*group].iter().copied())
            .filter(move |at| *at != track)
    }
}

/// One recording is one MusicBrainz recording, or one title by one artist at about one length.
fn recordings(tracks: &[Track]) -> Vec<Vec<usize>> {
    let mut joined = Joined::new(tracks.len());
    let mut recorded: HashMap<&str, usize> = HashMap::new();
    let mut named: HashMap<(String, Vec<String>), Vec<usize>> = HashMap::new();
    for (at, track) in tracks.iter().enumerate() {
        if let Some(recording) = track.recording_mbid.as_deref() {
            let first = *recorded.entry(recording).or_insert(at);
            if first != at {
                joined.join(first, at);
            }
        }
        // A title the file name stood in for, or no artist, says nothing of the recording.
        if !track.title_tagged || track.artists.is_empty() {
            continue;
        }
        let mut artists: Vec<String> = track.artists.iter().map(|one| fold(&one.name)).collect();
        artists.sort_unstable();
        artists.dedup();
        named
            .entry((fold(&track.title), artists))
            .or_default()
            .push(at);
    }
    for mut alike in named.into_values().filter(|alike| alike.len() > 1) {
        alike.sort_by_key(|at| tracks[*at].duration);
        for pair in alike.windows(2) {
            let apart = tracks[pair[1]].duration - tracks[pair[0]].duration;
            if apart <= SAME_LENGTH {
                joined.join(pair[0], pair[1]);
            }
        }
    }
    joined.groups()
}

/// Tracks found to hold one recording, merged as they are found.
struct Joined {
    parent: Vec<usize>,
    touched: Vec<usize>,
}

impl Joined {
    fn new(tracks: usize) -> Self {
        Self {
            parent: (0..tracks).collect(),
            touched: Vec::new(),
        }
    }

    fn root(&mut self, mut at: usize) -> usize {
        while self.parent[at] != at {
            self.parent[at] = self.parent[self.parent[at]];
            at = self.parent[at];
        }
        at
    }

    fn join(&mut self, one: usize, other: usize) {
        let (one_root, other_root) = (self.root(one), self.root(other));
        if one_root != other_root {
            self.parent[other_root] = one_root;
        }
        self.touched.extend([one, other]);
    }

    fn groups(mut self) -> Vec<Vec<usize>> {
        let mut by_root: BTreeMap<usize, BTreeSet<usize>> = BTreeMap::new();
        for at in std::mem::take(&mut self.touched) {
            let root = self.root(at);
            by_root.entry(root).or_default().insert(at);
        }
        by_root
            .into_values()
            .map(|group| group.into_iter().collect())
            .collect()
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

    fn recording(relative: &str, title: Option<&str>, artist: &str, seconds: u64) -> Scanned {
        let mut one = file(relative, relative, artist, None);
        one.tags.title = title.map(str::to_owned);
        one.properties.duration = Duration::from_secs(seconds);
        one
    }

    /// Every track with a copy, by path, with the paths of its copies.
    fn copied(files: &[Scanned]) -> Vec<(String, Vec<String>)> {
        let library = Library::build("Music".to_owned(), files);
        let checks = Checks::of(&library);
        let tracks = library.tracks();
        let mut found: Vec<(String, Vec<String>)> = (0..tracks.len())
            .filter(|at| checks.copied(*at))
            .map(|at| {
                let mut others: Vec<String> = checks
                    .copies_of(at)
                    .map(|other| tracks[other].relative.clone())
                    .collect();
                others.sort();
                (tracks[at].relative.clone(), others)
            })
            .collect();
        found.sort();
        found
    }

    #[test]
    fn one_title_by_one_artist_at_about_one_length_is_one_recording_in_two_places() {
        let files = [
            recording("Harvest/05.flac", Some("Harvest Moon"), "Neil Young", 300),
            recording(
                "Selection/harvest moon.mp3",
                Some("harvest moon"),
                "NEIL YOUNG",
                301,
            ),
        ];
        assert_eq!(
            copied(&files),
            [
                (
                    "Harvest/05.flac".to_owned(),
                    vec!["Selection/harvest moon.mp3".to_owned()]
                ),
                (
                    "Selection/harvest moon.mp3".to_owned(),
                    vec!["Harvest/05.flac".to_owned()]
                ),
            ]
        );
    }

    #[test]
    fn a_longer_version_another_artist_or_a_file_with_no_title_is_no_copy() {
        let files = [
            recording("Harvest/05.flac", Some("Harvest Moon"), "Neil Young", 300),
            recording("Live/05.flac", Some("Harvest Moon"), "Neil Young", 420),
            recording(
                "Covers/05.flac",
                Some("Harvest Moon"),
                "Cassandra Wilson",
                300,
            ),
            recording("Loose/harvest.mp3", None, "Neil Young", 300),
        ];
        assert!(copied(&files).is_empty(), "{:?}", copied(&files));
    }

    #[test]
    fn one_musicbrainz_recording_is_one_recording_whatever_its_tags_say() {
        let mut album = recording("Harvest/05.flac", Some("Harvest Moon"), "Neil Young", 300);
        let mut compilation = recording(
            "Various/12.flac",
            Some("Harvest Moon (Remastered)"),
            "Various",
            297,
        );
        for one in [&mut album, &mut compilation] {
            one.tags.musicbrainz_recording_id =
                Some("2f1d6c3e-8b4a-4c6e-9a1d-3b5e7f9a1c2d".to_owned());
        }
        assert_eq!(copied(&[album, compilation]).len(), 2);
    }

    #[test]
    fn copies_a_little_apart_each_are_one_group() {
        let files = [
            recording("a.flac", Some("Intro"), "Batida", 60),
            recording("b.mp3", Some("Intro"), "Batida", 61),
            recording("c/intro.flac", Some("Intro"), "Batida", 62),
        ];
        let found = copied(&files);
        assert_eq!(found.len(), 3);
        assert!(
            found.iter().all(|(_, others)| others.len() == 2),
            "{found:?}"
        );
    }
}
