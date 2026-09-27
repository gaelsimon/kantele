//! The ID3v2 tags heading a file. lofty reads every one it finds there and lets a later frame
//! replace an earlier one; the first tag is the one every other reader believes.

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};

const HEADER: u64 = 10;
const UNSYNCHRONISED: u8 = 0x80;
const EXTENDED: u8 = 0x40;
const FOOTER: u8 = 0x10;
/// A first tag larger than this is read as lofty reads it rather than copied.
const LARGEST: u64 = 64 * 1024 * 1024;

/// Where the first tag ends, and where the run of tags that follow it without a gap ends.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Heading {
    pub first: u64,
    pub all: u64,
    pub count: usize,
}

pub fn heading<R: Read + Seek>(file: &mut R) -> io::Result<Option<Heading>> {
    let length = file.seek(SeekFrom::End(0))?;
    let mut at = 0;
    let mut first = None;
    let mut count = 0;
    loop {
        file.seek(SeekFrom::Start(at))?;
        let mut header = [0u8; HEADER as usize];
        if file.read_exact(&mut header).is_err() || &header[..3] != b"ID3" {
            break;
        }
        let Some(size) = synchsafe(&header[6..10]) else {
            break;
        };
        let footer = match header[3] == 4 && header[5] & FOOTER != 0 {
            true => HEADER,
            false => 0,
        };
        let end = at + HEADER + u64::from(size) + footer;
        if end > length {
            break;
        }
        at = end;
        first.get_or_insert(end);
        count += 1;
    }
    Ok(first.map(|first| Heading {
        first,
        all: at,
        count,
    }))
}

fn synchsafe(bytes: &[u8]) -> Option<u32> {
    bytes.iter().all(|byte| byte & 0x80 == 0).then(|| {
        bytes
            .iter()
            .fold(0, |size, byte| (size << 7) | u32::from(*byte))
    })
}

/// The file as lofty should see it when tags are stacked: the first tag, then the audio.
pub fn first_only(file: &mut File, heading: Heading) -> io::Result<Option<Spliced<'_>>> {
    if heading.count < 2 || heading.first > LARGEST {
        return Ok(None);
    }
    let mut first = vec![0; heading.first as usize];
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut first)?;
    Spliced::new(file, first, heading.all).map(Some)
}

/// The first tag rebuilt from the frames `parses` accepts one at a time, then the audio. A tag
/// that cannot be walked is left out whole, so the file's other tags are still read.
pub fn salvaged(
    file: &mut File,
    heading: Heading,
    parses: impl Fn(&[u8]) -> bool,
) -> io::Result<Spliced<'_>> {
    let mut tag = vec![0; heading.first.min(LARGEST) as usize];
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut tag)?;
    let head = rebuilt(&tag, &parses).unwrap_or_default();
    Spliced::new(file, head, heading.all)
}

fn rebuilt(tag: &[u8], parses: &impl Fn(&[u8]) -> bool) -> Option<Vec<u8>> {
    let (major, written) = (*tag.get(3)?, *tag.get(5)?);
    let mut body = tag.get(HEADER as usize..)?.to_vec();
    if major == 4 && written & FOOTER != 0 {
        body.truncate(body.len().saturating_sub(HEADER as usize));
    }
    let mut flags = written & !(EXTENDED | FOOTER);
    // Before 2.4 the whole tag is unsynchronised at once, frame headers included.
    if major < 4 && written & UNSYNCHRONISED != 0 {
        body = resynchronised(&body);
        flags &= !UNSYNCHRONISED;
    }
    if written & EXTENDED != 0 {
        let skip = extended_length(major, &body)?;
        body.drain(..skip.min(body.len()));
    }
    let kept: Vec<u8> = frames(major, &body)
        .into_iter()
        .filter(|frame| parses(&tag_of(major, flags, frame)))
        .flatten()
        .copied()
        .collect();
    Some(tag_of(major, flags, &kept))
}

fn tag_of(major: u8, flags: u8, frames: &[u8]) -> Vec<u8> {
    let size = frames.len() as u32;
    let mut tag = vec![b'I', b'D', b'3', major, 0, flags];
    tag.extend((0..4).rev().map(|at| ((size >> (7 * at)) & 0x7f) as u8));
    tag.extend_from_slice(frames);
    tag
}

fn resynchronised(body: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(body.len());
    for (at, byte) in body.iter().enumerate() {
        if !(*byte == 0 && at > 0 && body[at - 1] == 0xff) {
            out.push(*byte);
        }
    }
    out
}

fn extended_length(major: u8, body: &[u8]) -> Option<usize> {
    let size = body.get(..4)?;
    match major {
        3 => Some(4 + u32::from_be_bytes(size.try_into().ok()?) as usize),
        4 => synchsafe(size).map(|size| size as usize),
        // In 2.2 the flag says the tag is compressed, which nothing defines.
        _ => None,
    }
}

/// Every frame up to the padding, or up to the first header that makes no sense.
fn frames(major: u8, body: &[u8]) -> Vec<&[u8]> {
    let (id, header) = match major {
        2 => (3, 6),
        _ => (4, 10),
    };
    let mut found = Vec::new();
    let mut at = 0;
    while at + header <= body.len() {
        if !body[at..at + id]
            .iter()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit())
        {
            break;
        }
        let raw = &body[at + id..at + id + if major == 2 { 3 } else { 4 }];
        let plain = raw
            .iter()
            .fold(0, |size, byte| (size << 8) | *byte as usize);
        let size = match major {
            4 => synchsafe(raw).map_or(plain, |size| size as usize),
            _ => plain,
        };
        let Some(end) = (at + header)
            .checked_add(size)
            .filter(|end| *end <= body.len())
        else {
            break;
        };
        found.push(&body[at..end]);
        at = end;
    }
    found
}

/// A file with its first bytes replaced: `head`, then the file from `from` to its end.
pub struct Spliced<'a> {
    head: Vec<u8>,
    file: &'a mut File,
    from: u64,
    length: u64,
    at: u64,
}

impl<'a> Spliced<'a> {
    pub fn new(file: &'a mut File, head: Vec<u8>, from: u64) -> io::Result<Self> {
        let end = file.seek(SeekFrom::End(0))?;
        let length = head.len() as u64 + end.saturating_sub(from);
        Ok(Self {
            head,
            file,
            from,
            length,
            at: 0,
        })
    }
}

impl Read for Spliced<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let head = self.head.len() as u64;
        let read = match self.at < head {
            true => {
                let start = self.at as usize;
                let count = out.len().min(self.head.len() - start);
                out[..count].copy_from_slice(&self.head[start..start + count]);
                count
            }
            false => {
                self.file
                    .seek(SeekFrom::Start(self.from + (self.at - head)))?;
                self.file.read(out)?
            }
        };
        self.at += read as u64;
        Ok(read)
    }
}

impl Seek for Spliced<'_> {
    fn seek(&mut self, to: SeekFrom) -> io::Result<u64> {
        let target = match to {
            SeekFrom::Start(offset) => Some(offset),
            SeekFrom::End(delta) => self.length.checked_add_signed(delta),
            SeekFrom::Current(delta) => self.at.checked_add_signed(delta),
        };
        self.at = target.ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "a seek before the start")
        })?;
        Ok(self.at)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn tag(size: u32) -> Vec<u8> {
        let mut tag = b"ID3\x03\x00\x00".to_vec();
        tag.extend_from_slice(&[0, 0, (size >> 7) as u8 & 0x7f, size as u8 & 0x7f]);
        tag.resize(10 + size as usize, 0);
        tag
    }

    #[test]
    fn a_run_of_tags_is_measured_to_its_end() {
        let mut bytes = tag(20);
        bytes.extend(tag(300));
        bytes.extend_from_slice(b"\xff\xfb\x90\x00 audio");
        let heading = heading(&mut Cursor::new(bytes))
            .expect("reading")
            .expect("tags");
        assert_eq!(
            heading,
            Heading {
                first: 30,
                all: 340,
                count: 2
            }
        );
    }

    #[test]
    fn a_tag_claiming_more_than_the_file_holds_ends_the_run() {
        let mut bytes = tag(20);
        bytes.extend_from_slice(&tag(300)[..40]);
        let heading = heading(&mut Cursor::new(bytes))
            .expect("reading")
            .expect("tags");
        assert_eq!(heading.count, 1);
    }

    fn frame(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut frame = id.to_vec();
        frame.extend_from_slice(&(body.len() as u32).to_be_bytes());
        frame.extend_from_slice(&[0, 0]);
        frame.extend_from_slice(body);
        frame
    }

    #[test]
    fn a_rebuilt_tag_keeps_the_frames_that_parse_and_their_order() {
        let frames = [
            frame(b"TIT2", b"\0Bam-Bam"),
            frame(b"TCON", b"\xd8"),
            frame(b"TPE1", b"\0Nancy"),
        ];
        let tag = tag_of(3, 0, &frames.concat());
        let rebuilt =
            rebuilt(&tag, &|one: &[u8]| !one.windows(4).any(|w| w == b"TCON")).expect("a tag");
        assert_eq!(
            rebuilt,
            tag_of(3, 0, &[frames[0].clone(), frames[2].clone()].concat())
        );
    }

    #[test]
    fn an_unsynchronised_tag_before_2_4_is_rebuilt_in_the_clear() {
        let clear = frame(b"TIT2", b"\0\xff\xe0");
        let mut unsynchronised = clear.clone();
        unsynchronised.splice(clear.len() - 1..clear.len() - 1, [0]);
        let tag = tag_of(3, UNSYNCHRONISED, &unsynchronised);
        assert_eq!(rebuilt(&tag, &|_: &[u8]| true), Some(tag_of(3, 0, &clear)));
    }

    #[test]
    fn a_walk_stops_at_the_padding() {
        let mut body = frame(b"TIT2", b"\0Bam-Bam");
        body.resize(body.len() + 64, 0);
        assert_eq!(frames(3, &body).len(), 1);
    }

    #[test]
    fn a_file_without_a_tag_has_no_heading() {
        let bytes = b"\xff\xfb\x90\x00 audio".to_vec();
        assert_eq!(heading(&mut Cursor::new(bytes)).expect("reading"), None);
    }
}
