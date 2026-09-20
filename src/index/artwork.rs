//! Choosing and reading a cover.

use std::path::{Path, PathBuf};

/// In order of preference.
const FOLDER_IMAGES: &[&str] = &[
    "cover.jpg",
    "cover.jpeg",
    "cover.png",
    "folder.jpg",
    "folder.jpeg",
    "folder.png",
    "front.jpg",
    "front.jpeg",
    "front.png",
    "album.jpg",
    "album.jpeg",
    "album.png",
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    File(PathBuf),
    Embedded { path: PathBuf, index: usize },
}

/// Which one wins where a folder holds an image and the file carries a picture of its own.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Prefer {
    #[default]
    Folder,
    Embedded,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Artwork {
    pub source: Source,
    pub mime: &'static str,
    pub dimensions: Option<(u32, u32)>,
}

impl Artwork {
    pub fn dlna_profile(&self) -> Option<&'static str> {
        let (width, height) = self.dimensions?;
        let fits = |box_width, box_height| width <= box_width && height <= box_height;
        if self.mime == "image/png" {
            return Some(if fits(160, 160) { "PNG_TN" } else { "PNG_LRG" });
        }
        Some(match () {
            _ if fits(160, 160) => "JPEG_TN",
            _ if fits(640, 480) => "JPEG_SM",
            _ if fits(1024, 768) => "JPEG_MED",
            _ => "JPEG_LRG",
        })
    }
}

const SAYS_FRONT: &[&str] = &[
    "front", "cover", "folder", "capa", "caratula", "portada", "sleeve",
];

const SAYS_NOT_FRONT: &[&str] = &[
    "back", "inlay", "inside", "tray", "booklet", "label", "disc", "cd", "obi", "spine", "matrix",
];

pub fn preferred_cover(entries: &[PathBuf]) -> Option<PathBuf> {
    for wanted in FOLDER_IMAGES {
        for path in entries {
            let matches = path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.eq_ignore_ascii_case(wanted));
            if matches {
                return Some(path.clone());
            }
        }
    }
    let named_front = entries.iter().find(|path| says_front(path));
    named_front
        .or_else(|| entries.first().filter(|_| entries.len() == 1))
        .cloned()
}

/// Whole words: `disc` is inside `disco`.
fn says_front(path: &Path) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let lowered = name.to_ascii_lowercase();
    let words: Vec<&str> = lowered
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect();
    let says = |list: &[&str]| words.iter().any(|word| list.contains(word));
    says(SAYS_FRONT) && !says(SAYS_NOT_FRONT)
}

/// Enough for a PNG header, and for the JPEG frame marker behind any run of `APPn` segments one
/// of which carries a thumbnail, which the format caps at 64 KB.
const HEADER: usize = 128 * 1024;

pub fn describe(path: &Path) -> Option<Artwork> {
    let head = head_of(path, HEADER).ok()?;
    let mime = image_mime(&head)?;
    let dimensions = match dimensions(&head) {
        Some(found) => Some(found),
        // Only where the header did not fit in the prefix: covers run to megabytes, and a cold
        // walk reads one per folder off a disk that turns.
        None if head.len() == HEADER => dimensions(&std::fs::read(path).ok()?),
        None => None,
    };
    Some(Artwork {
        source: Source::File(path.to_owned()),
        mime,
        dimensions,
    })
}

fn head_of(path: &Path, most: usize) -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let file = std::fs::File::open(path)?;
    let mut head = Vec::new();
    file.take(most as u64).read_to_end(&mut head)?;
    Ok(head)
}

pub fn image_mime_named(name: &str) -> Option<&'static str> {
    ["image/jpeg", "image/png"]
        .into_iter()
        .find(|known| *known == name)
}

/// DSD has its own reader; lofty refuses the container. A file whose extension lies is read by
/// its bytes, as the tag reader reads it: otherwise its cover is the one picture nothing serves.
fn tagged_file(path: &Path) -> Option<lofty::file::TaggedFile> {
    use lofty::probe::Probe;
    if let Some(dsd) = crate::tags::dsd::read(path) {
        return dsd.tagged;
    }
    let probe = Probe::open(path).ok()?;
    match probe.read() {
        Ok(tagged) => Some(tagged),
        Err(_) => Probe::open(path).ok()?.guess_file_type().ok()?.read().ok(),
    }
}

pub fn embedded(track: &Path) -> Option<Artwork> {
    use lofty::file::TaggedFileExt;

    let tagged = tagged_file(track)?;
    let tag = tagged.primary_tag().or_else(|| tagged.first_tag())?;
    of_picture(track, tag.pictures().first()?.data())
}

/// A picture the tag reader already had in hand, described without opening the file again.
pub fn of_picture(track: &Path, bytes: &[u8]) -> Option<Artwork> {
    Some(Artwork {
        source: Source::Embedded {
            path: track.to_owned(),
            index: 0,
        },
        mime: image_mime(bytes)?,
        dimensions: dimensions(bytes),
    })
}

pub fn read(source: &Source) -> std::io::Result<Vec<u8>> {
    match source {
        Source::File(path) => std::fs::read(path),
        Source::Embedded { path, index } => {
            use lofty::file::TaggedFileExt;
            let tagged = tagged_file(path)
                .ok_or_else(|| std::io::Error::other("no tag to read a picture out of"))?;
            let tag = tagged
                .primary_tag()
                .or_else(|| tagged.first_tag())
                .ok_or_else(|| std::io::Error::other("no tag"))?;
            let picture = tag
                .pictures()
                .get(*index)
                .ok_or_else(|| std::io::Error::other("no such picture"))?;
            Ok(picture.data().to_vec())
        }
    }
}

pub fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else {
        None
    }
}

pub fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        let width = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
        let height = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
        return Some((width, height));
    }
    if !bytes.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut i = 2;
    while i + 9 < bytes.len() {
        if bytes[i] != 0xFF {
            i += 1;
            continue;
        }
        let marker = bytes[i + 1];
        let length = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        let is_frame = (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC);
        if is_frame {
            let height = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]) as u32;
            let width = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]) as u32;
            return Some((width, height));
        }
        i += 2 + length;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_files_type_is_decided_by_its_bytes_not_its_name() {
        assert_eq!(image_mime(&[0xFF, 0xD8, 0xFF, 0xE0]), Some("image/jpeg"));
        assert_eq!(image_mime(b"\x89PNG\r\n\x1a\n"), Some("image/png"));
        assert_eq!(image_mime(b"RIFF____WEBPVP8 "), None);
        assert_eq!(image_mime(b""), None);
    }

    #[test]
    fn png_dimensions_come_from_the_header() {
        let mut png = Vec::from(b"\x89PNG\r\n\x1a\n");
        png.extend_from_slice(&[0, 0, 0, 13]); // IHDR length
        png.extend_from_slice(b"IHDR");
        png.extend_from_slice(&500_u32.to_be_bytes());
        png.extend_from_slice(&300_u32.to_be_bytes());
        assert_eq!(dimensions(&png), Some((500, 300)));
    }

    #[test]
    fn jpeg_dimensions_are_found_by_walking_the_segments() {
        let mut jpeg = vec![0xFF, 0xD8];
        jpeg.extend_from_slice(&[0xFF, 0xE0, 0x00, 0x04, 0x00, 0x00]);
        jpeg.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        jpeg.extend_from_slice(&600_u16.to_be_bytes()); // height
        jpeg.extend_from_slice(&800_u16.to_be_bytes()); // width
        jpeg.extend_from_slice(&[0; 8]);
        assert_eq!(dimensions(&jpeg), Some((800, 600)));
    }

    /// A JPEG whose frame marker sits at `at`, padded out to `size` with a segment nothing reads.
    fn jpeg_with_frame_at(at: usize, size: usize) -> Vec<u8> {
        let mut jpeg = vec![0xFF, 0xD8];
        let padding = at.saturating_sub(jpeg.len() + 4);
        jpeg.extend_from_slice(&[0xFF, 0xE0]);
        jpeg.extend_from_slice(&((padding + 2) as u16).to_be_bytes());
        jpeg.extend(std::iter::repeat_n(0u8, padding));
        jpeg.extend_from_slice(&[0xFF, 0xC0, 0x00, 0x11, 0x08]);
        jpeg.extend_from_slice(&600_u16.to_be_bytes());
        jpeg.extend_from_slice(&800_u16.to_be_bytes());
        // The scanner needs the frame to sit inside the bytes, not to end them.
        jpeg.extend_from_slice(&[0; 8]);
        jpeg.resize(size.max(jpeg.len()), 0);
        jpeg
    }

    fn written(name: &str, bytes: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!("kantele-art-{}-{name}", std::process::id()));
        std::fs::write(&path, bytes).expect("writing the image");
        path
    }

    #[test]
    fn a_cover_is_measured_without_reading_all_of_it() {
        let path = written("small-header.jpg", &jpeg_with_frame_at(64, 4 * HEADER));
        let art = describe(&path).expect("a jpeg");
        assert_eq!(art.mime, "image/jpeg");
        assert_eq!(art.dimensions, Some((800, 600)));
        assert_eq!(
            head_of(&path, HEADER).expect("the head").len(),
            HEADER,
            "and the read stops at the prefix however long the file is"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_cover_whose_header_runs_past_the_prefix_is_still_measured() {
        let path = written("late-header.jpg", &jpeg_with_frame_at(HEADER + 4_096, 0));
        let art = describe(&path).expect("a jpeg");
        assert_eq!(
            art.dimensions,
            Some((800, 600)),
            "a prefix that missed the frame falls back to the whole file rather than saying \
             nothing, which would cost the image its DLNA profile"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_unmeasured_image_claims_no_profile() {
        let art = Artwork {
            source: Source::File(PathBuf::from("/x/cover.jpg")),
            mime: "image/jpeg",
            dimensions: None,
        };
        assert_eq!(art.dlna_profile(), None);
    }

    #[test]
    fn the_profile_follows_the_pixels() {
        let art = |w, h, mime| Artwork {
            source: Source::File(PathBuf::from("/x/cover.jpg")),
            mime,
            dimensions: Some((w, h)),
        };
        assert_eq!(art(120, 120, "image/jpeg").dlna_profile(), Some("JPEG_TN"));
        assert_eq!(art(640, 480, "image/jpeg").dlna_profile(), Some("JPEG_SM"));
        assert_eq!(art(500, 500, "image/jpeg").dlna_profile(), Some("JPEG_MED"));
        assert_eq!(
            art(1024, 768, "image/jpeg").dlna_profile(),
            Some("JPEG_MED")
        );
        assert_eq!(
            art(1000, 800, "image/jpeg").dlna_profile(),
            Some("JPEG_LRG")
        );
        assert_eq!(
            art(3000, 3000, "image/jpeg").dlna_profile(),
            Some("JPEG_LRG")
        );
        assert_eq!(art(1200, 1200, "image/png").dlna_profile(), Some("PNG_LRG"));
    }

    #[test]
    fn the_preferred_names_are_ordered_and_have_no_duplicates() {
        let mut seen = FOLDER_IMAGES.to_vec();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), FOLDER_IMAGES.len());
        assert_eq!(FOLDER_IMAGES[0], "cover.jpg");
    }

    fn cover_of(names: &[&str]) -> Option<String> {
        let paths: Vec<PathBuf> = names
            .iter()
            .map(|name| PathBuf::from("/x").join(name))
            .collect();
        preferred_cover(&paths).map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
    }

    #[test]
    fn a_preferred_name_wins_over_everything_else() {
        assert_eq!(
            cover_of(&["scob066_a_1500.jpg", "cover.jpg", "front.jpg"]),
            Some("cover.jpg".to_owned())
        );
    }

    #[test]
    fn a_name_that_says_front_is_taken_when_no_preferred_name_is_there() {
        assert_eq!(
            cover_of(&[
                "the conga kings.back .jpg",
                "the conga kings.front.jpg",
                "the conga kings.inlay1.jpg",
            ]),
            Some("the conga kings.front.jpg".to_owned())
        );
    }

    #[test]
    fn a_front_that_also_says_back_is_not_a_front() {
        assert_eq!(
            cover_of(&["A Dwarf In The Hat (Front & Inside Cover).jpg"]),
            Some("A Dwarf In The Hat (Front & Inside Cover).jpg".to_owned()),
            "the only image in the folder is taken whatever it is called"
        );
        assert_eq!(
            cover_of(&[
                "A Dwarf In The Hat (Front & Inside Cover).jpg",
                "A Dwarf In The Hat (Cd).jpg",
            ]),
            None,
            "and with a second image there, neither name is trusted"
        );
    }

    #[test]
    fn a_word_inside_another_word_does_not_count() {
        assert_eq!(
            cover_of(&["disco edits front.jpg", "disco edits b.jpg"]),
            Some("disco edits front.jpg".to_owned()),
            "disc is inside disco and this library is full of disco"
        );
    }

    #[test]
    fn the_only_image_in_a_folder_is_its_cover() {
        assert_eq!(
            cover_of(&["R-1299277-1207594196.jpeg"]),
            Some("R-1299277-1207594196.jpeg".to_owned())
        );
    }

    #[test]
    fn two_images_named_nothing_in_particular_settle_nothing() {
        assert_eq!(
            cover_of(&["CS339628-01A-BIG.jpg", "CS339628-01B-BIG.jpg"]),
            None
        );
    }

    #[test]
    fn a_folder_with_no_image_has_no_cover() {
        assert_eq!(cover_of(&[]), None);
    }
}
