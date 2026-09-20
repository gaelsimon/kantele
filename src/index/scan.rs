//! Walking the library tree and reading its files.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rayon::prelude::*;

use crate::index::artwork::{self, Artwork};
use crate::index::playlist::is_playlist;
use crate::index::refusals::{Cause, Refusals};
use crate::index::roots::Roots;
use crate::index::store::Cache;
use crate::tags::{self, AudioProperties, FileTags};

fn epoch_seconds() -> i64 {
    std::time::SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or_default()
}

#[derive(Clone, Debug, Default)]
pub struct Stopping(std::sync::Arc<std::sync::atomic::AtomicBool>);

impl Stopping {
    pub fn stop(&self) {
        self.0.store(true, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn asked(&self) -> bool {
        self.0.load(std::sync::atomic::Ordering::Relaxed)
    }
}

/// Where a pass has got to. Idle is also where it ends, since nothing is underway then.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Phase {
    #[default]
    Idle,
    /// Listing the tree, which is the part with no total to count against.
    Walking,
    Reading,
    /// Deriving the albums, the artists and the menus from what was read.
    Building,
}

impl Phase {
    fn from_code(code: u8) -> Self {
        match code {
            1 => Self::Walking,
            2 => Self::Reading,
            3 => Self::Building,
            _ => Self::Idle,
        }
    }

    fn code(self) -> u8 {
        match self {
            Self::Idle => 0,
            Self::Walking => 1,
            Self::Reading => 2,
            Self::Building => 3,
        }
    }
}

/// How far the pass underway has got. Counted as the pass runs, so every field is read while it
/// is still moving.
#[derive(Debug, Default)]
pub struct Progress {
    phase: std::sync::atomic::AtomicU8,
    folders: std::sync::atomic::AtomicUsize,
    found: std::sync::atomic::AtomicUsize,
    read: std::sync::atomic::AtomicUsize,
    /// Seconds since the epoch, or zero where no pass is underway.
    began: std::sync::atomic::AtomicI64,
}

/// What a pass has done so far.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize)]
pub struct Reached {
    pub phase: Phase,
    pub folders: usize,
    /// Files the walk found, which is the total the reads are counted against.
    pub found: usize,
    pub read: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub began: Option<i64>,
}

impl Progress {
    pub fn begin(&self) {
        let order = std::sync::atomic::Ordering::Relaxed;
        self.folders.store(0, order);
        self.found.store(0, order);
        self.read.store(0, order);
        self.began.store(epoch_seconds(), order);
        self.phase.store(Phase::Walking.code(), order);
    }

    pub fn walked(&self, folders: usize) {
        self.folders
            .fetch_add(folders, std::sync::atomic::Ordering::Relaxed);
    }

    /// The walk is over and the reads begin, against the total it found.
    pub fn reading(&self, found: usize) {
        let order = std::sync::atomic::Ordering::Relaxed;
        self.found.store(found, order);
        self.phase.store(Phase::Reading.code(), order);
    }

    pub fn read(&self, files: usize) {
        self.read
            .fetch_add(files, std::sync::atomic::Ordering::Relaxed);
    }

    pub fn building(&self) {
        self.phase
            .store(Phase::Building.code(), std::sync::atomic::Ordering::Relaxed);
    }

    pub fn done(&self) {
        let order = std::sync::atomic::Ordering::Relaxed;
        self.phase.store(Phase::Idle.code(), order);
        self.began.store(0, order);
    }

    pub fn reached(&self) -> Reached {
        let order = std::sync::atomic::Ordering::Relaxed;
        Reached {
            phase: Phase::from_code(self.phase.load(order)),
            folders: self.folders.load(order),
            found: self.found.load(order),
            read: self.read.load(order),
            began: Some(self.began.load(order)).filter(|began| *began > 0),
        }
    }
}

#[derive(Clone, Debug)]
pub struct ScanOptions {
    /// Beyond a handful an ARM NAS goes slower.
    pub threads: usize,
    pub sweep: Option<std::time::Duration>,
    pub exclude: Exclusions,
    pub cover_art: artwork::Prefer,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            threads: 4,
            sweep: Some(std::time::Duration::from_secs(15 * 60)),
            exclude: Exclusions::default(),
            cover_art: artwork::Prefer::default(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Scanned {
    pub path: PathBuf,
    pub relative: PathBuf,
    pub tags: FileTags,
    pub properties: AudioProperties,
    pub size: u64,
    pub artwork: Option<Artwork>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fingerprint {
    pub size: u64,
    /// Whole seconds.
    pub mtime: i64,
}

impl Fingerprint {
    pub const UNKNOWN: Self = Self { size: 0, mtime: 0 };

    fn of(metadata: &std::fs::Metadata) -> Self {
        let mtime = metadata
            .modified()
            .ok()
            .and_then(|at| at.duration_since(UNIX_EPOCH).ok())
            .map(|since| since.as_secs() as i64)
            .unwrap_or_default();
        Self {
            size: metadata.len(),
            mtime,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Found {
    pub path: PathBuf,
    pub fingerprint: Fingerprint,
}

#[derive(Clone, Debug, Default)]
pub struct Walked {
    /// Sorted by path, so one folder's files are adjacent.
    pub files: Vec<Found>,
    pub covers: HashMap<PathBuf, Found>,
    /// Sorted by path.
    pub playlists: Vec<Found>,
    /// The folders the walk could not list, relative and folded, so the rows under one of them
    /// are known to say nothing.
    pub unreadable: Vec<String>,
    pub refusals: Refusals,
    pub stopped: bool,
}

impl Walked {
    pub fn complete(&self) -> bool {
        self.unreadable.is_empty() && !self.stopped
    }

    /// Whether the walk read the folder this file sits in. One folder it could not list says
    /// nothing about the files under it, and says nothing at all about the rest of the tree.
    pub fn reached(&self, relative: &Path) -> bool {
        if self.stopped {
            return false;
        }
        let path = crate::index::fold::path(relative);
        !self.unreadable.iter().any(|folder| under(&path, folder))
    }
}

fn under(path: &str, folder: &str) -> bool {
    path.len() > folder.len() && path.starts_with(folder) && path.as_bytes()[folder.len()] == b'/'
}

const SKIPPED_FOLDERS: &[&str] = &[
    "@eaDir",
    "#recycle",
    "#snapshot",
    "$RECYCLE.BIN",
    "System Volume Information",
    "lost+found",
    "__MACOSX",
];

/// Names and paths the owner keeps out of the library, matched without regard to case.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Exclusions {
    patterns: Vec<String>,
}

impl Exclusions {
    pub fn new<'a>(patterns: impl IntoIterator<Item = &'a str>) -> Self {
        Self {
            patterns: patterns
                .into_iter()
                .map(|pattern| pattern.trim().to_lowercase())
                .filter(|pattern| !pattern.is_empty())
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// A pattern with a `/` is held against the path and every folder above it; one without, against each name.
    pub fn excludes(&self, relative: &Path) -> bool {
        if self.patterns.is_empty() {
            return false;
        }
        let path = crate::index::fold::path(relative).to_lowercase();
        let path = path.trim_matches('/');
        self.patterns
            .iter()
            .any(|pattern| match pattern.contains('/') {
                true => path
                    .match_indices('/')
                    .map(|(at, _)| &path[..at])
                    .chain(std::iter::once(path))
                    .any(|prefix| wild(pattern, prefix)),
                false => path.split('/').any(|name| wild(pattern, name)),
            })
    }
}

/// `*` stands for any run of characters short of a `/`, `?` for one character that is not one.
fn wild(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();
    let (mut p, mut t) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while t < text.len() {
        let literal = p < pattern.len()
            && pattern[p] != '*'
            && (pattern[p] == text[t] || (pattern[p] == '?' && text[t] != '/'));
        if p < pattern.len() && pattern[p] == '*' {
            star = Some((p, t));
            p += 1;
        } else if literal {
            p += 1;
            t += 1;
        } else if let Some((at, from)) = star
            && text[from] != '/'
        {
            p = at + 1;
            t = from + 1;
            star = Some((at, from + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|c| *c == '*')
}

/// Paths are relative to the content root.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Region {
    Folder(PathBuf),
    Subtree(PathBuf),
}

impl Region {
    pub fn folder(&self) -> &Path {
        match self {
            Self::Folder(path) | Self::Subtree(path) => path,
        }
    }

    fn is_subtree(&self) -> bool {
        matches!(self, Self::Subtree(_))
    }

    fn covers(&self, folder: &Path) -> bool {
        match self {
            Self::Folder(path) => folder == path,
            Self::Subtree(path) => folder == path || folder.starts_with(path),
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Scope {
    regions: Vec<Region>,
}

impl Scope {
    pub fn whole_tree() -> Self {
        Self {
            regions: vec![Region::Subtree(PathBuf::new())],
        }
    }

    pub fn of(changed: impl IntoIterator<Item = PathBuf>) -> Self {
        let mut regions: Vec<Region> = Vec::new();
        for path in changed {
            let region = match path.file_name().and_then(|name| name.to_str()) {
                Some(name) if mime_for(&path).is_some() || is_image(name) || is_playlist(name) => {
                    Region::Folder(path.parent().unwrap_or(Path::new("")).to_path_buf())
                }
                _ => Region::Subtree(path),
            };
            if !regions.contains(&region) {
                regions.push(region);
            }
        }
        let mut scope = Self { regions };
        scope.simplify();
        scope
    }

    fn simplify(&mut self) {
        let taken = std::mem::take(&mut self.regions);
        let mut subtrees: Vec<PathBuf> = taken
            .iter()
            .filter_map(|region| match region {
                Region::Subtree(path) => Some(path.clone()),
                Region::Folder(_) => None,
            })
            .collect();
        subtrees.sort();
        let mut kept: Vec<PathBuf> = Vec::new();
        for path in subtrees {
            if !kept.iter().any(|above| path.starts_with(above)) {
                kept.push(path);
            }
        }
        let mut folders: Vec<PathBuf> = taken
            .into_iter()
            .filter_map(|region| match region {
                Region::Folder(path) => Some(path),
                Region::Subtree(_) => None,
            })
            .filter(|folder| !kept.iter().any(|above| folder.starts_with(above)))
            .collect();
        folders.sort();
        folders.dedup();
        self.regions = kept
            .into_iter()
            .map(Region::Subtree)
            .chain(folders.into_iter().map(Region::Folder))
            .collect();
    }

    pub fn regions(&self) -> &[Region] {
        &self.regions
    }

    pub fn is_whole_tree(&self) -> bool {
        *self == Self::whole_tree()
    }

    pub fn is_empty(&self) -> bool {
        self.regions.is_empty()
    }

    pub fn covers(&self, folder: &Path) -> bool {
        self.regions.iter().any(|region| region.covers(folder))
    }

    pub fn covers_file(&self, relative: &Path) -> bool {
        self.covers(relative.parent().unwrap_or(Path::new("")))
    }

    /// The scope that covers what either of them covers.
    pub fn union(&self, other: &Self) -> Self {
        let mut scope = Self {
            regions: self.regions.iter().chain(&other.regions).cloned().collect(),
        };
        scope.simplify();
        scope
    }

    /// Whether a folder on disk is where one of the regions starts.
    fn is_root(&self, roots: &Roots, folder: &Path) -> bool {
        self.regions.iter().any(|region| {
            roots
                .starts(region.folder())
                .iter()
                .any(|start| start == folder)
        })
    }
}

pub fn walk(root: &Path) -> std::io::Result<Walked> {
    walk_within(
        Roots::one(root),
        &Scope::whole_tree(),
        &Underway::default(),
        &Exclusions::default(),
    )
}

/// What a pass obeys and what it reports while it runs, which every part of it is handed.
#[derive(Debug, Default)]
pub struct Underway {
    pub stopping: Stopping,
    pub progress: Progress,
}

impl Underway {
    pub fn with(stopping: Stopping) -> Self {
        Self {
            stopping,
            progress: Progress::default(),
        }
    }

    fn asked_to_stop(&self) -> bool {
        self.stopping.asked()
    }
}

pub fn walk_within(
    roots: impl Into<Roots>,
    scope: &Scope,
    underway: &Underway,
    exclude: &Exclusions,
) -> std::io::Result<Walked> {
    let roots = &roots.into();
    let mut walk = Walk {
        inside: roots
            .paths()
            .map(|root| root.canonicalize().unwrap_or_else(|_| root.to_path_buf()))
            .collect(),
        ..Walk::default()
    };
    for region in scope.regions() {
        for folder in roots.starts(region.folder()) {
            walk.seen
                .insert(folder.canonicalize().unwrap_or_else(|_| folder.clone()));
            walk.folders.push((folder, region.is_subtree()));
        }
    }

    while let Some((folder, descend)) = walk.folders.pop() {
        if underway.asked_to_stop() {
            tracing::info!(
                folders = walk.folders.len() + 1,
                "stopping: leaving the rest of the tree unwalked, and forgetting nothing"
            );
            walk.walked.stopped = true;
            break;
        }
        underway.progress.walked(1);
        let entries = match std::fs::read_dir(&folder) {
            Ok(entries) => entries,
            Err(error) => match unreadable(roots, scope, &folder, error) {
                Some(error) => return Err(error),
                None => {
                    if !scope.is_root(roots, &folder) {
                        let relative = relative_to(roots, &folder);
                        walk.walked.unreadable.push(relative.clone());
                        walk.walked
                            .refusals
                            .refuse(Cause::UnreadableFolder, relative, None);
                    }
                    continue;
                }
            },
        };
        walk.list(roots, exclude, folder, descend, entries);
    }
    walk.walked
        .files
        .sort_by(|left, right| left.path.cmp(&right.path));
    walk.walked
        .playlists
        .sort_by(|left, right| left.path.cmp(&right.path));
    Ok(walk.walked)
}

#[derive(Default)]
struct Walk {
    walked: Walked,
    /// Each folder still to list, and whether to descend into it.
    folders: Vec<(PathBuf, bool)>,
    /// Canonical paths, so a symlink loop is walked once.
    seen: HashSet<PathBuf>,
    /// The music folders, canonical, so a link is known to leave them.
    inside: Vec<PathBuf>,
}

impl Walk {
    fn list(
        &mut self,
        roots: &Roots,
        exclude: &Exclusions,
        folder: PathBuf,
        descend: bool,
        entries: std::fs::ReadDir,
    ) {
        let mut images: Vec<PathBuf> = Vec::new();
        let mut music = false;
        for entry in entries {
            let entry = match entry {
                Ok(entry) => entry,
                // Half a listing is half a library, so the pass is incomplete rather than short.
                Err(error) => {
                    let relative = relative_to(roots, &folder);
                    self.walked.unreadable.push(relative.clone());
                    self.walked.refusals.refuse(
                        Cause::UnreadableFolder,
                        relative,
                        Some(error.to_string()),
                    );
                    continue;
                }
            };
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                self.unnamed(roots, &path);
                continue;
            };
            if is_skipped(name) || exclude.excludes(&roots.relative_or_self(&path)) {
                continue;
            }
            if self.leaves_the_library(roots, &entry, &path) {
                continue;
            }
            if is_folder(&entry, &path) {
                if descend {
                    push_folder(&path, &mut self.folders, &mut self.seen);
                }
            } else if mime_for(&path).is_some() {
                music = true;
                self.walked.files.push(found(&entry, path));
            } else if is_image(name) {
                images.push(path);
            } else if is_playlist(name) {
                self.walked.playlists.push(found(&entry, path));
            }
        }
        self.cover_of(folder, music, &images);
    }

    /// Whether a link points out of every music folder, which the library it names does not
    /// answer for: serving it would put a file nobody offered on the network.
    fn leaves_the_library(
        &mut self,
        roots: &Roots,
        entry: &std::fs::DirEntry,
        path: &Path,
    ) -> bool {
        if !entry.file_type().is_ok_and(|kind| kind.is_symlink()) {
            return false;
        }
        let Ok(target) = path.canonicalize() else {
            return false;
        };
        if self.inside.iter().any(|root| target.starts_with(root)) {
            return false;
        }
        self.walked.refusals.refuse(
            Cause::LinkedOutside,
            relative_to(roots, path),
            Some(format!("it points at {}", target.display())),
        );
        true
    }

    /// A name the file system holds as bytes no text can carry. Said only of what the extension
    /// calls music, since the rest is junk nobody would miss.
    fn unnamed(&mut self, roots: &Roots, path: &Path) {
        if mime_for(path).is_none() {
            return;
        }
        self.walked.refusals.refuse(
            Cause::UnreadableFile,
            relative_to(roots, path),
            Some("its name is not text, so it was left out".to_owned()),
        );
    }

    fn cover_of(&mut self, folder: PathBuf, music: bool, images: &[PathBuf]) {
        if music && let Some(cover) = artwork::preferred_cover(images) {
            let fingerprint = std::fs::metadata(&cover)
                .as_ref()
                .map(Fingerprint::of)
                .unwrap_or(Fingerprint::UNKNOWN);
            self.walked.covers.insert(
                folder,
                Found {
                    path: cover,
                    fingerprint,
                },
            );
        }
    }
}

fn unreadable(
    roots: &Roots,
    scope: &Scope,
    folder: &Path,
    error: std::io::Error,
) -> Option<std::io::Error> {
    if scope.is_root(roots, folder) {
        if error.kind() == std::io::ErrorKind::NotFound && !roots.is_root(folder) {
            tracing::info!(folder = %folder.display(), "gone: its files are forgotten");
            return None;
        }
        return Some(error);
    }
    tracing::warn!(folder = %folder.display(), %error, "skipping unreadable folder");
    None
}

/// Skips the outermost cause, the path the record already names.
fn why(error: &anyhow::Error) -> String {
    let causes: Vec<String> = error.chain().skip(1).map(ToString::to_string).collect();
    match causes.is_empty() {
        true => error.to_string(),
        false => causes.join(": "),
    }
}

pub(crate) fn relative_to(roots: &Roots, path: &Path) -> String {
    crate::index::fold::path(&roots.relative_or_self(path))
}

fn is_folder(entry: &std::fs::DirEntry, path: &Path) -> bool {
    match entry.file_type() {
        Ok(kind) if kind.is_dir() => true,
        Ok(kind) if kind.is_symlink() => path.is_dir(),
        _ => false,
    }
}

fn found(entry: &std::fs::DirEntry, path: PathBuf) -> Found {
    let metadata = match entry.metadata() {
        Ok(metadata) if metadata.is_symlink() => std::fs::metadata(&path),
        other => other,
    };
    let fingerprint = metadata
        .as_ref()
        .map(Fingerprint::of)
        .unwrap_or(Fingerprint::UNKNOWN);
    Found { path, fingerprint }
}

pub(crate) fn is_image(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".jpg", ".jpeg", ".png"]
        .iter()
        .any(|extension| lower.ends_with(extension))
}

fn push_folder(path: &Path, folders: &mut Vec<(PathBuf, bool)>, seen: &mut HashSet<PathBuf>) {
    let real = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if seen.insert(real) {
        folders.push((path.to_path_buf(), true));
    } else {
        tracing::debug!(folder = %path.display(), "already walked, not following");
    }
}

pub(crate) fn is_skipped(name: &str) -> bool {
    name.starts_with('.')
        || SKIPPED_FOLDERS
            .iter()
            .any(|skipped| name.eq_ignore_ascii_case(skipped))
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RefusedFile {
    pub relative: PathBuf,
    pub fingerprint: Fingerprint,
    pub why: String,
    /// Whether the file's own bytes explain this, which is what makes it worth remembering.
    pub remember: bool,
}

type Read = (Vec<Scanned>, Vec<RefusedFile>);

/// Keeps the walk's order.
pub fn read(
    roots: impl Into<Roots>,
    walked: &Walked,
    options: &ScanOptions,
    cache: &Cache,
    underway: &Underway,
) -> (Vec<Scanned>, Vec<RefusedFile>, Refusals) {
    let roots = &roots.into();
    let folders: Vec<&[Found]> = walked
        .files
        .chunk_by(|left, right| left.path.parent() == right.path.parent())
        .collect();
    let read_all = || -> Vec<Read> {
        folders
            .par_iter()
            .map(|folder| read_folder(roots, folder, walked, cache, underway, options.cover_art))
            .collect()
    };

    let read = match rayon::ThreadPoolBuilder::new()
        .num_threads(options.threads.max(1))
        .build()
    {
        Ok(pool) => pool.install(read_all),
        Err(error) => {
            tracing::warn!(%error, "no thread pool: scanning on one thread");
            folders
                .iter()
                .map(|folder| {
                    read_folder(roots, folder, walked, cache, underway, options.cover_art)
                })
                .collect()
        }
    };

    let mut files: Vec<Scanned> = Vec::with_capacity(walked.files.len());
    let mut refused: Vec<RefusedFile> = Vec::new();
    let mut refusals = Refusals::default();
    for (scanned, denied) in read {
        files.extend(scanned);
        for file in denied {
            refusals.refuse(
                Cause::UnreadableFile,
                crate::index::fold::path(&file.relative),
                Some(file.why.clone()),
            );
            refused.push(file);
        }
    }
    (files, refused, refusals)
}

fn read_folder(
    roots: &Roots,
    folder: &[Found],
    walked: &Walked,
    cache: &Cache,
    underway: &Underway,
    prefer: artwork::Prefer,
) -> Read {
    if underway.asked_to_stop() {
        return (Vec::new(), Vec::new());
    }
    underway.progress.read(folder.len());
    let cover = folder
        .first()
        .and_then(|found| found.path.parent())
        .and_then(|parent| walked.covers.get(parent))
        .and_then(|image| cover_for(image, cache));
    let mut scanned = Vec::with_capacity(folder.len());
    let mut refused = Vec::new();
    for found in folder {
        let relative = roots.relative_or_self(&found.path);
        if let Some(reason) = cache.refused(&relative, found.fingerprint) {
            refused.push(RefusedFile {
                relative,
                fingerprint: found.fingerprint,
                why: reason.to_owned(),
                remember: true,
            });
            continue;
        }
        match read_file(roots, found, cover.clone(), cache, prefer) {
            Ok(file) => scanned.push(file),
            Err(error) => {
                tracing::warn!(path = %found.path.display(), %error, "skipping file");
                refused.push(RefusedFile {
                    relative,
                    fingerprint: found.fingerprint,
                    why: why(&error),
                    remember: contents_explain(&found.path),
                });
            }
        }
    }
    (scanned, refused)
}

/// A file that will not open may open next time: a permission, a share half mounted, another
/// process holding it. Remembering that refusal would hide the file until something edited it.
fn contents_explain(path: &Path) -> bool {
    std::fs::File::open(path).is_ok()
}

fn cover_for(image: &Found, cache: &Cache) -> Option<Artwork> {
    cache
        .cover(image)
        .or_else(|| artwork::describe(&image.path))
}

fn read_file(
    roots: &Roots,
    found: &Found,
    cover: Option<Artwork>,
    cache: &Cache,
    prefer: artwork::Prefer,
) -> anyhow::Result<Scanned> {
    let path = &found.path;
    let relative = roots.relative_or_self(path);
    if let Some(cached) = cache.file(&relative, found.fingerprint) {
        return Ok(Scanned {
            relative,
            size: found.fingerprint.size,
            artwork: match prefer {
                artwork::Prefer::Folder => cover.or_else(|| reuse_embedded(&cached.artwork, path)),
                artwork::Prefer::Embedded => reuse_embedded(&cached.artwork, path).or(cover),
            },
            path: path.to_path_buf(),
            tags: cached.tags,
            properties: cached.properties,
        });
    }
    // The picture comes out of the parse this read already pays for, where it is wanted at all.
    let wanted = match prefer {
        artwork::Prefer::Embedded => tags::Cover::Wanted,
        artwork::Prefer::Folder if cover.is_none() => tags::Cover::Wanted,
        artwork::Prefer::Folder => tags::Cover::Skipped,
    };
    let (tags, properties, picture) = tags::read_keeping(path, wanted)?;
    let embedded = picture.and_then(|bytes| artwork::of_picture(path, &bytes));
    let size = match found.fingerprint {
        Fingerprint::UNKNOWN => std::fs::metadata(path)?.len(),
        fingerprint => fingerprint.size,
    };
    Ok(Scanned {
        relative,
        size,
        artwork: match prefer {
            artwork::Prefer::Folder => cover.or(embedded),
            artwork::Prefer::Embedded => embedded.or(cover),
        },
        path: path.to_path_buf(),
        tags,
        properties,
    })
}

fn reuse_embedded(cached: &Option<Artwork>, path: &Path) -> Option<Artwork> {
    match cached {
        Some(Artwork {
            source: artwork::Source::Embedded { .. },
            ..
        }) => cached.clone(),
        Some(_) => artwork::embedded(path),
        None => None,
    }
}

/// Also decides what counts as music.
pub fn mime_for(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "flac" => "audio/x-flac",
        "mp3" => "audio/mpeg",
        "m4a" | "mp4" => "audio/mp4",
        "ogg" | "oga" => "audio/ogg",
        "opus" => "audio/opus",
        "wav" => "audio/x-wav",
        "aiff" | "aif" => "audio/x-aiff",
        "wv" => "audio/x-wavpack",
        "ape" => "audio/x-monkeys-audio",
        // Each DSD container has its own MIME.
        "dsf" => "audio/x-dsf",
        "dff" => "audio/x-dff",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_file_that_opens_is_refused_for_a_reason_worth_remembering() {
        let here = std::env::temp_dir().join("kantele-scan-contents-explain.flac");
        std::fs::write(&here, b"not really audio").expect("writing a test file");
        assert!(
            contents_explain(&here),
            "a file that opens and will not parse is refused by its own bytes"
        );
        let _ = std::fs::remove_file(&here);
        assert!(
            !contents_explain(&here),
            "and one that is not there may be there next time"
        );
    }

    struct Tree(PathBuf);

    impl Tree {
        fn new(name: &str, files: &[&str]) -> Self {
            let root = std::env::temp_dir().join(format!("kantele-scan-{name}"));
            let _ = std::fs::remove_dir_all(&root);
            for file in files {
                let path = root.join(file);
                std::fs::create_dir_all(path.parent().expect("a file has a parent"))
                    .expect("creating a test folder");
                std::fs::write(&path, b"not really audio").expect("writing a test file");
            }
            std::fs::create_dir_all(&root).expect("creating the test root");
            Self(root)
        }

        fn relative(&self) -> Vec<String> {
            let mut found: Vec<String> = walk(&self.0)
                .expect("walking the test tree")
                .files
                .iter()
                .map(|found| self.name(&found.path))
                .collect();
            found.sort();
            found
        }

        fn name(&self, path: &Path) -> String {
            path.strip_prefix(&self.0)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/")
        }

        fn covers(&self) -> Vec<(String, String)> {
            let mut covers: Vec<(String, String)> = walk(&self.0)
                .expect("walking the test tree")
                .covers
                .iter()
                .map(|(folder, image)| (self.name(folder), self.name(&image.path)))
                .collect();
            covers.sort();
            covers
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_scope_keeps_only_the_regions_no_other_region_already_covers() {
        let scope = Scope::of([
            PathBuf::from("a/b/1.flac"),
            PathBuf::from("a/b/2.flac"),
            PathBuf::from("a/b"),
            PathBuf::from("a/b/deeper"),
            PathBuf::from("c/1.flac"),
        ]);
        assert_eq!(
            scope.regions(),
            [
                Region::Subtree(PathBuf::from("a/b")),
                Region::Folder(PathBuf::from("c")),
            ]
        );
    }

    #[test]
    fn a_scope_answers_for_the_folders_it_walked_and_no_others() {
        let scope = Scope::of([PathBuf::from("a/b/1.flac"), PathBuf::from("c")]);
        assert!(scope.covers(Path::new("a/b")));
        assert!(
            !scope.covers(Path::new("a/b/deeper")),
            "a file asks for its own folder only"
        );
        assert!(!scope.covers(Path::new("a")));
        assert!(scope.covers(Path::new("c")));
        assert!(
            scope.covers(Path::new("c/inside")),
            "a folder asks for its subtree"
        );
        assert!(scope.covers_file(Path::new("a/b/2.flac")));
        assert!(!scope.covers_file(Path::new("a/2.flac")));
    }

    #[test]
    fn the_whole_tree_is_the_subtree_at_the_root() {
        let whole = Scope::whole_tree();
        assert!(whole.is_whole_tree());
        assert!(whole.covers(Path::new("")));
        assert!(whole.covers(Path::new("a/b/c")));
        assert!(
            !Scope::default().covers(Path::new("")),
            "nothing is not everything"
        );
    }

    #[test]
    fn a_wildcard_stands_for_anything_short_of_a_separator() {
        assert!(wild("*.iso", "disc.iso"));
        assert!(wild("disc?.iso", "disc1.iso"));
        assert!(!wild("*.iso", "a/disc.iso"), "a star stays inside one name");
        assert!(!wild("a?c", "a/c"));
        assert!(wild("various/live*", "various/live 2024"));
        assert!(!wild("various/live*", "various/live 2024/01.flac"));
        assert!(wild("*", "anything"));
        assert!(!wild("podcast", "podcasts"));
    }

    #[test]
    fn an_exclusion_names_a_file_a_folder_or_a_path_under_the_root() {
        let exclude = Exclusions::new(["*.iso", "Podcasts", "Various/Live*"]);
        assert!(
            exclude.excludes(Path::new("Rips/Disc.ISO")),
            "case does not matter"
        );
        assert!(exclude.excludes(Path::new("podcasts")));
        assert!(
            exclude.excludes(Path::new("Spoken/Podcasts/ep.mp3")),
            "a name pattern reaches a folder anywhere in the tree"
        );
        assert!(exclude.excludes(Path::new("Various/Live 2024")));
        assert!(
            exclude.excludes(Path::new("Various/Live 2024/01.flac")),
            "a path pattern reaches everything under the folder it names"
        );
        assert!(!exclude.excludes(Path::new("Various/Studio/01.flac")));
        assert!(
            !exclude.excludes(Path::new("Live 2024/01.flac")),
            "a path pattern is anchored at the root"
        );
        assert!(!Exclusions::default().excludes(Path::new("anything")));
    }

    #[test]
    fn an_excluded_file_or_folder_is_not_walked() {
        let tree = Tree::new(
            "excluded",
            &["a/1.flac", "a/disc.iso", "Podcasts/ep.mp3", "b/2.flac"],
        );
        let walked = walk_within(
            &tree.0,
            &Scope::whole_tree(),
            &Underway::default(),
            &Exclusions::new(["*.iso", "podcasts"]),
        )
        .expect("walking");
        let mut names: Vec<String> = walked.files.iter().map(|f| tree.name(&f.path)).collect();
        names.sort();
        assert_eq!(names, vec!["a/1.flac", "b/2.flac"]);
        assert!(walked.complete(), "leaving something out is not a hole");
    }

    #[test]
    fn a_walk_confined_to_one_folder_does_not_descend_and_a_subtree_does() {
        let tree = Tree::new("scoped", &["a/1.flac", "a/deep/2.flac", "b/3.flac"]);
        let folder = walk_within(
            &tree.0,
            &Scope::of([PathBuf::from("a/1.flac")]),
            &Underway::default(),
            &Exclusions::default(),
        )
        .expect("walking one folder");
        assert_eq!(
            folder
                .files
                .iter()
                .map(|f| tree.name(&f.path))
                .collect::<Vec<_>>(),
            vec!["a/1.flac"]
        );

        let subtree = walk_within(
            &tree.0,
            &Scope::of([PathBuf::from("a")]),
            &Underway::default(),
            &Exclusions::default(),
        )
        .expect("walking a subtree");
        let mut names: Vec<String> = subtree.files.iter().map(|f| tree.name(&f.path)).collect();
        names.sort();
        assert_eq!(names, vec!["a/1.flac", "a/deep/2.flac"]);
    }

    #[test]
    fn a_pass_asked_to_stop_leaves_the_tree_and_says_it_did_not_finish() {
        let tree = Tree::new("stopped", &["a/1.flac", "b/2.flac", "c/3.flac", "d/4.flac"]);
        let underway = Underway::default();
        underway.stopping.stop();

        let walked = walk_within(
            &tree.0,
            &Scope::whole_tree(),
            &underway,
            &Exclusions::default(),
        )
        .expect("a stop is not a failure");
        assert!(
            !walked.complete(),
            "an unfinished pass is not an answer about what it covered"
        );
        assert!(
            walked.unreadable.is_empty(),
            "and it is not a folder that would not open, which is a different thing"
        );
        assert!(walked.refusals.total() == 0, "nothing was refused");

        let (files, denied, refused) = read(
            &tree.0,
            &walked,
            &ScanOptions::default(),
            &Cache::default(),
            &underway,
        );
        assert!(files.is_empty(), "no file is opened after the stop");
        assert!(denied.is_empty());
        assert_eq!(refused.total(), 0);
    }

    #[test]
    fn a_pass_nobody_stopped_walks_the_whole_tree_and_says_so() {
        let tree = Tree::new("not-stopped", &["a/1.flac", "b/2.flac"]);
        let walked = walk_within(
            &tree.0,
            &Scope::whole_tree(),
            &Underway::default(),
            &Exclusions::default(),
        )
        .expect("walking");
        assert_eq!(walked.files.len(), 2);
        assert!(walked.complete());
        assert!(!walked.stopped);
    }

    #[test]
    fn a_region_that_is_gone_reads_as_empty_rather_than_failing() {
        let tree = Tree::new("scoped-missing", &["a/1.flac"]);
        let walked = walk_within(
            &tree.0,
            &Scope::of([PathBuf::from("gone")]),
            &Underway::default(),
            &Exclusions::default(),
        )
        .expect("a folder that is not there is not an error");
        assert!(walked.files.is_empty());
        assert!(walked.complete(), "a deletion is not a hole in the library");
    }

    #[test]
    fn the_walk_is_recursive_and_sorted() {
        let tree = Tree::new(
            "recursive",
            &["b/2.flac", "a/1.flac", "a/deep/deeper/3.mp3"],
        );
        assert_eq!(
            tree.relative(),
            vec!["a/1.flac", "a/deep/deeper/3.mp3", "b/2.flac"]
        );
    }

    #[test]
    fn only_music_is_collected() {
        let tree = Tree::new(
            "filter",
            &["a/1.flac", "a/cover.jpg", "a/notes.txt", "a/playlist.m3u"],
        );
        assert_eq!(tree.relative(), vec!["a/1.flac"]);
    }

    #[test]
    fn the_platform_the_server_is_packaged_for_is_not_walked() {
        let tree = Tree::new(
            "synology",
            &[
                "a/1.flac",
                "a/@eaDir/1.flac/SYNOPHOTO_THUMB_M.flac",
                "#recycle/deleted.flac",
                ".hidden/2.flac",
            ],
        );
        assert_eq!(tree.relative(), vec!["a/1.flac"]);
    }

    #[test]
    fn an_archive_unpacked_on_a_mac_does_not_add_phantom_tracks() {
        let tree = Tree::new(
            "appledouble",
            &[
                "album/01 Real.mp3",
                "album/__MACOSX/album/._01 Real.mp3",
                "__MACOSX/album/._02 Real.mp3",
                "album/._03 Real.mp3",
            ],
        );
        assert_eq!(tree.relative(), vec!["album/01 Real.mp3"]);
    }

    #[test]
    fn only_audio_extensions_are_served() {
        assert_eq!(mime_for(Path::new("a.flac")), Some("audio/x-flac"));
        assert_eq!(mime_for(Path::new("a.wav")), Some("audio/x-wav"));
        assert_eq!(mime_for(Path::new("a.aiff")), Some("audio/x-aiff"));
        assert_eq!(mime_for(Path::new("a.FLAC")), Some("audio/x-flac"));
        assert_eq!(mime_for(Path::new("cover.jpg")), None);
        assert_eq!(mime_for(Path::new("playlist.m3u")), None);
        assert_eq!(mime_for(Path::new("noextension")), None);
    }

    #[test]
    fn the_default_parallelism_is_low_enough_for_a_nas() {
        assert!(ScanOptions::default().threads <= 4);
    }

    #[test]
    fn a_root_that_cannot_be_read_is_an_error_rather_than_an_empty_library() {
        let missing = std::env::temp_dir().join("kantele-scan-no-such-folder");
        let _ = std::fs::remove_dir_all(&missing);
        assert!(walk(&missing).is_err());
    }

    #[test]
    fn the_walk_chooses_each_folder_s_cover_from_the_listing_it_already_has() {
        let tree = Tree::new(
            "covers",
            &[
                "a/1.flac",
                "a/folder.jpg",
                "a/cover.jpg",
                "b/1.flac",
                "b/front.png",
            ],
        );
        assert_eq!(
            tree.covers(),
            vec![
                ("a".to_owned(), "a/cover.jpg".to_owned()),
                ("b".to_owned(), "b/front.png".to_owned()),
            ]
        );
    }

    #[test]
    fn a_folder_holding_no_music_offers_no_cover() {
        let tree = Tree::new("cover-alone", &["art/cover.jpg", "a/1.flac"]);
        assert_eq!(tree.covers(), Vec::<(String, String)>::new());
    }

    #[test]
    fn a_walked_file_carries_the_fingerprint_that_decides_whether_to_reopen_it() {
        let tree = Tree::new("fingerprint", &["a/1.flac"]);
        let walked = walk(&tree.0).expect("walking the test tree");
        let found = walked.files.first().expect("one file");
        assert_eq!(found.fingerprint.size, b"not really audio".len() as u64);
        assert_ne!(found.fingerprint, Fingerprint::UNKNOWN);
    }
}
