//! What an object identifier names, and what that object holds.

use crate::browse::{self, View};
use crate::index::{Album, Artist, Artwork, Library, Playlist, Track};
use crate::upnp::{ObjectId, didl};

use super::paging::{Listing, Window};
use super::parts;
use super::{
    ALBUMS, ARTISTS, FOLDERS, FOLDERS_TITLE, MUSIC, Menus, PLAYLISTS, PLAYLISTS_TITLE, RECENT,
    RECENT_TITLE, TAG_VIEW_TITLE, UNTAGGED, UNTAGGED_TITLE,
};

enum Named {
    Root,
    Albums,
    Artists,
    Music,
    Untagged,
    Playlists,
    Folders,
    Recent,
    Folder(String),
    Position(browse::Position),
    /// A disc or a run inside an album, which only the library can say exists.
    Part(String),
    Minted(ObjectId),
    Unknown,
}

fn named(view: &View, id: &str) -> Named {
    match id {
        ObjectId::ROOT => Named::Root,
        ALBUMS => Named::Albums,
        ARTISTS => Named::Artists,
        MUSIC => Named::Music,
        UNTAGGED => Named::Untagged,
        PLAYLISTS => Named::Playlists,
        FOLDERS => Named::Folders,
        RECENT => Named::Recent,
        other => {
            if let Some(path) = browse::folder_from_id(view, other) {
                return Named::Folder(path);
            }
            if let Some(position) = browse::Position::parse(other) {
                return Named::Position(position);
            }
            if crate::index::identity::part_album(other).is_some() {
                return Named::Part(other.to_owned());
            }
            match ObjectId::new(other.to_owned()) {
                Ok(object) => Named::Minted(object),
                Err(_) => Named::Unknown,
            }
        }
    }
}

pub(super) fn children<'a>(
    library: &'a Library,
    view: &'a View,
    menus: &'a Menus,
    id: &str,
    window: Window,
) -> Option<Listing<'a>> {
    let all = |children| Some(Listing::All(children));
    let flat = |total: usize, child: &dyn Fn(usize) -> didl::Child<'a>| {
        let page = window.range(total).map(child).collect();
        Some(Listing::Page(page, total))
    };
    match named(view, id) {
        Named::Root => all(root_children(library, view, menus)),
        Named::Albums => flat(library.albums().len(), &|at| {
            album_child(&library.albums()[at], &menus.albums)
        }),
        Named::Artists => flat(library.artists().len(), &|at| {
            artist_child(&library.artists()[at], &menus.artists)
        }),
        Named::Music => flat(library.tracks().len(), &|at| {
            didl::Child::Item(&library.tracks()[at], &menus.music)
        }),
        Named::Untagged => all(items_at(
            library,
            &browse::untagged(library, view),
            &menus.untagged,
        )),
        Named::Playlists => flat(library.playlists().len(), &|at| {
            playlist_child(&library.playlists()[at], &menus.playlists)
        }),
        Named::Folders => all(folder_children(library, view, menus, "", &menus.folders)),
        Named::Recent => all(recent_children(library, view, menus)),
        Named::Folder(path) => {
            let here = folder_object(&path)?;
            all(folder_children(library, view, menus, &path, &here))
        }
        Named::Position(position) => {
            let here = position_object(&position)?;
            let menu = browse::menu(library, view, &position)?;
            Some(facet_children(
                library, view, menus, &position, &here, menu, window,
            ))
        }
        Named::Part(id) => all(parts::children(library, &parts::find(library, &id)?)),
        Named::Minted(object) => minted_children(library, &object).map(Listing::All),
        Named::Unknown => None,
    }
}

fn minted_children<'a>(library: &'a Library, object: &ObjectId) -> Option<Vec<didl::Child<'a>>> {
    if let Some(album) = library.album(object) {
        return Some(parts::album_children(library, album));
    }
    if let Some(artist) = library.artist(object) {
        return Some(artist_children(library, artist));
    }
    if let Some(playlist) = library.playlist(object) {
        return Some(
            library
                .playlist_tracks(playlist)
                .map(|track| didl::Child::Item(track, &playlist.id))
                .collect(),
        );
    }
    // An item has no children: an empty list, not a fault.
    library.get(object).map(|_| Vec::new())
}

fn items_at<'a>(
    library: &'a Library,
    selected: &[usize],
    parent: &'a ObjectId,
) -> Vec<didl::Child<'a>> {
    selected
        .iter()
        .filter_map(|at| library.tracks().get(*at))
        .map(|track| didl::Child::Item(track, parent))
        .collect()
}

fn folder_object(path: &str) -> Option<ObjectId> {
    ObjectId::new(browse::folder_id(path)).ok()
}

fn position_object(position: &browse::Position) -> Option<ObjectId> {
    ObjectId::new(position.id()).ok()
}

fn root_children<'a>(
    library: &'a Library,
    view: &'a View,
    menus: &'a Menus,
) -> Vec<didl::Child<'a>> {
    browse::root::entries(library, view)
        .into_iter()
        .map(|entry| {
            let id = match &entry.opens {
                browse::root::Opens::Albums => menus.albums.clone(),
                browse::root::Opens::Music => menus.music.clone(),
                browse::root::Opens::Axis(_, at) => {
                    ObjectId::new(at.id()).expect("a position identifier is inside the alphabet")
                }
                browse::root::Opens::Untagged => menus.untagged.clone(),
                browse::root::Opens::Playlists => menus.playlists.clone(),
                browse::root::Opens::Recent => menus.recent.clone(),
                browse::root::Opens::Folders => menus.folders.clone(),
            };
            let spec = match entry.opens {
                browse::root::Opens::Folders => didl::ContainerSpec::folder,
                _ => didl::ContainerSpec::menu,
            };
            didl::Child::Container(spec(id, menus.root.clone(), entry.title, entry.children))
        })
        .collect()
}

fn folder_children<'a>(
    library: &'a Library,
    view: &'a View,
    menus: &'a Menus,
    path: &str,
    here: &ObjectId,
) -> Vec<didl::Child<'a>> {
    let (folders, tracks) = browse::folder(view, path);
    let mut children: Vec<didl::Child<'a>> = folders
        .iter()
        .map(|child| {
            didl::Child::Container(didl::ContainerSpec::folder(
                ObjectId::new(browse::folder_id(&child.path))
                    .expect("a digest behind an ascii prefix"),
                here.clone(),
                child.name.clone(),
                child.children,
            ))
        })
        .collect();

    // The root menu already offers these axes.
    let scoped = browse::Position::inside(path);
    if !path.is_empty()
        && let Some(browse::Menu::Facets(axes)) = browse::menu(library, view, &scoped)
    {
        children.push(didl::Child::Container(didl::ContainerSpec::menu(
            ObjectId::new(scoped.id()).expect("a position identifier is inside the alphabet"),
            here.clone(),
            TAG_VIEW_TITLE,
            axes.len(),
        )));
    }

    children.extend(
        tracks
            .into_iter()
            .filter_map(|at| library.tracks().get(at))
            .map(|track| didl::Child::Item(track, &menus.music)),
    );
    children
}

/// The albums the newest files belong to, newest first, then the loose files among them.
fn recent_children<'a>(
    library: &'a Library,
    view: &'a View,
    menus: &'a Menus,
) -> Vec<didl::Child<'a>> {
    view.recent()
        .iter()
        .filter_map(|entry| match *entry {
            browse::Recent::Album(at) => library
                .albums()
                .get(at)
                .map(|album| album_child(album, &menus.recent)),
            browse::Recent::Track(at) => library
                .tracks()
                .get(at)
                .map(|track| didl::Child::Item(track, &menus.recent)),
        })
        .collect()
}

fn facet_children<'a>(
    library: &'a Library,
    view: &'a View,
    menus: &'a Menus,
    position: &browse::Position,
    parent: &ObjectId,
    menu: browse::Menu<'_>,
    window: Window,
) -> Listing<'a> {
    let container = |id: String, title: String, children: usize, class: &'static str| {
        let id = ObjectId::new(id).expect("a position identifier is inside the alphabet");
        didl::Child::Container(didl::ContainerSpec {
            class,
            ..didl::ContainerSpec::menu(id, parent.clone(), title, children)
        })
    };

    match menu {
        browse::Menu::Facets(offered) => Listing::All(
            offered
                .into_iter()
                .map(|(facet, at, values)| {
                    container(at.id(), facet.title().to_owned(), values, didl::MENU)
                })
                .collect(),
        ),
        browse::Menu::Letters(_, groups) => Listing::All(
            groups
                .into_iter()
                .map(|group| {
                    container(
                        position.at_letter(group.key).id(),
                        group.display,
                        group.values,
                        didl::MENU,
                    )
                })
                .collect(),
        ),
        // The index entry is the first child, so it counts in the page arithmetic.
        browse::Menu::Values(facet, values) => {
            let letters = browse::index_offered(position, &values, &view.settings);
            let ahead = usize::from(letters.is_some());
            let total = values.len() + ahead;
            let range = window.range(total);
            let mut children = Vec::with_capacity(range.len());
            if let Some(letters) = letters
                && range.start == 0
                && !range.is_empty()
            {
                children.push(container(
                    position.at_index().id(),
                    browse::INDEX_TITLE.to_owned(),
                    letters,
                    didl::MENU,
                ));
            }
            let from = range.start.saturating_sub(ahead);
            let to = range.end.saturating_sub(ahead);
            children.extend(values[from..to].iter().map(|entry| {
                container(
                    position.chose(facet, entry.digest).id(),
                    entry.display.to_owned(),
                    entry.tracks,
                    facet.class(),
                )
            }));
            Listing::Page(children, total)
        }
        browse::Menu::Tracks(selected) => {
            let albums = browse::albums_in(library, &selected)
                .into_iter()
                .filter_map(|at| library.albums().get(at))
                .map(|album| album_child(album, parent));
            let loose = items_at(
                library,
                &browse::loose_tracks(library, &selected),
                &menus.music,
            );
            Listing::All(albums.chain(loose).collect())
        }
    }
}

fn artist_children<'a>(library: &'a Library, artist: &'a Artist) -> Vec<didl::Child<'a>> {
    let albums = artist
        .albums
        .iter()
        .filter_map(|at| library.albums().get(*at))
        .map(|album| album_child(album, &artist.id));
    let tracks = artist
        .tracks
        .iter()
        .filter_map(|at| library.tracks().get(*at))
        .map(|track| didl::Child::Item(track, &artist.id));
    albums.chain(tracks).collect()
}

pub(super) fn metadata<'a>(
    library: &'a Library,
    view: &'a View,
    menus: &'a Menus,
    id: &str,
) -> Option<didl::Child<'a>> {
    let menu = |spec| Some(didl::Child::Container(spec));
    match named(view, id) {
        Named::Root => menu(didl::ContainerSpec::menu(
            menus.root.clone(),
            menus.root.clone(),
            "Kantele",
            root_children(library, view, menus).len(),
        )),
        Named::Albums => menu(didl::ContainerSpec::menu(
            menus.albums.clone(),
            menus.root.clone(),
            "Albums",
            library.albums().len(),
        )),
        Named::Artists => menu(didl::ContainerSpec::menu(
            menus.artists.clone(),
            menus.root.clone(),
            "Artists",
            library.artists().len(),
        )),
        Named::Music => menu(music_spec(library, menus)),
        Named::Untagged => menu(didl::ContainerSpec::menu(
            menus.untagged.clone(),
            menus.root.clone(),
            UNTAGGED_TITLE,
            browse::untagged_count(library, view),
        )),
        Named::Playlists => menu(playlists_spec(library, menus)),
        Named::Recent => menu(didl::ContainerSpec::menu(
            menus.recent.clone(),
            menus.root.clone(),
            RECENT_TITLE,
            view.recent().len(),
        )),
        Named::Folders => {
            let (folders, loose) = browse::folder_size(view, "");
            menu(didl::ContainerSpec::folder(
                menus.folders.clone(),
                menus.root.clone(),
                FOLDERS_TITLE,
                folders + loose,
            ))
        }
        Named::Folder(path) => {
            let here = folder_object(&path)?;
            let (below, inside) = browse::folder_size(view, &path);
            menu(didl::ContainerSpec::folder(
                here,
                menus.folders.clone(),
                browse::folder_name(&path).to_owned(),
                below + inside,
            ))
        }
        Named::Position(position) => {
            let here = position_object(&position)?;
            let offered = browse::menu(library, view, &position)?;
            let children = facet_children(
                library,
                view,
                menus,
                &position,
                &here,
                offered,
                Window::NONE,
            )
            .total();
            let title = match (position.grouped, position.listing) {
                (Some(browse::Grouped::Letter(letter)), _) => browse::group_display(letter),
                (Some(browse::Grouped::Index), _) => browse::INDEX_TITLE.to_owned(),
                (None, Some(facet)) => facet.title().to_owned(),
                (None, None) => library.name().to_owned(),
            };
            menu(didl::ContainerSpec::menu(
                here,
                menus.root.clone(),
                title,
                children,
            ))
        }
        Named::Part(id) => Some(parts::metadata(&parts::find(library, &id)?)),
        Named::Minted(object) => {
            if library.album(&object).is_some()
                || library.artist(&object).is_some()
                || library.playlist(&object).is_some()
            {
                return minted_metadata(library, menus, &object);
            }
            let track = library.get(&object)?;
            Some(didl::Child::Item(
                track,
                track_parent(library, menus, track),
            ))
        }
        Named::Unknown => None,
    }
}

fn minted_metadata<'a>(
    library: &'a Library,
    menus: &'a Menus,
    object: &ObjectId,
) -> Option<didl::Child<'a>> {
    if let Some(album) = library.album(object) {
        return Some(album_child(album, &menus.albums));
    }
    if let Some(artist) = library.artist(object) {
        return Some(artist_child(artist, &menus.artists));
    }
    library
        .playlist(object)
        .map(|playlist| playlist_child(playlist, &menus.playlists))
}

pub(super) fn track_parent<'a>(
    library: &'a Library,
    menus: &'a Menus,
    track: &'a Track,
) -> &'a ObjectId {
    let album = track.album_id.as_ref().and_then(|id| library.album(id));
    match (album, library.track_index(&track.id)) {
        (Some(album), Some(at)) => parts::parent_of(album, at),
        (Some(album), None) => &album.id,
        (None, _) => &menus.music,
    }
}

fn music_spec<'a>(library: &'a Library, menus: &'a Menus) -> didl::ContainerSpec<'a> {
    didl::ContainerSpec {
        ..didl::ContainerSpec::menu(
            menus.music.clone(),
            menus.root.clone(),
            browse::root::counted(library.len(), "item"),
            library.len(),
        )
    }
}

fn album_spec<'a>(album: &'a Album, parent: ObjectId) -> didl::ContainerSpec<'a> {
    album_like(
        album,
        album.id.clone(),
        parent,
        album.title.clone(),
        parts::album_child_count(album),
    )
}

/// A container that is an album or a part of one, carrying the album's credit, date and cover.
pub(super) fn album_like<'a>(
    album: &'a Album,
    id: ObjectId,
    parent: ObjectId,
    title: String,
    children: usize,
) -> didl::ContainerSpec<'a> {
    didl::ContainerSpec {
        id,
        parent,
        title,
        child_count: children,
        searchable: true,
        art: base_art(album),
        class: didl::ALBUM,
        genres: &album.genres,
        date: album.date.as_deref(),
        artists: &album.artists,
        credited: album.credited,
    }
}

fn artist_spec<'a>(artist: &'a Artist, parent: ObjectId) -> didl::ContainerSpec<'a> {
    didl::ContainerSpec {
        class: didl::ARTIST,
        ..didl::ContainerSpec::menu(
            artist.id.clone(),
            parent,
            artist.name.clone(),
            artist.albums.len() + artist.tracks.len(),
        )
    }
}

pub(super) fn album_child<'a>(album: &'a Album, parent: &ObjectId) -> didl::Child<'a> {
    didl::Child::Container(album_spec(album, parent.clone()))
}

pub(super) fn artist_child<'a>(artist: &'a Artist, parent: &ObjectId) -> didl::Child<'a> {
    didl::Child::Container(artist_spec(artist, parent.clone()))
}

fn playlists_spec<'a>(library: &'a Library, menus: &'a Menus) -> didl::ContainerSpec<'a> {
    didl::ContainerSpec::menu(
        menus.playlists.clone(),
        menus.root.clone(),
        PLAYLISTS_TITLE,
        library.playlists().len(),
    )
}

pub(super) fn playlist_child<'a>(playlist: &'a Playlist, parent: &ObjectId) -> didl::Child<'a> {
    didl::Child::Container(didl::ContainerSpec {
        class: didl::PLAYLIST,
        ..didl::ContainerSpec::menu(
            playlist.id.clone(),
            parent.clone(),
            playlist.title.clone(),
            playlist.tracks.len(),
        )
    })
}

fn base_art(album: &Album) -> Option<(&ObjectId, &Artwork)> {
    album.artwork.as_ref().map(|art| (&album.id, art))
}
