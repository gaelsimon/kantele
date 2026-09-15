//! Disc markers in folder names and titles.

pub fn strip_disc_marker(title: &str) -> (&str, Option<u32>) {
    let trimmed = title.trim();
    if let Some(open) = bracketed_tail(trimmed)
        && let Some(disc) = disc_phrase(&trimmed[open + 1..trimmed.len() - 1])
    {
        return (trim_separator(&trimmed[..open]), Some(disc));
    }
    if let Some(start) = last_keyword(trimmed)
        && let Some(disc) = disc_phrase(&trimmed[start..])
    {
        return (trim_separator(&trimmed[..start]), Some(disc));
    }
    (trimmed, None)
}

const DISC_WORDS: &[&str] = &["disque", "disco", "disc", "disk", "cd"];

pub(super) fn folder_disc(name: &str) -> Option<u32> {
    let lowered = name.to_ascii_lowercase();
    for word in DISC_WORDS {
        let mut from = 0;
        while let Some(offset) = lowered[from..].find(*word) {
            let at = from + offset;
            from = at + word.len();
            let opens_a_word = at == 0
                || !lowered[..at]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_alphanumeric);
            if !opens_a_word {
                continue;
            }
            let rest = name[from..].trim_start_matches([' ', '.', ':', '-', '_', '\t', '#']);
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            if digits.is_empty() {
                continue;
            }
            let ends_the_number = rest[digits.len()..]
                .chars()
                .next()
                .is_none_or(|c| !c.is_alphanumeric());
            if !ends_the_number {
                continue;
            }
            if let Some(disc) = digits.parse().ok().filter(|disc| *disc <= MOST_DISCS) {
                return Some(disc);
            }
        }
    }
    None
}

/// Above this the number is a year or a catalogue number.
const MOST_DISCS: u32 = 300;

fn disc_phrase(phrase: &str) -> Option<u32> {
    let phrase = phrase.trim();
    let lowered = phrase.to_ascii_lowercase();
    let word = DISC_WORDS.iter().find(|word| lowered.starts_with(**word))?;
    let rest = phrase[word.len()..].trim_start_matches([' ', '.', ':', '-', '_', '\t', '#']);
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    if digits.is_empty() || !rest[digits.len()..].trim().is_empty() {
        return None;
    }
    digits.parse().ok().filter(|disc| *disc <= MOST_DISCS)
}

fn bracketed_tail(value: &str) -> Option<usize> {
    let (open, close) = match value.chars().last()? {
        ')' => ('(', ')'),
        ']' => ('[', ']'),
        _ => return None,
    };
    let start = value.rfind(open)?;
    (value[start..].chars().filter(|c| *c == close).count() == 1 && value.len() > start + 2)
        .then_some(start)
}

fn last_keyword(value: &str) -> Option<usize> {
    let lowered = value.to_ascii_lowercase();
    DISC_WORDS
        .iter()
        .filter_map(|word| {
            let at = lowered.rfind(*word)?;
            let boundary = at == 0
                || !lowered[..at]
                    .chars()
                    .next_back()
                    .is_some_and(char::is_alphanumeric);
            boundary.then_some(at)
        })
        .max()
}

pub(super) fn trim_separator(value: &str) -> &str {
    value
        .trim_end()
        .trim_end_matches(['-', ',', ':', '/'])
        .trim_end()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disc_markers_are_recognised_and_ordinary_numbers_are_not() {
        assert_eq!(
            strip_disc_marker("Symphonies [disc 2]"),
            ("Symphonies", Some(2))
        );
        assert_eq!(
            strip_disc_marker("Symphonies (CD 3)"),
            ("Symphonies", Some(3))
        );
        assert_eq!(
            strip_disc_marker("Symphonies - Disc 4"),
            ("Symphonies", Some(4))
        );
        assert_eq!(
            strip_disc_marker("Symphonies, disk 5"),
            ("Symphonies", Some(5))
        );
        assert_eq!(strip_disc_marker("Symphonies CD1"), ("Symphonies", Some(1)));
        assert_eq!(
            strip_disc_marker("Symphony No. 2"),
            ("Symphony No. 2", None)
        );
        assert_eq!(
            strip_disc_marker("Greatest Hits Vol. 2"),
            ("Greatest Hits Vol. 2", None)
        );
        assert_eq!(
            strip_disc_marker("Bitches Brew (Remastered)"),
            ("Bitches Brew (Remastered)", None)
        );
        assert_eq!(
            strip_disc_marker("Disc Jockey Hits"),
            ("Disc Jockey Hits", None)
        );
    }
    #[test]
    fn disc_folders_are_recognised_and_ordinary_folders_are_not() {
        for (name, disc) in [
            ("CD1", Some(1)),
            ("cd 2", Some(2)),
            ("Disc 03", Some(3)),
            ("disk-4", Some(4)),
            ("Disque 5", Some(5)),
            ("disc 2 - Dubs & Maw Vocals", Some(2)),
            ("Various Artists - Masters at Work - CD1 (Louie)", Some(1)),
            ("The Blue Note Studio Sessions - CD3 - Mosaic", Some(3)),
            ("Disco 2", Some(2)),
            ("Brahms", None),
            ("CD Singles", None),
            ("Vol 2", None),
            ("CD 2003", None),
            ("Discovery", None),
            ("Discos Fuentes 50", None),
            ("Sun Ra - Disco 3000", None),
        ] {
            assert_eq!(folder_disc(name), disc, "{name}");
        }
    }
}
