//! Which credits name somebody and which are placeholders.

use crate::index::fold;

/// MusicBrainz's special-purpose artist for a release with no one credit.
pub const VARIOUS_ARTISTS_MBID: &str = "89ad4ac3-39f7-470e-963a-56509c546377";

/// The one spelling every variation is published under. MusicBrainz and Rate Your Music write
/// this; Discogs and FreeDB write "Various". Serving both splits one collection in two.
pub const VARIOUS_ARTISTS: &str = "Various Artists";

/// Folded, so `V/A`, `V-A` and `V.A.` all match. These name the release as having no one artist,
/// which is a statement, so they are kept and spelt one way.
const VARIOUS: &[&str] = &[
    "various",
    "various artists",
    "various artist",
    "va",
    "v a",
    "v.a",
    "v.a.",
];

/// These name nobody at all, so they are dropped.
const UNKNOWN: &[&str] = &[
    "unknown",
    "unknown artist",
    "[unknown]",
    "[unknown artist]",
    "no artist",
    "none",
];

/// The identifier is checked too: the list is English only.
pub fn is_various(name: &str, mbid: Option<&str>) -> bool {
    VARIOUS.contains(&fold(name).as_str()) || mbid == Some(VARIOUS_ARTISTS_MBID)
}

pub fn is_unknown(name: &str) -> bool {
    UNKNOWN.contains(&fold(name).as_str())
}

/// Only when the tagger wrote one identifier per name.
fn aligned<'a>(names: &[String], mbids: &'a [String]) -> Option<&'a [String]> {
    (mbids.len() == names.len()).then_some(mbids)
}

/// A dropped name takes its sort spelling with it, so later sorts stay aligned. A Various spelling
/// is kept under the one spelling, and loses its sort value, which described the old spelling.
pub fn crediting(names: &[String], sorts: &[String], mbids: &[String]) -> Vec<Credit> {
    let mbids = aligned(names, mbids);
    names
        .iter()
        .enumerate()
        .filter(|(_, name)| !is_unknown(name))
        .map(|(at, name)| {
            match is_various(name, mbids.and_then(|ids| ids.get(at)).map(String::as_str)) {
                true => Credit::new(VARIOUS_ARTISTS),
                false => Credit {
                    name: name.clone(),
                    sort: sort_at(sorts, at),
                },
            }
        })
        .collect()
}

/// Whether the tagger said this release has no one artist, by name or by identifier.
pub fn credits_various(names: &[String], mbids: &[String]) -> bool {
    let mbids = aligned(names, mbids);
    names
        .iter()
        .enumerate()
        .any(|(at, name)| is_various(name, mbids.and_then(|ids| ids.get(at)).map(String::as_str)))
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Credit {
    pub name: String,
    pub sort: Option<String>,
}

impl Credit {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            sort: None,
        }
    }

    pub fn ordered_by(&self) -> &str {
        self.sort.as_deref().unwrap_or(&self.name)
    }
}

/// Fewer sort values than names leaves the rest without one.
pub fn paired(names: &[String], sorts: &[String]) -> Vec<Credit> {
    names
        .iter()
        .enumerate()
        .map(|(at, name)| Credit {
            name: name.clone(),
            sort: sort_at(sorts, at),
        })
        .collect()
}

fn sort_at(sorts: &[String], at: usize) -> Option<String> {
    sorts
        .get(at)
        .filter(|sort| !sort.trim().is_empty())
        .cloned()
}

pub fn folded(credits: &[Credit]) -> String {
    crate::index::identity::folded_list(&names(credits))
}

pub fn names(credits: &[Credit]) -> Vec<String> {
    credits.iter().map(|credit| credit.name.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| (*value).to_owned()).collect()
    }

    #[test]
    fn the_spellings_a_real_library_carries_all_mean_various() {
        for value in ["Various", "Various Artists", "VARIOUS ARTISTS", "various"] {
            assert!(is_various(value, None), "{value}");
        }
    }

    #[test]
    fn the_short_forms_a_tagger_writes_mean_various_however_they_are_punctuated() {
        for value in ["VA", "V.A.", "V/A", "V-A", "v.a"] {
            assert!(is_various(value, None), "{value}");
        }
    }

    #[test]
    fn a_missing_credit_written_out_names_nobody() {
        for value in ["Unknown", "Unknown Artist", "[Unknown Artist]", "None"] {
            assert!(is_unknown(value), "{value}");
            assert!(!is_various(value, None), "{value}");
        }
    }

    #[test]
    fn a_name_that_merely_contains_one_is_a_name() {
        for value in [
            "Various Production",
            "The Unknown",
            "Vanessa Paradis",
            "Van Halen",
            "Nonesuch Explorer Series",
        ] {
            assert!(!is_various(value, None) && !is_unknown(value), "{value}");
        }
    }

    fn shown(credits: &[Credit]) -> Vec<String> {
        credits.iter().map(|credit| credit.name.clone()).collect()
    }

    #[test]
    fn a_name_that_names_nobody_leaves_the_name_beside_it() {
        assert_eq!(
            shown(&crediting(
                &names(&["Unknown Artist", "Bob Marley"]),
                &[],
                &[]
            )),
            names(&["Bob Marley"])
        );
    }

    #[test]
    fn every_spelling_of_various_is_published_as_one() {
        assert_eq!(
            shown(&crediting(&names(&["Various", "VA"]), &[], &[])),
            names(&[VARIOUS_ARTISTS, VARIOUS_ARTISTS]),
            "two spellings of the same statement must not split a collection in two"
        );
    }

    #[test]
    fn a_list_with_no_placeholder_is_returned_whole() {
        let credits = names(&["Bob Marley", "The Wailers"]);
        assert_eq!(shown(&crediting(&credits, &[], &[])), credits);
    }

    #[test]
    fn a_dropped_credit_takes_the_sort_spelling_written_beside_it() {
        let credits = crediting(
            &names(&["Unknown Artist", "Bob Marley"]),
            &names(&["Unknown Artist", "Marley, Bob"]),
            &[],
        );
        assert_eq!(shown(&credits), names(&["Bob Marley"]));
        assert_eq!(credits[0].ordered_by(), "Marley, Bob");
    }

    #[test]
    fn the_musicbrainz_identifier_means_various_whatever_language_spelled_it() {
        let credits = crediting(
            &names(&["Divers", "Amadou & Mariam"]),
            &[],
            &names(&[VARIOUS_ARTISTS_MBID, "e0e5f61a-4c60-4e0d-bf3d-b0dc3f8b6e35"]),
        );
        assert_eq!(
            shown(&credits),
            names(&[VARIOUS_ARTISTS, "Amadou & Mariam"]),
            "no spelling of `various` in any language is on the list, and the identifier is"
        );
        assert!(credits_various(
            &names(&["Divers"]),
            &names(&[VARIOUS_ARTISTS_MBID])
        ));
    }

    #[test]
    fn an_identifier_list_that_does_not_line_up_with_the_names_is_not_read() {
        let credits = crediting(
            &names(&["Amadou & Mariam", "Manu Chao"]),
            &[],
            &names(&[VARIOUS_ARTISTS_MBID]),
        );
        assert_eq!(
            shown(&credits),
            names(&["Amadou & Mariam", "Manu Chao"]),
            "one identifier for two names says nothing about which name it belongs to"
        );
    }
    #[test]
    fn a_sort_spelling_pairs_with_the_name_at_its_position() {
        let credits = paired(
            &[
                "Johann Sebastian Bach".to_owned(),
                "Claudio Abbado".to_owned(),
            ],
            &["Bach, Johann Sebastian".to_owned()],
        );
        assert_eq!(credits[0].sort.as_deref(), Some("Bach, Johann Sebastian"));
        assert_eq!(
            credits[1].sort, None,
            "a tagger wrote fewer sorts than names"
        );
        assert_eq!(credits[0].ordered_by(), "Bach, Johann Sebastian");
        assert_eq!(credits[1].ordered_by(), "Claudio Abbado");
    }

    #[test]
    fn a_blank_sort_spelling_is_no_sort_spelling() {
        let credits = paired(&["Autechre".to_owned()], &["   ".to_owned()]);
        assert_eq!(credits[0].sort, None);
        assert_eq!(credits[0].ordered_by(), "Autechre");
    }

    #[test]
    fn more_sorts_than_names_cannot_invent_a_credit() {
        let credits = paired(
            &["Coil".to_owned()],
            &["Coil".to_owned(), "Ghost".to_owned()],
        );
        assert_eq!(credits.len(), 1);
    }
}
