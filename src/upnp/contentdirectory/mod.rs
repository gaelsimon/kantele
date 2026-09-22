//! The ContentDirectory service.

pub use crate::browse::root::{ALBUMS, FOLDERS, MUSIC, PLAYLISTS, RECENT, UNTAGGED};

use crate::browse::root::{FOLDERS_TITLE, PLAYLISTS_TITLE, RECENT_TITLE, UNTAGGED_TITLE};
use crate::upnp::{ObjectId, didl, search};

#[derive(Clone, Debug)]
pub struct BrowseResponse {
    pub result: String,
    pub number_returned: usize,
    pub total_matches: usize,
    pub update_id: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{code} {description}")]
pub struct Fault {
    pub code: u16,
    pub description: &'static str,
}

impl Fault {
    pub const NO_SUCH_OBJECT: Self = Self {
        code: 701,
        description: "No such object",
    };
    pub const INVALID_ARGS: Self = Self {
        code: 402,
        description: "Invalid Args",
    };
    pub const BAD_SEARCH_CRITERIA: Self = Self {
        code: 708,
        description: "Unsupported or invalid search criteria",
    };
    pub const NO_SUCH_CONNECTION: Self = Self {
        code: 706,
        description: "Invalid connection reference",
    };
}

pub const ARTISTS: &str = "artists";
const TAG_VIEW_TITLE: &str = "[tag view]";

struct Menus {
    root: ObjectId,
    albums: ObjectId,
    artists: ObjectId,
    music: ObjectId,
    untagged: ObjectId,
    playlists: ObjectId,
    folders: ObjectId,
    recent: ObjectId,
}

/// The fixed identifiers, minted once: every browse and every search names them.
static MENUS: std::sync::LazyLock<Menus> = std::sync::LazyLock::new(|| {
    let fixed = |value: &str| {
        ObjectId::new(value.to_owned()).expect("a constant in the permitted alphabet")
    };
    Menus {
        root: ObjectId::root(),
        albums: fixed(ALBUMS),
        artists: fixed(ARTISTS),
        music: fixed(MUSIC),
        untagged: fixed(UNTAGGED),
        playlists: fixed(PLAYLISTS),
        folders: fixed(FOLDERS),
        recent: fixed(RECENT),
    }
});

pub fn browse(
    served: &crate::browse::Served,
    request: &BrowseRequest,
    to: didl::To<'_>,
    update_id: u32,
) -> Result<BrowseResponse, Fault> {
    let (library, view) = (&served.library, &served.view);
    let menus = &*MENUS;
    match request.browse_flag {
        BrowseFlag::DirectChildren => {
            let window = Window::new(request.starting_index, request.requested_count);
            let listing = children(library, view, menus, &request.object_id, window)
                .ok_or(Fault::NO_SUCH_OBJECT)?;
            let (page, total) = listing.page(window);
            Ok(BrowseResponse {
                result: didl::children(page, to),
                number_returned: page.len(),
                total_matches: total,
                update_id,
            })
        }
        BrowseFlag::Metadata => {
            let child =
                metadata(library, view, menus, &request.object_id).ok_or(Fault::NO_SUCH_OBJECT)?;
            Ok(BrowseResponse {
                result: didl::children(std::slice::from_ref(&child), to),
                number_returned: 1,
                total_matches: 1,
                update_id,
            })
        }
    }
}

pub fn search(
    served: &crate::browse::Served,
    request: &SearchRequest,
    to: didl::To<'_>,
    update_id: u32,
) -> Result<BrowseResponse, Fault> {
    let criteria = search::parse(&request.criteria).map_err(|refused| {
        tracing::info!(criteria = %request.criteria, %refused, "refusing search criteria");
        Fault::BAD_SEARCH_CRITERIA
    })?;

    let (library, view) = (&served.library, &served.view);
    let menus = &*MENUS;
    let space =
        search_space(library, view, menus, &request.container_id).ok_or(Fault::NO_SUCH_OBJECT)?;
    let window = Window::new(request.starting_index, request.requested_count);
    let matched = match space {
        Space::Whole(kinds) => indexed_matches(library, view, menus, kinds, &criteria, window),
        Space::Children(children) => Listing::All(
            children
                .into_iter()
                .filter(|child| criteria.matches(&folded(child)))
                .collect(),
        ),
    };

    let (page, total) = matched.page(window);
    Ok(BrowseResponse {
        result: didl::children(page, to),
        number_returned: page.len(),
        total_matches: total,
        update_id,
    })
}

mod matching;
mod objects;
mod paging;
mod parts;
pub mod soap;

use matching::{Space, folded, indexed_matches, search_space};
use objects::{children, metadata};
use paging::{Listing, Window};

pub use soap::{
    BrowseFlag, BrowseRequest, SearchRequest, argument, arguments_envelope, browse_envelope,
    fault_envelope, parse_browse, parse_search, search_envelope, simple_envelope,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browse::{self, Served};
    use crate::index::Library;

    fn serving(library: Library) -> Served {
        Served::new(library, browse::Settings::default())
    }

    fn serving_with(library: Library, settings: browse::Settings) -> Served {
        Served::new(library, settings)
    }

    fn library() -> Library {
        use crate::index::Scanned;
        use crate::tags::{AudioProperties, FileTags};
        use std::path::{Path, PathBuf};
        use std::time::Duration;

        let file = |relative: &str, tags: FileTags| Scanned {
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
        };
        let tagged = |album: Option<&str>, title: &str, artist: &str, track: u32| FileTags {
            title: Some(title.to_owned()),
            album: album.map(str::to_owned),
            album_artists: album.map(|_| vec![artist.to_owned()]).unwrap_or_default(),
            composers: Vec::new(),
            artists: vec![artist.to_owned()],
            track_number: Some(track),
            ..FileTags::default()
        };
        Library::build(
            "Music".to_owned(),
            &[
                file(
                    "Sierra/01.flac",
                    tagged(Some("Dundunbanza"), "Juana Pena", "Sierra Maestra", 1),
                ),
                file(
                    "Sierra/02.flac",
                    tagged(Some("Dundunbanza"), "Dundunbanza", "Sierra Maestra", 2),
                ),
                file(
                    "Loose/stray.flac",
                    tagged(None, "A Single", "Sierra Maestra", 1),
                ),
            ],
        )
    }

    fn untagged_credit() -> Library {
        use crate::index::Scanned;
        use crate::tags::{AudioProperties, FileTags};
        use std::path::{Path, PathBuf};
        use std::time::Duration;

        let file = |relative: &str, title: &str, track: u32| Scanned {
            path: Path::new("/music").join(relative),
            relative: PathBuf::from(relative),
            tags: FileTags {
                title: Some(title.to_owned()),
                album: Some("Dundunbanza".to_owned()),
                artists: vec!["Sierra Maestra".to_owned()],
                track_number: Some(track),
                ..FileTags::default()
            },
            properties: AudioProperties {
                duration: Duration::from_secs(180),
                sample_rate: Some(44_100),
                bit_depth: Some(16),
                channels: Some(2),
                bitrate_bps: Some(1_411_000),
            },
            size: 27_296_681,
            artwork: None,
        };
        Library::build(
            "Music".to_owned(),
            &[
                file("Sierra/01.flac", "Juana Pena", 1),
                file("Sierra/02.flac", "Dundunbanza", 2),
            ],
        )
    }

    fn ask(served: &Served, id: &str, flag: BrowseFlag) -> BrowseResponse {
        browse(
            served,
            &BrowseRequest {
                object_id: id.to_owned(),
                browse_flag: flag,
                starting_index: 0,
                requested_count: 0,
            },
            didl::To::plain("http://host:8200"),
            1,
        )
        .expect("the object exists")
    }

    #[test]
    fn the_root_follows_the_reference_topology() {
        let served = serving(library());
        let root = ask(&served, ObjectId::ROOT, BrowseFlag::DirectChildren);
        for id in [ALBUMS, MUSIC] {
            assert!(
                root.result.contains(&format!(r#"id="{id}""#)),
                "the root should offer {id}: {}",
                root.result
            );
        }
        assert!(root.result.contains("<dc:title>1 album</dc:title>"));
        assert!(root.result.contains("<dc:title>3 items</dc:title>"));
        assert!(!root.result.contains(r#"id="artists""#));
        assert!(!root.result.contains("<item "));
    }

    #[test]
    fn the_untagged_container_appears_only_when_something_is_untagged() {
        let served = serving(library());
        let root = ask(&served, ObjectId::ROOT, BrowseFlag::DirectChildren);
        assert!(
            !root.result.contains("[untagged]"),
            "every track here carries a tag"
        );

        let bare = serving(untagged_library());
        let root = ask(&bare, ObjectId::ROOT, BrowseFlag::DirectChildren);
        assert!(root.result.contains("<dc:title>[untagged]</dc:title>"));
        let listing = ask(&bare, UNTAGGED, BrowseFlag::DirectChildren);
        assert_eq!(listing.total_matches, 1);
        assert!(listing.result.contains("Nameless"));
    }

    fn untagged_library() -> Library {
        use crate::index::Scanned;
        use crate::tags::{AudioProperties, FileTags};
        use std::path::{Path, PathBuf};
        use std::time::Duration;

        let file = |relative: &str, tags: FileTags| Scanned {
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
        };
        Library::build(
            "Music".to_owned(),
            &[
                file(
                    "a/1.flac",
                    FileTags {
                        title: Some("Tagged".to_owned()),
                        album: Some("An Album".to_owned()),
                        artists: vec!["Somebody".to_owned()],
                        genres: vec!["Latin".to_owned()],
                        track_number: Some(1),
                        ..FileTags::default()
                    },
                ),
                file(
                    "b/1.flac",
                    FileTags {
                        title: Some("Nameless".to_owned()),
                        ..FileTags::default()
                    },
                ),
            ],
        )
    }

    #[test]
    fn the_newest_files_are_offered_by_what_they_belong_to_once_the_store_has_dated_them() {
        let undated = serving(library());
        let root = ask(&undated, ObjectId::ROOT, BrowseFlag::DirectChildren);
        assert!(
            !root.result.contains(RECENT_TITLE),
            "nothing has a date yet, so there is nothing to list"
        );

        let mut dated = library();
        for track in dated.tracks_mut() {
            // The loose file arrived last; the album's two files together, earlier.
            track.date_added = Some(if track.album_id.is_none() { 200 } else { 100 });
        }
        let served = serving(dated);
        let root = ask(&served, ObjectId::ROOT, BrowseFlag::DirectChildren);
        assert!(root.result.contains(RECENT_TITLE), "{}", root.result);
        let recent = ask(&served, RECENT, BrowseFlag::DirectChildren);
        assert_eq!(
            recent.total_matches, 2,
            "one album, listed once, and one loose file"
        );
        let item = recent.result.find("<item ").expect("the loose file");
        let container = recent.result.find("<container ").expect("the album");
        assert!(item < container, "newest first: {}", recent.result);
        assert!(recent.result.contains(r#"parentID="recent""#));

        let off = serving_with(
            served.library.as_ref().clone(),
            browse::Settings {
                recent: None,
                ..browse::Settings::default()
            },
        );
        let root = ask(&off, ObjectId::ROOT, BrowseFlag::DirectChildren);
        assert!(
            !root.result.contains(RECENT_TITLE),
            "zero in the file removes the menu"
        );
    }

    /// One album, `Cantatas`, from files whose disc, subtitle and grouping the test chooses.
    fn cantatas(files: &[(u32, Option<&str>, u32, Option<&str>)]) -> Library {
        use crate::index::Scanned;
        use crate::tags::{AudioProperties, FileTags};
        use std::path::{Path, PathBuf};

        let scanned: Vec<Scanned> = files
            .iter()
            .map(|(disc, subtitle, track, grouping)| {
                let relative = PathBuf::from(format!("Bach/{disc}-{track:02}.flac"));
                Scanned {
                    path: Path::new("/music").join(&relative),
                    relative,
                    tags: FileTags {
                        title: Some(format!("Movement {disc}.{track}")),
                        album: Some("Cantatas".to_owned()),
                        album_artists: vec!["Gardiner".to_owned()],
                        artists: vec!["Gardiner".to_owned()],
                        disc_number: Some(*disc),
                        disc_subtitle: subtitle.map(str::to_owned),
                        track_number: Some(*track),
                        grouping: grouping.map(str::to_owned),
                        ..FileTags::default()
                    },
                    properties: AudioProperties::default(),
                    size: 1,
                    artwork: None,
                }
            })
            .collect();
        Library::build("Music".to_owned(), &scanned)
    }

    #[test]
    fn an_album_whose_discs_are_named_opens_into_them_and_a_merely_numbered_one_does_not() {
        let named = serving(cantatas(&[
            (1, Some("Studio"), 1, None),
            (1, Some("Studio"), 2, None),
            (2, Some("Live"), 1, None),
        ]));
        let album = &named.library.albums()[0];
        assert!(album.separate_discs);
        let listing = ask(&named, album.id.as_str(), BrowseFlag::DirectChildren);
        assert_eq!(titles(&listing.result), ["Studio", "Live"]);
        assert!(!listing.result.contains("<item "));
        assert_eq!(
            listing.result.matches(didl::ALBUM).count(),
            2,
            "a disc opens like an album: {}",
            listing.result
        );
        let itself = ask(&named, album.id.as_str(), BrowseFlag::Metadata);
        assert!(
            itself.result.contains(r#"childCount="2""#),
            "{}",
            itself.result
        );

        let studio = &album.discs[0];
        let inside = ask(&named, studio.id.as_str(), BrowseFlag::DirectChildren);
        assert_eq!(inside.total_matches, 2);
        assert!(
            inside
                .result
                .contains(&format!(r#"parentID="{}""#, studio.id)),
            "{}",
            inside.result
        );
        let track = named.library.album_tracks(album).next().expect("a track");
        let metadata = ask(&named, track.id.as_str(), BrowseFlag::Metadata);
        assert!(
            metadata
                .result
                .contains(&format!(r#"parentID="{}""#, studio.id)),
            "a track names its disc as its parent: {}",
            metadata.result
        );

        let numbered = serving(cantatas(&[(1, None, 1, None), (2, None, 1, None)]));
        let album = &numbered.library.albums()[0];
        assert!(!album.separate_discs, "numbers alone merge");
        let listing = ask(&numbered, album.id.as_str(), BrowseFlag::DirectChildren);
        assert_eq!(listing.total_matches, 2);
        assert!(!listing.result.contains("<container "));
    }

    #[test]
    fn consecutive_tracks_sharing_a_grouping_open_as_one_item() {
        let served = serving(cantatas(&[
            (1, None, 1, Some("Symphony No. 5")),
            (1, None, 2, Some("Symphony No. 5")),
            (1, None, 3, Some("Symphony No. 5")),
            (1, None, 4, None),
            (1, None, 5, Some("Symphony No. 5")),
            (1, None, 6, Some("Symphony No. 5")),
            (1, None, 7, Some("Encore")),
        ]));
        let album = &served.library.albums()[0];
        assert_eq!(
            album.runs.len(),
            2,
            "a name used twice is two runs, and a run of one is a track"
        );
        let listing = ask(&served, album.id.as_str(), BrowseFlag::DirectChildren);
        assert_eq!(
            titles(&listing.result),
            [
                "Symphony No. 5",
                "Movement 1.4",
                "Symphony No. 5",
                "Movement 1.7"
            ]
        );
        assert_eq!(listing.result.matches("<container ").count(), 2);

        let first = &album.runs[0];
        let inside = ask(&served, first.id.as_str(), BrowseFlag::DirectChildren);
        assert_eq!(inside.total_matches, 3);
        assert!(
            inside
                .result
                .contains(&format!(r#"parentID="{}""#, first.id))
        );
        let opened = ask(&served, first.id.as_str(), BrowseFlag::Metadata);
        assert!(
            opened.result.contains(r#"childCount="3""#),
            "{}",
            opened.result
        );
        assert!(
            opened
                .result
                .contains(&format!(r#"parentID="{}""#, album.id)),
            "{}",
            opened.result
        );
    }

    #[test]
    fn a_part_nobody_minted_is_no_such_object() {
        let served = serving(cantatas(&[(1, None, 1, None)]));
        let album = &served.library.albums()[0];
        for id in [
            format!("{}.d7", album.id),
            format!("{}.g0", album.id),
            "al-nothing.d0".to_owned(),
        ] {
            let request = BrowseRequest {
                object_id: id.clone(),
                browse_flag: BrowseFlag::DirectChildren,
                starting_index: 0,
                requested_count: 0,
            };
            assert_eq!(
                browse(&served, &request, didl::To::plain("http://host:8200"), 1).unwrap_err(),
                Fault::NO_SUCH_OBJECT,
                "{id}"
            );
        }
    }

    #[test]
    fn an_index_holding_nothing_is_not_offered() {
        let library = Library::default();
        let root = ask(
            &serving(library),
            ObjectId::ROOT,
            BrowseFlag::DirectChildren,
        );
        assert_eq!(root.total_matches, 1);
        assert!(!root.result.contains(r#"id="albums""#));
        assert!(!root.result.contains(r#"id="artists""#));
    }

    #[test]
    fn an_album_lists_its_tracks_and_nothing_else() {
        let served = serving(library());
        let album = &served.library.albums()[0];
        let listing = ask(&served, album.id.as_str(), BrowseFlag::DirectChildren);
        assert_eq!(listing.total_matches, album.tracks.len());
        assert!(!listing.result.contains("<container "));
        assert!(listing.result.contains("Juana Pena"));
    }

    #[test]
    fn an_album_container_carries_the_reference_class_and_its_credit() {
        let served = serving(library());
        let listing = ask(&served, ALBUMS, BrowseFlag::DirectChildren);
        assert!(listing.result.contains("object.container.album.musicAlbum"));
        assert!(
            listing
                .result
                .contains(r#"<upnp:artist role="AlbumArtist">Sierra Maestra</upnp:artist>"#)
        );
    }

    #[test]
    fn a_credit_nobody_tagged_is_not_claimed_as_an_album_artist() {
        let library = untagged_credit();
        let listing = ask(&serving(library), ALBUMS, BrowseFlag::DirectChildren);
        assert!(listing.result.contains("<upnp:artist>Sierra Maestra"));
        assert!(!listing.result.contains(r#"role="AlbumArtist""#));
    }

    #[test]
    fn an_artist_holds_its_albums_and_then_its_loose_tracks() {
        let served = serving(library());
        let artist = &served.library.artists()[0];
        let listing = ask(&served, artist.id.as_str(), BrowseFlag::DirectChildren);
        assert_eq!(listing.total_matches, 2);
        let container = listing.result.find("<container ").expect("an album");
        let item = listing.result.find("<item ").expect("the loose track");
        assert!(container < item, "albums come before loose tracks");
    }

    #[test]
    fn paging_runs_across_containers_and_items_together() {
        let served = serving(library());
        let artist = served.library.artists()[0].id.clone();
        let second = browse(
            &served,
            &BrowseRequest {
                object_id: artist.as_str().to_owned(),
                browse_flag: BrowseFlag::DirectChildren,
                starting_index: 1,
                requested_count: 1,
            },
            didl::To::plain("http://host:8200"),
            1,
        )
        .expect("the artist exists");
        assert_eq!(second.number_returned, 1);
        assert_eq!(second.total_matches, 2);
        assert!(second.result.contains("<item "));
        assert!(!second.result.contains("<container "));
    }

    fn titles(didl: &str) -> Vec<String> {
        didl.split("<dc:title>")
            .skip(1)
            .filter_map(|rest| rest.split_once("</dc:title>"))
            .map(|(title, _)| title.to_owned())
            .collect()
    }

    fn many_artists(count: usize) -> Library {
        use crate::index::Scanned;
        use crate::tags::{AudioProperties, FileTags};
        use std::path::{Path, PathBuf};
        use std::time::Duration;

        let files: Vec<Scanned> = (0..count)
            .map(|n| {
                let relative = PathBuf::from(format!("artist{n:02}/01.flac"));
                Scanned {
                    path: Path::new("/music").join(&relative),
                    relative,
                    tags: FileTags {
                        title: Some(format!("Track {n:02}")),
                        album: Some(format!("Album {n:02}")),
                        album_artists: vec![format!("Artist {n:02}")],
                        artists: vec![format!("Artist {n:02}")],
                        track_number: Some(1),
                        ..FileTags::default()
                    },
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
            })
            .collect();
        Library::build("Music".to_owned(), &files)
    }

    #[test]
    fn a_page_of_an_axis_is_the_slice_of_it_the_whole_listing_gives() {
        let served = serving(many_artists(40));
        let listing = browse::Position::default().listing(browse::Facet::Artist);
        let whole = ask(&served, &listing.id(), BrowseFlag::DirectChildren);
        let page = browse(
            &served,
            &BrowseRequest {
                object_id: listing.id(),
                browse_flag: BrowseFlag::DirectChildren,
                starting_index: 10,
                requested_count: 5,
            },
            didl::To::plain("http://host:8200"),
            1,
        )
        .expect("the axis exists");
        assert_eq!(whole.total_matches, 40);
        assert_eq!(page.total_matches, whole.total_matches);
        assert_eq!(page.number_returned, 5);
        assert_eq!(titles(&page.result), titles(&whole.result)[10..15]);
    }

    #[test]
    fn one_letter_offers_no_way_in_because_it_skips_nothing() {
        let library = many_artists(40);
        let settings = browse::Settings {
            alpha_group: Some(4),
            ..browse::Settings::default()
        };
        let listing = browse::Position::default().listing(browse::Facet::Artist);
        let page = ask(
            &serving_with(library, settings.clone()),
            &listing.id(),
            BrowseFlag::DirectChildren,
        );
        assert_eq!(
            page.total_matches,
            40,
            "forty values and nothing beside them: {}",
            &page.result[..200]
        );
    }

    #[test]
    fn the_way_into_the_index_is_the_first_child_and_is_counted_as_one() {
        let library = artists_named(&["Alva Noto", "Boards of Canada", "Coil", "Dopplereffekt"]);
        let settings = browse::Settings {
            album_threshold: 0,
            alpha_group: Some(4),
            ..browse::Settings::default()
        };
        let listing = browse::Position::default().listing(browse::Facet::Artist);
        let index = listing.at_index();

        let served = serving_with(library, settings);
        let whole = ask(&served, &listing.id(), BrowseFlag::DirectChildren);
        assert_eq!(
            whole.total_matches, 5,
            "four artists and the way into the index"
        );
        let first = whole
            .result
            .find(&index.id())
            .expect("the index is a child of the listing");
        let alva = whole.result.find("Alva Noto").expect("so are the artists");
        assert!(
            first < alva,
            "and it is the first of them: {}",
            whole.result
        );

        let rest = browse(
            &served,
            &BrowseRequest {
                object_id: listing.id(),
                browse_flag: BrowseFlag::DirectChildren,
                starting_index: 1,
                requested_count: 2,
            },
            didl::To::plain("http://host:8200"),
            1,
        )
        .expect("the object exists");
        assert_eq!(rest.total_matches, 5, "the total counts it from every page");
        assert_eq!(rest.number_returned, 2);
        assert!(
            !rest.result.contains(&index.id()),
            "a later page does not repeat it: {}",
            rest.result
        );
        assert!(rest.result.contains("Boards of Canada"));
    }

    fn artists_named(initials: &[&str]) -> Library {
        use crate::index::Scanned;
        use crate::tags::{AudioProperties, FileTags};
        use std::path::{Path, PathBuf};
        use std::time::Duration;

        let files: Vec<Scanned> = initials
            .iter()
            .enumerate()
            .map(|(at, artist)| {
                let relative = PathBuf::from(format!("{at}/01.flac"));
                Scanned {
                    path: Path::new("/music").join(&relative),
                    relative,
                    tags: FileTags {
                        title: Some(format!("Track {at}")),
                        album: Some(format!("Album {at}")),
                        album_artists: vec![(*artist).to_owned()],
                        artists: vec![(*artist).to_owned()],
                        track_number: Some(1),
                        ..FileTags::default()
                    },
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
            })
            .collect();
        Library::build("Music".to_owned(), &files)
    }

    #[test]
    fn a_letter_group_holds_the_values_under_it_and_names_itself() {
        let library = artists_named(&["Alva Noto", "Autechre", "Boards of Canada", "Coil"]);
        let settings = browse::Settings {
            album_threshold: 0,
            alpha_group: Some(1),
            ..browse::Settings::default()
        };
        let served = serving_with(library, settings);
        let listing = browse::Position::default().listing(browse::Facet::Artist);
        let groups = ask(&served, &listing.id(), BrowseFlag::DirectChildren);
        assert!(
            groups.total_matches > 1,
            "the fixture's artists start with more than one letter"
        );
        assert!(
            groups.result.contains(didl::MENU),
            "a group is a menu rather than an artist"
        );

        let Some(browse::Menu::Letters(_, offered)) =
            browse::menu(&served.library, &served.view, &listing.at_index())
        else {
            panic!("the index holds letters");
        };
        let first = offered.first().expect("a group").key;
        let inside = listing.at_letter(first);
        let values = ask(&served, &inside.id(), BrowseFlag::DirectChildren);
        assert!(values.total_matches >= 1, "the group holds its values");
        assert!(
            values.result.contains(didl::ARTIST),
            "and inside a group the values carry the axis class again"
        );

        let itself = ask(&served, &inside.id(), BrowseFlag::Metadata);
        assert!(
            itself
                .result
                .contains(&format!("<dc:title>{first}</dc:title>")),
            "a group is titled by its letter: {}",
            itself.result
        );
    }

    #[test]
    fn an_axis_gives_its_values_the_class_the_reference_gives_them() {
        let library = many_artists(40);
        for (facet, class) in [
            (browse::Facet::Artist, didl::ARTIST),
            (browse::Facet::AllArtists, didl::ARTIST),
            (browse::Facet::Composer, didl::ARTIST),
            (browse::Facet::Genre, didl::GENRE),
            (browse::Facet::Date, didl::MENU),
        ] {
            assert_eq!(facet.class(), class, "{facet:?}");
        }
        let listing = browse::Position::default().listing(browse::Facet::Artist);
        let page = ask(&serving(library), &listing.id(), BrowseFlag::DirectChildren);
        assert!(
            page.result.contains(didl::ARTIST),
            "a control point lays out by the class: {}",
            &page.result[..400]
        );
    }

    #[test]
    fn a_page_of_the_album_index_is_the_slice_of_it_the_whole_listing_gives() {
        let served = serving(many_artists(40));
        let whole = ask(&served, ALBUMS, BrowseFlag::DirectChildren);
        let page = browse(
            &served,
            &BrowseRequest {
                object_id: ALBUMS.to_owned(),
                browse_flag: BrowseFlag::DirectChildren,
                starting_index: 30,
                requested_count: 5,
            },
            didl::To::plain("http://host:8200"),
            1,
        )
        .expect("the album index exists");
        assert_eq!(whole.total_matches, 40);
        assert_eq!(page.total_matches, whole.total_matches);
        assert_eq!(titles(&page.result), titles(&whole.result)[30..35]);
    }

    #[test]
    fn a_page_of_an_axis_past_its_end_is_empty_and_still_reports_the_whole() {
        let served = serving(many_artists(40));
        let listing = browse::Position::default().listing(browse::Facet::Artist);
        let page = browse(
            &served,
            &BrowseRequest {
                object_id: listing.id(),
                browse_flag: BrowseFlag::DirectChildren,
                starting_index: 99,
                requested_count: 5,
            },
            didl::To::plain("http://host:8200"),
            1,
        )
        .expect("the axis exists");
        assert_eq!(page.number_returned, 0);
        assert_eq!(page.total_matches, 40);
    }

    #[test]
    fn the_metadata_of_an_axis_counts_its_values_without_listing_them() {
        let library = many_artists(40);
        let listing = browse::Position::default().listing(browse::Facet::Artist);
        let metadata = ask(&serving(library), &listing.id(), BrowseFlag::Metadata);
        assert_eq!(metadata.number_returned, 1);
        assert!(
            metadata.result.contains(r#"childCount="40""#),
            "{}",
            metadata.result
        );
    }

    #[test]
    fn a_track_names_its_album_as_its_parent_rather_than_the_flat_container() {
        let served = serving(library());
        let album = &served.library.albums()[0];
        let track = served.library.album_tracks(album).next().expect("a track");
        let metadata = ask(&served, track.id.as_str(), BrowseFlag::Metadata);
        assert!(
            metadata
                .result
                .contains(&format!(r#"parentID="{}""#, album.id)),
            "{}",
            metadata.result
        );
    }

    #[test]
    fn a_loose_track_still_names_the_container_it_was_found_in() {
        let served = serving(library());
        let loose = served
            .library
            .tracks()
            .iter()
            .find(|track| track.album_id.is_none())
            .expect("one file belongs to no album");
        let metadata = ask(&served, loose.id.as_str(), BrowseFlag::Metadata);
        assert!(metadata.result.contains(r#"parentID="music""#));
    }

    #[test]
    fn an_album_names_the_album_index_as_its_parent() {
        let served = serving(library());
        let album = &served.library.albums()[0];
        let metadata = ask(&served, album.id.as_str(), BrowseFlag::Metadata);
        assert!(metadata.result.contains(r#"parentID="albums""#));
        assert_eq!(metadata.total_matches, 1);
    }

    fn look_for(served: &Served, container: &str, criteria: &str) -> BrowseResponse {
        search(
            served,
            &SearchRequest {
                container_id: container.to_owned(),
                criteria: criteria.to_owned(),
                starting_index: 0,
                requested_count: 0,
            },
            didl::To::plain("http://host:8200"),
            1,
        )
        .expect("the container exists and the criteria parse")
    }

    /// The shipped fixture carries no genre, and a genre search needs some.
    fn genred() -> Library {
        use crate::index::Scanned;
        use crate::tags::{AudioProperties, FileTags};
        use std::path::{Path, PathBuf};

        let file = |relative: &str, title: &str, genre: &str| Scanned {
            path: Path::new("/music").join(relative),
            relative: PathBuf::from(relative),
            tags: FileTags {
                title: Some(title.to_owned()),
                album: Some("Dundunbanza".to_owned()),
                album_artists: vec!["Sierra Maestra".to_owned()],
                artists: vec!["Sierra Maestra".to_owned()],
                genres: vec![genre.to_owned()],
                track_number: Some(1),
                ..FileTags::default()
            },
            properties: AudioProperties::default(),
            size: 27,
            artwork: None,
        };
        Library::build(
            "Music".to_owned(),
            &[
                file("a/01.flac", "Juana Pena", "Son"),
                file("a/02.flac", "Dundunbanza", "Son"),
                file("b/01.flac", "Rougher Dub", "Reggae"),
            ],
        )
    }

    #[test]
    fn the_root_names_the_parent_the_specification_fixes_for_it() {
        let itself = ask(&serving(library()), ObjectId::ROOT, BrowseFlag::Metadata);
        assert!(
            itself.result.contains(r#"parentID="-1""#),
            "naming itself makes the root its own child to anything walking parents: {}",
            itself.result
        );
    }

    #[test]
    fn a_folder_is_published_as_a_folder_rather_than_as_a_menu() {
        let served = serving(library());
        for id in [ObjectId::ROOT, FOLDERS] {
            let listing = ask(&served, id, BrowseFlag::DirectChildren);
            assert!(
                listing.result.contains(didl::FOLDER),
                "a control point tells a folder from a menu by the class, and both incumbents \
                 mark theirs: browsing {id} gave {}",
                listing.result
            );
        }
        let itself = ask(&served, FOLDERS, BrowseFlag::Metadata);
        assert!(itself.result.contains(didl::FOLDER), "{}", itself.result);
    }

    #[test]
    fn the_search_an_amplifier_sends_for_its_own_genre_menu_finds_them() {
        let served = serving(genred());
        let found = look_for(
            &served,
            ObjectId::ROOT,
            r#"upnp:class derivedfrom "object.container.genre.musicGenre""#,
        );
        assert_eq!(
            found.total_matches, 2,
            "a control point that builds its own genre list off a search gets nothing otherwise, \
             which is what the HEOS app does"
        );
        assert!(found.result.contains("object.container.genre.musicGenre"));
        assert!(found.result.contains("Son") && found.result.contains("Reggae"));
        assert!(
            !found.result.contains("A-Z"),
            "the letter index is not a genre"
        );
    }

    #[test]
    fn the_query_an_amplifier_menu_sends_returns_album_containers() {
        let served = serving(library());
        let found = look_for(
            &served,
            ObjectId::ROOT,
            r#"upnp:class derivedfrom "object.container.album" and @refID exists false"#,
        );
        assert_eq!(found.total_matches, served.library.albums().len());
        assert!(found.result.contains("object.container.album.musicAlbum"));
        assert!(!found.result.contains("<item "));
    }

    #[test]
    fn a_class_this_server_does_not_publish_answers_empty_rather_than_faulting() {
        let found = look_for(
            &serving(library()),
            ObjectId::ROOT,
            r#"upnp:class derivedfrom "object.item.imageItem""#,
        );
        assert_eq!(found.total_matches, 0);
        assert_eq!(found.number_returned, 0);
        assert!(found.result.contains("DIDL-Lite"));
    }

    #[test]
    fn criteria_that_cannot_be_evaluated_fault_with_708() {
        for criteria in [
            // `and` and `or` at one level: the grammar gives them no precedence.
            r#"dc:title = "a" and dc:title = "b" or dc:title = "c""#,
            r#"kantele:nonsense contains "x""#,
            "",
        ] {
            let request = SearchRequest {
                container_id: ObjectId::ROOT.to_owned(),
                criteria: criteria.to_owned(),
                starting_index: 0,
                requested_count: 0,
            };
            assert_eq!(
                search(
                    &serving(library()),
                    &request,
                    didl::To::plain("http://host:8200"),
                    1
                )
                .unwrap_err(),
                Fault::BAD_SEARCH_CRITERIA,
                "{criteria:?} should be refused"
            );
        }
    }

    #[test]
    fn a_disjunction_answers_with_the_union_of_its_sides() {
        let served = serving(library());
        let title = look_for(&served, ObjectId::ROOT, r#"dc:title contains "juana""#);
        let artist = look_for(&served, ObjectId::ROOT, r#"upnp:artist contains "sierra""#);
        let either = look_for(
            &served,
            ObjectId::ROOT,
            r#"dc:title contains "juana" or upnp:artist contains "sierra""#,
        );
        assert!(title.total_matches > 0 && artist.total_matches > 0);
        assert!(
            either.total_matches >= artist.total_matches.max(title.total_matches),
            "a union cannot be smaller than either side: {} against {} and {}",
            either.total_matches,
            title.total_matches,
            artist.total_matches
        );

        let tracks = look_for(
            &served,
            ObjectId::ROOT,
            r#"upnp:class derivedfrom "object.item.audioItem" and (dc:title contains "nothinghere" or upnp:artist contains "sierra")"#,
        );
        assert_eq!(
            tracks.total_matches,
            served.library.tracks().len(),
            "every track is credited to that artist and none of them is a container"
        );
    }

    #[test]
    fn a_search_scoped_to_a_container_does_not_leave_it() {
        let served = serving(library());
        let album = &served.library.albums()[0];
        let everything = look_for(&served, ObjectId::ROOT, "*");
        let inside = look_for(&served, album.id.as_str(), "*");
        assert_eq!(inside.total_matches, album.tracks.len());
        assert!(inside.total_matches < everything.total_matches);
    }

    #[test]
    fn an_object_appears_once_even_though_two_containers_hold_it() {
        let served = serving(library());
        let found = look_for(
            &served,
            ObjectId::ROOT,
            r#"upnp:class derivedfrom "object.container""#,
        );
        assert_eq!(
            found.total_matches,
            served.library.albums().len() + served.library.artists().len()
        );
    }

    #[test]
    fn the_predicate_applies_before_the_paging() {
        let served = serving(library());
        let request = SearchRequest {
            container_id: ObjectId::ROOT.to_owned(),
            criteria: r#"upnp:class derivedfrom "object.item""#.to_owned(),
            starting_index: 1,
            requested_count: 1,
        };
        let page =
            search(&served, &request, didl::To::plain("http://host:8200"), 1).expect("it searches");
        assert_eq!(page.number_returned, 1);
        assert_eq!(page.total_matches, served.library.tracks().len());
    }

    #[test]
    fn a_page_of_a_search_is_the_slice_of_it_the_whole_result_gives() {
        let served = serving(many_artists(40));
        let criteria = r#"upnp:class derivedfrom "object.container.album.musicAlbum""#;
        let whole = search(
            &served,
            &SearchRequest {
                container_id: ObjectId::ROOT.to_owned(),
                criteria: criteria.to_owned(),
                starting_index: 0,
                requested_count: 0,
            },
            didl::To::plain("http://host:8200"),
            1,
        )
        .expect("it searches");
        let page = search(
            &served,
            &SearchRequest {
                container_id: ObjectId::ROOT.to_owned(),
                criteria: criteria.to_owned(),
                starting_index: 30,
                requested_count: 5,
            },
            didl::To::plain("http://host:8200"),
            1,
        )
        .expect("it searches");
        assert_eq!(whole.total_matches, 40);
        assert_eq!(page.total_matches, whole.total_matches);
        assert_eq!(page.number_returned, 5);
        assert_eq!(titles(&page.result), titles(&whole.result)[30..35]);
    }

    #[test]
    fn a_search_of_an_unknown_container_faults() {
        let request = SearchRequest {
            container_id: "ar-deadbeefdeadbeef".to_owned(),
            criteria: "*".to_owned(),
            starting_index: 0,
            requested_count: 0,
        };
        assert_eq!(
            search(
                &serving(library()),
                &request,
                didl::To::plain("http://host:8200"),
                1
            )
            .unwrap_err(),
            Fault::NO_SUCH_OBJECT
        );
    }

    #[test]
    fn every_container_a_listing_returns_is_marked_searchable() {
        let served = serving(library());
        for id in [ObjectId::ROOT, ALBUMS, ARTISTS] {
            let listing = ask(&served, id, BrowseFlag::DirectChildren);
            assert!(
                !listing.result.contains(r#"searchable="0""#),
                "{id} returns a container that is not searchable:\n{}",
                listing.result
            );
        }
    }

    #[test]
    fn an_unknown_object_still_faults_rather_than_answering_empty() {
        let served = serving(library());
        let request = BrowseRequest {
            object_id: "al-deadbeefdeadbeef".to_owned(),
            browse_flag: BrowseFlag::DirectChildren,
            starting_index: 0,
            requested_count: 0,
        };
        assert_eq!(
            browse(&served, &request, didl::To::plain("http://host:8200"), 1).unwrap_err(),
            Fault::NO_SUCH_OBJECT
        );
    }
}
