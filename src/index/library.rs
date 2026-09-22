//! The in-memory library: tracks, albums and artists.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::index::artwork::Artwork;
use crate::index::credits::{self, Credit};
use crate::index::derive::{albums_from, artists_from};
use crate::index::fold;
use crate::index::identity::{self, FileFacts, Minter, Placement, Release, Rule};
use crate::index::playlist::{self, Playlist};
use crate::index::refusals::{Cause, Refusals};
use crate::index::roots::Roots;
use crate::index::scan::{self, ScanOptions, Scanned, Walked};
use crate::index::searchable::Searchables;
use crate::index::store::Cache;
use crate::object::ObjectId;
use crate::tags::FileTags;

#[derive(Debug)]
pub struct Scan {
    pub library: Library,
    pub walked: Walked,
    pub files: Vec<Scanned>,
    pub playlists: Vec<playlist::Scanned>,
    pub refused: Vec<scan::RefusedFile>,
    pub refusals: Refusals,
}

impl Scan {
    pub fn reading(&self) -> crate::index::store::Reading<'_> {
        crate::index::store::Reading {
            walked: &self.walked,
            files: &self.files,
            playlists: &self.playlists,
            refused: &self.refused,
        }
    }
}

fn group(files: &[Scanned], held: &identity::Claims) -> identity::Grouping {
    let facts: Vec<FileFacts> = files
        .iter()
        .map(|file| FileFacts {
            relative_path: &file.relative,
            tags: &file.tags,
        })
        .collect();
    identity::group_releases_holding(&facts, held)
}

fn mint_tracks(files: &[Scanned], grouping: &identity::Grouping) -> Vec<Track> {
    let mut minter = Minter::default();
    files
        .iter()
        .enumerate()
        .map(|(index, file)| {
            let placement = grouping.placement[index];
            let release = placement.release.map(|at| &grouping.releases[at]);
            track_from(file, &mut minter, release, placement)
        })
        .collect()
}

/// The folder a relative path sits in, empty at the content root.
fn holding(relative: &str) -> &str {
    relative.rfind('/').map_or("", |cut| &relative[..cut])
}

fn refused_by_derivation(files: &[Scanned], tracks: &[Track], albums: &[Album]) -> Refusals {
    let mut refusals = Refusals::default();
    for album in albums.iter().filter(|album| album.rule == Rule::Path) {
        // The subject is the title, because that is what a reader recognises; the folder its
        // tracks sit in is what puts the refusal on a row of the tree.
        match album.tracks.first().and_then(|at| tracks.get(*at)) {
            Some(track) => refusals.refuse_in(
                Cause::AlbumKeyedOnPath,
                album.title.clone(),
                None,
                holding(&track.relative),
            ),
            None => refusals.refuse(Cause::AlbumKeyedOnPath, album.title.clone(), None),
        }
    }
    for (track, file) in tracks.iter().zip(files) {
        if track.rule == Rule::Path && identity::identifies_itself(&file.tags) {
            refusals.refuse(Cause::TrackKeyedOnPath, track.relative.clone(), None);
        }
    }
    refusals
}

#[derive(Clone, Debug)]
pub struct Track {
    pub id: ObjectId,
    pub rule: Rule,
    pub path: PathBuf,
    pub relative: String,
    pub album_id: Option<ObjectId>,
    pub album_inherited: bool,
    /// Set by the flag or by a placeholder credit, never by a missing one.
    pub compilation: bool,
    pub title: String,
    /// Whether the title is the file's own tag. A file with none is titled by its name, here and
    /// on the amplifier.
    pub title_tagged: bool,
    pub artists: Vec<Credit>,
    pub album_artists: Vec<Credit>,
    pub composers: Vec<Credit>,
    pub conductors: Vec<Credit>,
    pub album: Option<String>,
    pub genres: Vec<String>,
    pub date: Option<String>,
    pub track_number: Option<u32>,
    pub disc_number: Option<u32>,
    /// Set where the disc came from a marker in the album title rather than a disc-number tag.
    pub disc_from_title: bool,
    pub disc_subtitle: Option<String>,
    /// The work this track is a part of. An index value, never containment.
    pub work: Option<String>,
    /// The run of tracks this one belongs to. Containment inside the album, never an index value.
    pub grouping: Option<String>,
    pub duration: Duration,
    pub size: u64,
    pub mime: &'static str,
    pub sample_rate: Option<u32>,
    pub bit_depth: Option<u8>,
    pub channels: Option<u8>,
    pub bitrate_bps: Option<u32>,
    pub artwork: Option<Artwork>,
    pub recording_mbid: Option<String>,
    /// Filled in from the store.
    pub date_added: Option<i64>,
}

#[derive(Clone, Debug)]
pub struct Album {
    pub id: ObjectId,
    pub rule: Rule,
    /// The album tag, disc marker stripped, in its most common spelling.
    pub title: String,
    pub artists: Vec<Credit>,
    /// Set when an album-artist tag supplied `artists`.
    pub credited: bool,
    pub date: Option<String>,
    pub genres: Vec<String>,
    pub disc_count: u32,
    pub musicbrainz_id: Option<String>,
    pub artwork: Option<Artwork>,
    /// Indices into [`Library::tracks`], in play order.
    pub tracks: Vec<usize>,
    /// The discs in play order, one where nothing numbers them.
    pub discs: Vec<Disc>,
    /// Whether the discs are shown apart, which a subtitle or a marker in the title asks for.
    pub separate_discs: bool,
    /// Runs of consecutive tracks a tagger grouped, in play order.
    pub runs: Vec<Run>,
}

/// One disc of an album.
#[derive(Clone, Debug)]
pub struct Disc {
    pub id: ObjectId,
    pub number: Option<u32>,
    pub subtitle: Option<String>,
    /// Indices into [`Library::tracks`], in play order.
    pub tracks: Vec<usize>,
}

/// Consecutive tracks of one album sharing a grouping tag, opened as one item.
#[derive(Clone, Debug)]
pub struct Run {
    pub id: ObjectId,
    pub title: String,
    /// Indices into [`Library::tracks`], in play order.
    pub tracks: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct Artist {
    pub id: ObjectId,
    pub rule: Rule,
    pub name: String,
    pub musicbrainz_id: Option<String>,
    /// Indices into [`Library::albums`].
    pub albums: Vec<usize>,
    /// Indices into [`Library::tracks`] for tracks reached through no album.
    pub tracks: Vec<usize>,
}

#[derive(Clone, Debug, Default)]
pub struct Library {
    searchable: Searchables,
    refusals: Refusals,
    name: String,
    tracks: Vec<Track>,
    albums: Vec<Album>,
    artists: Vec<Artist>,
    playlists: Vec<Playlist>,
    tracks_by_id: HashMap<ObjectId, usize>,
    albums_by_id: HashMap<ObjectId, usize>,
    artists_by_id: HashMap<ObjectId, usize>,
    playlists_by_id: HashMap<ObjectId, usize>,
    claims: identity::Claims,
}

impl Library {
    pub fn scan(root: &Path) -> std::io::Result<Self> {
        Self::scan_with(root, &ScanOptions::default())
    }

    pub fn scan_with(root: &Path, options: &ScanOptions) -> std::io::Result<Self> {
        Ok(Self::scan_using(root, options, &Cache::default())?.library)
    }

    pub fn scan_using(
        roots: impl Into<Roots>,
        options: &ScanOptions,
        cache: &Cache,
    ) -> std::io::Result<Scan> {
        Self::scan_scoped(
            roots,
            options,
            cache,
            &scan::Scope::whole_tree(),
            &scan::Underway::default(),
            &fold::Ignored::default(),
        )
    }

    pub fn scan_scoped(
        roots: impl Into<Roots>,
        options: &ScanOptions,
        cache: &Cache,
        scope: &scan::Scope,
        underway: &scan::Underway,
        ignored: &fold::Ignored,
    ) -> std::io::Result<Scan> {
        let roots = &roots.into();
        underway.progress.begin();
        let mut walked = scan::walk_within(roots, scope, underway, &options.exclude)?;
        underway.progress.reading(walked.files.len());
        let (files, refused, refused_files) = scan::read(roots, &walked, options, cache, underway);
        let (playlists, refused_playlists) = playlist::read_all(roots, &walked, cache, underway);
        underway.progress.building();
        // Checked last, so a stop during the reads also marks the pass unfinished.
        walked.stopped |= underway.stopping.asked();
        let mut refusals = walked.refusals.clone();
        refusals.absorb(refused_files);
        refusals.absorb(refused_playlists);
        refusals.refuse_counted(Cause::UnreadableRow, cache.unreadable());
        debug_assert!(
            refusals.all_from(crate::index::refusals::Origin::Pass),
            "a walk recorded a refusal the index build derives for itself"
        );

        // A first read has no stored awards, and what it published as it went is what the index
        // it ends with has to agree with: an album that changed identifier between the two is one
        // a control point holds a dead identifier for.
        let awarded = cache
            .claims()
            .is_empty()
            .then(|| underway.partial.awarded());
        let held = awarded.as_ref().unwrap_or(cache.claims());
        let library = if scope.is_whole_tree() {
            Self::build_holding(roots.name(), &files, &playlists, ignored, held)
        } else {
            let mut all = cache.remembered_outside(scope);
            all.extend(files.iter().cloned());
            all.sort_by(|left, right| left.path.cmp(&right.path));
            let mut every = cache.remembered_playlists_outside(scope);
            every.extend(playlists.iter().cloned());
            every.sort_by(|left, right| left.relative.cmp(&right.relative));
            Self::build_holding(roots.name(), &all, &every, ignored, held)
        };
        Ok(Scan {
            library,
            walked,
            files,
            playlists,
            refused,
            refusals,
        })
    }

    pub fn build(name: String, files: &[Scanned]) -> Self {
        Self::build_with(name, files, &[])
    }

    pub fn build_with(name: String, files: &[Scanned], playlists: &[playlist::Scanned]) -> Self {
        Self::build_ordered(name, files, playlists, &fold::Ignored::default())
    }

    /// `ignored` decides where a title or a name with no sort tag falls in the lists.
    pub fn build_ordered(
        name: String,
        files: &[Scanned],
        playlists: &[playlist::Scanned],
        ignored: &fold::Ignored,
    ) -> Self {
        Self::build_holding(name, files, playlists, ignored, &identity::Claims::new())
    }

    /// `held` is which folder was awarded each album key last time.
    pub fn build_holding(
        name: String,
        files: &[Scanned],
        playlists: &[playlist::Scanned],
        ignored: &fold::Ignored,
        held: &identity::Claims,
    ) -> Self {
        let grouping = group(files, held);
        let tracks = mint_tracks(files, &grouping);
        let albums = albums_from(&grouping.releases, &tracks, ignored);
        let artists = artists_from(&albums, &tracks, files, ignored);
        let (playlists, aliases, mut refusals) = playlist::resolve_all(playlists, &tracks);
        refusals.absorb(refused_by_derivation(files, &tracks, &albums));
        debug_assert!(
            refusals.all_from(crate::index::refusals::Origin::Index),
            "an index build recorded a refusal only a walk can see"
        );
        let searchable = Searchables::build(&tracks, &albums, &artists, &playlists, &aliases);

        let mut library = Self {
            name,
            searchable,
            refusals,
            tracks,
            albums,
            artists,
            playlists,
            claims: grouping.claims,
            ..Self::default()
        };
        library.index_identifiers();
        library
    }

    /// Which folder holds each album key, for the store to hand back to the next pass.
    pub fn claims(&self) -> &identity::Claims {
        &self.claims
    }

    pub fn from_tracks(tracks: impl IntoIterator<Item = Track>) -> Self {
        let mut library = Self {
            tracks: tracks.into_iter().collect(),
            ..Self::default()
        };
        library.index_identifiers();
        library.searchable = Searchables::build(
            &library.tracks,
            &library.albums,
            &library.artists,
            &library.playlists,
            &[],
        );
        library
    }

    pub fn searchable(&self) -> &Searchables {
        &self.searchable
    }

    pub fn refusals(&self) -> &Refusals {
        &self.refusals
    }

    fn index_identifiers(&mut self) {
        self.tracks_by_id = self
            .tracks
            .iter()
            .enumerate()
            .map(|(at, track)| (track.id.clone(), at))
            .collect();
        self.albums_by_id = self
            .albums
            .iter()
            .enumerate()
            .map(|(at, album)| (album.id.clone(), at))
            .collect();
        self.artists_by_id = self
            .artists
            .iter()
            .enumerate()
            .map(|(at, artist)| (artist.id.clone(), at))
            .collect();
        self.playlists_by_id = self
            .playlists
            .iter()
            .enumerate()
            .map(|(at, playlist)| (playlist.id.clone(), at))
            .collect();
    }

    pub fn tracks_mut(&mut self) -> &mut [Track] {
        &mut self.tracks
    }

    pub fn name(&self) -> &str {
        if self.name.is_empty() {
            "Music"
        } else {
            &self.name
        }
    }

    pub fn tracks(&self) -> &[Track] {
        &self.tracks
    }

    pub fn albums(&self) -> &[Album] {
        &self.albums
    }

    pub fn artists(&self) -> &[Artist] {
        &self.artists
    }

    pub fn playlists(&self) -> &[Playlist] {
        &self.playlists
    }

    pub fn len(&self) -> usize {
        self.tracks.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tracks.is_empty()
    }

    pub fn get(&self, id: &ObjectId) -> Option<&Track> {
        self.tracks_by_id.get(id).map(|index| &self.tracks[*index])
    }

    pub fn album(&self, id: &ObjectId) -> Option<&Album> {
        self.albums_by_id.get(id).map(|index| &self.albums[*index])
    }

    pub fn artist(&self, id: &ObjectId) -> Option<&Artist> {
        self.artists_by_id
            .get(id)
            .map(|index| &self.artists[*index])
    }

    pub fn artist_named(&self, name: &str) -> Option<&Artist> {
        let wanted = fold(name);
        self.artists
            .iter()
            .find(|artist| fold(&artist.name) == wanted)
    }

    pub fn playlist(&self, id: &ObjectId) -> Option<&Playlist> {
        self.playlists_by_id
            .get(id)
            .map(|index| &self.playlists[*index])
    }

    pub fn playlist_tracks(&self, playlist: &Playlist) -> impl Iterator<Item = &Track> {
        playlist.tracks.iter().map(|at| &self.tracks[*at])
    }

    pub fn album_index(&self, id: &ObjectId) -> Option<usize> {
        self.albums_by_id.get(id).copied()
    }

    pub fn track_index(&self, id: &ObjectId) -> Option<usize> {
        self.tracks_by_id.get(id).copied()
    }

    pub fn artwork(&self, id: &ObjectId) -> Option<&Artwork> {
        if let Some(track) = self.get(id) {
            return track.artwork.as_ref();
        }
        self.album(id).and_then(|album| album.artwork.as_ref())
    }

    pub fn album_tracks(&self, album: &Album) -> impl Iterator<Item = &Track> {
        album.tracks.iter().map(|at| &self.tracks[*at])
    }
}

fn track_from(
    file: &Scanned,
    minter: &mut Minter,
    release: Option<&Release>,
    placement: Placement,
) -> Track {
    let facts = FileFacts {
        relative_path: &file.relative,
        tags: &file.tags,
    };
    let identity = minter.track(&facts, release, placement.disc);
    let tags = &file.tags;
    Track {
        id: identity.id,
        rule: identity.rule,
        relative: fold::path(&file.relative),
        album_id: release.map(|release| release.id.clone()),
        album_inherited: placement.inherited,
        compilation: tags.compilation
            || credits::credits_various(&tags.album_artists, &tags.musicbrainz_album_artist_ids),
        title: display_title(tags, &file.path),
        title_tagged: tagged_title(tags).is_some(),
        artists: credits::paired(&tags.artists, &tags.artist_sorts),
        // Placeholders are dropped from album artists only.
        album_artists: credits::crediting(
            &tags.album_artists,
            &tags.album_artist_sorts,
            &tags.musicbrainz_album_artist_ids,
        ),
        composers: credits::paired(&tags.composers, &tags.composer_sorts),
        conductors: credits::paired(&tags.conductors, &[]),
        album: release
            .map(|release| release.title.clone())
            .or_else(|| tags.album.clone()),
        genres: tags.genres.clone(),
        date: tags.date.clone(),
        track_number: tags.track_number,
        disc_number: placement.disc.or(tags.disc_number),
        disc_from_title: tags
            .album
            .as_deref()
            .is_some_and(|album| identity::strip_disc_marker(album).1.is_some()),
        disc_subtitle: identity::clean(tags.disc_subtitle.as_deref()).map(str::to_owned),
        work: identity::clean(tags.work.as_deref()).map(str::to_owned),
        grouping: identity::clean(tags.grouping.as_deref()).map(str::to_owned),
        duration: file.properties.duration,
        size: file.size,
        mime: scan::mime_for(&file.path).unwrap_or("application/octet-stream"),
        sample_rate: file.properties.sample_rate,
        bit_depth: file.properties.bit_depth,
        channels: file.properties.channels,
        bitrate_bps: file.properties.bitrate_bps,
        artwork: file.artwork.clone(),
        recording_mbid: identity::uuid(tags.musicbrainz_recording_id.as_deref()).map(str::to_owned),
        path: file.path.clone(),
        date_added: None,
    }
}

fn tagged_title(tags: &FileTags) -> Option<String> {
    tags.title.clone().filter(|title| !title.trim().is_empty())
}

fn display_title(tags: &FileTags, path: &Path) -> String {
    tagged_title(tags).unwrap_or_else(|| {
        path.file_stem()
            .map(|stem| fold::nfc(&stem.to_string_lossy()))
            .unwrap_or_else(|| "Untitled".to_owned())
    })
}

pub fn library_name(root: &Path) -> String {
    root.file_name()
        .map(|name| fold::nfc(&name.to_string_lossy()))
        .unwrap_or_else(|| "Music".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tags::AudioProperties;

    fn scanned(relative: &str, tags: FileTags) -> Scanned {
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
            size: 27_296_681,
            artwork: None,
        }
    }

    fn tags(album: &str, title: &str, disc: Option<u32>, track: u32) -> FileTags {
        FileTags {
            title: Some(title.to_owned()),
            album: Some(album.to_owned()),
            album_artists: vec!["Kremerata Baltica".to_owned()],
            artists: vec!["Gidon Kremer".to_owned()],
            genres: vec!["Classical".to_owned()],
            disc_number: disc,
            track_number: Some(track),
            ..FileTags::default()
        }
    }

    fn untagged(title: &str) -> FileTags {
        FileTags {
            title: Some(title.to_owned()),
            ..FileTags::default()
        }
    }

    #[test]
    fn a_file_whose_tags_would_not_parse_still_joins_the_album_around_it() {
        let library = Library::build(
            "Music".to_owned(),
            &[
                scanned("Reggae/01.mp3", tags("DJ Music", "One", None, 1)),
                scanned("Reggae/02.mp3", tags("DJ Music", "Two", None, 2)),
                scanned("Reggae/03.mp3", untagged("Nameless")),
            ],
        );
        assert_eq!(library.albums().len(), 1);
        assert_eq!(library.albums()[0].tracks.len(), 3);
        assert_eq!(
            library
                .tracks()
                .iter()
                .filter(|track| track.album_inherited)
                .count(),
            1
        );
    }

    #[test]
    fn a_file_with_no_number_plays_after_the_numbered_ones() {
        let library = Library::build(
            "Music".to_owned(),
            &[
                scanned("Reggae/x.mp3", untagged("Nameless")),
                scanned("Reggae/01.mp3", tags("DJ Music", "One", None, 1)),
                scanned("Reggae/02.mp3", tags("DJ Music", "Two", None, 2)),
            ],
        );
        let titles: Vec<&str> = library
            .album_tracks(&library.albums()[0])
            .map(|track| track.title.as_str())
            .collect();
        assert_eq!(titles, vec!["One", "Two", "Nameless"]);
    }

    #[test]
    fn a_folder_of_loose_files_is_not_swallowed_by_the_one_that_names_an_album() {
        let library = Library::build(
            "Music".to_owned(),
            &[
                scanned("Singles/a.mp3", tags("Some Album", "Tagged", None, 1)),
                scanned("Singles/b.mp3", untagged("Loose One")),
                scanned("Singles/c.mp3", untagged("Loose Two")),
            ],
        );
        assert_eq!(library.albums().len(), 1);
        assert_eq!(library.albums()[0].tracks.len(), 1);
        assert!(library.tracks().iter().all(|track| !track.album_inherited));
    }

    #[test]
    fn a_folder_holding_two_albums_gives_an_untagged_file_neither() {
        let library = Library::build(
            "Music".to_owned(),
            &[
                scanned("Mixed/01.mp3", tags("First", "A", None, 1)),
                scanned("Mixed/02.mp3", tags("First", "B", None, 2)),
                scanned("Mixed/03.mp3", tags("Second", "C", None, 1)),
                scanned("Mixed/04.mp3", tags("Second", "D", None, 2)),
                scanned("Mixed/05.mp3", untagged("Nameless")),
            ],
        );
        assert_eq!(library.albums().len(), 2);
        assert!(library.tracks().iter().all(|track| !track.album_inherited));
        let nameless = library
            .tracks()
            .iter()
            .find(|track| track.title == "Nameless")
            .expect("it is still served");
        assert!(nameless.album_id.is_none());
    }

    #[test]
    fn an_inherited_album_keeps_the_credit_of_the_files_that_named_it() {
        let library = Library::build(
            "Music".to_owned(),
            &[
                scanned("Reggae/01.mp3", tags("DJ Music", "One", None, 1)),
                scanned("Reggae/02.mp3", tags("DJ Music", "Two", None, 2)),
                scanned("Reggae/03.mp3", untagged("Nameless")),
            ],
        );
        let album = &library.albums()[0];
        assert_eq!(credits::names(&album.artists), ["Kremerata Baltica"]);
        assert!(album.credited);
    }

    #[test]
    fn an_album_holds_its_tracks_in_the_order_it_plays() {
        let library = Library::build(
            "Music".to_owned(),
            &[
                scanned("Kremer/02.flac", tags("Mozart", "Andante", None, 2)),
                scanned("Kremer/01.flac", tags("Mozart", "Allegro", None, 1)),
            ],
        );
        assert_eq!(library.albums().len(), 1);
        let titles: Vec<&str> = library
            .album_tracks(&library.albums()[0])
            .map(|track| track.title.as_str())
            .collect();
        assert_eq!(titles, vec!["Allegro", "Andante"]);
    }

    #[test]
    fn a_two_disc_set_is_one_album_and_its_items_carry_its_title() {
        let library = Library::build(
            "Music".to_owned(),
            &[
                scanned(
                    "Bach/01.flac",
                    tags("Cantatas [disc 1]", "Sinfonia", None, 1),
                ),
                scanned(
                    "Bach/02.flac",
                    tags("Cantatas [disc 2]", "Chorale", None, 1),
                ),
            ],
        );
        assert_eq!(library.albums().len(), 1);
        let album = &library.albums()[0];
        assert_eq!(album.title, "Cantatas");
        assert_eq!(album.disc_count, 2);
        assert!(
            library
                .album_tracks(album)
                .all(|track| track.album.as_deref() == Some("Cantatas"))
        );
    }

    #[test]
    fn a_two_disc_set_with_no_disc_tag_keeps_both_positions() {
        let library = Library::build(
            "Music".to_owned(),
            &[
                scanned("Bach/CD1/01.flac", tags("Cantatas", "Allegro", None, 1)),
                scanned("Bach/CD2/01.flac", tags("Cantatas", "Allegro", None, 1)),
            ],
        );
        assert_eq!(library.albums().len(), 1);
        assert_eq!(library.albums()[0].disc_count, 2);
        assert_ne!(library.tracks()[0].id, library.tracks()[1].id);
        assert!(
            library
                .tracks()
                .iter()
                .all(|track| track.rule == Rule::Strings)
        );
    }

    #[test]
    fn every_track_names_the_album_it_belongs_to() {
        let library = Library::build(
            "Music".to_owned(),
            &[scanned(
                "Kremer/01.flac",
                tags("Mozart", "Allegro", None, 1),
            )],
        );
        let album = &library.albums()[0];
        assert_eq!(library.tracks()[0].album_id.as_ref(), Some(&album.id));
        assert!(library.album(&album.id).is_some());
        assert!(library.get(&library.tracks()[0].id).is_some());
    }

    #[test]
    fn a_track_artist_stands_beside_the_album_artist_rather_than_replacing_it() {
        let library = Library::build(
            "Music".to_owned(),
            &[scanned(
                "Kremer/01.flac",
                tags("Mozart", "Allegro", None, 1),
            )],
        );
        let names: Vec<&str> = library
            .artists()
            .iter()
            .map(|artist| artist.name.as_str())
            .collect();
        assert_eq!(names, vec!["Gidon Kremer", "Kremerata Baltica"]);
        assert_eq!(library.artists()[1].albums, vec![0]);
        assert!(library.artists()[1].tracks.is_empty());
        assert!(library.artists()[0].albums.is_empty());
        assert_eq!(library.artists()[0].tracks, vec![0]);
    }

    #[test]
    fn an_album_with_no_credit_falls_back_to_a_unanimous_track_artist() {
        let mut first = tags("Mozart", "Allegro", None, 1);
        first.album_artists.clear();
        let mut second = tags("Mozart", "Andante", None, 2);
        second.album_artists.clear();
        let library = Library::build(
            "Music".to_owned(),
            &[
                scanned("Kremer/01.flac", first),
                scanned("Kremer/02.flac", second),
            ],
        );
        assert_eq!(
            credits::names(&library.albums()[0].artists),
            ["Gidon Kremer"]
        );
    }

    #[test]
    fn a_compilation_is_credited_to_nobody_rather_than_to_an_invented_name() {
        let mut first = tags("Now That's What I Call Music", "One", None, 1);
        first.album_artists.clear();
        first.artists = vec!["A".to_owned()];
        let mut second = tags("Now That's What I Call Music", "Two", None, 2);
        second.album_artists.clear();
        second.artists = vec!["B".to_owned()];
        let library = Library::build(
            "Music".to_owned(),
            &[
                scanned("Various/01.flac", first),
                scanned("Various/02.flac", second),
            ],
        );
        assert!(library.albums()[0].artists.is_empty());
        let names: Vec<&str> = library
            .artists()
            .iter()
            .map(|artist| artist.name.as_str())
            .collect();
        assert_eq!(names, vec!["A", "B"]);
        assert_eq!(library.artists()[0].tracks, vec![0]);
        assert_eq!(library.artists()[1].tracks, vec![1]);
    }

    #[test]
    fn a_file_with_no_album_is_still_served_and_still_has_an_artist() {
        let mut loose = tags("", "Improvisation", None, 1);
        loose.album = None;
        let library = Library::build("Music".to_owned(), &[scanned("loose/01.flac", loose)]);
        assert_eq!(library.len(), 1);
        assert!(library.albums().is_empty());
        assert_eq!(library.tracks()[0].album_id, None);
        assert_eq!(library.artists()[0].tracks, vec![0]);
    }

    #[test]
    fn albums_and_artists_are_listed_in_a_stable_order() {
        let build = || {
            Library::build(
                "Music".to_owned(),
                &[
                    scanned("b/01.flac", tags("Élégie", "One", None, 1)),
                    scanned("a/01.flac", tags("Cantatas", "Two", None, 1)),
                ],
            )
        };
        let titles: Vec<String> = build()
            .albums()
            .iter()
            .map(|album| album.title.clone())
            .collect();
        assert_eq!(titles, vec!["Cantatas".to_owned(), "Élégie".to_owned()]);
        assert_eq!(
            build()
                .albums()
                .iter()
                .map(|album| album.id.clone())
                .collect::<Vec<_>>(),
            build()
                .albums()
                .iter()
                .map(|album| album.id.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn the_display_title_falls_back_to_the_file_name_and_identity_does_not() {
        let mut untitled = tags("Mozart", "", None, 3);
        untitled.title = None;
        let library = Library::build(
            "Music".to_owned(),
            &[scanned("Kremer/03 - Rondo.flac", untitled)],
        );
        assert_eq!(library.tracks()[0].title, "03 - Rondo");
        assert_eq!(library.tracks()[0].rule, Rule::Strings);
        assert!(
            !library.tracks()[0].title_tagged,
            "and the page can say the file carries no title of its own"
        );

        let titled = Library::build(
            "Music".to_owned(),
            &[scanned(
                "Kremer/03 - Rondo.flac",
                tags("Mozart", "Rondo", None, 3),
            )],
        );
        assert!(titled.tracks()[0].title_tagged);
    }

    #[test]
    fn a_path_a_mac_wrote_decomposed_reaches_the_listing_composed() {
        let mut untitled = tags("Homogenic", "", None, 1);
        untitled.title = None;
        let library = Library::build(
            "Music".to_owned(),
            &[scanned("Bjo\u{308}rk/Jo\u{301}ga.flac", untitled)],
        );
        assert_eq!(library.tracks()[0].title, "Jóga");
        assert_eq!(library.tracks()[0].relative, "Björk/Jóga.flac");
        assert_eq!(
            library_name(Path::new("/Volumes/Musique\u{300}")),
            "Musiquè"
        );
    }
    #[test]
    fn grouping_reads_the_files_and_nothing_else() {
        let files = [
            scanned("a/1.flac", tags("Kanon", "One", None, 1)),
            scanned("a/2.flac", tags("Kanon", "Two", None, 2)),
            scanned("b/1.flac", tags("Fratres", "One", None, 1)),
        ];
        let grouping = group(&files, &Default::default());
        assert_eq!(
            grouping.releases.len(),
            2,
            "one release per folder and title"
        );
        assert_eq!(grouping.placement.len(), files.len());
        assert_ne!(grouping.placement[0].release, grouping.placement[2].release);
    }

    #[test]
    fn minting_produces_one_track_per_file_in_the_order_they_arrived() {
        let files = [
            scanned("a/1.flac", tags("Kanon", "One", None, 1)),
            scanned("a/2.flac", tags("Kanon", "Two", None, 2)),
        ];
        let tracks = mint_tracks(&files, &group(&files, &Default::default()));
        assert_eq!(tracks.len(), 2);
        assert_eq!(tracks[0].title, "One");
        assert_eq!(tracks[1].title, "Two");
        assert_ne!(tracks[0].id, tracks[1].id);
    }

    #[test]
    fn a_various_artists_credit_is_kept_and_raises_nothing() {
        let mut compiled = tags("Compiled", "One", None, 1);
        compiled.album_artists = vec!["VA".to_owned()];
        let files = [scanned("a/1.flac", compiled)];
        let grouping = group(&files, &Default::default());
        let tracks = mint_tracks(&files, &grouping);
        let albums = albums_from(&grouping.releases, &tracks, &fold::Ignored::default());

        assert_eq!(
            tracks[0].album_artists,
            vec![credits::Credit::new(credits::VARIOUS_ARTISTS)],
            "the credit is served under one spelling rather than dropped"
        );
        assert!(
            tracks[0].compilation,
            "and the release is still known to have no one artist"
        );
        assert!(
            refused_by_derivation(&files, &tracks, &albums).is_empty(),
            "naming a release Various Artists is how a compilation is tagged, not a fault"
        );
    }

    #[test]
    fn a_credited_release_is_refused_by_nothing() {
        let files = [scanned("a/1.flac", tags("Kanon", "One", None, 1))];
        let grouping = group(&files, &Default::default());
        let tracks = mint_tracks(&files, &grouping);
        let albums = albums_from(&grouping.releases, &tracks, &fold::Ignored::default());
        assert_eq!(refused_by_derivation(&files, &tracks, &albums).total(), 0);
    }

    #[test]
    fn a_grouping_shared_by_the_whole_album_is_not_a_run() {
        let grouped = |title: &str, grouping: Option<&str>| FileTags {
            title: Some(title.to_owned()),
            album: Some("Corre Riba Corre Baxo / Nos Maos".to_owned()),
            album_artists: vec!["Abel Lima E Les Sofas".to_owned()],
            grouping: grouping.map(str::to_owned),
            ..FileTags::default()
        };
        let whole = [
            scanned("a/1.flac", grouped("Corre Riba Corre Baxo", Some("4"))),
            scanned("a/2.flac", grouped("Nos Maos", Some("4"))),
        ];
        let grouping = group(&whole, &Default::default());
        let tracks = mint_tracks(&whole, &grouping);
        let albums = albums_from(&grouping.releases, &tracks, &fold::Ignored::default());
        assert!(
            albums[0].runs.is_empty(),
            "the album opens on its tracks, not on one container named 4: {:?}",
            albums[0].runs
        );

        let part = [
            scanned("b/1.flac", grouped("A", Some("Suite"))),
            scanned("b/2.flac", grouped("B", Some("Suite"))),
            scanned("b/3.flac", grouped("C", None)),
        ];
        let grouping = group(&part, &Default::default());
        let tracks = mint_tracks(&part, &grouping);
        let albums = albums_from(&grouping.releases, &tracks, &fold::Ignored::default());
        assert_eq!(
            albums[0].runs.len(),
            1,
            "a run over part of the album stays"
        );
    }

    #[test]
    fn two_runs_of_one_name_on_unnumbered_files_are_two_containers() {
        let grouped = |title: &str, grouping: Option<&str>| FileTags {
            title: Some(title.to_owned()),
            album: Some("Suites".to_owned()),
            album_artists: vec!["Kremerata Baltica".to_owned()],
            grouping: grouping.map(str::to_owned),
            ..FileTags::default()
        };
        let files = [
            scanned("a/a.flac", grouped("A", Some("Suite"))),
            scanned("a/b.flac", grouped("B", Some("Suite"))),
            scanned("a/c.flac", grouped("C", None)),
            scanned("a/d.flac", grouped("D", Some("Suite"))),
            scanned("a/e.flac", grouped("E", Some("Suite"))),
        ];
        let grouping = group(&files, &Default::default());
        let tracks = mint_tracks(&files, &grouping);
        let albums = albums_from(&grouping.releases, &tracks, &fold::Ignored::default());
        let runs = &albums[0].runs;
        assert_eq!(
            runs.len(),
            2,
            "the untagged track cuts the name into two runs"
        );
        assert_ne!(
            runs[0].id, runs[1].id,
            "and the second must be reachable as a container of its own"
        );
    }

    #[test]
    fn the_stages_compose_into_what_build_produces() {
        let files = [
            scanned("a/1.flac", tags("Kanon", "One", None, 1)),
            scanned("a/2.flac", tags("Kanon", "Two", None, 2)),
        ];
        let built = Library::build("Music".to_owned(), &files);
        let staged = mint_tracks(&files, &group(&files, &Default::default()));
        assert_eq!(
            built
                .tracks()
                .iter()
                .map(|t| t.id.clone())
                .collect::<Vec<_>>(),
            staged.iter().map(|t| t.id.clone()).collect::<Vec<_>>()
        );
    }
}
