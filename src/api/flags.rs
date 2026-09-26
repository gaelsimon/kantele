//! What the tree can be narrowed to, declared once for the listing and for the page.

use std::collections::HashSet;

use serde::de::Error as _;
use serde::{Deserialize, Deserializer, Serialize};

use crate::browse::Served;
use crate::index::{Cause, Check, Missing, Track};
use crate::object::ObjectId;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flag {
    Unserved,
    BrokenLinks,
    DuplicateTracks,
    Unmarked,
    TrackGaps,
    SmallCover,
    GenreSpellings,
    ArtistSpellings,
    /// A tag gets a check with one line in [`CHECKS`] and nothing else.
    Lacking(Missing),
}

/// Where the fix is made: in a file manager or a playlist, or in a tagger.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Group {
    Files,
    Tags,
}

/// One check, with its name on the wire and the words the page shows for it.
#[derive(Debug, Serialize)]
pub struct Declared {
    #[serde(skip)]
    pub of: Flag,
    pub flag: &'static str,
    pub group: Group,
    pub label: &'static str,
    pub says: &'static str,
    /// Whether a line parts it from the checks above it.
    pub apart: bool,
}

const fn check(
    of: Flag,
    flag: &'static str,
    group: Group,
    label: &'static str,
    says: &'static str,
) -> Declared {
    Declared {
        of,
        flag,
        group,
        label,
        says,
        apart: false,
    }
}

const fn apart(declared: Declared) -> Declared {
    Declared {
        apart: true,
        ..declared
    }
}

/// Every check, in the order the page lists them.
pub const CHECKS: &[Declared] = &[
    check(
        Flag::Unserved,
        "unserved",
        Group::Files,
        "Files not served",
        "Your players never see them: unreadable files, empty playlists",
    ),
    check(
        Flag::BrokenLinks,
        "broken-links",
        Group::Files,
        "Broken playlist links",
        "The playlist plays without them",
    ),
    check(
        Flag::DuplicateTracks,
        "duplicate-tracks",
        Group::Files,
        "Duplicate tracks",
        "The same recording in two places",
    ),
    check(
        Flag::Lacking(Missing::Artist),
        "no-artist",
        Group::Tags,
        "No artist",
        "Tracks with no artist tag",
    ),
    check(
        Flag::Lacking(Missing::Album),
        "no-album",
        Group::Tags,
        "No album",
        "Tracks with no album tag",
    ),
    check(
        Flag::Lacking(Missing::Genre),
        "no-genre",
        Group::Tags,
        "No genre",
        "Missing from the Genre menu",
    ),
    check(
        Flag::Lacking(Missing::Artwork),
        "no-cover",
        Group::Tags,
        "No cover",
        "A blank square on the player",
    ),
    apart(check(
        Flag::Lacking(Missing::Date),
        "no-date",
        Group::Tags,
        "No date",
        "Missing from the Date menu",
    )),
    check(
        Flag::GenreSpellings,
        "genre-spellings",
        Group::Tags,
        "Genres spelled several ways",
        "Listed twice in the Genre menu",
    ),
    check(
        Flag::ArtistSpellings,
        "artist-spellings",
        Group::Tags,
        "Artists spelled several ways",
        "Listed twice in the Artist menu",
    ),
    check(
        Flag::SmallCover,
        "small-cover",
        Group::Tags,
        "Small cover",
        "Blurred on a tablet or a television",
    ),
    check(
        Flag::Unmarked,
        "unmarked-compilations",
        Group::Tags,
        "Unmarked compilations",
        "Listed once per performer under Artist",
    ),
    check(
        Flag::TrackGaps,
        "track-gaps",
        Group::Tags,
        "Gaps in track numbers",
        "Fewer files than the track total says",
    ),
];

impl Flag {
    pub fn lacking(self) -> Option<Missing> {
        match self {
            Self::Lacking(tag) => Some(tag),
            _ => None,
        }
    }

    pub fn of(check: &Check) -> Self {
        match check {
            Check::Incomplete { .. } => Self::TrackGaps,
            Check::Unmarked { .. } => Self::Unmarked,
            Check::SmallCover { .. } => Self::SmallCover,
        }
    }

    /// The refusals counted under this flag. What the tags decided is under none.
    pub fn counts(self, cause: Cause) -> bool {
        match self {
            Self::Unserved => matches!(
                cause,
                Cause::UnreadableFolder
                    | Cause::UnreadableFile
                    | Cause::UnreadablePlaylist
                    | Cause::UnpublishedPlaylist
                    | Cause::LinkedOutside
                    | Cause::UnreadableRow
            ),
            Self::BrokenLinks => matches!(cause, Cause::MissingEntry | Cause::RepeatedEntry),
            _ => false,
        }
    }

    fn named(name: &str) -> Option<Self> {
        CHECKS.iter().find(|one| one.flag == name).map(|one| one.of)
    }
}

/// Which tracks the flags asked for hold, worked out once per answer so the folders and the files
/// of one view agree.
pub struct Asking<'a> {
    served: &'a Served,
    only: &'a [Flag],
    /// The albums a check asked for found wanting.
    wanting: HashSet<&'a ObjectId>,
}

impl<'a> Asking<'a> {
    pub fn new(served: &'a Served, only: &'a [Flag]) -> Self {
        let wanting = served
            .library
            .albums()
            .iter()
            .filter(|album| {
                served
                    .counts
                    .checks
                    .on(&album.id)
                    .iter()
                    .any(|check| only.contains(&Flag::of(check)))
            })
            .map(|album| &album.id)
            .collect();
        Self {
            served,
            only,
            wanting,
        }
    }

    pub fn anything(&self) -> bool {
        !self.only.is_empty()
    }

    pub fn asks(&self, flag: Flag) -> bool {
        self.only.contains(&flag)
    }

    pub fn holds(&self, at: usize, track: &Track) -> bool {
        let checks = &self.served.counts.checks;
        self.only.iter().any(|flag| match flag {
            Flag::Lacking(tag) => tag.absent(track),
            Flag::DuplicateTracks => checks.copied(at),
            Flag::GenreSpellings => checks.genres().minor_on(at),
            Flag::ArtistSpellings => checks.artists().minor_on(at),
            _ => false,
        }) || track
            .album_id
            .as_ref()
            .is_some_and(|album| self.wanting.contains(album))
    }
}

/// `only=duplicate-tracks,no-genre`, as the listing is asked.
pub fn listed<'de, D: Deserializer<'de>>(asked: D) -> Result<Vec<Flag>, D::Error> {
    String::deserialize(asked)?
        .split(',')
        .filter(|one| !one.is_empty())
        .map(|one| {
            Flag::named(one)
                .ok_or_else(|| D::Error::custom(format!("{one} is no check this server makes")))
        })
        .collect()
}

/// The same, for a shell with no `jq`.
pub fn lines() -> Vec<(String, String)> {
    CHECKS
        .iter()
        .enumerate()
        .map(|(at, one)| {
            let group = match one.group {
                Group::Files => "files",
                Group::Tags => "tags",
            };
            (
                format!("flag.{}", at + 1),
                format!("{}\t{group}\t{}", one.flag, one.label),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_check_has_a_name_of_its_own_that_asks_for_it() {
        for one in CHECKS {
            assert_eq!(
                CHECKS.iter().filter(|other| other.flag == one.flag).count(),
                1,
                "{}",
                one.flag
            );
            assert_eq!(Flag::named(one.flag), Some(one.of), "{}", one.flag);
        }
    }

    #[test]
    fn every_problem_a_folder_can_hold_is_under_a_flag_and_no_note_is() {
        for cause in Cause::ALL {
            let flagged = [Flag::Unserved, Flag::BrokenLinks]
                .iter()
                .any(|flag| flag.counts(*cause));
            assert_eq!(flagged, cause.is_problem(), "{cause:?}");
        }
    }

    #[test]
    fn every_check_an_album_can_fail_is_offered() {
        let offered: Vec<Flag> = CHECKS.iter().map(|one| one.of).collect();
        for check in [
            Check::Incomplete {
                disc: None,
                missing: vec![3],
                total: 4,
            },
            Check::Unmarked { artists: 3 },
            Check::SmallCover {
                width: 1,
                height: 1,
            },
        ] {
            assert!(offered.contains(&Flag::of(&check)), "{check:?}");
        }
    }
}
