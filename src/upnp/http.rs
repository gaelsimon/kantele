//! The HTTP surface: description, service definitions, SOAP control and the media itself.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{ConnectInfo, Path, Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{any, get, post};
use tokio_stream::StreamExt;
use tower::ServiceExt;
use tower_http::services::ServeFile;

use crate::upnp::description;
use crate::upnp::device::Device;
use crate::upnp::gena::{self, Service};
use crate::upnp::peers::Step;
use crate::upnp::{ObjectId, capture, contentdirectory, didl, search};

type Shared = Arc<Device>;

const XML: &str = "text/xml; charset=\"utf-8\"";

/// The trace layer and the fallback are the assembled server's.
pub fn router(device: Shared) -> Router {
    let control = Router::new()
        .route("/control/ContentDirectory", post(content_directory_control))
        .route(
            "/control/ConnectionManager",
            post(connection_manager_control),
        )
        .route_layer(axum::middleware::from_fn_with_state(
            device.clone(),
            capture_control,
        ));
    Router::new()
        .route("/description.xml", get(device_description))
        .route("/scpd/ContentDirectory.xml", get(content_directory_scpd))
        .route("/scpd/ConnectionManager.xml", get(connection_manager_scpd))
        .merge(control)
        .route("/event/ContentDirectory", any(content_directory_event))
        .route("/event/ConnectionManager", any(connection_manager_event))
        .route("/media/{id}", get(media))
        .route("/art/{id}", get(art))
        .route("/icon/{name}", get(icon))
        .route_layer(axum::middleware::from_fn_with_state(
            device.clone(),
            note_peer,
        ))
        .with_state(device)
}

fn peer_address(request: &Request) -> Option<std::net::IpAddr> {
    request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| address.ip())
}

pub(crate) fn peer_of(request: &Request) -> String {
    peer_address(request).map_or_else(|| "?".to_owned(), |address| address.to_string())
}

/// The step of discovery a request is, or nothing for the ones that say little about the device.
fn step_of(method: &Method, path: &str, headers: &HeaderMap) -> Option<Step> {
    match (method.as_str(), path) {
        ("GET", "/description.xml") => Some(Step::Described),
        ("POST", path) if path.starts_with("/control/") => Some(Step::Browsed),
        ("SUBSCRIBE", path) if path.starts_with("/event/") && !headers.contains_key("sid") => {
            Some(Step::Subscribed)
        }
        ("GET", path) if path.starts_with("/media/") => Some(Step::Played),
        _ => None,
    }
}

async fn note_peer(
    State(device): State<Shared>,
    request: Request,
    next: axum::middleware::Next,
) -> Response {
    if let Some(address) = peer_address(&request)
        && let Some(step) = step_of(request.method(), request.uri().path(), request.headers())
    {
        let agent = request
            .headers()
            .get(header::USER_AGENT)
            .and_then(|value| value.to_str().ok());
        device.peers.note(address, step, agent);
    }
    next.run(request).await
}

/// The same bound axum's own body extractors apply.
const CAPTURED_BODY_LIMIT: usize = 2 * 1024 * 1024;

async fn capture_control(
    State(device): State<Shared>,
    request: Request,
    next: axum::middleware::Next,
) -> Response {
    let Some(recorder) = device.capture() else {
        return next.run(request).await;
    };
    let peer = peer_of(&request);
    let (parts, body) = request.into_parts();
    let Ok(asked) = to_bytes(body, CAPTURED_BODY_LIMIT).await else {
        return StatusCode::PAYLOAD_TOO_LARGE.into_response();
    };
    let method = parts.method.to_string();
    let path = parts.uri.path().to_owned();
    let headers: Vec<(String, String)> = parts
        .headers
        .iter()
        .map(|(name, value)| {
            (
                name.to_string(),
                String::from_utf8_lossy(value.as_bytes()).into_owned(),
            )
        })
        .collect();
    let response = next
        .run(Request::from_parts(parts, Body::from(asked.clone())))
        .await;
    let (parts, body) = response.into_parts();
    let answered = to_bytes(body, usize::MAX).await.unwrap_or_default();
    let exchange = capture::Exchange {
        peer: &peer,
        method: &method,
        path: &path,
        headers,
        body: &asked,
        status: parts.status.as_u16(),
        response: &answered,
    };
    // A blocking write on the request path, accepted: capture is a diagnostic an owner turns on.
    if let Err(error) = recorder.record(&exchange) {
        tracing::warn!(%error, dir = %recorder.dir().display(), "exchange not captured");
    }
    Response::from_parts(parts, Body::from(answered))
}

pub(crate) fn trace_renderer(
    peer: &str,
    method: &Method,
    path: &str,
    status: u16,
    headers: &HeaderMap,
) {
    let read = |name: &str| {
        headers
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or("-")
    };
    tracing::info!(
        %peer, %method, %path, status,
        user_agent = read("user-agent"),
        av_client = read("x-av-client-info"),
        range = read("range"),
        time_seek = read("timeseekrange.dlna.org"),
        play_speed = read("playspeed.dlna.org"),
        wants_features = read("getcontentfeatures.dlna.org"),
        transfer_mode = read("transfermode.dlna.org"),
        "renderer"
    );
}

pub(crate) async fn unmatched(request: Request) -> Response {
    tracing::warn!(method = %request.method(), uri = %request.uri(), "no route for this request");
    StatusCode::NOT_FOUND.into_response()
}

fn xml(body: String) -> Response {
    ([(header::CONTENT_TYPE, XML)], body).into_response()
}

async fn device_description(State(device): State<Shared>) -> Response {
    xml(description::device(&device.identity))
}

async fn content_directory_scpd() -> Response {
    xml(description::CONTENT_DIRECTORY_SCPD.to_owned())
}

async fn connection_manager_scpd() -> Response {
    xml(description::CONNECTION_MANAGER_SCPD.to_owned())
}

fn base_url(headers: &HeaderMap) -> String {
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("127.0.0.1");
    format!("http://{host}")
}

fn soap_action(headers: &HeaderMap) -> Option<String> {
    let raw = headers.get("soapaction")?.to_str().ok()?;
    let trimmed = raw.trim().trim_matches('"');
    Some(trimmed.rsplit('#').next()?.to_owned())
}

async fn content_directory_control(
    State(device): State<Shared>,
    headers: HeaderMap,
    body: String,
) -> Response {
    let action = soap_action(&headers).unwrap_or_default();
    tracing::debug!(%action, body = %body.replace('\n', " "), "ContentDirectory request");

    match action.as_str() {
        "Browse" => match contentdirectory::parse_browse(&body) {
            Ok(request) => {
                let base = base_url(&headers);
                let to = didl::To::new(&base, device.clients.resolve(&headers));
                match contentdirectory::browse(
                    &device.served(),
                    &request,
                    to,
                    device.system_update_id(),
                ) {
                    Ok(response) => {
                        tracing::info!(
                            object = %request.object_id,
                            returned = response.number_returned,
                            total = response.total_matches,
                            "browse"
                        );
                        xml(contentdirectory::browse_envelope(&response))
                    }
                    Err(fault) => fault_response(&fault),
                }
            }
            Err(fault) => fault_response(&fault),
        },
        "Search" => match contentdirectory::parse_search(&body) {
            Ok(request) => {
                let base = base_url(&headers);
                let to = didl::To::new(&base, device.clients.resolve(&headers));
                match contentdirectory::search(
                    &device.served(),
                    &request,
                    to,
                    device.system_update_id(),
                ) {
                    Ok(response) => {
                        tracing::info!(
                            container = %request.container_id,
                            criteria = %request.criteria,
                            returned = response.number_returned,
                            total = response.total_matches,
                            "search"
                        );
                        xml(contentdirectory::search_envelope(&response))
                    }
                    Err(fault) => fault_response(&fault),
                }
            }
            Err(fault) => fault_response(&fault),
        },
        "GetSearchCapabilities" => xml(contentdirectory::simple_envelope(
            "GetSearchCapabilities",
            "ContentDirectory",
            "SearchCaps",
            &search::capabilities(),
        )),
        "GetSortCapabilities" => xml(contentdirectory::simple_envelope(
            "GetSortCapabilities",
            "ContentDirectory",
            "SortCaps",
            "",
        )),
        "GetSystemUpdateID" => xml(contentdirectory::simple_envelope(
            "GetSystemUpdateID",
            "ContentDirectory",
            "Id",
            &device.system_update_id().to_string(),
        )),
        other => {
            tracing::warn!(action = %other, "unimplemented ContentDirectory action");
            fault_response(&contentdirectory::Fault {
                code: 401,
                description: "Invalid Action",
            })
        }
    }
}

/// The one connection a server with no `PrepareForConnection` holds.
const DEFAULT_CONNECTION: i32 = 0;

async fn connection_manager_control(headers: HeaderMap, body: String) -> Response {
    match soap_action(&headers).unwrap_or_default().as_str() {
        "GetProtocolInfo" => {
            let source = crate::upnp::PROTOCOL_INFO.join(",");
            xml(format!(
                r#"<?xml version="1.0" encoding="utf-8"?>
<s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/" s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/"><s:Body><u:GetProtocolInfoResponse xmlns:u="urn:schemas-upnp-org:service:ConnectionManager:1"><Source>{source}</Source><Sink></Sink></u:GetProtocolInfoResponse></s:Body></s:Envelope>"#
            ))
        }
        "GetCurrentConnectionIDs" => xml(contentdirectory::simple_envelope(
            "GetCurrentConnectionIDs",
            "ConnectionManager",
            "ConnectionIDs",
            &DEFAULT_CONNECTION.to_string(),
        )),
        "GetCurrentConnectionInfo" => match current_connection_info(&body) {
            Ok(response) => xml(response),
            Err(fault) => fault_response(&fault),
        },
        other => {
            tracing::warn!(action = %other, "unimplemented ConnectionManager action");
            fault_response(&contentdirectory::Fault {
                code: 401,
                description: "Invalid Action",
            })
        }
    }
}

/// `-1` says there is no rendering control, no transport and no peer.
fn current_connection_info(body: &str) -> Result<String, contentdirectory::Fault> {
    let asked = contentdirectory::argument(body, "ConnectionID")
        .and_then(|value| value.trim().parse::<i32>().ok())
        .ok_or(contentdirectory::Fault::INVALID_ARGS)?;
    if asked != DEFAULT_CONNECTION {
        return Err(contentdirectory::Fault::NO_SUCH_CONNECTION);
    }
    Ok(contentdirectory::arguments_envelope(
        "GetCurrentConnectionInfo",
        "ConnectionManager",
        &[
            ("RcsID", "-1".to_owned()),
            ("AVTransportID", "-1".to_owned()),
            ("ProtocolInfo", crate::upnp::PROTOCOL_INFO.join(",")),
            ("PeerConnectionManager", String::new()),
            ("PeerConnectionID", "-1".to_owned()),
            ("Direction", "Output".to_owned()),
            ("Status", "OK".to_owned()),
        ],
    ))
}

fn connection_manager_state() -> Vec<(&'static str, String)> {
    vec![
        ("SourceProtocolInfo", crate::upnp::PROTOCOL_INFO.join(",")),
        ("SinkProtocolInfo", String::new()),
        ("CurrentConnectionIDs", "0".to_owned()),
    ]
}

async fn content_directory_event(State(device): State<Shared>, request: Request) -> Response {
    let state = device.content_directory_state();
    event(device, Service::ContentDirectory, request, state).await
}

async fn connection_manager_event(State(device): State<Shared>, request: Request) -> Response {
    event(
        device,
        Service::ConnectionManager,
        request,
        connection_manager_state(),
    )
    .await
}

async fn event(
    device: Shared,
    service: Service,
    request: Request,
    state: Vec<(&'static str, String)>,
) -> Response {
    let method = request.method().as_str().to_owned();
    let peer = request
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|ConnectInfo(address)| address.ip());
    let headers = request.headers();
    let sid = headers
        .get("sid")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let callback = headers
        .get("callback")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let timeout = headers
        .get("timeout")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);
    let user_agent = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    match method.as_str() {
        "SUBSCRIBE" => match (sid, callback) {
            (Some(sid), None) => match device.subscriptions.renew(&sid, timeout.as_deref()) {
                Some(granted) => granted_response(&granted),
                None => {
                    tracing::warn!(%sid, "renewal for an unknown subscription");
                    (StatusCode::PRECONDITION_FAILED, "unknown SID").into_response()
                }
            },
            (None, Some(callback)) => match device.subscriptions.subscribe(
                service,
                &callback,
                peer,
                timeout.as_deref(),
                user_agent,
            ) {
                Ok(granted) => {
                    tracing::info!(
                        sid = %granted.sid, %callback, ?service,
                        "subscribed"
                    );
                    let subscriptions = device.subscriptions.clone();
                    let sid = granted.sid.clone();
                    tokio::spawn(async move {
                        subscriptions.send_initial(&sid, &state).await;
                    });
                    granted_response(&granted)
                }
                Err(refused) => {
                    tracing::warn!(%callback, ?peer, %refused, "subscription refused");
                    (StatusCode::PRECONDITION_FAILED, refused.to_string()).into_response()
                }
            },
            _ => (StatusCode::BAD_REQUEST, "SUBSCRIBE needs CALLBACK or SID").into_response(),
        },
        "UNSUBSCRIBE" => match sid {
            Some(sid) if device.subscriptions.unsubscribe(&sid) => StatusCode::OK.into_response(),
            _ => (StatusCode::PRECONDITION_FAILED, "unknown SID").into_response(),
        },
        other => {
            tracing::warn!(%other, "unexpected method on an event URL");
            StatusCode::METHOD_NOT_ALLOWED.into_response()
        }
    }
}

fn granted_response(granted: &gena::Granted) -> Response {
    (
        StatusCode::OK,
        [
            ("SID", granted.sid.clone()),
            ("TIMEOUT", gena::timeout_header(granted.timeout)),
        ],
    )
        .into_response()
}

async fn icon(State(device): State<Shared>, Path(name): Path<String>) -> Response {
    match device.identity.icon.served(&name) {
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

fn paced(body: Body, bytes_per_second: u32) -> Body {
    let (sender, receiver) = tokio::sync::mpsc::channel(2);
    tokio::spawn(async move {
        let mut stream = body.into_data_stream();
        let start = std::time::Instant::now();
        let mut sent: u64 = 0;
        while let Some(chunk) = stream.next().await {
            let Ok(bytes) = chunk else { break };
            let due = std::time::Duration::from_secs_f64(sent as f64 / f64::from(bytes_per_second));
            if let Some(wait) = due.checked_sub(start.elapsed()) {
                tokio::time::sleep(wait).await;
            }
            sent += bytes.len() as u64;
            if sender.send(Ok::<_, axum::Error>(bytes)).await.is_err() {
                break;
            }
        }
    });
    Body::from_stream(tokio_stream::wrappers::ReceiverStream::new(receiver))
}

/// The short form the reference answers with, not the long one in its own DIDL; the parity gate pins it.
pub const CONTENT_FEATURES: &str = "DLNA.ORG_OP=01";

/// Several ranges at once would need a multipart answer, which `ServeFile` has none of and
/// refuses with a 416. RFC 7233 lets a server ignore the header instead, and the whole file is
/// something every renderer can play.
fn several_ranges(headers: &HeaderMap) -> bool {
    headers
        .get(header::RANGE)
        .is_some_and(|value| value.as_bytes().contains(&b','))
}

async fn media(
    State(device): State<Shared>,
    Path(id): Path<String>,
    mut request: Request,
) -> Response {
    let Ok(object_id) = ObjectId::new(id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let serving = device.served();
    let library = &serving.library;
    let Some(track) = library.get(&object_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };

    // CONTENT_FEATURES offers byte seeking and nothing else, so a request to scrub by time or to
    // play at another speed is refused. Answered with the file, a renderer plays from the start
    // while showing the position it asked for.
    if request.headers().contains_key("timeseekrange.dlna.org") {
        return StatusCode::NOT_ACCEPTABLE.into_response();
    }
    if let Some(speed) = request.headers().get("playspeed.dlna.org")
        && speed.as_bytes() != b"1"
    {
        return StatusCode::NOT_ACCEPTABLE.into_response();
    }

    // DLNA 7.5.4.3.2.35.1: the only legal value is 1.
    let asked_features = request.headers().get("getcontentfeatures.dlna.org");
    let wants_features = match asked_features.map(|value| value.as_bytes()) {
        None => false,
        Some(b"1") => true,
        Some(_) => return StatusCode::BAD_REQUEST.into_response(),
    };
    let transfer_mode = request
        .headers()
        .get("transfermode.dlna.org")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);

    if several_ranges(request.headers()) {
        request.headers_mut().remove(header::RANGE);
    }

    let client = device.clients.resolve(request.headers());
    let mime = client.mime_for(track.mime).to_owned();
    let cap = client
        .rate_cap
        .and_then(|multiple| didl::capped(track, multiple));

    match ServeFile::new_with_mime(
        &track.path,
        &mime.parse().unwrap_or(mime::APPLICATION_OCTET_STREAM),
    )
    .oneshot(request)
    .await
    {
        Ok(response) => {
            let mut response = response.map(Body::new);
            let headers = response.headers_mut();
            if let (true, Ok(value)) = (wants_features, CONTENT_FEATURES.parse()) {
                headers.insert("contentFeatures.dlna.org", value);
            }
            if let Some(mode) = transfer_mode.and_then(|mode| mode.parse().ok()) {
                headers.insert("transferMode.dlna.org", mode);
            }
            match cap {
                Some(bytes_per_second) => response.map(|body| paced(body, bytes_per_second)),
                None => response,
            }
        }
        Err(error) => {
            tracing::error!(path = %track.path.display(), %error, "serving media");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

async fn art(State(device): State<Shared>, Path(id): Path<String>) -> Response {
    let Ok(object_id) = ObjectId::new(id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let served = device.served();
    let Some(artwork) = served.library.artwork(&object_id) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let (source, mime) = (artwork.source.clone(), artwork.mime);
    drop(served);

    let read = tokio::task::spawn_blocking(move || crate::index::artwork::read(&source)).await;
    match read {
        Ok(Ok(bytes)) => (
            [
                (header::CONTENT_TYPE, mime),
                (header::CACHE_CONTROL, "public, max-age=86400"),
            ],
            bytes,
        )
            .into_response(),
        Ok(Err(error)) => {
            tracing::error!(%error, "reading artwork");
            StatusCode::NOT_FOUND.into_response()
        }
        Err(error) => {
            tracing::error!(%error, "the artwork read did not finish");
            StatusCode::INTERNAL_SERVER_ERROR.into_response()
        }
    }
}

fn fault_response(fault: &contentdirectory::Fault) -> Response {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        [(header::CONTENT_TYPE, XML)],
        contentdirectory::fault_envelope(fault),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers_with(name: &'static str, value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(name, value.parse().unwrap());
        headers
    }

    #[test]
    fn the_soap_action_is_read_past_its_service_prefix_and_quotes() {
        let headers = headers_with(
            "soapaction",
            r#""urn:schemas-upnp-org:service:ContentDirectory:1#Browse""#,
        );
        assert_eq!(soap_action(&headers).as_deref(), Some("Browse"));
    }

    #[test]
    fn a_missing_soap_action_is_not_a_panic() {
        assert_eq!(soap_action(&HeaderMap::new()), None);
    }

    #[tokio::test]
    async fn a_capped_stream_arrives_whole_and_no_faster_than_the_cap() {
        let chunks: Vec<Result<Vec<u8>, std::io::Error>> =
            (0..8).map(|_| Ok(vec![b'x'; 500])).collect();
        let start = std::time::Instant::now();
        let held = paced(Body::from_stream(tokio_stream::iter(chunks)), 20_000);
        let read = axum::body::to_bytes(held, usize::MAX)
            .await
            .expect("the stream completes");
        assert_eq!(read.len(), 4_000, "a cap delays bytes and drops none");
        assert!(
            start.elapsed() >= std::time::Duration::from_millis(150),
            "3.5 kB behind the first chunk cannot arrive sooner than that at 20 kB/s"
        );
    }

    #[test]
    fn each_step_of_discovery_is_read_off_the_request_and_a_renewal_is_not_one() {
        let subscribe = Method::from_bytes(b"SUBSCRIBE").expect("a method");
        let none = HeaderMap::new();
        assert_eq!(
            step_of(&Method::GET, "/description.xml", &none),
            Some(Step::Described)
        );
        assert_eq!(
            step_of(&Method::POST, "/control/ContentDirectory", &none),
            Some(Step::Browsed)
        );
        assert_eq!(
            step_of(&subscribe, "/event/ContentDirectory", &none),
            Some(Step::Subscribed)
        );
        assert_eq!(
            step_of(
                &subscribe,
                "/event/ContentDirectory",
                &headers_with("sid", "uuid:one")
            ),
            None,
            "a renewal says nothing new about the device"
        );
        assert_eq!(
            step_of(&Method::GET, "/media/tr-1", &none),
            Some(Step::Played)
        );
        assert_eq!(step_of(&Method::GET, "/art/tr-1", &none), None);
        assert_eq!(step_of(&Method::GET, "/icon/kantele.png", &none), None);
    }

    #[test]
    fn a_range_naming_one_span_is_served_and_one_naming_several_is_not_refused() {
        assert!(!several_ranges(&HeaderMap::new()));
        assert!(!several_ranges(&headers_with("range", "bytes=0-9")));
        assert!(!several_ranges(&headers_with("range", "bytes=-10")));
        assert!(
            several_ranges(&headers_with("range", "bytes=0-9,20-29")),
            "a 416 here leaves a renderer with no audio at all"
        );
        assert!(several_ranges(&headers_with(
            "range",
            "bytes=0-9, 20-29, 40-49"
        )));
    }

    #[test]
    fn resource_urls_follow_the_host_the_client_used() {
        let headers = headers_with("host", "192.0.2.42:8200");
        assert_eq!(base_url(&headers), "http://192.0.2.42:8200");
    }
}
