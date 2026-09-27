//! MP3 files as taggers and download sites leave them, read the way MinimServer reads them.

use kantele::tags;

mod fixtures;
use fixtures::{Tree, id3v1, id3v2, id3v2_frame, id3v2_text, mp3};

#[test]
fn of_two_id3v2_tags_heading_a_file_the_first_is_the_one_read() {
    let tree = Tree::new("mp3-stacked");
    let current = id3v2(
        3,
        0,
        &[
            id3v2_text(3, b"TIT2", "My Boy Lollipop"),
            id3v2_text(3, b"TPE1", "Millie Small"),
        ],
    );
    let stale = id3v2(
        4,
        0,
        &[
            id3v2_text(4, b"TIT2", "Track03"),
            id3v2_text(4, b"TALB", "Unknown Title"),
            id3v2_text(4, b"TPE1", "Unknown Artist"),
        ],
    );
    std::fs::write(tree.path("13.mp3"), mp3(&[current, stale], None)).expect("writing it");

    let (read, properties) = tags::read(&tree.path("13.mp3")).expect("an mp3 parses");
    assert_eq!(read.title.as_deref(), Some("My Boy Lollipop"));
    assert_eq!(read.artists, ["Millie Small"]);
    assert_eq!(
        read.album, None,
        "the tag a later tagger wrote in front is the file's tag; the one behind it is left over"
    );
    assert!(
        properties.duration.as_millis() > 200,
        "the audio is still found"
    );
}

#[test]
fn a_frame_lofty_refuses_costs_that_frame_and_not_the_tag() {
    let tree = Tree::new("mp3-damaged-frame");
    let tag = id3v2(
        4,
        0,
        &[
            id3v2_text(4, b"TIT2", "Bam-Bam"),
            id3v2_text(4, b"TPE1", "Sister Nancy"),
            // What a DJ tool left on the real file: an encoding byte no version defines, no text.
            id3v2_frame(4, b"TCON", &[0xD8]),
            id3v2_text(4, b"TPE2", "DaganJah's Collections - Reggae"),
        ],
    );
    std::fs::write(tree.path("bam.mp3"), mp3(&[tag], None)).expect("writing it");

    let (read, _) = tags::read(&tree.path("bam.mp3")).expect("an mp3 parses");
    assert_eq!(read.title.as_deref(), Some("Bam-Bam"));
    assert_eq!(read.artists, ["Sister Nancy"]);
    assert_eq!(read.album_artists, ["DaganJah's Collections - Reggae"]);
    assert!(read.genres.is_empty());
}

#[test]
fn a_tag_with_nothing_left_to_read_leaves_the_id3v1_behind_it() {
    let tree = Tree::new("mp3-nothing-left");
    let tag = id3v2(3, 0, &[id3v2_frame(3, b"TCON", &[0xD8])]);
    let trailer = id3v1("Shake Up", "Basement Jaxx", "Remedy", 255);
    std::fs::write(tree.path("shake.mp3"), mp3(&[tag], Some(trailer))).expect("writing it");

    let (read, _) = tags::read(&tree.path("shake.mp3")).expect("an mp3 parses");
    assert_eq!(read.title.as_deref(), Some("Shake Up"));
    assert_eq!(read.artists, ["Basement Jaxx"]);
}

#[test]
fn the_genre_byte_an_encoder_leaves_at_blues_or_other_names_no_genre() {
    let tree = Tree::new("mp3-id3v1-genre");
    let files = [
        (
            "blues.mp3",
            id3v1("Everyday People", "Arrested Development", "", 0),
            None,
        ),
        ("other.mp3", id3v1("Glasswalk", "Manfredas", "", 12), None),
        (
            "reggae.mp3",
            id3v1("Gal A Bubble", "Konshens", "", 16),
            None,
        ),
        (
            "written.mp3",
            id3v1("Boom Boom", "John Lee Hooker", "", 0),
            Some(id3v2(3, 0, &[id3v2_text(3, b"TCON", "Blues")])),
        ),
    ];
    for (name, trailer, tag) in files {
        let file = mp3(&tag.into_iter().collect::<Vec<_>>(), Some(trailer));
        std::fs::write(tree.path(name), file).expect("writing it");
    }
    let genres = |name: &str| {
        tags::read(&tree.path(name))
            .expect("an mp3 parses")
            .0
            .genres
    };

    assert!(
        genres("blues.mp3").is_empty(),
        "genre 0 is what a zeroed tag says"
    );
    assert!(genres("other.mp3").is_empty());
    assert_eq!(
        genres("reggae.mp3"),
        ["Reggae"],
        "a genre somebody chose is kept"
    );
    assert_eq!(
        genres("written.mp3"),
        ["Blues"],
        "Blues written out is Blues"
    );
}
