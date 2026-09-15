//! Albums and artists derived from tracks.

use std::path::PathBuf;

use crate::index::credits::{self, Credit};
use crate::index::fold;
use crate::index::identity::{self, ArtistMention, Release, most_common};
use crate::index::library::{Album, Artist, Disc, Run, Track};
use crate::index::scan::Scanned;

/// Sorted so a listing does not depend on scan order.
pub(super) fn albums_from(
    releases: &[Release],
    tracks: &[Track],
    ignored: &fold::Ignored,
) -> Vec<Album> {
    let mut albums: Vec<Album> = releases
        .iter()
        .map(|release| {
            let mut members = release.files.clone();
            members.sort_by(|left, right| {
                play_order(&tracks[*left]).cmp(&play_order(&tracks[*right]))
            });
            let credit = album_artists(&members, tracks);
            let mut discs: Vec<u32> = members
                .iter()
                .filter_map(|at| tracks[*at].disc_number)
                .collect();
            discs.sort_unstable();
            discs.dedup();
            let parts = discs_of(&release.id, &members, tracks);
            Album {
                id: release.id.clone(),
                rule: release.rule,
                title: release.title.clone(),
                artists: credit.names,
                credited: credit.tagged,
                date: most_common(members.iter().filter_map(|at| tracks[*at].date.as_deref())),
                genres: distinct(members.iter().flat_map(|at| tracks[*at].genres.iter())),
                disc_count: (discs.len() as u32).max(1),
                musicbrainz_id: release.musicbrainz_id.clone(),
                artwork: members.first().and_then(|at| tracks[*at].artwork.clone()),
                separate_discs: parts.len() > 1
                    && members.iter().any(|at| {
                        tracks[*at].disc_subtitle.is_some() || tracks[*at].disc_from_title
                    }),
                discs: parts,
                runs: runs_of(&release.id, &members, tracks),
                tracks: members,
            }
        })
        .collect();
    let key = |title: &str| ignored.strip(&fold(title)).to_owned();
    albums.sort_by(|left, right| (key(&left.title), &left.id).cmp(&(key(&right.title), &right.id)));
    albums
}

/// The members in play order cut where the disc number changes, so unnumbered files are one disc.
fn discs_of(album: &crate::object::ObjectId, members: &[usize], tracks: &[Track]) -> Vec<Disc> {
    let mut discs: Vec<Disc> = Vec::new();
    for at in members {
        let track = &tracks[*at];
        match discs.last_mut() {
            Some(disc) if disc.number == track.disc_number => disc.tracks.push(*at),
            _ => discs.push(Disc {
                id: identity::disc_part(album, track.disc_number),
                number: track.disc_number,
                subtitle: None,
                tracks: vec![*at],
            }),
        }
    }
    for disc in &mut discs {
        disc.subtitle = most_common(
            disc.tracks
                .iter()
                .filter_map(|at| tracks[*at].disc_subtitle.as_deref()),
        );
    }
    discs
}

/// Consecutive members sharing a grouping tag on one disc. A track without one ends the run, so
/// a name used twice is two runs.
fn runs_of(album: &crate::object::ObjectId, members: &[usize], tracks: &[Track]) -> Vec<Run> {
    let mut candidates: Vec<(String, Option<u32>, Vec<usize>)> = Vec::new();
    for at in members {
        let track = &tracks[*at];
        let key = track.grouping.as_deref().map(fold).unwrap_or_default();
        match candidates.last_mut() {
            Some((last, disc, run))
                if !key.is_empty() && *last == key && *disc == track.disc_number =>
            {
                run.push(*at);
            }
            _ => candidates.push((key, track.disc_number, vec![*at])),
        }
    }
    let mut named: Vec<(Option<u32>, String, Option<u32>)> = Vec::new();
    candidates
        .into_iter()
        .filter(|(key, _, run)| !key.is_empty() && run.len() > 1)
        .map(|(key, disc, run)| {
            let first = tracks[run[0]].track_number;
            let again = named
                .iter()
                .filter(|(d, k, f)| *d == disc && *k == key && *f == first)
                .count() as u32;
            named.push((disc, key.clone(), first));
            Run {
                id: identity::run_part(album, disc, &key, first, again),
                title: most_common(run.iter().filter_map(|at| tracks[*at].grouping.as_deref()))
                    .unwrap_or_default(),
                tracks: run,
            }
        })
        .collect()
}

fn play_order(track: &Track) -> (u32, u32, String, &PathBuf) {
    (
        track.disc_number.unwrap_or(u32::MAX),
        track.track_number.unwrap_or(u32::MAX),
        fold(&track.title),
        &track.path,
    )
}

pub struct AlbumCredit {
    pub names: Vec<Credit>,
    pub tagged: bool,
}

/// The list most files carry, the same one the release key uses.
fn album_artists(members: &[usize], tracks: &[Track]) -> AlbumCredit {
    let credited = || {
        members
            .iter()
            .map(|at| &tracks[*at].album_artists)
            .filter(|artists| !artists.is_empty())
    };
    if let Some(folded) =
        identity::most_common_of(credited().map(|artists| credits::folded(artists)))
        && let Some(credits) = credited().find(|artists| credits::folded(artists) == folded)
    {
        return AlbumCredit {
            names: credits.clone(),
            tagged: true,
        };
    }
    let unanimous = distinct_credits(members.iter().flat_map(|at| tracks[*at].artists.iter()));
    AlbumCredit {
        names: if unanimous.len() == 1 {
            unanimous
        } else {
            Vec::new()
        },
        tagged: false,
    }
}

fn distinct_credits<'a>(credits: impl Iterator<Item = &'a Credit>) -> Vec<Credit> {
    let mut seen: Vec<Credit> = Vec::new();
    let mut folded: Vec<String> = Vec::new();
    for credit in credits {
        let key = fold(&credit.name);
        if key.is_empty() || folded.contains(&key) {
            continue;
        }
        folded.push(key);
        seen.push(credit.clone());
    }
    seen
}

fn distinct<'a>(values: impl Iterator<Item = &'a String>) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    let mut folded: Vec<String> = Vec::new();
    for value in values {
        let key = fold(value);
        if key.is_empty() || folded.contains(&key) {
            continue;
        }
        folded.push(key);
        seen.push(value.clone());
    }
    seen
}

#[derive(Clone, Copy, Debug)]
enum Owner {
    Album(usize),
    Track(usize),
}

pub(super) fn artists_from(
    albums: &[Album],
    tracks: &[Track],
    files: &[Scanned],
    ignored: &fold::Ignored,
) -> Vec<Artist> {
    let mentioned = mentions_of(albums, tracks, files);
    let album_at = albums_by_track(albums, tracks.len());
    let mentions: Vec<ArtistMention> = mentioned
        .iter()
        .map(|(name, mbid, _)| ArtistMention {
            name,
            mbid: mbid.as_deref(),
        })
        .collect();

    let mut artists: Vec<Artist> = identity::group_artists(&mentions)
        .into_iter()
        .map(|group| {
            let (albums_of, tracks_of) = owned_by(&group.mentions, &mentioned, &album_at);
            Artist {
                id: group.id,
                rule: group.rule,
                name: group.name,
                musicbrainz_id: group.musicbrainz_id,
                albums: albums_of,
                tracks: tracks_of,
            }
        })
        .collect();
    let key = |name: &str| ignored.strip(&fold(name)).to_owned();
    artists.sort_by(|left, right| (key(&left.name), &left.id).cmp(&(key(&right.name), &right.id)));
    artists
}

/// Name, the identifier written beside it, and its owner.
type Mention = (String, Option<String>, Owner);

fn mentions_of(albums: &[Album], tracks: &[Track], files: &[Scanned]) -> Vec<Mention> {
    let mut mentioned: Vec<Mention> = Vec::new();
    for (at, album) in albums.iter().enumerate() {
        // Identifiers must come from the file the credit came from.
        let chosen = credits::folded(&album.artists);
        let credited = album
            .tracks
            .iter()
            .copied()
            .find(|track| credits::folded(&tracks[*track].album_artists) == chosen);
        let ids = credited
            .and_then(|track| files.get(track))
            .map(|file| file.tags.musicbrainz_album_artist_ids.as_slice());
        for (position, name) in album.artists.iter().enumerate() {
            let id = aligned_id(ids, album.artists.len(), position);
            mentioned.push((name.name.clone(), id, Owner::Album(at)));
        }
    }

    for (at, track) in tracks.iter().enumerate() {
        let tags = files.get(at).map(|file| &file.tags);
        // Inside an album the track mentions only its own performer.
        let (credited, ids) = if track.album_id.is_some() || track.album_artists.is_empty() {
            (
                &track.artists,
                tags.map(|tags| tags.musicbrainz_artist_ids.as_slice()),
            )
        } else {
            (
                &track.album_artists,
                tags.map(|tags| tags.musicbrainz_album_artist_ids.as_slice()),
            )
        };
        for (position, credit) in credited.iter().enumerate() {
            let id = aligned_id(ids, credited.len(), position);
            mentioned.push((credit.name.clone(), id, Owner::Track(at)));
        }
    }
    mentioned
}

fn albums_by_track(albums: &[Album], tracks: usize) -> Vec<Option<usize>> {
    let mut holder = vec![None; tracks];
    for (at, album) in albums.iter().enumerate() {
        for track in &album.tracks {
            if let Some(slot) = holder.get_mut(*track) {
                *slot = Some(at);
            }
        }
    }
    holder
}

fn owned_by(
    group: &[usize],
    mentioned: &[Mention],
    album_at: &[Option<usize>],
) -> (Vec<usize>, Vec<usize>) {
    let mut albums: Vec<usize> = Vec::new();
    for mention in group {
        if let Owner::Album(at) = mentioned[*mention].2
            && !albums.contains(&at)
        {
            albums.push(at);
        }
    }
    let mut tracks: Vec<usize> = Vec::new();
    for mention in group {
        if let Owner::Track(at) = mentioned[*mention].2
            && !tracks.contains(&at)
            && !album_at
                .get(at)
                .copied()
                .flatten()
                .is_some_and(|album| albums.contains(&album))
        {
            tracks.push(at);
        }
    }
    (albums, tracks)
}

/// Only when the tagger wrote one identifier per name.
fn aligned_id(ids: Option<&[String]>, names: usize, position: usize) -> Option<String> {
    let ids = ids?;
    if ids.len() != names {
        return None;
    }
    ids.get(position).cloned()
}
