//! DIDL-Lite generation.

use std::io::Cursor;
use std::time::Duration;

use quick_xml::Writer;
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};

use crate::index::credits::{self, Credit};
use crate::index::{Artwork, Track};
pub use crate::object::{ALBUM, ARTIST, MUSIC_TRACK, PLAYLIST};
use crate::upnp::ObjectId;
use crate::upnp::client::{Profile, Roles};

const DIDL_NS: &str = "urn:schemas-upnp-org:metadata-1-0/DIDL-Lite/";
const DC_NS: &str = "http://purl.org/dc/elements/1.1/";
const UPNP_NS: &str = "urn:schemas-upnp-org:metadata-1-0/upnp/";
const DLNA_NS: &str = "urn:schemas-dlna-org:metadata-1-0/";

/// Advertises a byte-seekable, streamable resource.
const PROTOCOL_FLAGS: &str = "DLNA.ORG_OP=01;DLNA.ORG_FLAGS=01700000000000000000000000000000";

/// The `AAC_ISO_320` profile's ceiling, in bytes per second.
const AAC_320_CEILING: u32 = 320_000 / 8;

#[derive(Clone, Copy, Debug)]
pub struct To<'a> {
    pub base_url: &'a str,
    pub profile: &'a Profile,
}

impl<'a> To<'a> {
    pub fn new(base_url: &'a str, profile: &'a Profile) -> Self {
        Self { base_url, profile }
    }

    pub fn plain(base_url: &'a str) -> Self {
        Self::new(base_url, crate::upnp::client::conservative())
    }
}

pub struct ContainerSpec<'a> {
    pub id: ObjectId,
    pub parent: ObjectId,
    pub title: String,
    pub child_count: usize,
    /// A container without it shows some clients an empty menu.
    pub searchable: bool,
    pub art: Option<(&'a ObjectId, &'a Artwork)>,
    pub class: &'a str,
    pub genres: &'a [String],
    pub date: Option<&'a str>,
    pub artists: &'a [Credit],
    pub credited: bool,
}

pub const MENU: &str = "object.container";
pub const GENRE: &str = "object.container.genre.musicGenre";
pub const FOLDER: &str = "object.container.storageFolder";

impl<'a> ContainerSpec<'a> {
    pub fn menu(id: ObjectId, parent: ObjectId, title: impl Into<String>, children: usize) -> Self {
        Self {
            id,
            parent,
            title: title.into(),
            child_count: children,
            searchable: true,
            art: None,
            class: MENU,
            genres: &[],
            date: None,
            artists: &[],
            credited: false,
        }
    }

    /// A container standing for a directory. A control point reads the class to tell a folder
    /// from a menu, and both incumbents mark theirs.
    pub fn folder(
        id: ObjectId,
        parent: ObjectId,
        title: impl Into<String>,
        children: usize,
    ) -> Self {
        Self {
            class: FOLDER,
            ..Self::menu(id, parent, title, children)
        }
    }
}

pub enum Child<'a> {
    Container(ContainerSpec<'a>),
    Item(&'a Track, &'a ObjectId),
}

pub fn children(children: &[Child<'_>], to: To<'_>) -> String {
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    open_didl(&mut writer);
    for child in children {
        match child {
            Child::Container(spec) => write_container(&mut writer, spec, to),
            Child::Item(track, parent) => write_item(&mut writer, track, parent, to),
        }
    }
    close_didl(&mut writer);
    finish(writer)
}

fn write_container(writer: &mut Writer<Cursor<Vec<u8>>>, spec: &ContainerSpec<'_>, to: To<'_>) {
    let mut start = BytesStart::new("container");
    start.push_attribute(("id", spec.id.as_str()));
    start.push_attribute(("parentID", spec.parent.as_str()));
    start.push_attribute(("restricted", "1"));
    start.push_attribute(("searchable", if spec.searchable { "1" } else { "0" }));
    start.push_attribute(("childCount", spec.child_count.to_string().as_str()));
    let _ = writer.write_event(Event::Start(start));

    text_element(writer, "dc:title", &spec.title);
    values(writer, "upnp:genre", spec.genres, to.profile);
    if let Some(date) = spec.date.map(iso_date) {
        text_element(writer, "dc:date", &date);
    }
    values(
        writer,
        "upnp:artist",
        &credits::names(spec.artists),
        to.profile,
    );
    if let Some(first) = spec.artists.first() {
        text_element(writer, "dc:creator", &first.name);
    }
    if spec.credited {
        roles(
            writer,
            "AlbumArtist",
            &credits::names(spec.artists),
            to.profile,
        );
    }
    if let Some((id, art)) = spec.art {
        write_album_art(writer, id, art, to.base_url);
    }
    text_element(writer, "upnp:class", spec.class);

    let _ = writer.write_event(Event::End(BytesEnd::new("container")));
}

fn open_didl(writer: &mut Writer<Cursor<Vec<u8>>>) {
    let mut start = BytesStart::new("DIDL-Lite");
    start.push_attribute(("xmlns", DIDL_NS));
    start.push_attribute(("xmlns:dc", DC_NS));
    start.push_attribute(("xmlns:upnp", UPNP_NS));
    start.push_attribute(("xmlns:dlna", DLNA_NS));
    let _ = writer.write_event(Event::Start(start));
}

fn close_didl(writer: &mut Writer<Cursor<Vec<u8>>>) {
    let _ = writer.write_event(Event::End(BytesEnd::new("DIDL-Lite")));
}

fn finish(writer: Writer<Cursor<Vec<u8>>>) -> String {
    String::from_utf8(writer.into_inner().into_inner()).unwrap_or_default()
}

fn write_item(writer: &mut Writer<Cursor<Vec<u8>>>, track: &Track, parent: &ObjectId, to: To<'_>) {
    let mut start = BytesStart::new("item");
    start.push_attribute(("id", track.id.as_str()));
    start.push_attribute(("parentID", parent.as_str()));
    start.push_attribute(("restricted", "1"));
    let _ = writer.write_event(Event::Start(start));

    text_element(writer, "dc:title", &track.title);
    values(writer, "upnp:genre", &track.genres, to.profile);
    if let Some(date) = track.date.as_deref().map(iso_date) {
        text_element(writer, "dc:date", &date);
    }
    if let Some(album) = &track.album {
        text_element(writer, "upnp:album", album);
    }
    values(
        writer,
        "upnp:artist",
        &credits::names(&track.artists),
        to.profile,
    );
    if let Some(first) = track.artists.first() {
        text_element(writer, "dc:creator", &first.name);
    }
    roles(
        writer,
        "AlbumArtist",
        &credits::names(&track.album_artists),
        to.profile,
    );
    roles(
        writer,
        "Composer",
        &credits::names(&track.composers),
        to.profile,
    );
    if let Some(number) = track.track_number {
        text_element(writer, "upnp:originalTrackNumber", &number.to_string());
    }
    if let Some(art) = &track.artwork {
        write_album_art(writer, &track.id, art, to.base_url);
    }

    write_res(writer, track, to);
    text_element(writer, "upnp:class", MUSIC_TRACK);

    let _ = writer.write_event(Event::End(BytesEnd::new("item")));
}

fn profile_name(track: &Track) -> Option<&'static str> {
    match track.mime {
        "audio/mpeg" => Some("MP3"),
        "audio/mp4" => byte_rate(track)
            .filter(|rate| *rate <= AAC_320_CEILING)
            .map(|_| "AAC_ISO_320"),
        _ => None,
    }
}

fn protocol_info(track: &Track, client: &Profile) -> String {
    let mime = client.mime_for(track.mime);
    let claimed = client
        .claim_for(track.mime)
        .unwrap_or_else(|| profile_name(track));
    match claimed {
        Some(name) => format!("http-get:*:{mime}:DLNA.ORG_PN={name};{PROTOCOL_FLAGS}"),
        None => format!("http-get:*:{mime}:{PROTOCOL_FLAGS}"),
    }
}

fn values(writer: &mut Writer<Cursor<Vec<u8>>>, name: &str, values: &[String], client: &Profile) {
    match &client.join {
        Some(separator) if values.len() > 1 => {
            text_element(writer, name, &values.join(separator));
        }
        _ => {
            for value in values {
                text_element(writer, name, value);
            }
        }
    }
}

fn roles(writer: &mut Writer<Cursor<Vec<u8>>>, role: &str, values: &[String], client: &Profile) {
    let elements: &[&str] = match client.roles {
        Roles::Artist => &["upnp:artist"],
        Roles::Author => &["upnp:author"],
        Roles::Both => &["upnp:artist", "upnp:author"],
    };
    for element in elements {
        match &client.join {
            Some(separator) if values.len() > 1 => {
                role_element(writer, element, role, &values.join(separator));
            }
            _ => {
                for value in values {
                    role_element(writer, element, role, value);
                }
            }
        }
    }
}

fn write_res(writer: &mut Writer<Cursor<Vec<u8>>>, track: &Track, to: To<'_>) {
    let mut res = BytesStart::new("res");
    res.push_attribute(("duration", format_duration(track.duration).as_str()));
    res.push_attribute(("size", track.size.to_string().as_str()));
    if let Some(bits) = track.bit_depth {
        res.push_attribute(("bitsPerSample", bits.to_string().as_str()));
    }
    if let Some(bytes_per_second) = byte_rate(track) {
        res.push_attribute(("bitrate", bytes_per_second.to_string().as_str()));
    }
    if let Some(rate) = track.sample_rate {
        res.push_attribute(("sampleFrequency", rate.to_string().as_str()));
    }
    if let Some(channels) = track.channels {
        res.push_attribute(("nrAudioChannels", channels.to_string().as_str()));
    }
    res.push_attribute(("protocolInfo", protocol_info(track, to.profile).as_str()));
    let _ = writer.write_event(Event::Start(res));
    let url = format!("{}/media/{}", to.base_url, track.id);
    let _ = writer.write_event(Event::Text(BytesText::new(&url)));
    let _ = writer.write_event(Event::End(BytesEnd::new("res")));
}

fn write_album_art(
    writer: &mut Writer<Cursor<Vec<u8>>>,
    id: &ObjectId,
    art: &Artwork,
    base_url: &str,
) {
    let mut start = BytesStart::new("upnp:albumArtURI");
    if let Some(profile) = art.dlna_profile() {
        start.push_attribute(("dlna:profileID", profile));
    }
    let _ = writer.write_event(Event::Start(start));
    let url = format!("{base_url}/art/{id}");
    let _ = writer.write_event(Event::Text(BytesText::new(&url)));
    let _ = writer.write_event(Event::End(BytesEnd::new("upnp:albumArtURI")));
}

fn text_element(writer: &mut Writer<Cursor<Vec<u8>>>, name: &str, value: &str) {
    let value = xml_text(value);
    let _ = writer.write_event(Event::Start(BytesStart::new(name)));
    let _ = writer.write_event(Event::Text(BytesText::new(&value)));
    let _ = writer.write_event(Event::End(BytesEnd::new(name)));
}

fn role_element(writer: &mut Writer<Cursor<Vec<u8>>>, name: &str, role: &str, value: &str) {
    let value = xml_text(value);
    let mut start = BytesStart::new(name);
    start.push_attribute(("role", role));
    let _ = writer.write_event(Event::Start(start));
    let _ = writer.write_event(Event::Text(BytesText::new(&value)));
    let _ = writer.write_event(Event::End(BytesEnd::new(name)));
}

/// XML 1.0 permits these characters nowhere, escaped or not.
fn xml_text(value: &str) -> std::borrow::Cow<'_, str> {
    if value.chars().all(legal_in_xml) {
        return std::borrow::Cow::Borrowed(value);
    }
    std::borrow::Cow::Owned(value.chars().filter(|c| legal_in_xml(*c)).collect())
}

const fn legal_in_xml(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r') || (c >= ' ' && c != '\u{fffe}' && c != '\u{ffff}')
}

fn iso_date(value: &str) -> String {
    let trimmed = value.trim();
    match trimmed.len() {
        4 if trimmed.chars().all(|c| c.is_ascii_digit()) => format!("{trimmed}-01-01"),
        7 if is_year_month(trimmed) => format!("{trimmed}-01"),
        _ => trimmed.to_owned(),
    }
}

fn is_year_month(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes[4] == b'-'
        && bytes[..4].iter().all(u8::is_ascii_digit)
        && bytes[5..].iter().all(u8::is_ascii_digit)
}

pub fn format_duration(duration: Duration) -> String {
    let total = duration.as_millis();
    let millis = total % 1000;
    let seconds = (total / 1000) % 60;
    let minutes = (total / 60_000) % 60;
    let hours = total / 3_600_000;
    format!("{hours}:{minutes:02}:{seconds:02}.{millis:03}")
}

/// Measured on the bytes, not `res@bitrate`, which carries the uncompressed rate.
pub fn capped(track: &Track, multiple: f32) -> Option<u32> {
    let playing = f64::from(stream_rate(track)?);
    let capped = playing * f64::from(multiple.max(1.0));
    Some(capped.min(f64::from(u32::MAX)) as u32)
}

fn stream_rate(track: &Track) -> Option<u32> {
    let seconds = track.duration.as_secs_f64();
    if seconds <= 0.0 || track.size == 0 {
        return byte_rate(track);
    }
    Some((track.size as f64 / seconds).min(f64::from(u32::MAX)) as u32)
}

/// `res@bitrate` is bytes per second, not bits.
fn byte_rate(track: &Track) -> Option<u32> {
    match (track.sample_rate, track.bit_depth, track.channels) {
        (Some(rate), Some(bits), Some(channels)) => {
            Some(rate * u32::from(bits) / 8 * u32::from(channels))
        }
        _ => track.bitrate_bps.map(|bps| bps / 8),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn items(tracks: &[Track], parent: &ObjectId, to: To<'_>) -> String {
        let items: Vec<Child<'_>> = tracks
            .iter()
            .map(|track| Child::Item(track, parent))
            .collect();
        children(&items, to)
    }

    fn track() -> Track {
        Track {
            id: ObjectId::new("t0123456789abcdef").unwrap(),
            rule: crate::index::Rule::Strings,
            album_id: None,
            album_inherited: false,
            title_tagged: true,
            compilation: false,
            path: PathBuf::from("/music/01.flac"),
            relative: "01 - Juana Peña.flac".to_owned(),
            title: "Juana Peña".to_owned(),
            artists: vec![Credit::new("Sierra Maestra")],
            album_artists: vec![Credit::new("Sierra Maestra")],
            composers: Vec::new(),
            album: Some("!Dundunbanza!".to_owned()),
            genres: vec!["Latin".to_owned()],
            date: Some("1994-01-01".to_owned()),
            track_number: Some(1),
            disc_number: None,
            duration: Duration::from_millis(243_026),
            size: 27_296_681,
            mime: "audio/x-flac",
            sample_rate: Some(44_100),
            bit_depth: Some(16),
            channels: Some(2),
            bitrate_bps: Some(1_411_000),
            artwork: None,
            recording_mbid: None,
            date_added: None,
            disc_from_title: false,
            disc_subtitle: None,
            work: None,
            grouping: None,
        }
    }

    #[test]
    fn duration_matches_the_reference_format() {
        assert_eq!(
            format_duration(Duration::from_millis(243_026)),
            "0:04:03.026"
        );
        assert_eq!(format_duration(Duration::ZERO), "0:00:00.000");
        assert_eq!(format_duration(Duration::from_secs(3661)), "1:01:01.000");
    }

    #[test]
    fn a_client_profile_decides_the_role_element_and_whether_values_are_joined() {
        let mut track = track();
        track.artists = vec![Credit::new("Duke Ellington"), Credit::new("Johnny Hodges")];
        track.composers = vec![Credit::new("Billy Strayhorn")];
        let parent = ObjectId::root();

        let plain = items(&[track.clone()], &parent, To::plain("http://h"));
        assert!(plain.contains("<upnp:artist>Duke Ellington</upnp:artist>"));
        assert!(plain.contains("<upnp:artist>Johnny Hodges</upnp:artist>"));
        assert!(plain.contains(r#"<upnp:artist role="Composer">Billy Strayhorn</upnp:artist>"#));
        assert!(!plain.contains("upnp:author"));

        let client = Profile {
            roles: Roles::Both,
            join: Some(", ".to_owned()),
            ..Profile::default()
        };
        let adapted = items(&[track], &parent, To::new("http://h", &client));
        assert!(adapted.contains("<upnp:artist>Duke Ellington, Johnny Hodges</upnp:artist>"));
        assert!(adapted.contains(r#"<upnp:artist role="Composer">Billy Strayhorn</upnp:artist>"#));
        assert!(adapted.contains(r#"<upnp:author role="Composer">Billy Strayhorn</upnp:author>"#));
    }

    #[test]
    fn a_profile_name_is_emitted_only_where_the_file_conforms_to_one() {
        let flac = track();
        assert_eq!(
            protocol_info(&flac, crate::upnp::client::conservative()),
            format!("http-get:*:audio/x-flac:{PROTOCOL_FLAGS}")
        );

        let mut mp3 = track();
        mp3.mime = "audio/mpeg";
        mp3.bit_depth = None;
        mp3.bitrate_bps = Some(320_000);
        assert_eq!(
            protocol_info(&mp3, crate::upnp::client::conservative()),
            format!("http-get:*:audio/mpeg:DLNA.ORG_PN=MP3;{PROTOCOL_FLAGS}")
        );

        let mut aac = track();
        aac.mime = "audio/mp4";
        aac.bit_depth = None;
        aac.bitrate_bps = Some(256_000);
        assert_eq!(
            protocol_info(&aac, crate::upnp::client::conservative()),
            format!("http-get:*:audio/mp4:DLNA.ORG_PN=AAC_ISO_320;{PROTOCOL_FLAGS}")
        );

        let mut alac = track();
        alac.mime = "audio/mp4";
        assert_eq!(
            protocol_info(&alac, crate::upnp::client::conservative()),
            format!("http-get:*:audio/mp4:{PROTOCOL_FLAGS}")
        );

        let mut unknown = track();
        unknown.mime = "audio/mp4";
        unknown.bit_depth = None;
        unknown.sample_rate = None;
        unknown.channels = None;
        unknown.bitrate_bps = None;
        assert_eq!(
            protocol_info(&unknown, crate::upnp::client::conservative()),
            format!("http-get:*:audio/mp4:{PROTOCOL_FLAGS}")
        );
    }

    #[test]
    fn bitrate_is_bytes_per_second_as_the_reference_emits_it() {
        assert_eq!(byte_rate(&track()), Some(176_400));
    }

    #[test]
    fn a_cap_is_measured_on_the_bytes_rather_than_on_the_advertised_rate() {
        let flac = track();
        assert_eq!(
            stream_rate(&flac),
            Some(112_320),
            "27,296,681 bytes over 243.026 seconds"
        );
        assert_eq!(capped(&flac, 1.0), Some(112_320));
        assert_eq!(capped(&flac, 2.0), Some(224_640));
        assert_eq!(
            capped(&flac, 0.5),
            Some(112_320),
            "a cap below real time is real time"
        );

        let mut unmeasured = track();
        unmeasured.duration = Duration::ZERO;
        assert_eq!(stream_rate(&unmeasured), Some(176_400));
    }

    #[test]
    fn the_item_carries_what_a_renderer_needs() {
        let parent = ObjectId::root();
        let xml = items(&[track()], &parent, To::plain("http://192.0.2.1:8200"));
        for expected in [
            r#"<dc:title>Juana Peña</dc:title>"#,
            r#"<upnp:artist role="AlbumArtist">Sierra Maestra</upnp:artist>"#,
            r#"<upnp:originalTrackNumber>1</upnp:originalTrackNumber>"#,
            r#"duration="0:04:03.026""#,
            r#"size="27296681""#,
            r#"bitrate="176400""#,
            r#"protocolInfo="http-get:*:audio/x-flac:DLNA.ORG_OP=01"#,
            r#"<upnp:class>object.item.audioItem.musicTrack</upnp:class>"#,
            "http://192.0.2.1:8200/media/t0123456789abcdef",
        ] {
            assert!(xml.contains(expected), "missing {expected} in:\n{xml}");
        }
    }

    #[test]
    fn every_artist_value_is_emitted_rather_than_joined() {
        let mut t = track();
        t.album_artists = vec![
            Credit::new("Christian Thielemann"),
            Credit::new("Wiener Philharmoniker"),
        ];
        let xml = items(&[t], &ObjectId::root(), To::plain("http://h"));
        assert_eq!(xml.matches(r#"role="AlbumArtist""#).count(), 2);
    }

    #[test]
    fn titles_needing_escaping_do_not_break_the_document() {
        let mut t = track();
        t.title = "Rock & Roll <live>".to_owned();
        let xml = items(&[t], &ObjectId::root(), To::plain("http://h"));
        assert!(xml.contains("Rock &amp; Roll &lt;live&gt;"), "{xml}");
        assert!(!xml.contains("<live>"));
    }

    #[test]
    fn a_bare_year_is_padded_the_way_the_reference_pads_it() {
        assert_eq!(iso_date("1994"), "1994-01-01");
        assert_eq!(iso_date("1994-05"), "1994-05-01");
        assert_eq!(iso_date("1994-05-17"), "1994-05-17");
        assert_eq!(iso_date(" 2012 "), "2012-01-01");
        assert_eq!(iso_date("circa 1600"), "circa 1600");
    }

    #[test]
    fn an_empty_listing_is_a_well_formed_document_not_an_error() {
        let xml = items(&[], &ObjectId::root(), To::plain("http://h"));
        assert!(xml.starts_with("<DIDL-Lite"));
        assert!(xml.ends_with("</DIDL-Lite>"));
    }
}
