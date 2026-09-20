//! SQLite store for the tag cache, the dates added and the device identity.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering::Relaxed;

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::index::artwork::{Artwork, Source};
use crate::index::identity::IDENTITY_VERSION;
use crate::index::library::Library;
use crate::index::playlist;
use crate::index::roots::Roots;
use crate::index::scan::{Fingerprint, RefusedFile, Scanned, Scope, Walked};
use crate::index::sweep;

/// Paths probed, spread across the rows, before deciding whether they describe another library.
const SAMPLE: usize = 16;

/// Bump when a row becomes unsafe to read back at all: every one is deleted and read again.
const SCHEMA_VERSION: &str = "2";
/// Versioned apart so a parser change rereads no audio file.
const PLAYLIST_VERSION: &str = "1";
/// Bump when a reader learns a field an older row cannot carry. Stamped on every row it writes:
/// rows from another reader are still served, so the library answers from the first second, and
/// they miss in the cache, so the pass reads those files again and replaces them one by one.
/// Emptying the tables instead costs a whole cold walk before anything is served at all.
const READER_VERSION: i64 = 1;

/// `STRICT` so a wrong type is refused at write time.
const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;
CREATE TABLE IF NOT EXISTS files (
    relative TEXT PRIMARY KEY,
    size INTEGER NOT NULL,
    mtime INTEGER NOT NULL,
    payload TEXT NOT NULL,
    reader INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE IF NOT EXISTS covers (
    relative TEXT PRIMARY KEY,
    size INTEGER NOT NULL,
    mtime INTEGER NOT NULL,
    payload TEXT NOT NULL,
    reader INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE IF NOT EXISTS playlists (
    relative TEXT PRIMARY KEY,
    size INTEGER NOT NULL,
    mtime INTEGER NOT NULL,
    payload TEXT NOT NULL,
    reader INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE IF NOT EXISTS refused (
    relative TEXT PRIMARY KEY,
    size INTEGER NOT NULL,
    mtime INTEGER NOT NULL,
    payload TEXT NOT NULL,
    reader INTEGER NOT NULL DEFAULT 0
) STRICT;
CREATE TABLE IF NOT EXISTS added (
    identity TEXT PRIMARY KEY,
    relative TEXT NOT NULL,
    added INTEGER NOT NULL
) STRICT;
CREATE INDEX IF NOT EXISTS added_by_relative ON added (relative);
CREATE TABLE IF NOT EXISTS claims (
    key TEXT PRIMARY KEY,
    scope TEXT NOT NULL
) STRICT;
";

pub struct Store {
    connection: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(folder) = path.parent() {
            std::fs::create_dir_all(folder)
                .with_context(|| format!("creating {}", folder.display()))?;
        }
        let connection =
            Connection::open(path).with_context(|| format!("opening {}", path.display()))?;
        Self::prepare(connection)
    }

    /// Opens the store, setting aside a file no build can read so the next start has one that
    /// works. Without this a single corrupt file costs a full walk of the share on every start,
    /// for ever, and the dates added never come back.
    pub fn open_or_replace(path: &Path) -> Result<Self> {
        let refused = match Self::open(path) {
            Ok(store) => return Ok(store),
            Err(refused) => refused,
        };
        if !beyond_reading(&refused) {
            return Err(refused);
        }
        let aside = set_aside(path)
            .with_context(|| format!("setting aside a store that will not open: {refused:#}"))?;
        tracing::warn!(
            path = %path.display(), aside = %aside.display(), why = %format!("{refused:#}"),
            "the saved index would not open: it is set aside and a new one started, so this start \
             reads every file and the dates added begin again"
        );
        Self::open(path)
    }

    /// Says nothing where the reader has not moved, and says what a moved one means: the rows are
    /// kept and served, and every file behind them is read again as the pass reaches it.
    fn note_reader(&mut self) -> Result<()> {
        let now = READER_VERSION.to_string();
        if let Some(older) = self.meta("reader_version")?
            && older != now
        {
            tracing::info!(
                from = %older, to = %now,
                "these rows were written by another reader: they are served while the files \
                 behind them are read again"
            );
        }
        self.set_meta("reader_version", &now)
    }

    fn drop_stale(&mut self, key: &str, version: &str, tables: &[&str]) -> Result<()> {
        match self.meta(key)?.as_deref() {
            Some(held) if held == version => return Ok(()),
            Some(older) => {
                tracing::info!(
                    %key, from = %older, to = %version,
                    "store written by another build: those rows are read again, the dates are kept"
                );
                let statements: String = tables
                    .iter()
                    .map(|table| format!("DELETE FROM {table};"))
                    .collect();
                self.connection.execute_batch(&statements)?;
            }
            None => {}
        }
        self.set_meta(key, version)
    }

    pub fn in_memory() -> Result<Self> {
        Self::prepare(Connection::open_in_memory()?)
    }

    fn prepare(connection: Connection) -> Result<Self> {
        connection.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA synchronous = NORMAL;
             PRAGMA busy_timeout = 5000;
             -- Sort and index scratch stays off a NAS disk, and the default page cache is small
             -- for a table of tens of thousands of payloads. Negative is KiB.
             PRAGMA temp_store = MEMORY;
             PRAGMA cache_size = -16000;",
        )?;
        connection.execute_batch(SCHEMA)?;
        for table in ["files", "covers", "playlists", "refused"] {
            // A column a database written before it existed has not got. Anything else, including
            // its already being there, is the table being as it should. The default is this
            // reader: the column arrived without the payload changing shape, so what is already
            // stored is what this reader writes, and an owner updating the package pays nothing.
            let _ = connection.execute_batch(&format!(
                "ALTER TABLE {table} ADD COLUMN reader INTEGER NOT NULL DEFAULT {READER_VERSION};"
            ));
        }
        let mut store = Self { connection };

        store.drop_stale(
            "schema_version",
            SCHEMA_VERSION,
            &["files", "covers", "refused"],
        )?;
        store.drop_stale("playlist_version", PLAYLIST_VERSION, &["playlists"])?;
        store.note_reader()?;

        let identity_version = IDENTITY_VERSION.to_string();
        if let Some(older) = store.meta("identity_version")?
            && older != identity_version
        {
            tracing::warn!(
                from = %older, to = %identity_version,
                "identity rules changed: dates added are recovered by path"
            );
        }
        store.set_meta("identity_version", &identity_version)?;
        Ok(store)
    }

    fn meta(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .connection
            .query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| {
                row.get(0)
            })
            .optional()?)
    }

    fn set_meta(&mut self, key: &str, value: &str) -> Result<()> {
        if self.meta(key)?.as_deref() == Some(value) {
            return Ok(());
        }
        self.connection.execute(
            "INSERT INTO meta (key, value) VALUES (?1, ?2)
             ON CONFLICT (key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn resumed_update_id(&self) -> Result<Option<u32>> {
        Ok(self
            .meta("system_update_id")?
            .and_then(|held| held.parse().ok()))
    }

    pub fn remember_update_id(&mut self, id: u32) -> Result<()> {
        self.set_meta("system_update_id", &id.to_string())
    }

    pub fn resumed_boot_id(&self) -> Result<Option<u32>> {
        Ok(self.meta("boot_id")?.and_then(|held| held.parse().ok()))
    }

    pub fn remember_boot_id(&mut self, id: u32) -> Result<()> {
        self.set_meta("boot_id", &id.to_string())
    }

    pub fn device_udn(&mut self) -> Result<String> {
        if let Some(udn) = self.meta("udn")? {
            return Ok(udn);
        }
        let minted = uuid::Uuid::new_v4().to_string();
        self.set_meta("udn", &minted)?;
        tracing::info!(udn = %minted, "minted a device identity, and persisted it");
        Ok(minted)
    }

    fn holds(&self, roots: &Roots) -> Result<bool> {
        let stored = self.meta("root")?;
        Ok(stored.is_some() && stored == roots.meta())
    }

    /// Drops no rows: an unmounted share reads like another library. Adopts the folders only where
    /// the rows describe them, so a start in between serves nothing about some other library.
    pub fn serving(&mut self, roots: impl Into<Roots>) -> Result<()> {
        let roots = roots.into();
        if self.holds(&roots)? {
            return Ok(());
        }
        if let Some(previous) = self.meta("root")? {
            if self.describes(&roots)? {
                tracing::info!(
                    from = %previous, to = %roots.describe(),
                    "the same library at another path: its rows are kept"
                );
            } else {
                tracing::warn!(
                    from = %previous, to = %roots.describe(),
                    "this store was written for another library, so every file here is read \
                     again and the dates added are kept. One store serves one library: give each \
                     of them its own state_dir"
                );
                return Ok(());
            }
        }
        if let Some(meta) = roots.meta() {
            self.set_meta("root", &meta)?;
        }
        Ok(())
    }

    /// More than half of the sample must be there: two libraries can share one path.
    fn describes(&self, roots: &Roots) -> Result<bool> {
        if self.holds(roots)? {
            return Ok(true);
        }
        let sampled = self.sample_paths(SAMPLE)?;
        if sampled.is_empty() {
            return Ok(true);
        }
        let present = sampled
            .iter()
            .filter(|relative| roots.absolute(relative).is_some_and(|path| path.exists()))
            .count();
        Ok(present * 2 > sampled.len())
    }

    fn sample_paths(&self, most: usize) -> Result<Vec<PathBuf>> {
        let held: i64 = self
            .connection
            .query_row("SELECT COUNT(*) FROM files", [], |row| row.get(0))?;
        let step = (held / most as i64).max(1);
        let mut statement = self.connection.prepare(
            "SELECT relative FROM (SELECT relative, ROW_NUMBER() OVER (ORDER BY relative) AS n FROM files)
             WHERE (n - 1) % ?1 = 0 ORDER BY relative LIMIT ?2",
        )?;
        let rows = statement.query_map([step, most as i64], |row| row.get::<_, String>(0))?;
        Ok(rows.filter_map(Result::ok).map(PathBuf::from).collect())
    }

    pub fn cache(&self, roots: impl Into<Roots>) -> Result<Cache> {
        let mut cache = Cache {
            roots: roots.into(),
            ..Cache::default()
        };
        let (files, files_unreadable) = self.rows("files")?;
        let (covers, covers_unreadable) = self.rows("covers")?;
        let (playlists, playlists_unreadable) = self.rows("playlists")?;
        let (refused, refused_unreadable) = self.rows("refused")?;
        cache.files = files;
        cache.covers = covers;
        cache.playlists = playlists;
        cache.refused = refused;
        cache.claims = self.claims()?;
        cache.unreadable.store(
            files_unreadable + covers_unreadable + playlists_unreadable + refused_unreadable,
            Relaxed,
        );
        Ok(cache)
    }

    pub fn fingerprints(&self) -> Result<sweep::Remembered> {
        // Refused files are remembered too, or every sweep reports them as new.
        let mut files = self.fingerprints_of("files")?;
        files.extend(self.fingerprints_of("refused")?);
        Ok(sweep::Remembered {
            files,
            covers: self.fingerprints_of("covers")?,
            playlists: self.fingerprints_of("playlists")?,
        })
    }

    fn fingerprints_of(&self, table: &str) -> Result<HashMap<PathBuf, Fingerprint>> {
        let mut statement = self
            .connection
            .prepare(&format!("SELECT relative, size, mtime FROM {table}"))?;
        let rows = statement.query_map([], |row| {
            let relative: String = row.get(0)?;
            let size: i64 = row.get(1)?;
            let mtime: i64 = row.get(2)?;
            Ok((
                PathBuf::from(relative),
                Fingerprint {
                    size: size as u64,
                    mtime,
                },
            ))
        })?;
        rows.map(|row| row.map_err(anyhow::Error::from)).collect()
    }

    pub fn remembered(&self, roots: impl Into<Roots>) -> Result<Vec<Scanned>> {
        let roots = &roots.into();
        if !self.describes(roots)? {
            tracing::warn!(
                folder = %roots.describe(),
                "the store's rows name another library, so this start walks before it answers"
            );
            return Ok(Vec::new());
        }
        let mut statement = self
            .connection
            .prepare("SELECT relative, size, payload FROM files")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut files = Vec::new();
        for row in rows {
            let Ok((relative, size, text)) = row else {
                continue;
            };
            let Ok(payload) = serde_json::from_str::<Payload>(&text) else {
                continue;
            };
            let relative = PathBuf::from(relative);
            let Some(path) = roots.absolute(&relative) else {
                continue;
            };
            files.push(Scanned {
                path,
                artwork: payload
                    .artwork
                    .as_ref()
                    .and_then(|image| image.artwork(roots, &relative)),
                relative,
                tags: payload.tags,
                properties: payload.properties,
                size: size as u64,
            });
        }
        files.sort_by(|left, right| left.path.cmp(&right.path));
        Ok(files)
    }

    pub fn remembered_playlists(&self, roots: impl Into<Roots>) -> Result<Vec<playlist::Scanned>> {
        if !self.describes(&roots.into())? {
            return Ok(Vec::new());
        }
        let mut statement = self
            .connection
            .prepare("SELECT relative, size, mtime, payload FROM playlists")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, i64>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut playlists = Vec::new();
        for row in rows {
            let Ok((relative, size, mtime, text)) = row else {
                continue;
            };
            let Ok(contents) = serde_json::from_str(&text) else {
                continue;
            };
            playlists.push(playlist::Scanned {
                relative: PathBuf::from(relative),
                fingerprint: Fingerprint {
                    size: size as u64,
                    mtime,
                },
                contents,
            });
        }
        playlists.sort_by(|left, right| left.relative.cmp(&right.relative));
        Ok(playlists)
    }

    fn rows<T: serde::de::DeserializeOwned>(
        &self,
        table: &str,
    ) -> Result<(HashMap<PathBuf, Row<T>>, usize)> {
        let mut statement = self.connection.prepare(&format!(
            "SELECT relative, size, mtime, payload, reader FROM {table}"
        ))?;
        let rows = statement.query_map([], |row| {
            let relative: String = row.get(0)?;
            let size: i64 = row.get(1)?;
            let mtime: i64 = row.get(2)?;
            let payload: String = row.get(3)?;
            let reader: i64 = row.get(4)?;
            Ok((
                PathBuf::from(relative),
                Fingerprint {
                    size: size as u64,
                    mtime,
                },
                payload,
                reader,
            ))
        })?;
        let mut kept = HashMap::new();
        let mut unreadable = 0usize;
        for row in rows {
            let Ok((relative, fingerprint, text, reader)) = row else {
                unreadable += 1;
                continue;
            };
            match serde_json::from_str::<T>(&text) {
                Ok(payload) => {
                    kept.insert(
                        relative,
                        Row {
                            fingerprint,
                            digest: digest(&text),
                            payload,
                            reader,
                        },
                    );
                }
                Err(_) => unreadable += 1,
            }
        }
        Ok((kept, unreadable))
    }

    pub fn save(
        &mut self,
        roots: impl Into<Roots>,
        reading: Reading<'_>,
        cache: &Cache,
        scope: &Scope,
    ) -> Result<Saved> {
        let roots = &roots.into();
        let Reading {
            walked,
            files,
            playlists,
            refused,
        } = reading;
        let prune = self.may_prune(roots, walked, files, cache, scope)?;
        let transaction = self.connection.transaction()?;
        let mut saved = Saved::default();
        {
            let written = Covered {
                files: write_files(&transaction, roots, walked, files, cache, &mut saved)?,
                covers: write_covers(&transaction, roots, walked, files, cache, &mut saved)?,
                playlists: write_playlists(&transaction, playlists, cache, &mut saved)?,
                refused: write_refused(&transaction, refused, cache, &mut saved)?,
            };
            if prune {
                forget_stale(&transaction, cache, scope, &written, &mut saved)?;
            }
        }
        transaction.commit()?;
        if let Some(meta) = roots.meta() {
            self.set_meta("root", &meta)?;
        }
        Ok(saved)
    }

    fn may_prune(
        &self,
        roots: &Roots,
        walked: &Walked,
        files: &[Scanned],
        cache: &Cache,
        scope: &Scope,
    ) -> Result<bool> {
        if !self.holds(roots)? {
            tracing::info!(
                folder = %roots.describe(),
                "the store was last used for another folder: rows are added, none are forgotten"
            );
            return Ok(false);
        }
        if !walked.complete() {
            tracing::error!(
                folders = walked.unreadable,
                rows = cache.len(),
                "the walk could not read every folder: keeping every cached row rather than \
                 forgetting the ones it did not reach"
            );
            return Ok(false);
        }
        if scope.is_whole_tree() && files.is_empty() && !cache.is_empty() {
            tracing::error!(
                rows = cache.len(),
                folder = %roots.describe(),
                "the library reads as empty: keeping every cached row rather than forgetting it. \
                 Check that the share is still mounted"
            );
            return Ok(false);
        }
        Ok(true)
    }

    pub fn stamp_dates_added(&mut self, library: &mut Library, now: i64) -> Result<Stamped> {
        let known = self.dates_added()?;
        let mut stamped = Stamped::default();
        let transaction = self.connection.transaction()?;
        {
            let mut upsert = transaction.prepare(
                "INSERT INTO added (identity, relative, added) VALUES (?1, ?2, ?3)
                 ON CONFLICT (identity) DO UPDATE SET relative = excluded.relative",
            )?;
            for track in library.tracks_mut() {
                let identity = track.id.as_str().to_owned();
                let relative = track.relative.clone();
                let row = known.by_identity.get(&identity);
                let added = match row
                    .map(|(_, added)| added)
                    .or_else(|| known.by_relative.get(&relative))
                {
                    Some(added) => {
                        stamped.known += 1;
                        *added
                    }
                    None => {
                        stamped.minted += 1;
                        now
                    }
                };
                track.date_added = Some(added);
                if row == Some(&(relative.clone(), added)) {
                    stamped.unchanged += 1;
                    continue;
                }
                upsert.execute(params![identity, relative, added])?;
            }
        }
        transaction.commit()?;
        Ok(stamped)
    }

    /// Which folder was awarded each album key, as the last pass left it.
    pub fn claims(&self) -> Result<crate::index::identity::Claims> {
        let mut statement = self.connection.prepare("SELECT key, scope FROM claims")?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        Ok(rows.filter_map(Result::ok).collect())
    }

    /// Replaces the lot, so a key nothing derives any more is released rather than held forever.
    pub fn remember_claims(&mut self, claims: &crate::index::identity::Claims) -> Result<()> {
        let transaction = self.connection.transaction()?;
        transaction.execute("DELETE FROM claims", [])?;
        {
            let mut insert =
                transaction.prepare("INSERT INTO claims (key, scope) VALUES (?1, ?2)")?;
            for (key, scope) in claims {
                insert.execute((key, scope))?;
            }
        }
        transaction.commit()?;
        Ok(())
    }

    fn dates_added(&self) -> Result<DatesAdded> {
        let mut held = DatesAdded::default();
        let mut statement = self
            .connection
            .prepare("SELECT identity, relative, added FROM added")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        for (identity, relative, added) in rows.filter_map(Result::ok) {
            held.by_identity.insert(identity, (relative.clone(), added));
            // A retag leaves the old row behind, so a path keeps its earliest date.
            held.by_relative
                .entry(relative)
                .and_modify(|earliest| *earliest = (*earliest).min(added))
                .or_insert(added);
        }
        Ok(held)
    }
}

/// A file that is not a database, as against a store that merely could not be written to: a full
/// disk or a folder gone read-only must never cost the dates added.
fn beyond_reading(error: &anyhow::Error) -> bool {
    error.chain().any(|cause| {
        matches!(
            cause.downcast_ref::<rusqlite::Error>(),
            Some(rusqlite::Error::SqliteFailure(failure, _))
                if matches!(
                    failure.code,
                    rusqlite::ErrorCode::NotADatabase | rusqlite::ErrorCode::DatabaseCorrupt
                )
        )
    })
}

fn set_aside(path: &Path) -> Result<PathBuf> {
    let aside = free_name(path);
    std::fs::rename(path, &aside).with_context(|| format!("moving {} aside", path.display()))?;
    for suffix in ["-wal", "-shm"] {
        let mut companion = path.as_os_str().to_owned();
        companion.push(suffix);
        let _ = std::fs::remove_file(PathBuf::from(companion));
    }
    Ok(aside)
}

/// The first store set aside keeps the plain name, and a second corruption does not overwrite it.
fn free_name(path: &Path) -> PathBuf {
    let first = path.with_extension("unreadable");
    if !first.exists() {
        return first;
    }
    (1..100)
        .map(|at| path.with_extension(format!("unreadable.{at}")))
        .find(|taken| !taken.exists())
        .unwrap_or(first)
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Saved {
    pub files: usize,
    pub covers: usize,
    pub playlists: usize,
    pub refused: usize,
    pub forgotten: usize,
    pub unchanged: usize,
}

#[derive(Debug, Default)]
struct DatesAdded {
    by_identity: HashMap<String, (String, i64)>,
    by_relative: HashMap<String, i64>,
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Stamped {
    pub known: usize,
    pub minted: usize,
    pub unchanged: usize,
}

pub use cache::{Cache, CachedFile};

mod cache;
mod payload;

use cache::Row;
use payload::{Image, Payload, digest, relative, text};

#[derive(Clone, Copy, Debug)]
pub struct Reading<'a> {
    pub walked: &'a Walked,
    pub files: &'a [Scanned],
    pub playlists: &'a [playlist::Scanned],
    pub refused: &'a [RefusedFile],
}

impl<'a> Reading<'a> {
    pub fn of(walked: &'a Walked, files: &'a [Scanned]) -> Self {
        Self {
            walked,
            files,
            playlists: &[],
            refused: &[],
        }
    }
}

struct Covered<'a> {
    files: HashSet<&'a Path>,
    covers: HashSet<PathBuf>,
    playlists: HashSet<&'a Path>,
    refused: HashSet<&'a Path>,
}

fn write_files<'a>(
    transaction: &Transaction<'_>,
    roots: &Roots,
    walked: &Walked,
    files: &'a [Scanned],
    cache: &Cache,
    saved: &mut Saved,
) -> Result<HashSet<&'a Path>> {
    let fingerprints: HashMap<&Path, Fingerprint> = walked
        .files
        .iter()
        .map(|found| (found.path.as_path(), found.fingerprint))
        .collect();
    let mut upsert = transaction.prepare(
        "INSERT INTO files (relative, size, mtime, payload, reader) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (relative) DO UPDATE
         SET size = excluded.size, mtime = excluded.mtime, payload = excluded.payload,
             reader = excluded.reader",
    )?;
    let mut covered: HashSet<&Path> = HashSet::with_capacity(files.len());
    for file in files {
        covered.insert(file.relative.as_path());
        let Some(relative) = text(&file.relative) else {
            continue;
        };
        let Some(fingerprint) = fingerprints.get(file.path.as_path()).copied() else {
            continue;
        };
        if fingerprint == Fingerprint::UNKNOWN {
            continue;
        }
        let payload = serde_json::to_string(&Payload {
            tags: file.tags.clone(),
            properties: file.properties,
            artwork: file.artwork.as_ref().map(|art| Image::of(art, roots)),
        })?;
        if cache.agrees(&file.relative, fingerprint, &payload) {
            saved.unchanged += 1;
            continue;
        }
        upsert.execute(params![
            relative,
            fingerprint.size as i64,
            fingerprint.mtime,
            payload,
            READER_VERSION
        ])?;
        saved.files += 1;
    }
    Ok(covered)
}

fn write_refused<'a>(
    transaction: &Transaction<'_>,
    refused: &'a [RefusedFile],
    cache: &Cache,
    saved: &mut Saved,
) -> Result<HashSet<&'a Path>> {
    let mut upsert = transaction.prepare(
        "INSERT INTO refused (relative, size, mtime, payload, reader) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (relative) DO UPDATE
         SET size = excluded.size, mtime = excluded.mtime, payload = excluded.payload,
             reader = excluded.reader",
    )?;
    let mut covered: HashSet<&Path> = HashSet::with_capacity(refused.len());
    for file in refused {
        // Left uncovered so any row it has is forgotten: the next pass tries the file again.
        if !file.remember {
            continue;
        }
        covered.insert(file.relative.as_path());
        let Some(relative) = text(&file.relative) else {
            continue;
        };
        if file.fingerprint == Fingerprint::UNKNOWN {
            continue;
        }
        let payload = serde_json::to_string(&file.why)?;
        if cache.agrees_on_refused(&file.relative, file.fingerprint, &payload) {
            saved.unchanged += 1;
            continue;
        }
        upsert.execute(params![
            relative,
            file.fingerprint.size as i64,
            file.fingerprint.mtime,
            payload,
            READER_VERSION
        ])?;
        saved.refused += 1;
    }
    Ok(covered)
}

fn write_covers(
    transaction: &Transaction<'_>,
    roots: &Roots,
    walked: &Walked,
    files: &[Scanned],
    cache: &Cache,
    saved: &mut Saved,
) -> Result<HashSet<PathBuf>> {
    let descriptions: HashMap<&Path, &Artwork> = files
        .iter()
        .filter_map(|file| {
            let described = file.artwork.as_ref()?;
            match &described.source {
                Source::File(path) => Some((path.as_path(), described)),
                Source::Embedded { .. } => None,
            }
        })
        .collect();
    let mut upsert = transaction.prepare(
        "INSERT INTO covers (relative, size, mtime, payload, reader) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (relative) DO UPDATE
         SET size = excluded.size, mtime = excluded.mtime, payload = excluded.payload,
             reader = excluded.reader",
    )?;
    let mut covered: HashSet<PathBuf> = HashSet::with_capacity(walked.covers.len());
    for image in walked.covers.values() {
        let relative = relative(&image.path, roots);
        covered.insert(relative.clone());
        let Some(text) = text(&relative) else {
            continue;
        };
        let Some(described) = descriptions.get(image.path.as_path()) else {
            continue;
        };
        if image.fingerprint == Fingerprint::UNKNOWN {
            continue;
        }
        let payload = serde_json::to_string(&Image::of(described, roots))?;
        if cache.agrees_on_cover(&relative, image.fingerprint, &payload) {
            saved.unchanged += 1;
            continue;
        }
        upsert.execute(params![
            text,
            image.fingerprint.size as i64,
            image.fingerprint.mtime,
            payload,
            READER_VERSION
        ])?;
        saved.covers += 1;
    }
    Ok(covered)
}

fn write_playlists<'a>(
    transaction: &Transaction<'_>,
    playlists: &'a [playlist::Scanned],
    cache: &Cache,
    saved: &mut Saved,
) -> Result<HashSet<&'a Path>> {
    let mut upsert = transaction.prepare(
        "INSERT INTO playlists (relative, size, mtime, payload, reader) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (relative) DO UPDATE
         SET size = excluded.size, mtime = excluded.mtime, payload = excluded.payload,
             reader = excluded.reader",
    )?;
    let mut covered: HashSet<&Path> = HashSet::with_capacity(playlists.len());
    for found in playlists {
        covered.insert(found.relative.as_path());
        let Some(relative) = text(&found.relative) else {
            continue;
        };
        if found.fingerprint == Fingerprint::UNKNOWN {
            continue;
        }
        let payload = serde_json::to_string(&found.contents)?;
        if cache.agrees_on_playlist(&found.relative, found.fingerprint, &payload) {
            saved.unchanged += 1;
            continue;
        }
        upsert.execute(params![
            relative,
            found.fingerprint.size as i64,
            found.fingerprint.mtime,
            payload,
            READER_VERSION
        ])?;
        saved.playlists += 1;
    }
    Ok(covered)
}

fn forget_stale(
    transaction: &Transaction<'_>,
    cache: &Cache,
    scope: &Scope,
    written: &Covered<'_>,
    saved: &mut Saved,
) -> Result<()> {
    let stale = |table: &'static str, kept: Vec<&PathBuf>| -> Result<usize> {
        let mut forget =
            transaction.prepare(&format!("DELETE FROM {table} WHERE relative = ?1"))?;
        let mut forgotten = 0;
        for path in kept {
            if let Some(text) = text(path) {
                forgotten += forget.execute([text])?;
            }
        }
        Ok(forgotten)
    };
    let inside = |path: &&PathBuf| scope.covers_file(path);
    saved.forgotten += stale(
        "files",
        cache
            .files
            .keys()
            .filter(inside)
            .filter(|kept| !written.files.contains(kept.as_path()))
            .collect(),
    )?;
    saved.forgotten += stale(
        "covers",
        cache
            .covers
            .keys()
            .filter(inside)
            .filter(|kept| !written.covers.contains(*kept))
            .collect(),
    )?;
    saved.forgotten += stale(
        "playlists",
        cache
            .playlists
            .keys()
            .filter(inside)
            .filter(|kept| !written.playlists.contains(kept.as_path()))
            .collect(),
    )?;
    saved.forgotten += stale(
        "refused",
        cache
            .refused
            .keys()
            .filter(inside)
            .filter(|kept| !written.refused.contains(kept.as_path()))
            .collect(),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::scan::Found;
    use crate::tags::{AudioProperties, FileTags};

    const ROOT: &str = "/music";

    struct OnDisk(PathBuf);

    impl OnDisk {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("kantele-store-{name}.sqlite"));
            let _ = std::fs::remove_file(&path);
            Self(path)
        }

        fn open(&self) -> Store {
            Store::open(&self.0).expect("opening the store")
        }
    }

    impl Drop for OnDisk {
        fn drop(&mut self) {
            for suffix in ["", "-wal", "-shm"] {
                let mut path = self.0.clone().into_os_string();
                path.push(suffix);
                let _ = std::fs::remove_file(PathBuf::from(path));
            }
        }
    }

    fn whole() -> Scope {
        Scope::whole_tree()
    }

    fn fingerprint(size: u64, mtime: i64) -> Fingerprint {
        Fingerprint { size, mtime }
    }

    fn scanned(relative: &str, title: &str, artwork: Option<Artwork>) -> Scanned {
        Scanned {
            path: PathBuf::from(ROOT).join(relative),
            relative: PathBuf::from(relative),
            tags: FileTags {
                title: Some(title.to_owned()),
                artists: vec!["Sierra Maestra".to_owned()],
                ..FileTags::default()
            },
            properties: AudioProperties {
                duration: std::time::Duration::from_secs(243),
                sample_rate: Some(44_100),
                ..AudioProperties::default()
            },
            size: 27,
            artwork,
        }
    }

    fn walked(files: &[Scanned], print: Fingerprint) -> Walked {
        Walked {
            stopped: false,
            playlists: Vec::new(),
            files: files
                .iter()
                .map(|file| Found {
                    path: file.path.clone(),
                    fingerprint: print,
                })
                .collect(),
            covers: HashMap::new(),
            unreadable: 0,
            refusals: Default::default(),
        }
    }

    fn saved(store: &mut Store, files: &[Scanned], print: Fingerprint) -> Saved {
        let cache = store.cache(Path::new(ROOT)).expect("a cache");
        store
            .save(
                Path::new(ROOT),
                Reading::of(&walked(files, print), files),
                &cache,
                &whole(),
            )
            .expect("saving")
    }

    #[test]
    fn a_file_whose_fingerprint_has_not_changed_is_answered_from_the_store() {
        let mut store = Store::in_memory().expect("a store");
        let files = [scanned("a/1.flac", "Juana Peña", None)];
        let print = fingerprint(27, 1_700_000_000);
        assert_eq!(saved(&mut store, &files, print).files, 1);

        let cache = store.cache(Path::new(ROOT)).expect("a cache");
        let hit = cache
            .file(Path::new("a/1.flac"), print)
            .expect("the row is there");
        assert_eq!(hit.tags.title.as_deref(), Some("Juana Peña"));
        assert_eq!(hit.properties.sample_rate, Some(44_100));
        assert_eq!(cache.hits(), 1);
    }

    fn every_kind_of_tag() -> FileTags {
        FileTags {
            title: Some("Juana Peña".to_owned()),
            artists: vec!["Sierra Maestra".to_owned(), "Juan de Marcos".to_owned()],
            album_artists: vec!["Sierra Maestra".to_owned()],
            album: Some("!Dundunbanza!".to_owned()),
            composers: vec!["Arsenio Rodríguez".to_owned()],
            artist_sorts: vec!["Sierra Maestra".to_owned(), "Marcos, Juan de".to_owned()],
            album_artist_sorts: vec!["Sierra Maestra".to_owned()],
            composer_sorts: vec!["Rodríguez, Arsenio".to_owned()],
            genres: vec!["Son".to_owned(), "Latin".to_owned()],
            compilation: true,
            date: Some("1994".to_owned()),
            track_number: Some(1),
            track_total: Some(14),
            disc_number: Some(1),
            disc_total: Some(2),
            disc_subtitle: Some("Cuba".to_owned()),
            work: Some("Suite".to_owned()),
            movement_name: Some("Allegro".to_owned()),
            movement_number: Some(2),
            grouping: Some("Sessions".to_owned()),
            musicbrainz_release_id: Some("2a1b8f1c-0000-4000-8000-000000000001".to_owned()),
            musicbrainz_release_track_id: Some("2a1b8f1c-0000-4000-8000-000000000002".to_owned()),
            musicbrainz_recording_id: Some("2a1b8f1c-0000-4000-8000-000000000003".to_owned()),
            musicbrainz_artist_ids: vec![
                "2a1b8f1c-0000-4000-8000-000000000004".to_owned(),
                "2a1b8f1c-0000-4000-8000-000000000005".to_owned(),
            ],
            musicbrainz_album_artist_ids: vec!["2a1b8f1c-0000-4000-8000-000000000006".to_owned()],
        }
    }

    #[test]
    fn every_field_of_a_tag_survives_the_round_trip() {
        let mut store = Store::in_memory().expect("a store");
        let mut file = scanned("a/1.flac", "Juana Peña", None);
        file.tags = every_kind_of_tag();
        let print = fingerprint(27, 1_700_000_000);
        saved(&mut store, &[file.clone()], print);

        let cache = store.cache(Path::new(ROOT)).expect("a cache");
        let read_back = cache
            .file(Path::new("a/1.flac"), print)
            .expect("the row is there");
        assert_eq!(
            format!("{:?}", read_back.tags),
            format!("{:?}", file.tags),
            "a tag went missing between the file and the store"
        );
        assert_eq!(
            format!("{:?}", read_back.properties),
            format!("{:?}", file.properties)
        );
    }

    #[test]
    fn a_stored_shape_is_frozen_so_a_change_to_one_has_to_bump_its_own_version() {
        let payload = serde_json::to_string_pretty(&Payload {
            tags: every_kind_of_tag(),
            properties: AudioProperties {
                duration: std::time::Duration::from_millis(243_026),
                sample_rate: Some(44_100),
                bit_depth: Some(16),
                channels: Some(2),
                bitrate_bps: Some(1_411_000),
            },
            artwork: Some(Image {
                file: Some(PathBuf::from("a/cover.jpg")),
                index: 0,
                mime: "image/jpeg".to_owned(),
                dimensions: Some((500, 500)),
            }),
        })
        .expect("serialising a payload");
        let playlist = serde_json::to_string_pretty(&playlist::Contents {
            title: "Party".to_owned(),
            entries: vec![playlist::Entry {
                target: PathBuf::from("a/1.flac"),
                display: Some("Sierra Maestra - Juana Peña".to_owned()),
            }],
        })
        .expect("serialising a playlist");
        insta::assert_snapshot!(
            "stored_payload",
            format!("schema {SCHEMA_VERSION} playlist {PLAYLIST_VERSION}\n{payload}\n{playlist}")
        );
    }

    #[test]
    fn a_store_written_before_the_reader_column_existed_opens_and_keeps_its_rows() {
        let store = OnDisk::new("without-reader");
        {
            // The tables as a build before this column wrote them.
            let connection = Connection::open(&store.0).expect("a database");
            connection
                .execute_batch(
                    "CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL) STRICT;
                     CREATE TABLE files (
                        relative TEXT PRIMARY KEY,
                        size INTEGER NOT NULL,
                        mtime INTEGER NOT NULL,
                        payload TEXT NOT NULL
                     ) STRICT;
                     INSERT INTO files VALUES ('a/1.flac', 27, 1700000000, '{\"tags\":{}}');
                     INSERT INTO meta VALUES ('schema_version', '2');",
                )
                .expect("the older shape");
            let roots: Roots = Path::new(ROOT).into();
            connection
                .execute(
                    "INSERT INTO meta VALUES ('root', ?1)",
                    [roots.meta().expect("a named root")],
                )
                .expect("the library those rows name");
        }

        let opened = store.open();
        assert_eq!(
            opened
                .remembered(Path::new(ROOT))
                .expect("what the store holds")
                .len(),
            1,
            "an owner updating the package keeps the library the last build wrote"
        );
        let cache = opened.cache(Path::new(ROOT)).expect("a cache");
        assert!(
            cache
                .file(Path::new("a/1.flac"), fingerprint(27, 1_700_000_000))
                .is_some(),
            "and nothing is read again: the column arrived without the payload changing shape, \
             so an owner updating the package pays nothing for it"
        );
    }

    #[test]
    fn a_row_an_older_reader_wrote_is_served_and_read_again_rather_than_deleted() {
        let mut store = Store::in_memory().expect("a store");
        let files = [scanned("a/1.flac", "Juana Peña", None)];
        let print = fingerprint(27, 1_700_000_000);
        assert_eq!(saved(&mut store, &files, print).files, 1);
        store
            .connection
            .execute("UPDATE files SET reader = 0", [])
            .expect("a row from an older reader");

        assert_eq!(
            store
                .remembered(Path::new(ROOT))
                .expect("what the store holds")
                .len(),
            1,
            "the library answers from the first second, where emptying the table would have \
             cost a whole cold walk before anything was served"
        );
        let cache = store.cache(Path::new(ROOT)).expect("a cache");
        assert!(
            cache.file(Path::new("a/1.flac"), print).is_none(),
            "and the file is read again, since this row cannot carry what this reader knows"
        );
    }

    #[test]
    fn a_playlist_the_parser_has_changed_under_is_read_again_and_no_file_is() {
        let store = OnDisk::new("playlist-version");
        {
            let mut open = store.open();
            saved(
                &mut open,
                &[scanned("a/1.flac", "Juana Peña", None)],
                fingerprint(27, 1_700_000_000),
            );
            open.connection
                .execute(
                    "INSERT INTO playlists (relative, size, mtime, payload, reader)
                     VALUES ('a/p.m3u', 9, 1, '{\"title\":\"P\"}', 1)",
                    [],
                )
                .expect("a playlist row");
            open.set_meta("playlist_version", "0")
                .expect("an older parser");
        }
        let reopened = store.open();
        let cache = reopened.cache(Path::new(ROOT)).expect("a cache");
        assert!(
            cache.playlists.is_empty(),
            "the playlist was parsed by a build that read it differently"
        );
        assert_eq!(
            cache.len(),
            1,
            "and the file rows cost a walk of the share to rebuild, so they stay"
        );
    }

    #[test]
    fn the_update_id_survives_a_restart_so_no_client_is_handed_a_value_it_holds() {
        let store = OnDisk::new("update-id");
        {
            let mut open = store.open();
            assert_eq!(
                open.resumed_update_id().expect("a fresh store"),
                None,
                "a store that has never served remembers nothing"
            );
            open.remember_update_id(7).expect("remembering");
        }
        let reopened = store.open();
        assert_eq!(
            reopened.resumed_update_id().expect("a written store"),
            Some(7)
        );
    }

    #[test]
    fn an_unreadable_update_id_reads_as_absent_rather_than_as_zero() {
        let store = OnDisk::new("update-id-junk");
        {
            let mut open = store.open();
            open.set_meta("system_update_id", "not a number")
                .expect("writing junk");
        }
        assert_eq!(store.open().resumed_update_id().expect("no error"), None);
    }

    #[test]
    fn a_file_that_changed_is_read_again() {
        let mut store = Store::in_memory().expect("a store");
        let files = [scanned("a/1.flac", "Juana Peña", None)];
        saved(&mut store, &files, fingerprint(27, 1_700_000_000));
        let cache = store.cache(Path::new(ROOT)).expect("a cache");

        assert!(
            cache
                .file(Path::new("a/1.flac"), fingerprint(27, 1_700_000_001))
                .is_none()
        );
        assert!(
            cache
                .file(Path::new("a/1.flac"), fingerprint(28, 1_700_000_000))
                .is_none()
        );
        assert!(
            cache
                .file(Path::new("a/1.flac"), Fingerprint::UNKNOWN)
                .is_none()
        );
        assert_eq!(cache.hits(), 0);
    }

    #[test]
    fn a_cover_file_is_remembered_relative_to_the_root_so_a_remount_keeps_it() {
        let mut store = Store::in_memory().expect("a store");
        let cover = Artwork {
            source: Source::File(PathBuf::from("/music/a/cover.jpg")),
            mime: "image/jpeg",
            dimensions: Some((500, 500)),
        };
        let files = [scanned("a/1.flac", "Juana Peña", Some(cover.clone()))];
        let print = fingerprint(27, 1_700_000_000);
        let mut walk = walked(&files, print);
        walk.covers.insert(
            PathBuf::from("/music/a"),
            Found {
                path: PathBuf::from("/music/a/cover.jpg"),
                fingerprint: fingerprint(80_000, 1_600_000_000),
            },
        );
        let cache = store.cache(Path::new(ROOT)).expect("a cache");
        let written = store
            .save(
                Path::new(ROOT),
                Reading::of(&walk, &files),
                &cache,
                &whole(),
            )
            .expect("saving");
        assert_eq!(written.covers, 1);

        let elsewhere = store.cache(Path::new("/Volumes/music-1")).expect("a cache");
        let found = Found {
            path: PathBuf::from("/Volumes/music-1/a/cover.jpg"),
            fingerprint: fingerprint(80_000, 1_600_000_000),
        };
        let read_back = elsewhere.cover(&found).expect("the row is there");
        assert_eq!(
            read_back.source,
            Source::File(PathBuf::from("/Volumes/music-1/a/cover.jpg"))
        );
        assert_eq!(read_back.dimensions, Some((500, 500)));
        assert_eq!(read_back.mime, "image/jpeg");
    }

    #[test]
    fn a_picture_inside_a_file_survives_the_round_trip_as_one() {
        let mut store = Store::in_memory().expect("a store");
        let embedded = Artwork {
            source: Source::Embedded {
                path: PathBuf::from("/music/a/1.flac"),
                index: 0,
            },
            mime: "image/png",
            dimensions: Some((300, 300)),
        };
        let files = [scanned("a/1.flac", "Juana Peña", Some(embedded))];
        let print = fingerprint(27, 1_700_000_000);
        saved(&mut store, &files, print);

        let cache = store.cache(Path::new(ROOT)).expect("a cache");
        let hit = cache.file(Path::new("a/1.flac"), print).expect("the row");
        assert_eq!(
            hit.artwork.map(|art| art.source),
            Some(Source::Embedded {
                path: PathBuf::from("/music/a/1.flac"),
                index: 0
            })
        );
    }

    #[test]
    fn a_row_the_scan_agrees_with_is_not_written_again() {
        let mut store = Store::in_memory().expect("a store");
        let files = [scanned("a/1.flac", "Juana Peña", None)];
        let print = fingerprint(27, 1_700_000_000);
        assert_eq!(saved(&mut store, &files, print).files, 1);

        let again = saved(&mut store, &files, print);
        assert_eq!((again.files, again.unchanged), (0, 1));

        let recovered = [scanned(
            "a/1.flac",
            "Juana Peña",
            Some(Artwork {
                source: Source::Embedded {
                    path: PathBuf::from("/music/a/1.flac"),
                    index: 0,
                },
                mime: "image/jpeg",
                dimensions: None,
            }),
        )];
        assert_eq!(saved(&mut store, &recovered, print).files, 1);
    }

    #[test]
    fn a_date_a_row_already_holds_is_not_written_again() {
        let mut store = Store::in_memory().expect("a store");
        let mut first = library(&[scanned("a/1.flac", "Juana Peña", None)]);
        store
            .stamp_dates_added(&mut first, 1_000)
            .expect("stamping");

        let mut again = library(&[scanned("a/1.flac", "Juana Peña", None)]);
        let stamped = store
            .stamp_dates_added(&mut again, 9_000)
            .expect("stamping");
        assert_eq!(
            (stamped.minted, stamped.known, stamped.unchanged),
            (0, 1, 1)
        );
        assert_eq!(dates(&again), vec![Some(1_000)]);
    }

    #[test]
    fn rows_for_files_the_tree_no_longer_holds_are_forgotten() {
        let mut store = Store::in_memory().expect("a store");
        let print = fingerprint(27, 1_700_000_000);
        let both = [
            scanned("a/1.flac", "Juana Peña", None),
            scanned("a/2.flac", "Dundunbanza", None),
        ];
        saved(&mut store, &both, print);
        let one = [scanned("a/1.flac", "Juana Peña", None)];
        assert_eq!(saved(&mut store, &one, print).forgotten, 1);
        assert_eq!(store.cache(Path::new(ROOT)).expect("a cache").len(), 1);
    }

    #[test]
    fn a_library_that_reads_as_empty_forgets_nothing() {
        let mut store = Store::in_memory().expect("a store");
        let print = fingerprint(27, 1_700_000_000);
        let files = [scanned("a/1.flac", "Juana Peña", None)];
        saved(&mut store, &files, print);

        let cache = store.cache(Path::new(ROOT)).expect("a cache");
        let written = store
            .save(
                Path::new(ROOT),
                Reading::of(&Walked::default(), &[]),
                &cache,
                &whole(),
            )
            .expect("saving");
        assert_eq!(written.forgotten, 0);
        assert_eq!(store.cache(Path::new(ROOT)).expect("a cache").len(), 1);
    }

    #[test]
    fn a_walk_that_could_not_read_every_folder_forgets_nothing() {
        let mut store = Store::in_memory().expect("a store");
        let print = fingerprint(27, 1_700_000_000);
        let both = [
            scanned("a/1.flac", "Juana Peña", None),
            scanned("b/2.flac", "Dundunbanza", None),
        ];
        saved(&mut store, &both, print);

        let half = [scanned("a/1.flac", "Juana Peña", None)];
        let mut walk = walked(&half, print);
        walk.unreadable = 1;
        let cache = store.cache(Path::new(ROOT)).expect("a cache");
        let written = store
            .save(Path::new(ROOT), Reading::of(&walk, &half), &cache, &whole())
            .expect("saving");
        assert_eq!(written.forgotten, 0);
        assert_eq!(store.cache(Path::new(ROOT)).expect("a cache").len(), 2);
    }

    #[test]
    fn a_payload_this_build_cannot_read_is_counted_rather_than_shrugged_at() {
        let mut store = Store::in_memory().expect("a store");
        let print = fingerprint(27, 1_700_000_000);
        let files = [scanned("a/1.flac", "Juana Peña", None)];
        saved(&mut store, &files, print);
        store
            .connection
            .execute("UPDATE files SET payload = '{\"tags\":42}'", [])
            .expect("writing a payload this build cannot read");

        let cache = store.cache(Path::new(ROOT)).expect("a cache");
        assert!(cache.file(Path::new("a/1.flac"), print).is_none());
        assert_eq!(cache.hits(), 0);
        assert_eq!(
            cache.unreadable(),
            1,
            "a full scan with no explanation is the failure mode this counter exists to name"
        );
    }

    #[test]
    fn a_scan_of_another_folder_forgets_nothing() {
        let mut store = Store::in_memory().expect("a store");
        let print = fingerprint(27, 1_700_000_000);
        let share = [scanned("a/1.flac", "Juana Peña", None)];
        saved(&mut store, &share, print);

        let subtree = [scanned("1.flac", "Juana Peña", None)];
        let cache = store.cache(Path::new("/music/a")).expect("a cache");
        let written = store
            .save(
                Path::new("/music/a"),
                Reading::of(&walked(&subtree, print), &subtree),
                &cache,
                &whole(),
            )
            .expect("saving");
        assert_eq!(written.forgotten, 0);
        assert_eq!(store.cache(Path::new(ROOT)).expect("a cache").len(), 2);
    }

    fn library(files: &[Scanned]) -> Library {
        Library::build("Music".to_owned(), files)
    }

    fn dates(library: &Library) -> Vec<Option<i64>> {
        library
            .tracks()
            .iter()
            .map(|track| track.date_added)
            .collect()
    }

    #[test]
    fn a_track_seen_for_the_first_time_is_stamped_with_now() {
        let mut store = Store::in_memory().expect("a store");
        let mut first = library(&[scanned("a/1.flac", "Juana Peña", None)]);
        let stamped = store
            .stamp_dates_added(&mut first, 1_000)
            .expect("stamping");
        assert_eq!((stamped.minted, stamped.known), (1, 0));
        assert_eq!(dates(&first), vec![Some(1_000)]);
    }

    #[test]
    fn a_renamed_file_keeps_the_date_it_was_first_seen() {
        let mut store = Store::in_memory().expect("a store");
        let mut before = library(&[scanned("a/01 juana.flac", "Juana Peña", None)]);
        store
            .stamp_dates_added(&mut before, 1_000)
            .expect("stamping");

        let mut after = library(&[scanned("b/Juana Peña.flac", "Juana Peña", None)]);
        let stamped = store
            .stamp_dates_added(&mut after, 9_000)
            .expect("stamping");
        assert_eq!((stamped.minted, stamped.known), (0, 1));
        assert_eq!(dates(&after), vec![Some(1_000)]);
    }

    #[test]
    fn a_retagged_file_keeps_its_date_although_its_identity_changed() {
        let mut store = Store::in_memory().expect("a store");
        let mut before = library(&[scanned("a/1.flac", "Juana Pena", None)]);
        store
            .stamp_dates_added(&mut before, 1_000)
            .expect("stamping");

        let mut after = library(&[scanned("a/1.flac", "Juana Peña", None)]);
        let stamped = store
            .stamp_dates_added(&mut after, 9_000)
            .expect("stamping");
        assert_eq!((stamped.minted, stamped.known), (0, 1));
        assert_eq!(dates(&after), vec![Some(1_000)]);
    }

    #[test]
    fn a_file_that_is_genuinely_new_is_new_beside_files_that_are_not() {
        let mut store = Store::in_memory().expect("a store");
        let mut before = library(&[scanned("a/1.flac", "Juana Peña", None)]);
        store
            .stamp_dates_added(&mut before, 1_000)
            .expect("stamping");

        let mut after = library(&[
            scanned("a/1.flac", "Juana Peña", None),
            scanned("a/2.flac", "Dundunbanza", None),
        ]);
        let stamped = store
            .stamp_dates_added(&mut after, 9_000)
            .expect("stamping");
        assert_eq!((stamped.minted, stamped.known), (1, 1));
        assert_eq!(dates(&after), vec![Some(1_000), Some(9_000)]);
    }

    #[test]
    fn a_date_added_survives_a_restart() {
        let file = OnDisk::new("dates");
        let mut before = library(&[scanned("a/1.flac", "Juana Peña", None)]);
        file.open()
            .stamp_dates_added(&mut before, 1_000)
            .expect("stamping");

        let mut after = library(&[scanned("a/1.flac", "Juana Peña", None)]);
        file.open()
            .stamp_dates_added(&mut after, 9_000)
            .expect("stamping");
        assert_eq!(dates(&after), vec![Some(1_000)]);
    }

    #[test]
    fn the_device_identity_is_minted_once_and_then_never_again() {
        let file = OnDisk::new("udn");
        let minted = file.open().device_udn().expect("minting");
        assert_eq!(file.open().device_udn().expect("reading"), minted);
        assert_eq!(minted.len(), 36, "a UUID in its text form");
    }

    #[test]
    fn a_saved_index_nothing_can_read_is_set_aside_so_the_next_start_is_not_a_cold_one() {
        let file = OnDisk::new("beyond-reading");
        let aside = file.0.with_extension("unreadable");
        let _ = std::fs::remove_file(&aside);
        let junk = b"not a database".to_vec();
        std::fs::write(&file.0, &junk).expect("writing over the store");

        let mut replaced = Store::open_or_replace(&file.0).expect("a working store in its place");
        assert!(
            replaced.device_udn().is_ok(),
            "the store that replaces it answers rather than merely opening"
        );
        assert_eq!(
            std::fs::read(&aside).expect("the old one is kept"),
            junk,
            "it is set aside rather than thrown away"
        );

        // A second corruption keeps the first rescue rather than writing over it. The store has
        // to be closed and its journal removed, or sqlite reads the library out of the WAL.
        drop(replaced);
        for suffix in ["-wal", "-shm"] {
            let mut companion = file.0.as_os_str().to_owned();
            companion.push(suffix);
            let _ = std::fs::remove_file(PathBuf::from(companion));
        }
        let again = b"not a database either".to_vec();
        std::fs::write(&file.0, &again).expect("writing over the store again");
        let second = file.0.with_extension("unreadable.1");
        let _ = std::fs::remove_file(&second);
        Store::open_or_replace(&file.0).expect("a working store in its place");
        assert_eq!(std::fs::read(&aside).expect("still there"), junk);
        assert_eq!(std::fs::read(&second).expect("beside it"), again);
        let _ = std::fs::remove_file(&second);
        let _ = std::fs::remove_file(&aside);
    }

    #[test]
    fn a_state_folder_nothing_has_written_yet_is_not_mistaken_for_a_broken_store() {
        let file = OnDisk::new("empty-store");
        std::fs::write(&file.0, b"").expect("an empty file, which sqlite reads as a new database");
        Store::open_or_replace(&file.0).expect("it opens");
        assert!(
            !file.0.with_extension("unreadable").exists(),
            "a fresh store must not be set aside on its first start"
        );
    }

    #[test]
    fn a_store_that_could_merely_not_be_written_is_never_set_aside() {
        let error = anyhow::anyhow!("disk full").context("opening the store");
        assert!(
            !beyond_reading(&error),
            "a full disk or a read-only folder must not cost the dates added"
        );
    }

    #[test]
    fn a_file_refused_for_a_reason_that_may_pass_is_not_remembered_as_refused() {
        let mut store = Store::in_memory().expect("a store");
        let print = fingerprint(27, 1_700_000_000);
        let refused = |relative: &str, remember: bool| RefusedFile {
            relative: PathBuf::from(relative),
            fingerprint: print,
            why: "Permission denied (os error 13)".to_owned(),
            remember,
        };
        let cache = store.cache(Path::new(ROOT)).expect("a cache");
        let written = store
            .save(
                Path::new(ROOT),
                Reading {
                    walked: &walked(&[], print),
                    files: &[],
                    playlists: &[],
                    refused: &[refused("a/1.flac", true), refused("a/2.flac", false)],
                },
                &cache,
                &whole(),
            )
            .expect("saving");
        assert_eq!(written.refused, 1);

        let cache = store.cache(Path::new(ROOT)).expect("a cache");
        assert!(cache.refused(Path::new("a/1.flac"), print).is_some());
        assert!(
            cache.refused(Path::new("a/2.flac"), print).is_none(),
            "a file that would not open must be tried again, not hidden until something edits it"
        );
    }

    #[test]
    fn a_row_for_a_file_that_will_open_again_is_forgotten_rather_than_kept() {
        let mut store = Store::in_memory().expect("a store");
        let print = fingerprint(27, 1_700_000_000);
        let refused = |remember: bool| RefusedFile {
            relative: PathBuf::from("a/1.flac"),
            fingerprint: print,
            why: "Permission denied (os error 13)".to_owned(),
            remember,
        };
        let save = |store: &mut Store, file: RefusedFile| {
            let cache = store.cache(Path::new(ROOT)).expect("a cache");
            store
                .save(
                    Path::new(ROOT),
                    Reading {
                        walked: &walked(&[], print),
                        files: &[],
                        playlists: &[],
                        refused: &[file],
                    },
                    &cache,
                    &whole(),
                )
                .expect("saving")
        };
        save(&mut store, refused(true));
        assert_eq!(save(&mut store, refused(false)).forgotten, 1);
        let cache = store.cache(Path::new(ROOT)).expect("a cache");
        assert!(cache.refused(Path::new("a/1.flac"), print).is_none());
    }

    #[test]
    fn a_store_written_by_another_build_drops_its_cache_and_keeps_its_dates() {
        let file = OnDisk::new("older");
        let files = [scanned("a/1.flac", "Juana Peña", None)];
        {
            let mut store = file.open();
            saved(&mut store, &files, fingerprint(27, 1_700_000_000));
            let mut built = library(&files);
            store
                .stamp_dates_added(&mut built, 1_000)
                .expect("stamping");
            let udn = store.device_udn().expect("minting");
            store
                .set_meta("schema_version", "0")
                .expect("pretending to be older");
            assert!(!udn.is_empty());
        }

        let mut store = file.open();
        assert!(
            store.cache(Path::new(ROOT)).expect("a cache").is_empty(),
            "a payload this build cannot read is not kept"
        );
        let mut built = library(&files);
        store
            .stamp_dates_added(&mut built, 9_000)
            .expect("stamping");
        assert_eq!(
            dates(&built),
            vec![Some(1_000)],
            "the dates are keyed on identity and outlive any payload"
        );
    }
}
