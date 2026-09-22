//! Keeping the index and the store in step, at startup and for as long as the server runs.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use arc_swap::{ArcSwap, ArcSwapOption};
use tokio::sync::mpsc;

use crate::browse;
use crate::index::refusals::{Cause, Refusals};
use crate::index::scan::Underway;
use crate::index::store::Cache;
use crate::index::{Library, Roots, Scan, ScanOptions, Store, scan, sweep, watch};
use crate::upnp::device::Device;

/// The folders a pass covered, which is what a report of it hands on.
pub use crate::index::scan::Scope;

/// Folders a sweep may hand to a scoped pass; past that the whole tree is cheaper to walk again.
const MANY_FOLDERS: usize = 64;

/// The passes: the one running, the last one, and the ones asked for over the interface.
pub struct Passes {
    last: ArcSwapOption<PassReport>,
    running: AtomicBool,
    requests: mpsc::Sender<()>,
    asked: std::sync::Mutex<Option<mpsc::Receiver<()>>>,
    /// What has been asked for and not yet taken, folded into one pass.
    pending: std::sync::Mutex<Option<Pass>>,
    /// When the run of passes that answered nothing began, or zero where the last one answered.
    failing_since: std::sync::atomic::AtomicI64,
    /// Seconds between timed looks at the library as they run now, or zero where none run.
    sweep_seconds: std::sync::atomic::AtomicU64,
}

impl Default for Passes {
    fn default() -> Self {
        let (requests, asked) = mpsc::channel(1);
        Self {
            last: ArcSwapOption::empty(),
            running: AtomicBool::new(false),
            requests,
            asked: std::sync::Mutex::new(Some(asked)),
            pending: std::sync::Mutex::new(None),
            failing_since: std::sync::atomic::AtomicI64::new(0),
            sweep_seconds: std::sync::atomic::AtomicU64::new(0),
        }
    }
}

impl Passes {
    /// The interval the timed looks run at now, stretched where the folders are watched.
    pub fn sweep(&self) -> Option<Duration> {
        let seconds = self.sweep_seconds.load(Ordering::Relaxed);
        (seconds > 0).then(|| Duration::from_secs(seconds))
    }

    fn note_sweep(&self, every: Option<Duration>) {
        self.sweep_seconds
            .store(every.map_or(0, |every| every.as_secs()), Ordering::Relaxed);
    }

    pub fn last(&self) -> Option<Arc<PassReport>> {
        self.last.load_full()
    }

    pub fn record(&self, pass: PassReport) {
        match pass.outcome.answered() {
            true => self.failing_since.store(0, Ordering::Relaxed),
            false => {
                let _ = self.failing_since.compare_exchange(
                    0,
                    pass.ended,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                );
            }
        }
        self.last.store(Some(Arc::new(pass)));
    }

    /// When the library stopped being answered for, or nothing where the last pass answered.
    pub fn failing_since(&self) -> Option<i64> {
        match self.failing_since.load(Ordering::Relaxed) {
            0 => None,
            since => Some(since),
        }
    }

    pub fn running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Whether this caller is the one that started it.
    pub fn begin(&self) -> bool {
        !self.running.swap(true, Ordering::Relaxed)
    }

    pub fn end(&self) {
        self.running.store(false, Ordering::Relaxed);
    }

    /// Asks for a pass, folded into whatever is already waiting: two folders make one pass, and a
    /// pass over everything absorbs a pass over one folder.
    pub fn request(&self, wanted: Pass) -> Asked {
        let mut pending = crate::held(&self.pending);
        let merged = match &*pending {
            Some(held) => held.merged(&wanted),
            None => wanted,
        };
        let widened = pending.as_ref() != Some(&merged);
        *pending = Some(merged);
        drop(pending);
        match (self.requests.try_send(()), widened) {
            (Ok(()), _) => Asked::Queued,
            // The nudge is in the slot already, and the pass it will take is wider than it was.
            (Err(mpsc::error::TrySendError::Full(())), true) => Asked::Queued,
            (Err(mpsc::error::TrySendError::Full(())), false) => Asked::AlreadyWaiting,
            (Err(mpsc::error::TrySendError::Closed(())), _) => Asked::NobodyIsListening,
        }
    }

    /// The pass that was asked for, which nothing is waiting on once it is taken.
    pub fn taken(&self) -> Pass {
        crate::held(&self.pending).take().unwrap_or(Pass::Whole)
    }

    /// The receiving end, handed out once to whoever runs the passes.
    pub fn requests_asked_for(&self) -> Option<mpsc::Receiver<()>> {
        crate::held(&self.asked).take()
    }
}

/// What asking for a pass came to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    Queued,
    AlreadyWaiting,
    NobodyIsListening,
}

/// How a start reads the library.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Start {
    /// Serve what the store remembers, then walk the tree and replace it.
    #[default]
    Cached,
    /// Serve what the store remembers and walk nothing until something changes.
    Remembered,
    /// Ignore what the store remembers and read every file again.
    Reread,
}

/// What every pass over the library needs besides the tree itself.
#[derive(Clone, Debug, Default)]
pub struct Indexing {
    pub roots: Roots,
    pub options: ScanOptions,
    pub menus: crate::browse::Settings,
    /// What the pass underway obeys and reports, shared with whoever asks it to stop or watches it.
    pub underway: Arc<Underway>,
}

impl Indexing {
    /// The defaults, for a caller that only knows which folder to serve.
    pub fn of(roots: impl Into<Roots>) -> Self {
        Self {
            roots: roots.into(),
            ..Self::default()
        }
    }

    /// What a configuration asks for, sharing the stop and the progress already in hand.
    pub fn of_config(config: &crate::config::Config, underway: Arc<Underway>) -> Result<Self> {
        Ok(Self {
            roots: roots_named(config, Missing::Kept)?,
            options: config.scan_options(),
            menus: config.menu_settings(),
            underway,
        })
    }

    fn root(&self) -> &Roots {
        &self.roots
    }
}

/// What a folder that is not on the disk means.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Missing {
    /// Refused. An owner naming a folder on the page is told now, not at the next pass.
    Refused,
    /// Kept as written. A share that is not mounted yet must not stop the server: it serves what
    /// it remembers, says the folder is not readable, and finds it when it comes back.
    Kept,
}

/// The music folders a configuration names, as they are on disk. None named is a server waiting
/// for the page to choose one.
pub fn roots_of(config: &crate::config::Config) -> Result<Roots> {
    roots_named(config, Missing::Refused)
}

pub fn roots_named(config: &crate::config::Config, missing: Missing) -> Result<Roots> {
    let named = config.content_dirs();
    if named.is_empty() {
        return Ok(Roots::default());
    }
    let mut folders = Vec::with_capacity(named.len());
    for folder in named {
        match folder.canonicalize() {
            Ok(path) => folders.push(path),
            Err(error) if missing == Missing::Kept => {
                tracing::warn!(
                    folder = %folder.display(), %error,
                    "this music folder is not there: serving what is remembered of it and \
                     looking again at every pass"
                );
                folders.push(folder);
            }
            Err(error) => {
                return Err(anyhow::Error::new(error))
                    .with_context(|| format!("no such folder: {}", folder.display()));
            }
        }
    }
    Ok(Roots::new(folders)?)
}

/// The indexing settings in force, which a written setting may replace while the server runs.
#[derive(Default)]
pub struct Live {
    indexing: ArcSwap<Indexing>,
    changed: tokio::sync::Notify,
}

impl Live {
    pub fn new(indexing: Indexing) -> Self {
        Self {
            indexing: ArcSwap::from_pointee(indexing),
            changed: tokio::sync::Notify::new(),
        }
    }

    pub fn now(&self) -> Arc<Indexing> {
        self.indexing.load_full()
    }

    /// Keeps the stop this process already holds, so a shutdown still reaches a pass underway.
    pub fn replace(&self, indexing: Indexing) {
        let underway = self.now().underway.clone();
        self.indexing.store(Arc::new(Indexing {
            underway,
            ..indexing
        }));
        self.changed.notify_one();
    }

    /// Resolves the next time somebody replaces these settings.
    pub async fn changed(&self) {
        self.changed.notified().await;
    }
}

/// An index built from what the store remembers, with no round trip to the library at all.
/// What the store remembers, built from one reading of its rows: the cache stays with the store
/// for the first pass, which would otherwise parse every payload a second time.
pub fn remembered(indexing: &Indexing, store: &mut Option<Store>) -> Option<Library> {
    let content_dir = indexing.root();
    let started = std::time::Instant::now();
    let cache = cache_for(content_dir, store);
    let files = match store.as_ref()?.describes_roots(content_dir.clone()) {
        Ok(true) => cache.remembered(),
        Ok(false) => return None,
        Err(error) => {
            tracing::warn!(%error, "the store held no usable rows: reading every file");
            return None;
        }
    };
    if files.is_empty() {
        return None;
    }
    let rows = files.len();
    let playlists = cache.remembered_playlists();
    let held = store.as_ref()?.claims().unwrap_or_default();
    if let Some(store) = store.as_mut() {
        store.hold(cache);
    }
    let mut library = Library::build_holding(
        content_dir.name(),
        &files,
        &playlists,
        &indexing.menus.ignored(),
        &held,
    );
    drop(files);
    if let Some(store) = store.as_mut()
        && let Err(error) = store.stamp_dates_added(&mut library, now())
    {
        tracing::warn!(%error, "dates added not read back");
    }
    tracing::info!(
        folder = %content_dir.describe(),
        tracks = library.len(),
        albums = library.albums().len(),
        playlists = library.playlists().len(),
        rows,
        seconds = started.elapsed().as_secs_f32(),
        "serving what the store remembers while the library is walked"
    );
    Some(library)
}

fn cache_for(content_dir: &Roots, store: &Option<Store>) -> Cache {
    match store.as_ref().map(|store| store.cache(content_dir)) {
        Some(Ok(cache)) => cache,
        Some(Err(error)) => {
            tracing::warn!(%error, "the store held no usable cache: reading every file");
            Cache::default()
        }
        None => Cache::default(),
    }
}

/// What one pass covers, and what it is allowed to trust.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Pass {
    /// Every folder, reading only the files the store cannot answer for.
    Whole,
    /// Every folder, reading every file, whatever the store says.
    Reread,
    /// The folders a change touched, with the rest of the library taken from the store.
    Within(Scope),
}

impl Pass {
    fn scope(&self) -> Scope {
        match self {
            Self::Whole | Self::Reread => Scope::whole_tree(),
            Self::Within(scope) => scope.clone(),
        }
    }

    /// The one pass that answers for both.
    fn merged(&self, other: &Self) -> Self {
        match (self, other) {
            (Self::Reread, _) | (_, Self::Reread) => Self::Reread,
            (Self::Whole, _) | (_, Self::Whole) => Self::Whole,
            (Self::Within(mine), Self::Within(theirs)) => Self::Within(mine.union(theirs)),
        }
    }

    /// What a pass over these folders is called in a log and in the interface.
    pub fn describe(&self) -> String {
        match self {
            Self::Whole => "the whole library".to_owned(),
            Self::Reread => "the whole library, every file read again".to_owned(),
            Self::Within(scope) => match scope.regions() {
                [one] => one.folder().display().to_string(),
                many => format!("{} folders", many.len()),
            },
        }
    }

    /// The pass a set of changed paths asks for.
    pub fn for_change(roots: impl Into<Roots>, change: &watch::Change) -> Self {
        if change.whole_tree || change.paths.is_empty() {
            return Self::Whole;
        }
        let roots = roots.into();
        let mut relative = Vec::with_capacity(change.paths.len());
        for path in &change.paths {
            match roots.relative(path) {
                Some(inside) => relative.push(inside),
                None => {
                    tracing::info!(
                        path = %path.display(),
                        "a change outside the content root: walking the whole tree"
                    );
                    return Self::Whole;
                }
            }
        }
        let scope = Scope::of(relative);
        if scope.is_whole_tree() || scope.is_empty() {
            return Self::Whole;
        }
        Self::Within(scope)
    }
}

/// One pass over the library: read what changed, stamp the dates, write the rows back.
pub fn index(indexing: &Indexing, store: &mut Option<Store>, pass: Pass) -> Result<Indexed> {
    let indexed = indexing_pass(indexing, store, pass);
    // However it ended, nothing is underway now.
    indexing.underway.progress.done();
    indexed
}

fn indexing_pass(indexing: &Indexing, store: &mut Option<Store>, pass: Pass) -> Result<Indexed> {
    let content_dir = indexing.root();
    let cache = if pass == Pass::Reread {
        tracing::info!("ignoring the store's cache on request: every file will be read");
        Cache::default()
    } else {
        match store
            .as_mut()
            .and_then(|store| store.held_cache(content_dir))
        {
            Some(held) => held,
            None => cache_for(content_dir, store),
        }
    };
    let pass = match pass {
        // The rest of the library comes from the rows.
        Pass::Within(_) if cache.is_empty() => {
            tracing::warn!(
                "the store answers for no file, so the folders that changed cannot be joined to \
                 the rest of the library: walking the whole tree"
            );
            Pass::Whole
        }
        pass => pass,
    };
    let scope = pass.scope();
    if let Pass::Within(within) = &pass {
        tracing::info!(
            folders = within.regions().len(),
            first = %within.regions()[0].folder().display(),
            "rescanning the folders that changed rather than the tree"
        );
    }

    let started = std::time::Instant::now();
    let mut scan = Library::scan_scoped(
        content_dir,
        &indexing.options,
        &cache,
        &scope,
        &indexing.underway,
        &indexing.menus.ignored(),
    )?;
    // What the pass produced.
    let opened = scan.files.len().saturating_sub(cache.hits());
    report_pass(content_dir, &scan, opened, &cache, started);

    let complete = scan.walked.complete();
    let persisted = match store.as_mut() {
        // With no store there is nothing to compare against.
        None => Persisted {
            changed: true,
            stored: false,
        },
        Some(store) => persist(store, content_dir, &mut scan, &cache, &scope),
    };
    // A whole pass that wrote is the moment the store is worth copying: the copy is what serves
    // the day the file will not open, with the identity and the dates added still in it.
    if persisted.stored
        && scope.is_whole_tree()
        && let Some(store) = store.as_ref()
    {
        match store.copy_beside() {
            Ok(copy) => tracing::info!(copy = %copy.display(), "the store is copied beside itself"),
            Err(error) => tracing::warn!(%error, "the store was not copied"),
        }
    }
    let report = PassReport {
        ended: now(),
        seconds: started.elapsed().as_secs_f32(),
        whole_tree: scope.is_whole_tree(),
        covered: scope.clone(),
        found: scan.walked.files.len(),
        opened,
        reused: cache.hits(),
        complete,
        // Whoever asked for the pass says what becomes of the index it built.
        outcome: Outcome::Ran,
        stored: persisted.stored,
        refusals: std::mem::take(&mut scan.refusals),
    };
    Ok(Indexed {
        complete,
        changed: persisted.changed,
        library: scan.library,
        pass: report,
    })
}

/// What one pass cost and what the store was worth on it.
fn report_pass(
    content_dir: &Roots,
    scan: &Scan,
    opened: usize,
    cache: &Cache,
    started: std::time::Instant,
) {
    let library = &scan.library;
    tracing::info!(
        folder = %content_dir.describe(),
        tracks = library.len(),
        albums = library.albums().len(),
        artists = library.artists().len(),
        playlists = library.playlists().len(),
        reused = cache.hits(),
        opened,
        found = scan.walked.files.len(),
        seconds = started.elapsed().as_secs_f32(),
        // A pass that left early built an index of part of the tree.
        "{}",
        if scan.walked.stopped {
            "pass stopped before it finished"
        } else {
            "library ready"
        }
    );
    if cache.unreadable() > 0 {
        tracing::warn!(
            rows = cache.unreadable(),
            "rows in the store could not be read and were ignored: those files were read again"
        );
    }
}

/// Stamps the dates and writes the rows back, and says whether anything moved and whether it stuck.
fn persist(
    store: &mut Store,
    content_dir: &Roots,
    scan: &mut Scan,
    cache: &Cache,
    scope: &Scope,
) -> Persisted {
    let mut changed = false;
    let mut stored = true;
    // Before the rows, so a pass that dies half-written still hands the next one the awards it
    // made: an album keeping its identifier matters more than the rows being current.
    if cache.claims() != scan.library.claims()
        && let Err(error) = store.remember_claims(scan.library.claims())
    {
        stored = false;
        tracing::warn!(%error, "album identities not remembered: a new folder may rename one");
    }
    match store.stamp_dates_added(&mut scan.library, now()) {
        Ok(stamped) => {
            changed |= stamped.minted > 0;
            tracing::info!(
                known = stamped.known,
                minted = stamped.minted,
                unchanged = stamped.unchanged,
                "dates added"
            );
        }
        Err(error) => {
            changed = true;
            tracing::warn!(%error, "dates added not persisted");
        }
    }
    match store.save(content_dir, scan.reading(), cache, scope) {
        Ok(saved) => {
            changed |=
                saved.files > 0 || saved.covers > 0 || saved.playlists > 0 || saved.forgotten > 0;
            tracing::info!(
                files = saved.files,
                covers = saved.covers,
                playlists = saved.playlists,
                refused = saved.refused,
                forgotten = saved.forgotten,
                unchanged = saved.unchanged,
                "store written"
            );
        }
        Err(error) => {
            changed = true;
            stored = false;
            tracing::warn!(%error, "the store was not written: the next pass walks the whole tree");
        }
    }
    Persisted { changed, stored }
}

/// What writing a pass back to the store came to.
struct Persisted {
    changed: bool,
    stored: bool,
}

/// What one pass produced, and what is known about the pass itself.
pub struct Indexed {
    pub library: Library,
    pub complete: bool,
    /// Whether this pass found anything the store did not already hold.
    pub changed: bool,
    /// What the pass cost and what it refused, which outlives it whether or not it published.
    pub pass: PassReport,
}

/// What became of a pass, which is how the interface says whether the library is answered for.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "outcome", rename_all = "kebab-case")]
pub enum Outcome {
    /// It finished, and nothing has said yet what became of the index it built.
    Ran,
    /// The index it built is the one being served.
    Published,
    /// The tree agrees with the index already being served.
    Agreed,
    /// It came back with a hole in it, so the index being served was kept instead.
    Kept { why: String },
    /// It did not finish.
    Failed { why: String },
}

impl Outcome {
    /// Whether the library is answered for. A pass that was kept or that failed answers nothing.
    pub fn answered(&self) -> bool {
        matches!(self, Self::Ran | Self::Published | Self::Agreed)
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ran => "ran",
            Self::Published => "published",
            Self::Agreed => "agreed",
            Self::Kept { .. } => "kept",
            Self::Failed { .. } => "failed",
        }
    }

    /// Why the library is not answered for, where it is not.
    pub fn why(&self) -> Option<&str> {
        match self {
            Self::Ran | Self::Published | Self::Agreed => None,
            Self::Kept { why } | Self::Failed { why } => Some(why),
        }
    }
}

/// What one pass did, kept beyond it because a pass that changed nothing publishes no index.
#[derive(Clone, Debug)]
pub struct PassReport {
    /// When it ended, in seconds since the epoch, because an uptime is not a wall clock.
    pub ended: i64,
    pub seconds: f32,
    /// Whether it answered for the whole tree or only for the folders a change named.
    pub whole_tree: bool,
    /// The folders it walked, which is what marks a row of the tree as freshly read.
    pub covered: Scope,
    /// Files the walk found, and how many of those it had to open rather than take from the store.
    pub found: usize,
    pub opened: usize,
    pub reused: usize,
    pub complete: bool,
    pub outcome: Outcome,
    /// Whether the store kept what it learned.
    pub stored: bool,
    /// What it refused.
    pub refusals: Refusals,
}

impl PassReport {
    /// A pass that did not come back, which is the one kind that leaves nothing else to say.
    pub fn failed(why: String) -> Self {
        Self {
            ended: now(),
            seconds: 0.0,
            whole_tree: false,
            covered: Scope::default(),
            found: 0,
            opened: 0,
            reused: 0,
            complete: false,
            outcome: Outcome::Failed { why },
            stored: false,
            refusals: Refusals::default(),
        }
    }

    pub fn published(&self) -> bool {
        self.outcome == Outcome::Published
    }
}

impl PassReport {
    /// This pass's refusals about the parts of the tree a later pass over `scope` did not walk.
    pub fn carried_forward(&self, scope: &Scope) -> Refusals {
        Refusals::carried_forward(
            &self.refusals,
            |cause, subject| {
                let path = Path::new(subject);
                match cause {
                    Cause::UnreadableFolder => scope.covers(path),
                    _ => scope.covers_file(path),
                }
            },
            |folder| scope.covers(Path::new(folder)),
        )
    }
}

/// Watches the library, verifies the index against it if asked to, and rebuilds it on a change.
pub async fn keep_fresh(
    device: Arc<Device>,
    passes: Arc<Passes>,
    indexing: Arc<Live>,
    store: Option<Store>,
    verify: bool,
    reread: bool,
) {
    let mut settings = indexing.now();
    let mut bumps = device.bumps();
    let (mut watcher, mut ceiling) = watching(&settings).await;

    let mut store = store;
    if verify {
        let first = if reread { Pass::Reread } else { Pass::Whole };
        store = rebuild(&device, &passes, &settings, store, first, "index verified").await;
        ceiling_advice(&mut ceiling, &passes, &settings);
    }
    let mut asked = passes.requests_asked_for();
    let mut sweeps = sweeps_every(settings.options.sweep, store.is_some(), watcher.is_some());
    passes.note_sweep(sweeps.as_ref().map(tokio::time::Interval::period));
    loop {
        let pass = tokio::select! {
            change = next_change(&mut watcher) => match change {
                Some(change) => Some(Pass::for_change(&settings.roots, &change)),
                None => return,
            },
            asked = next_request(&mut asked) => match asked {
                Some(()) => Some(passes.taken()),
                None => return,
            },
            () = next_sweep(&mut sweeps) => {
                let (returned, found) = look(&settings, store).await;
                store = returned;
                match found {
                    Some(pass) => Some(pass),
                    None => continue,
                }
            }
            // A written setting, which the next pass has to run under rather than the old one.
            () = indexing.changed() => None,
            // Whatever moved it, a rescan here or a setting written from the page.
            Ok(()) = bumps.changed() => {
                remembering(&mut store, *bumps.borrow_and_update());
                continue;
            }
        };
        let fresh = indexing.now();
        let was_watched = watcher.is_some();
        if fresh.roots != settings.roots {
            tracing::info!(folder = %fresh.roots.describe(), "the music folders changed");
            (watcher, ceiling) = watching(&fresh).await;
            store = repointed(store, fresh.roots.clone()).await;
        }
        if fresh.options.sweep != settings.options.sweep || watcher.is_some() != was_watched {
            sweeps = sweeps_every(fresh.options.sweep, store.is_some(), watcher.is_some());
            passes.note_sweep(sweeps.as_ref().map(tokio::time::Interval::period));
        }
        settings = fresh;
        let Some(pass) = pass else { continue };
        store = rebuild(&device, &passes, &settings, store, pass, "index replaced").await;
        ceiling_advice(&mut ceiling, &passes, &settings);
    }
}

/// Once a whole pass has counted the folders, the line that lifts the kernel's ceiling above
/// them, said once.
fn ceiling_advice(ceiling: &mut Option<Option<u64>>, passes: &Passes, settings: &Indexing) {
    let Some(refused) = *ceiling else { return };
    if !passes.last().is_some_and(|last| last.whole_tree) {
        return;
    }
    let folders = settings.underway.progress.reached().folders;
    if folders > 0 {
        tracing::warn!("{}", watch_advice(refused, folders));
        *ceiling = None;
    }
}

/// The watch on the folders in force, or nothing where none could be established. The second
/// is the kernel's ceiling on folder watches when that is what refused it, read where the kernel
/// publishes one; the exact line to raise it waits for a pass to count the folders.
async fn watching(indexing: &Indexing) -> (Option<watch::Watcher>, Option<Option<u64>>) {
    if indexing.roots.is_empty() {
        return (None, None);
    }
    let folder = indexing.roots.clone();
    let exclude = indexing.options.exclude.clone();
    match tokio::task::spawn_blocking(move || watch::Watcher::start(folder, watch::QUIET, exclude))
        .await
    {
        Ok(Ok(watcher)) => (Some(watcher), None),
        Ok(Err(error)) => {
            let at_ceiling = matches!(
                error.kind,
                notify_debouncer_full::notify::ErrorKind::MaxFilesWatch
            );
            let ceiling = at_ceiling.then(watch_ceiling);
            match ceiling.flatten() {
                Some(ceiling) => tracing::warn!(
                    ceiling,
                    "not watching for changes: the kernel allows fewer folder watches than the \
                     library has folders, so a new album appears at the next timed check instead"
                ),
                None => tracing::warn!(
                    %error,
                    "not watching for changes: a new album appears at the next timed check instead"
                ),
            }
            (None, ceiling)
        }
        Err(error) => {
            tracing::error!(%error, "the watch was not established");
            (None, None)
        }
    }
}

/// The kernel's ceiling on inotify watches, one per folder. Only Linux publishes one.
fn watch_ceiling() -> Option<u64> {
    std::fs::read_to_string("/proc/sys/fs/inotify/max_user_watches")
        .ok()?
        .trim()
        .parse()
        .ok()
}

/// The line that lifts the ceiling above the library, with room for the albums to come.
fn watch_advice(ceiling: Option<u64>, folders: usize) -> String {
    let needed = (folders + folders / 4).div_ceil(1024) * 1024;
    let allows = match ceiling {
        Some(ceiling) => format!("the kernel allows {ceiling} folder watches"),
        None => "the kernel allows fewer folder watches".to_owned(),
    };
    format!(
        "{allows} and the library has {folders} folders: \
         `sysctl fs.inotify.max_user_watches={needed}` lets it be watched"
    )
}

/// Tells the store which library it serves, after the folders changed under it.
async fn repointed(store: Option<Store>, roots: Roots) -> Option<Store> {
    let moved = tokio::task::spawn_blocking(move || {
        let mut store = store;
        if let Some(held) = store.as_mut()
            && let Err(error) = held.serving(&roots)
        {
            tracing::warn!(%error, "the store was not told which library it is serving");
        }
        store
    })
    .await;
    match moved {
        Ok(store) => store,
        Err(error) => {
            tracing::error!(%error, "the store was not handed back: the library is not remembered");
            None
        }
    }
}

/// How many times longer a watched library waits between looks: the watch reports what changed,
/// and a look there only catches what the watch missed.
const WATCHED_STRETCH: u32 = 24;

/// The timer a sweep runs on, or nothing where none is wanted or nothing remembers the tree.
fn sweeps_every(
    every: Option<Duration>,
    remembered: bool,
    watched: bool,
) -> Option<tokio::time::Interval> {
    let every = sweep_period(every?, watched);
    if !remembered {
        tracing::warn!("no store: the library is not looked at for changes on a timer");
        return None;
    }
    let mut sweeps = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
    sweeps.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tracing::info!(
        minutes = every.as_secs() / 60,
        watched,
        "looking at the library for changes on a timer"
    );
    Some(sweeps)
}

/// The interval the owner set, stretched where a watch already reports changes.
fn sweep_period(every: Duration, watched: bool) -> Duration {
    if watched {
        every * WATCHED_STRETCH
    } else {
        every
    }
}

/// The next time to look, or never where no timer runs.
async fn next_sweep(sweeps: &mut Option<tokio::time::Interval>) {
    match sweeps {
        Some(sweeps) => {
            sweeps.tick().await;
        }
        None => std::future::pending().await,
    }
}

/// One look at the tree on a blocking thread, with the store lent to it and handed back.
async fn look(indexing: &Indexing, store: Option<Store>) -> (Option<Store>, Option<Pass>) {
    let indexing = indexing.clone();
    let looked = tokio::task::spawn_blocking(move || {
        let found = match store.as_ref() {
            Some(held) => guarded(|| sweep(&indexing, held)),
            None => Ok(None),
        };
        (store, found)
    })
    .await;
    match looked {
        Ok((store, Ok(found))) => (store, found),
        Ok((store, Err(error))) => {
            tracing::warn!(%error, "the library could not be looked at for changes");
            (store, None)
        }
        Err(error) => {
            tracing::error!(%error, "the look at the library did not finish");
            (None, None)
        }
    }
}

/// What a look at the tree found changed, as the pass to run, or nothing where nothing did.
pub fn sweep(indexing: &Indexing, store: &Store) -> Result<Option<Pass>> {
    let root = indexing.root();
    let started = std::time::Instant::now();
    let walked = scan::walk_within(
        root,
        &Scope::whole_tree(),
        &indexing.underway,
        &indexing.options.exclude,
    )?;
    if walked.stopped {
        return Ok(None);
    }
    let changed = sweep::differences(root, &walked, &store.fingerprints()?);
    if changed.is_empty() {
        tracing::debug!(
            files = walked.files.len(),
            seconds = started.elapsed().as_secs_f32(),
            "looked at the library: nothing changed"
        );
        return Ok(None);
    }
    let change = watch::Change {
        paths: changed,
        whole_tree: false,
    };
    let pass = match Pass::for_change(root, &change) {
        Pass::Within(scope) if scope.regions().len() > MANY_FOLDERS => Pass::Whole,
        pass => pass,
    };
    tracing::info!(
        paths = change.paths.len(),
        whole_tree = pass == Pass::Whole,
        seconds = started.elapsed().as_secs_f32(),
        "looked at the library: it changed"
    );
    Ok(Some(pass))
}

/// The next change, or nothing for ever where no watch could be established.
async fn next_change(watcher: &mut Option<watch::Watcher>) -> Option<watch::Change> {
    match watcher {
        Some(watcher) => watcher.changed().await,
        None => std::future::pending().await,
    }
}

/// The next pass somebody asked for over the interface, or nothing for ever where nobody can.
async fn next_request(asked: &mut Option<tokio::sync::mpsc::Receiver<()>>) -> Option<()> {
    match asked {
        Some(asked) => asked.recv().await,
        None => std::future::pending().await,
    }
}

/// One rescan, on a blocking thread, published if it is worth publishing.
async fn rebuild(
    device: &Device,
    passes: &Passes,
    indexing: &Indexing,
    store: Option<Store>,
    pass: Pass,
    done: &str,
) -> Option<Store> {
    if indexing.roots.is_empty() {
        return store;
    }
    let indexing = indexing.clone();
    let pass = trusting_the_rows(passes, pass);
    let scope = pass.scope();
    let mut moved = store;
    let began = passes.begin();
    let serving_nothing = device.served().library.is_empty();
    let scanned = tokio::task::spawn_blocking(move || {
        // The store stays outside the guard.
        let derived = guarded(|| {
            let indexed = index(&indexing, &mut moved, pass)?;
            // The browse view is derived here rather than inside `publish`. It is hundreds.
            // Only what gets published is worth the menus and the counts over every track.
            let worth_it = indexed.changed || serving_nothing;
            Ok(Derived {
                served: worth_it
                    .then(|| browse::Served::new(indexed.library, indexing.menus.clone())),
                complete: indexed.complete,
                pass: indexed.pass,
            })
        });
        (moved, derived)
    })
    .await;
    if began {
        passes.end();
    }
    let (store, indexed) = match scanned {
        Ok((returned, indexed)) => (returned, indexed),
        Err(error) => {
            tracing::error!(%error, "the rescan did not finish");
            passes.record(PassReport::failed(format!(
                "the pass did not finish: {error}"
            )));
            return None;
        }
    };
    let mut fresh = match indexed {
        Ok(fresh) => fresh,
        Err(error) => {
            tracing::error!(%error, "the library was not rescanned");
            passes.record(PassReport::failed(format!("{error:#}")));
            return store;
        }
    };
    carry_forward(passes, &scope, &mut fresh.pass);
    match fresh.served.take() {
        None => {
            fresh.pass.outcome = Outcome::Agreed;
            tracing::info!("{done}: the tree agrees with the index being served");
        }
        Some(served) => {
            if let Some(why) = keeping(&served.library, fresh.complete, device) {
                fresh.pass.outcome = Outcome::Kept { why };
            } else {
                // The loop persists the number: this task is the only one holding the store,
                // and the settings page moves it too.
                let id = device.publish(served).await;
                fresh.pass.outcome = Outcome::Published;
                tracing::info!(system_update_id = id, "{done}");
            }
        }
    }
    passes.record(fresh.pass);
    store
}

/// Keeps the update id, or a restart resumes below one a client holds and hands it a stale cache.
fn remembering(store: &mut Option<Store>, id: u32) {
    if let Some(store) = store.as_mut()
        && let Err(error) = store.remember_update_id(id)
    {
        tracing::warn!(%error, "the update id was not persisted: a restart may repeat it");
    }
}

/// The pass to run, widened to the whole tree where the last pass left the store behind it.
fn trusting_the_rows(passes: &Passes, pass: Pass) -> Pass {
    match pass {
        Pass::Within(_) if passes.last().is_some_and(|last| !last.stored) => {
            tracing::warn!(
                "the store fell behind the tree on the last pass, so the folders that changed \
                 cannot be joined to it: walking the whole tree"
            );
            Pass::Whole
        }
        pass => pass,
    }
}

fn guarded<T>(work: impl FnOnce() -> Result<T>) -> Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(work)).unwrap_or_else(|payload| {
        let said = payload
            .downcast_ref::<&str>()
            .map(|message| (*message).to_owned())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "no message".to_owned());
        Err(anyhow::anyhow!("the pass panicked: {said}"))
    })
}

struct Derived {
    /// Nothing where the pass agreed with what is already served, since none of it is published.
    served: Option<browse::Served>,
    complete: bool,
    pass: PassReport,
}

/// Gives a pass over part of the tree what the last one said about the rest of it.
fn carry_forward(passes: &Passes, scope: &Scope, pass: &mut PassReport) {
    if scope.is_whole_tree() || pass.whole_tree {
        return;
    }
    let Some(previous) = passes.last() else {
        return;
    };
    let mut carried = previous.carried_forward(scope);
    carried.absorb(std::mem::take(&mut pass.refusals));
    pass.refusals = carried;
}

/// Why the index being served is kept rather than replaced, or nothing where it is replaced.
fn keeping(library: &Library, complete: bool, device: &Device) -> Option<String> {
    let serving = device.served().library.len();
    if library.is_empty() && serving > 0 {
        tracing::error!(
            serving,
            "the library now reads as empty: keeping the index that is being served rather than \
             publishing this one. Check that the share is still mounted, then restart to apply it"
        );
        return Some(format!(
            "the music folder read as empty, so the {serving} tracks last indexed are still being \
             served. Check that the share is still mounted"
        ));
    }
    if !complete && library.len() < serving {
        tracing::error!(
            serving,
            found = library.len(),
            "the walk could not read every folder and came back smaller: keeping the index that \
             is being served rather than publishing one with a hole in it"
        );
        return Some(format!(
            "the last check could not read every folder and found {} tracks where {serving} are \
             being served, so the index was kept",
            library.len()
        ));
    }
    None
}

/// Where `SystemUpdateID` carries on from, which is never a value a client may already hold.
/// The boot id this start announces, always above the last one's: a control point reads a boot id
/// that did not rise as the same boot, and two starts inside one second share a clock reading.
pub fn next_boot_id(store: &mut Option<Store>) -> u32 {
    let last = match store.as_ref().map(Store::resumed_boot_id) {
        Some(Ok(held)) => held,
        Some(Err(error)) => {
            tracing::warn!(%error, "the last boot id could not be read: a client may ignore this start");
            None
        }
        None => None,
    };
    let minted = seconds_since_epoch().max(last.map_or(0, |held| held.saturating_add(1)));
    if let Some(store) = store.as_mut()
        && let Err(error) = store.remember_boot_id(minted)
    {
        tracing::warn!(%error, "the boot id was not persisted: the next start may repeat it");
    }
    minted
}

pub fn resumed_update_id(store: &mut Option<Store>) -> u32 {
    match store.as_ref().map(Store::resumed_update_id) {
        Some(Ok(Some(held))) => return held,
        Some(Err(error)) => {
            tracing::warn!(%error, "the last update id could not be read: clients may hold a stale cache");
        }
        _ => {}
    }
    let seed = seconds_since_epoch();
    if let Some(store) = store.as_mut()
        && let Err(error) = store.remember_update_id(seed)
    {
        tracing::warn!(%error, "the update id was not persisted: a restart will move it");
    }
    seed
}

fn seconds_since_epoch() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs() as u32)
        .unwrap_or(1)
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_watch_advice_names_the_ceiling_the_folders_and_a_line_with_room_above_them() {
        let advice = super::watch_advice(Some(8192), 12_034);
        assert!(advice.contains("allows 8192 folder watches"), "{advice}");
        assert!(advice.contains("has 12034 folders"), "{advice}");
        assert!(
            advice.contains("sysctl fs.inotify.max_user_watches=15360"),
            "a quarter above the library, rounded up to the thousand: {advice}"
        );
        let unread = super::watch_advice(None, 12_034);
        assert!(unread.contains("max_user_watches=15360"), "{unread}");
    }

    use super::*;
    use crate::index::Scanned;
    use crate::upnp::client::Profiles;
    use crate::upnp::description::DeviceIdentity;
    use std::path::PathBuf;

    fn device(library: Library) -> Device {
        Device::new(
            library,
            crate::browse::Settings::default(),
            DeviceIdentity {
                friendly_name: "Kantele".to_owned(),
                udn: "uuid:test".to_owned(),
                icon: Default::default(),
            },
            Profiles::default(),
        )
    }

    fn track(name: &str) -> Scanned {
        Scanned {
            path: PathBuf::from(format!("/music/a/{name}.flac")),
            relative: PathBuf::from(format!("a/{name}.flac")),
            tags: crate::tags::FileTags {
                title: Some(name.to_owned()),
                ..Default::default()
            },
            properties: crate::tags::AudioProperties::default(),
            size: 27,
            artwork: None,
        }
    }

    /// Libraries built through the real rules, so the identifiers are the ones the server mints.
    fn one_track() -> Library {
        Library::build("Music".to_owned(), &[track("Juana Peña")])
    }

    fn two_tracks() -> Library {
        Library::build(
            "Music".to_owned(),
            &[track("Juana Peña"), track("Dundunbanza")],
        )
    }

    #[test]
    fn a_boot_id_rises_even_where_two_starts_share_a_second() {
        let mut store = Some(Store::in_memory().expect("a store"));
        let first = next_boot_id(&mut store);
        let second = next_boot_id(&mut store);
        assert!(
            second > first,
            "two starts inside one second read the same clock: {first} then {second}"
        );

        // A clock that went backwards is not a new boot id either.
        let ahead = second.saturating_add(10_000);
        store
            .as_mut()
            .expect("a store")
            .remember_boot_id(ahead)
            .expect("remembering one from the future");
        assert_eq!(next_boot_id(&mut store), ahead + 1);

        assert!(
            next_boot_id(&mut None) > 0,
            "a server with no store still announces one"
        );
    }

    #[tokio::test]
    async fn a_setting_written_from_the_page_leaves_the_update_id_where_a_restart_finds_it() {
        let dir = std::env::temp_dir().join(format!("kantele-update-id-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("index.sqlite");
        let device = Arc::new(device(one_track()));
        tokio::spawn(keep_fresh(
            device.clone(),
            Arc::new(Passes::default()),
            Arc::new(Live::new(Indexing::default())),
            Some(Store::open(&path).expect("a store")),
            false,
            false,
        ));

        // Nothing rescanned: the page alone moved it, and the page has no store to write to.
        let applied = device
            .apply_settings(crate::browse::Settings::default())
            .await;
        let mut held = None;
        for _ in 0..200 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            held = Store::open(&path)
                .expect("a second look at the same file")
                .resumed_update_id()
                .expect("the meta table");
            if held == Some(applied) {
                break;
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(
            held,
            Some(applied),
            "a client was told {applied} and a restart would resume below it"
        );
    }

    #[test]
    fn a_watched_library_is_looked_at_far_less_often_than_an_unwatched_one() {
        let set = Duration::from_secs(15 * 60);
        assert_eq!(sweep_period(set, false), set);
        assert_eq!(sweep_period(set, true), Duration::from_secs(6 * 60 * 60));
    }

    #[test]
    fn a_library_that_reads_as_empty_does_not_replace_one_that_does_not() {
        let serving = device(one_track());
        assert!(keeping(&Library::default(), true, &serving).is_some());
        assert_eq!(serving.system_update_id(), 1, "nothing was published");
    }

    #[test]
    fn a_walk_that_missed_folders_does_not_publish_a_smaller_library() {
        let serving = device(two_tracks());
        assert!(keeping(&one_track(), false, &serving).is_some());
        assert_eq!(serving.system_update_id(), 1, "nothing was published");

        assert!(keeping(&two_tracks(), false, &serving).is_none());
    }

    fn refused(cause: crate::index::refusals::Cause, subject: &str) -> PassReport {
        let mut refusals = Refusals::default();
        refusals.refuse(cause, subject, None);
        PassReport {
            ended: 0,
            seconds: 0.0,
            whole_tree: true,
            found: 0,
            opened: 0,
            reused: 0,
            complete: true,
            covered: Scope::whole_tree(),
            outcome: Outcome::Published,
            stored: true,
            refusals,
        }
    }

    #[test]
    fn a_pass_over_one_folder_walks_the_whole_tree_where_the_store_fell_behind() {
        let passes = Passes::default();
        let within = Pass::Within(Scope::of([PathBuf::from("a")]));
        assert_eq!(
            trusting_the_rows(&passes, within.clone()),
            within,
            "nothing has been said about the store yet"
        );

        passes.record(PassReport {
            stored: false,
            ..refused(Cause::UnreadableFile, "b/1.mp3")
        });
        assert_eq!(trusting_the_rows(&passes, within.clone()), Pass::Whole);
        assert_eq!(
            trusting_the_rows(&passes, Pass::Reread),
            Pass::Reread,
            "a pass over everything is left alone"
        );

        passes.record(refused(Cause::UnreadableFile, "b/1.mp3"));
        assert_eq!(
            trusting_the_rows(&passes, within.clone()),
            within,
            "a pass the store kept restores the trust"
        );
    }

    #[test]
    fn a_partial_pass_is_given_what_the_last_one_said_about_the_rest_of_the_tree() {
        let passes = Passes::default();
        passes.record(refused(Cause::UnreadableFile, "b/1.mp3"));

        let scope = Scope::of([PathBuf::from("a")]);
        let mut fresh = refused(Cause::UnreadableFile, "a/2.mp3");
        fresh.whole_tree = false;
        carry_forward(&passes, &scope, &mut fresh);
        assert_eq!(
            fresh.refusals.total_of(Cause::UnreadableFile),
            2,
            "the folder this pass did not walk is still refusing what it was"
        );
    }

    #[test]
    fn a_whole_tree_pass_replaces_the_record_rather_than_adding_to_it() {
        let passes = Passes::default();
        passes.record(refused(Cause::UnreadableFile, "b/1.mp3"));

        let mut fresh = refused(Cause::UnreadableFile, "a/2.mp3");
        carry_forward(&passes, &Scope::whole_tree(), &mut fresh);
        assert_eq!(
            fresh.refusals.total_of(Cause::UnreadableFile),
            1,
            "a walk of everything is an answer about everything, and the old record is spent"
        );
    }

    #[test]
    fn a_partial_pass_over_a_folder_supersedes_what_was_said_about_it() {
        let passes = Passes::default();
        passes.record(refused(Cause::UnreadableFile, "a/1.mp3"));

        let scope = Scope::of([PathBuf::from("a")]);
        let mut fresh = PassReport {
            whole_tree: false,
            ..refused(Cause::UnreadableFile, "a/1.mp3")
        };
        fresh.refusals = Refusals::default();
        carry_forward(&passes, &scope, &mut fresh);
        assert_eq!(
            fresh.refusals.total_of(Cause::UnreadableFile),
            0,
            "the file reads now, and the record must not keep saying it does not"
        );
    }

    #[test]
    fn a_library_nobody_changes_hands_a_client_the_same_update_id_after_a_restart() {
        let mut store = Some(Store::in_memory().expect("a store"));
        let first = resumed_update_id(&mut store);
        assert!(first > 0);
        assert_eq!(
            store
                .as_ref()
                .expect("a store")
                .resumed_update_id()
                .expect("readable"),
            Some(first),
            "a seed nothing writes down is a seed the next start cannot resume"
        );
        assert_eq!(
            resumed_update_id(&mut store),
            first,
            "a restart of a server whose library nobody touched must not move the identifier"
        );
    }

    #[test]
    fn a_stored_identifier_is_resumed_rather_than_replaced() {
        let mut store = Some(Store::in_memory().expect("a store"));
        store
            .as_mut()
            .expect("a store")
            .remember_update_id(41)
            .expect("remembering");
        assert_eq!(resumed_update_id(&mut store), 41);
    }

    #[test]
    fn a_pass_that_panics_is_an_error_and_the_store_it_was_handed_survives() {
        let mut store = Some(Store::in_memory().expect("a store"));
        let failed = guarded(|| -> Result<()> {
            let _held = store.as_mut().expect("the store is in hand");
            panic!("a malformed header took the reader down")
        });
        assert!(
            failed
                .expect_err("a panic is an error")
                .to_string()
                .contains("malformed header")
        );
        assert!(
            store
                .as_ref()
                .expect("the store is still held")
                .resumed_update_id()
                .is_ok(),
            "and it still answers"
        );
    }

    #[test]
    fn a_configuration_naming_no_folder_is_a_server_waiting_for_one() {
        let roots = roots_of(&crate::config::Config::default()).expect("it is not a refusal");
        assert!(
            roots.is_empty(),
            "and it serves nothing until one is chosen"
        );
    }

    /// The package copies this file on its first start, so a folder named here is one nobody chose.
    #[test]
    fn the_example_the_package_ships_names_no_folder() {
        let text = include_str!("../kantele.example.toml");
        let example = crate::config::Config::parse(text).expect("the example parses");
        assert!(
            roots_of(&example).expect("it is not a refusal").is_empty(),
            "or a first start dies on a folder nobody chose"
        );
    }

    #[test]
    fn every_other_rescan_is_published() {
        let serving = device(one_track());
        assert!(keeping(&one_track(), true, &serving).is_none());

        let empty = device(Library::default());
        assert!(keeping(&Library::default(), true, &empty).is_none());
        assert!(keeping(&one_track(), true, &empty).is_none());
    }
}
