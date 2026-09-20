//! What the server is doing, as a page reads it and as a shell greps it.

use axum::extract::State;
use axum::http::HeaderMap;
use axum::response::Response;
use serde::Serialize;

use crate::index::{Cause, Coverage, Origin, Refusal, Refusals, Reported};
use crate::service::Outcome;
use crate::upnp::gena::Listed;
use crate::upnp::peers::{self, Seen};

use super::{Shared, answer, refusals_now};

#[derive(Debug, Serialize)]
struct Status {
    name: String,
    version: &'static str,
    udn: String,
    state: &'static str,
    uptime_seconds: u64,
    system_update_id: u32,
    library: LibraryCounts,
    store: StoreStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    memory_bytes: Option<u64>,
    /// What the last pass did, or nothing where none has run since this process started.
    last_pass: Option<LastPass>,
    /// Set while the library is not answered for, which is the state the page has to show.
    #[serde(skip_serializing_if = "Option::is_none")]
    failing: Option<Failing>,
    refused: Refused,
    /// Background tasks that stopped while the server went on answering.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    stopped: Vec<&'static str>,
    subscribers: Vec<Listed>,
    /// Every address seen on the network, the most recently active first.
    devices: Vec<DeviceSeen>,
}

/// One address, what it did and when, and what that says about how far it got.
#[derive(Debug, Serialize)]
struct DeviceSeen {
    address: String,
    name: Option<String>,
    searched: Option<Seen>,
    described: Option<Seen>,
    subscribed: Option<Seen>,
    browsed: Option<Seen>,
    played: Option<Seen>,
    says: &'static str,
}

impl From<peers::Listed> for DeviceSeen {
    fn from(seen: peers::Listed) -> Self {
        Self {
            says: says_of(&seen),
            address: seen.address,
            name: seen.name,
            searched: seen.searched,
            described: seen.described,
            subscribed: seen.subscribed,
            browsed: seen.browsed,
            played: seen.played,
        }
    }
}

/// How far a device got, in the words a listener can act on.
fn says_of(seen: &peers::Listed) -> &'static str {
    let (searched, described) = (seen.searched.is_some(), seen.described.is_some());
    let (browsed, played, listening) = (
        seen.browsed.is_some(),
        seen.played.is_some(),
        seen.subscribed.is_some(),
    );
    match (searched, described, browsed, played, listening) {
        (true, false, false, false, false) => {
            "found this server but has not read its description since this start: it may know it \
             from before, or the reply did not reach it"
        }
        (false, _, false, false, false) => {
            "found this server by its announcement or by a given address, never by searching here"
        }
        (_, _, false, false, false) => "read the description and never came back",
        (_, _, false, false, true) => "listening for changes, and not browsing yet",
        // A renderer fetches what a control point chose and never asks for a listing itself.
        (_, _, false, true, _) => "playing what another device chose for it",
        (_, _, true, true, true) => "browsing, playing and listening for changes",
        (_, _, true, true, false) => "browsing and playing",
        (_, _, true, false, true) => "browsing and listening for changes",
        (_, _, true, false, false) => "browsing",
    }
}

#[derive(Debug, Serialize)]
struct LibraryCounts {
    tracks: usize,
    albums: usize,
    artists: usize,
    playlists: usize,
    /// Tracks no tag axis reaches.
    untagged: usize,
    /// How many tracks carry each tag.
    coverage: Coverage,
}

#[derive(Debug, Serialize)]
struct StoreStatus {
    #[serde(skip_serializing_if = "Option::is_none")]
    path: Option<String>,
    /// False where no store could be opened, which is every file read on every start.
    open: bool,
}

#[derive(Debug, Serialize)]
struct LastPass {
    /// When it ended, in seconds since the epoch, so a reader can subtract its own clock.
    ended: i64,
    seconds: f32,
    /// Whether it answered for the whole tree or only for the folders a change named.
    whole_tree: bool,
    found: usize,
    opened: usize,
    reused: usize,
    complete: bool,
    published: bool,
    /// What became of it, and why where that is not an answer about the library.
    #[serde(flatten)]
    outcome: Outcome,
    /// Whether the store kept what the pass learned; false is why every pass after walks the tree.
    stored: bool,
}

/// Why the library is not answered for, and since when, or nothing while it is.
#[derive(Debug, Serialize)]
struct Failing {
    since: i64,
    why: String,
}

/// What was refused, with the way to the list behind every count.
#[derive(Debug, Serialize)]
struct Refused {
    total: usize,
    walked: bool,
    causes: Vec<RefusedCause>,
}

/// One cause, with the words a reader is shown beside the count.
#[derive(Debug, Serialize)]
struct RefusedCause {
    cause: Cause,
    origin: Origin,
    says: &'static str,
    total: usize,
    shown: Vec<Refusal>,
}

impl From<Reported> for RefusedCause {
    fn from(reported: Reported) -> Self {
        Self {
            cause: reported.cause,
            origin: reported.origin,
            says: crate::report::label(reported.cause),
            total: reported.total,
            shown: reported.shown,
        }
    }
}

fn causes_of(refusals: &Refusals) -> Vec<RefusedCause> {
    refusals
        .reported()
        .into_iter()
        .map(RefusedCause::from)
        .collect()
}

pub(super) async fn status(State(control): State<Shared>, headers: HeaderMap) -> Response {
    let device = &control.device;
    let served = device.served();
    let library = &served.library;
    let last = control.passes.last();
    let refusals = refusals_now(&control, library);
    let operation = control.operation();
    let status = Status {
        name: device.identity.friendly_name.clone(),
        version: env!("CARGO_PKG_VERSION"),
        udn: device.identity.udn.clone(),
        state: if control.passes.running() {
            "indexing"
        } else {
            "serving"
        },
        uptime_seconds: control.uptime().as_secs(),
        system_update_id: device.system_update_id(),
        library: LibraryCounts {
            tracks: library.len(),
            albums: library.albums().len(),
            artists: library.artists().len(),
            playlists: library.playlists().len(),
            untagged: served.counts.untagged,
            coverage: served.counts.coverage.clone(),
        },
        store: StoreStatus {
            path: operation
                .store
                .as_ref()
                .map(|path| path.display().to_string()),
            open: operation.store.is_some(),
        },
        memory_bytes: resident_bytes(),
        last_pass: last.as_deref().map(|pass| LastPass {
            ended: pass.ended,
            seconds: pass.seconds,
            whole_tree: pass.whole_tree,
            found: pass.found,
            opened: pass.opened,
            reused: pass.reused,
            complete: pass.complete,
            published: pass.published(),
            outcome: pass.outcome.clone(),
            stored: pass.stored,
        }),
        failing: control.passes.failing_since().map(|since| Failing {
            since,
            why: last
                .as_deref()
                .and_then(|pass| pass.outcome.why())
                .unwrap_or("the last pass answered nothing about the library")
                .to_owned(),
        }),
        refused: Refused {
            total: refusals.total(),
            walked: last.is_some(),
            causes: causes_of(&refusals),
        },
        stopped: control.tasks.gone(),
        subscribers: device.subscriptions.listed(),
        devices: device
            .peers
            .listed()
            .into_iter()
            .map(DeviceSeen::from)
            .collect(),
    };
    answer(&headers, &status, || flatten_status(&status))
}

/// The status as lines a shell can read, where a nested value is a prefixed key.
fn flatten_status(status: &Status) -> Vec<(String, String)> {
    let mut lines = counted(status);
    if let Some(bytes) = status.memory_bytes {
        lines.push(("memory_bytes".to_owned(), bytes.to_string()));
    }
    lines.extend(pass_lines(status.last_pass.as_ref()));
    if let Some(failing) = &status.failing {
        lines.push((
            "failing.since".to_owned(),
            format!("{}\t{}", failing.since, failing.why),
        ));
    }
    for task in &status.stopped {
        lines.push(("stopped".to_owned(), (*task).to_owned()));
    }
    lines.push(("refused.total".to_owned(), status.refused.total.to_string()));
    lines.push((
        "refused.walked".to_owned(),
        status.refused.walked.to_string(),
    ));
    for cause in &status.refused.causes {
        lines.extend(cause_lines(cause));
    }
    for (at, subscriber) in status.subscribers.iter().enumerate() {
        lines.push((
            format!("subscriber.{}", at + 1),
            format!(
                "{} {}\t{}\trenews in {}s",
                subscriber.service,
                subscriber.callback.as_deref().unwrap_or("-"),
                subscriber.user_agent.as_deref().unwrap_or("unnamed"),
                subscriber.renews_in_seconds
            ),
        ));
    }
    let now = peers::epoch_seconds();
    for (at, device) in status.devices.iter().enumerate() {
        lines.extend(device_lines(at + 1, device, now));
    }
    lines
}

/// One device: who and how far, then how long ago each step was, or never.
fn device_lines(at: usize, device: &DeviceSeen, now: i64) -> Vec<(String, String)> {
    let ago = |seen: Option<Seen>| match seen {
        Some(seen) => format!(
            "{}s ago, {} times",
            now.saturating_sub(seen.last),
            seen.times
        ),
        None => "never".to_owned(),
    };
    vec![
        (
            format!("device.{at}"),
            format!(
                "{}\t{}\t{}",
                device.address,
                device.name.as_deref().unwrap_or("unnamed"),
                device.says
            ),
        ),
        (format!("device.{at}.searched"), ago(device.searched)),
        (format!("device.{at}.described"), ago(device.described)),
        (format!("device.{at}.subscribed"), ago(device.subscribed)),
        (format!("device.{at}.browsed"), ago(device.browsed)),
        (format!("device.{at}.played"), ago(device.played)),
    ]
}

/// What the server is and how much of it there is.
fn counted(status: &Status) -> Vec<(String, String)> {
    let line = |key: &str, value: String| (key.to_owned(), value);
    let coverage = &status.library.coverage;
    let mut lines = vec![
        line("name", status.name.clone()),
        line("version", status.version.to_owned()),
        line("udn", status.udn.clone()),
        line("state", status.state.to_owned()),
        line("uptime_seconds", status.uptime_seconds.to_string()),
        line("system_update_id", status.system_update_id.to_string()),
        line("tracks", status.library.tracks.to_string()),
        line("albums", status.library.albums.to_string()),
        line("artists", status.library.artists.to_string()),
        line("playlists", status.library.playlists.to_string()),
        line("untagged", status.library.untagged.to_string()),
        line(
            "store",
            status
                .store
                .path
                .clone()
                .unwrap_or_else(|| "none".to_owned()),
        ),
    ];
    for (field, count) in coverage.fields() {
        lines.push((
            format!("coverage.{field}"),
            format!("{count}/{}", coverage.tracks),
        ));
    }
    lines
}

/// What the last pass did, or the one line saying none has.
fn pass_lines(pass: Option<&LastPass>) -> Vec<(String, String)> {
    let Some(pass) = pass else {
        return vec![(
            "last_pass".to_owned(),
            "none since this process started".to_owned(),
        )];
    };
    let line = |key: &str, value: String| (format!("last_pass.{key}"), value);
    vec![
        line("ended", pass.ended.to_string()),
        line("seconds", pass.seconds.to_string()),
        line("whole_tree", pass.whole_tree.to_string()),
        line("found", pass.found.to_string()),
        line("opened", pass.opened.to_string()),
        line("reused", pass.reused.to_string()),
        line("complete", pass.complete.to_string()),
        line("published", pass.published.to_string()),
        line(
            "outcome",
            match pass.outcome.why() {
                Some(why) => format!("{}\t{why}", pass.outcome.as_str()),
                None => pass.outcome.as_str().to_owned(),
            },
        ),
        line("stored", pass.stored.to_string()),
    ]
}

/// One cause: what it is, how many there were, and the ones the bound kept.
fn cause_lines(cause: &RefusedCause) -> Vec<(String, String)> {
    let name = cause.cause.as_str();
    let mut lines = vec![
        (format!("refused.{name}.total"), cause.total.to_string()),
        (
            format!("refused.{name}.origin"),
            cause.origin.as_str().to_owned(),
        ),
        (format!("refused.{name}.says"), cause.says.to_owned()),
    ];
    for (at, refusal) in cause.shown.iter().enumerate() {
        let detail = refusal
            .detail
            .as_deref()
            .map(|detail| format!("\t{detail}"))
            .unwrap_or_default();
        lines.push((
            format!("refused.{name}.{}", at + 1),
            format!("{}{detail}", refusal.subject),
        ));
    }
    lines
}

fn resident_bytes() -> Option<u64> {
    resident_in(&std::fs::read_to_string("/proc/self/status").ok()?)
}

/// `VmRSS` out of `/proc/self/status`, which is the field that is already scaled.
fn resident_in(status: &str) -> Option<u64> {
    let line = status.lines().find(|line| line.starts_with("VmRSS:"))?;
    let mut fields = line.split_whitespace().skip(1);
    let value: u64 = fields.next()?.parse().ok()?;
    match fields.next() {
        Some("kB") => Some(value * 1024),
        // The kernel has written kB there since the field existed; anything else is not this field.
        other => {
            tracing::warn!(
                unit = other,
                "VmRSS is not in kB: no memory figure is reported"
            );
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The line is taken verbatim off the target NAS over SSH, not from memory of the format.
    #[test]
    fn the_memory_figure_is_read_in_the_units_the_kernel_writes() {
        let status = "VmPeak:\t   12345 kB\nVmHWM:\t     428 kB\nVmRSS:\t     428 kB\n";
        assert_eq!(resident_in(status), Some(428 * 1024));
        assert_eq!(resident_in("VmHWM:\t 428 kB\n"), None, "the wrong field");
        assert_eq!(
            resident_in("VmRSS:\t 428 pages\n"),
            None,
            "a unit this does not understand is no answer rather than a wrong one"
        );
    }

    fn a_status() -> Status {
        let mut refusals = Refusals::default();
        refusals.refuse(Cause::MissingEntry, "set.m3u", None);
        Status {
            name: "Kantele".to_owned(),
            version: "0.0.0",
            udn: "uuid:test".to_owned(),
            state: "serving",
            uptime_seconds: 1,
            system_update_id: 1,
            library: LibraryCounts {
                tracks: 1,
                albums: 1,
                artists: 1,
                playlists: 1,
                untagged: 1,
                coverage: Coverage {
                    tracks: 1,
                    artist: 1,
                    ..Coverage::default()
                },
            },
            store: StoreStatus {
                path: Some("/state/index.sqlite".to_owned()),
                open: true,
            },
            memory_bytes: Some(1),
            stopped: Vec::new(),
            last_pass: Some(LastPass {
                ended: 1,
                seconds: 1.0,
                whole_tree: true,
                found: 1,
                opened: 1,
                reused: 1,
                complete: true,
                published: false,
                outcome: Outcome::Kept {
                    why: "the music folder read as empty".to_owned(),
                },
                stored: true,
            }),
            failing: Some(Failing {
                since: 1,
                why: "the music folder read as empty".to_owned(),
            }),
            refused: Refused {
                total: 1,
                walked: true,
                causes: causes_of(&refusals),
            },
            subscribers: vec![Listed {
                sid: "uuid:one".to_owned(),
                service: "ContentDirectory",
                callback: Some("http://192.0.2.1:9/notify".to_owned()),
                user_agent: Some("Yamaha".to_owned()),
                renews_in_seconds: 1,
                events: 1,
            }],
            devices: vec![DeviceSeen::from(peers::Listed {
                address: "192.0.2.1".to_owned(),
                name: Some("Yamaha".to_owned()),
                searched: Some(Seen { last: 1, times: 1 }),
                described: None,
                subscribed: None,
                browsed: None,
                played: None,
            })],
        }
    }

    fn seen_at(steps: &[&str]) -> peers::Listed {
        let at = |name: &str| steps.contains(&name).then_some(Seen { last: 1, times: 1 });
        peers::Listed {
            address: "192.0.2.1".to_owned(),
            name: None,
            searched: at("searched"),
            described: at("described"),
            subscribed: at("subscribed"),
            browsed: at("browsed"),
            played: at("played"),
        }
    }

    #[test]
    fn what_a_device_did_says_where_discovery_stopped() {
        assert!(says_of(&seen_at(&["searched"])).starts_with("found this server but has not read"));
        assert!(
            says_of(&seen_at(&["described"])).starts_with("found this server by its announcement")
        );
        assert_eq!(
            says_of(&seen_at(&["browsed"])),
            "browsing",
            "a device browsing off a remembered address is browsing, whatever it skipped"
        );
        assert_eq!(
            says_of(&seen_at(&["searched", "described"])),
            "read the description and never came back"
        );
        assert_eq!(
            says_of(&seen_at(&["searched", "described", "browsed"])),
            "browsing"
        );
        assert_eq!(
            says_of(&seen_at(&["described", "browsed", "played", "subscribed"])),
            "browsing, playing and listening for changes"
        );
        assert_eq!(
            says_of(&seen_at(&["searched", "described", "played"])),
            "playing what another device chose for it",
            "a renderer never asks for a listing; the control point did that"
        );
        assert_eq!(
            says_of(&seen_at(&["described", "subscribed"])),
            "listening for changes, and not browsing yet",
            "a device that subscribes before it browses is not browsing"
        );
        assert_eq!(
            says_of(&seen_at(&["described", "browsed", "subscribed"])),
            "browsing and listening for changes"
        );
    }

    #[test]
    fn a_device_is_lines_a_shell_can_read_with_never_where_a_step_did_not_happen() {
        let device = DeviceSeen::from(seen_at(&["searched", "browsed"]));
        let lines = device_lines(2, &device, 11);
        assert_eq!(lines[0].0, "device.2");
        assert!(lines[0].1.starts_with("192.0.2.1\tunnamed\t"));
        assert_eq!(
            lines[1],
            (
                "device.2.searched".to_owned(),
                "10s ago, 1 times".to_owned()
            )
        );
        assert_eq!(
            lines[2],
            ("device.2.described".to_owned(), "never".to_owned())
        );
    }

    /// The two renderings are written by hand and could drift, so this is what holds them together.
    #[test]
    fn every_field_of_the_status_reaches_both_renderings() {
        let status = a_status();
        let json = serde_json::to_value(&status).expect("the status serialises");
        let keys: Vec<&str> = json
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        let lines = flatten_status(&status);
        let carried = |prefix: &str| {
            lines
                .iter()
                .any(|(key, _)| key == prefix || key.starts_with(&format!("{prefix}.")))
        };

        for key in &keys {
            let line = match *key {
                // The counts are named on their own, because a shell greps `tracks`, not `library`.
                "library" => "tracks",
                // One line naming the file, since a path that is absent is the whole of the answer.
                "store" => "store",
                "subscribers" => "subscriber.1",
                "devices" => "device.1",
                other => other,
            };
            assert!(
                carried(line),
                "{key} is in the JSON and in no line of the plain text"
            );
        }
        assert!(
            keys.contains(&"refused") && carried("refused.missing-entry"),
            "a cause is keyed by its name rather than by its position in a list"
        );
        assert!(
            lines.contains(&("coverage.artist".to_owned(), "1/1".to_owned()))
                && lines.contains(&("coverage.date".to_owned(), "0/1".to_owned())),
            "every tag's coverage is a line of its own, as a count over the total"
        );
    }
}
