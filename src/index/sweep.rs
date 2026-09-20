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

/// A missing file counts as changed only where the folder it sat in could be read.
pub fn differences(
    roots: impl Into<Roots>,
    walked: &Walked,
    remembered: &Remembered,
) -> Vec<PathBuf> {
    let roots = &roots.into();
    let mut changed = differing(roots, walked.files.iter(), &remembered.files, walked);
    changed.extend(differing(
        roots,
        walked.covers.values(),
        &remembered.covers,
        walked,
    ));
    changed.extend(differing(
        roots,
        walked.playlists.iter(),
        &remembered.playlists,
        walked,
    ));
    changed.sort();
    changed.dedup();
    changed
}

fn differing<'a>(
    roots: &Roots,
    found: impl Iterator<Item = &'a Found>,
    remembered: &HashMap<PathBuf, Fingerprint>,
    walked: &Walked,
) -> Vec<PathBuf> {
    let mut changed = Vec::new();
    let mut found_here: HashSet<PathBuf> = HashSet::new();
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
        found_here.insert(relative);
    }
    for relative in remembered.keys() {
        if !found_here.contains(relative)
            && walked.reached(relative)
            && let Some(path) = roots.absolute(relative)
        {
            changed.push(path);
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
    fn a_folder_that_would_not_open_is_a_hole_only_under_itself() {
        let tree = Walked {
            unreadable: vec!["c".to_owned()],
            ..walked(vec![found("a/1.flac", 10)])
        };
        let known = remembered(&[("a/1.flac", 10), ("b/1.flac", 7), ("c/1.flac", 7)]);
        assert_eq!(
            differences(Path::new(ROOT), &tree, &known),
            vec![PathBuf::from("/music/b/1.flac")],
            "the rows under the folder that would not open say nothing, and the rest of the tree \
             is still walked"
        );
    }

    #[test]
    fn a_walk_that_stopped_infers_no_deletion_anywhere() {
        let tree = Walked {
            stopped: true,
            ..walked(vec![found("a/1.flac", 10)])
        };
        let known = remembered(&[("a/1.flac", 10), ("c/1.flac", 7)]);
        assert!(
            differences(Path::new(ROOT), &tree, &known).is_empty(),
            "a pass that stopped part way has not been anywhere it did not reach"
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
