//! The folder view, kept beside the tag view because tagging fails.

use std::collections::{HashMap, HashSet};

use super::View;
use crate::index::{Album, Library, Track, fold};
use crate::object::ObjectId;

use super::digest_of;

#[derive(Clone, Debug, Default)]
pub struct Folders {
    /// Every folder holding a track or standing above one, the content root first.
    nodes: Vec<Node>,
    by_path: HashMap<String, u32>,
    /// What a client sends back, to what it names.
    by_digest: HashMap<String, u32>,
}

/// One folder: where it is, what is under it, and what is in it.
#[derive(Clone, Debug, Default)]
struct Node {
    /// The path relative to the content root, empty for the root itself.
    path: String,
    /// Where the last component of that path starts, so the name needs no second string.
    name_at: usize,
    /// The folders directly under it, in the order a listing shows them.
    children: Vec<u32>,
    /// The tracks directly in it, as indices into [`Library::tracks`], in the library's order.
    tracks: Vec<u32>,
}

impl Node {
    fn name(&self) -> &str {
        &self.path[self.name_at..]
    }

    fn entries(&self) -> usize {
        self.children.len() + self.tracks.len()
    }

    fn own_tracks(&self) -> Vec<usize> {
        self.tracks.iter().map(|at| *at as usize).collect()
    }
}

impl Folders {
    /// Builds the tree from the paths the tracks carry.
    pub fn build(tracks: &[Track]) -> Self {
        let mut folders = Self::default();
        folders.ensure("");
        for (at, track) in tracks.iter().enumerate() {
            let holding = track
                .relative
                .rfind('/')
                .map_or("", |cut| &track.relative[..cut]);
            let id = folders.ensure(holding);
            folders.nodes[id as usize].tracks.push(at as u32);
        }
        for at in 0..folders.nodes.len() {
            let mut children = std::mem::take(&mut folders.nodes[at].children);
            children.sort_by_key(|child| fold(folders.nodes[*child as usize].name()));
            folders.nodes[at].children = children;
        }
        folders.by_digest = folders
            .nodes
            .iter()
            .enumerate()
            .map(|(at, node)| (path_digest(&node.path), at as u32))
            .collect();
        folders
    }

    /// Roughly what the tree holds, in bytes, for the memory report.
    pub fn footprint(&self) -> usize {
        let nodes: usize = self
            .nodes
            .iter()
            .map(|node| node.path.len() + node.children.len() * 4 + node.tracks.len() * 4 + 88)
            .sum();
        let keys: usize = self
            .by_path
            .keys()
            .chain(self.by_digest.keys())
            .map(|key| key.len() + 24 + 16)
            .sum();
        nodes + keys
    }

    /// How many folders the tree holds, the content root included.
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    /// Every folder in the tree, by its path relative to the content root.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.nodes.iter().map(|node| node.path.as_str())
    }

    pub fn holds(&self, path: &str) -> bool {
        self.by_path.contains_key(path)
    }

    /// The tracks in this folder and every folder below it, in the library's order.
    pub fn tracks_below(&self, path: &str) -> Vec<usize> {
        self.by_path
            .get(path)
            .map(|at| self.below(*at))
            .unwrap_or_default()
    }

    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// The folder at this path, making it and every folder above it if they are new.
    fn ensure(&mut self, path: &str) -> u32 {
        if let Some(id) = self.by_path.get(path) {
            return *id;
        }
        let id = self.nodes.len() as u32;
        let name_at = path.rfind('/').map_or(0, |cut| cut + 1);
        self.nodes.push(Node {
            path: path.to_owned(),
            name_at,
            ..Node::default()
        });
        self.by_path.insert(path.to_owned(), id);
        if !path.is_empty() {
            let above = self.ensure(&path[..name_at.saturating_sub(1)]);
            self.nodes[above as usize].children.push(id);
        }
        id
    }

    fn at(&self, path: &str) -> Option<&Node> {
        self.nodes.get(*self.by_path.get(path)? as usize)
    }

    /// The tracks in a folder and every folder below it, in the order the library holds them.
    fn below(&self, at: u32) -> Vec<usize> {
        let mut tracks = Vec::new();
        let mut waiting = vec![at];
        while let Some(node) = waiting.pop() {
            let Some(node) = self.nodes.get(node as usize) else {
                continue;
            };
            tracks.extend(node.tracks.iter().map(|at| *at as usize));
            waiting.extend(&node.children);
        }
        tracks.sort_unstable();
        tracks
    }
}

/// What one folder holds: the folders directly under it, and the tracks directly in it.
pub fn folder(view: &View, path: &str) -> (Vec<Subfolder>, Vec<usize>) {
    let folders = view.folders();
    let Some(node) = folders.at(path) else {
        return (Vec::new(), Vec::new());
    };
    let below: Vec<Subfolder> = node
        .children
        .iter()
        .filter_map(|child| folders.nodes.get(*child as usize))
        .map(|child| Subfolder {
            path: child.path.clone(),
            name: child.name().to_owned(),
            children: child.entries(),
        })
        .collect();
    (below, node.own_tracks())
}

/// The tracks directly in a folder, as against everything below it.
pub fn tracks_in(view: &View, path: &str) -> Vec<usize> {
    view.folders()
        .at(path)
        .map_or_else(Vec::new, Node::own_tracks)
}

/// The albums a set of tracks belongs to, in the order the tracks name them. A folder is not an
/// album, so what one holds is asked of the library rather than read off the tree.
pub fn albums_of<'a>(library: &'a Library, tracks: &[usize]) -> Vec<&'a Album> {
    let mut seen: HashSet<&ObjectId> = HashSet::new();
    tracks
        .iter()
        .filter_map(|at| library.tracks().get(*at))
        .filter_map(|track| track.album_id.as_ref())
        .filter(|id| seen.insert(id))
        .filter_map(|id| library.album(id))
        .collect()
}

/// What a folder holds, counted without building the listing a menu does not show.
pub fn folder_size(view: &View, path: &str) -> (usize, usize) {
    view.folders()
        .at(path)
        .map_or((0, 0), |node| (node.children.len(), node.tracks.len()))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subfolder {
    pub path: String,
    pub name: String,
    pub children: usize,
}

/// The display name of a folder: its last component.
pub fn folder_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// The identifier of a folder, and the path it names.
pub fn folder_id(path: &str) -> String {
    format!("{FOLDER_PREFIX}{}", path_digest(path))
}

/// The digest an identifier carries for a folder.
pub(super) fn path_digest(path: &str) -> String {
    digest_of(path.as_bytes())
}

/// The prefix of a folder identifier.
pub const FOLDER_PREFIX: &str = "fv";

/// The folder an identifier names.
pub fn folder_from_id(view: &View, id: &str) -> Option<String> {
    folder_named(view, id.strip_prefix(FOLDER_PREFIX)?).filter(|path| !path.is_empty())
}

/// The folder whose path digests to this, or nothing if the library holds no such folder.
pub(super) fn folder_named(view: &View, wanted: &str) -> Option<String> {
    let folders = view.folders();
    let at = folders.by_digest.get(wanted)?;
    folders
        .nodes
        .get(*at as usize)
        .map(|node| node.path.clone())
}

/// The tracks a folder scope selects, or nothing where it names no folder.
pub(super) fn scoped(view: &View, wanted: &str) -> Option<Vec<usize>> {
    view.folders()
        .by_digest
        .get(wanted)
        .map(|at| view.folders().below(*at))
}

#[cfg(test)]
pub(super) fn inside(track: &Track, folder: &str) -> bool {
    folder.is_empty() || track.relative.starts_with(&format!("{folder}/"))
}
