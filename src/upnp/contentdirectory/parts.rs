//! What an album opens into: its discs where they stand apart, and the runs a tagger grouped.

use crate::index::identity;
use crate::index::{Album, Library};
use crate::upnp::{ObjectId, didl};

use super::objects::album_like;

/// One part of one album, found from its identifier.
pub(super) enum Part<'a> {
    Disc(&'a Album, usize),
    Run(&'a Album, usize),
}

/// The part an identifier names, or nothing where it names none this library holds.
pub(super) fn find<'a>(library: &'a Library, id: &str) -> Option<Part<'a>> {
    let album = library.album(&identity::part_album(id)?)?;
    if let Some(at) = album.discs.iter().position(|disc| disc.id.as_str() == id) {
        return Some(Part::Disc(album, at));
    }
    album
        .runs
        .iter()
        .position(|run| run.id.as_str() == id)
        .map(|at| Part::Run(album, at))
}

/// What an album offers when opened.
pub(super) fn album_children<'a>(library: &'a Library, album: &'a Album) -> Vec<didl::Child<'a>> {
    if album.separate_discs {
        return album
            .discs
            .iter()
            .map(|disc| didl::Child::Container(disc_spec(album, disc)))
            .collect();
    }
    listed(library, album, &album.tracks, &album.id)
}

/// How many children an album offers, which is what its container claims.
pub(super) fn album_child_count(album: &Album) -> usize {
    match album.separate_discs {
        true => album.discs.len(),
        false => shown(album, &album.tracks).len(),
    }
}

pub(super) fn children<'a>(library: &'a Library, part: &Part<'a>) -> Vec<didl::Child<'a>> {
    match part {
        Part::Disc(album, at) => {
            let disc = &album.discs[*at];
            listed(library, album, &disc.tracks, &disc.id)
        }
        Part::Run(album, at) => {
            let run = &album.runs[*at];
            run.tracks
                .iter()
                .filter_map(|track| library.tracks().get(*track))
                .map(|track| didl::Child::Item(track, &run.id))
                .collect()
        }
    }
}

pub(super) fn metadata<'a>(part: &Part<'a>) -> didl::Child<'a> {
    match part {
        Part::Disc(album, at) => didl::Child::Container(disc_spec(album, &album.discs[*at])),
        Part::Run(album, at) => didl::Child::Container(run_spec(album, *at)),
    }
}

/// The container a track opens from: its run, else its disc where discs stand apart, else its album.
pub(super) fn parent_of(album: &Album, track: usize) -> &ObjectId {
    if let Some(run) = album.runs.iter().find(|run| run.tracks.contains(&track)) {
        return &run.id;
    }
    if album.separate_discs
        && let Some(disc) = album.discs.iter().find(|disc| disc.tracks.contains(&track))
    {
        return &disc.id;
    }
    &album.id
}

/// What one entry of a listing is: a run where a track opens one, a track where it opens none.
enum Shown {
    Run(usize),
    Track(usize),
}

/// A track inside a run is shown once, as the run, where the run begins.
fn shown(album: &Album, tracks: &[usize]) -> Vec<Shown> {
    tracks
        .iter()
        .filter_map(
            |at| match album.runs.iter().position(|run| run.tracks.contains(at)) {
                Some(run) if album.runs[run].tracks.first() == Some(at) => Some(Shown::Run(run)),
                Some(_) => None,
                None => Some(Shown::Track(*at)),
            },
        )
        .collect()
}

fn listed<'a>(
    library: &'a Library,
    album: &'a Album,
    tracks: &[usize],
    parent: &'a ObjectId,
) -> Vec<didl::Child<'a>> {
    shown(album, tracks)
        .into_iter()
        .filter_map(|entry| match entry {
            Shown::Run(at) => Some(didl::Child::Container(run_spec(album, at))),
            Shown::Track(at) => library
                .tracks()
                .get(at)
                .map(|track| didl::Child::Item(track, parent)),
        })
        .collect()
}

fn disc_spec<'a>(album: &'a Album, disc: &'a crate::index::Disc) -> didl::ContainerSpec<'a> {
    let title = disc.subtitle.clone().unwrap_or_else(|| match disc.number {
        Some(number) => format!("Disc {number}"),
        None => "Disc".to_owned(),
    });
    album_like(
        album,
        disc.id.clone(),
        album.id.clone(),
        title,
        shown(album, &disc.tracks).len(),
    )
}

fn run_spec<'a>(album: &'a Album, at: usize) -> didl::ContainerSpec<'a> {
    let run = &album.runs[at];
    let parent = match run.tracks.first() {
        Some(track) if album.separate_discs => album
            .discs
            .iter()
            .find(|disc| disc.tracks.contains(track))
            .map_or(&album.id, |disc| &disc.id),
        _ => &album.id,
    };
    album_like(
        album,
        run.id.clone(),
        parent.clone(),
        run.title.clone(),
        run.tracks.len(),
    )
}
