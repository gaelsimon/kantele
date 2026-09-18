//! What the index made of a library, in words for a reader.

use std::path::Path;

use crate::index::artwork::Source;
use crate::index::credits::Credit;
use crate::index::{Album, Cause, IDENTITY_VERSION, Library, Rule, Track, fold};

/// A refusal cause as a form labels a field: the field and its state, never a sentence.
pub fn label(cause: Cause) -> &'static str {
    match cause {
        Cause::UnreadableFolder => "Unreadable folder",
        Cause::UnreadableFile => "Unreadable file",
        Cause::UnreadablePlaylist => "Unreadable playlist",
        Cause::UnreadableRow => "Not in the saved index",
        Cause::LinkedOutside => "Link out of the music folder",
        Cause::MissingEntry => "Broken playlist link",
        Cause::UnpublishedPlaylist => "Empty playlist",
        Cause::RepeatedEntry => "Repeated playlist link",
        Cause::AlbumKeyedOnPath => "Identical album tags",
        Cause::TrackKeyedOnPath => "Identical track tags",
        Cause::PlaceholderCredit => "No album artist",
    }
}

/// What a folder holds: `9,434 tracks, 812 albums`. What is wrong with it is listed apart.
pub fn holds(tracks: usize, albums: usize, folders: usize) -> String {
    match (tracks, folders) {
        (0, 0) => "Empty".to_owned(),
        (0, _) => "No music".to_owned(),
        (1, _) => held_in("1 track", albums),
        (tracks, _) => held_in(&format!("{} tracks", grouped(tracks)), albums),
    }
}

fn held_in(held: &str, albums: usize) -> String {
    match albums {
        0 => format!("{held}, no album"),
        1 => format!("{held}, 1 album"),
        albums => format!("{held}, {} albums", grouped(albums)),
    }
}

/// Digits a reader can take in at a glance.
fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut written = String::with_capacity(digits.len() + digits.len() / 3);
    for (at, digit) in digits.chars().enumerate() {
        if at > 0 && (digits.len() - at).is_multiple_of(3) {
            written.push(',');
        }
        written.push(digit);
    }
    written
}

/// What a folder does not show about an album some of whose tracks are in it: which disc it is,
/// or how much of the album is elsewhere. Nothing where the folder holds all of it.
pub fn album_part(
    here: usize,
    of: usize,
    folders: usize,
    disc: Option<(u32, u32)>,
) -> Option<String> {
    if here >= of {
        return None;
    }
    Some(match disc {
        Some((number, discs)) => format!("Disc {number} of {discs}, {here} of {of} tracks"),
        None => format!("{here} of {of} tracks, in {folders} folders"),
    })
}

/// Why a folder an owner takes for one album shows as two. Leaves only: the counts beside it
/// cover everything below, and this clause covers the folder's own tracks.
pub fn split_by(albums: &[&Album], own_tracks: usize, subfolders: usize) -> Option<&'static str> {
    if subfolders > 0 || albums.len() < 2 || albums.len() * 2 > own_tracks {
        return None;
    }
    let first = fold(&albums[0].title);
    if albums.iter().any(|album| fold(&album.title) != first) {
        return Some("Album titles differ");
    }
    // Tracks are grouped by folder and title, so one folder under one title splits on nothing else.
    Some("Release ids differ")
}

/// Everybody credited, in the order the tags name them. Nothing where nobody is.
pub fn credited(credits: &[Credit]) -> Option<String> {
    let joined = credits
        .iter()
        .map(|credit| credit.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    (!joined.is_empty()).then_some(joined)
}

/// Where the cover a listener sees came from, or nothing where there is none.
pub fn cover_from(track: &Track) -> Option<&'static str> {
    Some(match track.artwork.as_ref()?.source {
        Source::Embedded { .. } => "in the file",
        Source::File(_) => "in the folder",
    })
}

/// What a refusal count counts.
pub fn subject(cause: Cause, count: usize) -> &'static str {
    let (one, many) = match cause {
        Cause::UnreadableFolder => ("folder", "folders"),
        Cause::UnreadableFile | Cause::UnreadableRow => ("file", "files"),
        Cause::LinkedOutside | Cause::MissingEntry | Cause::RepeatedEntry => ("link", "links"),
        Cause::UnreadablePlaylist | Cause::UnpublishedPlaylist => ("playlist", "playlists"),
        Cause::AlbumKeyedOnPath => ("album", "albums"),
        Cause::TrackKeyedOnPath | Cause::PlaceholderCredit => ("track", "tracks"),
    };
    if count == 1 { one } else { many }
}

/// Every album as one tab-separated line: track count, first credit, title.
pub fn write_album_list(library: &Library, path: &Path) -> std::io::Result<()> {
    use std::io::Write;

    let column = |value: &str| -> String {
        value
            .chars()
            .map(|c| if c.is_control() { ' ' } else { c })
            .collect()
    };

    let mut file = std::io::BufWriter::new(std::fs::File::create(path)?);
    for album in library.albums() {
        writeln!(
            file,
            "{}\t{}\t{}",
            album.tracks.len(),
            column(
                album
                    .artists
                    .first()
                    .map(|credit| credit.name.as_str())
                    .unwrap_or_default()
            ),
            column(&album.title),
        )?;
    }
    file.flush()
}

/// What the identity rules made of a real library, in plain text on stdout.
pub fn report(library: &Library) {
    println!("identity version {IDENTITY_VERSION}");
    println!(
        "tracks {}, albums {}, artists {}",
        library.len(),
        library.albums().len(),
        library.artists().len()
    );
    if !library.playlists().is_empty() {
        let entries: usize = library
            .playlists()
            .iter()
            .map(|playlist| playlist.tracks.len())
            .sum();
        println!(
            "playlists {}, naming {entries} tracks",
            library.playlists().len()
        );
    }
    let inherited = library
        .tracks()
        .iter()
        .filter(|track| track.album_inherited)
        .count();
    if inherited > 0 {
        // A rule that guesses has to be falsifiable, so it says how often it guessed.
        println!("tracks whose album was inherited from their folder: {inherited}");
    }
    println!(
        "track identity: {}",
        tally(library.tracks().iter().map(|track| track.rule))
    );
    println!(
        "album identity: {}",
        tally(library.albums().iter().map(|album| album.rule))
    );
    counts(library);
    coverage(library);
    largest_albums(library);
    collisions(library);
}

/// How many tracks carry each tag.
fn coverage(library: &Library) {
    let coverage = crate::index::Coverage::of(library);
    println!("tracks carrying each tag, of {}:", coverage.tracks);
    for (field, count) in coverage.fields() {
        println!("  {field} {count}");
    }
}

/// The counts worth watching, each falsifiable against the library it was run on.
fn counts(library: &Library) {
    let albums = |kept: fn(&&Album) -> bool| library.albums().iter().filter(kept).count();
    println!(
        "albums on several discs: {}",
        albums(|album| album.disc_count > 1)
    );
    println!(
        "albums with no credit: {}",
        albums(|album| album.artists.is_empty())
    );
    println!(
        "tracks belonging to no album: {}",
        library
            .tracks()
            .iter()
            .filter(|track| track.album_id.is_none())
            .count()
    );
}

/// How a grouping rule that merged too much announces itself.
fn largest_albums(library: &Library) {
    let mut largest: Vec<&Album> = library.albums().iter().collect();
    largest.sort_by_key(|album| std::cmp::Reverse(album.tracks.len()));
    println!("largest albums:");
    for album in largest.iter().take(10) {
        println!(
            "  {:4} tracks  {:>3} disc(s)  {}",
            album.tracks.len(),
            album.disc_count,
            album.title
        );
    }
}

/// The albums that fell to the path rung, which means two things claimed one identity.
fn collisions(library: &Library) {
    let collided: Vec<&Album> = library
        .albums()
        .iter()
        .filter(|album| album.rule == Rule::Path)
        .collect();
    if collided.is_empty() {
        return;
    }
    println!("albums keyed on their path, because another album derived the same identity:");
    for album in collided.iter().take(20) {
        println!("  {}", album.title);
    }
}

/// How many things each rule answered for, most first.
fn tally(rules: impl Iterator<Item = Rule>) -> String {
    let mut counts: Vec<(Rule, usize)> = Vec::new();
    for rule in rules {
        match counts.iter_mut().find(|(seen, _)| *seen == rule) {
            Some((_, count)) => *count += 1,
            None => counts.push((rule, 1)),
        }
    }
    counts.sort_by_key(|(_, count)| std::cmp::Reverse(*count));
    counts
        .iter()
        .map(|(rule, count)| format!("{rule} {count}"))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::Scanned;
    use crate::tags::FileTags;
    use std::path::PathBuf;

    fn file(album: &str, artist: &str, title: &str) -> Scanned {
        Scanned {
            path: PathBuf::from("/music").join(album).join(title),
            relative: PathBuf::from(album).join(title),
            tags: FileTags {
                title: Some(title.to_owned()),
                album: Some(album.to_owned()),
                album_artists: vec![artist.to_owned()],
                track_number: Some(1),
                ..FileTags::default()
            },
            properties: crate::tags::AudioProperties::default(),
            size: 1,
            artwork: None,
        }
    }

    #[test]
    fn the_album_list_is_one_line_an_album_that_two_servers_can_be_diffed_on() {
        let library = Library::build(
            "Music".to_owned(),
            &[
                file("Dundunbanza", "Sierra Maestra", "01.flac"),
                file("Dub Landing", "Scientist", "01.flac"),
            ],
        );
        let path = std::env::temp_dir().join("kantele-report-albums.tsv");
        write_album_list(&library, &path).expect("writing the list");
        let mut lines: Vec<String> = std::fs::read_to_string(&path)
            .expect("reading it back")
            .lines()
            .map(str::to_owned)
            .collect();
        lines.sort();
        assert_eq!(
            lines,
            vec![
                "1\tScientist\tDub Landing".to_owned(),
                "1\tSierra Maestra\tDundunbanza".to_owned(),
            ]
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_tag_holding_a_tab_does_not_split_one_album_across_the_columns() {
        // There is a real one on the library this was written against.
        let library = Library::build(
            "Music".to_owned(),
            &[file("Two\tColumns", "One\u{1}Artist", "01.flac")],
        );
        let path = std::env::temp_dir().join("kantele-report-control.tsv");
        write_album_list(&library, &path).expect("writing the list");
        let written = std::fs::read_to_string(&path).expect("reading it back");
        assert_eq!(written.trim_end(), "1\tOne Artist\tTwo Columns");
        assert_eq!(written.matches('\t').count(), 2, "{written:?}");
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_report_of_an_empty_library_is_a_report_rather_than_a_panic() {
        // This runs on whatever a user points it at, including a folder they have not filled yet.
        report(&Library::default());
        report(&Library::build(
            "Music".to_owned(),
            &[file("Dundunbanza", "Sierra Maestra", "01.flac")],
        ));
    }

    #[test]
    fn a_folder_says_what_it_holds_and_nothing_about_what_is_wrong() {
        assert_eq!(holds(0, 0, 0), "Empty");
        assert_eq!(holds(0, 0, 2), "No music");
        assert_eq!(holds(1, 1, 0), "1 track, 1 album");
        assert_eq!(holds(24, 2, 0), "24 tracks, 2 albums");
        assert_eq!(holds(12, 0, 0), "12 tracks, no album");
        assert_eq!(
            holds(9434, 812, 0),
            "9,434 tracks, 812 albums",
            "a number a reader takes in at a glance"
        );
    }

    #[test]
    fn an_album_a_folder_holds_all_of_says_nothing_about_where_it_is() {
        assert_eq!(album_part(12, 12, 1, None), None);
        assert_eq!(
            album_part(12, 24, 2, Some((2, 2))).as_deref(),
            Some("Disc 2 of 2, 12 of 24 tracks"),
            "the disc is what an owner sees on the shelf"
        );
        assert_eq!(
            album_part(1, 9, 3, None).as_deref(),
            Some("1 of 9 tracks, in 3 folders"),
            "and where no disc numbers them, the folders it is spread over"
        );
    }

    #[test]
    fn a_count_of_one_is_said_in_the_singular() {
        assert_eq!(subject(Cause::UnreadableFile, 1), "file");
        assert_eq!(subject(Cause::UnreadableFile, 2), "files");
        assert_eq!(subject(Cause::MissingEntry, 0), "links");
    }

    #[test]
    fn the_tally_puts_the_commonest_rule_first() {
        let counted = tally(
            [
                Rule::Strings,
                Rule::Path,
                Rule::Strings,
                Rule::ReleaseMbid,
                Rule::Strings,
            ]
            .into_iter(),
        );
        assert!(counted.starts_with("strings 3"), "{counted}");
        assert!(counted.contains("path 1"), "{counted}");
    }
}
