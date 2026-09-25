//! Turning the index into menus.

use sha2::{Digest, Sha256};

use crate::index::credits::Credit;
use crate::index::{Library, Track, fold};

pub use axes::{Axes, Entry};
pub use folders::{
    Folders, Subfolder, albums_of, folder, folder_from_id, folder_id, folder_name, folder_size,
    tracks_in,
};
pub use view::{Served, View};

mod axes;
mod folders;
pub mod root;
mod view;

use folders::scoped;

/// An axis the library can be narrowed along.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize, serde::Serialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum Facet {
    Genre,
    Artist,
    AllArtists,
    Composer,
    Work,
    Date,
    Quality,
    Bits,
    Channels,
    Frequency,
    Type,
}

/// Every axis that exists, in the order a menu offers them when it offers them all.
pub const FACETS: &[Facet] = &[
    Facet::Genre,
    Facet::Artist,
    Facet::AllArtists,
    Facet::Composer,
    Facet::Work,
    Facet::Date,
    // What the file is rather than what the music is.
    Facet::Quality,
    Facet::Bits,
    Facet::Channels,
    Facet::Frequency,
    Facet::Type,
];

/// The axes a menu offers unless the file says otherwise.
pub const DEFAULT_AXES: &[Facet] = &[
    Facet::Genre,
    Facet::Artist,
    Facet::AllArtists,
    Facet::Composer,
    Facet::Date,
    Facet::Quality,
];

impl Facet {
    /// The letter that names this axis inside an object identifier.
    pub const fn code(self) -> char {
        match self {
            Self::Genre => 'g',
            Self::Artist => 'a',
            Self::AllArtists => 'A',
            Self::Composer => 'c',
            Self::Work => 'w',
            Self::Date => 'd',
            Self::Quality => 'q',
            Self::Bits => 'b',
            Self::Channels => 'n',
            Self::Frequency => 'h',
            Self::Type => 't',
        }
    }

    fn from_code(code: char) -> Option<Self> {
        FACETS.iter().copied().find(|facet| facet.code() == code)
    }

    /// What a client shows as the menu entry.
    pub const fn title(self) -> &'static str {
        match self {
            Self::Genre => "Genre",
            Self::Artist => "Artist",
            Self::AllArtists => "All Artists",
            Self::Composer => "Composer",
            Self::Work => "Work",
            Self::Date => "Date",
            // The names the competitor with the most axes uses, because a listener has seen them.
            Self::Quality => "Quality",
            Self::Bits => "Bits",
            Self::Channels => "Channels",
            Self::Frequency => "Frequency",
            Self::Type => "Type",
        }
    }

    pub const fn class(self) -> &'static str {
        match self {
            Self::Genre => crate::upnp::didl::GENRE,
            Self::Artist | Self::AllArtists | Self::Composer => crate::upnp::didl::ARTIST,
            Self::Work
            | Self::Date
            | Self::Quality
            | Self::Bits
            | Self::Channels
            | Self::Frequency
            | Self::Type => crate::upnp::didl::MENU,
        }
    }

    /// This track's values on the axis.
    fn values(self, track: &Track) -> Values<'_> {
        match self {
            Self::Genre => Values::Many(&track.genres),
            // A compilation is served under its own credit but stands under no artist: one entry
            // holding every compilation is not an artist anybody looks for.
            Self::Artist => Values::Credited(match () {
                () if track.compilation => &[],
                () if !track.album_artists.is_empty() => &track.album_artists,
                () => &track.artists,
            }),
            Self::AllArtists => Values::Credited(&track.artists),
            Self::Composer => Values::Credited(&track.composers),
            Self::Work => Values::One(track.work.as_deref()),
            Self::Date => Values::One(track.date.as_deref().and_then(year)),
            Self::Quality => Values::One(quality(track)),
            Self::Bits => Values::Written(track.bit_depth.map(|bits| format!("{bits} bit"))),
            Self::Channels => Values::Written(track.channels.map(channels)),
            Self::Frequency => Values::Written(track.sample_rate.map(frequency)),
            Self::Type => Values::One(Some(format_name(track.mime))),
        }
    }

    pub const fn from_tags(self) -> bool {
        match self {
            Self::Genre
            | Self::Artist
            | Self::AllArtists
            | Self::Composer
            | Self::Work
            | Self::Date => true,
            Self::Quality | Self::Bits | Self::Channels | Self::Frequency | Self::Type => false,
        }
    }

    /// What a listing orders this axis's values by, which is the folded form unless it is a number.
    pub(super) fn sort_key(self, folded: &str) -> String {
        match self {
            Self::Quality => padded(quality_rank(folded)),
            Self::Bits | Self::Frequency => padded(leading_number(folded)),
            Self::Channels => padded(match folded {
                "mono" => Some(1.0),
                "stereo" => Some(2.0),
                other => leading_number(other),
            }),
            _ => folded.to_owned(),
        }
    }
}

/// The year inside a date tag, which is the first run of four digits wherever a tagger put it.
fn year(date: &str) -> Option<&str> {
    let bytes = date.as_bytes();
    (0..bytes.len().checked_sub(3)?)
        .find(|at| bytes[*at..*at + 4].iter().all(u8::is_ascii_digit))
        .map(|at| &date[at..at + 4])
}

/// Quality buckets, from lossy up to DSD. `LC` is renamed and `CD-` folds into `CD`.
const QUALITIES: &[&str] = &[
    "Lossy", "Below CD", "CD", "CD+", "HD", "HD+", "DXD", "DSD64", "DSD128", "DSD256", "DSD512",
];

/// DSD64 is this many samples a second, and every higher rate doubles it.
const DSD64: u32 = 2_822_400;

fn quality(track: &Track) -> Option<&'static str> {
    if matches!(track.mime, "audio/x-dsf" | "audio/x-dff") {
        let steps = track.sample_rate?.checked_div(DSD64)?;
        let bucket =
            QUALITIES.iter().position(|q| *q == "DSD64")? + steps.checked_ilog2()? as usize;
        return QUALITIES.get(bucket).copied();
    }
    // An MPEG-4 container holds ALAC, which has a bit depth, or AAC, which has none.
    let lossy = matches!(track.mime, "audio/mpeg" | "audio/ogg" | "audio/opus")
        || (track.mime == "audio/mp4" && track.bit_depth.is_none());
    if lossy {
        return Some("Lossy");
    }
    let rate = track.sample_rate?;
    let bits = track.bit_depth?;
    // The rate decides the high-definition steps; the word depth only lifts a file above CD.
    Some(match (bits, rate) {
        (_, 352_800..) => "DXD",
        (_, 176_400..) => "HD+",
        (_, 88_200..) => "HD",
        (..16, _) | (_, ..44_100) => "Below CD",
        (16, 44_100) => "CD",
        _ => "CD+",
    })
}

/// Where a quality bucket sits from worst to best, so a listing climbs rather than alphabetises.
fn quality_rank(folded: &str) -> Option<f64> {
    QUALITIES
        .iter()
        .position(|q| fold(q) == folded)
        .map(|rank| rank as f64)
}

/// A channel count as a listener names it, and as a count above two where there is no name.
fn channels(count: u8) -> String {
    match count {
        1 => "Mono".to_owned(),
        2 => "Stereo".to_owned(),
        other => format!("{other} channels"),
    }
}

/// A sample rate in kilohertz, carrying the one decimal the rates in use need and no more.
fn frequency(hertz: u32) -> String {
    let khz = f64::from(hertz) / 1000.0;
    match khz.fract() == 0.0 {
        true => format!("{khz:.0} kHz"),
        false => format!("{khz:.1} kHz"),
    }
}

/// The name a listener knows a format by.
pub fn format_name(mime: &'static str) -> &'static str {
    match mime {
        "audio/x-flac" => "FLAC",
        "audio/mpeg" => "MP3",
        "audio/mp4" => "M4A",
        "audio/ogg" => "Ogg",
        "audio/opus" => "Opus",
        "audio/x-wav" => "WAV",
        "audio/x-aiff" => "AIFF",
        "audio/x-wavpack" => "WavPack",
        "audio/x-monkeys-audio" => "APE",
        "audio/x-dsf" => "DSF",
        "audio/x-dff" => "DFF",
        other => other,
    }
}

/// A number written so that the text order over these keys is the numeric order.
fn padded(value: Option<f64>) -> String {
    match value {
        // A value carrying no number sorts after every value that does.
        None => "~".to_owned(),
        Some(number) => format!("{number:018.4}"),
    }
}

fn leading_number(folded: &str) -> Option<f64> {
    folded
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect::<String>()
        .parse()
        .ok()
}

/// One track's values on one axis, borrowed where the tags hold them and written where they do not.
enum Values<'a> {
    Many(&'a [String]),
    /// Names that carry a sort spelling of their own.
    Credited(&'a [Credit]),
    One(Option<&'a str>),
    /// A value this server spells rather than reads, owned for the length of the call.
    Written(Option<String>),
}

impl<'a> Values<'a> {
    /// Each value with what it is ordered by.
    fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        let (many, credited, one) = match self {
            Self::Many(values) => (*values, &[][..], None),
            Self::Credited(values) => (&[][..], *values, None),
            Self::One(value) => (&[][..], &[][..], *value),
            Self::Written(value) => (&[][..], &[][..], value.as_deref()),
        };
        many.iter()
            .map(|value| (value.as_str(), value.as_str()))
            .chain(
                credited
                    .iter()
                    .map(|credit| (credit.name.as_str(), credit.ordered_by())),
            )
            .chain(one.map(|value| (value, value)))
    }
}

/// Where a browse has got to.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Position {
    /// The folder this position is confined to, as the digest of its relative path.
    pub scope: Option<String>,
    pub chosen: Vec<(Facet, String)>,
    pub listing: Option<Facet>,
    /// Where inside a listing's letter index this position is, or nothing for the listing itself.
    pub grouped: Option<Grouped>,
}

/// A position inside the letter index of one listing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Grouped {
    /// The index itself, which offers one entry per letter.
    Index,
    /// One letter of it, which offers the values falling under it.
    Letter(char),
}

/// The prefix of every faceted position.
pub const PREFIX: &str = "f";

/// The letter that introduces a folder scope.
const SCOPE: char = 'p';

/// The letter that introduces a letter group, which is not a facet and needs its own marker.
const GROUP: char = 'l';

/// The group a value whose folded form starts with no letter falls into.
const OTHERS: char = '0';

impl Position {
    /// The object identifier for this position.
    pub fn id(&self) -> String {
        let mut id = String::from(PREFIX);
        if let Some(scope) = &self.scope {
            id.push('-');
            id.push(SCOPE);
            id.push_str(scope);
        }
        for (facet, value) in &self.chosen {
            id.push('-');
            id.push(facet.code());
            id.push_str(value);
        }
        if let Some(facet) = self.listing {
            id.push('-');
            id.push(facet.code());
        }
        match self.grouped {
            None => {}
            Some(Grouped::Index) => {
                id.push('-');
                id.push(GROUP);
            }
            Some(Grouped::Letter(letter)) => {
                id.push('-');
                id.push(GROUP);
                id.push(letter);
            }
        }
        id
    }

    /// Reads a position back out of an identifier, or nothing if it is not one.
    pub fn parse(id: &str) -> Option<Self> {
        let mut parts = id.split('-');
        if parts.next()? != PREFIX {
            return None;
        }
        let mut position = Self::default();
        for part in parts {
            let mut characters = part.chars();
            let code = characters.next()?;
            let value: String = characters.collect();
            let hex = value.len() == DIGEST && value.chars().all(|c| c.is_ascii_hexdigit());
            if code == GROUP {
                // The index and its letters both live inside a listing.
                if position.listing.is_none() || position.grouped.is_some() {
                    return None;
                }
                position.grouped = match value.chars().collect::<Vec<char>>()[..] {
                    [] => Some(Grouped::Index),
                    [letter] if group_key(letter) => Some(Grouped::Letter(letter)),
                    _ => return None,
                };
                continue;
            }
            if position.listing.is_some() {
                return None;
            }
            if code == SCOPE {
                if !hex || position.scope.is_some() || !position.chosen.is_empty() {
                    return None;
                }
                position.scope = Some(value);
                continue;
            }
            let facet = Facet::from_code(code)?;
            let in_order = position.chosen.last().is_none_or(|(last, _)| *last < facet);
            if value.is_empty() {
                position.listing = Some(facet);
            } else if hex && in_order {
                position.chosen.push((facet, value));
            } else {
                return None;
            }
        }
        Some(position)
    }

    /// The same choices with one more made.
    #[cfg(test)]
    fn narrowed(&self, facet: Facet, value: &str) -> Self {
        self.chose(facet, &digest(value))
    }

    /// The same choices with one more made, named by the digest the interned axis already holds.
    pub fn chose(&self, facet: Facet, digest: &str) -> Self {
        let mut chosen = self.chosen.clone();
        chosen.push((facet, digest.to_owned()));
        // One order whatever order the choices were made in, so two routes are one object.
        chosen.sort();
        Self {
            scope: self.scope.clone(),
            chosen,
            listing: None,
            grouped: None,
        }
    }

    /// The same listing, showing its letter index rather than its values.
    pub fn at_index(&self) -> Self {
        Self {
            grouped: Some(Grouped::Index),
            ..self.clone()
        }
    }

    /// The same listing, narrowed to one letter group.
    pub fn at_letter(&self, letter: char) -> Self {
        Self {
            grouped: Some(Grouped::Letter(letter)),
            ..self.clone()
        }
    }

    /// The same choices, showing one axis's values.
    pub fn listing(&self, facet: Facet) -> Self {
        Self {
            scope: self.scope.clone(),
            chosen: self.chosen.clone(),
            listing: Some(facet),
            grouped: None,
        }
    }

    /// A tag view confined to one folder.
    pub fn inside(folder: &str) -> Self {
        Self {
            scope: Some(folders::path_digest(folder)),
            ..Self::default()
        }
    }
}

/// Hex characters of the digest carried in an identifier.
const DIGEST: usize = 16;

/// The digest an identifier carries for a value.
fn digest(value: &str) -> String {
    digest_of(fold(value).as_bytes())
}

fn digest_of(bytes: &[u8]) -> String {
    hex::encode(&Sha256::digest(bytes)[..8])
}

/// What a position offers.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Menu<'a> {
    /// The axes that still narrow something, each with the number of values it offers.
    Facets(Vec<(Facet, Position, usize)>),
    /// One axis and its values, borrowed so a page can be minted rather than the whole list, and
    /// the selection they were drawn from.
    Values(Facet, Vec<Entry<'a>>, Vec<usize>),
    /// One axis split into letter groups, which only a configured menu ever answers.
    Letters(Facet, Vec<Group>),
    /// Nothing narrows any further, so the selection itself is shown, as indices into the tracks.
    Tracks(Vec<usize>),
}

/// One letter group of one axis.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    /// The character an identifier carries, which is the letter itself or `0` for the rest.
    pub key: char,
    /// What a client shows, which is the letter or `#`.
    pub display: String,
    /// How many values of the axis fall in this group.
    pub values: usize,
}

/// Whether a character may name a letter group, which the wire alphabet decides.
fn group_key(letter: char) -> bool {
    letter == OTHERS || letter.is_ascii_uppercase()
}

/// What a client shows for a letter group, which is the letter or `#` for everything else.
pub fn group_display(key: char) -> String {
    match key {
        OTHERS => "#".to_owned(),
        letter => letter.to_string(),
    }
}

/// The group one value falls into, decided on the same string the listing is ordered by so.
fn group_of(sort: &str) -> char {
    match sort.chars().next() {
        Some(first) if first.is_ascii_alphabetic() => first.to_ascii_uppercase(),
        _ => OTHERS,
    }
}

/// The values of one axis gathered into the groups they fall into, in the order they arrive.
fn grouped(values: &[Entry<'_>]) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for entry in values {
        let key = group_of(entry.sort);
        match groups.last_mut() {
            Some(last) if last.key == key => last.values += 1,
            _ => groups.push(Group {
                key,
                display: group_display(key),
                values: 1,
            }),
        }
    }
    groups
}

/// How many albums a selection may hold before the menus give up and show them.
pub const ALBUM_THRESHOLD: usize = 24;

/// How many values a listing holds before it is worth offering a way to skip through it. Inside.
pub const ALPHA_GROUP: usize = 100;

/// How many of the newest files the recently added menu reaches back over.
pub const RECENT: usize = 300;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Settings {
    pub album_threshold: usize,
    /// How many values a listing must hold before it offers the way into its letter index.
    pub alpha_group: Option<usize>,
    /// How many of the newest files the recently added menu is built from, or nothing to offer none.
    pub recent: Option<usize>,
    /// The axes a menu offers, in the order it offers them.
    pub axes: Vec<Facet>,
    /// Words a listing looks past when ordering a value that has no sort tag.
    pub sort_ignore: Vec<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            album_threshold: ALBUM_THRESHOLD,
            alpha_group: Some(ALPHA_GROUP),
            recent: Some(RECENT),
            axes: DEFAULT_AXES.to_vec(),
            sort_ignore: fold::DEFAULT_SORT_IGNORE
                .iter()
                .map(|word| (*word).to_owned())
                .collect(),
        }
    }
}

impl Settings {
    pub fn ignored(&self) -> fold::Ignored {
        fold::Ignored::new(self.sort_ignore.iter().map(String::as_str))
    }
}

/// The tracks a set of choices selects, as indices into [`Library::tracks`].
pub fn selection(library: &Library, view: &View, position: &Position) -> Option<Vec<usize>> {
    let wanted = resolve(view, &position.chosen)?;
    let within: Option<Vec<usize>> = match &position.scope {
        Some(digest) => Some(scoped(view, digest)?),
        None => None,
    };
    let axes = view.axes();
    let chosen = |at: &usize| {
        wanted
            .iter()
            .all(|(facet, want)| axes.carries(*facet, *at, *want))
    };
    Some(match within {
        Some(tracks) => tracks.into_iter().filter(|at| chosen(at)).collect(),
        None => (0..library.tracks().len()).filter(chosen).collect(),
    })
}

/// Turns each chosen digest back into the value it names.
fn resolve(view: &View, chosen: &[(Facet, String)]) -> Option<Vec<(Facet, u32)>> {
    chosen
        .iter()
        .map(|(facet, wanted)| view.axes().id(*facet, wanted).map(|id| (*facet, id)))
        .collect()
}

/// What to show at a position, or nothing where the position names no object.
pub fn menu<'a>(library: &'a Library, view: &'a View, position: &Position) -> Option<Menu<'a>> {
    let selected = selection(library, view, position)?;
    if let Some(facet) = position.listing {
        let values = view.axes().distinct(facet, &selected);
        return Some(match position.grouped {
            // The listing itself is one sorted list, whatever its length, with the way into.
            None => Menu::Values(facet, values, selected),
            Some(Grouped::Index) => Menu::Letters(facet, grouped(&values)),
            Some(Grouped::Letter(letter)) => Menu::Values(
                facet,
                values
                    .into_iter()
                    .filter(|entry| group_of(entry.sort) == letter)
                    .collect(),
                selected,
            ),
        });
    }
    let made: Vec<Facet> = position.chosen.iter().map(|(facet, _)| *facet).collect();
    let offered = narrowing(library, view, &made, &selected);
    if offered.is_empty() {
        return Some(Menu::Tracks(selected));
    }
    Some(Menu::Facets(
        offered
            .into_iter()
            .map(|facet| {
                let at = position.listing(facet);
                let size = listing_size(view, &at, facet, &selected);
                (facet, at, size)
            })
            .collect(),
    ))
}

/// The axes a selection can still be narrowed along, or none where it is shown as it is.
fn narrowing(library: &Library, view: &View, made: &[Facet], selected: &[usize]) -> Vec<Facet> {
    // The root keeps the menus the owner chose, whatever the library holds: the threshold spares a
    // selection narrowed by hand from one menu too many, and a first sight of the library is not one.
    if !made.is_empty() && !albums_exceed(library, selected, view.settings.album_threshold) {
        return Vec::new();
    }
    view.settings
        .axes
        .iter()
        .copied()
        .filter(|facet| !made.contains(facet))
        .filter(|facet| view.axes().varies(*facet, selected))
        .collect()
}

/// How many children a listing renders: its values, and the way into its letter index.
fn listing_size(view: &View, listing: &Position, facet: Facet, selected: &[usize]) -> usize {
    let count = view.axes().count(facet, selected);
    let long = view
        .settings
        .alpha_group
        .is_some_and(|least| count >= least);
    let indexed = long
        && index_offered(
            listing,
            &view.axes().distinct(facet, selected),
            &view.settings,
        )
        .is_some();
    count + usize::from(indexed)
}

/// How many children each of these values of a listing opens onto, in the order they are given.
pub fn choice_sizes(
    library: &Library,
    view: &View,
    listing: &Position,
    facet: Facet,
    selected: &[usize],
    values: &[Entry<'_>],
) -> Vec<usize> {
    let mut made: Vec<Facet> = listing.chosen.iter().map(|(made, _)| *made).collect();
    made.push(facet);
    let digests: Vec<&str> = values.iter().map(|entry| entry.digest).collect();
    view.axes()
        .holders(facet, selected, &digests)
        .iter()
        .map(|held| match narrowing(library, view, &made, held).len() {
            0 => shown_size(library, held),
            axes => axes,
        })
        .collect()
}

/// How many children a selection shown as it is renders: its albums, then its loose files.
pub fn shown_size(library: &Library, selected: &[usize]) -> usize {
    albums_in(library, selected).len() + loose_tracks(library, selected).len()
}

/// One place the menus put a track: an axis, a value it carries, and the position that lists it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Place {
    pub facet: Facet,
    pub axis: &'static str,
    pub value: String,
    /// What a device browses to reach the value.
    pub at: String,
    /// How many tracks of the library carry it.
    pub tracks: usize,
}

/// Where the menus the owner chose put one track. An axis it carries no value on is left out,
/// which is what a menu it cannot be found under looks like from here.
pub fn places(view: &View, track: usize) -> Vec<Place> {
    let root = Position::default();
    view.settings
        .axes
        .iter()
        .flat_map(|facet| {
            view.axes()
                .of_track(*facet, track)
                .into_iter()
                .map(|value| Place {
                    facet: *facet,
                    axis: facet.title(),
                    value: value.display.to_owned(),
                    at: root.chose(*facet, value.digest).id(),
                    tracks: value.tracks,
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// Whether a listing offers the way into its letter index, and how many letters it holds.
pub fn index_offered(
    position: &Position,
    values: &[Entry<'_>],
    settings: &Settings,
) -> Option<usize> {
    if position.grouped.is_some() {
        return None;
    }
    let least = settings.alpha_group?;
    if values.len() < least {
        return None;
    }
    let letters = grouped(values).len();
    (letters > 1).then_some(letters)
}

/// The title of the container that leads into a listing's letter index.
pub const INDEX_TITLE: &str = "A-Z";

fn untagged_tracks<'a>(library: &'a Library, view: &'a View) -> impl Iterator<Item = usize> + 'a {
    let axes = view.axes();
    (0..library.tracks().len()).filter(move |at| {
        FACETS
            .iter()
            .filter(|facet| facet.from_tags())
            .all(|facet| !axes.reaches(*facet, *at))
    })
}

/// The tracks no tag reaches.
pub fn untagged(library: &Library, view: &View) -> Vec<usize> {
    untagged_tracks(library, view).collect()
}

/// The same, where only how many of them is wanted.
pub fn untagged_count(library: &Library, view: &View) -> usize {
    untagged_tracks(library, view).count()
}

/// One entry of the recently added menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recent {
    /// An index into [`Library::albums`].
    Album(usize),
    /// An index into [`Library::tracks`], for a file that belongs to no album.
    Track(usize),
}

/// What the newest files belong to, newest first, each album once. Empty until the store has
/// stamped a date on something.
pub fn recently_added(library: &Library, settings: &Settings) -> Vec<Recent> {
    let Some(most) = settings.recent else {
        return Vec::new();
    };
    let tracks = library.tracks();
    let mut newest: Vec<(i64, usize)> = tracks
        .iter()
        .enumerate()
        .filter_map(|(at, track)| track.date_added.map(|added| (added, at)))
        .collect();
    // Ties fall to the play order the library already holds, so two files added together keep it.
    newest.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
    let mut entries: Vec<Recent> = Vec::new();
    for (_, at) in newest.into_iter().take(most) {
        let entry = match tracks[at]
            .album_id
            .as_ref()
            .and_then(|id| library.album_index(id))
        {
            Some(album) => Recent::Album(album),
            None => Recent::Track(at),
        };
        if !entries.contains(&entry) {
            entries.push(entry);
        }
    }
    entries
}

/// The albums a selection touches, as indices into [`Library::albums`].
pub fn albums_in(library: &Library, selected: &[usize]) -> Vec<usize> {
    let mut albums: Vec<usize> = selected
        .iter()
        .filter_map(|at| library.tracks()[*at].album_id.as_ref())
        .filter_map(|id| library.album_index(id))
        .collect();
    albums.sort_unstable();
    albums.dedup();
    albums
}

/// Whether a selection holds more albums than `most`, answered without counting the rest.
fn albums_exceed(library: &Library, selected: &[usize], most: usize) -> bool {
    let mut seen: Vec<usize> = Vec::with_capacity(most + 1);
    for at in selected {
        let Some(album) = library.tracks()[*at]
            .album_id
            .as_ref()
            .and_then(|id| library.album_index(id))
        else {
            continue;
        };
        if seen.contains(&album) {
            continue;
        }
        seen.push(album);
        if seen.len() > most {
            return true;
        }
    }
    false
}

/// The tracks a selection holds that belong to no album, as indices into [`Library::tracks`].
pub fn loose_tracks(library: &Library, selected: &[usize]) -> Vec<usize> {
    selected
        .iter()
        .copied()
        .filter(|at| library.tracks()[*at].album_id.is_none())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::Scanned;
    use crate::tags::{AudioProperties, FileTags};
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    fn file(relative: &str, tags: FileTags) -> Scanned {
        Scanned {
            path: Path::new("/music").join(relative),
            relative: PathBuf::from(relative),
            tags,
            properties: AudioProperties {
                duration: Duration::from_secs(180),
                sample_rate: Some(44_100),
                bit_depth: Some(16),
                channels: Some(2),
                bitrate_bps: Some(1_411_000),
            },
            size: 1,
            artwork: None,
        }
    }

    fn tagged(album: &str, title: &str, artist: &str, genre: &str, n: u32) -> FileTags {
        FileTags {
            title: Some(title.to_owned()),
            album: Some(album.to_owned()),
            album_artists: vec![artist.to_owned()],
            artists: vec![artist.to_owned()],
            genres: vec![genre.to_owned()],
            track_number: Some(n),
            ..FileTags::default()
        }
    }

    /// Two genres, three artists, four albums.
    fn library() -> Library {
        Library::build(
            "Music".to_owned(),
            &[
                file(
                    "a/1.flac",
                    tagged("Cuban", "A", "Sierra Maestra", "Latin", 1),
                ),
                file(
                    "a/2.flac",
                    tagged("Cuban", "B", "Sierra Maestra", "Latin", 2),
                ),
                file(
                    "b/1.flac",
                    tagged("Habana", "C", "Sierra Maestra", "Latin", 1),
                ),
                file("c/1.flac", tagged("Dub", "D", "Scientist", "Reggae", 1)),
                file(
                    "d/1.flac",
                    tagged("Roots", "E", "Burning Spear", "Reggae", 1),
                ),
            ],
        )
    }

    /// Settings that never give up early, so a test exercises the rule rather than the threshold.
    fn eager() -> Settings {
        Settings {
            album_threshold: 0,
            ..Settings::default()
        }
    }

    fn eager_view(library: &Library) -> View {
        View::with_settings(library, eager())
    }

    #[test]
    fn an_identifier_survives_a_round_trip() {
        let position = Position {
            scope: None,
            chosen: vec![
                (Facet::Genre, digest("Latin")),
                (Facet::Artist, digest("Sierra Maestra")),
            ],
            listing: Some(Facet::Date),
            grouped: None,
        };
        assert_eq!(Position::parse(&position.id()), Some(position));
    }

    #[test]
    fn the_root_offers_the_menus_chosen_and_leaves_out_an_axis_with_one_value() {
        let library = library();
        let entries = root::entries(&library, &eager_view(&library));
        let titles: Vec<&str> = entries.iter().map(|entry| entry.title.as_str()).collect();
        assert_eq!(titles.first().copied(), Some("4 albums"));
        assert!(titles.contains(&"Genre"), "{titles:?}");
        assert!(titles.contains(&"Artist"), "{titles:?}");
        assert!(
            !titles.contains(&"Quality"),
            "every file is 16 bit 44.1 kHz, so Quality narrows nothing: {titles:?}"
        );
        assert_eq!(titles.last().copied(), Some("[folder view]"));
    }

    #[test]
    fn the_root_position_is_the_bare_prefix() {
        let root = Position::default();
        assert_eq!(root.id(), "f");
        assert_eq!(Position::parse("f"), Some(root));
    }

    #[test]
    fn an_identifier_that_is_not_a_position_is_refused_rather_than_guessed() {
        for id in [
            "",
            "g",
            "al-0123456789abcdef",
            "f-z0123456789abcdef",
            "f-g-a",
            "f-gzz",
            // Choices out of their one order, and one axis chosen twice.
            "f-a0123456789abcdef-g0123456789abcdef",
            "f-g0123456789abcdef-g0123456789abcdef",
        ] {
            assert_eq!(Position::parse(id), None, "{id} should not parse");
        }
    }

    #[test]
    fn two_routes_to_one_set_of_choices_are_one_object() {
        let by_genre = Position::default()
            .narrowed(Facet::Genre, "Reggae")
            .narrowed(Facet::Artist, "Scientist");
        let by_artist = Position::default()
            .narrowed(Facet::Artist, "Scientist")
            .narrowed(Facet::Genre, "Reggae");
        assert_eq!(by_genre.id(), by_artist.id());
        assert_eq!(Position::parse(&by_artist.id()), Some(by_genre));
    }

    #[test]
    fn every_identifier_it_mints_is_legal_on_the_wire() {
        let position = Position::default().narrowed(Facet::Genre, "Rhythm & Blues / Soul");
        assert!(crate::upnp::ObjectId::new(position.id()).is_ok());
    }

    #[test]
    fn the_root_offers_the_axes_that_narrow_something() {
        let library = library();
        match menu(&library, &eager_view(&library), &Position::default())
            .expect("a position naming an object")
        {
            Menu::Facets(offered) => {
                let axes: Vec<Facet> = offered.iter().map(|(facet, _, _)| *facet).collect();
                assert_eq!(axes, vec![Facet::Genre, Facet::Artist, Facet::AllArtists]);
            }
            other => panic!("expected a menu of axes, got {other:?}"),
        }
    }

    #[test]
    fn an_axis_every_track_agrees_on_is_hidden_rather_than_offered() {
        let library = Library::build(
            "Music".to_owned(),
            &[
                file("a/1.flac", tagged("One", "A", "Sierra Maestra", "Latin", 1)),
                file("b/1.flac", tagged("Two", "B", "Sierra Maestra", "Latin", 1)),
            ],
        );
        match menu(&library, &eager_view(&library), &Position::default())
            .expect("a position naming an object")
        {
            Menu::Facets(offered) => assert!(
                offered.is_empty() || !offered.iter().any(|(facet, _, _)| *facet == Facet::Genre),
                "one genre across the library cannot narrow anything"
            ),
            Menu::Tracks(_) => {}
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn choosing_a_genre_drops_the_axes_it_settled() {
        let library = library();
        let reggae = Position::default().narrowed(Facet::Genre, "Reggae");
        match menu(&library, &eager_view(&library), &reggae).expect("a position naming an object") {
            Menu::Facets(offered) => {
                let axes: Vec<Facet> = offered.iter().map(|(facet, _, _)| *facet).collect();
                assert!(!axes.contains(&Facet::Genre));
                assert!(axes.contains(&Facet::Artist));
            }
            other => panic!("expected a menu of axes, got {other:?}"),
        }
    }

    #[test]
    fn narrowing_to_one_artist_leaves_nothing_to_ask() {
        let library = library();
        let position = Position::default()
            .narrowed(Facet::Genre, "Reggae")
            .narrowed(Facet::Artist, "Scientist");
        assert!(matches!(
            menu(&library, &eager_view(&library), &position),
            Some(Menu::Tracks(_))
        ));
    }

    #[test]
    fn listing_an_axis_gives_its_values_sorted_and_counted() {
        let library = library();
        let listing = Position::default().listing(Facet::Genre);
        match menu(&library, &eager_view(&library), &listing).expect("a position naming an object")
        {
            Menu::Values(_, values, _) => {
                let named: Vec<(&str, usize)> = values
                    .iter()
                    .map(|entry| (entry.display, entry.tracks))
                    .collect();
                assert_eq!(named, vec![("Latin", 3), ("Reggae", 2)]);
            }
            other => panic!("expected values, got {other:?}"),
        }
    }

    #[test]
    fn a_listing_inside_a_folder_counts_that_folder_rather_than_the_library() {
        let library = library();
        let listing = Position::inside("a").listing(Facet::Genre);
        match menu(&library, &eager_view(&library), &listing).expect("a position naming an object")
        {
            Menu::Values(_, values, _) => {
                let named: Vec<(&str, usize)> = values
                    .iter()
                    .map(|entry| (entry.display, entry.tracks))
                    .collect();
                assert_eq!(named, vec![("Latin", 2)]);
            }
            other => panic!("expected values, got {other:?}"),
        }
    }

    #[test]
    fn a_listing_is_in_the_order_the_axis_holds_its_values() {
        let library = library();
        let listing = Position::default().listing(Facet::Artist);
        match menu(&library, &eager_view(&library), &listing).expect("a position naming an object")
        {
            Menu::Values(_, values, _) => {
                let shown: Vec<&str> = values.iter().map(|entry| entry.display).collect();
                let mut sorted = shown.clone();
                sorted.sort_by_key(|name| fold(name));
                assert_eq!(shown, sorted);
            }
            other => panic!("expected values, got {other:?}"),
        }
    }

    #[test]
    fn a_selection_holds_only_tracks_satisfying_every_choice() {
        let library = library();
        let position = Position::default()
            .narrowed(Facet::Genre, "Latin")
            .narrowed(Facet::Artist, "Sierra Maestra");
        assert_eq!(
            selection(&library, &eager_view(&library), &position)
                .expect("every choice names a value")
                .len(),
            3
        );
    }

    #[test]
    fn two_spellings_of_one_value_are_one_menu_entry() {
        let library = Library::build(
            "Music".to_owned(),
            &[
                file("a/1.flac", tagged("One", "A", "Sierra Maestra", "Latin", 1)),
                file(
                    "b/1.flac",
                    tagged("Two", "B", "sierra  maestra", "Reggae", 1),
                ),
            ],
        );
        let listing = Position::default().listing(Facet::Artist);
        match menu(&library, &eager_view(&library), &listing).expect("a position naming an object")
        {
            Menu::Values(_, values, _) => assert_eq!(values.len(), 1, "{values:?}"),
            other => panic!("expected values, got {other:?}"),
        }
    }

    #[test]
    fn the_album_threshold_ends_the_menus_early_but_never_at_the_root() {
        let library = library();
        let view = View::build(&library);
        assert!(
            matches!(
                menu(&library, &view, &Position::default()),
                Some(Menu::Facets(_))
            ),
            "the menus an owner chose are what a device meets first, whatever the library holds"
        );
        let narrowed = Position {
            scope: None,
            chosen: vec![(Facet::Genre, digest("Latin"))],
            listing: None,
            grouped: None,
        };
        assert!(
            matches!(menu(&library, &view, &narrowed), Some(Menu::Tracks(_))),
            "and a selection narrowed by hand is spared one menu too many"
        );
    }

    #[test]
    fn a_folder_holds_its_subfolders_and_its_own_tracks() {
        let library = Library::build(
            "Music".to_owned(),
            &[
                file("top.flac", tagged("T", "Top", "A", "Latin", 1)),
                file("jazz/1.flac", tagged("J", "One", "B", "Jazz", 1)),
                file("jazz/deep/2.flac", tagged("D", "Two", "C", "Jazz", 1)),
                file("reggae/3.flac", tagged("R", "Three", "D", "Reggae", 1)),
            ],
        );
        let (folders, tracks) = folder(&eager_view(&library), "");
        let paths: Vec<&str> = folders.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["jazz", "reggae"]);
        assert_eq!(tracks.len(), 1);
        assert_eq!(folders[0].children, 2);

        let (folders, tracks) = folder(&eager_view(&library), "jazz");
        let paths: Vec<&str> = folders.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["jazz/deep"]);
        assert_eq!(tracks.len(), 1);
    }

    #[test]
    fn a_tag_view_inside_a_folder_sees_only_that_folder() {
        let library = Library::build(
            "Music".to_owned(),
            &[
                file("jazz/1.flac", tagged("J", "One", "Miles", "Jazz", 1)),
                file("jazz/2.flac", tagged("J", "Two", "Coltrane", "Jazz", 2)),
                file("reggae/3.flac", tagged("R", "Three", "Tubby", "Reggae", 1)),
            ],
        );
        let inside = Position::inside("jazz");
        assert_eq!(
            selection(&library, &eager_view(&library), &inside)
                .expect("every choice names a value")
                .len(),
            2
        );
        match menu(&library, &eager_view(&library), &inside).expect("a position naming an object") {
            Menu::Facets(offered) => {
                let axes: Vec<Facet> = offered.iter().map(|(f, _, _)| *f).collect();
                assert!(!axes.contains(&Facet::Genre), "{axes:?}");
                assert!(axes.contains(&Facet::Artist));
            }
            other => panic!("expected axes, got {other:?}"),
        }
    }

    #[test]
    fn a_scoped_position_survives_a_round_trip_and_keeps_its_folder() {
        let position = Position::inside("jazz/deep").narrowed(Facet::Genre, "Jazz");
        let read = Position::parse(&position.id()).expect("it parses");
        assert_eq!(read, position);
        assert!(read.scope.is_some());
    }

    #[test]
    fn the_folder_tree_selects_exactly_the_tracks_a_path_test_would() {
        let library = Library::build(
            "Music".to_owned(),
            &[
                file("top.flac", tagged("T", "Top", "A", "Latin", 1)),
                file("jazz/1.flac", tagged("J", "One", "B", "Jazz", 1)),
                file("jazz/deep/2.flac", tagged("D", "Two", "C", "Jazz", 1)),
                file("jazzy/3.flac", tagged("Y", "Three", "D", "Jazz", 1)),
                file("reggae/4.flac", tagged("R", "Four", "E", "Reggae", 1)),
            ],
        );
        for folder in ["", "jazz", "jazz/deep", "jazzy", "reggae"] {
            let expected: Vec<usize> = library
                .tracks()
                .iter()
                .enumerate()
                .filter(|(_, track)| folders::inside(track, folder))
                .map(|(at, _)| at)
                .collect();
            assert_eq!(
                selection(&library, &eager_view(&library), &Position::inside(folder)),
                Some(expected),
                "the tracks inside {folder:?}"
            );
        }
    }

    #[test]
    fn two_folders_whose_names_fold_together_are_two_containers() {
        let library = Library::build(
            "Music".to_owned(),
            &[
                file("Rock/Pop/1.flac", tagged("A", "One", "X", "Rock", 1)),
                file("Rock Pop/2.flac", tagged("B", "Two", "Y", "Pop", 1)),
            ],
        );
        assert_ne!(folder_id("Rock/Pop"), folder_id("Rock Pop"));
        assert_eq!(
            selection(
                &library,
                &eager_view(&library),
                &Position::inside("Rock/Pop")
            ),
            Some(vec![0]),
            "the folder the identifier names, and not the one it folds to"
        );
        assert_eq!(
            selection(
                &library,
                &eager_view(&library),
                &Position::inside("Rock Pop")
            ),
            Some(vec![1])
        );
    }

    #[test]
    fn a_folder_scope_naming_nothing_names_no_object() {
        let library = library();
        assert!(
            selection(
                &library,
                &eager_view(&library),
                &Position::inside("nowhere")
            )
            .is_none()
        );
    }

    #[test]
    fn a_long_axis_is_one_sorted_list_because_that_is_what_the_reference_sends() {
        let files: Vec<_> = (0..40)
            .map(|n| {
                let name = format!("Artist {n:02}");
                file(
                    &format!("a/{n}.flac",),
                    tagged("Album", &format!("T{n}"), &name, "Latin", 1),
                )
            })
            .collect();
        let library = Library::build("Music".to_owned(), &files);
        let settings = Settings {
            album_threshold: 0,
            ..Settings::default()
        };
        let listing = Position::default().listing(Facet::Artist);
        let view = View::with_settings(&library, settings.clone());
        let values = match menu(&library, &view, &listing).expect("a position naming an object") {
            Menu::Values(_, values, _) => values,
            other => panic!("expected one list, got {other:?}"),
        };
        assert_eq!(
            values.len(),
            40,
            "every value is offered, and none is a letter"
        );
    }

    #[test]
    fn the_untagged_are_the_tracks_no_menu_can_reach() {
        let library = Library::build(
            "Music".to_owned(),
            &[
                file("a/1.flac", tagged("One", "A", "Sierra Maestra", "Latin", 1)),
                file(
                    "a/2.flac",
                    FileTags {
                        title: Some("Nameless".to_owned()),
                        ..FileTags::default()
                    },
                ),
                file(
                    "b/1.flac",
                    FileTags {
                        title: Some("Dated".to_owned()),
                        date: Some("1994".to_owned()),
                        ..FileTags::default()
                    },
                ),
            ],
        );
        let untagged: Vec<&str> = untagged(&library, &eager_view(&library))
            .into_iter()
            .map(|at| library.tracks()[at].title.as_str())
            .collect();
        assert_eq!(untagged, vec!["Nameless"]);
    }

    /// One file with the audio properties named rather than the fixture's defaults.
    fn sounding(
        relative: &str,
        tags: FileTags,
        sample_rate: u32,
        bit_depth: u8,
        channels: u8,
    ) -> Scanned {
        Scanned {
            properties: AudioProperties {
                duration: Duration::from_secs(180),
                sample_rate: Some(sample_rate),
                bit_depth: Some(bit_depth),
                channels: Some(channels),
                bitrate_bps: None,
            },
            ..file(relative, tags)
        }
    }

    /// The values one axis offers over the whole library, in the order a listing sends them.
    fn values_of(library: &Library, facet: Facet) -> Vec<String> {
        let all: Vec<usize> = (0..library.tracks().len()).collect();
        View::build(library)
            .axes()
            .distinct(facet, &all)
            .into_iter()
            .map(|entry| entry.display.to_owned())
            .collect()
    }

    fn formats() -> Library {
        Library::build(
            "Music".to_owned(),
            &[
                sounding(
                    "a/1.flac",
                    tagged("Hi", "One", "Someone", "Jazz", 1),
                    96_000,
                    24,
                    2,
                ),
                sounding(
                    "a/2.mp3",
                    tagged("Lo", "Two", "Someone", "Jazz", 2),
                    44_100,
                    16,
                    1,
                ),
                sounding(
                    "a/3.dsf",
                    tagged("Dsd", "Three", "Someone", "Jazz", 3),
                    2_822_400,
                    1,
                    2,
                ),
                sounding(
                    "a/4.wav",
                    tagged("Hi", "Four", "Someone", "Jazz", 4),
                    192_000,
                    24,
                    6,
                ),
            ],
        )
    }

    #[test]
    fn the_file_property_axes_spell_their_values_for_a_listener() {
        let library = formats();
        assert_eq!(
            values_of(&library, Facet::Bits),
            ["1 bit", "16 bit", "24 bit"]
        );
        assert_eq!(
            values_of(&library, Facet::Channels),
            ["Mono", "Stereo", "6 channels"]
        );
        assert_eq!(
            values_of(&library, Facet::Frequency),
            ["44.1 kHz", "96 kHz", "192 kHz", "2822.4 kHz"]
        );
        assert_eq!(
            values_of(&library, Facet::Type),
            ["DSF", "FLAC", "MP3", "WAV"]
        );
    }

    #[test]
    fn a_numeric_axis_is_ordered_by_its_number_and_not_by_its_text() {
        let library = formats();
        let rates = values_of(&library, Facet::Frequency);
        let mut lexical = rates.clone();
        lexical.sort();
        assert_ne!(
            rates, lexical,
            "the test is worthless if the two orders agree"
        );
        assert_eq!(
            rates.first().map(String::as_str),
            Some("44.1 kHz"),
            "192 sorts first as text and last as a number"
        );
    }

    #[test]
    fn a_file_property_is_not_a_tag_so_it_leaves_a_track_untagged() {
        let library = Library::build(
            "Music".to_owned(),
            &[sounding("a/bare.flac", FileTags::default(), 44_100, 16, 2)],
        );
        assert_eq!(
            values_of(&library, Facet::Type),
            ["FLAC"],
            "the file axes reach it"
        );
        assert_eq!(
            untagged(&library, &eager_view(&library)),
            [0],
            "and it is still what `[untagged]` is for"
        );
    }

    #[test]
    fn every_axis_has_a_code_of_its_own_that_reads_back() {
        let mut seen: Vec<char> = FACETS.iter().map(|facet| facet.code()).collect();
        let codes = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), codes, "two axes share a code: {seen:?}");
        for facet in FACETS {
            assert_eq!(Facet::from_code(facet.code()), Some(*facet));
            assert_ne!(facet.code(), SCOPE, "a code may not be the folder marker");
            assert_ne!(
                facet.code(),
                GROUP,
                "a code may not be the letter group marker"
            );
        }
    }

    #[test]
    fn the_axes_are_listed_in_the_order_they_are_declared() {
        for (at, facet) in FACETS.iter().enumerate() {
            assert_eq!(
                *facet as usize, at,
                "an axis is found by its place: {facet:?}"
            );
        }
    }

    /// A library of one artist per letter asked for, plus one starting with a digit.
    fn initials(letters: &[char]) -> Library {
        let files: Vec<Scanned> = letters
            .iter()
            .enumerate()
            .map(|(at, letter)| {
                let artist = format!("{letter}rtist");
                file(
                    &format!("{at}.flac"),
                    tagged("Album", "Title", &artist, "Jazz", at as u32 + 1),
                )
            })
            .collect();
        Library::build("Music".to_owned(), &files)
    }

    fn splitting(least: usize) -> Settings {
        Settings {
            album_threshold: 0,
            alpha_group: Some(least),
            ..Settings::default()
        }
    }

    /// The values a listing holds, whatever the settings say about its index.
    fn listed<'a>(library: &'a Library, view: &'a View, position: &Position) -> Vec<Entry<'a>> {
        match menu(library, view, position).expect("a position naming an object") {
            Menu::Values(_, values, _) => values,
            other => panic!("a listing holds values, not {other:?}"),
        }
    }

    #[test]
    fn a_listing_is_one_sorted_list_however_the_index_is_configured() {
        let library = initials(&['A', 'B', 'C', 'D']);
        let listing = Position::default().listing(Facet::Artist);
        for settings in [eager(), splitting(2), splitting(400)] {
            assert_eq!(
                listed(&library, &View::with_settings(&library, settings), &listing).len(),
                4,
                "the index is offered beside the values and never instead of them"
            );
        }
    }

    #[test]
    fn the_way_into_the_index_is_offered_only_where_it_helps() {
        let library = initials(&['A', 'B', 'C']);
        let listing = Position::default().listing(Facet::Artist);
        let offered = |settings: &Settings| {
            index_offered(
                &listing,
                &listed(
                    &library,
                    &View::with_settings(&library, settings.clone()),
                    &listing,
                ),
                settings,
            )
        };
        assert_eq!(
            offered(&splitting(3)),
            Some(3),
            "three values, three letters"
        );
        assert_eq!(
            offered(&splitting(4)),
            None,
            "a list shorter than the threshold is short enough to read"
        );
        assert_eq!(
            offered(&eager()),
            None,
            "and a threshold of nothing offers it never"
        );
    }

    #[test]
    fn one_letter_is_not_an_index_worth_offering() {
        let library = Library::build(
            "Music".to_owned(),
            &[
                file("1.flac", tagged("A", "T", "ABBA", "Jazz", 1)),
                file("2.flac", tagged("A", "T", "Alva Noto", "Jazz", 2)),
                file("3.flac", tagged("A", "T", "Autechre", "Jazz", 3)),
            ],
        );
        let listing = Position::default().listing(Facet::Artist);
        let settings = splitting(1);
        let view = View::with_settings(&library, settings.clone());
        let values = listed(&library, &view, &listing);
        assert_eq!(values.len(), 3, "three artists, all under one letter");
        assert_eq!(
            index_offered(&listing, &values, &settings),
            None,
            "an index of one entry skips nothing"
        );
    }

    #[test]
    fn the_index_is_not_offered_from_inside_itself() {
        let library = initials(&['A', 'B', 'C']);
        let listing = Position::default().listing(Facet::Artist);
        let settings = splitting(1);
        let inside = listing.at_letter('B');
        assert_eq!(
            index_offered(
                &inside,
                &listed(
                    &library,
                    &View::with_settings(&library, settings.clone()),
                    &inside
                ),
                &settings,
            ),
            None,
            "a letter group does not offer the index it came from"
        );
    }

    #[test]
    fn a_group_holds_the_values_that_start_with_its_letter() {
        let library = initials(&['A', 'B', 'B', 'C']);
        let listing = Position::default().listing(Facet::Artist);
        let view = View::with_settings(&library, splitting(1));
        let Some(Menu::Letters(_, groups)) = menu(&library, &view, &listing.at_index()) else {
            panic!("the index holds letters");
        };
        assert_eq!(
            groups
                .iter()
                .map(|group| (group.display.clone(), group.values))
                .collect::<Vec<(String, usize)>>(),
            // Two artists whose names fold together are one value, so `B` offers one.
            [
                ("A".to_owned(), 1),
                ("B".to_owned(), 1),
                ("C".to_owned(), 1)
            ]
        );
        let inside = listing.at_letter('B');
        let view = View::with_settings(&library, splitting(1));
        let Some(Menu::Values(_, values, _)) = menu(&library, &view, &inside) else {
            panic!("a group holds values");
        };
        assert_eq!(
            values
                .iter()
                .map(|entry| entry.display)
                .collect::<Vec<&str>>(),
            ["Brtist"]
        );
    }

    #[test]
    fn a_value_starting_with_no_letter_falls_into_one_group_of_its_own() {
        let library = Library::build(
            "Music".to_owned(),
            &[
                file("1.flac", tagged("A", "T", "9 Lazy 9", "Jazz", 1)),
                // Punctuation the fold keeps, so this is not under `D` and a listener looks in `#`.
                file("2.flac", tagged("A", "T", "!!!", "Jazz", 2)),
                // Punctuation the fold removes, so the letter behind it is what decides.
                file("3.flac", tagged("A", "T", "_Rtist", "Jazz", 3)),
            ],
        );
        let listing = Position::default().listing(Facet::Artist);
        let view = View::with_settings(&library, splitting(1));
        let Some(Menu::Letters(_, groups)) = menu(&library, &view, &listing.at_index()) else {
            panic!("the index holds letters");
        };
        assert_eq!(
            groups
                .iter()
                .map(|group| (group.key, group.values))
                .collect::<Vec<(char, usize)>>(),
            [(OTHERS, 2), ('R', 1)]
        );
    }

    #[test]
    fn an_accented_value_groups_with_its_letter_rather_than_on_its_own() {
        let library = Library::build(
            "Music".to_owned(),
            &[
                file("1.flac", tagged("A", "T", "Ábaco", "Jazz", 1)),
                file("2.flac", tagged("A", "T", "Zoe", "Jazz", 2)),
            ],
        );
        let listing = Position::default().listing(Facet::Artist);
        let view = View::with_settings(&library, splitting(1));
        let Some(Menu::Letters(_, groups)) = menu(&library, &view, &listing.at_index()) else {
            panic!("the index holds letters");
        };
        assert_eq!(
            groups.iter().map(|group| group.key).collect::<Vec<char>>(),
            ['A', 'Z'],
            "the fold decides the group, so an accent is its unaccented letter"
        );
    }

    #[test]
    fn a_group_survives_a_round_trip_and_a_bad_one_is_refused() {
        let index = Position::default().listing(Facet::Artist).at_index();
        assert_eq!(Position::parse(&index.id()), Some(index.clone()));
        let inside = Position::default().listing(Facet::Artist).at_letter('B');
        assert_eq!(Position::parse(&inside.id()), Some(inside.clone()));
        assert!(
            Position::parse(&format!("{PREFIX}-l B")).is_none(),
            "a group is one character of the wire alphabet"
        );
        assert!(
            Position::parse(&format!("{PREFIX}-lb")).is_none(),
            "a lower case letter is not a group key"
        );
        assert!(
            Position::parse(&format!("{PREFIX}-lA")).is_none(),
            "a group outside a listing is not a position"
        );
        assert!(
            Position::parse(&format!("{PREFIX}-l")).is_none(),
            "and neither is the index outside one"
        );
        assert!(
            Position::parse(&format!("{}-lA", inside.id())).is_none(),
            "and there is only ever one of them"
        );
        assert!(
            Position::parse(&format!("{}-lA", index.id())).is_none(),
            "the index is not a letter and a letter is not inside it"
        );
    }

    #[test]
    fn an_unmapped_type_shows_itself_rather_than_nothing() {
        assert_eq!(format_name("audio/x-flac"), "FLAC");
        assert_eq!(format_name("audio/x-newthing"), "audio/x-newthing");
    }

    #[test]
    fn a_digest_naming_nothing_names_no_object_rather_than_everything() {
        let library = library();
        let stale = Position {
            chosen: vec![(Facet::Genre, digest("Ambient"))],
            ..Position::default()
        };
        assert!(selection(&library, &eager_view(&library), &stale).is_none());
    }

    #[test]
    fn a_value_identifier_does_not_move_when_another_value_appears() {
        let before = Position::default().narrowed(Facet::Genre, "Reggae").id();
        let after = Position::default().narrowed(Facet::Genre, "Reggae").id();
        assert_eq!(before, after);
        assert_ne!(
            before,
            Position::default().narrowed(Facet::Genre, "Latin").id()
        );
    }

    #[test]
    fn a_value_that_folds_to_nothing_leaves_a_track_untagged() {
        let library = Library::build(
            "Music".to_owned(),
            &[
                file(
                    "blank/1.flac",
                    FileTags {
                        genres: vec!["   ".to_owned()],
                        ..FileTags::default()
                    },
                ),
                file(
                    "real/2.flac",
                    FileTags {
                        genres: vec!["Son".to_owned()],
                        ..FileTags::default()
                    },
                ),
            ],
        );
        assert_eq!(untagged(&library, &eager_view(&library)), vec![0]);
    }

    fn values_shown(library: &Library, facet: Facet) -> Vec<String> {
        let view = eager_view(library);
        let all: Vec<usize> = (0..library.tracks().len()).collect();
        view.axes()
            .distinct(facet, &all)
            .into_iter()
            .map(|entry| entry.display.to_owned())
            .collect()
    }

    #[test]
    fn the_date_axis_offers_the_year_wherever_the_tagger_put_it() {
        let dated = |relative: &str, date: &str| {
            file(
                relative,
                FileTags {
                    date: Some(date.to_owned()),
                    ..FileTags::default()
                },
            )
        };
        let library = Library::build(
            "Music".to_owned(),
            &[
                dated("a/1.flac", "1963-04-12"),
                dated("b/1.flac", "1963"),
                dated("c/1.flac", "12/04/1975"),
                dated("d/1.flac", "unknown"),
            ],
        );
        assert_eq!(values_shown(&library, Facet::Date), ["1963", "1975"]);
        assert_eq!(year("1963-04-12"), Some("1963"));
        assert_eq!(year("196"), None);
    }

    #[test]
    fn a_file_is_summed_up_in_one_quality_word_and_the_words_climb() {
        let shaped = |relative: &str, rate: u32, bits: Option<u8>| {
            let mut scanned = file(relative, FileTags::default());
            scanned.properties.sample_rate = Some(rate);
            scanned.properties.bit_depth = bits;
            scanned
        };
        let library = Library::build(
            "Music".to_owned(),
            &[
                shaped("a/1.flac", 22_050, Some(16)),
                shaped("b/1.dsf", 2_822_400 * 2, Some(1)),
                shaped("c/1.flac", 192_000, Some(24)),
                shaped("d/1.flac", 96_000, Some(24)),
                shaped("d/2.flac", 96_000, Some(16)),
                shaped("e/1.flac", 48_000, Some(24)),
                shaped("e/2.flac", 44_100, Some(24)),
                shaped("f/1.flac", 44_100, Some(16)),
                shaped("g/1.mp3", 44_100, None),
                shaped("h/1.m4a", 44_100, None),
                shaped("i/1.m4a", 44_100, Some(16)),
            ],
        );
        assert_eq!(
            values_shown(&library, Facet::Quality),
            ["Lossy", "Below CD", "CD", "CD+", "HD", "HD+", "DSD128"],
            "worst to best rather than alphabetical, and an AAC file is lossy where an ALAC one is not"
        );
    }
}
