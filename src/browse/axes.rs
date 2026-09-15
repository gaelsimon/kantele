//! Every browsing axis, interned once when the library is built.

use std::collections::HashMap;

use crate::index::{Track, fold};

use super::{FACETS, Facet, digest};

/// Every axis, interned once when the library is built.
#[derive(Clone, Debug, Default)]
pub struct Axes {
    axes: Vec<Axis>,
}

/// One value of one axis inside a selection, borrowed from the axis rather than minted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry<'a> {
    /// The spelling shown, which is the one most of the library uses.
    pub display: &'a str,
    pub folded: &'a str,
    /// What this value is ordered and grouped by.
    pub sort: &'a str,
    /// What a position carries to name this value.
    pub digest: &'a str,
    /// How many tracks of the selection carry it.
    pub tracks: usize,
}

impl Axes {
    /// Interns every axis of every track.
    pub fn build(tracks: &[Track], ignored: &fold::Ignored) -> Self {
        Self {
            axes: FACETS
                .iter()
                .map(|facet| Axis::build(*facet, tracks, ignored))
                .collect(),
        }
    }

    fn axis(&self, facet: Facet) -> Option<&Axis> {
        self.axes.get(FACETS.iter().position(|at| *at == facet)?)
    }

    /// Roughly what these tables hold, in bytes, for the memory report.
    pub fn footprint(&self) -> usize {
        self.axes
            .iter()
            .map(|axis| {
                let values: usize = axis
                    .values
                    .iter()
                    .map(|value| {
                        value.folded.len()
                            + value.sort.len()
                            + value.display.len()
                            + value.digest.len()
                            + 72
                    })
                    .sum();
                let rows = axis.ids.len() * 4 + axis.spans.len() * 8 + axis.totals.len() * 4;
                let digests: usize = axis
                    .by_digest
                    .keys()
                    .map(|digest| digest.len() + 24 + 16)
                    .sum();
                values + rows + digests
            })
            .sum()
    }

    /// The value one digest names on one axis, or nothing where it names none.
    pub(super) fn id(&self, facet: Facet, digest: &str) -> Option<u32> {
        self.axis(facet)?.by_digest.get(digest).copied()
    }

    /// Whether a track carries a value on an axis.
    pub(super) fn carries(&self, facet: Facet, track: usize, id: u32) -> bool {
        self.axis(facet)
            .is_some_and(|axis| axis.of(track).contains(&id))
    }

    /// Whether an axis can reach a track at all.
    pub(super) fn reaches(&self, facet: Facet, track: usize) -> bool {
        self.axis(facet)
            .is_some_and(|axis| !axis.of(track).is_empty())
    }

    /// The distinct values of one axis inside a selection, already in the order they are shown.
    pub(super) fn distinct(&self, facet: Facet, selected: &[usize]) -> Vec<Entry<'_>> {
        let Some(axis) = self.axis(facet) else {
            return Vec::new();
        };
        if axis.holds_everything(selected) {
            return axis
                .values
                .iter()
                .zip(&axis.totals)
                .map(|(value, total)| value.entry(*total as usize))
                .collect();
        }
        axis.values
            .iter()
            .zip(axis.counts(selected))
            .filter(|(_, count)| *count > 0)
            .map(|(value, count)| value.entry(count as usize))
            .collect()
    }

    /// How many values of one axis a selection reaches, without gathering them.
    pub(super) fn count(&self, facet: Facet, selected: &[usize]) -> usize {
        let Some(axis) = self.axis(facet) else {
            return 0;
        };
        if axis.holds_everything(selected) {
            return axis.values.len();
        }
        axis.counts(selected)
            .into_iter()
            .filter(|count| *count > 0)
            .count()
    }
}

/// One axis: its distinct values, and which of them each track carries.
#[derive(Clone, Debug, Default)]
struct Axis {
    values: Vec<Value>,
    /// How many tracks of the whole library carry each value.
    totals: Vec<u32>,
    ids: Vec<u32>,
    /// Where each track's ids start and how many there are.
    spans: Vec<(u32, u32)>,
    /// What a client sends back, to what it names.
    by_digest: HashMap<String, u32>,
}

/// One value of one axis.
#[derive(Clone, Debug)]
struct Value {
    folded: String,
    /// What a listing orders by, which is the folded form on every axis that is not a number.
    sort: String,
    /// The spelling shown, which is the one most of the library uses.
    display: String,
    /// What a position carries to name this value.
    digest: String,
}

impl Value {
    fn entry(&self, tracks: usize) -> Entry<'_> {
        Entry {
            display: &self.display,
            folded: &self.folded,
            sort: &self.sort,
            digest: &self.digest,
            tracks,
        }
    }
}

/// The distinct values of one axis as a pass over the tracks finds them.
#[derive(Default)]
struct Interning {
    by_folded: HashMap<String, u32>,
    /// The spelling exactly as written, so a value seen again is not folded twice.
    by_raw: HashMap<String, u32>,
    folded: Vec<String>,
    totals: Vec<u32>,
    spellings: Vec<HashMap<String, usize>>,
    /// How often each companion sort spelling was written.
    sorts: Vec<HashMap<String, usize>>,
}

impl Interning {
    /// The value one written form belongs to, or nothing where it folds away to nothing.
    fn id(&mut self, written: &str) -> Option<u32> {
        let id = match self.by_raw.get(written) {
            Some(id) => *id,
            None => {
                let form = fold(written);
                if form.is_empty() {
                    return None;
                }
                let id = match self.by_folded.get(&form) {
                    Some(id) => *id,
                    None => {
                        let id = self.folded.len() as u32;
                        self.by_folded.insert(form.clone(), id);
                        self.folded.push(form);
                        self.totals.push(0);
                        self.spellings.push(HashMap::new());
                        self.sorts.push(HashMap::new());
                        id
                    }
                };
                self.by_raw.insert(written.to_owned(), id);
                id
            }
        };
        // Owned only for a spelling not seen before, so a repeat costs a lookup and nothing else.
        match self.spellings[id as usize].get_mut(written) {
            Some(times) => *times += 1,
            None => {
                self.spellings[id as usize].insert(written.to_owned(), 1);
            }
        }
        Some(id)
    }

    /// Records the sort spelling a tagger wrote beside one value.
    fn note_sort(&mut self, id: u32, sort: &str) {
        if sort.trim().is_empty() {
            return;
        }
        match self.sorts[id as usize].get_mut(sort) {
            Some(times) => *times += 1,
            None => {
                self.sorts[id as usize].insert(sort.to_owned(), 1);
            }
        }
    }

    /// One value per folded form, shown in the spelling most of the library uses.
    fn settle(self, facet: Facet, ignored: &fold::Ignored) -> (Vec<Value>, Vec<u32>) {
        let values = self
            .folded
            .into_iter()
            .zip(self.spellings)
            .zip(self.sorts)
            .map(|((folded, spellings), sorts)| {
                let display = most_written(&spellings).unwrap_or_else(|| folded.clone());
                // The companion sort tag wins where a tagger wrote one.
                let sort = match most_written(&sorts) {
                    Some(written) => facet.sort_key(&fold(&written)),
                    None => facet.sort_key(ignored.strip(&folded)),
                };
                Value {
                    digest: digest(&folded),
                    sort,
                    folded,
                    display,
                }
            })
            .collect();
        (values, self.totals)
    }
}

/// The spelling a majority of the library wrote.
fn most_written(counts: &HashMap<String, usize>) -> Option<String> {
    counts
        .iter()
        .max_by_key(|(spelling, times)| (*times, std::cmp::Reverse(*spelling)))
        .map(|(spelling, _)| spelling.clone())
}

impl Axis {
    fn build(facet: Facet, tracks: &[Track], ignored: &fold::Ignored) -> Self {
        let mut axis = Axis {
            spans: Vec::with_capacity(tracks.len()),
            ..Axis::default()
        };
        let mut interning = Interning::default();
        for track in tracks {
            let start = axis.ids.len() as u32;
            for (value, sort) in facet.values(track).iter() {
                let Some(id) = interning.id(value) else {
                    continue;
                };
                if sort != value {
                    interning.note_sort(id, sort);
                }
                if !axis.ids[start as usize..].contains(&id) {
                    axis.ids.push(id);
                    interning.totals[id as usize] += 1;
                }
            }
            axis.spans.push((start, axis.ids.len() as u32 - start));
        }
        (axis.values, axis.totals) = interning.settle(facet, ignored);
        axis.order_by_sort_key();
        axis.by_digest = axis
            .values
            .iter()
            .enumerate()
            .map(|(id, value)| (value.digest.clone(), id as u32))
            .collect();
        axis
    }

    /// Puts the values in the order a listing shows them, and moves every reference with them.
    fn order_by_sort_key(&mut self) {
        let mut sorted: Vec<(u32, Value, u32)> = std::mem::take(&mut self.values)
            .into_iter()
            .zip(std::mem::take(&mut self.totals))
            .enumerate()
            .map(|(id, (value, total))| (id as u32, value, total))
            .collect();
        sorted.sort_by(|(_, left, _), (_, right, _)| {
            (&left.sort, &left.folded).cmp(&(&right.sort, &right.folded))
        });
        let mut rank = vec![0u32; sorted.len()];
        for (at, (was, _, _)) in sorted.iter().enumerate() {
            rank[*was as usize] = at as u32;
        }
        for id in &mut self.ids {
            *id = rank[*id as usize];
        }
        (self.values, self.totals) = sorted
            .into_iter()
            .map(|(_, value, total)| (value, total))
            .unzip();
    }

    fn holds_everything(&self, selected: &[usize]) -> bool {
        selected.len() == self.spans.len()
    }

    /// How many tracks of a selection carry each value, by value.
    fn counts(&self, selected: &[usize]) -> Vec<u32> {
        let mut counts = vec![0u32; self.values.len()];
        for at in selected {
            for id in self.of(*at) {
                counts[*id as usize] += 1;
            }
        }
        counts
    }

    /// The values this track carries on this axis.
    fn of(&self, track: usize) -> &[u32] {
        match self.spans.get(track) {
            Some((start, len)) => &self.ids[*start as usize..(*start + *len) as usize],
            None => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{Library, Scanned};
    use crate::tags::{AudioProperties, FileTags};

    fn file(relative: &str, artist: &str, sort: Option<&str>) -> Scanned {
        Scanned {
            path: std::path::PathBuf::from("/music").join(relative),
            relative: std::path::PathBuf::from(relative),
            tags: FileTags {
                title: Some(relative.to_owned()),
                album: Some("Recital".to_owned()),
                artists: vec![artist.to_owned()],
                album_artists: vec![artist.to_owned()],
                artist_sorts: sort.map(|s| vec![s.to_owned()]).unwrap_or_default(),
                album_artist_sorts: sort.map(|s| vec![s.to_owned()]).unwrap_or_default(),
                track_number: Some(1),
                ..FileTags::default()
            },
            properties: AudioProperties {
                duration: std::time::Duration::from_secs(180),
                ..AudioProperties::default()
            },
            size: 1,
            artwork: None,
        }
    }

    fn ordered(files: &[Scanned], facet: Facet) -> Vec<String> {
        let library = Library::build("Music".to_owned(), files);
        let axes = Axes::build(library.tracks(), &fold::Ignored::default());
        let axis = axes.axis(facet).expect("the axis");
        axis.values.iter().map(|v| v.display.clone()).collect()
    }

    #[test]
    fn a_companion_sort_tag_decides_the_order_and_not_the_display() {
        let files = [
            file(
                "a.flac",
                "Johann Sebastian Bach",
                Some("Bach, Johann Sebastian"),
            ),
            file("b.flac", "Yo-Yo Ma", Some("Ma, Yo-Yo")),
            file("c.flac", "Claudio Abbado", Some("Abbado, Claudio")),
        ];
        // Ordered on the surname the tagger wrote, shown in the spelling a listener reads.
        assert_eq!(
            ordered(&files, Facet::AllArtists),
            ["Claudio Abbado", "Johann Sebastian Bach", "Yo-Yo Ma"]
        );
    }

    #[test]
    fn a_library_tagged_by_halves_orders_by_halves_and_that_is_the_tagger_speaking() {
        let files = [
            file(
                "a.flac",
                "Johann Sebastian Bach",
                Some("Bach, Johann Sebastian"),
            ),
            file("b.flac", "Claudio Abbado", None),
        ];
        // Bach was given a sort spelling and Abbado was not.
        assert_eq!(
            ordered(&files, Facet::AllArtists),
            ["Johann Sebastian Bach", "Claudio Abbado"]
        );
    }

    #[test]
    fn a_value_is_filed_under_the_letter_it_is_ordered_by() {
        let files = [file(
            "a.flac",
            "Johann Sebastian Bach",
            Some("Bach, Johann"),
        )];
        let library = Library::build("Music".to_owned(), &files);
        let axes = Axes::build(library.tracks(), &fold::Ignored::default());
        let axis = axes.axis(Facet::AllArtists).expect("the axis");
        let entry = axis.values[0].entry(1);
        assert_eq!(entry.display, "Johann Sebastian Bach");
        assert!(
            entry.sort.starts_with('b'),
            "ordering and lettering read one string, got {:?}",
            entry.sort
        );
    }

    #[test]
    fn without_a_sort_tag_the_order_is_the_display_spelling() {
        let files = [
            file("a.flac", "Johann Sebastian Bach", None),
            file("b.flac", "Claudio Abbado", None),
            file("c.flac", "Yo-Yo Ma", None),
        ];
        // No sort tag anywhere, so `Johann` files under J and sorts before `Yo-Yo`.
        assert_eq!(
            ordered(&files, Facet::AllArtists),
            ["Claudio Abbado", "Johann Sebastian Bach", "Yo-Yo Ma"]
        );
    }

    #[test]
    fn taggers_disagreeing_about_one_sort_spelling_are_settled_by_the_majority() {
        let files = [
            file(
                "a.flac",
                "Johann Sebastian Bach",
                Some("Bach, Johann Sebastian"),
            ),
            file(
                "b.flac",
                "Johann Sebastian Bach",
                Some("Bach, Johann Sebastian"),
            ),
            file("c.flac", "Johann Sebastian Bach", Some("Zzz Wrong")),
            file("d.flac", "Claudio Abbado", Some("Abbado, Claudio")),
        ];
        assert_eq!(
            ordered(&files, Facet::AllArtists),
            ["Claudio Abbado", "Johann Sebastian Bach"],
            "the minority spelling would have filed Bach last"
        );
    }
}
