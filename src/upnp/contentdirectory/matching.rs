//! Search matching.

use crate::browse::View;
use crate::index::Library;
use crate::index::searchable;
use crate::upnp::{didl, search};

use super::objects::{album_child, artist_child, children, playlist_child, track_parent};
use super::paging::{Listing, Window};
use super::{ALBUMS, ARTISTS, MUSIC, Menus, PLAYLISTS};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    Albums,
    Artists,
    Tracks,
    Playlists,
    /// The values of the Genre axis, which a control point looks for by class rather than by
    /// walking the menus. The HEOS app builds its own Genres list this way.
    Genres,
}

pub(super) enum Space<'a> {
    Whole(&'static [Kind]),
    Children(Vec<didl::Child<'a>>),
}

pub(super) fn search_space<'a>(
    library: &'a Library,
    view: &'a View,
    menus: &'a Menus,
    id: &str,
) -> Option<Space<'a>> {
    match id {
        _ if id == menus.root.as_str() => Some(Space::Whole(&[
            Kind::Albums,
            Kind::Artists,
            Kind::Tracks,
            Kind::Playlists,
            Kind::Genres,
        ])),
        ALBUMS => Some(Space::Whole(&[Kind::Albums])),
        ARTISTS => Some(Space::Whole(&[Kind::Artists])),
        MUSIC => Some(Space::Whole(&[Kind::Tracks])),
        PLAYLISTS => Some(Space::Whole(&[Kind::Playlists])),
        minted => children(library, view, menus, minted, Window::ALL)
            .map(|listing| Space::Children(listing.into_all())),
    }
}

pub(super) fn indexed_matches<'a>(
    library: &'a Library,
    view: &'a View,
    menus: &'a Menus,
    kinds: &[Kind],
    criteria: &search::Criteria,
    window: Window,
) -> Listing<'a> {
    let mut matched = Matched::within(window);
    let searchable = library.searchable();
    for kind in kinds {
        match kind {
            Kind::Albums => {
                for (at, album) in library.albums().iter().enumerate() {
                    if criteria.matches(&searchable.albums.object(at)) {
                        matched.keep(|| album_child(album, &menus.albums));
                    }
                }
            }
            Kind::Artists => {
                for (at, artist) in library.artists().iter().enumerate() {
                    if criteria.matches(&searchable.artists.object(at)) {
                        matched.keep(|| artist_child(artist, &menus.artists));
                    }
                }
            }
            Kind::Tracks => {
                for (at, track) in library.tracks().iter().enumerate() {
                    if criteria.matches(&searchable.tracks.object(at)) {
                        matched
                            .keep(|| didl::Child::Item(track, track_parent(library, menus, track)));
                    }
                }
            }
            Kind::Playlists => {
                for (at, playlist) in library.playlists().iter().enumerate() {
                    if criteria.matches(&searchable.playlists.object(at)) {
                        matched.keep(|| playlist_child(playlist, &menus.playlists));
                    }
                }
            }
            Kind::Genres => {
                for genre in genres(library, view, menus) {
                    if criteria.matches(&folded(&genre)) {
                        matched.keep(|| genre);
                    }
                }
            }
        }
    }
    matched.listing()
}

/// The genre containers as the menus offer them, so a search and a browse name the same objects.
/// Nothing where the owner left the axis out: what the menus do not hold, a search does not find.
fn genres<'a>(library: &'a Library, view: &'a View, menus: &'a Menus) -> Vec<didl::Child<'a>> {
    let listing = crate::browse::root::entries(library, view)
        .into_iter()
        .find_map(|entry| match entry.opens {
            crate::browse::root::Opens::Axis(crate::browse::Facet::Genre, at) => Some(at),
            _ => None,
        })
        .and_then(|at| children(library, view, menus, &at.id(), Window::ALL));
    let Some(listing) = listing else {
        return Vec::new();
    };
    listing
        .into_all()
        .into_iter()
        .filter(|child| matches!(child, didl::Child::Container(spec) if spec.class == didl::GENRE))
        .collect()
}

struct Matched<'a> {
    page: Vec<didl::Child<'a>>,
    total: usize,
    wanted: std::ops::Range<usize>,
}

impl<'a> Matched<'a> {
    fn within(window: Window) -> Self {
        Self {
            page: Vec::new(),
            total: 0,
            wanted: window.open_range(),
        }
    }

    fn keep(&mut self, child: impl FnOnce() -> didl::Child<'a>) {
        if self.wanted.contains(&self.total) {
            self.page.push(child());
        }
        self.total += 1;
    }

    fn listing(self) -> Listing<'a> {
        Listing::Page(self.page, self.total)
    }
}

pub(super) fn folded(child: &didl::Child<'_>) -> searchable::Folded {
    match child {
        didl::Child::Container(spec) => searchable::Folded::of(
            spec.class,
            searchable::Raw {
                title: &spec.title,
                aliases: &[],
                album: Some(&spec.title),
                artists: spec.artists,
                album_artists: if spec.credited { spec.artists } else { &[] },
                genres: spec.genres,
                date: spec.date,
                composers: &[],
            },
        ),
        didl::Child::Item(track, _) => searchable::Folded::of(
            didl::MUSIC_TRACK,
            searchable::Raw {
                title: &track.title,
                aliases: &[],
                album: track.album.as_deref(),
                artists: &track.artists,
                album_artists: &track.album_artists,
                composers: &track.composers,
                genres: &track.genres,
                date: track.date.as_deref(),
            },
        ),
    }
}
