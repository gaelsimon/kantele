//! Reading what is on disk, and nothing else.

pub mod dsd;
mod vorbis;

use std::path::Path;
use std::time::Duration;

use lofty::config::ParseOptions;
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::prelude::ItemKey;
use lofty::probe::Probe;

/// What one file says about itself, before any decision about identity or containment.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct FileTags {
    pub title: Option<String>,
    /// The credited artists. `ARTISTS` when the tagger wrote it.
    pub artists: Vec<String>,
    pub album_artists: Vec<String>,
    /// The sort spelling of each artist.
    pub artist_sorts: Vec<String>,
    /// Positionally aligned with `album_artists` on the same terms.
    pub album_artist_sorts: Vec<String>,
    /// Positionally aligned with `composers` on the same terms.
    pub composer_sorts: Vec<String>,
    pub album: Option<String>,
    pub composers: Vec<String>,
    pub genres: Vec<String>,
    pub date: Option<String>,
    pub track_number: Option<u32>,
    pub track_total: Option<u32>,
    pub disc_number: Option<u32>,
    pub disc_total: Option<u32>,
    pub disc_subtitle: Option<String>,
    /// The work this track belongs to, read here and indexed with the facets.
    pub work: Option<String>,
    pub movement_name: Option<String>,
    pub movement_number: Option<u32>,
    /// `GROUPING`, which shares the `TIT1` frame with the work in ID3 and is not the same field.
    pub grouping: Option<String>,
    /// The tagger's own statement that this release has no one artist.
    pub compilation: bool,
    /// Present on roughly one file in forty in a real library, so never depended upon.
    pub musicbrainz_release_id: Option<String>,
    /// The only identifier meaning "this track on this release", from `MUSICBRAINZ_RELEASETRACKID`.
    pub musicbrainz_release_track_id: Option<String>,
    /// Named `MUSICBRAINZ_TRACKID` on disk and holding the recording, which many releases share.
    pub musicbrainz_recording_id: Option<String>,
    /// Positionally aligned with `artists` when the tagger wrote both.
    pub musicbrainz_artist_ids: Vec<String>,
    /// Positionally aligned with `album_artists` on the same terms.
    pub musicbrainz_album_artist_ids: Vec<String>,
}

impl FileTags {
    /// A whitespace-only value is no value and takes the entries aligned with it. What remains is
    /// published in NFC.
    pub fn tidied(mut self) -> Self {
        for field in [
            &mut self.title,
            &mut self.album,
            &mut self.date,
            &mut self.disc_subtitle,
            &mut self.work,
            &mut self.movement_name,
            &mut self.grouping,
        ] {
            if field
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
            {
                *field = None;
            }
            if let Some(value) = field {
                composed(value);
            }
        }
        for list in [
            &mut self.artists,
            &mut self.album_artists,
            &mut self.artist_sorts,
            &mut self.album_artist_sorts,
            &mut self.composers,
            &mut self.composer_sorts,
            &mut self.genres,
        ] {
            list.iter_mut().for_each(composed);
        }
        self.genres.retain(|genre| !genre.trim().is_empty());
        drop_blank(
            &mut self.artists,
            &mut [&mut self.artist_sorts, &mut self.musicbrainz_artist_ids],
        );
        drop_blank(
            &mut self.album_artists,
            &mut [
                &mut self.album_artist_sorts,
                &mut self.musicbrainz_album_artist_ids,
            ],
        );
        drop_blank(&mut self.composers, &mut [&mut self.composer_sorts]);
        self
    }
}

fn composed(value: &mut String) {
    use unicode_normalization::UnicodeNormalization;
    if !unicode_normalization::is_nfc(value) {
        *value = value.nfc().collect();
    }
}

fn drop_blank(names: &mut Vec<String>, aligned: &mut [&mut Vec<String>]) {
    let mut at = 0;
    while at < names.len() {
        if names[at].trim().is_empty() {
            names.remove(at);
            for list in aligned.iter_mut() {
                if at < list.len() {
                    list.remove(at);
                }
            }
        } else {
            at += 1;
        }
    }
}

#[derive(Clone, Copy, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct AudioProperties {
    pub duration: Duration,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u8>,
    pub channels: Option<u8>,
    pub bitrate_bps: Option<u32>,
}

#[derive(Debug, thiserror::Error)]
#[error("reading {path}")]
pub struct TagError {
    pub path: String,
    #[source]
    pub source: lofty::error::FileParseError,
}

/// Reads one file's tags and audio properties.
pub fn read(path: &Path) -> Result<(FileTags, AudioProperties), TagError> {
    let (tags, properties) = read_as_written(path)?;
    Ok((tags.tidied(), properties))
}

fn read_as_written(path: &Path) -> Result<(FileTags, AudioProperties), TagError> {
    if let Some(dsd) = dsd::read(path) {
        let tags = dsd.tagged.as_ref().map(tags_of).unwrap_or_default();
        return Ok((tags, dsd.properties));
    }
    match probe(path, ParseOptions::new()) {
        Ok(tagged) => Ok((tags_from(path, &tagged), properties_of(&tagged))),
        Err(refused) => read_again(path, refused),
    }
}

/// One file's tags, with the ones the tag crate discards read from the container. `GROUP` names
/// the run a track belongs to, beside the `GROUPING` the crate reads.
fn tags_from(path: &Path, tagged: &lofty::file::TaggedFile) -> FileTags {
    let mut tags = tags_of(tagged);
    let wanted = tags.composer_sorts.is_empty() || tags.grouping.is_none();
    if wanted
        && tagged.file_type() == lofty::file::FileType::Flac
        && let Some(block) = vorbis::block(path)
    {
        if tags.composer_sorts.is_empty() {
            tags.composer_sorts = vorbis::values(&block, "COMPOSERSORT");
        }
        if tags.grouping.is_none() {
            tags.grouping = vorbis::values(&block, "GROUP").into_iter().next();
        }
    }
    tags
}

/// Two things go wrong on a real library, and the file is worth serving after either.
fn read_again(path: &Path, refused: TagError) -> Result<(FileTags, AudioProperties), TagError> {
    if let Ok(tagged) = probe_sniffed(path, ParseOptions::new()) {
        tracing::warn!(
            path = %path.display(),
            found = ?tagged.file_type(),
            "the extension does not match the bytes; read as the format they say"
        );
        return Ok((tags_from(path, &tagged), properties_of(&tagged)));
    }
    tracing::warn!(
        path = %path.display(),
        cause = %causes(&refused),
        "tags will not parse; serving the file without them"
    );
    let tagged = probe(path, ParseOptions::new().read_tags(false))?;
    Ok((FileTags::default(), properties_of(&tagged)))
}

fn probe(path: &Path, options: ParseOptions) -> Result<lofty::file::TaggedFile, TagError> {
    Probe::open(path)
        .and_then(|probe| probe.options(options).read())
        .map_err(|source| refusal(path, source))
}

/// The same read with the format taken from the first bytes rather than from the name.
fn probe_sniffed(path: &Path, options: ParseOptions) -> Result<lofty::file::TaggedFile, TagError> {
    let probe = Probe::open(path).map_err(|source| refusal(path, source))?;
    let probe = probe
        .options(options)
        .guess_file_type()
        .map_err(|source| refusal(path, lofty::error::FileParseError::from(source)))?;
    probe.read().map_err(|source| refusal(path, source))
}

fn refusal(path: &Path, source: lofty::error::FileParseError) -> TagError {
    TagError {
        path: path.display().to_string(),
        source,
    }
}

fn causes(error: &dyn std::error::Error) -> String {
    let mut parts = vec![error.to_string()];
    let mut source = error.source();
    while let Some(inner) = source {
        parts.push(inner.to_string());
        source = inner.source();
    }
    parts.join(": ")
}

fn properties_of(tagged: &lofty::file::TaggedFile) -> AudioProperties {
    let p = tagged.properties();
    AudioProperties {
        duration: p.duration(),
        sample_rate: p.sample_rate(),
        bit_depth: p.bit_depth(),
        channels: p.channels(),
        bitrate_bps: p.audio_bitrate().map(|kbps| kbps * 1000),
    }
}

fn tags_of(tagged: &lofty::file::TaggedFile) -> FileTags {
    let Some(tag) = tagged.primary_tag().or_else(|| tagged.first_tag()) else {
        return FileTags::default();
    };

    let many = |key: ItemKey| -> Vec<String> { once_each(tag.get_strings(key)) };
    let one = |key: ItemKey| -> Option<String> { tag.get_string(key).map(str::to_owned) };

    FileTags {
        title: one(ItemKey::TrackTitle),
        artists: credited(&many(ItemKey::TrackArtists), &many(ItemKey::TrackArtist)),
        album_artists: credited(&many(ItemKey::AlbumArtists), &many(ItemKey::AlbumArtist)),
        artist_sorts: many(ItemKey::TrackArtistSortOrder),
        album_artist_sorts: many(ItemKey::AlbumArtistSortOrder),
        composer_sorts: many(ItemKey::ComposerSortOrder),
        album: one(ItemKey::AlbumTitle),
        composers: many(ItemKey::Composer),
        genres: many(ItemKey::Genre),
        date: one(ItemKey::RecordingDate).or_else(|| one(ItemKey::Year)),
        track_number: one(ItemKey::TrackNumber).and_then(|v| leading_number(&v)),
        track_total: one(ItemKey::TrackTotal).and_then(|v| leading_number(&v)),
        disc_number: one(ItemKey::DiscNumber).and_then(|v| leading_number(&v)),
        disc_total: one(ItemKey::DiscTotal).and_then(|v| leading_number(&v)),
        disc_subtitle: one(ItemKey::SetSubtitle),
        work: one(ItemKey::Work),
        movement_name: one(ItemKey::Movement),
        movement_number: one(ItemKey::MovementNumber).and_then(|v| leading_number(&v)),
        grouping: one(ItemKey::ContentGroup),
        compilation: flag(one(ItemKey::FlagCompilation)),
        musicbrainz_release_id: one(ItemKey::MusicBrainzReleaseId).map(canonical),
        musicbrainz_release_track_id: one(ItemKey::MusicBrainzTrackId).map(canonical),
        musicbrainz_recording_id: one(ItemKey::MusicBrainzRecordingId).map(canonical),
        musicbrainz_artist_ids: many(ItemKey::MusicBrainzArtistId)
            .into_iter()
            .map(canonical)
            .collect(),
        musicbrainz_album_artist_ids: many(ItemKey::MusicBrainzReleaseArtistId)
            .into_iter()
            .map(canonical)
            .collect(),
    }
}

/// The values of one tag, with an exact repeat kept once.
fn once_each<'a>(values: impl Iterator<Item = &'a str>) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for value in values {
        if !kept.iter().any(|seen| seen == value) {
            kept.push(value.to_owned());
        }
    }
    kept
}

/// The list a tagger meant. `ARTISTS` and `ALBUMARTISTS` hold one value per person.
fn credited(split: &[String], joined: &[String]) -> Vec<String> {
    if split.is_empty() {
        return joined.to_vec();
    }
    split.to_vec()
}

/// An identifier in the one spelling it has.
fn canonical(value: String) -> String {
    value.to_ascii_lowercase()
}

/// A boolean tag, which taggers write as `1`, `true` or `yes` and rarely as anything else.
fn flag(value: Option<String>) -> bool {
    value.is_some_and(|value| {
        matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes"
        )
    })
}

fn leading_number(value: &str) -> Option<u32> {
    let digits: String = value
        .trim()
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blank_value_is_no_value_and_takes_its_aligned_entries_with_it() {
        let tags = FileTags {
            title: Some("  ".to_owned()),
            album: Some(String::new()),
            artists: vec![String::new(), "Sierra Maestra".to_owned()],
            artist_sorts: vec!["ignored".to_owned(), "Maestra, Sierra".to_owned()],
            musicbrainz_artist_ids: vec!["mbid-of-nobody".to_owned()],
            composers: vec!["Coltrane".to_owned(), " ".to_owned()],
            composer_sorts: vec!["Coltrane, John".to_owned(), "Nobody".to_owned()],
            genres: vec!["".to_owned(), "Son".to_owned()],
            work: Some("Kind of Blue".to_owned()),
            ..FileTags::default()
        }
        .tidied();
        assert_eq!(tags.title, None);
        assert_eq!(tags.album, None);
        assert_eq!(tags.artists, vec!["Sierra Maestra".to_owned()]);
        assert_eq!(tags.artist_sorts, vec!["Maestra, Sierra".to_owned()]);
        assert!(tags.musicbrainz_artist_ids.is_empty());
        assert_eq!(tags.composers, vec!["Coltrane".to_owned()]);
        assert_eq!(tags.composer_sorts, vec!["Coltrane, John".to_owned()]);
        assert_eq!(tags.genres, vec!["Son".to_owned()]);
        assert_eq!(tags.work.as_deref(), Some("Kind of Blue"));
    }

    #[test]
    fn a_decomposed_spelling_is_published_composed_in_every_text_field() {
        let tags = FileTags {
            title: Some("Jo\u{301}ga".to_owned()),
            album: Some("Homogenic".to_owned()),
            artists: vec!["Bjo\u{308}rk".to_owned()],
            genres: vec!["E\u{301}lectronique".to_owned()],
            ..FileTags::default()
        }
        .tidied();
        assert_eq!(tags.title.as_deref(), Some("Jóga"));
        assert_eq!(tags.artists, vec!["Björk".to_owned()]);
        assert_eq!(tags.genres, vec!["Électronique".to_owned()]);
        assert_eq!(tags.album.as_deref(), Some("Homogenic"));
    }

    #[test]
    fn a_tag_listing_one_value_twice_says_it_once() {
        assert_eq!(
            once_each(["Gotan Project", "Gotan Project"].into_iter()),
            ["Gotan Project"]
        );
        assert_eq!(
            once_each(["Simon", "Garfunkel", "Simon"].into_iter()),
            ["Simon", "Garfunkel"],
            "a repeat is dropped where it sits, and the order the tagger wrote is kept"
        );
        assert_eq!(
            once_each(["Bob Marley", "bob marley"].into_iter()),
            ["Bob Marley", "bob marley"],
            "two spellings are two spellings here and are merged by the axis, not by this"
        );
    }

    #[test]
    fn the_tag_a_tagger_already_split_is_the_one_that_is_read() {
        let split = vec!["Simon".to_owned(), "Garfunkel".to_owned()];
        let joined = vec!["Simon & Garfunkel".to_owned()];
        assert_eq!(credited(&split, &joined), split);
        assert_eq!(credited(&[], &joined), joined);
        assert!(credited(&[], &[]).is_empty());
    }

    #[test]
    fn an_identifier_is_held_in_one_case_so_it_keys_one_identity() {
        let upper = "C2B3D0F1-4A5E-4B6C-8D9E-0F1A2B3C4D5E";
        assert_eq!(canonical(upper.to_owned()), upper.to_ascii_lowercase());
        assert_eq!(
            canonical("Unknown".to_owned()),
            "unknown",
            "a placeholder still reaches the shape check that refuses it"
        );
    }

    #[test]
    fn a_compilation_flag_is_read_in_the_shapes_taggers_write() {
        for value in ["1", "true", "TRUE", "yes", " 1 "] {
            assert!(flag(Some(value.to_owned())), "{value}");
        }
        for value in ["0", "false", "no", "", "  "] {
            assert!(!flag(Some(value.to_owned())), "{value}");
        }
        assert!(!flag(None));
    }

    #[test]
    fn track_numbers_arrive_in_several_shapes() {
        assert_eq!(leading_number("4"), Some(4));
        assert_eq!(leading_number("04"), Some(4));
        assert_eq!(leading_number("4/12"), Some(4));
        assert_eq!(leading_number(" 7 "), Some(7));
        assert_eq!(leading_number("A4"), None);
        assert_eq!(leading_number(""), None);
    }
}
