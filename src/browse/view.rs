//! The browsing view of a library: the axes, the folder tree and the menu settings.

use std::sync::Arc;

use crate::index::{Checks, Coverage, Library};

use super::{Axes, Folders, Recent, Settings, recently_added};

#[derive(Clone, Debug, Default)]
pub struct View {
    axes: Axes,
    folders: Folders,
    /// Settled once here, since a root browse asks for it and the answer sorts the library.
    recent: Vec<Recent>,
    pub settings: Settings,
}

impl View {
    pub fn build(library: &Library) -> Self {
        Self::with_settings(library, Settings::default())
    }

    pub fn with_settings(library: &Library, settings: Settings) -> Self {
        Self {
            axes: Axes::build(library.tracks(), &settings.ignored()),
            folders: Folders::build(library.tracks()),
            recent: recently_added(library, &settings),
            settings,
        }
    }

    pub fn axes(&self) -> &Axes {
        &self.axes
    }

    pub fn folders(&self) -> &Folders {
        &self.folders
    }

    /// What the newest files belong to, newest first.
    pub fn recent(&self) -> &[Recent] {
        &self.recent
    }
}

/// Counted once here because the page asks for them on a timer and each walks every track.
#[derive(Clone, Debug, Default)]
pub struct Counts {
    pub coverage: Coverage,
    /// Tracks no tag axis reaches, which the axes decide, so a settings change moves it.
    pub untagged: usize,
    pub checks: Arc<Checks>,
}

/// A library and the view over it, swapped in as one piece.
pub struct Served {
    pub library: Arc<Library>,
    pub view: View,
    pub counts: Counts,
}

impl Served {
    pub fn new(library: Library, settings: Settings) -> Self {
        let view = View::with_settings(&library, settings);
        let counts = Counts {
            coverage: Coverage::of(&library),
            untagged: super::untagged_count(&library, &view),
            checks: Arc::new(Checks::of(&library)),
        };
        Self {
            library: Arc::new(library),
            view,
            counts,
        }
    }

    pub fn with_settings(&self, settings: Settings) -> Self {
        let view = View::with_settings(&self.library, settings);
        Self {
            counts: Counts {
                coverage: self.counts.coverage.clone(),
                untagged: super::untagged_count(&self.library, &view),
                checks: self.counts.checks.clone(),
            },
            library: self.library.clone(),
            view,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browse::Facet;
    use crate::index::{Scanned, Track};
    use crate::tags::{AudioProperties, FileTags};

    fn library(artists: &[&str]) -> Library {
        let files: Vec<Scanned> = artists
            .iter()
            .enumerate()
            .map(|(n, artist)| Scanned {
                path: std::path::PathBuf::from(format!("/music/{n}.flac")),
                relative: std::path::PathBuf::from(format!("{n}.flac")),
                tags: FileTags {
                    title: Some(format!("Track {n}")),
                    album: Some("Record".to_owned()),
                    artists: vec![(*artist).to_owned()],
                    album_artists: vec![(*artist).to_owned()],
                    track_number: Some(n as u32 + 1),
                    ..FileTags::default()
                },
                properties: AudioProperties::default(),
                size: 1,
                artwork: None,
            })
            .collect();
        Library::build("Music".to_owned(), &files)
    }

    fn values(view: &View, library: &Library) -> Vec<String> {
        let all: Vec<usize> = (0..library.tracks().len()).collect();
        view.axes()
            .distinct(Facet::AllArtists, &all)
            .into_iter()
            .map(|entry| entry.display.to_owned())
            .collect()
    }

    #[test]
    fn the_counts_a_status_reads_are_settled_once_and_agree_with_walking_the_library() {
        let library = library(&["Autechre", "Coil"]);
        let served = Served::new(library, Settings::default());
        assert_eq!(served.counts.coverage, Coverage::of(&served.library));
        assert_eq!(
            served.counts.untagged,
            super::super::untagged(&served.library, &served.view).len()
        );
    }

    #[test]
    fn settings_carried_over_keep_the_coverage_and_count_the_untagged_again() {
        let served = Served::new(library(&["The Fall"]), Settings::default());
        let narrowed = served.with_settings(Settings {
            sort_ignore: vec!["The".to_owned()],
            ..Settings::default()
        });
        assert_eq!(
            narrowed.counts.coverage, served.counts.coverage,
            "the same library, so the same tags"
        );
        assert_eq!(
            narrowed.counts.untagged,
            super::super::untagged(&narrowed.library, &narrowed.view).len(),
            "the axes are built again, so what they reach is counted again"
        );
    }

    #[test]
    fn a_view_shows_the_library_it_was_built_from() {
        let library = library(&["Autechre", "Coil"]);
        let view = View::build(&library);
        assert_eq!(values(&view, &library), ["Autechre", "Coil"]);
    }

    #[test]
    fn a_view_of_a_later_library_does_not_show_the_earlier_one() {
        let first = library(&["Autechre"]);
        let second = library(&["Coil", "Dopplereffekt"]);
        let view = View::build(&second);
        assert_eq!(values(&view, &second), ["Coil", "Dopplereffekt"]);
        assert!(!values(&view, &second).contains(&"Autechre".to_owned()));
        assert_eq!(first.tracks().len(), 1);
    }

    #[test]
    fn settings_reach_the_view_and_the_library_never_carries_them() {
        let library = library(&["Autechre"]);
        let settings = Settings {
            album_threshold: 0,
            alpha_group: Some(7),
            ..Settings::default()
        };
        let view = View::with_settings(&library, settings);
        assert_eq!(view.settings.album_threshold, 0);
        assert_eq!(view.settings.alpha_group, Some(7));
    }

    #[test]
    fn an_article_is_looked_past_when_ordering_unless_the_settings_say_otherwise() {
        let library = library(&["The Beatles", "Cure"]);
        let view = View::build(&library);
        assert_eq!(values(&view, &library), ["The Beatles", "Cure"]);
        let all: Vec<usize> = (0..library.tracks().len()).collect();
        let sorts: Vec<&str> = view
            .axes()
            .distinct(Facet::AllArtists, &all)
            .into_iter()
            .map(|entry| entry.sort)
            .collect();
        assert_eq!(
            sorts,
            ["beatles", "cure"],
            "and the letter follows the same string"
        );

        let literal = View::with_settings(
            &library,
            Settings {
                sort_ignore: Vec::new(),
                ..Settings::default()
            },
        );
        assert_eq!(values(&literal, &library), ["Cure", "The Beatles"]);
    }

    #[test]
    fn a_menu_offers_the_axes_the_settings_name_in_their_order() {
        use crate::browse::{Menu, Position, menu};
        let library = library(&["Autechre", "Coil"]);
        let view = View::with_settings(
            &library,
            Settings {
                album_threshold: 0,
                axes: vec![Facet::AllArtists, Facet::Artist],
                ..Settings::default()
            },
        );
        match menu(&library, &view, &Position::default()).expect("the root is an object") {
            Menu::Facets(offered) => {
                let facets: Vec<Facet> = offered.into_iter().map(|(facet, _, _)| facet).collect();
                assert_eq!(facets, [Facet::AllArtists, Facet::Artist]);
            }
            other => panic!("expected facets, got {other:?}"),
        }
    }

    #[test]
    fn the_folder_tree_comes_from_the_same_tracks() {
        let library = library(&["Autechre", "Coil"]);
        let view = View::build(&library);
        assert!(!view.folders().is_empty());
        let tracks: &[Track] = library.tracks();
        assert_eq!(tracks.len(), 2);
    }
}
