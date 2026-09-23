//! The music folder as a tree, with what the server made of each folder.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::browse::Served;
use crate::index::{Missing, Refusals, Tally};
use crate::service::Scope;

/// Rows one answer carries. A library of thousands of folders is read a level at a time.
const MOST: usize = 500;

#[derive(Debug, Default, Deserialize)]
pub struct Asked {
    /// The folder to list, empty for the top of the library.
    #[serde(default)]
    pub under: String,
    /// A search over folder names, answered from the whole tree rather than from one level.
    pub q: Option<String>,
    /// Only the folders the last pass read again.
    #[serde(default)]
    pub changed: bool,
    /// Only the folders holding tracks that carry no such tag.
    pub missing: Option<Missing>,
}

#[derive(Debug, Serialize)]
pub struct Listing {
    pub under: String,
    /// The folder asked for, as its own row.
    pub here: Row,
    pub folders: Vec<Row>,
    /// Whether rows were left out because one answer is bounded.
    pub more: bool,
}

/// One folder: where it is, what is under it, and what the server made of it.
#[derive(Debug, Serialize)]
pub struct Row {
    pub path: String,
    pub name: String,
    /// Folders directly under it, which says whether the row opens.
    pub folders: usize,
    /// Everything below it, not only the folder's own files.
    pub tracks: usize,
    pub albums: usize,
    /// Only what went wrong. What the tags decided is in the sentence.
    pub problems: usize,
    pub notes: usize,
    /// Tracks below it carrying no such tag, where one was asked about.
    pub missing: usize,
    /// Whether the last pass read this folder again.
    pub changed: bool,
    /// What to ask `/art/` for: the first track below it that carries a cover, which in a folder
    /// holding one album is that album's.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artwork: Option<String>,
    /// What the folder holds. What is wrong with it is in `issues`, one entry each.
    pub says: String,
    pub issues: Vec<Issue>,
}

/// One thing wrong with a folder, named the way a form names a field.
#[derive(Debug, Serialize)]
pub struct Issue {
    /// A refusal cause, or `no-artwork`, which no pass refuses but a listener still sees.
    pub cause: String,
    pub label: &'static str,
    pub count: usize,
    /// What the count counts: files, tracks, links.
    pub subject: &'static str,
    /// A fault, as against something the tags decided.
    pub problem: bool,
    /// Whether asking for the files behind it will name any.
    pub opens: bool,
}

/// The tree under `asked.under`, or the folders matching `asked.q` from anywhere in it.
pub fn listing(
    served: &Served,
    refusals: &Refusals,
    walked: Option<&Scope>,
    asked: &Asked,
) -> Option<Listing> {
    let folders = served.view.folders();
    if !folders.holds(&asked.under) {
        return None;
    }
    let problems = problems_below(refusals);
    let counted = Counting {
        served,
        problems: &problems,
        walked,
        missing: asked.missing,
    };
    let wanted = asked.q.as_deref().map(str::trim).filter(|q| !q.is_empty());
    let paths: Vec<String> = match wanted {
        Some(wanted) => matching(served, &asked.under, wanted),
        None => children_of(served, &asked.under),
    };
    // Counted past the cap, because a filter that rejects the first five hundred folders would
    // otherwise answer that the library holds none of what it holds.
    let mut rows = paths
        .iter()
        .map(|path| counted.row(path))
        .filter(|row| !asked.changed || row.changed)
        .filter(|row| asked.missing.is_none() || row.missing > 0);
    let folders: Vec<Row> = rows.by_ref().take(MOST).collect();
    Some(Listing {
        here: counted.row(&asked.under),
        under: asked.under.clone(),
        folders,
        more: rows.next().is_some(),
    })
}

/// The folders directly under a path, in the order the tree holds them.
fn children_of(served: &Served, under: &str) -> Vec<String> {
    let (below, _) = crate::browse::folder(&served.view, under);
    below.into_iter().map(|child| child.path).collect()
}

/// Every folder in the tree under `under` whose name holds the words asked for.
fn matching(served: &Served, under: &str, wanted: &str) -> Vec<String> {
    let wanted = crate::index::fold(wanted);
    let mut found: Vec<String> = served
        .view
        .folders()
        .paths()
        .filter(|path| !path.is_empty() && inside(path, under))
        .filter(|path| crate::index::fold(name_of(path)).contains(&wanted))
        .map(ToOwned::to_owned)
        .collect();
    found.sort();
    found
}

fn inside(path: &str, under: &str) -> bool {
    under.is_empty() || path.starts_with(&format!("{under}/"))
}

fn name_of(path: &str) -> &str {
    crate::browse::folder_name(path)
}

/// The numbers for one row, read off the library rather than kept anywhere.
struct Counting<'a> {
    served: &'a Served,
    problems: &'a HashMap<&'a str, Tally>,
    walked: Option<&'a Scope>,
    missing: Option<Missing>,
}

impl Counting<'_> {
    fn lacking(&self, tracks: &[usize], wanted: Missing) -> usize {
        tracks
            .iter()
            .filter_map(|at| self.served.library.tracks().get(*at))
            .filter(|track| wanted.absent(track))
            .count()
    }

    fn row(&self, path: &str) -> Row {
        let view = &self.served.view;
        let library = &self.served.library;
        let below = view.folders().tracks_below(path);
        let albums = crate::browse::albums_of(library, &below);
        let missing = self
            .missing
            .map_or(0, |wanted| self.lacking(&below, wanted));
        let here = crate::browse::tracks_in(view, path);
        let own = crate::browse::albums_of(library, &here);
        let artwork = below
            .iter()
            .filter_map(|at| library.tracks().get(*at))
            .find(|track| track.artwork.is_some())
            .map(|track| track.id.as_str().to_owned());
        let (folders, _) = crate::browse::folder_size(view, path);
        let empty = Tally::new();
        let problems = self.problems.get(path).unwrap_or(&empty);
        let tracks = below.len();
        let counted = |wanted: bool| -> usize {
            problems
                .iter()
                .filter(|(cause, _)| cause.is_problem() == wanted)
                .map(|(_, count)| count)
                .sum()
        };
        Row {
            name: name_of(path).to_owned(),
            path: path.to_owned(),
            folders,
            tracks,
            albums: albums.len(),
            artwork,
            problems: counted(true),
            notes: counted(false),
            missing,
            changed: self
                .walked
                .is_some_and(|scope| scope.covers(std::path::Path::new(path))),
            says: crate::report::holds(tracks, albums.len(), folders),
            issues: issues_of(
                problems,
                crate::report::split_by(&own, here.len(), folders),
                self.lacking(&below, Missing::Artwork),
            ),
        }
    }
}

/// What fell in each folder counting everything below it, by cause.
fn problems_below(refusals: &Refusals) -> HashMap<&str, Tally> {
    let mut below: HashMap<&str, Tally> = HashMap::new();
    for (folder, tally) in refusals.by_folder() {
        let mut at = Some(folder.as_str());
        while let Some(path) = at {
            let carried = below.entry(path).or_default();
            for (cause, count) in tally {
                *carried.entry(*cause).or_default() += count;
            }
            at = match path.rfind('/') {
                Some(cut) => Some(&path[..cut]),
                None if path.is_empty() => None,
                None => Some(""),
            };
        }
    }
    below
}

/// Neither is a refusal: nothing is declined for having no cover, and both halves of a split
/// album are served.
pub const NO_ARTWORK: &str = "no-artwork";
pub const SPLIT: &str = "split";

/// Everything wrong with a folder, faults first and the largest of each first.
fn issues_of(problems: &Tally, split: Option<&'static str>, no_artwork: usize) -> Vec<Issue> {
    let mut issues: Vec<Issue> = problems
        .iter()
        .map(|(cause, count)| Issue {
            cause: cause.as_str().to_owned(),
            label: crate::report::label(*cause),
            count: *count,
            subject: crate::report::subject(*cause, *count),
            problem: cause.is_problem(),
            opens: cause.names_a_path(),
        })
        .collect();
    issues.sort_by(|left, right| {
        right
            .problem
            .cmp(&left.problem)
            .then(right.count.cmp(&left.count))
    });
    if no_artwork > 0 {
        issues.push(Issue {
            cause: NO_ARTWORK.to_owned(),
            label: "No cover art",
            count: no_artwork,
            subject: if no_artwork == 1 { "track" } else { "tracks" },
            problem: false,
            opens: true,
        });
    }
    if let Some(split) = split {
        issues.push(Issue {
            cause: SPLIT.to_owned(),
            label: split,
            count: 0,
            subject: "",
            problem: false,
            opens: false,
        });
    }
    issues
}

/// The tracks under a folder that carry no cover, as refusals so one route answers for both, and
/// how many there are: a refusal record keeps a hundred of a kind and this answer says so too.
pub fn tracks_lacking_artwork(
    served: &Served,
    folder: &str,
) -> (Vec<crate::index::Refusal>, usize) {
    let below = served.view.folders().tracks_below(folder);
    let lacking = below
        .iter()
        .filter_map(|at| served.library.tracks().get(*at))
        .filter(|track| Missing::Artwork.absent(track));
    let mut total = 0;
    let mut shown = Vec::new();
    for track in lacking {
        total += 1;
        if shown.len() < Refusals::KEPT {
            shown.push(crate::index::Refusal {
                subject: track.relative.clone(),
                detail: None,
                folder: None,
            });
        }
    }
    (shown, total)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{Cause, Library, Scanned};

    fn track(relative: &str, album: &str) -> Scanned {
        Scanned {
            path: std::path::PathBuf::from(format!("/music/{relative}")),
            relative: std::path::PathBuf::from(relative),
            tags: crate::tags::FileTags {
                title: Some(relative.to_owned()),
                album: Some(album.to_owned()),
                ..Default::default()
            },
            properties: crate::tags::AudioProperties::default(),
            size: 27,
            artwork: None,
        }
    }

    fn served() -> Served {
        let library = Library::build(
            "Music".to_owned(),
            &[
                track("Blue Note/Sierra/01.flac", "Dundunbanza"),
                track("Blue Note/Sierra/02.flac", "Dundunbanza"),
                track("Blue Note/Kremerata/01.flac", "Eight Seasons"),
                track("Autechre/01.flac", "Amber"),
            ],
        );
        Served::new(library, crate::browse::Settings::default())
    }

    fn dated(relative: &str, album: &str, year: &str) -> Scanned {
        let mut scanned = track(relative, album);
        scanned.tags.date = Some(year.to_owned());
        scanned
    }

    fn lacking(served: &Served, wanted: Missing, under: &str) -> Vec<Row> {
        listing(
            served,
            &Refusals::default(),
            None,
            &Asked {
                under: under.to_owned(),
                missing: Some(wanted),
                ..Asked::default()
            },
        )
        .expect("the folder is in the tree")
        .folders
    }

    #[test]
    fn asking_for_a_missing_tag_keeps_the_folders_holding_it_and_counts_them() {
        let served = Served::new(
            Library::build(
                "Music".to_owned(),
                &[
                    dated("Blue Note/Sierra/01.flac", "Dundunbanza", "1994"),
                    track("Blue Note/Sierra/02.flac", "Dundunbanza"),
                    dated("Autechre/01.flac", "Amber", "1994"),
                ],
            ),
            crate::browse::Settings::default(),
        );

        let top = lacking(&served, Missing::Date, "");
        assert_eq!(top.len(), 1, "the folder whose tracks all carry one is out");
        assert_eq!(top[0].name, "Blue Note");
        assert_eq!(top[0].missing, 1);

        assert!(
            lacking(&served, Missing::Genre, "").len() > 1,
            "a tag nothing carries keeps every folder holding a track"
        );
        assert!(
            lacking(&served, Missing::Album, "").is_empty(),
            "and a tag everything carries keeps none"
        );
    }

    #[test]
    fn a_folder_holding_two_albums_says_what_tells_them_apart() {
        let served = Served::new(
            Library::build(
                "Music".to_owned(),
                &[
                    track("Split/01.flac", "One Night"),
                    track("Split/02.flac", "One Night"),
                    track("Split/03.flac", "One Nite"),
                    track("Split/04.flac", "One Nite"),
                ],
            ),
            crate::browse::Settings::default(),
        );
        let row = &rows(&served, &Refusals::default(), "")[0];
        assert_eq!(row.albums, 2, "one folder, two spellings of one title");
        assert_eq!(row.says, "4 tracks, 2 albums");
        assert!(
            row.issues
                .iter()
                .any(|one| one.label == "Album titles differ"),
            "{:?}",
            row.issues.iter().map(|one| one.label).collect::<Vec<_>>()
        );
    }

    /// A folder of singles holds as many albums as tracks and nothing in it was split.
    #[test]
    fn a_folder_of_loose_tracks_is_not_called_split() {
        let served = Served::new(
            Library::build(
                "Music".to_owned(),
                &[
                    track("Singles/01.flac", "One Night"),
                    track("Singles/02.flac", "Another Night"),
                    track("Singles/03.flac", "A Third Night"),
                ],
            ),
            crate::browse::Settings::default(),
        );
        let row = &rows(&served, &Refusals::default(), "")[0];
        assert_eq!(row.albums, 3);
        assert_eq!(
            row.says, "3 tracks, 3 albums",
            "one real folder holds 175 tracks in 110 albums and the clause was noise there"
        );
    }

    #[test]
    fn two_album_artists_under_one_title_are_one_album_and_no_split() {
        let mut ana = track("Split/01.flac", "One Night");
        ana.tags.album_artists = vec!["Ana".to_owned()];
        let mut bo = track("Split/02.flac", "One Night");
        bo.tags.album_artists = vec!["Bo".to_owned()];
        let served = Served::new(
            Library::build("Music".to_owned(), &[ana, bo]),
            crate::browse::Settings::default(),
        );
        let row = &rows(&served, &Refusals::default(), "")[0];
        assert_eq!(row.albums, 1, "the chain takes the commonest album artist");
        assert_eq!(row.says, "2 tracks, 1 album");
    }

    #[test]
    fn a_folder_whose_albums_are_all_below_it_is_not_called_split() {
        let served = served();
        let label = rows(&served, &Refusals::default(), "")
            .into_iter()
            .find(|row| row.name == "Blue Note")
            .expect("listed");
        assert_eq!(label.albums, 2, "two albums, each in a folder of its own");
        assert_eq!(label.says, "3 tracks, 2 albums", "so nothing is split");
    }

    #[test]
    fn a_row_counts_nothing_missing_where_none_was_asked_about() {
        let served = served();
        let row = &rows(&served, &Refusals::default(), "")[0];
        assert_eq!(row.missing, 0, "though every track here lacks a date");
    }

    #[test]
    fn a_filter_reaches_past_the_folders_one_listing_shows() {
        let mut files: Vec<Scanned> = (0..MOST + 1)
            .map(|at| track(&format!("{at:04}/01.flac"), "Album"))
            .collect();
        // The one folder the filter keeps sits past the cap, alphabetically last.
        files.push(track(&format!("{:04}/02.flac", MOST + 1), "Album"));
        let last = format!("{:04}", MOST + 1);
        let mut lacking = files.pop().expect("the last file");
        lacking.tags.album = None;
        files.push(lacking);
        let served = Served::new(
            Library::build("Music".to_owned(), &files),
            crate::browse::Settings::default(),
        );

        let listed = listing(
            &served,
            &Refusals::default(),
            None,
            &Asked {
                missing: Some(Missing::Album),
                ..Asked::default()
            },
        )
        .expect("the top of the library");
        assert_eq!(
            listed.folders.len(),
            1,
            "the folder past the cap is the only one that answers the filter"
        );
        assert_eq!(listed.folders[0].path, last);
        assert!(!listed.more, "and nothing else answers it");
    }

    #[test]
    fn the_tracks_lacking_a_cover_are_counted_past_the_hundred_that_are_named() {
        let files: Vec<Scanned> = (0..Refusals::KEPT + 50)
            .map(|at| track(&format!("Bare/{at:04}.flac"), "Album"))
            .collect();
        let served = Served::new(
            Library::build("Music".to_owned(), &files),
            crate::browse::Settings::default(),
        );
        let (shown, total) = tracks_lacking_artwork(&served, "Bare");
        assert_eq!(
            total,
            Refusals::KEPT + 50,
            "the page says how many carry no cover"
        );
        assert_eq!(
            shown.len(),
            Refusals::KEPT,
            "and names the hundred it keeps"
        );
    }

    fn rows(served: &Served, refusals: &Refusals, under: &str) -> Vec<Row> {
        listing(
            served,
            refusals,
            None,
            &Asked {
                under: under.to_owned(),
                ..Asked::default()
            },
        )
        .expect("the folder is in the tree")
        .folders
    }

    #[test]
    fn a_row_counts_the_tracks_and_the_albums_below_it_rather_than_in_it() {
        let served = served();
        let refusals = Refusals::default();
        let top = rows(&served, &refusals, "");
        let label = top
            .iter()
            .find(|row| row.name == "Blue Note")
            .expect("the label folder is listed");
        assert_eq!(label.tracks, 3, "two albums' worth, one folder down");
        assert_eq!(label.albums, 2);
        assert_eq!(label.folders, 2, "and it says that it opens");
        assert_eq!(label.says, "3 tracks, 2 albums");

        let inside = rows(&served, &refusals, "Blue Note");
        let album = inside
            .iter()
            .find(|row| row.name == "Sierra")
            .expect("the album folder is listed");
        assert_eq!((album.tracks, album.albums, album.folders), (2, 1, 0));
        assert_eq!(album.says, "2 tracks, 1 album");
    }

    #[test]
    fn a_folder_that_refused_something_carries_it_and_so_does_every_folder_above() {
        let served = served();
        let mut refusals = Refusals::default();
        refusals.refuse(Cause::UnreadableFile, "Blue Note/Sierra/03.flac", None);
        let label = rows(&served, &refusals, "")
            .into_iter()
            .find(|row| row.name == "Blue Note")
            .expect("listed");
        assert_eq!(label.problems, 1);
        assert_eq!(label.says, "3 tracks, 2 albums");
        assert_eq!(label.issues[0].label, "Unreadable file");
    }

    fn refused(causes: &[(Cause, usize)]) -> Tally {
        causes.iter().copied().collect()
    }

    #[test]
    fn a_folder_with_nothing_wrong_with_it_lists_nothing() {
        assert!(issues_of(&Tally::new(), None, 0).is_empty());
    }

    /// A row names every trouble a folder has, not only the worst.
    #[test]
    fn every_thing_wrong_with_a_folder_is_listed_faults_first() {
        let issues = issues_of(
            &refused(&[
                (Cause::AlbumKeyedOnPath, 2192),
                (Cause::MissingEntry, 177),
                (Cause::UnreadableFile, 6),
            ]),
            None,
            0,
        );
        let listed: Vec<(&str, usize)> = issues
            .iter()
            .map(|issue| (issue.label, issue.count))
            .collect();
        assert_eq!(
            listed,
            vec![
                ("Broken playlist link", 177),
                ("Unreadable file", 6),
                ("Identical album tags", 2192),
            ],
            "faults before what the tags decided, and the largest of each first"
        );
    }

    #[test]
    fn an_issue_says_what_its_count_counts() {
        let one = issues_of(&refused(&[(Cause::UnreadableFile, 1)]), None, 0);
        assert_eq!(one[0].subject, "file");
        let many = issues_of(&refused(&[(Cause::UnreadableFile, 3)]), None, 0);
        assert_eq!(many[0].subject, "files");
    }

    #[test]
    fn a_split_album_is_listed_with_the_rest_and_opens_onto_nothing() {
        let issues = issues_of(&Tally::new(), Some("Album titles differ"), 0);
        assert_eq!(issues.len(), 1);
        assert_eq!(issues[0].label, "Album titles differ");
        assert_eq!(issues[0].count, 0, "there is no count to give it");
        assert!(!issues[0].opens, "and no files to name behind it");
    }

    #[test]
    fn what_the_tags_decided_is_no_problem() {
        let issues = issues_of(
            &refused(&[(Cause::AlbumKeyedOnPath, 12), (Cause::UnreadableFile, 1)]),
            None,
            0,
        );
        assert!(issues[0].problem, "a file that would not read is a fault");
        assert!(!issues[1].problem, "two albums tagged alike is not");
    }

    #[test]
    fn a_problem_is_counted_in_its_folder_and_in_every_folder_above_it() {
        let mut refusals = Refusals::default();
        refusals.refuse(Cause::UnreadableFile, "Bach/Cantatas/1.flac", None);
        refusals.refuse(Cause::UnreadableFile, "Bach/Cantatas/2.flac", None);
        refusals.refuse(Cause::UnreadableFolder, "Bach/Sonatas", None);
        refusals.refuse_in(
            Cause::AlbumKeyedOnPath,
            "An Album Title",
            None,
            "Bach/Sonatas",
        );

        let below = problems_below(&refusals);
        let total =
            |path: &str| -> usize { below.get(path).map(|t| t.values().sum()).unwrap_or(0) };
        assert_eq!(total("Bach/Cantatas"), 2);
        assert_eq!(total("Bach/Sonatas"), 2);
        assert_eq!(total("Bach"), 4);
        assert_eq!(total(""), 4, "and the root counts every one of them");
        assert_eq!(
            below["Bach/Sonatas"][&Cause::AlbumKeyedOnPath],
            1,
            "an album keyed on its path names a title, and sits where its tracks sit"
        );
    }
}
