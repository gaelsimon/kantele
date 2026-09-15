//! What a pass and an index build refused.

use serde::{Deserialize, Serialize};

/// Kept per cause; the rest are only counted.
const KEPT: usize = 100;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Origin {
    Pass,
    Index,
}

impl Origin {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Index => "index",
        }
    }
}

/// Variant order is listing order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Cause {
    UnreadableFolder,
    UnreadableFile,
    UnreadablePlaylist,
    UnreadableRow,
    MissingEntry,
    UnpublishedPlaylist,
    RepeatedEntry,
    AlbumKeyedOnPath,
    TrackKeyedOnPath,
    PlaceholderCredit,
}

impl Cause {
    pub const ALL: &'static [Self] = &[
        Self::UnreadableFolder,
        Self::UnreadableFile,
        Self::UnreadablePlaylist,
        Self::UnreadableRow,
        Self::MissingEntry,
        Self::UnpublishedPlaylist,
        Self::RepeatedEntry,
        Self::AlbumKeyedOnPath,
        Self::TrackKeyedOnPath,
        Self::PlaceholderCredit,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::UnreadableFolder => "unreadable-folder",
            Self::UnreadableFile => "unreadable-file",
            Self::UnreadablePlaylist => "unreadable-playlist",
            Self::UnreadableRow => "unreadable-row",
            Self::MissingEntry => "missing-entry",
            Self::UnpublishedPlaylist => "unpublished-playlist",
            Self::RepeatedEntry => "repeated-entry",
            Self::AlbumKeyedOnPath => "album-keyed-on-path",
            Self::TrackKeyedOnPath => "track-keyed-on-path",
            Self::PlaceholderCredit => "placeholder-credit",
        }
    }

    /// An unreadable row is recounted every pass, never carried forward.
    pub fn localised(self) -> bool {
        !matches!(self, Self::UnreadableRow)
    }

    /// Whether the subject is a path under the content root, which is what puts a refusal in a
    /// folder.
    pub fn names_a_path(self) -> bool {
        !matches!(self, Self::UnreadableRow | Self::AlbumKeyedOnPath)
    }

    /// A fault of a playlist file, not of the folder it sits in: rippers leave `.m3u` files inside
    /// album folders.
    pub fn about_playlists(self) -> bool {
        matches!(
            self,
            Self::UnreadablePlaylist
                | Self::MissingEntry
                | Self::UnpublishedPlaylist
                | Self::RepeatedEntry
        )
    }

    /// The other three are how a library is tagged, not faults in it.
    pub fn is_problem(self) -> bool {
        !matches!(
            self,
            Self::AlbumKeyedOnPath | Self::TrackKeyedOnPath | Self::PlaceholderCredit
        )
    }

    pub fn origin(self) -> Origin {
        match self {
            Self::UnreadableFolder
            | Self::UnreadableFile
            | Self::UnreadablePlaylist
            | Self::UnreadableRow => Origin::Pass,
            Self::MissingEntry
            | Self::UnpublishedPlaylist
            | Self::RepeatedEntry
            | Self::AlbumKeyedOnPath
            | Self::TrackKeyedOnPath
            | Self::PlaceholderCredit => Origin::Index,
        }
    }

    /// The field and its state, the way a form labels one. Never a sentence.
    pub fn label(self) -> &'static str {
        match self {
            Self::UnreadableFolder => "Folder unreadable",
            Self::UnreadableFile => "Unreadable",
            Self::UnreadablePlaylist => "Playlist unreadable",
            Self::UnreadableRow => "Not in the saved index",
            Self::MissingEntry => "Playlist link broken",
            Self::UnpublishedPlaylist => "Playlist empty",
            Self::RepeatedEntry => "Playlist link repeated",
            Self::AlbumKeyedOnPath => "Album tags identical",
            Self::TrackKeyedOnPath => "Track tags identical",
            Self::PlaceholderCredit => "Album artist not set",
        }
    }

    /// What the count counts.
    pub fn subject(self, count: usize) -> &'static str {
        let (one, many) = match self {
            Self::UnreadableFolder => ("folder", "folders"),
            Self::UnreadableFile | Self::UnreadableRow => ("file", "files"),
            Self::UnreadablePlaylist | Self::UnpublishedPlaylist => ("playlist", "playlists"),
            Self::MissingEntry | Self::RepeatedEntry => ("link", "links"),
            Self::AlbumKeyedOnPath => ("album", "albums"),
            Self::TrackKeyedOnPath | Self::PlaceholderCredit => ("track", "tracks"),
        };
        if count == 1 { one } else { many }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Refusal {
    /// A path relative to the content root where there is one.
    pub subject: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct Kept {
    held: Vec<Refusal>,
    total: usize,
}

/// What fell in one folder, by cause.
pub type Tally = std::collections::BTreeMap<Cause, usize>;

#[derive(Clone, Debug, Default)]
pub struct Refusals {
    by_cause: std::collections::BTreeMap<Cause, Kept>,
    /// What each folder refused and for what, counted as the refusals are made, so a tally is right
    /// past the hundred each cause keeps.
    by_folder: std::collections::BTreeMap<String, Tally>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Reported {
    pub cause: Cause,
    pub origin: Origin,
    pub says: &'static str,
    pub total: usize,
    pub shown: Vec<Refusal>,
}

impl Refusals {
    pub fn refuse(&mut self, cause: Cause, subject: impl Into<String>, detail: Option<String>) {
        let subject = subject.into();
        let folder = folder_of(cause, &subject);
        self.refused_in(cause, subject, detail, folder);
    }

    /// The same, for a refusal whose subject names no path but which happened somewhere.
    pub fn refuse_in(
        &mut self,
        cause: Cause,
        subject: impl Into<String>,
        detail: Option<String>,
        folder: impl Into<String>,
    ) {
        self.refused_in(cause, subject.into(), detail, Some(folder.into()));
    }

    fn refused_in(
        &mut self,
        cause: Cause,
        subject: String,
        detail: Option<String>,
        folder: Option<String>,
    ) {
        if let Some(folder) = folder {
            *self
                .by_folder
                .entry(folder)
                .or_default()
                .entry(cause)
                .or_default() += 1;
        }
        let kept = self.by_cause.entry(cause).or_default();
        kept.total += 1;
        if kept.held.len() < KEPT {
            kept.held.push(Refusal { subject, detail });
        }
    }

    /// What each folder refused and for what, by its path relative to the content root.
    pub fn by_folder(&self) -> &std::collections::BTreeMap<String, Tally> {
        &self.by_folder
    }

    pub fn refuse_counted(&mut self, cause: Cause, count: usize) {
        if count > 0 {
            self.by_cause.entry(cause).or_default().total += count;
        }
    }

    pub fn is_empty(&self) -> bool {
        self.total() == 0
    }

    pub fn total(&self) -> usize {
        self.by_cause.values().map(|kept| kept.total).sum()
    }

    pub fn total_of(&self, cause: Cause) -> usize {
        self.by_cause.get(&cause).map_or(0, |kept| kept.total)
    }

    pub fn held(&self, cause: Cause) -> &[Refusal] {
        self.by_cause
            .get(&cause)
            .map_or(&[], |kept| kept.held.as_slice())
    }

    /// What was kept for one cause under one folder, with how many fell there in all. Only the first
    /// hundred of each cause are kept, so a deep folder can count and show nothing.
    pub fn held_under(&self, cause: Cause, folder: &str) -> Vec<&Refusal> {
        self.held(cause)
            .iter()
            .filter(|refusal| {
                folder_of(cause, &refusal.subject)
                    .is_some_and(|at| at == folder || at.starts_with(&with_slash(folder)))
            })
            .collect()
    }

    pub fn reported(&self) -> Vec<Reported> {
        Cause::ALL
            .iter()
            .filter(|cause| self.total_of(**cause) > 0)
            .map(|cause| Reported {
                cause: *cause,
                origin: cause.origin(),
                says: cause.label(),
                total: self.total_of(*cause),
                shown: self.held(*cause).to_vec(),
            })
            .collect()
    }

    pub fn all_from(&self, origin: Origin) -> bool {
        self.by_cause.keys().all(|cause| cause.origin() == origin)
    }

    pub fn absorb(&mut self, other: Self) {
        for (cause, kept) in other.by_cause {
            let mine = self.by_cause.entry(cause).or_default();
            mine.total += kept.total;
            let room = KEPT.saturating_sub(mine.held.len());
            mine.held.extend(kept.held.into_iter().take(room));
        }
        for (folder, tally) in other.by_folder {
            let mine = self.by_folder.entry(folder).or_default();
            for (cause, count) in tally {
                *mine.entry(cause).or_default() += count;
            }
        }
    }

    /// From a truncated record the total can only read high.
    pub fn carried_forward(
        previous: &Self,
        superseded: impl Fn(Cause, &str) -> bool,
        walked: impl Fn(&str) -> bool,
    ) -> Self {
        let mut carried = Self::default();
        for (cause, kept) in &previous.by_cause {
            if cause.origin() != Origin::Pass || !cause.localised() {
                continue;
            }
            let held: Vec<Refusal> = kept
                .held
                .iter()
                .filter(|refusal| !superseded(*cause, &refusal.subject))
                .cloned()
                .collect();
            let dropped = kept.held.len() - held.len();
            let total = kept.total.saturating_sub(dropped);
            if total > 0 {
                carried.by_cause.insert(*cause, Kept { held, total });
            }
        }
        carried.by_folder = previous
            .by_folder
            .iter()
            .filter(|(folder, _)| !walked(folder))
            .map(|(folder, tally)| (folder.clone(), tally.clone()))
            .collect();
        carried
    }
}

/// The folder a refusal falls in, or nothing where its subject names no path. The top of the
/// library is spelled as the empty string.
fn with_slash(folder: &str) -> String {
    match folder.is_empty() {
        true => String::new(),
        false => format!("{folder}/"),
    }
}

fn folder_of(cause: Cause, subject: &str) -> Option<String> {
    if !cause.names_a_path() {
        return None;
    }
    if cause == Cause::UnreadableFolder {
        return Some(subject.to_owned());
    }
    Some(match subject.rfind('/') {
        Some(cut) => subject[..cut].to_owned(),
        None => String::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_keeps_the_first_hundred_of_a_cause_and_the_real_total() {
        let mut refusals = Refusals::default();
        for at in 0..250 {
            refusals.refuse(Cause::MissingEntry, "list.m3u", Some(format!("{at}")));
        }
        assert_eq!(refusals.total_of(Cause::MissingEntry), 250);
        assert_eq!(refusals.held(Cause::MissingEntry).len(), KEPT);
        let reported = refusals.reported();
        assert_eq!(reported.len(), 1);
        assert_eq!(reported[0].total, 250);
        assert_eq!(reported[0].shown.len(), KEPT);
    }

    #[test]
    fn a_clean_library_costs_nothing() {
        let refusals = Refusals::default();
        assert!(refusals.is_empty());
        assert!(refusals.reported().is_empty());
    }

    #[test]
    fn absorbing_adds_the_totals_and_keeps_the_bound() {
        let mut left = Refusals::default();
        let mut right = Refusals::default();
        for at in 0..80 {
            left.refuse(Cause::UnreadableFile, format!("a/{at}.mp3"), None);
            right.refuse(Cause::UnreadableFile, format!("b/{at}.mp3"), None);
        }
        left.absorb(right);
        assert_eq!(left.total_of(Cause::UnreadableFile), 160);
        assert_eq!(left.held(Cause::UnreadableFile).len(), KEPT);
    }

    #[test]
    fn a_partial_pass_keeps_what_it_did_not_walk_and_drops_what_it_did() {
        let mut previous = Refusals::default();
        previous.refuse(Cause::UnreadableFile, "a/1.mp3", None);
        previous.refuse(Cause::UnreadableFile, "b/1.mp3", None);
        previous.refuse(Cause::MissingEntry, "a/list.m3u", None);

        let carried = Refusals::carried_forward(
            &previous,
            |_, subject| subject.starts_with("a/"),
            |folder| folder.starts_with('a'),
        );
        assert_eq!(carried.total_of(Cause::UnreadableFile), 1);
        assert_eq!(carried.held(Cause::UnreadableFile)[0].subject, "b/1.mp3");
        assert_eq!(
            carried.total_of(Cause::MissingEntry),
            0,
            "an index-built refusal is derived afresh every time and is never carried"
        );
    }

    #[test]
    fn a_row_the_store_could_not_read_is_counted_afresh_rather_than_carried() {
        let mut previous = Refusals::default();
        previous.refuse_counted(Cause::UnreadableRow, 12);
        let carried = Refusals::carried_forward(&previous, |_, _| false, |_| false);
        assert_eq!(
            carried.total_of(Cause::UnreadableRow),
            0,
            "every pass counts the whole store, so carrying one forward counts it twice"
        );
    }

    #[test]
    fn every_cause_says_where_it_came_from_and_is_listed_once() {
        let mut seen: Vec<&str> = Cause::ALL.iter().map(|cause| cause.as_str()).collect();
        let named = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), named, "two causes share a name on the wire");

        let (walked, derived): (Vec<Cause>, Vec<Cause>) = Cause::ALL
            .iter()
            .partition(|cause| cause.origin() == Origin::Pass);
        assert!(!walked.is_empty(), "nothing would ever be carried forward");
        assert!(
            !derived.is_empty(),
            "a start from the store would answer nothing"
        );
        assert_eq!(walked.len() + derived.len(), Cause::ALL.len());
    }

    #[test]
    fn a_record_can_say_which_half_it_belongs_to() {
        let mut walked = Refusals::default();
        walked.refuse(Cause::UnreadableFile, "a.mp3", None);
        assert!(walked.all_from(Origin::Pass));
        assert!(!walked.all_from(Origin::Index));

        walked.refuse(Cause::MissingEntry, "a.m3u", None);
        assert!(
            !walked.all_from(Origin::Pass),
            "one cause on the wrong side is a number the status counts twice"
        );
    }

    #[test]
    fn a_cause_that_is_only_counted_says_so_rather_than_looking_truncated() {
        let mut refusals = Refusals::default();
        refusals.refuse_counted(Cause::UnreadableRow, 4);
        let reported = refusals.reported();
        let [reported] = reported.as_slice() else {
            panic!("one cause")
        };
        assert_eq!(reported.total, 4);
        assert!(
            reported.shown.is_empty() && !Cause::UnreadableRow.localised(),
            "an empty list is a defect for every cause that names paths, and this one does not"
        );
    }
}
