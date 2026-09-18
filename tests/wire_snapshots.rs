//! Every document Kantele puts on the wire, frozen.

use std::path::PathBuf;
use std::time::Duration;

use kantele::index::credits::Credit;
use kantele::index::{Library, Rule, Track};
use kantele::upnp::description::{self, DeviceIdentity};
use kantele::upnp::{ObjectId, contentdirectory, didl};

/// A fixed identity, so the snapshots describe the format rather than the machine.
const BASE_URL: &str = "http://192.0.2.1:8200";

fn identity() -> DeviceIdentity {
    DeviceIdentity {
        friendly_name: "Kantele".to_owned(),
        udn: "3d5d1cbe-8f2a-4d1e-9a9c-7c2f0a1b2c3d".to_owned(),
        icon: Default::default(),
    }
}

fn track(n: u32, title: &str, millis: u64, size: u64) -> Track {
    Track {
        id: ObjectId::new(format!("t{n:016x}")).unwrap(),
        rule: Rule::Strings,
        album_id: None,
        album_inherited: false,
        title_tagged: true,
        compilation: false,
        path: PathBuf::from(format!("/music/{n:02} - {title}.flac")),
        relative: format!("{n:02} - {title}.flac"),
        title: title.to_owned(),
        artists: vec![Credit::new("Sierra Maestra")],
        album_artists: vec![Credit::new("Sierra Maestra")],
        composers: Vec::new(),
        album: Some("!Dundunbanza!".to_owned()),
        genres: vec!["Latin".to_owned()],
        date: Some("1994".to_owned()),
        track_number: Some(n),
        disc_number: None,
        duration: Duration::from_millis(millis),
        size,
        mime: "audio/x-flac",
        sample_rate: Some(44_100),
        bit_depth: Some(16),
        channels: Some(2),
        bitrate_bps: Some(1_411_000),
        artwork: None,
        recording_mbid: None,
        date_added: None,
        disc_from_title: false,
        disc_subtitle: None,
        work: None,
        grouping: None,
    }
}

/// The three files milestone 1 is tested against, with their real durations and sizes.
fn library() -> Library {
    Library::from_tracks([
        track(1, "Juana Peña", 243_026, 27_296_681),
        track(2, "Dundunbanza", 309_573, 33_619_600),
        track(3, "No Me Llores Más", 316_120, 34_336_845),
    ])
}

fn browse(
    served: &kantele::browse::Served,
    object_id: &str,
    flag: contentdirectory::BrowseFlag,
    count: usize,
) -> String {
    let request = contentdirectory::BrowseRequest {
        object_id: object_id.to_owned(),
        browse_flag: flag,
        starting_index: 0,
        requested_count: count,
    };
    let response = contentdirectory::browse(
        served,
        &request,
        didl::To::plain("http://192.0.2.1:8200"),
        1,
    )
    .unwrap();
    contentdirectory::browse_envelope(&response)
}

fn one_item(track: &Track) -> String {
    let root = ObjectId::root();
    didl::children(
        &[didl::Child::Item(track, &root)],
        didl::To::plain(BASE_URL),
    )
}

fn readable(xml: &str) -> String {
    xml.replace("><", ">\n<")
}

#[test]
fn device_description() {
    let xml = description::device(&identity()).replace(
        &format!("<modelNumber>{}</modelNumber>", env!("CARGO_PKG_VERSION")),
        "<modelNumber>[version]</modelNumber>",
    );
    insta::assert_snapshot!(xml);
}

#[test]
fn content_directory_service_definition() {
    insta::assert_snapshot!(description::CONTENT_DIRECTORY_SCPD);
}

#[test]
fn connection_manager_service_definition() {
    insta::assert_snapshot!(description::CONNECTION_MANAGER_SCPD);
}

/// A library built through the identity rules, so a snapshot freezes what a client receives.
fn serving() -> kantele::browse::Served {
    kantele::browse::Served::new(indexed(), kantele::browse::Settings::default())
}

fn indexed() -> Library {
    use kantele::index::Scanned;
    use kantele::tags::{AudioProperties, FileTags};

    let file = |relative: &str, title: &str, album: Option<&str>, n: u32| Scanned {
        path: PathBuf::from("/music").join(relative),
        relative: PathBuf::from(relative),
        tags: FileTags {
            title: Some(title.to_owned()),
            album: album.map(str::to_owned),
            album_artists: album
                .map(|_| vec!["Sierra Maestra".to_owned()])
                .unwrap_or_default(),
            composers: Vec::new(),
            artists: vec!["Sierra Maestra".to_owned()],
            genres: vec!["Latin".to_owned()],
            date: Some("1994".to_owned()),
            track_number: Some(n),
            ..FileTags::default()
        },
        properties: AudioProperties {
            duration: Duration::from_millis(243_026),
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
            file("Sierra/01.flac", "Juana Peña", Some("!Dundunbanza!"), 1),
            file("Sierra/02.flac", "Dundunbanza", Some("!Dundunbanza!"), 2),
            file("Loose/stray.flac", "No Me Llores Más", None, 1),
        ],
    )
}

#[test]
fn browse_a_root_that_has_albums() {
    insta::assert_snapshot!(readable(&browse(
        &serving(),
        ObjectId::ROOT,
        contentdirectory::BrowseFlag::DirectChildren,
        10
    )));
}

#[test]
fn browse_the_album_index() {
    insta::assert_snapshot!(readable(&browse(
        &serving(),
        contentdirectory::ALBUMS,
        contentdirectory::BrowseFlag::DirectChildren,
        10
    )));
}

#[test]
fn browse_an_artist_holding_an_album_and_a_loose_track() {
    let served = serving();
    let artist = served.library.artists()[0].id.clone();
    insta::assert_snapshot!(readable(&browse(
        &served,
        artist.as_str(),
        contentdirectory::BrowseFlag::DirectChildren,
        10
    )));
}

#[test]
fn browse_an_album_metadata() {
    let served = serving();
    let album = served.library.albums()[0].id.clone();
    insta::assert_snapshot!(readable(&browse(
        &served,
        album.as_str(),
        contentdirectory::BrowseFlag::Metadata,
        1
    )));
}

fn look_for(served: &kantele::browse::Served, criteria: &str) -> String {
    let request = contentdirectory::SearchRequest {
        container_id: ObjectId::ROOT.to_owned(),
        criteria: criteria.to_owned(),
        starting_index: 0,
        requested_count: 0,
    };
    let response = contentdirectory::search(
        served,
        &request,
        didl::To::plain("http://192.0.2.1:8200"),
        1,
    )
    .unwrap();
    contentdirectory::search_envelope(&response)
}

#[test]
fn search_for_albums_the_way_an_amplifier_asks() {
    insta::assert_snapshot!(readable(&look_for(
        &serving(),
        r#"upnp:class derivedfrom "object.container.album" and @refID exists false"#
    )));
}

#[test]
fn search_matching_nothing_is_a_document_rather_than_a_fault() {
    insta::assert_snapshot!(readable(&look_for(
        &serving(),
        r#"upnp:class derivedfrom "object.item.imageItem""#
    )));
}

#[test]
fn browse_root_children() {
    insta::assert_snapshot!(readable(&browse(
        &kantele::browse::Served::new(library(), Default::default()),
        ObjectId::ROOT,
        contentdirectory::BrowseFlag::DirectChildren,
        10
    )));
}

#[test]
fn browse_the_music_container() {
    insta::assert_snapshot!(readable(&browse(
        &kantele::browse::Served::new(library(), Default::default()),
        contentdirectory::MUSIC,
        contentdirectory::BrowseFlag::DirectChildren,
        10
    )));
}

#[test]
fn browse_root_metadata() {
    insta::assert_snapshot!(readable(&browse(
        &kantele::browse::Served::new(library(), Default::default()),
        ObjectId::ROOT,
        contentdirectory::BrowseFlag::Metadata,
        1
    )));
}

#[test]
fn browse_an_empty_library() {
    insta::assert_snapshot!(readable(&browse(
        &kantele::browse::Served::new(Library::default(), Default::default()),
        contentdirectory::MUSIC,
        contentdirectory::BrowseFlag::DirectChildren,
        10
    )));
}

#[test]
fn didl_for_one_item() {
    let didl = one_item(&track(1, "Juana Peña", 243_026, 27_296_681));
    insta::assert_snapshot!(readable(&didl));
}

#[test]
fn a_title_needing_escaping() {
    let mut awkward = track(1, "Rock & Roll <live> \"take 2\"", 1000, 10);
    awkward.album = Some("A & B".to_owned());
    let didl = one_item(&awkward);
    insta::assert_snapshot!(readable(&didl));
}

#[test]
fn several_values_for_one_tag() {
    let mut track = track(1, "Symphony No. 9", 1000, 10);
    track.album_artists = vec![
        Credit::new("Christian Thielemann"),
        Credit::new("Wiener Philharmoniker"),
    ];
    track.artists = vec![
        Credit::new("Christian Thielemann"),
        Credit::new("Beethoven"),
    ];
    let didl = one_item(&track);
    insta::assert_snapshot!(readable(&didl));
}

#[test]
fn an_item_with_a_cover() {
    let mut with_art = track(1, "Juana Peña", 243_026, 27_296_681);
    with_art.artwork = Some(kantele::index::Artwork {
        source: kantele::index::artwork::Source::File(PathBuf::from("/music/cover.jpg")),
        mime: "image/jpeg",
        dimensions: Some((600, 600)),
    });
    let didl = one_item(&with_art);
    insta::assert_snapshot!(readable(&didl));
}

/// DSD on the wire.
#[test]
fn an_item_that_is_dsd() {
    let mut dsd = track(1, "Celia", 372_192, 263_554_618);
    dsd.path = PathBuf::from("/music/03 - Celia.dsf");
    dsd.relative = "03 - Celia.dsf".to_owned();
    dsd.mime = "audio/x-dsf";
    dsd.sample_rate = Some(2_822_400);
    dsd.bit_depth = Some(1);
    dsd.bitrate_bps = Some(5_644_800);
    insta::assert_snapshot!(readable(&one_item(&dsd)));
}

#[test]
fn faults() {
    insta::assert_snapshot!(
        "no_such_object",
        readable(&contentdirectory::fault_envelope(
            &contentdirectory::Fault::NO_SUCH_OBJECT
        ))
    );
    insta::assert_snapshot!(
        "invalid_args",
        readable(&contentdirectory::fault_envelope(
            &contentdirectory::Fault::INVALID_ARGS
        ))
    );
}

#[test]
fn capability_answers() {
    insta::assert_snapshot!(
        "search_capabilities",
        readable(&contentdirectory::simple_envelope(
            "GetSearchCapabilities",
            "ContentDirectory",
            "SearchCaps",
            ""
        ))
    );
    insta::assert_snapshot!(
        "system_update_id",
        readable(&contentdirectory::simple_envelope(
            "GetSystemUpdateID",
            "ContentDirectory",
            "Id",
            "1"
        ))
    );
}

/// A tag carrying characters XML 1.0 forbids outright.
#[test]
fn a_corrupt_tag_cannot_produce_malformed_xml() {
    let mut track = track(1, "Juana Peña", 243_026, 27_296_681);
    track.title = "Cru\u{1}el\u{8} Summer\u{b}".to_owned();
    track.album = Some("Bad\u{c}Tag".to_owned());
    track.artists = vec![Credit::new("\u{1f}Binary\u{0}")];
    let library = Library::from_tracks([track]);
    let root = ObjectId::root();
    let didl = didl::children(
        &library
            .tracks()
            .iter()
            .map(|track| didl::Child::Item(track, &root))
            .collect::<Vec<_>>(),
        didl::To::plain(BASE_URL),
    );

    let mut reader = quick_xml::Reader::from_str(&didl);
    loop {
        match reader.read_event() {
            Ok(quick_xml::events::Event::Eof) => break,
            Ok(_) => {}
            Err(error) => panic!("Kantele emitted XML no parser will accept: {error}\n{didl}"),
        }
    }
    for forbidden in ['\u{0}', '\u{1}', '\u{8}', '\u{b}', '\u{c}', '\u{1f}'] {
        assert!(
            !didl.contains(forbidden),
            "{forbidden:?} reached the wire: {didl}"
        );
    }
}
