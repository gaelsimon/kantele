//! Folded text a `Search` criterion reads.

use std::collections::HashMap;

use crate::index::credits::Credit;
use crate::index::fold;
use crate::index::library::{Album, Artist, Track};
use crate::index::playlist::Playlist;
use crate::object;

#[derive(Clone, Debug, Default)]
pub struct Searchables {
    pub tracks: Searchable,
    pub albums: Searchable,
    pub artists: Searchable,
    pub playlists: Searchable,
}

impl Searchables {
    /// `aliases`: per track, the text playlists wrote beside it.
    pub fn build(
        tracks: &[Track],
        albums: &[Album],
        artists: &[Artist],
        playlists: &[Playlist],
        aliases: &[Vec<String>],
    ) -> Self {
        Self {
            tracks: Searchable::build(
                object::MUSIC_TRACK,
                tracks.iter().enumerate().map(|(at, track)| Raw {
                    title: &track.title,
                    aliases: aliases.get(at).map_or(&[][..], Vec::as_slice),
                    album: track.album.as_deref(),
                    artists: &track.artists,
                    album_artists: &track.album_artists,
                    composers: &track.composers,
                    genres: &track.genres,
                    date: track.date.as_deref(),
                }),
            ),
            albums: Searchable::build(
                object::ALBUM,
                albums.iter().map(|album| Raw {
                    title: &album.title,
                    aliases: &[],
                    album: Some(&album.title),
                    artists: &album.artists,
                    album_artists: if album.credited { &album.artists } else { &[] },
                    composers: &[],
                    genres: &album.genres,
                    date: album.date.as_deref(),
                }),
            ),
            artists: Searchable::build(
                object::ARTIST,
                artists.iter().map(|artist| Raw {
                    title: &artist.name,
                    album: Some(&artist.name),
                    ..Raw::default()
                }),
            ),
            playlists: Searchable::build(
                object::PLAYLIST,
                playlists.iter().map(|playlist| Raw {
                    title: &playlist.title,
                    album: Some(&playlist.title),
                    ..Raw::default()
                }),
            ),
        }
    }

    pub fn footprint(&self) -> usize {
        self.tracks.footprint()
            + self.albums.footprint()
            + self.artists.footprint()
            + self.playlists.footprint()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Title,
    Album,
    Artists,
    AlbumArtists,
    Composers,
    Genres,
    Date,
    /// `dc:creator`, the first artist.
    Creator,
}

const STORED: [Field; 7] = [
    Field::Title,
    Field::Album,
    Field::Artists,
    Field::AlbumArtists,
    Field::Composers,
    Field::Genres,
    Field::Date,
];

impl Field {
    const fn stored(self) -> usize {
        match self {
            Self::Title => 0,
            Self::Album => 1,
            Self::Artists | Self::Creator => 2,
            Self::AlbumArtists => 3,
            Self::Composers => 4,
            Self::Genres => 5,
            Self::Date => 6,
        }
    }

    const fn most(self) -> u32 {
        match self {
            Self::Creator => 1,
            _ => u32::MAX,
        }
    }
}

pub trait Fields {
    /// The folded `upnp:class`.
    fn class(&self) -> &str;

    fn any(&self, field: Field, test: &mut dyn FnMut(&str) -> bool) -> bool;

    fn present(&self, field: Field) -> bool;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Raw<'a> {
    pub title: &'a str,
    pub aliases: &'a [String],
    pub album: Option<&'a str>,
    pub artists: &'a [Credit],
    pub album_artists: &'a [Credit],
    pub composers: &'a [Credit],
    pub genres: &'a [String],
    pub date: Option<&'a str>,
}

impl<'a> Raw<'a> {
    fn stored(&self, at: usize) -> Values<'a> {
        match STORED[at] {
            Field::Title => Values::Both(Some(self.title), self.aliases),
            Field::Album => Values::One(self.album),
            Field::Artists => Values::Credited(self.artists),
            Field::AlbumArtists => Values::Credited(self.album_artists),
            Field::Composers => Values::Credited(self.composers),
            Field::Genres => Values::Many(self.genres),
            _ => Values::One(self.date),
        }
    }
}

enum Values<'a> {
    Many(&'a [String]),
    Credited(&'a [Credit]),
    One(Option<&'a str>),
    Both(Option<&'a str>, &'a [String]),
}

impl<'a> Values<'a> {
    fn iter(&self) -> impl Iterator<Item = &'a str> {
        let (many, credited, one) = match self {
            Self::Many(values) => (*values, &[][..], None),
            Self::Credited(values) => (&[][..], *values, None),
            Self::One(value) => (&[][..], &[][..], *value),
            Self::Both(value, values) => (*values, &[][..], *value),
        };
        many.iter()
            .map(String::as_str)
            .chain(credited.iter().map(|credit| credit.name.as_str()))
            .chain(one)
    }
}

/// Start and length.
type Span = (u32, u32);

#[derive(Clone, Debug, Default)]
pub struct Searchable {
    class: String,
    text: String,
    /// Spans into `text`.
    values: Vec<Span>,
    /// Spans into `values`, one per stored field.
    fields: Vec<[Span; STORED.len()]>,
}

impl Searchable {
    pub fn build<'a>(class: &str, objects: impl Iterator<Item = Raw<'a>>) -> Self {
        let mut table = Self {
            class: fold(class),
            ..Self::default()
        };
        // Keyed on raw text first so a repeated value is folded once.
        let mut by_raw: HashMap<&'a str, Span> = HashMap::new();
        let mut by_folded: HashMap<String, Span> = HashMap::new();
        for raw in objects {
            let mut spans = [(0u32, 0u32); STORED.len()];
            for (at, span) in spans.iter_mut().enumerate() {
                let start = table.values.len() as u32;
                for value in raw.stored(at).iter() {
                    let held = match by_raw.get(value) {
                        Some(held) => *held,
                        None => {
                            let folded = fold(value);
                            let held = match by_folded.get(&folded) {
                                Some(held) => *held,
                                None => {
                                    let from = table.text.len() as u32;
                                    table.text.push_str(&folded);
                                    let held = (from, folded.len() as u32);
                                    by_folded.insert(folded, held);
                                    held
                                }
                            };
                            by_raw.insert(value, held);
                            held
                        }
                    };
                    table.values.push(held);
                }
                *span = (start, table.values.len() as u32 - start);
            }
            table.fields.push(spans);
        }
        table.text.shrink_to_fit();
        table.values.shrink_to_fit();
        table.fields.shrink_to_fit();
        table
    }

    pub fn len(&self) -> usize {
        self.fields.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    pub fn object(&self, at: usize) -> Object<'_> {
        Object { table: self, at }
    }

    pub fn footprint(&self) -> usize {
        self.class.capacity()
            + self.text.capacity()
            + self.values.capacity() * 8
            + self.fields.capacity() * 8 * STORED.len()
    }

    fn span(&self, at: usize, field: Field) -> Span {
        match self.fields.get(at) {
            Some(spans) => {
                let (start, len) = spans[field.stored()];
                (start, len.min(field.most()))
            }
            None => (0, 0),
        }
    }

    fn value(&self, at: usize) -> &str {
        let (start, len) = self.values[at];
        &self.text[start as usize..(start + len) as usize]
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Object<'a> {
    table: &'a Searchable,
    at: usize,
}

impl Fields for Object<'_> {
    fn class(&self) -> &str {
        &self.table.class
    }

    fn any(&self, field: Field, test: &mut dyn FnMut(&str) -> bool) -> bool {
        let (start, len) = self.table.span(self.at, field);
        (start..start + len).any(|at| test(self.table.value(at as usize)))
    }

    fn present(&self, field: Field) -> bool {
        self.table.span(self.at, field).1 > 0
    }
}

/// Folded per query, for a container too small to index.
#[derive(Clone, Debug, Default)]
pub struct Folded {
    class: String,
    values: Vec<(usize, String)>,
}

impl Folded {
    pub fn of(class: &str, raw: Raw<'_>) -> Self {
        let mut values = Vec::new();
        for at in 0..STORED.len() {
            for value in raw.stored(at).iter() {
                values.push((at, fold(value)));
            }
        }
        Self {
            class: fold(class),
            values,
        }
    }

    fn of_field(&self, field: Field) -> impl Iterator<Item = &str> {
        self.values
            .iter()
            .filter(move |(at, _)| *at == field.stored())
            .map(|(_, value)| value.as_str())
            .take(field.most() as usize)
    }
}

impl Fields for Folded {
    fn class(&self) -> &str {
        &self.class
    }

    fn any(&self, field: Field, test: &mut dyn FnMut(&str) -> bool) -> bool {
        self.of_field(field).any(test)
    }

    fn present(&self, field: Field) -> bool {
        self.of_field(field).next().is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CLASS: &str = "object.item.audioItem.musicTrack";

    fn raw<'a>(title: &'a str, artists: &'a [Credit], genres: &'a [String]) -> Raw<'a> {
        Raw {
            title,
            artists,
            genres,
            ..Raw::default()
        }
    }

    fn read(fields: &impl Fields, field: Field) -> Vec<String> {
        let mut seen = Vec::new();
        fields.any(field, &mut |value: &str| {
            seen.push(value.to_owned());
            false
        });
        seen
    }

    #[test]
    fn the_class_is_folded_with_the_table_rather_than_per_query() {
        let none: Vec<String> = Vec::new();
        let nobody: Vec<Credit> = Vec::new();
        let table = Searchable::build(
            "Object.Container.Album",
            [raw("x", &nobody, &none)].into_iter(),
        );
        assert_eq!(table.object(0).class(), "object.container.album");
        assert_eq!(
            Folded::of("Object.Container.Album", Raw::default()).class(),
            "object.container.album"
        );
    }

    #[test]
    fn the_table_holds_what_a_query_will_compare_against() {
        let artists = vec![Credit::new("Antonín Dvořák"), Credit::new("Wu-Tang Clan")];
        let genres = vec!["Classical".to_owned()];
        let table = Searchable::build(CLASS, [raw("Élégie", &artists, &genres)].into_iter());
        assert_eq!(table.len(), 1);
        let object = table.object(0);
        assert_eq!(read(&object, Field::Title), vec!["elegie"]);
        assert_eq!(
            read(&object, Field::Artists),
            vec!["antonin dvorak", "wu tang clan"]
        );
        assert_eq!(
            read(&object, Field::Creator),
            vec!["antonin dvorak"],
            "dc:creator is the first artist and no others"
        );
        assert_eq!(read(&object, Field::Genres), vec!["classical"]);
        assert!(read(&object, Field::Album).is_empty());
        assert!(!object.present(Field::Album));
        assert!(object.present(Field::Title));
    }

    #[test]
    fn folding_on_the_spot_and_folding_at_index_time_agree() {
        let artists = vec![Credit::new("Sierra Maestra"), Credit::new("Juan de Marcos")];
        let genres = vec!["Son Cubano".to_owned()];
        let one = Raw {
            album: Some("¡Dundunbanza!"),
            date: Some("1994"),
            ..raw("Juana Peña", &artists, &genres)
        };
        let table = Searchable::build(CLASS, [one].into_iter());
        let owned = Folded::of(CLASS, one);
        for field in [
            Field::Title,
            Field::Album,
            Field::Artists,
            Field::AlbumArtists,
            Field::Composers,
            Field::Genres,
            Field::Date,
            Field::Creator,
        ] {
            assert_eq!(
                read(&table.object(0), field),
                read(&owned, field),
                "{field:?}"
            );
            assert_eq!(
                table.object(0).present(field),
                owned.present(field),
                "{field:?}"
            );
        }
    }

    #[test]
    fn objects_do_not_read_each_other_s_values() {
        let first = vec![Credit::new("Scientist")];
        let second = vec![Credit::new("Burning Spear")];
        let none: Vec<String> = Vec::new();
        let table = Searchable::build(
            CLASS,
            [raw("Dub", &first, &none), raw("Roots", &second, &none)].into_iter(),
        );
        assert_eq!(read(&table.object(0), Field::Artists), vec!["scientist"]);
        assert_eq!(
            read(&table.object(1), Field::Artists),
            vec!["burning spear"]
        );
        assert_eq!(read(&table.object(0), Field::Title), vec!["dub"]);
        assert!(
            read(&table.object(2), Field::Artists).is_empty(),
            "past the end reads nothing rather than the last object"
        );
    }

    #[test]
    fn a_match_stops_reading_the_rest() {
        let artists = vec![Credit::new("A"), Credit::new("B"), Credit::new("C")];
        let none: Vec<String> = Vec::new();
        let table = Searchable::build(CLASS, [raw("t", &artists, &none)].into_iter());
        let mut seen = 0;
        assert!(table.object(0).any(Field::Artists, &mut |value| {
            seen += 1;
            value == "b"
        }));
        assert_eq!(seen, 2);
    }
}
