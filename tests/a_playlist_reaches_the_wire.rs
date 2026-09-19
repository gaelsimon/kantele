//! What a playlist file becomes: a container a client can open, and text a `Search` can reach.

use kantele::index::{Library, ScanOptions, Store};
use kantele::service::{self, Indexing, Pass};
use kantele::upnp::contentdirectory::{
    BrowseFlag, BrowseRequest, PLAYLISTS, SearchRequest, browse, search,
};
use kantele::upnp::{ObjectId, didl};

mod fixtures;
use fixtures::Tree;

fn crate_of_selections(name: &str) -> Tree {
    let tree = Tree::new(name);
    tree.album("Sierra Maestra", &["01 Juana.wav", "02 Dundun.wav"], true);
    tree.album("Kremerata", &["01 Elegie.wav"], false);
    tree.text(
        "_mariage/_party.m3u",
        "#EXTM3U\n\
         #PLAYLIST:Party\n\
         #EXTINF:1,Vainberg conducted by Thielemann\n\
         ../Kremerata/01 Elegie.wav\n\
         #EXTINF:1,Sierra Maestra - Juana Pena\n\
         ../Sierra Maestra/01 Juana.wav\n",
    );
    tree
}

fn serving(library: &Library) -> kantele::browse::Served {
    kantele::browse::Served::new(library.clone(), kantele::browse::Settings::default())
}

fn scanned(tree: &Tree) -> Library {
    Library::scan_with(&tree.0, &ScanOptions::default()).expect("scanning the test tree")
}

/// What the walk made of the tree, for an assertion that has to say why it is not there.
fn found(library: &Library) -> String {
    let tracks: Vec<String> = library
        .tracks()
        .iter()
        .map(|track| track.path.display().to_string())
        .collect();
    let playlists: Vec<&str> = library
        .playlists()
        .iter()
        .map(|playlist| playlist.title.as_str())
        .collect();
    format!("tracks {tracks:?}, playlists {playlists:?}")
}

fn ask(library: &Library, id: &str, flag: BrowseFlag) -> String {
    browse(
        &serving(library),
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
    .result
}

fn find(library: &Library, container: &str, criteria: &str) -> String {
    search(
        &serving(library),
        &SearchRequest {
            container_id: container.to_owned(),
            criteria: criteria.to_owned(),
            starting_index: 0,
            requested_count: 0,
        },
        didl::To::plain("http://host:8200"),
        1,
    )
    .expect("the container exists")
    .result
}

#[test]
fn a_playlist_becomes_a_container_the_root_offers() {
    let tree = crate_of_selections("playlist-root");
    let library = scanned(&tree);

    assert_eq!(library.playlists().len(), 1, "{}", found(&library));
    assert_eq!(library.playlists()[0].title, "Party");

    let root = ask(&library, ObjectId::ROOT, BrowseFlag::DirectChildren);
    assert!(
        root.contains(&format!(r#"id="{PLAYLISTS}""#)),
        "the root should offer the playlists menu: {root}"
    );

    let menu = ask(&library, PLAYLISTS, BrowseFlag::DirectChildren);
    assert!(menu.contains("<dc:title>Party</dc:title>"));
    assert!(
        menu.contains("object.container.playlistContainer"),
        "a playlist is a playlistContainer, which is the class a control point lays out: {menu}"
    );
}

#[test]
fn a_playlist_plays_in_its_own_order_and_not_the_folders() {
    let tree = crate_of_selections("playlist-order");
    let library = scanned(&tree);

    let playlist = &library.playlists()[0];
    let titles: Vec<&str> = library
        .playlist_tracks(playlist)
        .map(|track| track.title.as_str())
        .collect();
    assert_eq!(
        titles,
        ["01 Elegie", "01 Juana"],
        "the playlist names Kremerata first and the walk does not"
    );

    let opened = ask(&library, playlist.id.as_str(), BrowseFlag::DirectChildren);
    let elegie = opened.find("01 Elegie").expect("the first entry is served");
    let juana = opened.find("01 Juana").expect("the second entry is served");
    assert!(
        elegie < juana,
        "the wire keeps the playlist's order: {opened}"
    );
}

#[test]
fn a_track_is_found_by_the_text_its_playlist_wrote_for_it() {
    let tree = crate_of_selections("playlist-search");
    let library = scanned(&tree);

    // The word is in no tag or file name, only in the `#EXTINF` line.
    let found = find(
        &library,
        ObjectId::ROOT,
        r#"upnp:class derivedfrom "object.item.audioItem.musicTrack" and dc:title contains "thielemann""#,
    );
    assert!(
        found.contains("01 Elegie"),
        "the entry text should reach the track it names: {found}"
    );
    assert!(
        !found.contains("01 Juana"),
        "and it should reach no other track: {found}"
    );
}

#[test]
fn a_playlist_answers_a_search_for_its_own_class() {
    let tree = crate_of_selections("playlist-class");
    let library = scanned(&tree);

    let found = find(
        &library,
        ObjectId::ROOT,
        r#"upnp:class derivedfrom "object.container.playlistContainer" and dc:title contains "party""#,
    );
    assert!(
        found.contains("<dc:title>Party</dc:title>"),
        "a search for the playlist class should answer with it: {found}"
    );
}

#[test]
fn an_entry_written_in_another_case_still_names_its_track() {
    let tree = Tree::new("playlist-case");
    tree.album("kremerata", &["01 elegie.wav"], false);
    tree.text("Crate.m3u", "#EXTM3U\nKremerata/01 Elegie.wav\n");

    let library = scanned(&tree);
    assert_eq!(library.playlists().len(), 1);
    assert_eq!(library.playlists()[0].tracks.len(), 1);
}

#[test]
fn a_playlist_naming_nothing_this_library_holds_is_not_a_menu_entry() {
    let tree = Tree::new("playlist-empty");
    tree.album("Sierra Maestra", &["01 Juana.wav"], false);
    tree.text("Radio.m3u", "http://example.org/stream\n../outside.wav\n");

    let library = scanned(&tree);
    assert!(library.playlists().is_empty());
    let root = ask(&library, ObjectId::ROOT, BrowseFlag::DirectChildren);
    assert!(!root.contains(&format!(r#"id="{PLAYLISTS}""#)));
}

#[test]
fn a_rescan_of_one_folder_keeps_the_playlists_of_every_other() {
    let tree = crate_of_selections("playlist-rescan");
    let mut store = Some(Store::in_memory().expect("a store"));
    let first = service::index(&Indexing::of(&tree.0), &mut store, Pass::Whole)
        .expect("the first pass")
        .library;
    assert_eq!(first.playlists().len(), 1);

    tree.write("Kremerata/02 New.wav", 9);
    let after = service::index(
        &Indexing::of(&tree.0),
        &mut store,
        Pass::Within(kantele::index::scan::Scope::of([std::path::PathBuf::from(
            "Kremerata/02 New.wav",
        )])),
    )
    .expect("an incremental pass")
    .library;

    assert_eq!(
        after.playlists().len(),
        1,
        "the playlist lives in a folder this pass never walked"
    );
    assert_eq!(after.playlists()[0].tracks.len(), 2);
}

#[test]
fn a_start_from_the_store_serves_the_playlists_it_remembers() {
    let tree = crate_of_selections("playlist-remembered");
    let mut store = Some(Store::in_memory().expect("a store"));
    service::index(&Indexing::of(&tree.0), &mut store, Pass::Whole).expect("the first pass");

    let remembered =
        service::remembered(&Indexing::of(&tree.0), &mut store).expect("rows to serve");
    assert_eq!(remembered.playlists().len(), 1);
    assert_eq!(remembered.playlists()[0].tracks.len(), 2);
}

#[test]
fn a_playlist_that_goes_away_is_forgotten() {
    let tree = crate_of_selections("playlist-forgotten");
    let mut store = Some(Store::in_memory().expect("a store"));
    service::index(&Indexing::of(&tree.0), &mut store, Pass::Whole).expect("the first pass");

    tree.remove("_mariage/_party.m3u");
    let after = service::index(&Indexing::of(&tree.0), &mut store, Pass::Whole)
        .expect("a second pass")
        .library;
    assert!(after.playlists().is_empty());

    let remembered = service::remembered(&Indexing::of(&tree.0), &mut store).expect("rows");
    assert!(
        remembered.playlists().is_empty(),
        "the row went with the file"
    );
}
