//! Playlist files, parsed and resolved against the library's tracks.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use unicode_normalization::UnicodeNormalization;

use crate::index::fold;
use crate::index::identity;
use crate::index::library::Track;
use crate::index::refusals::{Cause, Refusals};
use crate::index::roots::Roots;
use crate::index::scan::{Fingerprint, Found, Walked};
use crate::index::store::Cache;
use crate::object::ObjectId;

/// Above this a playlist is refused unread.
const LARGEST: u64 = 8 * 1024 * 1024;

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Contents {
    pub title: String,
    pub entries: Vec<Entry>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Entry {
    /// Relative to the content root.
    pub target: PathBuf,
    pub display: Option<String>,
}

pub fn is_playlist(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".m3u", ".m3u8", ".pls"]
        .iter()
        .any(|extension| lower.ends_with(extension))
}

pub fn read(roots: impl Into<Roots>, path: &Path) -> std::io::Result<Contents> {
    let roots = roots.into();
    let bytes = read_bounded(path)?;
    let text = decode(bytes);
    let relative = roots.relative_or_self(path);
    let folder = relative.parent().unwrap_or(Path::new("")).to_path_buf();
    let stem = relative
        .file_stem()
        .map(|stem| fold::nfc(&stem.to_string_lossy()))
        .unwrap_or_default();
    Ok(match is_pls(path) {
        true => parse_pls(&text, &roots, &folder, stem),
        false => parse_m3u(&text, &roots, &folder, stem),
    })
}

#[derive(Clone, Debug)]
pub struct Scanned {
    pub relative: PathBuf,
    pub fingerprint: Fingerprint,
    pub contents: Contents,
}

pub fn read_all(
    roots: impl Into<Roots>,
    walked: &Walked,
    cache: &Cache,
    underway: &crate::index::scan::Underway,
) -> (Vec<Scanned>, Refusals) {
    let roots = &roots.into();
    let mut read = Vec::with_capacity(walked.playlists.len());
    let mut refusals = Refusals::default();
    for found in &walked.playlists {
        if underway.stopping.asked() {
            break;
        }
        match one(roots, found, cache) {
            Ok(scanned) => read.push(scanned),
            Err((relative, error)) => {
                refusals.refuse(Cause::UnreadablePlaylist, relative, Some(error))
            }
        }
    }
    (read, refusals)
}

fn one(roots: &Roots, found: &Found, cache: &Cache) -> Result<Scanned, (String, String)> {
    let relative = roots.relative_or_self(&found.path);
    let contents = match cache.playlist(&relative, found.fingerprint) {
        Some(contents) => contents,
        None => match read(roots, &found.path) {
            Ok(contents) => contents,
            Err(error) => {
                tracing::warn!(path = %found.path.display(), %error, "skipping playlist");
                return Err((fold::path(&relative), format!("{error:#}")));
            }
        },
    };
    Ok(Scanned {
        relative,
        fingerprint: found.fingerprint,
        contents,
    })
}

#[derive(Clone, Debug)]
pub struct Playlist {
    pub id: ObjectId,
    pub relative: String,
    pub title: String,
    /// Indices into [`crate::index::Library::tracks`].
    pub tracks: Vec<usize>,
}

/// Playlists, aliases indexed by track, and refusals.
pub type Resolved = (Vec<Playlist>, Vec<Vec<String>>, Refusals);

pub fn resolve_all(playlists: &[Scanned], tracks: &[Track]) -> Resolved {
    // Empty aliases are fine: `Searchables` reads a missing row as none.
    if playlists.is_empty() {
        return (Vec::new(), Vec::new(), Refusals::default());
    }
    let mut aliases: Vec<Vec<String>> = vec![Vec::new(); tracks.len()];
    let by_path: HashMap<&str, usize> = tracks
        .iter()
        .enumerate()
        .map(|(at, track)| (track.relative.as_str(), at))
        .collect();
    let mut by_key: Option<HashMap<String, usize>> = None;

    let mut resolved: Vec<Playlist> = Vec::with_capacity(playlists.len());
    let mut refusals = Refusals::default();
    let mut unresolved = 0usize;
    for found in playlists {
        let relative = &fold::path(&found.relative);
        let mut members: Vec<usize> = Vec::with_capacity(found.contents.entries.len());
        let mut held: HashSet<usize> = HashSet::with_capacity(found.contents.entries.len());
        for entry in &found.contents.entries {
            let Some(at) = entry
                .target
                .to_str()
                .and_then(|path| named(path, &by_path, &mut by_key, tracks))
            else {
                unresolved += 1;
                refusals.refuse(Cause::MissingEntry, relative, Some(names(entry)));
                continue;
            };
            // A repeated track is dropped: clients index children by identifier.
            if held.insert(at) {
                members.push(at);
            } else {
                refusals.refuse(Cause::RepeatedEntry, relative, Some(names(entry)));
            }
            if let Some(display) = &entry.display
                && !aliases[at].contains(display)
            {
                aliases[at].push(display.clone());
            }
        }
        if members.is_empty() {
            refusals.refuse(
                Cause::UnpublishedPlaylist,
                relative,
                Some(format!(
                    "{} entries, none of them a file this library holds",
                    found.contents.entries.len()
                )),
            );
            continue;
        }
        resolved.push(Playlist {
            id: identity::playlist(relative),
            relative: relative.to_owned(),
            title: found.contents.title.clone(),
            tracks: members,
        });
    }
    disambiguate(&mut resolved);
    resolved.sort_by(|left, right| {
        (fold(&left.title), &left.relative).cmp(&(fold(&right.title), &right.relative))
    });
    if unresolved > 0 {
        tracing::info!(
            entries = unresolved,
            playlists = playlists.len(),
            "playlist entries naming a file this library does not hold"
        );
    }
    (resolved, aliases, refusals)
}

fn names(entry: &Entry) -> String {
    match &entry.display {
        Some(display) => format!("{}  ({display})", entry.target.display()),
        None => entry.target.display().to_string(),
    }
}

fn named(
    path: &str,
    by_path: &HashMap<&str, usize>,
    by_key: &mut Option<HashMap<String, usize>>,
    tracks: &[Track],
) -> Option<usize> {
    if let Some(at) = by_path.get(path) {
        return Some(*at);
    }
    by_key
        .get_or_insert_with(|| keyed(tracks))
        .get(&path_key(path))
        .copied()
}

fn keyed(tracks: &[Track]) -> HashMap<String, usize> {
    let mut keyed = HashMap::with_capacity(tracks.len());
    for (at, track) in tracks.iter().enumerate() {
        keyed.entry(path_key(&track.relative)).or_insert(at);
    }
    keyed
}

fn path_key(path: &str) -> String {
    path.to_lowercase().nfc().collect()
}

fn disambiguate(playlists: &mut [Playlist]) {
    let mut seen: HashMap<String, usize> = HashMap::new();
    for playlist in playlists.iter() {
        *seen.entry(fold(&playlist.title)).or_default() += 1;
    }
    for playlist in playlists.iter_mut() {
        if seen.get(&fold(&playlist.title)).copied().unwrap_or(0) < 2 {
            continue;
        }
        let Some(folder) = holding_folder(&playlist.relative) else {
            continue;
        };
        playlist.title = format!("{} ({folder})", playlist.title);
    }
}

fn holding_folder(relative: &str) -> Option<&str> {
    let folder = &relative[..relative.rfind('/')?];
    Some(match folder.rfind('/') {
        Some(cut) => &folder[cut + 1..],
        None => folder,
    })
}

fn is_pls(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("pls"))
}

fn read_bounded(path: &Path) -> std::io::Result<Vec<u8>> {
    let size = std::fs::metadata(path)?.len();
    if size > LARGEST {
        return Err(std::io::Error::other(format!(
            "{size} bytes is too large to be a playlist"
        )));
    }
    std::fs::read(path)
}

/// UTF-8, falling back to Latin-1.
fn decode(bytes: Vec<u8>) -> String {
    let text = match String::from_utf8(bytes) {
        Ok(text) => text,
        Err(refused) => refused
            .into_bytes()
            .into_iter()
            .map(|byte| byte as char)
            .collect(),
    };
    // A BOM would turn the first directive into a path.
    text.strip_prefix('\u{feff}').unwrap_or(&text).to_owned()
}

fn parse_m3u(text: &str, roots: impl Into<Roots>, folder: &Path, stem: String) -> Contents {
    let roots = &roots.into();
    let mut playlist = Contents {
        title: stem,
        ..Contents::default()
    };
    let mut display: Option<String> = None;
    for line in text.lines().map(str::trim) {
        if line.is_empty() {
            continue;
        }
        if let Some(directive) = line.strip_prefix('#') {
            if let Some(named) = directive.strip_prefix("PLAYLIST:") {
                playlist.title = named.trim().to_owned();
            } else if let Some(information) = directive.strip_prefix("EXTINF:") {
                display = after_comma(information);
            }
            continue;
        }
        if let Some(target) = resolve(roots, folder, line) {
            playlist.entries.push(Entry {
                target,
                display: display.take(),
            });
        } else {
            display = None;
        }
    }
    playlist
}

fn after_comma(information: &str) -> Option<String> {
    let text = information.split_once(',')?.1.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

fn parse_pls(text: &str, roots: impl Into<Roots>, folder: &Path, stem: String) -> Contents {
    let roots = &roots.into();
    let mut title = stem;
    let mut entries: Vec<(u32, Entry)> = Vec::new();
    let mut displays: HashMap<u32, String> = HashMap::new();
    for line in text.lines().map(str::trim) {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let (key, value) = (key.trim(), value.trim());
        if value.is_empty() {
            continue;
        }
        if let Some(at) = numbered(key, "File") {
            if let Some(target) = resolve(roots, folder, value) {
                entries.push((
                    at,
                    Entry {
                        target,
                        display: None,
                    },
                ));
            }
        } else if let Some(at) = numbered(key, "Title") {
            displays.entry(at).or_insert_with(|| value.to_owned());
        } else if key.eq_ignore_ascii_case("PlaylistName") {
            title = value.to_owned();
        }
    }
    entries.sort_by_key(|(at, _)| *at);
    Contents {
        title,
        entries: entries
            .into_iter()
            .map(|(at, entry)| Entry {
                display: displays.get(&at).cloned(),
                ..entry
            })
            .collect(),
    }
}

fn numbered(key: &str, prefix: &str) -> Option<u32> {
    let rest = key.get(..prefix.len())?;
    rest.eq_ignore_ascii_case(prefix)
        .then(|| key[prefix.len()..].parse().ok())
        .flatten()
}

fn resolve(roots: &Roots, folder: &Path, entry: &str) -> Option<PathBuf> {
    let cleaned = unescape(entry);
    let text = cleaned.trim();
    if text.is_empty() {
        return None;
    }
    if text.contains("://") {
        return None;
    }
    // A backslash may be a separator or part of a name, so the literal path is tried first.
    if text.contains('\\') {
        if let Some(literal) = pointed(roots, folder, text)
            && roots.absolute(&literal).is_some_and(|path| path.exists())
        {
            return Some(literal);
        }
        return pointed(roots, folder, &text.replace('\\', "/"));
    }
    pointed(roots, folder, text)
}

fn pointed(roots: &Roots, folder: &Path, text: &str) -> Option<PathBuf> {
    if text.starts_with('/') || drive_letter(text) {
        return roots.relative(Path::new(text));
    }
    normalise(folder, text)
}

fn drive_letter(text: &str) -> bool {
    let mut characters = text.chars();
    characters
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && characters.next() == Some(':')
}

fn unescape(entry: &str) -> String {
    let Some(rest) = entry.strip_prefix("file://") else {
        return entry.to_owned();
    };
    let bytes = rest.as_bytes();
    let mut decoded: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        match hex_pair(bytes, at) {
            Some(byte) => {
                decoded.push(byte);
                at += 3;
            }
            None => {
                decoded.push(bytes[at]);
                at += 1;
            }
        }
    }
    String::from_utf8_lossy(&decoded).into_owned()
}

fn hex_pair(bytes: &[u8], at: usize) -> Option<u8> {
    if bytes.get(at) != Some(&b'%') {
        return None;
    }
    let digit = |offset: usize| (*bytes.get(at + offset)? as char).to_digit(16);
    Some((digit(1)? * 16 + digit(2)?) as u8)
}

fn normalise(folder: &Path, text: &str) -> Option<PathBuf> {
    // A backslash is a separator only where the platform says so: a Unix folder may hold one in
    // its name, and the walk would have spelt it as part of the name.
    let separators: &[char] = if cfg!(windows) { &['/', '\\'] } else { &['/'] };
    let mut parts: Vec<&str> = folder
        .to_str()?
        .split(separators)
        .filter(|part| !part.is_empty())
        .collect();
    for part in text.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                // Above the content root the entry is dropped, not clamped.
                parts.pop()?;
            }
            name => parts.push(name),
        }
    }
    // Joined the way the index spells a relative path, which is what the entry is matched against.
    (!parts.is_empty()).then(|| PathBuf::from(parts.join("/")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m3u(text: &str, folder: &str) -> Contents {
        parse_m3u(
            text,
            Path::new("/music"),
            Path::new(folder),
            "stem".to_owned(),
        )
    }

    fn targets(playlist: &Contents) -> Vec<String> {
        playlist
            .entries
            .iter()
            .map(|entry| entry.target.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn an_extinf_names_the_line_below_it() {
        let playlist = m3u(
            "#EXTM3U\n\
             #EXTINF:346,Ltj X-Perience - And I Love Him\n\
             03. Ltj X-Perience - And I Love Him.flac\n",
            "DJ Kicks",
        );
        assert_eq!(
            targets(&playlist),
            ["DJ Kicks/03. Ltj X-Perience - And I Love Him.flac"]
        );
        assert_eq!(
            playlist.entries[0].display.as_deref(),
            Some("Ltj X-Perience - And I Love Him")
        );
    }

    #[test]
    fn a_playlist_with_no_extinf_still_names_its_files() {
        let playlist = m3u("01.flac\n02.flac\n", "Album");
        assert_eq!(targets(&playlist), ["Album/01.flac", "Album/02.flac"]);
        assert!(playlist.entries.iter().all(|entry| entry.display.is_none()));
    }

    #[test]
    fn an_extinf_belongs_to_one_entry_and_not_to_the_next() {
        let playlist = m3u("#EXTINF:1,Named\na.flac\nb.flac\n", "");
        assert_eq!(playlist.entries[0].display.as_deref(), Some("Named"));
        assert_eq!(playlist.entries[1].display, None);
    }

    #[test]
    fn attributes_before_the_comma_are_not_the_display_text() {
        let playlist = m3u("#EXTINF:-1 tvg-id=\"x\",The Title\na.flac\n", "");
        assert_eq!(playlist.entries[0].display.as_deref(), Some("The Title"));
    }

    #[test]
    fn a_playlist_directive_names_the_playlist() {
        assert_eq!(m3u("#PLAYLIST:Wedding\na.flac\n", "").title, "Wedding");
        assert_eq!(m3u("a.flac\n", "").title, "stem");
    }

    #[test]
    fn a_parent_entry_climbs_out_of_the_playlists_own_folder() {
        let playlist = m3u("../Other/01.flac\n", "Selections");
        assert_eq!(targets(&playlist), ["Other/01.flac"]);
    }

    #[test]
    fn an_entry_above_the_content_root_is_dropped() {
        assert!(
            m3u("../../elsewhere/01.flac\n", "Selections")
                .entries
                .is_empty()
        );
    }

    #[test]
    fn a_backslash_is_a_separator_because_playlists_are_written_on_windows() {
        let playlist = m3u("Disc 1\\01.flac\n", "Album");
        assert_eq!(targets(&playlist), ["Album/Disc 1/01.flac"]);
    }

    #[test]
    fn an_absolute_entry_inside_the_root_is_kept_and_one_outside_is_not() {
        assert_eq!(
            targets(&m3u("/music/Album/01.flac\n", "Whatever")),
            ["Album/01.flac"]
        );
        assert!(m3u("/elsewhere/01.flac\n", "Whatever").entries.is_empty());
    }

    #[test]
    fn a_stream_is_not_a_file_this_server_holds() {
        assert!(m3u("http://example.org/stream\n", "").entries.is_empty());
        assert!(
            m3u("#EXTINF:-1,Radio\nhttp://example.org/stream\na.flac\n", "")
                .entries
                .iter()
                .all(|entry| entry.display.is_none())
        );
    }

    #[test]
    fn a_file_url_is_unescaped() {
        assert_eq!(
            targets(&m3u("file:///music/Caf%C3%A9%20Tacvba/01.flac\n", "")),
            ["Café Tacvba/01.flac"]
        );
    }

    #[test]
    fn a_windows_drive_is_refused_rather_than_read_as_a_relative_path() {
        assert!(m3u("C:\\Music\\01.flac\n", "Album").entries.is_empty());
    }

    #[test]
    fn a_pls_is_ordered_by_its_numbers_and_titled_by_them() {
        let playlist = parse_pls(
            "[playlist]\n\
             NumberOfEntries=2\n\
             File2=b.flac\n\
             Title2=The Second\n\
             File1=a.flac\n\
             Title1=The First\n",
            Path::new("/music"),
            Path::new("Crate"),
            "stem".to_owned(),
        );
        assert_eq!(targets(&playlist), ["Crate/a.flac", "Crate/b.flac"]);
        assert_eq!(playlist.entries[0].display.as_deref(), Some("The First"));
        assert_eq!(playlist.entries[1].display.as_deref(), Some("The Second"));
    }

    #[test]
    fn a_number_written_twice_takes_the_first_title() {
        let playlist = parse_pls(
            "[playlist]\n\
             File1=a.flac\n\
             Title1=The First\n\
             Title1=Edited Later\n",
            Path::new("/music"),
            Path::new("Crate"),
            "stem".to_owned(),
        );
        assert_eq!(playlist.entries[0].display.as_deref(), Some("The First"));
    }

    struct Tree(PathBuf);

    impl Tree {
        fn new(name: &str, files: &[&str]) -> Self {
            let root = std::env::temp_dir().join(format!("kantele-playlist-{name}"));
            let _ = std::fs::remove_dir_all(&root);
            for file in files {
                let path = root.join(file);
                std::fs::create_dir_all(path.parent().expect("a file has a parent"))
                    .expect("creating a test folder");
                std::fs::write(&path, b"not really audio").expect("writing a test file");
            }
            Self(root)
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    // Windows forbids a backslash in a file name, so the case this rule answers cannot arise there.
    #[cfg(unix)]
    fn a_backslash_the_disk_holds_in_a_name_is_not_read_as_a_separator() {
        let tree = Tree::new("backslash", &["Album/AC\\DC - Jailbreak.flac"]);
        let playlist = parse_m3u(
            "AC\\DC - Jailbreak.flac\n",
            &tree.0,
            Path::new("Album"),
            "stem".to_owned(),
        );
        assert_eq!(targets(&playlist), ["Album/AC\\DC - Jailbreak.flac"]);
    }

    #[test]
    fn a_backslash_naming_nothing_is_still_read_as_a_separator() {
        let tree = Tree::new("separator", &["Album/Disc 1/01.flac"]);
        let playlist = parse_m3u(
            "Disc 1\\01.flac\n",
            &tree.0,
            Path::new("Album"),
            "stem".to_owned(),
        );
        // The entry is compared as a path, since Windows spells the separator the other way and
        // reads the backslash as one before this rule ever applies.
        let [entry] = &playlist.entries[..] else {
            panic!("one entry, got {:?}", targets(&playlist))
        };
        assert!(
            tree.0.join(&entry.target).exists(),
            "the entry names the file on the disk: {:?}",
            entry.target
        );
    }

    #[test]
    fn a_byte_order_mark_does_not_turn_the_first_directive_into_a_path() {
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"#EXTM3U\na.flac\n");
        let playlist = parse_m3u(
            &decode(bytes),
            Path::new("/music"),
            Path::new(""),
            "s".to_owned(),
        );
        assert_eq!(targets(&playlist), ["a.flac"]);
    }

    #[test]
    fn an_entry_is_matched_under_its_case_and_its_composition() {
        assert_eq!(
            path_key("Album/01 Rikslyd - Oriented.flac"),
            path_key("album/01 rikslyd - oriented.flac")
        );
        assert_eq!(
            path_key("Havana/02-Homenaje a Benny More\u{301}.flac"),
            path_key("Havana/02-Homenaje a Benny Mor\u{e9}.flac")
        );
    }

    #[test]
    fn a_separator_survives_the_key_that_a_fold_would_have_eaten() {
        assert!(path_key("Rock/Pop/01.flac").contains('/'));
        assert_ne!(path_key("Rock/Pop/01.flac"), path_key("Rock Pop/01.flac"));
    }

    #[test]
    fn latin_1_bytes_are_read_rather_than_refused() {
        let bytes = vec![b'C', b'a', b'f', 0xe9, b'\n'];
        assert_eq!(decode(bytes), "Café\n");
    }

    fn named(relative: &str, title: &str) -> Playlist {
        Playlist {
            id: identity::playlist(relative),
            relative: relative.to_owned(),
            title: title.to_owned(),
            tracks: vec![0],
        }
    }

    #[test]
    fn a_title_two_playlists_share_is_qualified_by_the_folder_holding_it() {
        let mut playlists = [
            named("Kamasi Washington/play.m3u", "play"),
            named("Propellerheads/play.m3u", "play"),
            named("Andsnes/selection.m3u", "selection"),
        ];
        disambiguate(&mut playlists);
        assert_eq!(playlists[0].title, "play (Kamasi Washington)");
        assert_eq!(playlists[1].title, "play (Propellerheads)");
        assert_eq!(
            playlists[2].title, "selection",
            "a title nobody shares is left alone"
        );
    }

    #[test]
    fn a_playlist_at_the_content_root_has_no_folder_to_be_named_by() {
        let mut playlists = [named("play.m3u", "play"), named("Album/play.m3u", "play")];
        disambiguate(&mut playlists);
        assert_eq!(playlists[0].title, "play");
        assert_eq!(playlists[1].title, "play (Album)");
    }

    fn track(relative: &str) -> Track {
        Track {
            id: identity::playlist(relative),
            rule: crate::index::identity::Rule::Path,
            path: PathBuf::from("/music").join(relative),
            relative: relative.to_owned(),
            album_id: None,
            album_inherited: false,
            title_tagged: true,
            compilation: false,
            title: relative.to_owned(),
            artists: Vec::new(),
            album_artists: Vec::new(),
            composers: Vec::new(),
            conductors: Vec::new(),
            album: None,
            genres: Vec::new(),
            date: None,
            track_number: None,
            disc_number: None,
            duration: std::time::Duration::ZERO,
            size: 0,
            mime: "audio/x-flac",
            sample_rate: None,
            bit_depth: None,
            channels: None,
            bitrate_bps: None,
            artwork: None,
            recording_mbid: None,
            date_added: None,
            disc_from_title: false,
            disc_subtitle: None,
            work: None,
            grouping: None,
        }
    }

    fn scanned(relative: &str, entries: &[(&str, Option<&str>)]) -> Scanned {
        Scanned {
            relative: PathBuf::from(relative),
            fingerprint: Fingerprint::UNKNOWN,
            contents: Contents {
                title: "Set".to_owned(),
                entries: entries
                    .iter()
                    .map(|(target, display)| Entry {
                        target: PathBuf::from(target),
                        display: display.map(str::to_owned),
                    })
                    .collect(),
            },
        }
    }

    #[test]
    fn a_track_named_twice_by_one_playlist_is_one_child_of_it() {
        let tracks = [track("a.flac"), track("b.flac")];
        let playlists = [scanned(
            "Set.m3u",
            &[
                ("a.flac", Some("Opener")),
                ("b.flac", None),
                ("a.flac", Some("Reprise")),
            ],
        )];
        let (resolved, aliases, refusals) = resolve_all(&playlists, &tracks);
        assert_eq!(
            resolved[0].tracks,
            [0, 1],
            "the repeat is dropped and the first position is the one kept"
        );
        assert_eq!(
            refusals.total_of(Cause::RepeatedEntry),
            1,
            "and the record says which entry was lost rather than losing it quietly"
        );
        assert_eq!(
            aliases[0],
            ["Opener", "Reprise"],
            "both names it was written under are still searchable"
        );
    }

    #[test]
    fn the_three_extensions_are_playlists_and_nothing_else_is() {
        for name in ["a.m3u", "a.M3U", "a.m3u8", "a.pls"] {
            assert!(is_playlist(name), "{name} should be a playlist");
        }
        for name in ["a.flac", "a.jpg", "a.m3u.bak", "m3u"] {
            assert!(!is_playlist(name), "{name} should not be a playlist");
        }
    }
}
