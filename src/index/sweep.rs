//! Finding changes by comparing a walk with the store's rows.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use crate::index::roots::Roots;
use crate::index::scan::{Fingerprint, Found, Walked};

#[derive(Debug, Default)]
pub struct Remembered {
    pub files: HashMap<PathBuf, Fingerprint>,
    pub covers: HashMap<PathBuf, Fingerprint>,
    pub playlists: HashMap<PathBuf, Fingerprint>,
}

/// A missing file counts as changed only when every folder could be read.
pub fn differences(
    roots: impl Into<Roots>,
    walked: &Walked,
    remembered: &Remembered,
) -> Vec<PathBuf> {
    let roots = &roots.into();
    let complete = walked.complete();
    let mut changed = differing(roots, walked.files.iter(), &remembered.files, complete);
    changed.extend(differing(
        roots,
        walked.covers.values(),
        &remembered.covers,
        complete,
    ));
    changed.extend(differing(
        roots,
        walked.playlists.iter(),
        &remembered.playlists,
        complete,
    ));
    changed.sort();
    changed.dedup();
    changed
}

fn differing<'a>(
    roots: &Roots,
    found: impl Iterator<Item = &'a Found>,
    remembered: &HashMap<PathBuf, Fingerprint>,
    complete: bool,
) -> Vec<PathBuf> {
    let mut changed = Vec::new();
    let mut walked: HashSet<PathBuf> = HashSet::new();
    for item in found {
        let Some(relative) = roots.relative(&item.path) else {
            continue;
        };
        // An unknown fingerprint is not a change.
        if item.fingerprint != Fingerprint::UNKNOWN
            && remembered.get(&relative) != Some(&item.fingerprint)
        {
            changed.push(item.path.clone());
        }
        walked.insert(relative);
    }
    if complete {
        for relative in remembered.keys() {
            if !walked.contains(relative)
                && let Some(path) = roots.absolute(relative)
            {
                changed.push(path);
            }
        }
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const ROOT: &str = "/music";

    fn found(relative: &str, size: u64) -> Found {
        Found {
            path: Path::new(ROOT).join(relative),
            fingerprint: Fingerprint { size, mtime: 1 },
        }
    }

    fn rows(files: &[(&str, u64)]) -> HashMap<PathBuf, Fingerprint> {
        files
            .iter()
            .map(|(relative, size)| {
                (
                    PathBuf::from(relative),
                    Fingerprint {
                        size: *size,
                        mtime: 1,
                    },
                )
            })
            .collect()
    }

    fn walked(files: Vec<Found>) -> Walked {
        Walked {
            files,
            ..Walked::default()
        }
    }

    fn remembered(files: &[(&str, u64)]) -> Remembered {
        Remembered {
            files: rows(files),
            ..Remembered::default()
        }
    }

    #[test]
    fn a_tree_that_matches_the_rows_has_no_differences() {
        let tree = walked(vec![found("a/1.flac", 10), found("a/2.flac", 20)]);
        let known = remembered(&[("a/1.flac", 10), ("a/2.flac", 20)]);
        assert!(differences(Path::new(ROOT), &tree, &known).is_empty());
    }

    #[test]
    fn a_new_file_a_changed_one_and_a_gone_one_are_each_a_difference() {
        let tree = walked(vec![
            found("a/1.flac", 10),
            found("a/2.flac", 21),
            found("b/1.flac", 5),
        ]);
        let known = remembered(&[("a/1.flac", 10), ("a/2.flac", 20), ("c/1.flac", 7)]);
        assert_eq!(
            differences(Path::new(ROOT), &tree, &known),
            vec![
                PathBuf::from("/music/a/2.flac"),
                PathBuf::from("/music/b/1.flac"),
                PathBuf::from("/music/c/1.flac"),
            ]
        );
    }

    #[test]
    fn a_walk_that_could_not_read_every_folder_infers_no_deletion() {
        let tree = Walked {
            unreadable: 1,
            ..walked(vec![found("a/1.flac", 10)])
        };
        let known = remembered(&[("a/1.flac", 10), ("c/1.flac", 7)]);
        assert!(
            differences(Path::new(ROOT), &tree, &known).is_empty(),
            "a folder that would not open is a hole, and its rows are not gone"
        );
    }

    #[test]
    fn a_file_whose_metadata_could_not_be_read_is_not_a_difference() {
        let tree = walked(vec![Found {
            path: Path::new(ROOT).join("a/1.flac"),
            fingerprint: Fingerprint::UNKNOWN,
        }]);
        let known = remembered(&[("a/1.flac", 10)]);
        assert!(differences(Path::new(ROOT), &tree, &known).is_empty());
    }

    #[test]
    fn a_cover_and_a_playlist_are_compared_like_a_track() {
        let mut tree = walked(vec![found("a/1.flac", 10)]);
        tree.covers
            .insert(Path::new(ROOT).join("a"), found("a/cover.jpg", 300));
        tree.playlists.push(found("a/play.m3u", 40));
        let known = Remembered {
            files: rows(&[("a/1.flac", 10)]),
            covers: rows(&[("a/cover.jpg", 299)]),
            playlists: rows(&[("a/play.m3u", 40), ("b/play.m3u", 41)]),
        };
        assert_eq!(
            differences(Path::new(ROOT), &tree, &known),
            vec![
                PathBuf::from("/music/a/cover.jpg"),
                PathBuf::from("/music/b/play.m3u"),
            ]
        );
    }
}
