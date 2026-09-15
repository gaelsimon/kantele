//! How many tracks carry each tag the menus are built from, counted after cleaning.

use crate::index::{Library, Track};

/// Disc number, composer and the MusicBrainz ids are left out: nobody goes and fills those in.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Missing {
    Artist,
    Album,
    AlbumArtist,
    Date,
    Genre,
    TrackNumber,
    Artwork,
}

impl Missing {
    pub fn absent(self, track: &Track) -> bool {
        match self {
            Self::Artist => track.artists.is_empty(),
            Self::Album => track.album.is_none(),
            Self::AlbumArtist => track.album_artists.is_empty(),
            Self::Date => track.date.is_none(),
            Self::Genre => track.genres.is_empty(),
            Self::TrackNumber => track.track_number.is_none(),
            Self::Artwork => track.artwork.is_none(),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Coverage {
    pub tracks: usize,
    pub artist: usize,
    pub album: usize,
    pub album_artist: usize,
    pub date: usize,
    pub genre: usize,
    pub track_number: usize,
    pub disc_number: usize,
    pub composer: usize,
    pub recording_mbid: usize,
    pub artwork: usize,
}

impl Coverage {
    pub fn of(library: &Library) -> Self {
        let mut coverage = Self {
            tracks: library.len(),
            ..Self::default()
        };
        for track in library.tracks() {
            coverage.artist += usize::from(!track.artists.is_empty());
            coverage.album += usize::from(track.album.is_some());
            coverage.album_artist += usize::from(!track.album_artists.is_empty());
            coverage.date += usize::from(track.date.is_some());
            coverage.genre += usize::from(!track.genres.is_empty());
            coverage.track_number += usize::from(track.track_number.is_some());
            coverage.disc_number += usize::from(track.disc_number.is_some());
            coverage.composer += usize::from(!track.composers.is_empty());
            coverage.recording_mbid += usize::from(track.recording_mbid.is_some());
            coverage.artwork += usize::from(track.artwork.is_some());
        }
        coverage
    }

    /// Every field with its count, in the order a report prints them.
    pub fn fields(&self) -> [(&'static str, usize); 10] {
        [
            ("artist", self.artist),
            ("album", self.album),
            ("album_artist", self.album_artist),
            ("date", self.date),
            ("genre", self.genre),
            ("track_number", self.track_number),
            ("disc_number", self.disc_number),
            ("composer", self.composer),
            ("recording_mbid", self.recording_mbid),
            ("artwork", self.artwork),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::Scanned;
    use crate::tags::FileTags;
    use std::path::PathBuf;

    fn file(name: &str, tags: FileTags) -> Scanned {
        Scanned {
            path: PathBuf::from(format!("/music/a/{name}.flac")),
            relative: PathBuf::from(format!("a/{name}.flac")),
            tags,
            properties: crate::tags::AudioProperties::default(),
            size: 1,
            artwork: None,
        }
    }

    #[test]
    fn each_field_counts_the_tracks_that_carry_it() {
        let library = Library::build(
            "Music".to_owned(),
            &[
                file(
                    "1",
                    FileTags {
                        title: Some("One".to_owned()),
                        artists: vec!["Ana".to_owned()],
                        album: Some("Songs".to_owned()),
                        date: Some("1999".to_owned()),
                        track_number: Some(1),
                        ..FileTags::default()
                    },
                ),
                file(
                    "2",
                    FileTags {
                        title: Some("Two".to_owned()),
                        album: Some("Songs".to_owned()),
                        genres: vec!["Son".to_owned()],
                        ..FileTags::default()
                    },
                ),
            ],
        );
        let coverage = Coverage::of(&library);
        assert_eq!(coverage.tracks, 2);
        assert_eq!(coverage.artist, 1);
        assert_eq!(coverage.album, 2);
        assert_eq!(coverage.date, 1);
        assert_eq!(coverage.genre, 1);
        assert_eq!(coverage.track_number, 1);
        assert_eq!(coverage.composer, 0);
        assert_eq!(coverage.fields().len(), 10);
    }

    #[test]
    fn an_empty_library_covers_nothing_and_does_not_divide_by_it() {
        let coverage = Coverage::of(&Library::default());
        assert_eq!(coverage.tracks, 0);
        assert!(coverage.fields().iter().all(|(_, n)| *n == 0));
    }
}
