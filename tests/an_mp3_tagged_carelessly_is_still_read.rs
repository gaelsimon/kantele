//! MP3 files as taggers and download sites leave them, read the way MinimServer reads them.

use kantele::tags;

mod fixtures;
use fixtures::{Tree, id3v2, id3v2_text, mp3};

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
