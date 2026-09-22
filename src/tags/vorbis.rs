//! The Vorbis comments lofty's generic tag drops.

use std::io::{Read, Seek, SeekFrom};

/// The metadata block that holds the comments.
const VORBIS_COMMENT: u8 = 4;

const LARGEST: u32 = 4 * 1024 * 1024;

/// The bytes of the comment block, or nothing where this is not a `FLAC` that carries one.
pub fn block<R: Read + Seek>(file: &mut R) -> Option<Vec<u8>> {
    file.seek(SeekFrom::Start(0)).ok()?;
    let mut marker = [0u8; 4];
    file.read_exact(&mut marker).ok()?;
    if &marker != b"fLaC" {
        return None;
    }
    loop {
        let mut header = [0u8; 4];
        file.read_exact(&mut header).ok()?;
        let last = header[0] & 0x80 != 0;
        let kind = header[0] & 0x7f;
        let length = u32::from_be_bytes([0, header[1], header[2], header[3]]);
        if kind == VORBIS_COMMENT {
            if length > LARGEST {
                return None;
            }
            let mut block = vec![0u8; length as usize];
            file.read_exact(&mut block).ok()?;
            return Some(block);
        }
        if last {
            return None;
        }
        file.seek(SeekFrom::Current(i64::from(length))).ok()?;
    }
}

/// The values of one comment, matched without regard to case as the format requires.
pub(crate) fn values(block: &[u8], name: &str) -> Vec<String> {
    let Some(vendor) = length_at(block, 0) else {
        return Vec::new();
    };
    let mut at = 4 + vendor as usize;
    let Some(count) = length_at(block, at) else {
        return Vec::new();
    };
    at += 4;

    let mut found = Vec::new();
    for _ in 0..count {
        let Some(length) = length_at(block, at) else {
            break;
        };
        at += 4;
        let Some(line) = block.get(at..at.saturating_add(length as usize)) else {
            break;
        };
        at += length as usize;
        let Ok(line) = std::str::from_utf8(line) else {
            continue;
        };
        if let Some((key, value)) = line.split_once('=')
            && key.trim().eq_ignore_ascii_case(name)
            && !value.trim().is_empty()
        {
            found.push(value.to_owned());
        }
    }
    found
}

/// A little-endian length at `at`, or nothing where the block is shorter than it claims.
fn length_at(block: &[u8], at: usize) -> Option<u32> {
    let bytes = block.get(at..at.saturating_add(4))?;
    Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A comment block: the vendor, the count, and one length-prefixed line each.
    fn block(vendor: &str, comments: &[&str]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
        out.extend_from_slice(vendor.as_bytes());
        out.extend_from_slice(&(comments.len() as u32).to_le_bytes());
        for line in comments {
            out.extend_from_slice(&(line.len() as u32).to_le_bytes());
            out.extend_from_slice(line.as_bytes());
        }
        out
    }

    #[test]
    fn every_value_of_one_comment_is_read_in_the_order_it_was_written() {
        let block = block(
            "reference libFLAC",
            &[
                "COMPOSER=Johann Sebastian Bach",
                "COMPOSERSORT=Bach, Johann Sebastian",
                "TITLE=Sinfonia",
                "COMPOSERSORT=Abbado, Claudio",
            ],
        );
        assert_eq!(
            values(&block, "COMPOSERSORT"),
            ["Bach, Johann Sebastian", "Abbado, Claudio"]
        );
    }

    #[test]
    fn a_field_name_is_matched_whatever_case_it_was_written_in() {
        let block = block("x", &["composersort=Bach, Johann Sebastian"]);
        assert_eq!(values(&block, "COMPOSERSORT"), ["Bach, Johann Sebastian"]);
    }

    #[test]
    fn a_comment_nothing_wrote_is_no_values_rather_than_an_empty_one() {
        let block = block("x", &["TITLE=Sinfonia", "COMPOSERSORT=   "]);
        assert!(values(&block, "COMPOSERSORT").is_empty());
        assert!(values(&block, "ARTISTSORT").is_empty());
    }

    #[test]
    fn a_block_that_lies_about_its_own_lengths_is_read_as_far_as_it_goes() {
        let mut truncated = block("x", &["TITLE=One", "COMPOSERSORT=Bach, Johann Sebastian"]);
        truncated.truncate(truncated.len() - 10);
        assert!(values(&truncated, "COMPOSERSORT").is_empty());

        let mut overstated = block("x", &["COMPOSERSORT=Bach"]);
        overstated[4 + 1 + 4] = 0xff;
        assert!(values(&overstated, "COMPOSERSORT").is_empty());

        assert!(values(&[], "COMPOSERSORT").is_empty());
        assert!(values(&[0, 0, 0], "COMPOSERSORT").is_empty());
    }

    #[test]
    fn a_count_larger_than_the_block_stops_at_the_end_of_it() {
        let mut lying = block("x", &["COMPOSERSORT=Bach"]);
        lying[4 + 1..4 + 1 + 4].copy_from_slice(&9_999u32.to_le_bytes());
        assert_eq!(values(&lying, "COMPOSERSORT"), ["Bach"]);
    }
}
