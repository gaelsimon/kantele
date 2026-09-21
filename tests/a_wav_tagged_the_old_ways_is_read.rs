//! WAV carries its tags in three incompatible places. A library migrated from another server
//! brings all three, and a file whose tags are not read is a file in [untagged].

use kantele::tags;

mod fixtures;
use fixtures::Tree;

/// A RIFF chunk: identifier, length, payload, padded to an even length.
fn chunk(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(id);
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        out.push(0);
    }
    out
}

/// A WAVE holding one second of silence and whatever extra chunks are given, in order.
fn wave(extra: &[Vec<u8>]) -> Vec<u8> {
    let mut fmt = Vec::new();
    fmt.extend_from_slice(&1u16.to_le_bytes()); // PCM
    fmt.extend_from_slice(&1u16.to_le_bytes()); // one channel
    fmt.extend_from_slice(&8_000u32.to_le_bytes());
    fmt.extend_from_slice(&8_000u32.to_le_bytes());
    fmt.extend_from_slice(&1u16.to_le_bytes());
    fmt.extend_from_slice(&8u16.to_le_bytes());

    let mut body = Vec::new();
    body.extend_from_slice(b"WAVE");
    body.extend_from_slice(&chunk(b"fmt ", &fmt));
    for one in extra {
        body.extend_from_slice(one);
    }
    body.extend_from_slice(&chunk(b"data", &vec![128u8; 8_000]));

    let mut file = Vec::new();
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&(body.len() as u32).to_le_bytes());
    file.extend_from_slice(&body);
    file
}

/// A `LIST`/`INFO` chunk, the tagging Windows wrote and dBpoweramp still writes.
fn list_info(fields: &[(&[u8; 4], &str)]) -> Vec<u8> {
    let mut payload = Vec::new();
    payload.extend_from_slice(b"INFO");
    for (id, value) in fields {
        let mut text = value.as_bytes().to_vec();
        text.push(0);
        payload.extend_from_slice(&chunk(id, &text));
    }
    chunk(b"LIST", &payload)
}

/// The 128 bytes an ID3v1.1 tag occupies at the very end of a file, outside the RIFF length.
fn id3v1(title: &str, artist: &str, album: &str, year: &str, track: u8) -> Vec<u8> {
    fn padded(value: &str, width: usize) -> Vec<u8> {
        let mut field = value.as_bytes().to_vec();
        field.truncate(width);
        field.resize(width, 0);
        field
    }
    let mut tag = b"TAG".to_vec();
    tag.extend_from_slice(&padded(title, 30));
    tag.extend_from_slice(&padded(artist, 30));
    tag.extend_from_slice(&padded(album, 30));
    tag.extend_from_slice(&padded(year, 4));
    let mut comment = padded("", 30);
    comment[29] = track; // v1.1 puts the track number in the last byte of the comment
    tag.extend_from_slice(&comment);
    tag.push(255); // no genre
    tag
}

/// An ID3v2.3 tag carrying one text frame per field, inside an `id3 ` chunk.
fn id3v2_chunk(frames: &[(&[u8; 4], &str)]) -> Vec<u8> {
    let mut body = Vec::new();
    for (id, value) in frames {
        let mut content = vec![0u8]; // ISO-8859-1
        content.extend_from_slice(value.as_bytes());
        body.extend_from_slice(*id);
        body.extend_from_slice(&(content.len() as u32).to_be_bytes());
        body.extend_from_slice(&[0, 0]);
        body.extend_from_slice(&content);
    }
    let mut tag = b"ID3".to_vec();
    tag.extend_from_slice(&[3, 0, 0]);
    let size = body.len() as u32;
    tag.extend_from_slice(&[
        ((size >> 21) & 0x7F) as u8,
        ((size >> 14) & 0x7F) as u8,
        ((size >> 7) & 0x7F) as u8,
        (size & 0x7F) as u8,
    ]);
    tag.extend_from_slice(&body);
    chunk(b"id3 ", &tag)
}

#[test]
fn a_wav_tagged_in_list_info_is_not_untagged() {
    let tree = Tree::new("wav-list-info");
    let file = wave(&[list_info(&[
        (b"INAM", "The Pines"),
        (b"IART", "Phantom Limb"),
        (b"IPRD", "Shake Me"),
        (b"ICRD", "2012"),
        (b"IGNR", "Folk"),
        (b"ITRK", "3"),
    ])]);
    std::fs::write(tree.path("list-info.wav"), &file).expect("writing it");

    let (read, properties) = tags::read(&tree.path("list-info.wav")).expect("a wav parses");
    assert_eq!(properties.sample_rate, Some(8_000));
    assert_eq!(read.title.as_deref(), Some("The Pines"));
    assert_eq!(read.artists, ["Phantom Limb"]);
    assert_eq!(read.album.as_deref(), Some("Shake Me"));
    assert_eq!(read.date.as_deref(), Some("2012"));
    assert_eq!(read.genres, ["Folk"]);
    assert_eq!(read.track_number, Some(3));
}

#[test]
#[ignore = "lofty reads RIFF INFO and an id3 chunk in a WAV, never a trailing ID3v1 block"]
fn a_wav_tagged_in_id3v1_is_not_untagged() {
    let tree = Tree::new("wav-id3v1");
    let mut file = wave(&[]);
    file.extend_from_slice(&id3v1("The Pines", "Phantom Limb", "Shake Me", "2012", 3));
    std::fs::write(tree.path("id3v1.wav"), &file).expect("writing it");

    let (read, properties) = tags::read(&tree.path("id3v1.wav")).expect("a wav parses");
    assert_eq!(properties.sample_rate, Some(8_000));
    assert_eq!(read.title.as_deref(), Some("The Pines"));
    assert_eq!(read.artists, ["Phantom Limb"]);
    assert_eq!(read.album.as_deref(), Some("Shake Me"));
    assert_eq!(read.date.as_deref(), Some("2012"));
    assert_eq!(read.track_number, Some(3));
}

#[test]
fn a_wav_tagged_in_an_id3_chunk_is_not_untagged() {
    let tree = Tree::new("wav-id3v2");
    let file = wave(&[id3v2_chunk(&[
        (b"TIT2", "The Pines"),
        (b"TPE1", "Phantom Limb"),
        (b"TALB", "Shake Me"),
        (b"TRCK", "3"),
    ])]);
    std::fs::write(tree.path("id3v2.wav"), &file).expect("writing it");

    let (read, _) = tags::read(&tree.path("id3v2.wav")).expect("a wav parses");
    assert_eq!(read.title.as_deref(), Some("The Pines"));
    assert_eq!(read.artists, ["Phantom Limb"]);
    assert_eq!(read.album.as_deref(), Some("Shake Me"));
    assert_eq!(read.track_number, Some(3));
}

#[test]
fn a_wav_tagged_twice_keeps_what_only_one_container_holds() {
    let tree = Tree::new("wav-twice");
    // Two taggers over the years: the id3 chunk names the track, the older LIST/INFO alone still
    // holds the album and the genre. Where both speak, the id3 chunk is the one believed.
    let file = wave(&[
        id3v2_chunk(&[(b"TIT2", "The Pines"), (b"TPE1", "Phantom Limb")]),
        list_info(&[
            (b"INAM", "the pines (old rip)"),
            (b"IPRD", "Shake Me"),
            (b"IGNR", "Folk"),
        ]),
    ]);
    std::fs::write(tree.path("twice.wav"), &file).expect("writing it");

    let (read, _) = tags::read(&tree.path("twice.wav")).expect("a wav parses");
    assert_eq!(read.title.as_deref(), Some("The Pines"));
    assert_eq!(read.artists, ["Phantom Limb"]);
    assert_eq!(read.album.as_deref(), Some("Shake Me"));
    assert_eq!(read.genres, ["Folk"]);
}
