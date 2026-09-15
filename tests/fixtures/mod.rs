//! A library on disk, and the reading of it as text, shared by the tests that need both.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use kantele::index::Library;

pub struct Tree(pub PathBuf);

impl Tree {
    pub fn new(name: &str) -> Self {
        // The pid keeps concurrent runs from removing each other's fixtures.
        let root = std::env::temp_dir().join(format!("kantele-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("creating the test root");
        Self(root)
    }

    pub fn album(&self, folder: &str, tracks: &[&str], cover: bool) {
        let folder = self.0.join(folder);
        std::fs::create_dir_all(&folder).expect("creating an album folder");
        for (at, name) in tracks.iter().enumerate() {
            std::fs::write(folder.join(name), wav(at as u8 + 1)).expect("writing a wav");
        }
        if cover {
            std::fs::write(folder.join("cover.jpg"), jpeg()).expect("writing a cover");
        }
    }

    pub fn path(&self, relative: &str) -> PathBuf {
        self.0.join(relative)
    }

    pub fn write(&self, relative: &str, sample: u8) {
        let path = self.path(relative);
        std::fs::create_dir_all(path.parent().expect("a file has a parent"))
            .expect("creating a folder");
        std::fs::write(&path, wav(sample)).expect("writing a wav");
    }

    pub fn text(&self, relative: &str, contents: &str) {
        let path = self.path(relative);
        std::fs::create_dir_all(path.parent().expect("a file has a parent"))
            .expect("creating a folder");
        std::fs::write(&path, contents).expect("writing a text file");
    }

    pub fn remove(&self, relative: &str) {
        let path = self.path(relative);
        if path.is_dir() {
            std::fs::remove_dir_all(&path).expect("removing a folder");
        } else {
            std::fs::remove_file(&path).expect("removing a file");
        }
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A wav header and 8000 bytes of one sample value.
pub fn wav(sample: u8) -> Vec<u8> {
    let samples = vec![sample; 8_000];
    let mut file = Vec::new();
    file.extend_from_slice(b"RIFF");
    file.extend_from_slice(&(36 + samples.len() as u32).to_le_bytes());
    file.extend_from_slice(b"WAVEfmt ");
    file.extend_from_slice(&16u32.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&8_000u32.to_le_bytes());
    file.extend_from_slice(&8_000u32.to_le_bytes());
    file.extend_from_slice(&1u16.to_le_bytes());
    file.extend_from_slice(&8u16.to_le_bytes());
    file.extend_from_slice(b"data");
    file.extend_from_slice(&(samples.len() as u32).to_le_bytes());
    file.extend_from_slice(&samples);
    file
}

/// A JPEG header and padding, enough for the type to be read from the bytes.
pub fn jpeg() -> Vec<u8> {
    let mut file = vec![0xFF, 0xD8, 0xFF, 0xE0];
    file.extend_from_slice(&[0x00, 0x10]);
    file.extend_from_slice(b"JFIF\0");
    file.extend_from_slice(&[0u8; 64]);
    file
}

/// A FLAC: stream marker, `STREAMINFO`, the given Vorbis comments, a `PICTURE` block if asked.
pub fn flac(comments: &[(&str, &str)], picture: bool) -> Vec<u8> {
    const RATE: u32 = 44_100;
    const CHANNELS: u32 = 2;
    const BITS: u32 = 16;
    let samples = u64::from(RATE); // one second

    let mut streaminfo = Vec::new();
    streaminfo.extend_from_slice(&4096u16.to_be_bytes()); // min block size
    streaminfo.extend_from_slice(&4096u16.to_be_bytes()); // max block size
    streaminfo.extend_from_slice(&[0, 0, 0, 0, 0, 0]); // min and max frame size, unknown
    let packed = (u64::from(RATE) << 44)
        | (u64::from(CHANNELS - 1) << 41)
        | (u64::from(BITS - 1) << 36)
        | samples;
    streaminfo.extend_from_slice(&packed.to_be_bytes());
    streaminfo.extend_from_slice(&[0u8; 16]); // md5 of the audio, unchecked

    let mut vorbis = Vec::new();
    vorbis.extend_from_slice(&(b"kantele".len() as u32).to_le_bytes());
    vorbis.extend_from_slice(b"kantele");
    vorbis.extend_from_slice(&(comments.len() as u32).to_le_bytes());
    for (name, value) in comments {
        let line = format!("{name}={value}");
        vorbis.extend_from_slice(&(line.len() as u32).to_le_bytes());
        vorbis.extend_from_slice(line.as_bytes());
    }

    let cover = jpeg();
    let mut art = Vec::new();
    art.extend_from_slice(&3u32.to_be_bytes()); // picture type 3, front cover
    art.extend_from_slice(&(b"image/jpeg".len() as u32).to_be_bytes());
    art.extend_from_slice(b"image/jpeg");
    art.extend_from_slice(&0u32.to_be_bytes()); // no description
    for value in [500u32, 500, 24, 0] {
        art.extend_from_slice(&value.to_be_bytes());
    }
    art.extend_from_slice(&(cover.len() as u32).to_be_bytes());
    art.extend_from_slice(&cover);

    let mut file = Vec::from(*b"fLaC");
    block(&mut file, 0, &streaminfo, false);
    block(&mut file, 4, &vorbis, !picture);
    if picture {
        block(&mut file, 6, &art, true);
    }
    file
}

fn block(file: &mut Vec<u8>, kind: u8, body: &[u8], last: bool) {
    file.push(if last { 0x80 | kind } else { kind });
    let length = body.len() as u32;
    file.extend_from_slice(&length.to_be_bytes()[1..]);
    file.extend_from_slice(body);
}

pub fn tagged(
    relative: &str,
    album: &str,
    artist: &str,
    genre: &str,
    track: u32,
) -> kantele::index::Scanned {
    kantele::index::Scanned {
        path: PathBuf::from("/music").join(relative),
        relative: PathBuf::from(relative),
        tags: kantele::tags::FileTags {
            title: Some(format!("{album} {track}")),
            album: Some(album.to_owned()),
            artists: vec![artist.to_owned()],
            album_artists: vec![artist.to_owned()],
            genres: vec![genre.to_owned()],
            track_number: Some(track),
            ..kantele::tags::FileTags::default()
        },
        properties: kantele::tags::AudioProperties {
            duration: std::time::Duration::from_secs(180),
            sample_rate: Some(44_100),
            bit_depth: Some(16),
            channels: Some(2),
            bitrate_bps: Some(1_411_000),
        },
        size: 27,
        artwork: None,
    }
}

pub fn shape(library: &Library) -> Vec<String> {
    let mut lines: Vec<String> = library
        .tracks()
        .iter()
        .map(|track| {
            format!(
                "track {} rule={:?} title={} relative={} album={:?} millis={} size={} \
                 rate={:?} depth={:?} mime={} art={:?}",
                track.id.as_str(),
                track.rule,
                track.title,
                track.relative,
                track.album_id.as_ref().map(|id| id.as_str().to_owned()),
                track.duration.as_millis(),
                track.size,
                track.sample_rate,
                track.bit_depth,
                track.mime,
                track.artwork.as_ref().map(|art| art.source.clone()),
            )
        })
        .collect();
    lines.extend(library.albums().iter().map(|album| {
        format!(
            "album {} rule={:?} title={} artists={:?} tracks={} art={:?}",
            album.id.as_str(),
            album.rule,
            album.title,
            album.artists,
            album.tracks.len(),
            album.artwork.as_ref().map(|art| art.source.clone()),
        )
    }));
    lines.extend(
        library
            .artists()
            .iter()
            .map(|artist| format!("artist {} name={}", artist.id.as_str(), artist.name)),
    );
    lines.extend(library.playlists().iter().map(|playlist| {
        format!(
            "playlist {} title={} relative={} tracks={}",
            playlist.id.as_str(),
            playlist.title,
            playlist.relative,
            playlist.tracks.len(),
        )
    }));
    lines.extend(library.refusals().reported().iter().flat_map(|cause| {
        let name = cause.cause.as_str();
        std::iter::once(format!("refused {name} total={}", cause.total)).chain(
            cause.shown.iter().map(move |refusal| {
                format!("refused {name} {} {:?}", refusal.subject, refusal.detail)
            }),
        )
    }));
    lines.sort();
    lines
}

pub fn walked_paths(root: &Path, walked: &kantele::index::Walked) -> Vec<String> {
    let mut paths: Vec<String> = walked
        .files
        .iter()
        .map(|found| {
            found
                .path
                .strip_prefix(root)
                .unwrap_or(&found.path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    paths.sort();
    paths
}
