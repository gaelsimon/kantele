//! The letters Windows-1252 puts where Latin-1 has control characters.

/// Bytes 0x80 to 0x9F. The five Windows-1252 leaves unassigned stay what Latin-1 says.
const HIGH: [Option<char>; 32] = [
    Some('€'),
    None,
    Some('‚'),
    Some('ƒ'),
    Some('„'),
    Some('…'),
    Some('†'),
    Some('‡'),
    Some('ˆ'),
    Some('‰'),
    Some('Š'),
    Some('‹'),
    Some('Œ'),
    None,
    Some('Ž'),
    None,
    None,
    Some('‘'),
    Some('’'),
    Some('“'),
    Some('”'),
    Some('•'),
    Some('–'),
    Some('—'),
    Some('˜'),
    Some('™'),
    Some('š'),
    Some('›'),
    Some('œ'),
    None,
    Some('ž'),
    Some('Ÿ'),
];

/// Text that is not UTF-8 and claims to be Latin-1 is almost always Windows-1252.
pub fn decoded(byte: u8) -> char {
    match byte {
        0x80..=0x9F => HIGH[usize::from(byte - 0x80)].unwrap_or(char::from(byte)),
        _ => char::from(byte),
    }
}

fn control(c: char) -> bool {
    matches!(c, '\u{80}'..='\u{9f}')
}

/// The text with each C1 control, which no tag means, back to the letter its byte stood for.
/// Nothing where there is none.
pub fn repaired(text: &str) -> Option<String> {
    text.chars().any(control).then(|| {
        text.chars()
            .map(|c| match control(c) {
                true => decoded(c as u8),
                false => c,
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_byte_windows_1252_leaves_unassigned_keeps_its_latin_1_reading() {
        assert_eq!(decoded(0x81), '\u{81}');
        assert_eq!(decoded(0x9d), '\u{9d}');
    }

    #[test]
    fn a_byte_both_code_pages_agree_on_is_left_alone() {
        assert_eq!(decoded(b'a'), 'a');
        assert_eq!(decoded(0xe9), 'é');
        assert_eq!(repaired("café"), None);
    }
}
