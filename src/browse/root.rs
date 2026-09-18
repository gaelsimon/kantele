//! The root a device meets: what the library holds, and the menus the owner chose, in wire order.

use crate::index::Library;

use super::{Facet, Menu, Position, View, folder_size, menu, untagged_count};

/// The identifier each fixed entry is published under.
pub const MUSIC: &str = "music";
pub const ALBUMS: &str = "albums";
pub const UNTAGGED: &str = "untagged";
pub const PLAYLISTS: &str = "playlists";
pub const FOLDERS: &str = "folders";
pub const RECENT: &str = "recent";

pub const UNTAGGED_TITLE: &str = "[untagged]";
pub const PLAYLISTS_TITLE: &str = "Playlists";
pub const FOLDERS_TITLE: &str = "[folder view]";
pub const RECENT_TITLE: &str = "Recently added";

/// What one root entry opens onto. The caller mints the identifier it publishes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Opens {
    Albums,
    Music,
    /// One axis the owner chose, with the position that lists its values.
    Axis(Facet, Position),
    Untagged,
    Playlists,
    Recent,
    Folders,
}

impl Opens {
    /// The identifier a device browses to reach it.
    pub fn id(&self) -> String {
        match self {
            Self::Albums => ALBUMS.to_owned(),
            Self::Music => MUSIC.to_owned(),
            Self::Axis(_, at) => at.id(),
            Self::Untagged => UNTAGGED.to_owned(),
            Self::Playlists => PLAYLISTS.to_owned(),
            Self::Recent => RECENT.to_owned(),
            Self::Folders => FOLDERS.to_owned(),
        }
    }
}

/// One entry of the root.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub opens: Opens,
    pub title: String,
    pub children: usize,
}

pub fn counted(count: usize, noun: &str) -> String {
    let plural = if count == 1 { "" } else { "s" };
    format!("{count} {noun}{plural}")
}

/// The root, in the order a device reads it. An entry with nothing behind it is not offered.
pub fn entries(library: &Library, view: &View) -> Vec<Entry> {
    let mut entries = Vec::new();
    let mut offer = |opens, title: String, children| {
        entries.push(Entry {
            opens,
            title,
            children,
        });
    };

    let albums = library.albums().len();
    if albums > 0 {
        offer(Opens::Albums, counted(albums, "album"), albums);
    }
    offer(Opens::Music, counted(library.len(), "item"), library.len());

    if let Some(Menu::Facets(offered)) = menu(library, view, &Position::default()) {
        for (facet, at, values) in offered {
            offer(Opens::Axis(facet, at), facet.title().to_owned(), values);
        }
    }

    let untagged = untagged_count(library, view);
    if untagged > 0 {
        offer(Opens::Untagged, UNTAGGED_TITLE.to_owned(), untagged);
    }
    let playlists = library.playlists().len();
    if playlists > 0 {
        offer(Opens::Playlists, PLAYLISTS_TITLE.to_owned(), playlists);
    }
    let recent = view.recent().len();
    if recent > 0 {
        offer(Opens::Recent, RECENT_TITLE.to_owned(), recent);
    }
    let (folders, loose) = folder_size(view, "");
    if folders + loose > 0 {
        offer(Opens::Folders, FOLDERS_TITLE.to_owned(), folders + loose);
    }
    entries
}
