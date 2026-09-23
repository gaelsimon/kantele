//! DSD, which lofty does not read at all.

use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::time::Duration;

use lofty::config::ParseOptions;
use lofty::file::{FileType, TaggedFile};
use lofty::probe::Probe;

use crate::tags::AudioProperties;

/// DSD is one bit per sample, always; DSF's own field of that name is the bit order.
const BITS_PER_SAMPLE: u8 = 1;
const LARGEST_TAG: u64 = 32 * 1024 * 1024;

/// What a DSD file says about itself.
pub struct Dsd {
    pub properties: AudioProperties,
    /// The `ID3v2` tag the container carries, when it carries one.
    pub tagged: Option<TaggedFile>,
}

/// Reads a DSD file, or nothing when the bytes are not DSD.
pub fn read(path: &Path) -> Option<Dsd> {
    read_from(&mut File::open(path).ok()?)
}

/// The same from a file already open, left at its start when the bytes are not DSD.
pub fn read_from<R: Read + Seek>(file: &mut R) -> Option<Dsd> {
    file.seek(SeekFrom::Start(0)).ok()?;
    let mut magic = [0u8; 4];
    file.read_exact(&mut magic).ok()?;
    file.seek(SeekFrom::Start(0)).ok()?;
    match &magic {
        b"DSD " => dsf(file),
        b"FRM8" => dsdiff(file),
        _ => None,
    }
}

/// `DSF`: a fixed 80-byte prologue of three chunks, then the samples.
fn dsf<R: Read + Seek>(file: &mut R) -> Option<Dsd> {
    let mut head = [0u8; 80];
    file.read_exact(&mut head).ok()?;
    if &head[0..4] != b"DSD " || &head[28..32] != b"fmt " {
        return None;
    }
    let metadata = u64::from_le_bytes(head[20..28].try_into().ok()?);
    let channels = u32::from_le_bytes(head[52..56].try_into().ok()?);
    let rate = u32::from_le_bytes(head[56..60].try_into().ok()?);
    let samples = u64::from_le_bytes(head[64..72].try_into().ok()?);
    if rate == 0 || channels == 0 {
        return None;
    }
    Some(Dsd {
        properties: properties(rate, channels, samples as f64 / f64::from(rate)),
        tagged: (metadata > 0).then(|| id3_at(file, metadata)).flatten(),
    })
}

fn dsdiff<R: Read + Seek>(file: &mut R) -> Option<Dsd> {
    let mut header = [0u8; 16];
    file.read_exact(&mut header).ok()?;
    if &header[0..4] != b"FRM8" || &header[12..16] != b"DSD " {
        return None;
    }
    let mut rate = 0u32;
    let mut channels = 0u32;
    let mut audio = 0u64;
    let mut tag = None;
    let mut at = 16u64;
    while let Some((id, size, payload)) = chunk(file, at) {
        match &id {
            b"PROP" => sound(file, payload, size, &mut rate, &mut channels),
            b"DSD " => audio = size,
            // Compressed. The header cannot say how long it plays without decoding it.
            b"DST " => audio = 0,
            b"ID3 " => tag = id3_at(file, payload),
            _ => {}
        }
        let Some(next) = after(payload, size) else {
            break;
        };
        at = next;
    }
    if rate == 0 || channels == 0 {
        return None;
    }
    let bytes_per_second = f64::from(rate) / 8.0 * f64::from(channels);
    Some(Dsd {
        properties: properties(rate, channels, audio as f64 / bytes_per_second),
        tagged: tag,
    })
}

/// The `SND ` property chunk, which is where the rate and the channel count live.
fn sound<R: Read + Seek>(
    file: &mut R,
    payload: u64,
    size: u64,
    rate: &mut u32,
    channels: &mut u32,
) {
    let mut kind = [0u8; 4];
    if read_at(file, payload, &mut kind).is_none() || &kind != b"SND " {
        return;
    }
    let end = payload.saturating_add(size);
    let mut at = payload + 4;
    while at < end {
        let Some((id, inner, body)) = chunk(file, at) else {
            return;
        };
        match &id {
            b"FS  " => {
                let mut bytes = [0u8; 4];
                if read_at(file, body, &mut bytes).is_some() {
                    *rate = u32::from_be_bytes(bytes);
                }
            }
            b"CHNL" => {
                let mut bytes = [0u8; 2];
                if read_at(file, body, &mut bytes).is_some() {
                    *channels = u32::from(u16::from_be_bytes(bytes));
                }
            }
            _ => {}
        }
        let Some(next) = after(body, inner) else {
            return;
        };
        at = next;
    }
}

/// Where the chunk after a body of `size` bytes at `payload` starts, or nothing where the size lies.
fn after(payload: u64, size: u64) -> Option<u64> {
    payload.checked_add(size)?.checked_add(size % 2)
}

/// One `IFF` chunk header at `at`: its identifier, its length, and where its body starts.
fn chunk<R: Read + Seek>(file: &mut R, at: u64) -> Option<([u8; 4], u64, u64)> {
    let mut header = [0u8; 12];
    read_at(file, at, &mut header)?;
    let id: [u8; 4] = header[0..4].try_into().ok()?;
    let size = u64::from_be_bytes(header[4..12].try_into().ok()?);
    Some((id, size, at + 12))
}

fn read_at<R: Read + Seek>(file: &mut R, at: u64, into: &mut [u8]) -> Option<()> {
    file.seek(SeekFrom::Start(at)).ok()?;
    file.read_exact(into).ok()
}

fn id3_at<R: Read + Seek>(file: &mut R, at: u64) -> Option<TaggedFile> {
    let end = file.seek(SeekFrom::End(0)).ok()?;
    if at >= end || end - at > LARGEST_TAG {
        return None;
    }
    let mut bytes = vec![0u8; (end - at) as usize];
    read_at(file, at, &mut bytes)?;
    Probe::new(std::io::Cursor::new(bytes))
        .set_file_type(FileType::Mpeg)
        .options(ParseOptions::new().read_properties(false))
        .read()
        .ok()
}

fn properties(rate: u32, channels: u32, seconds: f64) -> AudioProperties {
    AudioProperties {
        // A count no duration can hold is a header that lies, and unknown is the honest answer.
        duration: Duration::try_from_secs_f64(seconds).unwrap_or_default(),
        sample_rate: Some(rate),
        bit_depth: Some(BITS_PER_SAMPLE),
        channels: u8::try_from(channels).ok(),
        bitrate_bps: rate.checked_mul(channels),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A DSF file with no samples in it, which is all the header reader looks at.
    fn dsf_bytes(rate: u32, channels: u32, samples: u64, metadata: u64) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(b"DSD ");
        out.extend_from_slice(&28u64.to_le_bytes());
        out.extend_from_slice(&80u64.to_le_bytes());
        out.extend_from_slice(&metadata.to_le_bytes());
        out.extend_from_slice(b"fmt ");
        out.extend_from_slice(&52u64.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes()); // version
        out.extend_from_slice(&0u32.to_le_bytes()); // format id
        out.extend_from_slice(&2u32.to_le_bytes()); // channel type
        out.extend_from_slice(&channels.to_le_bytes());
        out.extend_from_slice(&rate.to_le_bytes());
        out.extend_from_slice(&1u32.to_le_bytes()); // bits per sample
        out.extend_from_slice(&samples.to_le_bytes());
        out.extend_from_slice(&4096u32.to_le_bytes()); // block size
        out.extend_from_slice(&0u32.to_le_bytes()); // reserved
        out
    }

    /// A `DSDIFF` file: the container, one `PROP`/`SND ` with the rate and the channels, and audio.
    fn dff_bytes(rate: u32, channels: u16, audio: usize) -> Vec<u8> {
        let mut sound = Vec::new();
        sound.extend_from_slice(b"SND ");
        sound.extend_from_slice(b"FS  ");
        sound.extend_from_slice(&4u64.to_be_bytes());
        sound.extend_from_slice(&rate.to_be_bytes());
        sound.extend_from_slice(b"CHNL");
        sound.extend_from_slice(&2u64.to_be_bytes());
        sound.extend_from_slice(&channels.to_be_bytes());

        let mut body = Vec::new();
        body.extend_from_slice(b"DSD "); // form type
        body.extend_from_slice(b"PROP");
        body.extend_from_slice(&(sound.len() as u64).to_be_bytes());
        body.extend_from_slice(&sound);
        body.extend_from_slice(b"DSD ");
        body.extend_from_slice(&(audio as u64).to_be_bytes());
        body.extend(std::iter::repeat_n(0u8, audio));

        let mut out = Vec::new();
        out.extend_from_slice(b"FRM8");
        out.extend_from_slice(&(body.len() as u64).to_be_bytes());
        out.extend_from_slice(&body);
        out
    }

    fn written(name: &str, bytes: &[u8]) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("kantele-dsd-{name}"));
        std::fs::write(&path, bytes).expect("writing the file");
        path
    }

    #[test]
    fn a_dsf_header_gives_the_properties_a_renderer_needs() {
        // One second of DSD64 stereo: 2,822,400 samples at 2,822,400 Hz.
        let path = written("one-second.dsf", &dsf_bytes(2_822_400, 2, 2_822_400, 0));
        let dsd = read(&path).expect("a dsf is read");
        assert_eq!(dsd.properties.duration.as_millis(), 1_000);
        assert_eq!(dsd.properties.sample_rate, Some(2_822_400));
        assert_eq!(dsd.properties.channels, Some(2));
        assert_eq!(dsd.properties.bit_depth, Some(1));
        assert_eq!(dsd.properties.bitrate_bps, Some(5_644_800));
        assert!(dsd.tagged.is_none(), "no metadata pointer means no tag");
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_dsf_stored_most_significant_bit_first_is_still_one_bit_a_sample() {
        let mut bytes = dsf_bytes(2_822_400, 2, 2_822_400, 0);
        // The field says the bit order within a byte: 1 least significant first, 8 most.
        bytes[60..64].copy_from_slice(&8u32.to_le_bytes());
        let path = written("msb-first.dsf", &bytes);
        let dsd = read(&path).expect("a dsf is read");
        assert_eq!(dsd.properties.bitrate_bps, Some(5_644_800));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_dsdiff_header_gives_the_same() {
        // A second of DSD64 stereo is 2,822,400 / 8 * 2 bytes.
        let path = written("one-second.dff", &dff_bytes(2_822_400, 2, 705_600));
        let dsd = read(&path).expect("a dff is read");
        assert_eq!(dsd.properties.duration.as_millis(), 1_000);
        assert_eq!(dsd.properties.sample_rate, Some(2_822_400));
        assert_eq!(dsd.properties.channels, Some(2));
        assert_eq!(dsd.properties.bit_depth, Some(1));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_file_that_is_not_dsd_is_not_claimed() {
        let path = written("not-dsd.bin", b"RIFFxxxxWAVEfmt ");
        assert!(read(&path).is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_truncated_dsf_is_refused_rather_than_guessed() {
        let path = written("truncated.dsf", &dsf_bytes(2_822_400, 2, 0, 0)[..40]);
        assert!(read(&path).is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_rate_of_zero_is_not_a_file_this_can_describe() {
        let path = written("no-rate.dsf", &dsf_bytes(0, 2, 100, 0));
        assert!(read(&path).is_none());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_sample_count_no_duration_can_hold_is_left_unknown_rather_than_a_panic() {
        let path = written("endless.dsf", &dsf_bytes(1, 1, u64::MAX, 0));
        let dsd = read(&path).expect("the header still reads");
        assert_eq!(dsd.properties.duration, Duration::ZERO);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_chunk_whose_size_wraps_the_offset_ends_the_read_rather_than_looping() {
        // A chunk at offset 16 whose declared size brings the next offset back to 16.
        let mut bytes = Vec::new();
        bytes.extend_from_slice(b"FRM8");
        bytes.extend_from_slice(&40u64.to_be_bytes());
        bytes.extend_from_slice(b"DSD ");
        bytes.extend_from_slice(b"JUNK");
        bytes.extend_from_slice(&(u64::MAX - 11).to_be_bytes());
        bytes.extend_from_slice(&[0u8; 16]);
        let path = written("wrapping.dff", &bytes);
        let read_from = path.clone();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || sender.send(read(&read_from).is_none()));
        assert!(
            receiver
                .recv_timeout(Duration::from_secs(10))
                .expect("the read ends"),
            "a header naming no rate is not a file this can describe"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn a_metadata_pointer_past_the_end_is_ignored() {
        let path = written(
            "bad-pointer.dsf",
            &dsf_bytes(2_822_400, 2, 2_822_400, 999_999),
        );
        let dsd = read(&path).expect("the header still reads");
        assert!(dsd.tagged.is_none());
        let _ = std::fs::remove_file(path);
    }
}
