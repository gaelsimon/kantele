//! What one store row holds.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::index::artwork::{self, Artwork, Source};
use crate::index::roots::Roots;
use crate::tags::{AudioProperties, FileTags};

/// `serde(default)` so an older row still reads back.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Payload {
    #[serde(deserialize_with = "tidied_tags")]
    pub tags: FileTags,
    pub properties: AudioProperties,
    pub artwork: Option<Image>,
}

/// Rows written before blank values were refused at the reader still hold them.
fn tidied_tags<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<FileTags, D::Error> {
    FileTags::deserialize(deserializer).map(FileTags::tidied)
}

/// Paths are relative to the root so a remount keeps the cache.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Image {
    /// None when the picture is embedded.
    pub file: Option<PathBuf>,
    pub index: usize,
    pub mime: String,
    pub dimensions: Option<(u32, u32)>,
}

impl Image {
    pub fn of(artwork: &Artwork, roots: &Roots) -> Self {
        let (file, index) = match &artwork.source {
            Source::File(path) => (Some(relative(path, roots)), 0),
            Source::Embedded { index, .. } => (None, *index),
        };
        Self {
            file,
            index,
            mime: artwork.mime.to_owned(),
            dimensions: artwork.dimensions,
        }
    }

    pub fn artwork(&self, roots: &Roots, relative: &Path) -> Option<Artwork> {
        let source = match &self.file {
            Some(file) => Source::File(roots.absolute(file)?),
            None => Source::Embedded {
                path: roots.absolute(relative)?,
                index: self.index,
            },
        };
        Some(Artwork {
            source,
            mime: artwork::image_mime_named(&self.mime)?,
            dimensions: self.dimensions,
        })
    }
}

/// Only ever compared within one process.
pub fn digest(payload: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    payload.hash(&mut hasher);
    hasher.finish()
}

pub fn relative(path: &Path, roots: &Roots) -> PathBuf {
    roots.relative_or_self(path)
}

pub fn text(path: &Path) -> Option<String> {
    path.to_str().map(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_row_written_before_a_field_existed_still_reads_back() {
        let older = r#"{"tags":{"title":"Juana Peña","artists":["Sierra Maestra"]},
                        "properties":{"duration":{"secs":243,"nanos":0}}}"#;
        let payload: Payload = serde_json::from_str(older).expect("an older row reads");
        assert_eq!(payload.tags.title.as_deref(), Some("Juana Peña"));
        assert!(!payload.tags.compilation, "an absent field defaults");
        assert!(payload.tags.artist_sorts.is_empty());
        assert!(payload.artwork.is_none());
    }

    #[test]
    fn a_row_holding_blank_values_reads_back_without_them() {
        let row = r#"{"tags":{"title":"One","album":" ","artists":["", "Sierra Maestra"],
                      "artist_sorts":["", "Maestra, Sierra"],"genres":["", "Son"]}}"#;
        let payload: Payload = serde_json::from_str(row).expect("the row reads");
        assert_eq!(payload.tags.album, None);
        assert_eq!(payload.tags.artists, vec!["Sierra Maestra".to_owned()]);
        assert_eq!(
            payload.tags.artist_sorts,
            vec!["Maestra, Sierra".to_owned()]
        );
        assert_eq!(payload.tags.genres, vec!["Son".to_owned()]);
    }

    #[test]
    fn an_absent_field_defaults_rather_than_recording_what_the_file_says() {
        let older = r#"{"tags":{"title":"One"}}"#;
        let payload: Payload = serde_json::from_str(older).expect("an older row reads");
        assert!(
            !payload.tags.compilation,
            "which is why adding a field bumps SCHEMA_VERSION: the row reads, and it is wrong \
             for any file that carries the flag"
        );
    }
}
