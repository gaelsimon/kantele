//! Identifiers for tracks, releases, artists and playlists, and the rule that produced each.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::index::fold;

pub use discs::strip_disc_marker;

mod discs;

use crate::object::ObjectId;
use crate::tags::FileTags;
use discs::folder_disc;

/// Bump whenever a rule below changes.
pub const IDENTITY_VERSION: u16 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    /// `MUSICBRAINZ_RELEASETRACKID`
    ReleaseTrackMbid,
    /// `MUSICBRAINZ_ALBUMID`
    ReleaseMbid,
    ReleaseMbidPropagated,
    ArtistMbid,
    Strings,
    Path,
}

impl Rule {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ReleaseTrackMbid => "release-track-mbid",
            Self::ReleaseMbid => "release-mbid",
            Self::ReleaseMbidPropagated => "release-mbid-propagated",
            Self::ArtistMbid => "artist-mbid",
            Self::Strings => "strings",
            Self::Path => "path",
        }
    }
}

impl std::fmt::Display for Rule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub id: ObjectId,
    pub rule: Rule,
}

const TRACK: &str = "tr-";
const RELEASE: &str = "al-";
const ARTIST: &str = "ar-";
const PLAYLIST: &str = "pl-";

/// The letter a disc carries after its album's identifier and a dot.
pub const DISC: char = 'd';
/// The letter a run of grouped tracks carries on the same terms.
pub const RUN: char = 'g';

/// A disc of an album, named by its number, so a rescan that adds a disc renames no other.
pub fn disc_part(album: &ObjectId, number: Option<u32>) -> ObjectId {
    part(album, DISC, &number.unwrap_or(0).to_string())
}

/// A run of grouped tracks: its disc, its grouping, and the number of the track it opens with.
/// Where the files carry no numbers, `again` counts the runs of that name already seen.
pub fn run_part(
    album: &ObjectId,
    disc: Option<u32>,
    grouping: &str,
    first: Option<u32>,
    again: u32,
) -> ObjectId {
    let mut key = format!(
        "{}{UNIT}{}{UNIT}{}",
        disc.unwrap_or(0),
        fold(grouping),
        first.unwrap_or(0)
    );
    if again > 0 {
        key.push(UNIT);
        key.push_str(&again.to_string());
    }
    part(album, RUN, &short_digest(&key))
}

fn part(album: &ObjectId, kind: char, suffix: &str) -> ObjectId {
    ObjectId::new(format!("{album}.{kind}{suffix}")).expect("an identifier with an ascii suffix")
}

fn short_digest(key: &str) -> String {
    hex::encode(&Sha256::digest(key.as_bytes())[..8])
}

/// The album an identifier names a part of, or nothing where it is not shaped like one.
pub fn part_album(id: &str) -> Option<ObjectId> {
    let (album, suffix) = id.rsplit_once('.')?;
    let mut characters = suffix.chars();
    characters
        .next()
        .filter(|kind| matches!(*kind, DISC | RUN))?;
    let rest = characters.as_str();
    if rest.is_empty() || !rest.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
        return None;
    }
    ObjectId::new(album.to_owned()).ok()
}

/// Field separator: cannot occur inside a tag value.
const UNIT: char = '\u{1f}';

#[derive(Clone, Copy, Debug)]
pub struct FileFacts<'a> {
    pub relative_path: &'a Path,
    pub tags: &'a FileTags,
}

#[derive(Clone, Debug)]
pub struct Release {
    pub id: ObjectId,
    pub rule: Rule,
    pub key: String,
    pub title: String,
    pub musicbrainz_id: Option<String>,
    /// Indices into the input file list.
    pub files: Vec<usize>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Placement {
    /// Index into `Grouping::releases`.
    pub release: Option<usize>,
    pub disc: Option<u32>,
    pub inherited: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Grouping {
    pub releases: Vec<Release>,
    /// One entry per input file, in input order.
    pub placement: Vec<Placement>,
    /// Which folder was awarded each key, for the next pass to honour.
    pub claims: Claims,
}

/// Which folder holds each album key, as the last pass left it. Without it, two folders deriving
/// one key are settled by walk order.
pub type Claims = std::collections::BTreeMap<String, String>;

pub fn group_releases(files: &[FileFacts]) -> Grouping {
    group_releases_holding(files, &Claims::new())
}

pub fn group_releases_holding(files: &[FileFacts], held: &Claims) -> Grouping {
    let prepared: Vec<Option<Prepared>> = files.iter().map(prepare).collect();
    let mut folded = bucket_by_folder_and_title(&prepared);
    let adopted = adopt_orphans(files, &prepared, &mut folded);

    let mut grouping = Grouping {
        placement: vec![Placement::default(); files.len()],
        ..Grouping::default()
    };
    let wanted = candidates(files, &folded);
    // The folder a key was last awarded to takes it back before the walk order gets a say.
    let mut claimed: HashSet<String> = HashSet::new();
    let owners: HashSet<usize> = wanted
        .iter()
        .enumerate()
        .filter(|(_, one)| held.get(&one.key) == Some(&one.bucket.0))
        .filter(|(_, one)| claimed.insert(one.key.clone()))
        .map(|(at, _)| at)
        .collect();

    for (at, one) in wanted.into_iter().enumerate() {
        let Wanted {
            bucket,
            mbid,
            subgroup,
            key,
            rule,
        } = one;
        let (key, rule) = match owners.contains(&at) || claimed.insert(key.clone()) {
            true => (key, rule),
            false => {
                // Debug level: this fires thousands of times per pass.
                tracing::debug!(
                    folder = %bucket.0,
                    album = %bucket.1,
                    "two folders derive the same album identity; the second is keyed on its path"
                );
                (
                    claim_free(&mut claimed, format!("release/path/{}", bucket.0)),
                    Rule::Path,
                )
            }
        };
        grouping.claims.insert(key.clone(), bucket.0.clone());
        for index in &subgroup {
            grouping.placement[*index] = Placement {
                release: Some(grouping.releases.len()),
                disc: prepared[*index].as_ref().and_then(|p| p.disc),
                inherited: adopted.contains(index),
            };
        }
        grouping.releases.push(Release {
            id: hashed(RELEASE, &key),
            rule,
            key,
            title: most_common(
                subgroup
                    .iter()
                    .filter_map(|index| prepared[*index].as_ref())
                    .map(|prepared| prepared.title.as_str()),
            )
            .unwrap_or_default(),
            musicbrainz_id: mbid,
            files: subgroup,
        });
    }
    grouping
}

/// What each subgroup would be keyed on if nothing else wanted the same key.
struct Wanted {
    bucket: Bucket,
    mbid: Option<String>,
    subgroup: Vec<usize>,
    key: String,
    rule: Rule,
}

fn candidates(files: &[FileFacts], folded: &Buckets) -> Vec<Wanted> {
    let mut wanted = Vec::new();
    for bucket in &folded.order {
        for (mbid, subgroup) in release_subgroups(files, &folded.buckets[bucket], bucket) {
            let (key, rule) = release_key(files, bucket, mbid, &subgroup);
            wanted.push(Wanted {
                bucket: bucket.clone(),
                mbid: mbid.map(str::to_owned),
                subgroup,
                key,
                rule,
            });
        }
    }
    wanted
}

struct Buckets {
    order: Vec<Bucket>,
    buckets: HashMap<Bucket, Vec<usize>>,
}

/// (folder, folded album title).
type Bucket = (String, String);

fn bucket_by_folder_and_title(prepared: &[Option<Prepared>]) -> Buckets {
    let mut held = Buckets {
        order: Vec::new(),
        buckets: HashMap::new(),
    };
    for (index, entry) in prepared.iter().enumerate() {
        let Some(prepared) = entry else { continue };
        let bucket = (prepared.scope.clone(), prepared.folded_title.clone());
        held.buckets
            .entry(bucket.clone())
            .or_insert_with(|| {
                held.order.push(bucket);
                Vec::new()
            })
            .push(index);
    }
    held
}

fn adopt_orphans(
    files: &[FileFacts],
    prepared: &[Option<Prepared>],
    folded: &mut Buckets,
) -> HashSet<usize> {
    let inherited = inherit_album_from_folder(files, prepared, &folded.buckets);
    for (bucket, adopted) in &inherited {
        folded
            .buckets
            .get_mut(bucket)
            .expect("a bucket that was found")
            .extend(adopted);
    }
    inherited.values().flatten().copied().collect()
}

fn release_subgroups<'a>(
    files: &'a [FileFacts],
    members: &[usize],
    bucket: &Bucket,
) -> Vec<(Option<&'a str>, Vec<usize>)> {
    let identified: Vec<(usize, Option<&str>)> = members
        .iter()
        .map(|index| {
            (
                *index,
                uuid(files[*index].tags.musicbrainz_release_id.as_deref()),
            )
        })
        .collect();
    let subgroups = split_by_identifier(&identified);
    if subgroups.len() > 1 {
        tracing::warn!(
            folder = %bucket.0,
            album = %bucket.1,
            releases = subgroups.len(),
            "one folder carries conflicting release identifiers and is served as several albums"
        );
    }
    subgroups
}

fn release_key(
    files: &[FileFacts],
    bucket: &Bucket,
    mbid: Option<&str>,
    subgroup: &[usize],
) -> (String, Rule) {
    match mbid {
        Some(mbid) => {
            let complete = subgroup
                .iter()
                .all(|index| uuid(files[*index].tags.musicbrainz_release_id.as_deref()).is_some());
            (
                format!("release/mbid/{mbid}"),
                if complete {
                    Rule::ReleaseMbid
                } else {
                    Rule::ReleaseMbidPropagated
                },
            )
        }
        None => (
            format!(
                "release/strings/{}{UNIT}{}",
                bucket.1,
                release_artist(files, subgroup)
            ),
            Rule::Strings,
        ),
    }
}

fn claim_free(claimed: &mut HashSet<String>, candidate: String) -> String {
    if claimed.insert(candidate.clone()) {
        return candidate;
    }
    for occurrence in 2.. {
        let numbered = format!("{candidate}{UNIT}{occurrence}");
        if claimed.insert(numbered.clone()) {
            return numbered;
        }
    }
    unreachable!("a counter over usize outlives any library")
}

fn inherit_album_from_folder(
    files: &[FileFacts],
    prepared: &[Option<Prepared>],
    buckets: &HashMap<Bucket, Vec<usize>>,
) -> HashMap<Bucket, Vec<usize>> {
    let mut by_scope: HashMap<&str, Vec<&Bucket>> = HashMap::new();
    for bucket in buckets.keys() {
        by_scope.entry(bucket.0.as_str()).or_default().push(bucket);
    }

    let mut orphans: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, entry) in prepared.iter().enumerate() {
        if entry.is_none() {
            let (scope, _) = grouping_scope(files[index].relative_path, files[index].tags);
            orphans.entry(scope).or_default().push(index);
        }
    }

    let mut adopted: HashMap<Bucket, Vec<usize>> = HashMap::new();
    for (scope, indices) in orphans {
        let Some(candidates) = by_scope.get(scope.as_str()) else {
            continue;
        };
        let [bucket] = candidates[..] else { continue };
        if buckets[bucket].len() <= indices.len() {
            tracing::debug!(
                folder = %scope,
                album = %bucket.1,
                tagged = buckets[bucket].len(),
                untagged = indices.len(),
                "not inheriting an album into a folder that mostly does not name one"
            );
            continue;
        }
        adopted.insert(bucket.clone(), indices);
    }
    adopted
}

pub fn split_by_identifier<'a>(
    members: &[(usize, Option<&'a str>)],
) -> Vec<(Option<&'a str>, Vec<usize>)> {
    let mut distinct: Vec<&str> = Vec::new();
    for (_, identifier) in members {
        if let Some(identifier) = identifier
            && !distinct.contains(identifier)
        {
            distinct.push(identifier);
        }
    }
    let all = || members.iter().map(|(index, _)| *index).collect::<Vec<_>>();
    match distinct.len() {
        0 => vec![(None, all())],
        1 => vec![(Some(distinct[0]), all())],
        _ => {
            let mut subgroups: Vec<(Option<&str>, Vec<usize>)> = distinct
                .iter()
                .map(|identifier| {
                    let members = members
                        .iter()
                        .filter(|(_, carried)| *carried == Some(*identifier))
                        .map(|(index, _)| *index)
                        .collect();
                    (Some(*identifier), members)
                })
                .collect();
            let unidentified: Vec<usize> = members
                .iter()
                .filter(|(_, carried)| carried.is_none())
                .map(|(index, _)| *index)
                .collect();
            if !unidentified.is_empty() {
                subgroups.push((None, unidentified));
            }
            subgroups
        }
    }
}

#[derive(Debug, Default)]
pub struct Minter {
    claimed: HashSet<String>,
}

impl Minter {
    pub fn track(
        &mut self,
        file: &FileFacts,
        release: Option<&Release>,
        disc: Option<u32>,
    ) -> Identity {
        let tags = file.tags;
        if let Some(mbid) = uuid(tags.musicbrainz_release_track_id.as_deref())
            && let Some(id) = self.claim(TRACK, &format!("track/release-track-mbid/{mbid}"))
        {
            return Identity {
                id,
                rule: Rule::ReleaseTrackMbid,
            };
        }

        let title = clean(tags.title.as_deref());
        if identifies_itself(tags) {
            let material = match release {
                Some(release) => format!(
                    "track/on-release/{}{UNIT}{}{UNIT}{}{UNIT}{}",
                    release.key,
                    disc.or(tags.disc_number).unwrap_or(0),
                    tags.track_number.unwrap_or(0),
                    fold(title.unwrap_or_default()),
                ),
                None => format!(
                    "track/single/{}{UNIT}{}",
                    fold(title.unwrap_or_default()),
                    folded_list(&tags.artists),
                ),
            };
            if let Some(id) = self.claim(TRACK, &material) {
                return Identity {
                    id,
                    rule: Rule::Strings,
                };
            }
            tracing::debug!(
                path = %file.relative_path.display(),
                "two files derive the same track identity; this one is keyed on its path"
            );
        }

        let material = format!("track/path/{}", file.relative_path.display());
        self.claimed.insert(material.clone());
        Identity {
            id: hashed(TRACK, &material),
            rule: Rule::Path,
        }
    }

    fn claim(&mut self, prefix: &str, material: &str) -> Option<ObjectId> {
        self.claimed
            .insert(material.to_owned())
            .then(|| hashed(prefix, material))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct ArtistMention<'a> {
    pub name: &'a str,
    pub mbid: Option<&'a str>,
}

#[derive(Clone, Debug)]
pub struct ArtistGroup {
    pub id: ObjectId,
    pub rule: Rule,
    pub key: String,
    pub name: String,
    pub musicbrainz_id: Option<String>,
    /// Indices into the input mention list.
    pub mentions: Vec<usize>,
}

pub fn group_artists(mentions: &[ArtistMention]) -> Vec<ArtistGroup> {
    let mut order: Vec<String> = Vec::new();
    let mut buckets: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, mention) in mentions.iter().enumerate() {
        let Some(name) = clean(Some(mention.name)) else {
            continue;
        };
        let folded = fold(name);
        buckets
            .entry(folded.clone())
            .or_insert_with(|| {
                order.push(folded);
                Vec::new()
            })
            .push(index);
    }

    // Two name buckets can derive one key.
    let mut keys: Vec<String> = Vec::new();
    let mut grouped: HashMap<String, ArtistGroup> = HashMap::new();
    for folded in &order {
        let members = &buckets[folded];
        let identified: Vec<(usize, Option<&str>)> = members
            .iter()
            .map(|index| (*index, uuid(mentions[*index].mbid)))
            .collect();
        for (mbid, subgroup) in split_by_identifier(&identified) {
            let (key, rule) = match mbid {
                Some(mbid) => (format!("artist/mbid/{mbid}"), Rule::ArtistMbid),
                None => (format!("artist/name/{folded}"), Rule::Strings),
            };
            match grouped.get_mut(&key) {
                Some(held) => held.mentions.extend(subgroup),
                None => {
                    keys.push(key.clone());
                    grouped.insert(
                        key.clone(),
                        ArtistGroup {
                            id: hashed(ARTIST, &key),
                            rule,
                            key,
                            name: String::new(),
                            musicbrainz_id: mbid.map(str::to_owned),
                            mentions: subgroup,
                        },
                    );
                }
            }
        }
    }

    let mut artists = Vec::with_capacity(keys.len());
    for key in &keys {
        let mut artist = grouped.remove(key).expect("a key that was just held");
        artist.mentions.sort_unstable();
        // Named after merging, so a spelling from another bucket counts.
        artist.name = most_common(artist.mentions.iter().map(|index| mentions[*index].name))
            .unwrap_or_default();
        artists.push(artist);
    }
    artists
}

#[derive(Clone, Debug)]
struct Prepared {
    scope: String,
    title: String,
    folded_title: String,
    disc: Option<u32>,
}

fn prepare(file: &FileFacts) -> Option<Prepared> {
    let album = clean(file.tags.album.as_deref())?;
    let (title, title_disc) = strip_disc_marker(album);
    let (scope, folder_disc) = grouping_scope(file.relative_path, file.tags);
    Some(Prepared {
        scope,
        folded_title: fold(title),
        title: title.to_owned(),
        disc: file.tags.disc_number.or(title_disc).or(folder_disc),
    })
}

fn grouping_scope(relative: &Path, tags: &FileTags) -> (String, Option<u32>) {
    let folder = relative.parent().unwrap_or(Path::new(""));
    let above = || {
        folder
            .parent()
            .unwrap_or(Path::new(""))
            .to_string_lossy()
            .into_owned()
    };
    let name = folder
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default();
    if let Some(disc) = folder_disc(name) {
        return (above(), Some(disc));
    }
    if tags.disc_total.is_some_and(|total| total > 1) {
        return (above(), None);
    }
    (folder.to_string_lossy().into_owned(), None)
}

fn release_artist(files: &[FileFacts], members: &[usize]) -> String {
    let credits = |pick: fn(&FileTags) -> &Vec<String>| {
        members
            .iter()
            .map(move |index| pick(files[*index].tags))
            .filter(|values| !values.is_empty())
            .map(|values| folded_list(values))
    };
    most_common_of(credits(|tags| &tags.album_artists))
        .or_else(|| most_common_of(credits(|tags| &tags.artists)))
        .unwrap_or_default()
}

/// Ties go to the smallest value.
pub fn most_common<'a>(values: impl Iterator<Item = &'a str>) -> Option<String> {
    let mut counts: HashMap<&str, usize> = HashMap::new();
    for value in values {
        let value = value.trim();
        if !value.is_empty() {
            *counts.entry(value).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .max_by(|(left, left_count), (right, right_count)| {
            left_count.cmp(right_count).then(right.cmp(left))
        })
        .map(|(value, _)| value.to_owned())
}

pub fn most_common_of(values: impl Iterator<Item = String>) -> Option<String> {
    let mut counts: HashMap<String, usize> = HashMap::new();
    for value in values {
        if !value.trim().is_empty() {
            *counts.entry(value).or_default() += 1;
        }
    }
    counts
        .into_iter()
        .max_by(|(left, left_count), (right, right_count)| {
            left_count.cmp(right_count).then(right.cmp(left))
        })
        .map(|(value, _)| value)
}

pub(crate) fn folded_list(values: &[String]) -> String {
    values
        .iter()
        .map(|value| fold(value))
        .collect::<Vec<_>>()
        .join(";")
}

pub fn clean(value: Option<&str>) -> Option<&str> {
    let value = value?.trim();
    (!value.is_empty()).then_some(value)
}

pub fn identifies_itself(tags: &FileTags) -> bool {
    clean(tags.title.as_deref()).is_some() || tags.track_number.is_some()
}

pub fn uuid(value: Option<&str>) -> Option<&str> {
    let value = clean(value)?;
    let bytes = value.as_bytes();
    let shaped = bytes.len() == 36
        && bytes.iter().enumerate().all(|(at, byte)| match at {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_hexdigit(),
        });
    shaped.then_some(value)
}

pub fn playlist(relative: &str) -> ObjectId {
    hashed(PLAYLIST, &format!("playlist/path/{relative}"))
}

fn hashed(prefix: &str, material: &str) -> ObjectId {
    let digest = Sha256::digest(material.as_bytes());
    ObjectId::new(format!("{prefix}{}", hex::encode(&digest[..8])))
        .expect("a hex digest behind an ascii prefix is inside the permitted alphabet")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn tags(album: &str, title: &str, track: u32) -> FileTags {
        FileTags {
            title: Some(title.to_owned()),
            album: Some(album.to_owned()),
            album_artists: vec!["Kremerata Baltica".to_owned()],
            track_number: Some(track),
            ..FileTags::default()
        }
    }

    fn facts<'a>(files: &'a [(PathBuf, FileTags)]) -> Vec<FileFacts<'a>> {
        files
            .iter()
            .map(|(path, tags)| FileFacts {
                relative_path: path,
                tags,
            })
            .collect()
    }

    const MBID: &str = "1a2b3c4d-5e6f-4a8b-9c0d-1e2f3a4b5c6d";
    const OTHER_MBID: &str = "9f8e7d6c-5b4a-4938-8271-6a5b4c3d2e1f";

    #[test]
    fn a_half_tagged_album_stays_one_album() {
        let mut tagged = tags("Symphonies", "Allegro", 1);
        tagged.musicbrainz_release_id = Some(MBID.to_owned());
        let files = vec![
            (PathBuf::from("Brahms/01.flac"), tagged),
            (
                PathBuf::from("Brahms/02.flac"),
                tags("Symphonies", "Andante", 2),
            ),
        ];
        let grouping = group_releases(&facts(&files));
        assert_eq!(grouping.releases.len(), 1);
        assert_eq!(grouping.releases[0].rule, Rule::ReleaseMbidPropagated);
        assert_eq!(grouping.releases[0].musicbrainz_id.as_deref(), Some(MBID));
        assert_eq!(grouping.releases[0].files, vec![0, 1]);
    }

    #[test]
    fn a_conflicting_identifier_splits_an_album() {
        let mut first = tags("Symphonies", "Allegro", 1);
        first.musicbrainz_release_id = Some(MBID.to_owned());
        let mut second = tags("Symphonies", "Andante", 2);
        second.musicbrainz_release_id = Some(OTHER_MBID.to_owned());
        let files = vec![
            (PathBuf::from("Brahms/01.flac"), first),
            (PathBuf::from("Brahms/02.flac"), second),
        ];
        let grouping = group_releases(&facts(&files));
        assert_eq!(grouping.releases.len(), 2);
        assert_ne!(grouping.releases[0].id, grouping.releases[1].id);
    }

    #[test]
    fn every_file_of_a_fully_tagged_album_says_so() {
        let mut first = tags("Symphonies", "Allegro", 1);
        first.musicbrainz_release_id = Some(MBID.to_owned());
        let mut second = tags("Symphonies", "Andante", 2);
        second.musicbrainz_release_id = Some(MBID.to_owned());
        let files = vec![
            (PathBuf::from("Brahms/01.flac"), first),
            (PathBuf::from("Brahms/02.flac"), second),
        ];
        let grouping = group_releases(&facts(&files));
        assert_eq!(grouping.releases[0].rule, Rule::ReleaseMbid);
    }

    #[test]
    fn a_differing_date_or_album_artist_no_longer_splits_an_album() {
        let mut first = tags("Symphonies", "Allegro", 1);
        first.date = Some("1994".to_owned());
        first.album_artists = vec!["Kremerata Baltica".to_owned()];
        let mut second = tags("Symphonies", "Andante", 2);
        second.date = Some("2011".to_owned());
        second.album_artists = vec![];
        let files = vec![
            (PathBuf::from("Brahms/01.flac"), first),
            (PathBuf::from("Brahms/02.flac"), second),
        ];
        let grouping = group_releases(&facts(&files));
        assert_eq!(grouping.releases.len(), 1);
        assert_eq!(grouping.releases[0].files, vec![0, 1]);
    }

    #[test]
    fn a_disc_folder_merges_into_one_album() {
        let files = vec![
            (
                PathBuf::from("Brahms/CD1/01.flac"),
                tags("Symphonies", "Allegro", 1),
            ),
            (
                PathBuf::from("Brahms/CD2/01.flac"),
                tags("Symphonies", "Andante", 1),
            ),
        ];
        let grouping = group_releases(&facts(&files));
        assert_eq!(grouping.releases.len(), 1);
        assert_eq!(grouping.placement[0].disc, Some(1));
        assert_eq!(grouping.placement[1].disc, Some(2));
    }

    #[test]
    fn a_folder_whose_name_ends_in_a_disc_marker_merges_too() {
        let files = vec![
            (
                PathBuf::from("Bach/Cantatas (Disc 1)/01.flac"),
                tags("Cantatas", "Sinfonia", 1),
            ),
            (
                PathBuf::from("Bach/Cantatas (Disc 2)/01.flac"),
                tags("Cantatas", "Chorale", 1),
            ),
        ];
        let grouping = group_releases(&facts(&files));
        assert_eq!(grouping.releases.len(), 1);
        assert_eq!(grouping.placement[1].disc, Some(2));
    }

    #[test]
    fn discs_named_after_their_contents_merge_on_the_disc_total() {
        let mut kyrie = tags("Cantatas", "Kyrie", 1);
        kyrie.disc_number = Some(1);
        kyrie.disc_total = Some(2);
        let mut gloria = tags("Cantatas", "Gloria", 1);
        gloria.disc_number = Some(2);
        gloria.disc_total = Some(2);
        let files = vec![
            (PathBuf::from("Bach/Kyrie/01.flac"), kyrie),
            (PathBuf::from("Bach/Gloria/01.flac"), gloria),
        ];
        let grouping = group_releases(&facts(&files));
        assert_eq!(grouping.releases.len(), 1);
        assert_eq!(grouping.placement[0].disc, Some(1));
        assert_eq!(grouping.placement[1].disc, Some(2));
    }

    #[test]
    fn a_single_disc_album_does_not_merge_with_its_neighbour() {
        let mut first = tags("Hits", "One", 1);
        first.disc_number = Some(1);
        first.disc_total = Some(1);
        let mut second = tags("Hits", "One", 1);
        second.disc_number = Some(1);
        second.disc_total = Some(1);
        let files = vec![
            (PathBuf::from("A/Hits/01.flac"), first),
            (PathBuf::from("B/Hits/01.flac"), second),
        ];
        let grouping = group_releases(&facts(&files));
        assert_eq!(grouping.releases.len(), 2);
    }

    #[test]
    fn a_disc_marker_in_the_title_merges_and_leaves_the_title_clean() {
        let files = vec![
            (
                PathBuf::from("Brahms/01.flac"),
                tags("Symphonies [disc 1]", "Allegro", 1),
            ),
            (
                PathBuf::from("Brahms/02.flac"),
                tags("Symphonies [disc 2]", "Andante", 1),
            ),
        ];
        let grouping = group_releases(&facts(&files));
        assert_eq!(grouping.releases.len(), 1);
        assert_eq!(grouping.releases[0].title, "Symphonies");
        assert_eq!(grouping.placement[1].disc, Some(2));
    }

    #[test]
    fn two_albums_in_one_folder_stay_two_albums() {
        let files = vec![
            (
                PathBuf::from("Box/01.flac"),
                tags("Symphonies", "Allegro", 1),
            ),
            (PathBuf::from("Box/02.flac"), tags("Concertos", "Largo", 1)),
        ];
        let grouping = group_releases(&facts(&files));
        assert_eq!(grouping.releases.len(), 2);
    }

    #[test]
    fn the_same_album_in_two_folders_keeps_two_identities() {
        let files = vec![
            (
                PathBuf::from("flac/Brahms/01.flac"),
                tags("Symphonies", "Allegro", 1),
            ),
            (
                PathBuf::from("mp3/Brahms/01.flac"),
                tags("Symphonies", "Allegro", 1),
            ),
        ];
        let grouping = group_releases(&facts(&files));
        assert_eq!(grouping.releases.len(), 2);
        assert_ne!(grouping.releases[0].id, grouping.releases[1].id);
        assert_eq!(grouping.releases[1].rule, Rule::Path);
    }

    #[test]
    fn same_titled_albums_by_different_artists_do_not_share_an_identifier() {
        let mut brahms = tags("Symphonies", "Allegro", 1);
        brahms.album_artists = vec!["Brahms".to_owned()];
        let mut mahler = tags("Symphonies", "Allegro", 1);
        mahler.album_artists = vec!["Mahler".to_owned()];
        let files = vec![
            (PathBuf::from("a/01.flac"), brahms),
            (PathBuf::from("b/01.flac"), mahler),
        ];
        let grouping = group_releases(&facts(&files));
        assert_eq!(grouping.releases.len(), 2);
        assert_ne!(grouping.releases[0].id, grouping.releases[1].id);
        assert!(
            grouping
                .releases
                .iter()
                .all(|release| release.rule == Rule::Strings)
        );
    }

    #[test]
    fn a_file_with_no_album_belongs_to_no_release() {
        let mut orphan = tags("", "Improvisation", 1);
        orphan.album = None;
        let files = vec![(PathBuf::from("loose/01.flac"), orphan)];
        let grouping = group_releases(&facts(&files));
        assert!(grouping.releases.is_empty());
        assert_eq!(grouping.placement[0].release, None);
    }

    #[test]
    fn a_release_identity_survives_a_move() {
        let before = vec![(
            PathBuf::from("Brahms/01.flac"),
            tags("Symphonies", "Allegro", 1),
        )];
        let after = vec![(
            PathBuf::from("classical/brahms 1876/01.flac"),
            tags("Symphonies", "Allegro", 1),
        )];
        assert_eq!(
            group_releases(&facts(&before)).releases[0].id,
            group_releases(&facts(&after)).releases[0].id
        );
    }

    #[test]
    fn a_track_identity_survives_a_move_and_a_rename() {
        let before = vec![(
            PathBuf::from("Brahms/01.flac"),
            tags("Symphonies", "Allegro", 1),
        )];
        let after = vec![(
            PathBuf::from("classical/brahms/01 - Allegro.flac"),
            tags("Symphonies", "Allegro", 1),
        )];
        let identity = |files: &[(PathBuf, FileTags)]| {
            let facts = facts(files);
            let grouping = group_releases(&facts);
            Minter::default().track(
                &facts[0],
                grouping.releases.first(),
                grouping.placement[0].disc,
            )
        };
        let first = identity(&before);
        assert_eq!(first, identity(&after));
        assert_eq!(first.rule, Rule::Strings);
    }

    #[test]
    fn a_recording_identifier_never_keys_a_track() {
        let mut compilation = tags("Greatest Hits", "Allegro", 4);
        compilation.musicbrainz_recording_id = Some(MBID.to_owned());
        let mut original = tags("Symphonies", "Allegro", 1);
        original.musicbrainz_recording_id = Some(MBID.to_owned());
        let files = vec![
            (PathBuf::from("Hits/04.flac"), compilation),
            (PathBuf::from("Brahms/01.flac"), original),
        ];
        let facts = facts(&files);
        let grouping = group_releases(&facts);
        let mut minter = Minter::default();
        let first = minter.track(
            &facts[0],
            grouping.releases.first(),
            grouping.placement[0].disc,
        );
        let second = minter.track(
            &facts[1],
            grouping.releases.get(1),
            grouping.placement[1].disc,
        );
        assert_ne!(first.id, second.id);
        assert_eq!(first.rule, Rule::Strings);
    }

    #[test]
    fn a_release_track_identifier_keys_a_track() {
        let mut tagged = tags("Symphonies", "Allegro", 1);
        tagged.musicbrainz_release_track_id = Some(MBID.to_owned());
        let files = vec![(PathBuf::from("Brahms/01.flac"), tagged)];
        let facts = facts(&files);
        let grouping = group_releases(&facts);
        let identity = Minter::default().track(
            &facts[0],
            grouping.releases.first(),
            grouping.placement[0].disc,
        );
        assert_eq!(identity.rule, Rule::ReleaseTrackMbid);
    }

    #[test]
    fn a_file_identifying_nothing_falls_to_its_path() {
        let files = vec![(PathBuf::from("loose/unnamed.flac"), FileTags::default())];
        let facts = facts(&files);
        let identity = Minter::default().track(&facts[0], None, None);
        assert_eq!(identity.rule, Rule::Path);
    }

    #[test]
    fn two_files_deriving_one_identity_do_not_share_it() {
        let files = vec![
            (
                PathBuf::from("Brahms/01.flac"),
                tags("Symphonies", "Allegro", 1),
            ),
            (
                PathBuf::from("Brahms/01 (copy).flac"),
                tags("Symphonies", "Allegro", 1),
            ),
        ];
        let facts = facts(&files);
        let grouping = group_releases(&facts);
        let mut minter = Minter::default();
        let first = minter.track(
            &facts[0],
            grouping.releases.first(),
            grouping.placement[0].disc,
        );
        let second = minter.track(
            &facts[1],
            grouping.releases.first(),
            grouping.placement[1].disc,
        );
        assert_ne!(first.id, second.id);
        assert_eq!(second.rule, Rule::Path);
    }

    #[test]
    fn identifiers_are_legal_and_say_what_they_are() {
        let files = vec![(
            PathBuf::from("Brahms/01.flac"),
            tags("Symphonies", "Allegro", 1),
        )];
        let facts = facts(&files);
        let grouping = group_releases(&facts);
        let track = Minter::default().track(
            &facts[0],
            grouping.releases.first(),
            grouping.placement[0].disc,
        );
        for id in [&track.id, &grouping.releases[0].id] {
            assert!(id.as_str().chars().all(ObjectId::is_legal), "{id}");
        }
        assert!(track.id.as_str().starts_with("tr-"));
        assert!(grouping.releases[0].id.as_str().starts_with("al-"));
    }

    #[test]
    fn a_placeholder_identifier_is_refused() {
        assert_eq!(uuid(Some(MBID)), Some(MBID));
        for junk in [
            "",
            "  ",
            "0",
            "unknown",
            "none",
            "1a2b3c4d5e6f4a8b9c0d1e2f3a4b5c6d",
        ] {
            assert_eq!(uuid(Some(junk)), None, "{junk}");
        }
    }

    #[test]
    fn a_disc_folder_with_a_subtitle_merges_into_one_album() {
        let files = vec![
            (
                PathBuf::from("Masters at Work/disc 2 - Dubs & Maw Vocals/01.flac"),
                tags("The Tenth Anniversary Collection", "Dub", 1),
            ),
            (
                PathBuf::from("Masters at Work/disc 3 - Beats & Loops/01.flac"),
                tags("The Tenth Anniversary Collection", "Beat", 1),
            ),
            (
                PathBuf::from("Masters at Work/disc 4 - Tools & Grooves/01.flac"),
                tags("The Tenth Anniversary Collection", "Tool", 1),
            ),
        ];
        let grouping = group_releases(&facts(&files));
        assert_eq!(grouping.releases.len(), 1);
        assert_eq!(grouping.placement[0].disc, Some(2));
        assert_eq!(grouping.placement[2].disc, Some(4));
    }

    #[test]
    fn a_single_identifier_covers_a_group_and_several_split_it() {
        assert_eq!(
            split_by_identifier(&[(0, Some("a")), (1, None), (2, Some("a"))]),
            vec![(Some("a"), vec![0, 1, 2])]
        );
        assert_eq!(
            split_by_identifier(&[(0, Some("a")), (1, Some("b")), (2, None)]),
            vec![(Some("a"), vec![0]), (Some("b"), vec![1]), (None, vec![2])]
        );
        assert_eq!(
            split_by_identifier(&[(0, None), (1, None)]),
            vec![(None, vec![0, 1])]
        );
    }

    #[test]
    fn two_albums_falling_to_the_same_folder_do_not_share_an_identifier() {
        let files = vec![
            (PathBuf::from("aa/01.flac"), tags("X", "one", 1)),
            (PathBuf::from("ab/01.flac"), tags("Y", "one", 1)),
            (PathBuf::from("b/01.flac"), tags("X", "one", 1)),
            (PathBuf::from("b/02.flac"), tags("Y", "one", 1)),
        ];
        let grouping = group_releases(&facts(&files));
        assert_eq!(grouping.releases.len(), 4);

        let path_keyed: Vec<&Release> = grouping
            .releases
            .iter()
            .filter(|release| release.rule == Rule::Path)
            .collect();
        assert_eq!(path_keyed.len(), 2, "both albums in `b` collided");
        assert_ne!(
            path_keyed[0].id, path_keyed[1].id,
            "and neither may take the other's identifier"
        );

        let mut ids: Vec<&str> = grouping
            .releases
            .iter()
            .map(|release| release.id.as_str())
            .collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), 4, "every release mints its own identifier");
    }

    #[test]
    fn two_releases_differing_only_in_their_second_artist_are_two_releases() {
        let credited = |names: &[&str]| FileTags {
            title: Some("one".to_owned()),
            album: Some("Live".to_owned()),
            album_artists: names.iter().map(|name| (*name).to_owned()).collect(),
            track_number: Some(1),
            ..FileTags::default()
        };
        let files = vec![
            (PathBuf::from("one/01.flac"), credited(&["A", "B"])),
            (PathBuf::from("two/01.flac"), credited(&["A", "C"])),
        ];
        let grouping = group_releases(&facts(&files));
        assert_eq!(grouping.releases.len(), 2);
        assert_eq!(
            grouping
                .releases
                .iter()
                .filter(|release| release.rule == Rule::Strings)
                .count(),
            2,
            "neither had to fall back to its path: {:?}",
            grouping.releases.iter().map(|r| &r.key).collect::<Vec<_>>()
        );
    }

    #[test]
    fn a_track_crediting_one_of_two_artists_does_not_rekey_the_album() {
        let credited = |name: &str, names: &[&str]| {
            (
                PathBuf::from(format!("one/{name}.flac")),
                FileTags {
                    title: Some(name.to_owned()),
                    album: Some("Live".to_owned()),
                    album_artists: names.iter().map(|value| (*value).to_owned()).collect(),
                    track_number: Some(1),
                    ..FileTags::default()
                },
            )
        };
        let both: Vec<(PathBuf, FileTags)> = ["01", "02", "03"]
            .iter()
            .map(|name| credited(name, &["A", "B"]))
            .collect();
        let before = group_releases(&facts(&both)).releases[0].id.clone();

        let mut with_one_more = both;
        with_one_more.push(credited("04", &["B"]));
        let after = group_releases(&facts(&with_one_more)).releases[0]
            .id
            .clone();
        assert_eq!(before, after);
    }

    #[test]
    fn one_identifier_under_two_spellings_is_one_artist() {
        let artists = group_artists(&[
            ArtistMention {
                name: "The Beatles",
                mbid: Some(MBID),
            },
            ArtistMention {
                name: "Beatles, The",
                mbid: Some(MBID),
            },
            ArtistMention {
                name: "The Beatles",
                mbid: Some(MBID),
            },
        ]);
        assert_eq!(artists.len(), 1);
        assert_eq!(artists[0].mentions, vec![0, 1, 2]);
        assert_eq!(
            artists[0].name, "The Beatles",
            "and it is shown under the spelling most of the mentions use"
        );
    }

    #[test]
    fn one_artist_spelled_two_ways_is_one_artist() {
        let artists = group_artists(&[
            ArtistMention {
                name: "Arvo Pärt",
                mbid: None,
            },
            ArtistMention {
                name: "arvo part",
                mbid: Some(MBID),
            },
            ArtistMention {
                name: "Arvo Pärt",
                mbid: None,
            },
        ]);
        assert_eq!(artists.len(), 1);
        assert_eq!(artists[0].rule, Rule::ArtistMbid);
        assert_eq!(artists[0].name, "Arvo Pärt");
        assert_eq!(artists[0].mentions, vec![0, 1, 2]);
    }

    #[test]
    fn two_artists_sharing_a_name_are_told_apart_by_their_identifiers() {
        let artists = group_artists(&[
            ArtistMention {
                name: "Bill Evans",
                mbid: Some(MBID),
            },
            ArtistMention {
                name: "Bill Evans",
                mbid: Some(OTHER_MBID),
            },
            ArtistMention {
                name: "Bill Evans",
                mbid: None,
            },
        ]);
        assert_eq!(artists.len(), 3);
        assert_ne!(artists[0].id, artists[1].id);
        assert_eq!(artists[2].rule, Rule::Strings);
    }

    #[test]
    fn an_untagged_library_gets_one_artist_per_name() {
        let artists = group_artists(&[
            ArtistMention {
                name: "Brahms",
                mbid: None,
            },
            ArtistMention {
                name: "Mahler",
                mbid: None,
            },
            ArtistMention {
                name: "  brahms ",
                mbid: None,
            },
        ]);
        assert_eq!(artists.len(), 2);
        assert_eq!(artists[0].mentions, vec![0, 2]);
        assert!(
            artists
                .iter()
                .all(|artist| artist.id.as_str().starts_with("ar-"))
        );
    }

    #[test]
    fn the_most_common_spelling_wins_and_ties_are_settled() {
        assert_eq!(
            most_common(["Bill Evans", "Bill Evans", "bill evans"].into_iter()),
            Some("Bill Evans".to_owned())
        );
        assert_eq!(most_common(["b", "a"].into_iter()), Some("a".to_owned()));
        assert_eq!(most_common(["", "  "].into_iter()), None);
    }
}
