use std::net::SocketAddr;

use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{Method, Request, StatusCode, header};
use axum::response::Response;
use kantele::index::{Library, ScanOptions};
use kantele::server::{self, Server};
use kantele::upnp::client::Profiles;
use kantele::upnp::description::DeviceIdentity;
use kantele::upnp::{http, search};
use tower::ServiceExt;

mod fixtures;
use fixtures::Tree;

/// A server over a real folder, because media and artwork are served from disk.
fn serving(tree: &Tree) -> Server {
    let library = Library::scan_with(&tree.0, &ScanOptions::default()).expect("scanning");
    Server::new(
        library,
        kantele::browse::Settings::default(),
        DeviceIdentity {
            friendly_name: "Kantele Test".to_owned(),
            udn: "3d5d1cbe-8f2a-4d1e-9a9c-7c2f0a1b2c3d".to_owned(),
            icon: Default::default(),
        },
        Profiles::default(),
    )
}

async fn ask(server: &Server, request: Request<Body>) -> Response {
    server::router(server)
        .oneshot(request)
        .await
        .expect("the router answers")
}

async fn body_of(response: Response) -> String {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    String::from_utf8_lossy(&bytes).into_owned()
}

fn get(path: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        .body(Body::empty())
        .expect("a request")
}

/// A SOAP request shaped the way a control point sends one.
fn soap(action: &str, arguments: &str) -> Request<Body> {
    soap_to(service_of(action), action, arguments)
}

/// Which of the two services declares an action, so a caller names the action alone.
fn service_of(action: &str) -> &'static str {
    match action {
        "GetProtocolInfo" | "GetCurrentConnectionIDs" | "GetCurrentConnectionInfo" => {
            "ConnectionManager"
        }
        _ => "ContentDirectory",
    }
}

fn soap_to(service: &str, action: &str, arguments: &str) -> Request<Body> {
    let body = format!(
        r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
<s:Body><u:{action} xmlns:u="urn:schemas-upnp-org:service:{service}:1">{arguments}</u:{action}>
</s:Body></s:Envelope>"#
    );
    Request::builder()
        .method(Method::POST)
        .uri(format!("/control/{service}"))
        .header(
            "SOAPAction",
            format!("\"urn:schemas-upnp-org:service:{service}:1#{action}\""),
        )
        .header(header::HOST, "192.0.2.1:8200")
        .body(Body::from(body))
        .expect("a request")
}

fn browse(object: &str, flag: &str, start: usize, count: usize) -> Request<Body> {
    soap(
        "Browse",
        &format!(
            "<ObjectID>{object}</ObjectID><BrowseFlag>{flag}</BrowseFlag><Filter>*</Filter>\
             <StartingIndex>{start}</StartingIndex><RequestedCount>{count}</RequestedCount>\
             <SortCriteria></SortCriteria>"
        ),
    )
}

/// The identifier of the first item the flat container holds.
async fn first_track(server: &Server) -> String {
    let answer = body_of(ask(server, browse("music", "BrowseDirectChildren", 0, 1)).await).await;
    let at = answer.find("&lt;item id=&quot;").expect("an item") + 18;
    let rest = &answer[at..];
    rest[..rest.find("&quot;").expect("a closing quote")].to_owned()
}

fn library_tree(name: &str) -> Tree {
    let tree = Tree::new(name);
    tree.album("Sierra Maestra", &["01.wav", "02.wav"], true);
    tree
}

/// One `NOTIFY` read off a socket: the request line, its headers, and its body.
async fn next_notify(listener: &tokio::net::TcpListener) -> String {
    use tokio::io::AsyncReadExt;
    let accepted = tokio::time::timeout(std::time::Duration::from_secs(5), listener.accept()).await;
    let (mut stream, _) = accepted
        .expect("a subscriber is entitled to its first event")
        .expect("the connection");
    let mut read = Vec::new();
    // The server closes after writing, so a read to the end is the whole request.
    tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stream.read_to_end(&mut read),
    )
    .await
    .expect("the event arrives")
    .expect("reading it");
    String::from_utf8_lossy(&read).into_owned()
}

/// The bytes of an event.
#[tokio::test]
async fn a_subscriber_is_sent_the_state_it_subscribed_to_and_then_every_change() {
    let tree = library_tree("wire-event-bytes");
    let server = serving(&tree);

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("a callback to deliver to");
    let callback = format!("<http://{}/notify>", listener.local_addr().expect("bound"));

    let subscribed = ask(
        &server,
        Request::builder()
            .method("SUBSCRIBE")
            .uri("/event/ContentDirectory")
            .extension(ConnectInfo(SocketAddr::from(([127, 0, 0, 1], 1))))
            .header("CALLBACK", &callback)
            .header("NT", "upnp:event")
            .header("TIMEOUT", "Second-1800")
            .body(Body::empty())
            .expect("a request"),
    )
    .await;
    assert_eq!(subscribed.status(), StatusCode::OK);
    let sid = subscribed
        .headers()
        .get("SID")
        .and_then(|value| value.to_str().ok())
        .expect("a subscription identifier")
        .to_owned();
    assert!(sid.starts_with("uuid:"), "{sid}");

    let first = next_notify(&listener).await;
    for expected in [
        "NOTIFY /notify HTTP/1.1\r\n",
        "NT: upnp:event\r\n",
        "NTS: upnp:propchange\r\n",
        "SEQ: 0\r\n",
        "CONTENT-TYPE: text/xml; charset=\"utf-8\"\r\n",
    ] {
        assert!(
            first.contains(expected),
            "missing {expected:?} in:\n{first}"
        );
    }
    assert!(
        first.contains(&format!("SID: {sid}\r\n")),
        "an event names the subscription it belongs to:\n{first}"
    );
    assert!(
        first.contains("<e:propertyset"),
        "and carries a property set:\n{first}"
    );
    assert!(
        first.contains("<SystemUpdateID>1</SystemUpdateID>"),
        "which is what a client compares its cache against:\n{first}"
    );

    let (headers, body) = first.split_once("\r\n\r\n").expect("a blank line");
    let declared: usize = headers
        .lines()
        .find_map(|line| line.strip_prefix("CONTENT-LENGTH: "))
        .and_then(|value| value.trim().parse().ok())
        .expect("a content length");
    assert_eq!(
        declared,
        body.len(),
        "a body shorter than its declared length is a client waiting for the rest"
    );

    // A pass that publishes is what a subscriber is told about, on the same subscription.
    let published = kantele::browse::Served::new(
        Library::scan_with(&tree.0, &ScanOptions::default()).expect("scanning"),
        kantele::browse::Settings::default(),
    );
    let id = server.device.publish(published).await;
    assert_eq!(id, 2);

    let second = next_notify(&listener).await;
    assert!(second.contains("SEQ: 1\r\n"), "{second}");
    assert!(
        second.contains("<SystemUpdateID>2</SystemUpdateID>"),
        "the identifier moved, which is the whole message:\n{second}"
    );
}

/// Every action the two service definitions declare, taken from the definitions themselves.
fn declared_actions(scpd: &str) -> Vec<String> {
    scpd.split("<action>")
        .skip(1)
        .filter_map(|action| action.split_once("<name>"))
        .filter_map(|(_, rest)| rest.split_once("</name>"))
        .map(|(name, _)| name.trim().to_owned())
        .collect()
}

/// A service definition is a promise. Publishing an action the control endpoint faults.
#[tokio::test]
async fn every_action_the_definitions_declare_is_answered() {
    let tree = library_tree("wire-declared-actions");
    let server = serving(&tree);

    let declared = [
        (
            "ContentDirectory",
            kantele::upnp::description::CONTENT_DIRECTORY_SCPD,
        ),
        (
            "ConnectionManager",
            kantele::upnp::description::CONNECTION_MANAGER_SCPD,
        ),
    ];
    let mut asked = 0;
    for (service, scpd) in declared {
        let actions = declared_actions(scpd);
        assert!(!actions.is_empty(), "{service} declares nothing");
        for action in actions {
            // Every in-argument of every declared action, so nothing faults for want of one.
            let arguments = match action.as_str() {
                "Browse" => {
                    "<ObjectID>0</ObjectID><BrowseFlag>BrowseMetadata</BrowseFlag>                             <Filter>*</Filter><StartingIndex>0</StartingIndex>                             <RequestedCount>1</RequestedCount><SortCriteria></SortCriteria>"
                }
                "Search" => {
                    "<ContainerID>0</ContainerID><SearchCriteria>*</SearchCriteria>                             <Filter>*</Filter><StartingIndex>0</StartingIndex>                             <RequestedCount>1</RequestedCount><SortCriteria></SortCriteria>"
                }
                "GetCurrentConnectionInfo" => "<ConnectionID>0</ConnectionID>",
                _ => "",
            };
            let answer = body_of(ask(&server, soap_to(service, &action, arguments)).await).await;
            assert!(
                !answer.contains("<errorCode>401</errorCode>"),
                "{service} declares {action} and the control endpoint refuses it: {answer}"
            );
            assert!(
                answer.contains(&format!("{action}Response")),
                "{service}'s {action} answered no response document: {answer}"
            );
            asked += 1;
        }
    }
    assert!(asked >= 8, "both definitions were swept, not one: {asked}");
}

#[tokio::test]
async fn a_connection_nobody_opened_is_refused_rather_than_described() {
    let tree = library_tree("wire-unknown-connection");
    let server = serving(&tree);

    let known = body_of(
        ask(
            &server,
            soap("GetCurrentConnectionInfo", "<ConnectionID>0</ConnectionID>"),
        )
        .await,
    )
    .await;
    assert!(known.contains("<Direction>Output</Direction>"), "{known}");
    assert!(known.contains("<Status>OK</Status>"), "{known}");

    let unknown = ask(
        &server,
        soap("GetCurrentConnectionInfo", "<ConnectionID>7</ConnectionID>"),
    )
    .await;
    assert_eq!(unknown.status(), StatusCode::INTERNAL_SERVER_ERROR);
    assert!(
        body_of(unknown)
            .await
            .contains("<errorCode>706</errorCode>"),
        "a connection this server never opened"
    );
}

#[tokio::test]
async fn the_description_and_the_service_definitions_are_served() {
    let tree = library_tree("wire-description");
    let server = serving(&tree);

    for path in [
        "/description.xml",
        "/scpd/ContentDirectory.xml",
        "/scpd/ConnectionManager.xml",
    ] {
        let response = ask(&server, get(path)).await;
        assert_eq!(response.status(), StatusCode::OK, "{path}");
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("text/xml; charset=\"utf-8\""),
            "{path} must be served as XML"
        );
        let body = body_of(response).await;
        assert!(body.starts_with("<?xml"), "{path}: {body}");
    }

    let description = body_of(ask(&server, get("/description.xml")).await).await;
    assert!(
        description.contains("<UDN>uuid:3d5d1cbe-8f2a-4d1e-9a9c-7c2f0a1b2c3d</UDN>"),
        "the description carries the persisted identity: {description}"
    );
}

#[tokio::test]
async fn a_browse_of_the_root_answers_containers_over_soap() {
    let tree = library_tree("wire-browse");
    let server = serving(&tree);
    let response = ask(&server, browse("0", "BrowseDirectChildren", 0, 0)).await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_of(response).await;
    assert!(body.contains("<u:BrowseResponse"), "{body}");
    assert!(body.contains("&lt;container id="), "{body}");
    assert!(
        !body.contains("&lt;item id="),
        "a root of bare items is listed and then refused: {body}"
    );
}

#[tokio::test]
async fn every_control_answer_carries_ext() {
    let tree = library_tree("wire-ext");
    let server = serving(&tree);
    let answers = [
        browse("0", "BrowseDirectChildren", 0, 10),
        soap("GetSystemUpdateID", ""),
        soap("GetProtocolInfo", ""),
        // A fault is an answer to a control request too.
        soap("CreateObject", ""),
    ];
    for request in answers {
        let uri = request.uri().to_string();
        let response = ask(&server, request).await;
        assert_eq!(
            response.headers().get("ext").map(|value| value.as_bytes()),
            Some(b"".as_slice()),
            "{uri}: the one header UDA 1.0 makes mandatory on a control answer"
        );
    }
}

#[tokio::test]
async fn an_unknown_object_is_a_fault_with_a_code_a_client_can_read() {
    let tree = library_tree("wire-fault");
    let server = serving(&tree);
    let response = ask(&server, browse("no-such-object", "BrowseMetadata", 0, 0)).await;
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let body = body_of(response).await;
    assert!(body.contains("<errorCode>701</errorCode>"), "{body}");
}

#[tokio::test]
async fn an_action_this_server_does_not_implement_faults_with_401() {
    let tree = library_tree("wire-action");
    let server = serving(&tree);
    for action in ["CreateObject", "DestroyObject"] {
        let body = body_of(ask(&server, soap(action, "")).await).await;
        assert!(
            body.contains("<errorCode>401</errorCode>"),
            "{action}: {body}"
        );
    }
}

#[tokio::test]
async fn the_capability_answers_are_the_evaluated_set_and_an_honest_empty_sort() {
    let tree = library_tree("wire-capabilities");
    let server = serving(&tree);

    let caps = body_of(ask(&server, soap("GetSearchCapabilities", "")).await).await;
    assert!(
        caps.contains(&search::capabilities().replace('"', "&quot;")),
        "an empty search capability is what makes a client say no results: {caps}"
    );

    let sort = body_of(ask(&server, soap("GetSortCapabilities", "")).await).await;
    assert!(sort.contains("<SortCaps></SortCaps>"), "{sort}");

    let update = body_of(ask(&server, soap("GetSystemUpdateID", "")).await).await;
    assert!(
        update.contains("<Id>1</Id>"),
        "one rather than zero, so a client can tell a fresh server from one that never indexed: \
         {update}"
    );
}

#[tokio::test]
async fn the_connection_manager_lists_what_this_server_can_emit_and_nothing_it_accepts() {
    let tree = library_tree("wire-connection-manager");
    let server = serving(&tree);
    let body = body_of(ask(&server, soap("GetProtocolInfo", "")).await).await;
    assert!(body.contains("http-get:*:audio/x-flac:*"), "{body}");
    assert!(
        body.contains("<Sink></Sink>"),
        "it is not a renderer: {body}"
    );
}

#[tokio::test]
async fn media_is_served_with_the_headers_a_renderer_asks_for() {
    let tree = library_tree("wire-media");
    let server = serving(&tree);
    let id = first_track(&server).await;

    let plain = ask(&server, get(&format!("/media/{id}"))).await;
    assert_eq!(plain.status(), StatusCode::OK);
    assert_eq!(
        plain
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("audio/x-wav")
    );
    assert!(
        plain.headers().get("contentFeatures.dlna.org").is_none(),
        "the header is answered when it is asked for, not always"
    );
    assert_eq!(body_of(plain).await.len(), fixtures::wav(1).len());

    let asked = ask(
        &server,
        Request::builder()
            .uri(format!("/media/{id}"))
            .header("getcontentfeatures.dlna.org", "1")
            .header("transferMode.dlna.org", "Streaming")
            .body(Body::empty())
            .expect("a request"),
    )
    .await;
    assert_eq!(
        asked
            .headers()
            .get("contentFeatures.dlna.org")
            .and_then(|value| value.to_str().ok()),
        Some(http::CONTENT_FEATURES)
    );
    assert_eq!(
        asked
            .headers()
            .get("transferMode.dlna.org")
            .and_then(|value| value.to_str().ok()),
        Some("Streaming"),
        "echoed rather than invented"
    );
}

#[tokio::test]
async fn a_seek_this_server_cannot_do_is_refused_rather_than_answered_with_the_file() {
    let tree = library_tree("wire-timeseek");
    let server = serving(&tree);
    let id = first_track(&server).await;
    let asking = |header: &'static str, value: &'static str| {
        Request::builder()
            .uri(format!("/media/{id}"))
            .header(header, value)
            .body(Body::empty())
            .expect("a request")
    };

    let by_time = ask(&server, asking("TimeSeekRange.dlna.org", "npt=10.000-")).await;
    assert_eq!(
        by_time.status(),
        StatusCode::NOT_ACCEPTABLE,
        "the features offer byte seeking only"
    );

    let fast = ask(&server, asking("PlaySpeed.dlna.org", "2")).await;
    assert_eq!(fast.status(), StatusCode::NOT_ACCEPTABLE);

    let normal = ask(&server, asking("PlaySpeed.dlna.org", "1")).await;
    assert_eq!(
        normal.status(),
        StatusCode::OK,
        "playing at the rate the file was written is what every renderer does"
    );
}

#[tokio::test]
async fn a_renderer_that_seeks_gets_a_range_and_a_content_range() {
    let tree = library_tree("wire-range");
    let server = serving(&tree);
    let id = first_track(&server).await;
    let response = ask(
        &server,
        Request::builder()
            .uri(format!("/media/{id}"))
            .header(header::RANGE, "bytes=100-199")
            .body(Body::empty())
            .expect("a request"),
    )
    .await;
    assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
    let total = fixtures::wav(1).len();
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_RANGE)
            .and_then(|value| value.to_str().ok()),
        Some(format!("bytes 100-199/{total}").as_str())
    );
    assert_eq!(body_of(response).await.len(), 100);
}

#[tokio::test]
async fn artwork_is_served_from_the_same_port_as_the_audio() {
    let tree = library_tree("wire-artwork");
    let server = serving(&tree);
    let id = first_track(&server).await;

    let response = ask(&server, get(&format!("/art/{id}"))).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("image/jpeg"),
        "the type comes from the bytes, so a renderer is never handed a lie"
    );
    assert!(
        response.headers().contains_key(header::CACHE_CONTROL),
        "a control point asks for the same cover once per item in a listing"
    );
}

#[tokio::test]
async fn a_cover_a_renderer_already_holds_is_not_read_again() {
    let tree = library_tree("wire-artwork-fresh");
    let server = serving(&tree);
    let id = first_track(&server).await;

    let first = ask(&server, get(&format!("/art/{id}"))).await;
    let tag = first
        .headers()
        .get(header::ETAG)
        .and_then(|value| value.to_str().ok())
        .expect("a validator, or a renderer has nothing to ask with")
        .to_owned();
    let bytes = body_of(first).await.len();
    assert!(bytes > 0);

    let again = Request::builder()
        .uri(format!("/art/{id}"))
        .header(header::IF_NONE_MATCH, &tag)
        .body(Body::empty())
        .expect("a request");
    let answered = ask(&server, again).await;
    assert_eq!(answered.status(), StatusCode::NOT_MODIFIED);
    assert_eq!(
        body_of(answered).await.len(),
        0,
        "and the bytes are not read at all: a picture held in a music file costs a parse of \
         every tag in it"
    );

    // The same cover, changed on disk: the validator moves and the bytes come back.
    let cover = tree.path("Sierra Maestra/cover.jpg");
    let mut grown = std::fs::read(&cover).expect("the cover");
    grown.extend_from_slice(&[0; 16]);
    std::fs::write(&cover, &grown).expect("writing it again");
    let moved = serving(&tree);
    let after = ask(&moved, get(&format!("/art/{}", first_track(&moved).await))).await;
    assert_eq!(after.status(), StatusCode::OK);
    assert_ne!(
        after
            .headers()
            .get(header::ETAG)
            .and_then(|value| value.to_str().ok()),
        Some(tag.as_str()),
        "a cover replaced on disk is a different cover"
    );
}

#[tokio::test]
async fn nothing_that_names_no_object_is_answered_with_bytes() {
    let tree = library_tree("wire-missing");
    let server = serving(&tree);
    for path in [
        "/media/tr-0000000000000000",
        "/art/tr-0000000000000000",
        "/media/not%20an%20id",
        "/nothing-here",
    ] {
        let response = ask(&server, get(path)).await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND, "{path}");
    }
}

/// An eventing request from a device at `192.0.2.9`, which is the host its callbacks may name.
fn event(method: &str, headers: &[(&str, &str)]) -> Request<Body> {
    let mut request = Request::builder()
        .method(method)
        .uri("/event/ContentDirectory")
        .extension(ConnectInfo(SocketAddr::from(([192, 0, 2, 9], 49153))));
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    request.body(Body::empty()).expect("a request")
}

#[tokio::test]
async fn a_callback_naming_another_host_than_the_subscriber_is_refused() {
    let tree = library_tree("wire-gena-elsewhere");
    let server = serving(&tree);
    let refused = ask(
        &server,
        event(
            "SUBSCRIBE",
            &[
                ("CALLBACK", "<http://192.0.2.44:49153/notify>"),
                ("NT", "upnp:event"),
            ],
        ),
    )
    .await;
    assert_eq!(refused.status(), StatusCode::PRECONDITION_FAILED);
    assert!(
        server.device.subscriptions.is_empty(),
        "and nothing is held for a device that would never hear from it"
    );
}

#[tokio::test]
async fn a_device_that_will_not_browse_until_it_can_subscribe_gets_a_subscription() {
    let tree = library_tree("wire-gena");
    let server = serving(&tree);

    let granted = ask(
        &server,
        event(
            "SUBSCRIBE",
            &[
                ("CALLBACK", "<http://192.0.2.9:49153/notify>"),
                ("NT", "upnp:event"),
                ("TIMEOUT", "Second-300"),
            ],
        ),
    )
    .await;
    assert_eq!(granted.status(), StatusCode::OK);
    let sid = granted
        .headers()
        .get("SID")
        .and_then(|value| value.to_str().ok())
        .expect("a subscription identifier")
        .to_owned();
    assert!(sid.starts_with("uuid:"), "{sid}");
    assert_eq!(
        granted
            .headers()
            .get("TIMEOUT")
            .and_then(|value| value.to_str().ok()),
        Some("Second-300"),
        "the granted timeout is what the client asked for, inside the bounds"
    );
    assert_eq!(server.device.subscriptions.len(), 1);

    let renewed = ask(&server, event("SUBSCRIBE", &[("SID", &sid)])).await;
    assert_eq!(renewed.status(), StatusCode::OK);
    assert_eq!(
        renewed
            .headers()
            .get("TIMEOUT")
            .and_then(|value| value.to_str().ok()),
        Some("Second-1800"),
        "a renewal that asks for nothing gets the default"
    );

    let dropped = ask(&server, event("UNSUBSCRIBE", &[("SID", &sid)])).await;
    assert_eq!(dropped.status(), StatusCode::OK);
    assert!(server.device.subscriptions.is_empty());
}

#[tokio::test]
async fn a_subscription_nobody_recognises_is_refused_rather_than_granted() {
    let tree = library_tree("wire-gena-refused");
    let server = serving(&tree);

    let unknown = ask(&server, event("SUBSCRIBE", &[("SID", "uuid:nobody")])).await;
    assert_eq!(unknown.status(), StatusCode::PRECONDITION_FAILED);

    let gone = ask(&server, event("UNSUBSCRIBE", &[("SID", "uuid:nobody")])).await;
    assert_eq!(gone.status(), StatusCode::PRECONDITION_FAILED);

    let neither = ask(&server, event("SUBSCRIBE", &[("NT", "upnp:event")])).await;
    assert_eq!(neither.status(), StatusCode::BAD_REQUEST);

    let wrong_method = ask(&server, event("GET", &[])).await;
    assert_eq!(wrong_method.status(), StatusCode::METHOD_NOT_ALLOWED);
}

#[tokio::test]
async fn a_search_over_soap_answers_the_query_an_amplifier_menu_sends() {
    let tree = library_tree("wire-search");
    let server = serving(&tree);
    let response = ask(
        &server,
        soap(
            "Search",
            "<ContainerID>0</ContainerID><SearchCriteria>upnp:class derivedfrom \
             &quot;object.item.audioItem&quot; and @refID exists false</SearchCriteria>\
             <Filter>*</Filter><StartingIndex>0</StartingIndex><RequestedCount>1</RequestedCount>\
             <SortCriteria>+dc:title</SortCriteria>",
        ),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = body_of(response).await;
    assert!(body.contains("<u:SearchResponse"), "{body}");
    assert!(
        body.contains("<NumberReturned>1</NumberReturned>"),
        "{body}"
    );
    assert!(
        body.contains("<TotalMatches>2</TotalMatches>"),
        "the real total, which is the field that says a second page exists: {body}"
    );
}

#[tokio::test]
async fn criteria_this_server_cannot_evaluate_fault_rather_than_answering_everything() {
    let tree = library_tree("wire-search-refused");
    let server = serving(&tree);
    let body = body_of(
        ask(
            &server,
            soap(
                "Search",
                "<ContainerID>0</ContainerID><SearchCriteria>dc:title = &quot;a&quot; and \
                 dc:title = &quot;b&quot; or dc:title = &quot;c&quot;</SearchCriteria>\
                 <Filter>*</Filter>\
                 <StartingIndex>0</StartingIndex><RequestedCount>0</RequestedCount>\
                 <SortCriteria></SortCriteria>",
            ),
        )
        .await,
    )
    .await;
    assert!(body.contains("<errorCode>708</errorCode>"), "{body}");
}

/// The query BubbleUPnP sends for every name typed into its search box, driven through the router.
#[tokio::test]
async fn a_disjunction_a_control_point_sends_is_answered_rather_than_faulted() {
    let tree = library_tree("wire-search-disjunction");
    let server = serving(&tree);
    let body = body_of(
        ask(
            &server,
            soap(
                "Search",
                "<ContainerID>0</ContainerID><SearchCriteria>upnp:class derivedfrom \
                 &quot;object.item.audioItem&quot; and (dc:title contains &quot;01&quot; \
                 or dc:title contains &quot;02&quot;)</SearchCriteria><Filter>*</Filter>\
                 <StartingIndex>0</StartingIndex><RequestedCount>0</RequestedCount>\
                 <SortCriteria></SortCriteria>",
            ),
        )
        .await,
    )
    .await;
    assert!(!body.contains("errorCode"), "it must not fault: {body}");
    assert!(
        body.contains("<TotalMatches>2</TotalMatches>"),
        "each side of the disjunction names one track and the union names both: {body}"
    );
}

#[tokio::test]
async fn the_resource_url_follows_the_host_the_client_used() {
    let tree = library_tree("wire-host");
    let server = serving(&tree);
    let body = body_of(ask(&server, browse("music", "BrowseDirectChildren", 0, 1)).await).await;
    assert!(
        body.contains("http://192.0.2.1:8200/media/"),
        "a machine with several interfaces must not advertise the wrong one: {body}"
    );
}

/// The DIDL a SOAP answer carries, which travels escaped inside the envelope.
fn didl_in(answer: &str) -> String {
    answer
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

/// The container a listing gives an object as its parent.
fn parent_in(answer: &str, id: &str) -> String {
    let didl = didl_in(answer);
    let (_, after) = didl
        .split_once(&format!("id=\"{id}\""))
        .expect("the object is in the listing");
    let (_, rest) = after.split_once("parentID=\"").expect("a parent");
    rest[..rest.find('"').expect("a closing quote")].to_owned()
}

/// The identifiers a listing offers, with the title each one carries.
fn listing(answer: &str) -> Vec<(String, String)> {
    let didl = didl_in(answer);
    let mut found = Vec::new();
    for element in didl.split('<').skip(1) {
        let Some(rest) = element
            .strip_prefix("container id=\"")
            .or_else(|| element.strip_prefix("item id=\""))
        else {
            continue;
        };
        let id = rest[..rest.find('"').expect("a closing quote")].to_owned();
        let title = didl
            .split_once(&format!("id=\"{id}\""))
            .and_then(|(_, after)| after.split_once("<dc:title>"))
            .and_then(|(_, after)| after.split_once("</dc:title>"))
            .map(|(title, _)| title.to_owned())
            .unwrap_or_default();
        found.push((id, title));
    }
    found
}

async fn children_of(server: &Server, object: &str) -> Vec<(String, String)> {
    listing(&body_of(ask(server, browse(object, "BrowseDirectChildren", 0, 0)).await).await)
}

/// A server over a library the menus have something to say about.
fn serving_tagged() -> Server {
    let mut files = Vec::new();
    for (folder, artist, genre, album, count) in [
        (
            "Reggae/Scientist/Dub",
            "Scientist",
            "Reggae",
            "Dub Landing",
            2,
        ),
        (
            "Reggae/Spear/Marcus",
            "Burning Spear",
            "Reggae",
            "Marcus Garvey",
            1,
        ),
        (
            "Latin/Sierra/Dundunbanza",
            "Sierra Maestra",
            "Latin",
            "Dundunbanza",
            2,
        ),
    ] {
        for track in 1..=count {
            files.push(fixtures::tagged(
                &format!("{folder}/0{track}.wav"),
                album,
                artist,
                genre,
                track,
            ));
        }
    }
    let library = Library::build("Music".to_owned(), &files);
    // The menus give up and show the albums once a selection is small enough.
    let settings = kantele::browse::Settings {
        album_threshold: 0,
        ..kantele::browse::Settings::default()
    };
    Server::new(
        library,
        settings,
        DeviceIdentity {
            friendly_name: "Kantele Test".to_owned(),
            udn: "3d5d1cbe-8f2a-4d1e-9a9c-7c2f0a1b2c3d".to_owned(),
            icon: Default::default(),
        },
        Profiles::default(),
    )
}

#[tokio::test]
async fn the_folder_view_can_be_walked_from_the_root_to_a_track() {
    let server = serving_tagged();

    let root = children_of(&server, "0").await;
    let (folders, _) = root
        .iter()
        .find(|(_, title)| title == "[folder view]")
        .expect("the folder view is offered")
        .clone();

    let top = children_of(&server, &folders).await;
    let names: Vec<&str> = top.iter().map(|(_, title)| title.as_str()).collect();
    assert_eq!(names, vec!["Latin", "Reggae"], "sorted, and nothing else");

    // Reggae holds two artists, so the tag rule has something to say inside it.
    let (reggae, _) = top[1].clone();
    let inside = children_of(&server, &reggae).await;
    assert!(
        inside.iter().any(|(_, title)| title == "Scientist"),
        "the folders below: {inside:?}"
    );
    assert!(
        inside.iter().any(|(_, title)| title == "[tag view]"),
        "a folder is one more thing to filter on: {inside:?}"
    );

    // Latin holds one artist and one genre.
    let (latin, _) = top[0].clone();
    let quiet = children_of(&server, &latin).await;
    assert!(
        !quiet.iter().any(|(_, title)| title == "[tag view]"),
        "{quiet:?}"
    );

    // And the container describes itself the same way its parent's listing did.
    let metadata = body_of(ask(&server, browse(&latin, "BrowseMetadata", 0, 0)).await).await;
    assert!(
        metadata.contains("&lt;dc:title&gt;Latin&lt;/dc:title&gt;"),
        "{metadata}"
    );
    assert!(
        metadata.contains("<NumberReturned>1</NumberReturned>"),
        "{metadata}"
    );

    // Down to the tracks.
    let (sierra, _) = quiet[0].clone();
    let album_folder = children_of(&server, &sierra).await;
    let (leaf, _) = album_folder[0].clone();
    let tracks = children_of(&server, &leaf).await;
    assert!(
        tracks.iter().any(|(id, _)| id.starts_with("tr-")),
        "the folder view reaches the music: {tracks:?}"
    );
}

#[tokio::test]
async fn in_the_folder_view_a_track_belongs_to_the_folder_it_sits_in() {
    let server = serving_tagged();

    let root = children_of(&server, "0").await;
    let (folders, _) = root
        .iter()
        .find(|(_, title)| title == "[folder view]")
        .expect("the folder view is offered")
        .clone();
    let (latin, _) = children_of(&server, &folders).await[0].clone();
    let (sierra, _) = children_of(&server, &latin).await[0].clone();
    let (leaf, _) = children_of(&server, &sierra).await[0].clone();

    let answer = body_of(ask(&server, browse(&leaf, "BrowseDirectChildren", 0, 0)).await).await;
    let (track, _) = listing(&answer)
        .into_iter()
        .find(|(id, _)| id.starts_with("tr-"))
        .expect("the leaf folder holds the music");
    assert_eq!(
        parent_in(&answer, &track),
        leaf,
        "going up from a track leads back to the folder it was listed in, not to every title"
    );
}

#[tokio::test]
async fn a_container_describes_itself_the_way_its_parent_listed_it() {
    let server = serving_tagged();

    // The root's own listing is what a device reads first.
    let root = children_of(&server, "0").await;
    for (id, title) in &root {
        let metadata = body_of(ask(&server, browse(id, "BrowseMetadata", 0, 0)).await).await;
        let didl = didl_in(&metadata);
        assert!(
            didl.contains(&format!("<dc:title>{title}</dc:title>")),
            "{id} is \"{title}\" in the root and something else in its own metadata: {didl}"
        );
    }

    // And a nested folder answers the folder it sits in, not the top of the view.
    let (folders, _) = root
        .iter()
        .find(|(_, title)| title == "[folder view]")
        .expect("the folder view is offered")
        .clone();
    let (latin, _) = children_of(&server, &folders).await[0].clone();
    let listed = body_of(ask(&server, browse(&latin, "BrowseDirectChildren", 0, 0)).await).await;
    let (sierra, _) = listing(&listed)[0].clone();

    let metadata = body_of(ask(&server, browse(&sierra, "BrowseMetadata", 0, 0)).await).await;
    assert_eq!(
        parent_in(&metadata, &sierra),
        parent_in(&listed, &sierra),
        "one folder, two parents, depending on which way it was asked about"
    );
}

#[tokio::test]
async fn a_folder_counts_the_children_it_renders() {
    let server = serving_tagged();

    let root = children_of(&server, "0").await;
    let (folders, _) = root
        .iter()
        .find(|(_, title)| title == "[folder view]")
        .expect("the folder view is offered")
        .clone();
    // Reggae holds two artists, so the tag rule offers a view inside it and that is a child too.
    let top = children_of(&server, &folders).await;
    let (reggae, _) = top[1].clone();

    let listed = body_of(ask(&server, browse(&folders, "BrowseDirectChildren", 0, 0)).await).await;
    let rendered = children_of(&server, &reggae).await.len();
    let didl = didl_in(&listed);
    assert!(
        didl.contains(&format!("childCount=\"{rendered}\"")),
        "the parent's listing promises a count this folder does not render ({rendered}): {didl}"
    );
}

/// `TotalMatches` out of a browse answer.
fn total_in(answer: &str) -> usize {
    let (_, after) = answer
        .split_once("<TotalMatches>")
        .expect("every answer carries a total");
    after
        .split_once("</TotalMatches>")
        .expect("a closing tag")
        .0
        .parse()
        .expect("a number")
}

fn returned_in(answer: &str) -> usize {
    let (_, after) = answer
        .split_once("<NumberReturned>")
        .expect("every answer says how many it carried");
    after
        .split_once("</NumberReturned>")
        .expect("a closing tag")
        .0
        .parse()
        .expect("a number")
}

#[tokio::test]
async fn the_total_is_the_same_at_every_offset_and_past_the_end() {
    let server = serving_tagged();
    let whole = body_of(ask(&server, browse("music", "BrowseDirectChildren", 0, 0)).await).await;
    let total = total_in(&whole);
    assert!(total > 1, "this needs a container with several items");

    for offset in 0..=total + 1 {
        let answer =
            body_of(ask(&server, browse("music", "BrowseDirectChildren", offset, 1)).await).await;
        assert_eq!(
            total_in(&answer),
            total,
            "offset {offset} answers a different total than offset 0"
        );
        let expected = usize::from(offset < total);
        assert_eq!(
            returned_in(&answer),
            expected,
            "offset {offset} of {total} returned the wrong number of items"
        );
    }
}

#[tokio::test]
async fn a_paging_number_this_server_cannot_read_is_a_page_and_not_a_fault() {
    let server = serving_tagged();
    let whole = body_of(ask(&server, browse("music", "BrowseDirectChildren", 0, 0)).await).await;
    let total = total_in(&whole);

    let asked = |index: &str, count: &str| {
        soap(
            "Browse",
            &format!(
                "<ObjectID>music</ObjectID><BrowseFlag>BrowseDirectChildren</BrowseFlag>\
                 <Filter>*</Filter>{index}{count}<SortCriteria></SortCriteria>"
            ),
        )
    };
    for index in [
        "<StartingIndex> 1 </StartingIndex>",
        "<StartingIndex>abc</StartingIndex>",
        "<StartingIndex>-1</StartingIndex>",
        "",
    ] {
        let answer =
            body_of(ask(&server, asked(index, "<RequestedCount>1</RequestedCount>")).await).await;
        assert!(!answer.contains("errorCode"), "{index}: {answer}");
        assert_eq!(total_in(&answer), total, "{index}: the total is the truth");
        assert_eq!(returned_in(&answer), 1, "{index}: one item was asked for");
    }

    for count in [
        "<RequestedCount>abc</RequestedCount>",
        "<RequestedCount>-1</RequestedCount>",
        "",
    ] {
        let answer =
            body_of(ask(&server, asked("<StartingIndex>0</StartingIndex>", count)).await).await;
        assert!(!answer.contains("errorCode"), "{count}: {answer}");
        assert_eq!(
            returned_in(&answer),
            total,
            "{count}: a count this server cannot read means the whole container, as zero does"
        );
    }
}

#[tokio::test]
async fn the_root_and_the_folder_view_count_the_children_they_render() {
    let server = serving_tagged();
    let mut checked = 0;
    let mut waiting = vec!["0".to_owned()];

    while let Some(id) = waiting.pop() {
        let listed = body_of(ask(&server, browse(&id, "BrowseDirectChildren", 0, 0)).await).await;
        let didl = didl_in(&listed);
        for (child, title) in listing(&listed) {
            let Some(count) = child_count_in(&didl, &child) else {
                continue;
            };
            let below = children_of(&server, &child).await;
            assert_eq!(
                below.len(),
                count,
                "{title} ({child}) promises {count} children and renders {}",
                below.len()
            );
            checked += 1;
            // Down the folder view, where a nested folder also offers the tag rule as a child.
            if child.starts_with("fv") {
                waiting.push(child);
            }
        }
    }
    assert!(checked > 5, "only {checked} containers were checked");
}

#[tokio::test]
async fn a_chosen_value_counts_the_albums_it_renders_rather_than_its_tracks() {
    let server = serving_tagged();
    let root = children_of(&server, "0").await;
    let (artists, _) = root
        .iter()
        .find(|(_, title)| title == "Artist")
        .expect("the artist axis is offered")
        .clone();

    let listed = body_of(ask(&server, browse(&artists, "BrowseDirectChildren", 0, 0)).await).await;
    let didl = didl_in(&listed);
    let (scientist, _) = listing(&listed)
        .into_iter()
        .find(|(_, title)| title == "Scientist")
        .expect("an artist with two tracks on one album");

    let promised = child_count_in(&didl, &scientist).expect("a count");
    let rendered = children_of(&server, &scientist).await;
    assert_eq!(rendered.len(), 1, "the one album its two tracks belong to");
    assert_eq!(promised, rendered.len(), "a client pages by the count");
}

/// A library whose tag menus reach every shape a container renders: axes still to narrow, albums
/// with a loose file beside them, and listings long enough to open on their letter index.
fn serving_every_shape() -> Server {
    let mut files = Vec::new();
    for (folder, artist, genre, album) in [
        ("Reggae/Scientist/Dub", "Scientist", "Reggae", "Dub Landing"),
        (
            "Reggae/Scientist/Heavy",
            "Scientist",
            "Reggae",
            "Heavyweight Dub",
        ),
        ("Reggae/Spear", "Burning Spear", "Reggae", "Marcus Garvey"),
        ("Reggae/Tubby", "King Tubby", "Reggae", "Dub Gone Crazy"),
        ("Latin/Sierra", "Sierra Maestra", "Latin", "Dundunbanza"),
        ("Latin/Ochoa", "Eliades Ochoa", "Latin", "Sublime Ilusion"),
    ] {
        for track in 1..=2 {
            files.push(fixtures::tagged(
                &format!("{folder}/0{track}.wav"),
                album,
                artist,
                genre,
                track,
            ));
        }
    }
    let mut loose = fixtures::tagged("Reggae/Scientist/single.wav", "", "Scientist", "Reggae", 1);
    loose.tags.album = None;
    files.push(loose);
    let library = Library::build("Music".to_owned(), &files);
    let settings = kantele::browse::Settings {
        album_threshold: 1,
        alpha_group: Some(3),
        ..kantele::browse::Settings::default()
    };
    Server::new(
        library,
        settings,
        DeviceIdentity {
            friendly_name: "Kantele Test".to_owned(),
            udn: "3d5d1cbe-8f2a-4d1e-9a9c-7c2f0a1b2c3d".to_owned(),
            icon: Default::default(),
        },
        Profiles::default(),
    )
}

#[tokio::test]
async fn every_container_counts_the_children_it_renders() {
    let server = serving_every_shape();
    // A letter index, albums beside a loose file, and a chosen value that still narrows.
    let mut shapes = [false; 3];
    let mut checked = 0;
    let mut seen = std::collections::HashSet::new();
    let mut waiting = vec!["0".to_owned()];

    while let Some(id) = waiting.pop() {
        let listed = body_of(ask(&server, browse(&id, "BrowseDirectChildren", 0, 0)).await).await;
        let didl = didl_in(&listed);
        for (child, title) in listing(&listed) {
            let Some(count) = child_count_in(&didl, &child) else {
                continue;
            };
            let below = children_of(&server, &child).await;
            assert_eq!(
                below.len(),
                count,
                "{title} ({child}) under {id} promises {count} children and renders {below:?}"
            );
            let opens = |prefix: &str| below.iter().any(|(at, _)| at.starts_with(prefix));
            if child.starts_with("f-") {
                shapes[0] |= below.iter().any(|(_, title)| title == "A-Z");
                shapes[1] |= opens("al-") && opens("tr-");
                shapes[2] |= child.starts_with("f-g") && child.len() > 3 && opens("f-");
            }
            checked += 1;
            if seen.insert(child.clone()) {
                waiting.push(child);
            }
        }
    }
    assert!(checked > 30, "only {checked} containers were checked");
    assert_eq!(shapes, [true; 3], "a shape the walk never reached");
}

/// The `childCount` a listing gives a container, where it gives one.
fn child_count_in(didl: &str, id: &str) -> Option<usize> {
    let (_, after) = didl.split_once(&format!("id=\"{id}\""))?;
    let (before, _) = after.split_once('>')?;
    let (_, rest) = before.split_once("childCount=\"")?;
    rest.split_once('"')?.0.parse().ok()
}

#[tokio::test]
async fn a_faceted_position_narrows_until_it_offers_the_albums() {
    let server = serving_tagged();

    let root = children_of(&server, "0").await;
    let (genre, _) = root
        .iter()
        .find(|(_, title)| title == "Genre")
        .expect("an axis that still narrows something")
        .clone();
    assert!(genre.starts_with('f'), "a faceted position: {genre}");

    let values = children_of(&server, &genre).await;
    assert_eq!(values.len(), 2, "two genres: {values:?}");

    let (one, chosen) = values[0].clone();
    let held = children_of(&server, &one).await;
    assert!(!held.is_empty(), "choosing {chosen} showed nothing");

    let metadata = body_of(ask(&server, browse(&genre, "BrowseMetadata", 0, 0)).await).await;
    assert!(
        metadata.contains("&lt;dc:title&gt;Genre&lt;/dc:title&gt;"),
        "{metadata}"
    );

    // A position a client cached against a library that has moved on names no object.
    let stale = ask(
        &server,
        browse("f-g0123456789abcdef", "BrowseDirectChildren", 0, 0),
    )
    .await;
    assert_eq!(stale.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let fault = body_of(stale).await;
    assert!(fault.contains("<errorCode>701</errorCode>"), "{fault}");
}

#[tokio::test]
async fn a_search_scoped_to_a_folder_container_stays_inside_it() {
    let server = serving_tagged();
    let root = children_of(&server, "0").await;
    let (albums, _) = root
        .iter()
        .find(|(_, title)| title.ends_with("albums"))
        .expect("an album index")
        .clone();

    let inside = children_of(&server, &albums).await;
    let (one_album, title) = inside[0].clone();
    let answer = body_of(
        ask(
            &server,
            soap(
                "Search",
                &format!(
                    "<ContainerID>{one_album}</ContainerID><SearchCriteria>*</SearchCriteria>\
                     <Filter>*</Filter><StartingIndex>0</StartingIndex>\
                     <RequestedCount>0</RequestedCount><SortCriteria></SortCriteria>"
                ),
            ),
        )
        .await,
    )
    .await;
    let held = listing(&answer);
    assert!(!held.is_empty(), "the album {title} holds tracks: {answer}");
    assert!(
        held.len() < inside.len() + 3,
        "a scoped search must not answer with the whole library: {held:?}"
    );
}

#[tokio::test]
async fn a_content_features_header_the_client_wrote_wrong_is_a_bad_request() {
    let tree = library_tree("wire-features");
    let server = serving(&tree);
    let id = first_track(&server).await;
    let asked = |value: &str| {
        Request::builder()
            .uri(format!("/media/{id}"))
            .header("getcontentfeatures.dlna.org", value)
            .body(Body::empty())
            .expect("a request")
    };
    assert_eq!(ask(&server, asked("1")).await.status(), StatusCode::OK);
    for wrong in ["0", "true", "", "1,1"] {
        assert_eq!(
            ask(&server, asked(wrong)).await.status(),
            StatusCode::BAD_REQUEST,
            "the header has one legal value and {wrong:?} is not it"
        );
    }
}

#[tokio::test]
async fn a_range_written_backwards_or_unreadable_is_refused_rather_than_ignored() {
    let tree = library_tree("wire-bad-range");
    let server = serving(&tree);
    let id = first_track(&server).await;
    for range in ["bytes=34-0", "bytes=a-b", "bytes=999999-"] {
        let response = ask(
            &server,
            Request::builder()
                .uri(format!("/media/{id}"))
                .header(header::RANGE, range)
                .body(Body::empty())
                .expect("a request"),
        )
        .await;
        assert_eq!(
            response.status(),
            StatusCode::RANGE_NOT_SATISFIABLE,
            "{range} should not be answered with bytes"
        );
        assert_eq!(body_of(response).await.len(), 0);
    }
}

/// Every item identifier a browse answered, in the order the response wrote them.
fn item_ids(answer: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut rest = answer;
    while let Some(at) = rest.find("&lt;item id=&quot;") {
        rest = &rest[at + 18..];
        let end = rest.find("&quot;").expect("a closing quote");
        ids.push(rest[..end].to_owned());
        rest = &rest[end..];
    }
    ids
}

#[tokio::test]
async fn a_page_of_a_container_is_a_slice_of_the_order_the_whole_container_has() {
    let tree = Tree::new("wire-paging");
    tree.album("Zulu", &["Zulu.wav"], false);
    tree.album("Mango", &["Mango.wav"], false);
    tree.album("Alpha", &["Alpha.wav"], false);
    tree.album("Kilo", &["Kilo.wav"], false);
    let server = serving(&tree);

    let whole =
        item_ids(&body_of(ask(&server, browse("music", "BrowseDirectChildren", 0, 0)).await).await);
    assert_eq!(whole.len(), 4, "the fixture holds four tracks");

    for (at, expected) in whole.iter().enumerate() {
        let page = item_ids(
            &body_of(ask(&server, browse("music", "BrowseDirectChildren", at, 1)).await).await,
        );
        assert_eq!(
            page.as_slice(),
            std::slice::from_ref(expected),
            "index {at} of a one-item page disagrees with the whole list"
        );
    }

    let middle =
        item_ids(&body_of(ask(&server, browse("music", "BrowseDirectChildren", 1, 2)).await).await);
    assert_eq!(
        middle.as_slice(),
        &whole[1..3],
        "a two-item page is the same slice"
    );
}

#[tokio::test]
async fn with_a_capture_folder_a_browse_is_written_verbatim_and_still_answered() {
    let tree = library_tree("wire-capture");
    let server = serving(&tree);
    let folder = tree.path("captures");
    server
        .device
        .capture_to(kantele::upnp::capture::Recorder::new(&folder).expect("a folder"));

    let mut request = browse("0", "BrowseDirectChildren", 0, 10);
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([192, 0, 2, 219], 51000))));
    request
        .headers_mut()
        .insert(header::USER_AGENT, "Denon-Heos/1".parse().expect("a value"));
    let response = ask(&server, request).await;
    assert_eq!(response.status(), StatusCode::OK);
    let answer = body_of(response).await;
    assert!(answer.contains("<NumberReturned>"), "answered as before");

    let peer = folder.join("192.0.2.219");
    let written =
        std::fs::read_to_string(peer.join("000001-Browse.request.txt")).expect("the request");
    assert!(written.starts_with("POST /control/ContentDirectory\n"));
    assert!(written.contains("user-agent: Denon-Heos/1\n"));
    assert!(
        written.ends_with("</s:Envelope>"),
        "the body follows the headers verbatim"
    );
    let recorded =
        std::fs::read_to_string(peer.join("000001-Browse.200.response.xml")).expect("the response");
    assert_eq!(recorded, answer, "what was written is what the client got");
}

/// A request as it arrives over a socket, with the address it came from.
fn from_device(mut request: Request<Body>, last: u8, agent: &str) -> Request<Body> {
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([192, 0, 2, last], 51000))));
    request
        .headers_mut()
        .insert(header::USER_AGENT, agent.parse().expect("a value"));
    request
}

#[tokio::test]
async fn the_status_says_how_far_each_device_got_and_a_shell_reads_the_same() {
    let tree = library_tree("wire-devices");
    let server = serving(&tree);
    ask(
        &server,
        from_device(get("/description.xml"), 219, "Denon-Heos/1"),
    )
    .await;
    ask(
        &server,
        from_device(
            browse("0", "BrowseDirectChildren", 0, 10),
            219,
            "Denon-Heos/1",
        ),
    )
    .await;
    ask(
        &server,
        from_device(get("/description.xml"), 7, "BubbleUPnP"),
    )
    .await;
    ask(&server, browse("0", "BrowseDirectChildren", 0, 10)).await;

    let mut asked = get("/api/status");
    asked
        .headers_mut()
        .insert(header::ACCEPT, "application/json".parse().expect("a value"));
    let status: serde_json::Value =
        serde_json::from_str(&body_of(ask(&server, asked).await).await).expect("json");
    let devices = status["devices"].as_array().expect("a list");
    assert_eq!(
        devices.len(),
        2,
        "a request with no address is nobody: {devices:?}"
    );
    let heos = devices
        .iter()
        .find(|d| d["address"] == "192.0.2.219")
        .expect("the HEOS");
    assert_eq!(heos["name"], "Denon-Heos/1");
    assert!(
        heos["searched"].is_null(),
        "it never searched over SSDP here"
    );
    assert_eq!(heos["described"]["times"], 1);
    assert_eq!(heos["browsed"]["times"], 1);
    assert_eq!(heos["says"], "browsing");
    let bubble = devices
        .iter()
        .find(|d| d["address"] == "192.0.2.7")
        .expect("the phone");
    assert!(
        bubble["says"]
            .as_str()
            .is_some_and(|says| says.starts_with("found this server by its announcement")),
        "{}",
        bubble["says"]
    );

    let lines = body_of(ask(&server, get("/api/status")).await).await;
    assert!(
        lines.contains("\tDenon-Heos/1\tbrowsing\n"),
        "the same words for a shell: {lines}"
    );
    assert!(lines.contains(".searched = never\n"), "{lines}");
}

#[tokio::test]
async fn without_a_capture_folder_nothing_is_written() {
    let tree = library_tree("wire-no-capture");
    let server = serving(&tree);
    let response = ask(&server, browse("0", "BrowseDirectChildren", 0, 10)).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(!tree.path("captures").exists());
}
