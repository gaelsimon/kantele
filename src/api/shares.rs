//! The bounded listing the folder and icon choosers walk: the shares and what is under them,
//! never the filesystem.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::index::Roots;

/// Entries one answer carries. High, because the chooser has no search to narrow a listing with
/// and a music volume holds a couple of thousand album folders side by side.
const MOST: usize = 2000;

#[derive(Debug, Default, Deserialize)]
pub struct Asked {
    /// The folder to list, empty for the shares themselves.
    #[serde(default)]
    pub under: String,
    /// Whether pictures are listed beside the folders.
    #[serde(default)]
    pub images: bool,
}

#[derive(Debug, Serialize)]
pub struct Listing {
    pub under: String,
    /// The folder above, or nothing at a share, which is as far up as this goes.
    pub above: Option<String>,
    pub entries: Vec<Entry>,
    /// Whether entries were left out because one answer is bounded.
    pub more: bool,
}

#[derive(Debug, Serialize)]
pub struct Entry {
    pub path: String,
    pub name: String,
    pub folder: bool,
}

/// Why a listing was refused, which is either where it pointed or what the disk said.
#[derive(Debug)]
pub enum Refused {
    Outside,
    /// The folder is there and this server may not read it, which is a permission to grant.
    Denied(String),
    Unreadable(String),
}

fn refused(path: &str, error: &std::io::Error) -> Refused {
    if error.kind() == std::io::ErrorKind::PermissionDenied {
        Refused::Denied(path.to_owned())
    } else {
        Refused::Unreadable(format!("{path}: {error}"))
    }
}

/// The folders a listing may start from and may never climb above: the shares, and whatever this
/// server is already serving.
pub fn shares(serving: &Roots) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = mounted();
    found.extend(std::env::var_os("HOME").map(PathBuf::from));
    among(found, serving)
}

fn among(found: Vec<PathBuf>, serving: &Roots) -> Vec<PathBuf> {
    // Resolved, because every path asked for is, and a share that is not would match none of them.
    let mut found: Vec<PathBuf> = found
        .into_iter()
        .map(|share| share.canonicalize().unwrap_or(share))
        .collect();
    for root in serving.paths() {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        if !found.iter().any(|share| root.starts_with(share)) {
            found.push(root);
        }
    }
    found.sort();
    found.dedup();
    found
}

/// The volumes of the box, and the folders each platform mounts a disk under, which is where music
/// that is not in the home folder lives. The boot volume is left out: it is the whole filesystem
/// under another name, and this listing is not a way to read the filesystem.
fn mounted() -> Vec<PathBuf> {
    let mut found = volumes();
    for under in ["/Volumes", "/mnt", "/media"] {
        let Ok(entries) = std::fs::read_dir(under) else {
            continue;
        };
        found.extend(
            entries
                .filter_map(Result::ok)
                .map(|entry| entry.path())
                .filter(|path| path.is_dir())
                .filter(|path| {
                    path.canonicalize()
                        .is_ok_and(|resolved| resolved != Path::new("/"))
                }),
        );
    }
    found
}

/// `/volume1`, `/volume2` and any other the box was built with.
fn volumes() -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir("/") else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| is_volume(path) && path.is_dir())
        .collect()
}

fn is_volume(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("volume"))
        .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|digit| digit.is_ascii_digit()))
}

/// One level of a share, or the shares themselves where nothing under them was named.
pub fn listing(serving: &Roots, asked: &Asked) -> Result<Listing, Refused> {
    let shares = shares(serving);
    if asked.under.is_empty() {
        return Ok(Listing {
            under: String::new(),
            above: None,
            entries: shares.iter().map(|share| named(share, true)).collect(),
            more: false,
        });
    }
    // Resolved against the disk first, so neither `..` nor a symlink reaches outside a share.
    let under = Path::new(&asked.under)
        .canonicalize()
        .map_err(|error| refused(&asked.under, &error))?;
    let Some(share) = shares.iter().find(|share| under.starts_with(share)) else {
        return Err(Refused::Outside);
    };
    let mut entries =
        read(&under, asked.images).map_err(|error| refused(&under.to_string_lossy(), &error))?;
    entries.sort_by(|left, right| {
        (!left.folder, crate::index::fold(&left.name))
            .cmp(&(!right.folder, crate::index::fold(&right.name)))
    });
    let more = entries.len() > MOST;
    entries.truncate(MOST);
    Ok(Listing {
        above: (under != *share).then(|| {
            under
                .parent()
                .unwrap_or(share)
                .to_string_lossy()
                .into_owned()
        }),
        under: under.to_string_lossy().into_owned(),
        entries,
        more,
    })
}

fn read(under: &Path, images: bool) -> std::io::Result<Vec<Entry>> {
    let mut entries = Vec::new();
    for entry in std::fs::read_dir(under)?.filter_map(Result::ok) {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if crate::index::is_skipped(name) {
            continue;
        }
        // The kind comes with the directory entry; asking the path costs a stat each time, so it
        // is kept for the links, whose target is what counts.
        let folder = match entry.file_type() {
            Ok(kind) if kind.is_symlink() => path.is_dir(),
            Ok(kind) => kind.is_dir(),
            Err(_) => path.is_dir(),
        };
        if folder || (images && crate::index::is_image(name)) {
            entries.push(named(&path, folder));
        }
    }
    Ok(entries)
}

fn named(path: &Path, folder: bool) -> Entry {
    Entry {
        name: path
            .file_name()
            .unwrap_or(path.as_os_str())
            .to_string_lossy()
            .into_owned(),
        path: path.to_string_lossy().into_owned(),
        folder,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_home_folder_is_offered_beside_whatever_is_mounted() {
        let offered = shares(&Roots::one("/srv/music"));
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .map(|home| home.canonicalize().unwrap_or(home));
        if let Some(home) = home {
            assert!(
                offered.contains(&home),
                "the home folder is where music is: {offered:?}"
            );
        }
        assert!(
            !offered.iter().any(|share| share == Path::new("/")),
            "the whole disk is never a share: {offered:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_share_reached_through_a_link_holds_the_folders_under_it() {
        let dir = std::env::temp_dir().join("kantele-shares-linked");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("disk/music")).expect("a folder");
        std::os::unix::fs::symlink(dir.join("disk"), dir.join("mounted")).expect("a link");

        let shares = among(vec![dir.join("mounted")], &Roots::default());
        let asked = dir
            .join("mounted/music")
            .canonicalize()
            .expect("it is there");
        assert!(
            shares.iter().any(|share| asked.starts_with(share)),
            "a listing resolves what it is asked, so a share that is not resolved holds nothing: \
             {shares:?} against {asked:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_share_is_a_volume_and_nothing_else_at_the_top_of_the_disk() {
        assert!(is_volume(Path::new("/volume1")));
        assert!(is_volume(Path::new("/volume12")));
        assert!(!is_volume(Path::new("/volume")));
        assert!(!is_volume(Path::new("/volumes")));
        assert!(!is_volume(Path::new("/etc")));
    }

    #[test]
    fn the_folder_being_served_is_offered_even_where_it_is_under_no_share() {
        let serving = Roots::one("/srv/music");
        assert!(
            shares(&serving)
                .iter()
                .any(|share| share == Path::new("/srv/music")),
            "an owner has to be able to see the folder the server is on"
        );
    }

    #[test]
    fn a_path_outside_every_share_is_refused_rather_than_listed() {
        let dir = std::env::temp_dir().join("kantele-shares-outside");
        std::fs::create_dir_all(dir.join("inside")).expect("a folder");
        let serving = Roots::one(&dir);
        let listed = listing(
            &serving,
            &Asked {
                under: dir.join("inside").display().to_string(),
                images: false,
            },
        );
        assert!(listed.is_ok(), "a folder under the share reads");

        // The folder holding the share: outside it on every platform, and it is certainly there.
        let refused = listing(
            &serving,
            &Asked {
                under: std::env::temp_dir().display().to_string(),
                images: false,
            },
        );
        assert!(
            matches!(refused, Err(Refused::Outside)),
            "the page has no password, so it does not read the disk"
        );

        let climbed = listing(
            &serving,
            &Asked {
                under: dir.join("inside/../../..").display().to_string(),
                images: false,
            },
        );
        assert!(
            matches!(climbed, Err(Refused::Outside)),
            "and it cannot walk up out of one"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
