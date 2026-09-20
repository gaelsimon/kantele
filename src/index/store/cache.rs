//! The store's rows held in memory for a scan.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering::Relaxed;

use crate::index::artwork::{self, Artwork, Source};
use crate::index::playlist;
use crate::index::roots::Roots;
use crate::index::scan::{Fingerprint, Found, Scanned, Scope};
use crate::tags::{AudioProperties, FileTags};

use super::payload::{Image, Payload, digest};

#[derive(Debug, Default)]
pub struct Cache {
    pub(super) roots: Roots,
    pub(super) files: HashMap<PathBuf, Row<Payload>>,
    pub(super) covers: HashMap<PathBuf, Row<Image>>,
    pub(super) playlists: HashMap<PathBuf, Row<playlist::Contents>>,
    pub(super) refused: HashMap<PathBuf, Row<String>>,
    pub(super) hits: AtomicUsize,
    pub(super) unreadable: AtomicUsize,
    pub(super) claims: crate::index::identity::Claims,
}

impl Cache {
    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    pub fn hits(&self) -> usize {
        self.hits.load(Relaxed)
    }

    pub fn unreadable(&self) -> usize {
        self.unreadable.load(Relaxed)
    }

    /// Which folder was awarded each album key last time, empty where nothing is remembered.
    pub fn claims(&self) -> &crate::index::identity::Claims {
        &self.claims
    }

    pub fn file(&self, relative: &Path, fingerprint: Fingerprint) -> Option<CachedFile> {
        if fingerprint == Fingerprint::UNKNOWN {
            return None;
        }
        let row = self.files.get(relative)?;
        if !row.answers_for(fingerprint) {
            return None;
        }
        self.hits.fetch_add(1, Relaxed);
        Some(CachedFile {
            artwork: row
                .payload
                .artwork
                .as_ref()
                .and_then(|image| image.artwork(&self.roots, relative)),
            tags: row.payload.tags.clone(),
            properties: row.payload.properties,
        })
    }

    pub fn refused(&self, relative: &Path, fingerprint: Fingerprint) -> Option<&str> {
        if fingerprint == Fingerprint::UNKNOWN {
            return None;
        }
        let row = self.refused.get(relative)?;
        row.answers_for(fingerprint).then_some(row.payload.as_str())
    }

    pub(super) fn agrees_on_refused(
        &self,
        relative: &Path,
        fingerprint: Fingerprint,
        payload: &str,
    ) -> bool {
        match self.refused.get(relative) {
            Some(row) => row.fingerprint == fingerprint && row.digest == digest(payload),
            None => false,
        }
    }

    pub(super) fn agrees(&self, relative: &Path, fingerprint: Fingerprint, payload: &str) -> bool {
        match self.files.get(relative) {
            Some(row) => row.fingerprint == fingerprint && row.digest == digest(payload),
            None => false,
        }
    }

    pub(super) fn agrees_on_cover(
        &self,
        relative: &Path,
        fingerprint: Fingerprint,
        payload: &str,
    ) -> bool {
        match self.covers.get(relative) {
            Some(row) => row.fingerprint == fingerprint && row.digest == digest(payload),
            None => false,
        }
    }

    /// Whether this cache was built for the folders a pass is about to cover.
    pub fn is_for(&self, roots: &Roots) -> bool {
        self.roots == *roots
    }

    pub fn remembered(&self) -> Vec<Scanned> {
        self.remembered_outside(&Scope::default())
    }

    pub fn remembered_outside(&self, scope: &Scope) -> Vec<Scanned> {
        let mut files: Vec<Scanned> = self
            .files
            .iter()
            .filter(|(relative, _)| !scope.covers_file(relative))
            .filter_map(|(relative, row)| {
                // A row naming no folder was written for another set of them, and is stale.
                let path = self.roots.absolute(relative)?;
                Some(Scanned {
                    path,
                    relative: relative.clone(),
                    tags: row.payload.tags.clone(),
                    properties: row.payload.properties,
                    size: row.fingerprint.size,
                    artwork: row
                        .payload
                        .artwork
                        .as_ref()
                        .and_then(|image| image.artwork(&self.roots, relative)),
                })
            })
            .collect();
        files.sort_by(|left, right| left.path.cmp(&right.path));
        files
    }

    pub fn playlist(
        &self,
        relative: &Path,
        fingerprint: Fingerprint,
    ) -> Option<playlist::Contents> {
        if fingerprint == Fingerprint::UNKNOWN {
            return None;
        }
        let row = self.playlists.get(relative)?;
        row.answers_for(fingerprint).then(|| row.payload.clone())
    }

    pub fn remembered_playlists(&self) -> Vec<playlist::Scanned> {
        self.remembered_playlists_outside(&Scope::default())
    }

    pub fn remembered_playlists_outside(&self, scope: &Scope) -> Vec<playlist::Scanned> {
        let mut playlists: Vec<playlist::Scanned> = self
            .playlists
            .iter()
            .filter(|(relative, _)| !scope.covers_file(relative))
            .map(|(relative, row)| playlist::Scanned {
                relative: relative.clone(),
                fingerprint: row.fingerprint,
                contents: row.payload.clone(),
            })
            .collect();
        playlists.sort_by(|left, right| left.relative.cmp(&right.relative));
        playlists
    }

    pub(super) fn agrees_on_playlist(
        &self,
        relative: &Path,
        fingerprint: Fingerprint,
        payload: &str,
    ) -> bool {
        match self.playlists.get(relative) {
            Some(row) => row.fingerprint == fingerprint && row.digest == digest(payload),
            None => false,
        }
    }

    pub fn cover(&self, image: &Found) -> Option<Artwork> {
        if image.fingerprint == Fingerprint::UNKNOWN {
            return None;
        }
        let relative = self.roots.relative(&image.path)?;
        let row = self.covers.get(&relative)?;
        if !row.answers_for(image.fingerprint) {
            return None;
        }
        Some(Artwork {
            source: Source::File(image.path.clone()),
            mime: artwork::image_mime_named(&row.payload.mime)?,
            dimensions: row.payload.dimensions,
        })
    }
}

#[derive(Clone, Debug)]
pub struct CachedFile {
    pub tags: FileTags,
    pub properties: AudioProperties,
    pub artwork: Option<Artwork>,
}

#[derive(Clone, Debug)]
pub(super) struct Row<T> {
    pub(super) fingerprint: Fingerprint,
    /// Digest of the stored text, compared instead of the text.
    pub(super) digest: u64,
    pub(super) payload: T,
    /// The reader that wrote this row. Another one means the row is served but not trusted to
    /// answer for the file, so the pass reads it again.
    pub(super) reader: i64,
}

impl<T> Row<T> {
    fn answers_for(&self, fingerprint: Fingerprint) -> bool {
        self.fingerprint == fingerprint && self.reader == super::READER_VERSION
    }
}
