//! Folding strings for comparison, indexing and sorting.

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// The one spelling a name is published in, whichever a tagger or a file system wrote.
pub fn nfc(value: &str) -> String {
    match unicode_normalization::is_nfc(value) {
        true => value.to_owned(),
        false => value.nfc().collect(),
    }
}

pub fn fold(value: &str) -> String {
    let lowered = value.to_lowercase();
    let stripped: String = lowered
        .nfkd()
        .filter(|c| !is_combining_mark(*c))
        .flat_map(expand)
        .collect();
    collapse_whitespace(&stripped)
}

fn expand(c: char) -> impl Iterator<Item = char> {
    let replacement = transliterate(c);
    let single = replacement.is_none().then(|| canonical_punctuation(c));
    replacement.unwrap_or_default().chars().chain(single)
}

fn canonical_punctuation(c: char) -> char {
    match c {
        '\u{2018}' | '\u{2019}' | '\u{02BC}' | '\u{FF07}' => '\'',
        '\u{201C}' | '\u{201D}' | '\u{FF02}' => '"',
        '-' | '_' | '/' | '\\' => ' ',
        '\u{2010}'..='\u{2015}' | '\u{2212}' | '\u{FF0D}' => ' ',
        '\u{00A0}' | '\u{2007}' | '\u{202F}' | '\u{FEFF}' => ' ',
        other => other,
    }
}

/// Letters NFKD does not decompose.
fn transliterate(c: char) -> Option<&'static str> {
    Some(match c {
        'ø' | 'Ø' => "o",
        'æ' | 'Æ' => "ae",
        'œ' | 'Œ' => "oe",
        'ß' | 'ẞ' => "ss",
        'đ' | 'Đ' | 'ð' | 'Ð' => "d",
        'þ' | 'Þ' => "th",
        'ł' | 'Ł' => "l",
        'ħ' | 'Ħ' => "h",
        'ŧ' | 'Ŧ' => "t",
        'ŋ' | 'Ŋ' => "n",
        'ə' | 'Ə' => "e",
        'ı' => "i",
        'ĸ' => "k",
        _ => return None,
    })
}

fn collapse_whitespace(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut pending_space = false;
    for c in value.chars() {
        if c.is_whitespace() {
            pending_space = !out.is_empty();
            continue;
        }
        if pending_space {
            out.push(' ');
            pending_space = false;
        }
        out.push(c);
    }
    out
}

/// What a listing looks past when ordering a value that has no sort tag.
pub const DEFAULT_SORT_IGNORE: &[&str] = &["The"];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ignored {
    prefixes: Vec<String>,
}

impl Default for Ignored {
    fn default() -> Self {
        Self::new(DEFAULT_SORT_IGNORE.iter().copied())
    }
}

impl Ignored {
    pub fn new<'a>(words: impl IntoIterator<Item = &'a str>) -> Self {
        Self {
            prefixes: words
                .into_iter()
                .map(fold)
                .filter(|word| !word.is_empty())
                .collect(),
        }
    }

    /// A whole word, or an apostrophe-final prefix such as `l'`, and never the whole value.
    pub fn strip<'a>(&self, folded: &'a str) -> &'a str {
        for prefix in &self.prefixes {
            let rest = match prefix.ends_with('\'') {
                true => folded.strip_prefix(prefix.as_str()),
                false => folded
                    .strip_prefix(prefix.as_str())
                    .and_then(|rest| rest.strip_prefix(' ')),
            };
            if let Some(rest) = rest.map(str::trim_start).filter(|rest| !rest.is_empty()) {
                return rest;
            }
        }
        folded
    }
}

#[cfg(test)]
mod ignored {
    use super::{Ignored, fold};

    #[test]
    fn the_article_is_looked_past_as_a_whole_word_and_never_inside_one() {
        let ignored = Ignored::default();
        assert_eq!(ignored.strip(&fold("The Beatles")), "beatles");
        assert_eq!(ignored.strip(&fold("Theatre of Hate")), "theatre of hate");
        assert_eq!(
            ignored.strip(&fold("The")),
            "the",
            "a value that is only the word keeps it"
        );
        assert_eq!(ignored.strip(&fold("Cure")), "cure");
    }

    #[test]
    fn an_apostrophe_final_prefix_needs_no_space_and_an_empty_list_changes_nothing() {
        let french = Ignored::new(["L'", "Les"]);
        assert_eq!(
            french.strip(&fold("L'Orchestre National")),
            "orchestre national"
        );
        assert_eq!(french.strip(&fold("Les Rita Mitsouko")), "rita mitsouko");
        assert_eq!(french.strip(&fold("The Beatles")), "the beatles");
        assert_eq!(Ignored::new([]).strip("the beatles"), "the beatles");
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_letter_that_is_not_an_accented_letter_still_answers_a_plain_keyboard() {
        assert_eq!(fold("Trentemøller"), "trentemoller");
        assert_eq!(fold("Sigur Rós"), "sigur ros");
        assert_eq!(fold("Motörhead"), "motorhead");
        assert_eq!(fold("Björk"), "bjork");
        assert_eq!(fold("Straße"), "strasse");
        assert_eq!(fold("Cœur de Pirate"), "coeur de pirate");
        assert_eq!(fold("Æther"), "aether");
        assert_eq!(fold("Þeyr"), "theyr");
        assert_eq!(fold("Guðmundsson"), "gudmundsson");
        assert_eq!(fold("Łukasz"), "lukasz");
    }

    #[test]
    fn a_search_typed_on_a_plain_keyboard_finds_the_tag() {
        let matches = |tag: &str, typed: &str| fold(tag).contains(&fold(typed));
        assert!(matches("Trentemøller", "trentemoller"));
        assert!(matches("Cœur de Pirate", "coeur"));
        assert!(matches("Straße der Besten", "strasse"));
        assert!(matches("Trentemøller", "Trentemøller"));
        assert!(matches("Cœur de Pirate", "cœur"));
    }

    #[test]
    fn compatibility_forms_fold_onto_the_plain_ones() {
        assert_eq!(fold("ﬁnale"), "finale");
        assert_eq!(fold("ＷＵ ＴＡＮＧ"), "wu tang");
        assert_eq!(fold("Vol Ⅳ"), "vol iv");
    }

    #[test]
    fn cyrillic_and_greek_are_left_exactly_as_they_are() {
        assert_eq!(fold("Кино"), "кино");
        assert_eq!(fold("Ελευθερία"), "ελευθερια");
    }

    #[test]
    fn a_hyphen_is_a_space_because_nobody_types_it() {
        assert_eq!(fold("Wu-Tang Clan"), "wu tang clan");
        assert!(fold("Wu-Tang Clan").contains(&fold("wu tang")));
        assert!(fold("Wu-Tang Clan").contains(&fold("wu-tang")));
        assert_eq!(fold("Jay-Z"), "jay z");
        assert_eq!(fold("AC/DC"), "ac dc");
        assert_eq!(fold("hydroponic_sound_system"), "hydroponic sound system");
        assert_eq!(fold("a - b"), "a b");
        assert_eq!(fold("a--b"), "a b");
    }

    #[test]
    fn an_apostrophe_and_a_full_stop_stay_inside_their_words() {
        assert_eq!(fold("Don't Look Back"), "don't look back");
        assert_eq!(fold("R.E.M."), "r.e.m.");
    }

    use super::*;

    #[test]
    fn diacritics_fold_away() {
        assert_eq!(fold("Dvořák"), "dvorak");
        assert_eq!(fold("Élégie"), "elegie");
        assert_eq!(fold("Arvo Pärt"), "arvo part");
        assert_eq!(fold("No Me Llores Más"), "no me llores mas");
    }

    #[test]
    fn lowercasing_is_unicode_not_ascii() {
        assert_eq!(fold("ÉLÉGIE"), "elegie");
        assert_eq!(fold("ÅRSTIDER"), "arstider");
    }

    #[test]
    fn folding_is_idempotent() {
        for value in ["Dvořák", "  Béla   Bartók ", "L’Élégie"] {
            let once = fold(value);
            assert_eq!(fold(&once), once, "{value}");
        }
    }

    #[test]
    fn quotes_and_dashes_that_render_alike_compare_alike() {
        assert_eq!(fold("L’Élégie"), fold("L'Elegie"));
        assert_eq!(fold("Rock ‘n’ Roll"), fold("Rock 'n' Roll"));
        assert_eq!(fold("A – B"), fold("A - B"));
    }

    #[test]
    fn invisible_whitespace_differences_disappear() {
        assert_eq!(fold("Symphonies "), "symphonies");
        assert_eq!(
            fold("Wiener\u{00A0}Philharmoniker"),
            "wiener philharmoniker"
        );
        assert_eq!(fold("  The   Wall  "), "the wall");
    }

    #[test]
    fn distinct_values_stay_distinct() {
        assert_ne!(fold("Symphony 2"), fold("Symphony 10"));
        assert_ne!(fold("Beethoven"), fold("Beethovens"));
        assert_ne!(fold("Blur"), fold("Blu 4"));
        assert_ne!(fold("Wu-Tang"), fold("WuTang"));
        assert_ne!(fold("Rogers'"), fold("Rogers"));
        assert_ne!(fold("Кино"), fold("Kino"));
        assert_ne!(fold("Sun"), fold("Son"));
    }

    #[test]
    fn a_transliteration_is_the_spelling_a_keyboard_produces_and_not_a_guess() {
        for (exotic, plain) in [
            ("ø", "o"),
            ("æ", "ae"),
            ("œ", "oe"),
            ("ß", "ss"),
            ("ð", "d"),
            ("đ", "d"),
            ("þ", "th"),
            ("ł", "l"),
            ("ŋ", "n"),
        ] {
            assert_eq!(fold(exotic), plain, "{exotic} should fold to {plain}");
            assert_eq!(fold(&exotic.to_uppercase()), plain);
        }
    }

    #[test]
    fn folding_is_still_idempotent_after_transliteration() {
        for value in [
            "Trentemøller",
            "Straße",
            "Cœur",
            "Þeyr",
            "ﬁnale",
            "Wu-Tang Clan",
        ] {
            assert_eq!(fold(&fold(value)), fold(value), "{value}");
        }
    }

    #[test]
    fn empty_and_whitespace_only_fold_to_empty() {
        assert_eq!(fold(""), "");
        assert_eq!(fold("   \t\n "), "");
    }

    #[test]
    fn a_name_a_mac_wrote_decomposed_is_published_composed() {
        assert_eq!(nfc("Bjo\u{308}rk"), "Björk");
        assert_eq!(nfc("Björk"), "Björk");
        assert_eq!(nfc("Кино"), "Кино");
    }
}
