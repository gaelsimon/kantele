//! One level of the menu a device walks, so the page draws what the server builds rather than a
//! copy of the rule. Media is the wire's to carry; this answers the menu down to the tracks.

use serde::{Deserialize, Serialize};

use crate::browse::{self, Menu, Position, Served, root};

#[derive(Debug, Default, Deserialize)]
pub struct Asked {
    /// The position to open, empty for the root.
    #[serde(default)]
    pub at: String,
}

/// What a level is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    /// The root, where the menus the owner chose sit among the ones the library gives.
    Root,
    /// The axes that still narrow the selection.
    Axes,
    /// One axis and its values.
    Values,
    /// One axis split into letter groups.
    Letters,
    /// Nothing narrows further, so the selection itself is offered. The wire carries it.
    Tracks,
}

#[derive(Debug, Serialize)]
pub struct Level {
    pub at: String,
    pub kind: Kind,
    pub entries: Vec<Entry>,
    /// The tracks a selection came down to, where it narrows no further.
    pub tracks: usize,
}

/// One entry of a level.
#[derive(Debug, Serialize)]
pub struct Entry {
    /// What a device browses to reach it.
    pub at: String,
    pub title: String,
    pub children: usize,
    /// A menu the owner chose, as against one the library gives.
    pub chosen: bool,
}

impl Entry {
    fn chosen(at: String, title: String, children: usize) -> Self {
        Self {
            at,
            title,
            children,
            chosen: true,
        }
    }

    fn given(at: String, title: String, children: usize) -> Self {
        Self {
            at,
            title,
            children,
            chosen: false,
        }
    }
}

/// The level at one position, or nothing where the position names no menu of this library.
pub fn level(served: &Served, asked: &Asked) -> Option<Level> {
    let at = if asked.at.is_empty() {
        Position::default()
    } else {
        Position::parse(&asked.at)?
    };
    if at == Position::default() {
        return Some(Level {
            at: at.id(),
            kind: Kind::Root,
            entries: root::entries(&served.library, &served.view)
                .into_iter()
                .map(|entry| {
                    let id = entry.opens.id();
                    match entry.opens {
                        root::Opens::Axis(..) => Entry::chosen(id, entry.title, entry.children),
                        _ => Entry::given(id, entry.title, entry.children),
                    }
                })
                .collect(),
            tracks: 0,
        });
    }

    let (kind, entries, tracks) = match browse::menu(&served.library, &served.view, &at)? {
        Menu::Facets(offered) => (
            Kind::Axes,
            offered
                .into_iter()
                .map(|(facet, to, values)| Entry::chosen(to.id(), facet.title().to_owned(), values))
                .collect(),
            0,
        ),
        Menu::Letters(_, groups) => (
            Kind::Letters,
            groups
                .into_iter()
                .map(|group| {
                    Entry::given(at.at_letter(group.key).id(), group.display, group.values)
                })
                .collect(),
            0,
        ),
        Menu::Values(facet, values) => {
            // The way into the letter index is the first entry, as it is on the wire.
            let letters = browse::index_offered(&at, &values, &served.view.settings);
            let mut entries: Vec<Entry> = letters
                .map(|letters| {
                    Entry::given(at.at_index().id(), browse::INDEX_TITLE.to_owned(), letters)
                })
                .into_iter()
                .collect();
            entries.extend(values.into_iter().map(|value| {
                Entry::given(
                    at.chose(facet, value.digest).id(),
                    value.display.to_owned(),
                    value.tracks,
                )
            }));
            (Kind::Values, entries, 0)
        }
        Menu::Tracks(selected) => (Kind::Tracks, Vec::new(), selected.len()),
    };
    Some(Level {
        at: at.id(),
        kind,
        entries,
        tracks,
    })
}

/// The same, for a shell with no `jq`.
pub fn lines(level: &Level) -> Vec<(String, String)> {
    let mut lines = vec![
        ("at".to_owned(), level.at.clone()),
        (
            "kind".to_owned(),
            serde_json::to_value(level.kind)
                .ok()
                .and_then(|kind| kind.as_str().map(str::to_owned))
                .unwrap_or_default(),
        ),
        ("tracks".to_owned(), level.tracks.to_string()),
    ];
    for (at, entry) in level.entries.iter().enumerate() {
        lines.push((
            format!("entry.{}", at + 1),
            format!(
                "{}\t{}\t{}\t{}",
                entry.at,
                entry.title,
                entry.children,
                if entry.chosen { "chosen" } else { "library" }
            ),
        ));
    }
    lines
}
