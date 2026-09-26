//! Names a menu lists apart that differ only in punctuation or in how "and" or "the" is written.

use std::collections::{HashMap, HashSet};

use crate::index::{Track, fold};

#[derive(Clone, Debug, Default)]
pub struct Spelling {
    /// Each group's forms, the reference first and the rest commonest first.
    groups: Vec<Vec<Form>>,
    /// The group each grouped form is in, by its folded form.
    grouped: HashMap<String, usize>,
    /// The tracks carrying a minority form, by their index in the library.
    minor_on: HashSet<usize>,
}

#[derive(Clone, Debug)]
struct Form {
    folded: String,
    written: String,
    files: usize,
}

/// Every form of one list of names, counted over the files.
#[derive(Default)]
struct Tally {
    /// The form each spelling folds to, so a spelling seen again is not folded twice.
    by_raw: HashMap<String, Option<usize>>,
    by_folded: HashMap<String, usize>,
    folded: Vec<String>,
    files: Vec<usize>,
    spellings: Vec<HashMap<String, usize>>,
}

impl Tally {
    fn form(&mut self, written: &str) -> Option<usize> {
        if let Some(form) = self.by_raw.get(written) {
            return *form;
        }
        let folded = fold(written);
        let form = match (folded.is_empty(), self.by_folded.get(&folded)) {
            (true, _) => None,
            (false, Some(form)) => Some(*form),
            (false, None) => {
                let form = self.folded.len();
                self.by_folded.insert(folded.clone(), form);
                self.folded.push(folded);
                self.files.push(0);
                self.spellings.push(HashMap::new());
                Some(form)
            }
        };
        self.by_raw.insert(written.to_owned(), form);
        form
    }

    fn count<'a>(&mut self, names: impl IntoIterator<Item = &'a str>) {
        let mut seen: Vec<usize> = Vec::new();
        for written in names {
            let Some(form) = self.form(written) else {
                continue;
            };
            *self.spellings[form].entry(written.to_owned()).or_default() += 1;
            if !seen.contains(&form) {
                seen.push(form);
                self.files[form] += 1;
            }
        }
    }

    fn known<'a>(&self, names: impl IntoIterator<Item = &'a str>) -> impl Iterator<Item = usize> {
        names
            .into_iter()
            .filter_map(|written| self.by_raw.get(written).copied().flatten())
    }
}

impl Spelling {
    pub fn of<'a, N>(tracks: &'a [Track], names: impl Fn(&'a Track) -> N) -> Self
    where
        N: IntoIterator<Item = &'a str>,
    {
        let mut tally = Tally::default();
        for track in tracks {
            tally.count(names(track));
        }
        let groups = groups(&tally);
        let grouped: HashMap<String, usize> = groups
            .iter()
            .enumerate()
            .flat_map(|(at, forms)| forms.iter().map(move |form| (form.folded.clone(), at)))
            .collect();
        let minor: HashSet<usize> = groups
            .iter()
            .flat_map(|forms| &forms[1..])
            .filter_map(|form| tally.by_folded.get(&form.folded).copied())
            .collect();
        let mut minor_on = HashSet::new();
        for (at, track) in tracks.iter().enumerate() {
            if tally.known(names(track)).any(|form| minor.contains(&form)) {
                minor_on.insert(at);
            }
        }
        Self {
            groups,
            grouped,
            minor_on,
        }
    }

    /// Whether this name is written the way fewer files write it.
    pub fn minor(&self, name: &str) -> bool {
        let folded = fold(name);
        self.grouped
            .get(&folded)
            .is_some_and(|at| self.groups[*at][0].folded != folded)
    }

    pub fn minor_on(&self, track: usize) -> bool {
        self.minor_on.contains(&track)
    }

    /// The other spellings of this name, each with the files that write it, commonest first.
    pub fn others(&self, name: &str) -> Vec<(String, usize)> {
        let folded = fold(name);
        self.grouped
            .get(&folded)
            .map(|at| {
                self.groups[*at]
                    .iter()
                    .filter(|form| form.folded != folded)
                    .map(|form| (form.written.clone(), form.files))
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn groups(tally: &Tally) -> Vec<Vec<Form>> {
    let mut by_key: HashMap<String, Vec<usize>> = HashMap::new();
    for (at, folded) in tally.folded.iter().enumerate() {
        let key = loose(folded);
        if !key.is_empty() {
            by_key.entry(key).or_default().push(at);
        }
    }
    let mut groups: Vec<Vec<Form>> = by_key
        .into_values()
        .filter(|forms| forms.len() > 1)
        .map(|forms| {
            let mut forms: Vec<Form> = forms
                .into_iter()
                .map(|at| Form {
                    folded: tally.folded[at].clone(),
                    written: most_written(&tally.spellings[at]),
                    files: tally.files[at],
                })
                .collect();
            forms.sort_by(|one, other| {
                other
                    .files
                    .cmp(&one.files)
                    .then_with(|| one.folded.cmp(&other.folded))
            });
            forms
        })
        .collect();
    groups.sort_by(|one, other| one[0].folded.cmp(&other[0].folded));
    groups
}

fn most_written(spellings: &HashMap<String, usize>) -> String {
    spellings
        .iter()
        .max_by_key(|(spelling, times)| (*times, std::cmp::Reverse(*spelling)))
        .map(|(spelling, _)| spelling.clone())
        .unwrap_or_default()
}

fn loose(folded: &str) -> String {
    let anded = folded.replace('&', "and");
    let words: Vec<&str> = anded
        .split(' ')
        .map(|word| match word {
            "n" | "'n'" => "and",
            other => other,
        })
        .collect();
    let joined = words.join(" ");
    joined
        .strip_prefix("the ")
        .unwrap_or(&joined)
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::index::{Library, Scanned};
    use crate::tags::FileTags;

    fn genred(genres: &[&[&str]]) -> Library {
        let files: Vec<Scanned> = genres
            .iter()
            .enumerate()
            .map(|(at, genres)| Scanned {
                path: Path::new("/music").join(format!("{at}.flac")),
                relative: PathBuf::from(format!("{at}.flac")),
                tags: FileTags {
                    title: Some(format!("Track {at}")),
                    genres: genres.iter().map(|one| (*one).to_owned()).collect(),
                    ..FileTags::default()
                },
                properties: Default::default(),
                size: 1,
                artwork: None,
            })
            .collect();
        Library::build("Music".to_owned(), &files)
    }

    fn of_genres(genres: &[&[&str]]) -> Spelling {
        let library = genred(genres);
        Spelling::of(library.tracks(), |track| {
            track.genres.iter().map(String::as_str)
        })
    }

    fn grouped(spelling: &Spelling) -> Vec<Vec<&str>> {
        spelling
            .groups
            .iter()
            .map(|forms| forms.iter().map(|form| form.written.as_str()).collect())
            .collect()
    }

    fn one_file_each(names: &[&str]) -> Spelling {
        let files: Vec<[&str; 1]> = names.iter().map(|name| [*name]).collect();
        let files: Vec<&[&str]> = files.iter().map(|one| &one[..]).collect();
        of_genres(&files)
    }

    #[test]
    fn the_spellings_measured_on_a_library_are_grouped() {
        for names in [
            &["Drum & Bass", "Drum and Bass", "Drum n Bass"][..],
            &["Afro Beat", "Afrobeat"],
            &["Dance Hall", "Dancehall"],
            &["Hip-Hop", "hiphop"],
            &["MUNGOS HI FI", "Mungo's Hi-Fi", "Mungo's hifi"],
            &["Booker T. & The M.G.s", "Booker T. & The MGs"],
            &["Notorious B.I.G", "The Notorious B.I.G."],
            &["T.O.K", "T.O.K.", "Tok"],
        ] {
            let spelling = one_file_each(names);
            let mut found = grouped(&spelling);
            for group in &mut found {
                group.sort_unstable();
            }
            let mut expected = names.to_vec();
            expected.sort_unstable();
            assert_eq!(found, [expected], "{names:?}");
        }
    }

    #[test]
    fn names_apart_in_case_or_accents_alone_are_one_entry_already_and_no_group() {
        let spelling = one_file_each(&["Electronica", "ELECTRONICA", "Électronica"]);
        assert!(grouped(&spelling).is_empty());
    }

    #[test]
    fn names_in_other_scripts_keep_their_letters_and_never_merge_with_each_other() {
        let spelling = one_file_each(&["Кино", "Kino", "Ελευθερία", "坂本龍一", "坂本", "فيروز"]);
        assert!(grouped(&spelling).is_empty());
        let punctuated = one_file_each(&["Кино", "КИНО!"]);
        assert_eq!(grouped(&punctuated), [["Кино", "КИНО!"]]);
    }

    #[test]
    fn the_form_on_most_files_is_the_reference_and_a_tie_goes_to_the_first_alphabetically() {
        let spelling = of_genres(&[
            &["Drum n Bass"],
            &["Drum & Bass"],
            &["Drum & Bass"],
            &["Drum and Bass", "Drum & Bass"],
        ]);
        assert_eq!(
            grouped(&spelling),
            [["Drum & Bass", "Drum and Bass", "Drum n Bass"]]
        );
        assert!(!spelling.minor("drum & bass"));
        assert!(spelling.minor("Drum n Bass"));
        assert_eq!(
            spelling.others("Drum n Bass"),
            [
                ("Drum & Bass".to_owned(), 3),
                ("Drum and Bass".to_owned(), 1)
            ]
        );
        assert!(spelling.minor_on(0) && spelling.minor_on(3));
        assert!(!spelling.minor_on(1));

        let tied = of_genres(&[&["Afrobeat"], &["Afro Beat"]]);
        assert_eq!(grouped(&tied), [["Afro Beat", "Afrobeat"]]);
        assert!(tied.minor("Afrobeat"));
    }

    #[test]
    fn a_leading_the_and_every_way_of_writing_and_are_one_name() {
        for names in [
            &["The Clash", "Clash"][..],
            &[
                "Rock & Roll",
                "Rock and Roll",
                "Rock n Roll",
                "Rock 'n' Roll",
            ],
            &["Salt-N-Pepa", "Salt 'n' Pepa"],
            &["R&B", "R and B"],
        ] {
            assert_eq!(
                grouped(&one_file_each(names))[0].len(),
                names.len(),
                "{names:?}"
            );
        }
    }

    #[test]
    fn a_form_is_shown_as_it_is_most_written_and_a_file_counts_once_per_form() {
        let spelling = of_genres(&[
            &["Hip-Hop", "hip-hop"],
            &["hip-hop"],
            &["hiphop"],
            &["hiphop"],
            &["HIPHOP"],
        ]);
        assert_eq!(
            spelling.others("Hip-Hop"),
            [("hiphop".to_owned(), 3)],
            "three files, two of them written hiphop"
        );
        assert_eq!(spelling.others("hiphop"), [("hip-hop".to_owned(), 2)]);
    }

    #[test]
    fn a_name_of_punctuation_alone_and_a_name_in_no_group_are_left_alone() {
        let spelling = of_genres(&[&["!!!"], &["???"], &["Jazz"]]);
        assert!(grouped(&spelling).is_empty());
        assert!(!spelling.minor("!!!"));
        assert!(spelling.others("Jazz").is_empty());
    }
}
