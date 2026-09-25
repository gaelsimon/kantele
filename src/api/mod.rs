//! The control interface: what the server is doing, what it was told to do, and how to ask again.

use std::borrow::Cow;
use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use arc_swap::ArcSwap;
use axum::Extension;
use axum::Router;
use axum::extract::{ConnectInfo, Query, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::Serialize;

pub mod describe;
pub mod files;
pub mod folders;
pub mod log;
pub mod menu;
pub mod settings;
pub mod shares;
pub mod status;
pub mod track;

use crate::config::Resolved;
use crate::index::Refusals;
use crate::service::{Asked, Live, Pass, Passes, Scope};
use crate::upnp::device::Device;

/// What this process is running as, which nothing in the index knows.
#[derive(Clone, Debug, Default)]
pub struct Operation {
    /// Where the store is, or nothing where none could be opened.
    pub store: Option<PathBuf>,
    /// Every setting in force, and the layers to read them from again.
    pub config: Resolved,
}

/// What the interface knows of its own, and the device and the passes it reports on.
pub struct Control {
    pub device: Arc<Device>,
    pub passes: Arc<Passes>,
    /// The indexing settings in force, which a write replaces and the pass loop reads.
    pub indexing: Arc<Live>,
    /// The background tasks, and which of them are no longer running.
    pub tasks: crate::tasks::Watched,
    operation: ArcSwap<Operation>,
    started: Instant,
    /// Held across a whole settings write, so two saves at once land one after the other.
    settings_writes: tokio::sync::Mutex<()>,
}

impl Control {
    pub fn new(device: Arc<Device>, passes: Arc<Passes>) -> Self {
        Self {
            device,
            passes,
            indexing: Arc::new(Live::default()),
            tasks: crate::tasks::Watched::default(),
            operation: ArcSwap::from_pointee(Operation::default()),
            started: Instant::now(),
            settings_writes: tokio::sync::Mutex::new(()),
        }
    }

    pub fn operation(&self) -> Arc<Operation> {
        self.operation.load_full()
    }

    pub fn set_operation(&self, operation: Operation) {
        self.operation.store(Arc::new(operation));
    }

    pub fn uptime(&self) -> Duration {
        self.started.elapsed()
    }
}

type Shared = Arc<Control>;

/// The address a request came from, which only a real listener hands over.
type Peer = Option<Extension<ConnectInfo<SocketAddr>>>;

/// The page, in the binary. A NAS may have no route to the internet.
const PAGE: &str = include_str!("../../assets/web/index.html");

/// Everything refused, as any of the three readers of it wants it: what the last pass found leads,
/// because building the index from a store carries refusals a pass has since answered for.
fn refusals_now(control: &Control, library: &crate::index::Library) -> Refusals {
    let Some(last) = control.passes.last() else {
        return library.refusals().clone();
    };
    let mut refusals = last.refusals.clone();
    refusals.absorb(library.refusals().clone());
    refusals
}

pub fn router(control: Shared) -> Router {
    Router::new()
        .route("/config", get(page))
        .route("/favicon.ico", get(favicon))
        .route("/api/status", get(status::status))
        .route("/api/log", get(log::tail))
        .route("/api/progress", get(progress))
        .route(
            "/api/config",
            get(settings::configuration).put(settings::write_configuration),
        )
        .route("/api/folders", get(folder_tree))
        .route("/api/files", get(folder_files))
        .route("/api/menu", get(menu_level))
        .route("/api/track", get(track_detail))
        .route("/api/problems", get(problem_files))
        .route("/api/shares", get(share_listing))
        .route("/api/rescan", post(rescan))
        .with_state(control)
}

/// What the icon in a NAS application menu opens.
async fn page() -> Response {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], PAGE).into_response()
}

/// The picture a browser asks for on its own, which is the one a control point is offered.
async fn favicon(State(control): State<Shared>) -> Response {
    match control
        .device
        .identity
        .icon
        .served(crate::upnp::icon::PREFERRED)
    {
        Some((mime, bytes)) => (
            [
                (header::CONTENT_TYPE, mime),
                (header::CACHE_CONTROL, "max-age=86400"),
            ],
            bytes.to_vec(),
        )
            .into_response(),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// How far the pass underway has got, on a route of its own: the status counts the whole library
/// on every read, and a strip asks every second.
async fn progress(State(control): State<Shared>, headers: HeaderMap) -> Response {
    let reached = control.indexing.now().underway.progress.reached();
    answer(&headers, &reached, || {
        let mut lines = vec![
            ("phase".to_owned(), format!("{:?}", reached.phase)),
            ("folders".to_owned(), reached.folders.to_string()),
            ("found".to_owned(), reached.found.to_string()),
            ("read".to_owned(), reached.read.to_string()),
        ];
        if let Some(began) = reached.began {
            lines.push(("began".to_owned(), began.to_string()));
        }
        lines
    })
}

fn not_in_library(asked: &str, kind: &str) -> Response {
    (
        StatusCode::NOT_FOUND,
        format!("{asked} is no {kind} of this library\n"),
    )
        .into_response()
}

async fn menu_level(
    State(control): State<Shared>,
    headers: HeaderMap,
    Query(asked): Query<menu::Asked>,
) -> Response {
    let served = control.device.served();
    let Some(level) = menu::level(&served, &asked) else {
        return not_in_library(&asked.at, "menu");
    };
    answer(&headers, &level, || menu::lines(&level))
}

async fn folder_files(
    State(control): State<Shared>,
    headers: HeaderMap,
    Query(asked): Query<files::Asked>,
) -> Response {
    let served = control.device.served();
    let failed = |folder: &str| folders::named_by(&refusals_now(&control, &served.library), folder);
    let Some(listing) = files::listing(&served, &asked, failed) else {
        return not_in_library(&asked.folder, "folder");
    };
    answer(&headers, &listing, || files::lines(&listing))
}

async fn track_detail(
    State(control): State<Shared>,
    headers: HeaderMap,
    Query(asked): Query<track::Asked>,
) -> Response {
    let served = control.device.served();
    let Some(found) = track::found(&served, &asked) else {
        return not_in_library(&asked.path, "track");
    };
    answer(&headers, &found, || track::lines(&found))
}

async fn folder_tree(
    State(control): State<Shared>,
    headers: HeaderMap,
    Query(asked): Query<folders::Asked>,
) -> Response {
    let served = control.device.served();
    let last = control.passes.last();
    let refusals = refusals_now(&control, &served.library);
    // A pass over the whole tree read every folder, which marks none of them out.
    let walked = last
        .as_deref()
        .filter(|pass| !pass.whole_tree)
        .map(|pass| &pass.covered);
    let Some(listing) = folders::listing(&served, &refusals, walked, &asked) else {
        return not_in_library(&asked.under, "folder");
    };
    answer(&headers, &listing, || folder_lines(&listing))
}

#[derive(serde::Deserialize)]
struct AskedProblems {
    #[serde(default)]
    folder: String,
    /// A refusal cause, or `no-artwork`, which the library answers for rather than the refusals.
    cause: String,
}

/// The files behind one line of a folder row. Only the first hundred of each cause are kept, so
/// `shown` can be shorter than `total` or empty.
#[derive(serde::Serialize)]
struct ProblemFiles {
    folder: String,
    cause: String,
    label: &'static str,
    total: usize,
    shown: Vec<crate::index::Refusal>,
}

async fn problem_files(
    State(control): State<Shared>,
    headers: HeaderMap,
    Query(asked): Query<AskedProblems>,
) -> Response {
    let served = control.device.served();
    let (answered, total) = match asked.cause.as_str() {
        folders::NO_ARTWORK => folders::tracks_lacking_artwork(&served, &asked.folder),
        named => {
            let Some(cause) = crate::index::Cause::ALL
                .iter()
                .find(|cause| cause.as_str() == named)
            else {
                return (
                    StatusCode::BAD_REQUEST,
                    format!("{named} is nothing this server refuses for\n"),
                )
                    .into_response();
            };
            let refusals = refusals_now(&control, &served.library);
            // Counted off the folder tallies: only the first hundred of a cause are kept.
            (
                refusals
                    .held_under(*cause, &asked.folder)
                    .into_iter()
                    .cloned()
                    .collect(),
                refusals.total_under(*cause, &asked.folder),
            )
        }
    };
    let label = crate::index::Cause::ALL
        .iter()
        .find(|cause| cause.as_str() == asked.cause)
        .map_or("No cover art", |cause| crate::report::label(*cause));
    let answered = ProblemFiles {
        folder: asked.folder,
        cause: asked.cause,
        label,
        total,
        shown: answered,
    };
    let lines = || {
        answered
            .shown
            .iter()
            .enumerate()
            .map(|(at, refusal)| {
                let detail = refusal.detail.as_deref().unwrap_or_default();
                (
                    format!("file.{}", at + 1),
                    format!("{}\t{detail}", refusal.subject),
                )
            })
            .collect()
    };
    answer(&headers, &answered, lines)
}

fn folder_lines(listing: &folders::Listing) -> Vec<(String, String)> {
    let row = |row: &folders::Row| {
        format!(
            "{}\t{} tracks\t{} albums\t{} problems\t{} notes\t{} checks\t{}",
            row.path, row.tracks, row.albums, row.problems, row.notes, row.checks, row.says
        )
    };
    let mut lines = vec![("under".to_owned(), row(&listing.here))];
    for (at, folder) in listing.folders.iter().enumerate() {
        lines.push((format!("folder.{}", at + 1), row(folder)));
    }
    lines.push(("more".to_owned(), listing.more.to_string()));
    lines
}

async fn share_listing(
    State(control): State<Shared>,
    peer: Peer,
    headers: HeaderMap,
    Query(asked): Query<shares::Asked>,
) -> Response {
    if let Some(refused) = refused(
        peer,
        &headers,
        "a listing of the shares",
        "the shares may only be listed from this server's own page\n",
    ) {
        return refused;
    }
    let serving = control.indexing.now().roots.clone();
    match shares::listing(&serving, &asked) {
        Ok(listing) => answer(&headers, &listing, || {
            let mut lines = vec![(
                "above".to_owned(),
                listing.above.clone().unwrap_or_else(|| "none".to_owned()),
            )];
            for (at, entry) in listing.entries.iter().enumerate() {
                lines.push((
                    format!("entry.{}", at + 1),
                    format!(
                        "{}\t{}",
                        entry.path,
                        if entry.folder { "folder" } else { "file" }
                    ),
                ));
            }
            lines
        }),
        Err(shares::Refused::Outside) => (
            StatusCode::FORBIDDEN,
            "this listing answers for the shares and what is under them, and for nothing else\n",
        )
            .into_response(),
        Err(shares::Refused::Denied(path)) => (
            StatusCode::FORBIDDEN,
            format!(
                "{path}: this server may not read it. It reads as the user it runs as, which on a \
                 Synology is the package user, given read access to a shared folder in Control \
                 Panel.\n"
            ),
        )
            .into_response(),
        Err(shares::Refused::Unreadable(why)) => {
            (StatusCode::NOT_FOUND, format!("{why}\n")).into_response()
        }
    }
}

#[derive(Debug, Default, serde::Deserialize)]
struct Rescanning {
    /// One folder, by the path the tree gives it, or nothing for the whole library.
    folder: Option<String>,
}

#[derive(Debug, Serialize)]
struct Rescan {
    started: bool,
    says: String,
}

async fn rescan(
    State(control): State<Shared>,
    peer: Peer,
    headers: HeaderMap,
    Query(asked): Query<Rescanning>,
) -> Response {
    if let Some(refused) = refused(
        peer,
        &headers,
        "a pass asked for",
        "a rescan may only be asked for from this server's own page\n",
    ) {
        return refused;
    }
    let wanted = match asked
        .folder
        .as_deref()
        .map(str::trim)
        .filter(|f| !f.is_empty())
    {
        None => Pass::Whole,
        Some(folder) => {
            if !control.device.served().view.folders().holds(folder) {
                return (
                    StatusCode::NOT_FOUND,
                    format!("{folder} is no folder of this library\n"),
                )
                    .into_response();
            }
            Pass::Within(Scope::of([PathBuf::from(folder)]))
        }
    };
    let reading = wanted.describe();
    let (status, says) = match control.passes.request(wanted) {
        // A pass is queued behind a running one rather than refused.
        Asked::Queued if control.passes.running() => (
            StatusCode::ACCEPTED,
            format!("the library is being read now; {reading} will be read again after that"),
        ),
        Asked::Queued => (
            StatusCode::ACCEPTED,
            format!("{reading} will be read again"),
        ),
        Asked::AlreadyWaiting => (
            StatusCode::CONFLICT,
            format!("already waiting: {reading} will be read again"),
        ),
        Asked::NobodyIsListening => (
            StatusCode::SERVICE_UNAVAILABLE,
            "nothing in this process is watching the library, so it cannot be read again"
                .to_owned(),
        ),
    };
    let reply = Rescan {
        started: status == StatusCode::ACCEPTED,
        says,
    };
    let body = answer(&headers, &reply, || {
        vec![
            ("started".to_owned(), reply.started.to_string()),
            ("says".to_owned(), reply.says.clone()),
        ]
    });
    (status, body).into_response()
}

/// The refusal of a command from beyond this network or from another site's page, or nothing.
fn refused(peer: Peer, headers: &HeaderMap, what: &str, answer: &'static str) -> Option<Response> {
    let peer = peer.map(|Extension(ConnectInfo(address))| address.ip());
    from_outside(peer, what).or_else(|| from_elsewhere(headers, what, answer))
}

fn from_outside(peer: Option<IpAddr>, what: &str) -> Option<Response> {
    if peer.is_some_and(on_this_network) {
        return None;
    }
    tracing::warn!(
        peer = %peer.map_or_else(|| "?".to_owned(), |address| address.to_string()),
        "refusing {what} from outside this network"
    );
    Some(
        (
            StatusCode::FORBIDDEN,
            "only a device on this server's own network may ask for this\n",
        )
            .into_response(),
    )
}

/// Loopback, private and link-local, which a request through a forwarded port never comes from.
fn on_this_network(peer: IpAddr) -> bool {
    match peer.to_canonical() {
        IpAddr::V4(v4) => v4.is_loopback() || v4.is_private() || v4.is_link_local(),
        IpAddr::V6(v6) => v6.is_loopback() || v6.is_unique_local() || v6.is_unicast_link_local(),
    }
}

fn from_elsewhere(headers: &HeaderMap, what: &str, answer: &'static str) -> Option<Response> {
    if same_origin(headers) {
        return None;
    }
    tracing::warn!(
        origin = ?headers.get(header::ORIGIN).and_then(|value| value.to_str().ok()),
        host = ?headers.get(header::HOST).and_then(|value| value.to_str().ok()),
        "refusing {what} from another site"
    );
    Some((StatusCode::FORBIDDEN, answer).into_response())
}

fn same_origin(headers: &HeaderMap) -> bool {
    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return false;
    };
    // A name the world can resolve can be pointed at this server's address, and the page holding
    // it then sends an Origin matching its own Host. Comparing the two would pass.
    if !named_on_this_network(host) {
        return false;
    }
    let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) else {
        return true;
    };
    // Firefox sends `null` for a form post from a sandboxed or file-backed page.
    let Some(authority) = origin
        .trim()
        .strip_prefix("http://")
        .or_else(|| origin.trim().strip_prefix("https://"))
    else {
        return false;
    };
    host == authority
}

/// Suffixes nothing outside this network can be answering for.
const LOCAL_SUFFIXES: &[&str] = &[".local", ".home.arpa", ".internal", ".lan"];

/// Whether a `Host` is an address or a name only this network hands out. A bare name carries no
/// dot, so it cannot be a public one.
fn named_on_this_network(host: &str) -> bool {
    let name = host.rsplit_once(':').map_or(host, |(name, _)| name);
    let name = name.trim_start_matches('[').trim_end_matches(']');
    if name.parse::<std::net::IpAddr>().is_ok() {
        return true;
    }
    let lower = name.to_ascii_lowercase();
    !lower.contains('.') || LOCAL_SUFFIXES.iter().any(|end| lower.ends_with(end))
}

/// JSON where the caller asked for it, and greppable lines otherwise.
fn answer<T: Serialize>(
    headers: &HeaderMap,
    value: &T,
    lines: impl FnOnce() -> Vec<(String, String)>,
) -> Response {
    if wants_json(headers) {
        return match serde_json::to_string_pretty(value) {
            Ok(body) => (
                [(header::CONTENT_TYPE, "application/json; charset=utf-8")],
                body,
            )
                .into_response(),
            Err(error) => {
                tracing::error!(%error, "the status could not be rendered");
                StatusCode::INTERNAL_SERVER_ERROR.into_response()
            }
        };
    }
    let body: String = lines()
        .into_iter()
        .map(|(key, value)| format!("{key} = {}\n", one_line(&value)))
        .collect();
    ([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], body).into_response()
}

/// A value that cannot break the line it is written on.
fn one_line(value: &str) -> Cow<'_, str> {
    match value.chars().any(|c| c.is_control() && c != '\t') {
        false => Cow::Borrowed(value),
        true => Cow::Owned(
            value
                .chars()
                .map(|c| if c.is_control() && c != '\t' { ' ' } else { c })
                .collect(),
        ),
    }
}

/// Whether the caller asked for JSON.
fn wants_json(headers: &HeaderMap) -> bool {
    headers
        .get(header::ACCEPT)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|accept| accept.contains("application/json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn a_value_cannot_invent_a_line_the_way_a_tag_on_this_library_would() {
        assert_eq!(one_line("plain"), "plain");
        assert_eq!(
            one_line("a/track.flac\ttitle = 0"),
            "a/track.flac\ttitle = 0",
            "the tab separates the columns of one line and is not a break"
        );
        assert_eq!(
            one_line("a/track.flac\nrefused.total = 0"),
            "a/track.flac refused.total = 0",
            "a newline in a path or a playlist's own text would invent a key"
        );
        assert_eq!(one_line("bell\u{7}here"), "bell here");
    }

    fn asking(host: Option<&str>, origin: Option<&str>) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(host) = host {
            headers.insert(header::HOST, host.parse().expect("a host"));
        }
        if let Some(origin) = origin {
            headers.insert(header::ORIGIN, origin.parse().expect("an origin"));
        }
        headers
    }

    #[test]
    fn a_name_the_world_can_point_at_this_server_is_not_this_servers_own_page() {
        assert!(
            !same_origin(&asking(Some("evil.example"), Some("http://evil.example"))),
            "a page whose name was pointed here sends an Origin matching its own Host, and a \
             comparison of the two passes"
        );
        assert!(!same_origin(&asking(
            Some("evil.example:8200"),
            Some("http://evil.example:8200")
        )));
        assert!(
            !same_origin(&asking(Some("music.example.com"), None)),
            "and the same page reaches the write with no Origin at all"
        );
    }

    #[test]
    fn the_ways_an_owner_actually_opens_the_page_still_write() {
        for host in [
            "192.0.2.42:8200",
            "127.0.0.1:8200",
            "[::1]:8200",
            "localhost:8200",
            "diskstation:8200",
            "nas.local:8200",
            "nas.home.arpa:8200",
        ] {
            assert!(
                same_origin(&asking(Some(host), Some(&format!("http://{host}")))),
                "{host} is how this server is reached"
            );
            assert!(
                same_origin(&asking(Some(host), None)),
                "{host} with no Origin, which is curl on the NAS"
            );
        }
    }

    #[test]
    fn a_cross_site_page_and_a_request_naming_no_host_are_both_refused() {
        assert!(!same_origin(&asking(
            Some("192.0.2.42:8200"),
            Some("http://evil.example")
        )));
        assert!(
            !same_origin(&asking(Some("192.0.2.42:8200"), Some("null"))),
            "a sandboxed page says null"
        );
        assert!(!same_origin(&asking(None, None)));
    }

    #[test]
    fn the_addresses_a_home_network_hands_out_are_this_network_and_no_others_are() {
        for local in [
            "127.0.0.1",
            "10.0.0.7",
            "172.16.0.1",
            "172.31.255.254",
            "192.168.8.227",
            "169.254.10.1",
            "::1",
            "fd12:3456::1",
            "fe80::1",
            "::ffff:192.168.8.227",
        ] {
            let address: IpAddr = local.parse().expect("an address");
            assert!(on_this_network(address), "{local} is on this network");
        }
        for outside in [
            "8.8.8.8",
            "203.0.113.9",
            "172.32.0.1",
            "100.64.0.1",
            "0.0.0.0",
            "2001:db8::1",
            "2a01:cb00::1",
            "::",
            "::ffff:8.8.8.8",
        ] {
            let address: IpAddr = outside.parse().expect("an address");
            assert!(!on_this_network(address), "{outside} is not");
        }
    }

    #[test]
    fn a_browser_is_answered_json_and_a_shell_is_answered_lines() {
        let mut headers = HeaderMap::new();
        assert!(
            !wants_json(&headers),
            "a caller that said nothing wants text"
        );
        headers.insert(header::ACCEPT, HeaderValue::from_static("text/plain"));
        assert!(!wants_json(&headers));
        headers.insert(
            header::ACCEPT,
            HeaderValue::from_static("application/json, */*"),
        );
        assert!(wants_json(&headers));
    }
}
