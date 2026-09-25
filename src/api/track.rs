//! One file, as the page shows it: what the tags say, and where those tags put it in the menus.

use serde::{Deserialize, Serialize};

use crate::browse::{self, Place, Served, format_name};
use crate::index::{Album, Library, Rule, Track};
use crate::report;

#[derive(Debug, Default, Deserialize)]
pub struct Asked {
    /// The track's path, relative to the music folder, as a problem list names it.
    #[serde(default)]
    pub path: String,
}

/// One row of the tag panel. Nothing where the file carries no such tag.
#[derive(Debug, Serialize)]
pub struct Tag {
    pub label: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
}

/// The album this file was filed under.
#[derive(Debug, Serialize)]
pub struct Filed {
    pub title: String,
    /// What a device browses to reach it.
    pub at: String,
    /// Whether the folder path told this album apart, because the tags did not.
    pub keyed_on_path: bool,
}

#[derive(Debug, Serialize)]
pub struct Found {
    pub path: String,
    pub title: String,
    pub tags: Vec<Tag>,
    pub places: Vec<Place>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub album: Option<Filed>,
    /// What to ask `/art/` for, where this file has a cover. The one the wire serves.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artwork: Option<String>,
    /// The format and what it holds, in the words the Quality menu uses.
    pub format: String,
    pub bytes: u64,
    pub seconds: u64,
}

fn format(track: &Track) -> String {
    let mut said = format_name(track.mime).to_owned();
    if let Some(bits) = track.bit_depth {
        said.push_str(&format!(" {bits} bit"));
    }
    if let Some(rate) = track.sample_rate {
        said.push_str(&format!(" {:.1} kHz", f64::from(rate) / 1000.0));
    }
    said
}

fn filed(library: &Library, track: &Track) -> Option<Filed> {
    let id = track.album_id.as_ref()?;
    let album: &Album = library.albums().get(library.album_index(id)?)?;
    Some(Filed {
        title: album.title.clone(),
        at: album.id.as_str().to_owned(),
        keyed_on_path: album.rule == Rule::Path,
    })
}

fn optional(label: &'static str, value: Option<String>) -> Option<Tag> {
    value.map(|value| Tag {
        label,
        value: Some(value),
    })
}

/// What the page shows for one file, or nothing where the library holds no such track.
pub fn found(served: &Served, asked: &Asked) -> Option<Found> {
    let at = served
        .library
        .tracks()
        .iter()
        .position(|track| track.relative == asked.path)?;
    let track = &served.library.tracks()[at];
    Some(Found {
        path: track.relative.clone(),
        title: track.title.clone(),
        tags: vec![
            Tag {
                // A file with no title tag is listed by its name, here and on the amplifier.
                label: "Title",
                value: track.title_tagged.then(|| track.title.clone()),
            },
            Tag {
                label: "Artist",
                value: report::credited(&track.artists),
            },
            Tag {
                label: "Album",
                value: track.album.clone(),
            },
            Tag {
                label: "Album artist",
                value: report::credited(&track.album_artists),
            },
            Tag {
                label: "Composer",
                value: report::credited(&track.composers),
            },
            Tag {
                label: "Genre",
                value: (!track.genres.is_empty()).then(|| track.genres.join(", ")),
            },
            Tag {
                label: "Date",
                value: track.date.clone(),
            },
            Tag {
                label: "Track number",
                value: track.track_number.map(|n| n.to_string()),
            },
            Tag {
                label: "Cover art",
                value: report::cover_from(track).map(str::to_owned),
            },
        ]
        .into_iter()
        // Only a file that carries one is asked about: every pop track has no work, and a row
        // saying so on all of them is noise rather than a finding.
        .chain(optional("Disc", track.disc_number.map(|n| n.to_string())))
        .chain(optional("Work", track.work.clone()))
        .collect(),
        places: browse::places(&served.view, at),
        artwork: track.artwork.as_ref().map(|_| track.id.as_str().to_owned()),
        album: filed(&served.library, track),
        format: format(track),
        bytes: track.size,
        seconds: track.duration.as_secs(),
    })
}

/// The same, for a shell with no `jq`.
pub fn lines(found: &Found) -> Vec<(String, String)> {
    let mut lines = vec![
        ("path".to_owned(), found.path.clone()),
        ("title".to_owned(), found.title.clone()),
        ("format".to_owned(), found.format.clone()),
        ("bytes".to_owned(), found.bytes.to_string()),
        ("seconds".to_owned(), found.seconds.to_string()),
    ];
    for tag in &found.tags {
        lines.push((
            tag.label.to_lowercase().replace(' ', "_"),
            tag.value.clone().unwrap_or_else(|| "none".to_owned()),
        ));
    }
    if let Some(album) = &found.album {
        lines.push((
            "album_container".to_owned(),
            format!(
                "{}\t{}{}",
                album.at,
                album.title,
                if album.keyed_on_path {
                    "\tnamed by folder"
                } else {
                    ""
                }
            ),
        ));
    }
    for (at, place) in found.places.iter().enumerate() {
        lines.push((
            format!("place.{}", at + 1),
            format!(
                "{}\t{}\t{}\t{}",
                place.at,
                place.axis,
                place.value,
                browse::root::counted(place.tracks, "track")
            ),
        ));
    }
    lines
}
