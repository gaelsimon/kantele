//! Reading the tags and properties of one file.

use kantele::index::{Library, ScanOptions};
use kantele::tags;

mod fixtures;
use fixtures::Tree;

#[test]
fn the_properties_a_renderer_needs_come_off_the_file() {
    let tree = Tree::new("tags-properties");
    tree.write("a/01.wav", 1);
    let (read, properties) = tags::read(&tree.path("a/01.wav")).expect("a wav parses");

    assert_eq!(properties.sample_rate, Some(8_000));
    assert_eq!(properties.bit_depth, Some(8));
    assert_eq!(properties.channels, Some(1));
    assert_eq!(
        properties.duration.as_millis(),
        1_000,
        "an inaccurate duration is one of the four ways to break gapless playback"
    );
    assert!(
        read.title.is_none() && read.artists.is_empty(),
        "an untagged file says nothing, and inventing a title from the path is not reading"
    );
}

#[test]
fn an_extension_that_lies_costs_nothing() {
    let tree = Tree::new("tags-wrong-extension");
    std::fs::write(tree.path("wav-called-mp3.mp3"), fixtures::wav(1)).expect("writing it");
    let (_, properties) = tags::read(&tree.path("wav-called-mp3.mp3"))
        .expect("a file lofty refuses by name is read again by its bytes");
    assert_eq!(
        properties.duration.as_millis(),
        1_000,
        "one file on the real library is named .wav and its bytes say AIFF; it was being dropped"
    );
    assert_eq!(properties.sample_rate, Some(8_000));
}

#[test]
fn a_file_that_is_not_audio_at_all_is_an_error_that_names_the_path() {
    let tree = Tree::new("tags-garbage");
    std::fs::write(tree.path("notes.txt"), b"not audio, not even close").expect("writing it");
    let refused = tags::read(&tree.path("notes.txt")).expect_err("nothing to read");
    assert!(
        refused.to_string().contains("notes.txt"),
        "the log has to say which file: {refused}"
    );
}

#[test]
fn a_file_nothing_can_be_read_from_is_skipped_and_the_rest_of_the_folder_is_not() {
    let tree = Tree::new("tags-skipped");
    tree.write("a/01.wav", 1);
    std::fs::write(tree.path("a/02.wav"), b"RIFF....WAVEnonsense").expect("writing it");

    let library = Library::scan_with(&tree.0, &ScanOptions::default()).expect("scanning");
    assert_eq!(library.len(), 1, "the good file is still served");
    assert_eq!(library.tracks()[0].relative, "a/01.wav");
}

/// A `DSF` header with no samples after it.
fn dsf(rate: u32, channels: u32, samples: u64) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(b"DSD ");
    out.extend_from_slice(&28u64.to_le_bytes());
    out.extend_from_slice(&80u64.to_le_bytes());
    out.extend_from_slice(&0u64.to_le_bytes()); // no metadata pointer
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&52u64.to_le_bytes());
    for value in [1u32, 0, 2, channels, rate, 1] {
        out.extend_from_slice(&value.to_le_bytes());
    }
    out.extend_from_slice(&samples.to_le_bytes());
    out.extend_from_slice(&4096u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out
}

#[test]
fn a_dsd_file_reaches_the_index_although_lofty_refuses_the_container() {
    let tree = Tree::new("tags-dsd");
    std::fs::create_dir_all(tree.path("a")).expect("creating the folder");
    std::fs::write(tree.path("a/01.dsf"), dsf(2_822_400, 2, 2_822_400)).expect("writing it");
    tree.write("a/02.wav", 1);

    let library = Library::scan_with(&tree.0, &ScanOptions::default()).expect("scanning");
    assert_eq!(library.len(), 2, "the dsf is served beside the wav");

    let (_, properties) = tags::read(&tree.path("a/01.dsf")).expect("a dsf is read");
    assert_eq!(properties.duration.as_millis(), 1_000);
    assert_eq!(properties.sample_rate, Some(2_822_400));
    assert_eq!(properties.bit_depth, Some(1), "DSD is one bit per sample");
}

#[test]
fn a_title_the_tags_do_not_carry_falls_back_to_the_file_name_for_display_only() {
    let tree = Tree::new("tags-fallback");
    tree.write("a/01 Juana Peña.wav", 1);
    let library = Library::scan_with(&tree.0, &ScanOptions::default()).expect("scanning");
    let track = &library.tracks()[0];
    assert_eq!(track.title, "01 Juana Peña");
    assert_eq!(
        track.rule,
        kantele::index::Rule::Path,
        "a name derived from the path is a path, and identity never sees it"
    );
}

#[test]
fn the_tags_a_tagger_writes_are_the_tags_that_reach_the_index() {
    const RELEASE: &str = "0f5c1a2b-3d4e-4f50-8a61-7b2c3d4e5f60";
    const RELEASE_TRACK: &str = "1a2b3c4d-5e6f-4708-9a1b-2c3d4e5f6071";
    const RECORDING: &str = "2b3c4d5e-6f70-4819-aa2b-3c4d5e6f7082";
    const ARTIST_ONE: &str = "3c4d5e6f-7081-492a-bb3c-4d5e6f708193";
    const ARTIST_TWO: &str = "4d5e6f70-8192-4a3b-8c4d-5e6f708192a4";

    let tree = Tree::new("tags-flac");
    std::fs::create_dir_all(tree.path("a")).expect("creating the folder");
    std::fs::write(
        tree.path("a/01.flac"),
        fixtures::flac(
            &[
                ("TITLE", "Juana Peña"),
                ("ALBUM", "!Dundunbanza!"),
                ("ARTIST", "Sierra Maestra & Juan de Marcos"),
                ("ARTISTS", "Sierra Maestra"),
                ("ARTISTS", "Juan de Marcos"),
                ("ARTISTSORT", "Sierra Maestra, La"),
                ("ARTISTSORT", "Marcos, Juan de"),
                ("ALBUMARTIST", "Sierra Maestra"),
                ("ALBUMARTISTSORT", "Sierra Maestra, La"),
                ("COMPOSER", "Arsenio Rodríguez"),
                ("COMPOSERSORT", "Rodríguez, Arsenio"),
                ("GENRE", "Latin"),
                ("GENRE", "Son"),
                ("GENRE", "Latin"),
                ("DATE", "1994-05-17"),
                ("TRACKNUMBER", "1/12"),
                ("DISCNUMBER", "1"),
                ("DISCTOTAL", "2"),
                ("COMPILATION", "1"),
                ("MUSICBRAINZ_ALBUMID", RELEASE),
                ("MUSICBRAINZ_RELEASETRACKID", RELEASE_TRACK),
                ("MUSICBRAINZ_TRACKID", RECORDING),
                ("MUSICBRAINZ_ARTISTID", ARTIST_ONE),
                ("MUSICBRAINZ_ARTISTID", ARTIST_TWO),
            ],
            true,
        ),
    )
    .expect("writing the flac");

    let (tags, properties) = tags::read(&tree.path("a/01.flac")).expect("a flac parses");

    assert_eq!(properties.sample_rate, Some(44_100));
    assert_eq!(properties.bit_depth, Some(16));
    assert_eq!(properties.channels, Some(2));
    assert_eq!(properties.duration.as_millis(), 1_000);

    assert_eq!(tags.title.as_deref(), Some("Juana Peña"));
    assert_eq!(tags.album.as_deref(), Some("!Dundunbanza!"));
    assert_eq!(
        tags.artists,
        ["Sierra Maestra", "Juan de Marcos"],
        "the tag the tagger already split is the one that is read"
    );
    assert_eq!(
        tags.artist_sorts,
        ["Sierra Maestra, La", "Marcos, Juan de"],
        "a companion sort tag decides the order and is read positionally"
    );
    assert_eq!(tags.album_artists, ["Sierra Maestra"]);
    assert_eq!(tags.album_artist_sorts, ["Sierra Maestra, La"]);
    assert_eq!(tags.composers, ["Arsenio Rodríguez"]);
    // lofty has no key for this one, so it is read from the raw comment block.
    assert_eq!(tags.composer_sorts, ["Rodríguez, Arsenio"]);
    assert_eq!(
        tags.genres,
        ["Latin", "Son"],
        "an exact repeat of one value is read once"
    );
    assert_eq!(tags.date.as_deref(), Some("1994-05-17"));
    assert_eq!(tags.track_number, Some(1), "`1/12` is track one");
    assert_eq!(tags.track_total, Some(12));
    assert_eq!(tags.disc_number, Some(1));
    assert_eq!(tags.disc_total, Some(2));
    assert!(tags.compilation);

    // `MUSICBRAINZ_TRACKID` is the recording, `MUSICBRAINZ_RELEASETRACKID` the track.
    assert_eq!(
        tags.musicbrainz_release_track_id.as_deref(),
        Some(RELEASE_TRACK)
    );
    assert_eq!(tags.musicbrainz_recording_id.as_deref(), Some(RECORDING));
    assert_eq!(tags.musicbrainz_release_id.as_deref(), Some(RELEASE));
    assert_eq!(tags.musicbrainz_artist_ids, [ARTIST_ONE, ARTIST_TWO]);
}

#[test]
fn an_identifier_written_in_upper_case_is_held_in_one_spelling() {
    const RELEASE: &str = "0F5C1A2B-3D4E-4F50-8A61-7B2C3D4E5F60";

    let tree = Tree::new("tags-flac-case");
    std::fs::create_dir_all(tree.path("a")).expect("creating the folder");
    std::fs::write(
        tree.path("a/01.flac"),
        fixtures::flac(&[("TITLE", "One"), ("MUSICBRAINZ_ALBUMID", RELEASE)], false),
    )
    .expect("writing the flac");

    let (tags, _) = tags::read(&tree.path("a/01.flac")).expect("a flac parses");
    assert_eq!(
        tags.musicbrainz_release_id.as_deref(),
        Some(RELEASE.to_ascii_lowercase().as_str())
    );
}

#[test]
fn a_picture_inside_the_file_reaches_the_index_and_is_served_from_it() {
    let tree = Tree::new("tags-flac-picture");
    std::fs::create_dir_all(tree.path("a")).expect("creating the folder");
    std::fs::write(
        tree.path("a/01.flac"),
        fixtures::flac(&[("TITLE", "One"), ("ALBUM", "A Record")], true),
    )
    .expect("writing the flac");

    let library = Library::scan_with(&tree.0, &ScanOptions::default()).expect("scanning");
    let shown = library.tracks()[0]
        .artwork
        .as_ref()
        .expect("the picture the file carries");
    assert_eq!(shown.mime, "image/jpeg");
    assert_eq!(
        shown.source,
        kantele::index::artwork::Source::Embedded {
            path: tree.path("a/01.flac"),
            index: 0,
        }
    );
    let bytes = kantele::index::artwork::read(&shown.source).expect("the bytes are readable");
    assert_eq!(bytes, fixtures::jpeg(), "and they are the picture written");
}
