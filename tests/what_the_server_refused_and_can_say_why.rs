//! What a pass refused, what survives a start from the store, and the three routes over it.

use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, StatusCode, header};
use axum::response::Response;
use kantele::api::Operation;
use kantele::index::refusals::{Cause, Origin};
use kantele::index::{Library, Store};
use kantele::server::{self, Server};
use kantele::service::{self, Indexing, Pass};
use kantele::upnp::client::Profiles;
use kantele::upnp::description::DeviceIdentity;
use tower::ServiceExt;

mod fixtures;
use fixtures::Tree;

fn library_with_a_broken_playlist(name: &str) -> Tree {
    let tree = Tree::new(name);
    tree.album("Sierra Maestra", &["01.wav", "02.wav"], true);
    tree.text(
        "Sierra Maestra/set.m3u",
        "#EXTM3U\n\
         #EXTINF:1,Dundunbanza\n\
         01.wav\n\
         #EXTINF:1,A track that moved\n\
         03.wav\n\
         #EXTINF:1,Dundunbanza again\n\
         01.wav\n",
    );
    tree.text("Sierra Maestra/gone.m3u", "#EXTM3U\nnowhere/at/all.wav\n");
    tree
}

fn scanned(tree: &Tree) -> Library {
    Library::scan(&tree.0).expect("scanning the test tree")
}

#[test]
fn a_file_the_pass_refused_is_remembered_and_not_opened_again_until_it_changes() {
    let tree = Tree::new("refused-remembered");
    tree.album("Sierra Maestra", &["01.wav"], false);
    tree.text("Sierra Maestra/broken.mp3", "not audio at all");
    let mut store = Some(Store::in_memory().expect("a store"));
    let indexing = Indexing::of(&tree.0);

    let first = service::index(&indexing, &mut store, Pass::Whole).expect("a pass");
    assert_eq!(first.pass.refusals.total_of(Cause::UnreadableFile), 1);
    assert_eq!(first.library.len(), 1);

    assert_eq!(
        service::sweep(&indexing, store.as_ref().expect("a store")).expect("a look"),
        None,
        "a file that would not read is not a change every look reports"
    );
    let second = service::index(&indexing, &mut store, Pass::Whole).expect("a pass");
    assert_eq!(second.pass.refusals.total_of(Cause::UnreadableFile), 1);
    assert_eq!(second.pass.opened, 0);
    assert!(
        !second.changed,
        "a pass that learned nothing publishes nothing"
    );

    // Fixed in place: the file reads now.
    tree.write("Sierra Maestra/broken.mp3", 5);
    let third = service::index(&indexing, &mut store, Pass::Whole).expect("a pass");
    assert_eq!(third.pass.refusals.total_of(Cause::UnreadableFile), 0);
    assert_eq!(third.library.len(), 2);
    assert_eq!(
        service::sweep(&indexing, store.as_ref().expect("a store")).expect("a look"),
        None
    );
}

#[test]
fn a_refusal_the_index_build_produced_survives_a_start_that_walks_nothing() {
    let tree = library_with_a_broken_playlist("refusals-from-the-store");
    let mut store = Some(Store::in_memory().expect("a store"));
    let indexing = Indexing::of(&tree.0);
    let walked = service::index(&indexing, &mut store, Pass::Whole).expect("a pass");

    // gone.m3u's one missing entry also leaves the playlist itself unpublished.
    for (cause, expected) in [
        (Cause::MissingEntry, 2),
        (Cause::UnpublishedPlaylist, 1),
        (Cause::RepeatedEntry, 1),
    ] {
        assert_eq!(
            walked.library.refusals().total_of(cause),
            expected,
            "{} was not recorded by the pass",
            cause.as_str()
        );
    }

    let remembered = service::remembered(&indexing, &mut store).expect("the store answers");
    for cause in [
        Cause::MissingEntry,
        Cause::UnpublishedPlaylist,
        Cause::RepeatedEntry,
    ] {
        assert_eq!(
            remembered.refusals().total_of(cause),
            walked.library.refusals().total_of(cause),
            "{} did not survive a start from the store",
            cause.as_str()
        );
        assert_eq!(cause.origin(), Origin::Index);
    }
}

#[test]
fn a_refusal_names_what_it_refused_and_what_that_named() {
    let tree = library_with_a_broken_playlist("refusals-name-things");
    let library = scanned(&tree);
    let held = library.refusals().held(Cause::MissingEntry);
    let named = held
        .iter()
        .find(|refusal| refusal.subject == "Sierra Maestra/set.m3u")
        .unwrap_or_else(|| panic!("the playlist that named a moved file, got {held:?}"));
    let detail = named.detail.as_deref().expect("what the entry named");
    assert!(detail.contains("03.wav"), "{detail}");
    assert!(
        detail.contains("A track that moved"),
        "the text the playlist wrote is what an owner searches for: {detail}"
    );
}

#[test]
fn a_clean_library_refuses_nothing_and_the_record_costs_nothing() {
    let tree = Tree::new("refusals-clean");
    tree.album("Kremerata", &["01.wav", "02.wav"], true);
    let library = scanned(&tree);
    assert!(library.refusals().is_empty());
    assert!(library.refusals().reported().is_empty());
}

#[test]
fn a_pass_over_one_folder_keeps_what_the_last_one_said_about_the_rest() {
    let tree = Tree::new("refusals-carried-forward");
    tree.album("Sierra Maestra", &["01.wav"], false);
    tree.album("Kremerata", &["01.wav"], false);
    tree.text("Kremerata/02.wav", "this is not audio");

    let mut store = Some(Store::in_memory().expect("a store"));
    let indexing = Indexing::of(&tree.0);
    let whole = service::index(&indexing, &mut store, Pass::Whole).expect("a pass");
    assert_eq!(whole.pass.refusals.total_of(Cause::UnreadableFile), 1);

    let scope = kantele::index::scan::Scope::of([std::path::PathBuf::from("Sierra Maestra")]);
    let partial =
        service::index(&indexing, &mut store, Pass::Within(scope.clone())).expect("a second pass");
    assert_eq!(
        partial.pass.refusals.total_of(Cause::UnreadableFile),
        0,
        "a pass over one folder saw nothing of the other"
    );
    assert_eq!(
        whole
            .pass
            .carried_forward(&scope)
            .total_of(Cause::UnreadableFile),
        1,
        "and what the whole-tree pass said about the folder it did not walk still stands"
    );
}

#[test]
fn the_two_halves_of_the_record_never_hold_the_same_refusal() {
    let tree = Tree::new("refusals-are-disjoint");
    tree.album("Kremerata", &["01.wav"], false);
    tree.text("Kremerata/02.wav", "this is not audio");
    tree.text("Kremerata/set.m3u", "#EXTM3U\nnowhere.wav\n");

    let mut store = Some(Store::in_memory().expect("a store"));
    let indexed = service::index(&Indexing::of(&tree.0), &mut store, Pass::Whole).expect("a pass");
    for cause in Cause::ALL {
        let index_built = indexed.library.refusals().total_of(*cause);
        let walked = indexed.pass.refusals.total_of(*cause);
        assert!(
            index_built == 0 || walked == 0,
            "{} is in both halves, so the status would count it twice",
            cause.as_str()
        );
        assert_eq!(
            index_built > 0,
            cause.origin() == Origin::Index && index_built + walked > 0,
            "{} landed on the wrong side of the split",
            cause.as_str()
        );
    }
    assert_eq!(indexed.pass.refusals.total_of(Cause::UnreadableFile), 1);
    assert_eq!(indexed.library.refusals().total_of(Cause::MissingEntry), 1);
}

fn serving(tree: &Tree, pass: Option<service::PassReport>) -> Server {
    let server = Server::new(
        scanned(tree),
        kantele::browse::Settings::default(),
        DeviceIdentity {
            friendly_name: "Kantele Test".to_owned(),
            udn: "3d5d1cbe-8f2a-4d1e-9a9c-7c2f0a1b2c3d".to_owned(),
            icon: Default::default(),
        },
        Profiles::default(),
    );
    server.control.set_operation(Operation {
        store: Some(std::path::PathBuf::from("/state/index.sqlite")),
        config: kantele::config::Resolved::load(None, Some("/music".into()))
            .expect("the layers resolve"),
    });
    if let Some(pass) = pass {
        server.passes.record(pass);
    }
    server
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

fn get_json(path: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        .header(header::ACCEPT, "application/json")
        .body(Body::empty())
        .expect("a request")
}

#[tokio::test]
async fn the_status_answers_every_number_that_has_needed_ssh_to_read() {
    let tree = library_with_a_broken_playlist("status-route");
    let server = serving(&tree, None);
    let response = ask(&server, get_json("/api/status")).await;
    assert_eq!(response.status(), StatusCode::OK);
    let status: serde_json::Value =
        serde_json::from_str(&body_of(response).await).expect("json out");

    assert_eq!(status["name"], "Kantele Test");
    assert_eq!(status["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(status["state"], "serving");
    assert_eq!(status["library"]["tracks"], 2);
    assert_eq!(status["library"]["playlists"], 1);
    assert_eq!(status["system_update_id"], 1);
    assert_eq!(status["store"]["open"], true);
    assert!(status["uptime_seconds"].is_number());
    assert!(status["subscribers"].as_array().expect("a list").is_empty());
}

#[tokio::test]
async fn the_status_says_nobody_has_looked_rather_than_that_nothing_was_skipped() {
    let tree = library_with_a_broken_playlist("status-has-not-walked");
    let served = serving(&tree, None);
    let status: serde_json::Value =
        serde_json::from_str(&body_of(ask(&served, get_json("/api/status")).await).await)
            .expect("json out");
    assert_eq!(status["last_pass"], serde_json::Value::Null);
    assert_eq!(
        status["refused"]["walked"], false,
        "a start from the store has walked nothing, and a page must not read that as all clear"
    );
    assert!(
        status["refused"]["total"].as_u64().expect("a total") > 0,
        "and what the index build refused is answered all the same"
    );
}

#[tokio::test]
async fn the_status_carries_the_way_to_the_list_behind_every_count() {
    let tree = library_with_a_broken_playlist("status-refusals");
    let server = serving(&tree, None);
    let status: serde_json::Value =
        serde_json::from_str(&body_of(ask(&server, get_json("/api/status")).await).await)
            .expect("json out");
    let causes = status["refused"]["causes"].as_array().expect("a list");
    let missing = causes
        .iter()
        .find(|cause| cause["cause"] == "missing-entry")
        .expect("the entries naming nothing");
    assert_eq!(missing["total"], 2);
    assert_eq!(missing["origin"], "index");
    let named: Vec<&str> = missing["shown"]
        .as_array()
        .expect("the list behind the count")
        .iter()
        .map(|refusal| refusal["subject"].as_str().expect("a subject"))
        .collect();
    assert!(
        named.contains(&"Sierra Maestra/set.m3u"),
        "a count with no list behind it is the failure this record exists to end: {named:?}"
    );
}

#[tokio::test]
async fn a_shell_is_answered_lines_and_a_page_is_answered_json() {
    let tree = library_with_a_broken_playlist("status-plain-text");
    let server = serving(&tree, None);
    let response = ask(&server, get("/api/status")).await;
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/plain; charset=utf-8")
    );
    let text = body_of(response).await;
    assert!(text.contains("tracks = 2\n"), "{text}");
    assert!(text.contains("refused.missing-entry.total = 2\n"), "{text}");
    assert!(
        text.contains("last_pass = none since this process started\n"),
        "{text}"
    );
}

#[tokio::test]
async fn the_configuration_says_which_layer_each_value_came_from() {
    let tree = library_with_a_broken_playlist("config-route");
    let server = serving(&tree, None);
    let config: serde_json::Value =
        serde_json::from_str(&body_of(ask(&server, get_json("/api/config")).await).await)
            .expect("json out");
    let settings = config["settings"].as_array().expect("a list");
    let named = |key: &str| {
        settings
            .iter()
            .find(|setting| setting["key"] == key)
            .unwrap_or_else(|| panic!("{key} is answered for"))
            .clone()
    };
    assert_eq!(named("content_dir")["source"]["layer"], "command-line");
    assert_eq!(named("scan.threads")["source"]["layer"], "default");
    assert_eq!(named("menus.album_threshold")["writable"], true);
    assert_eq!(named("menus.album_threshold")["apply"], "immediate");
    assert_eq!(
        named("state_dir")["apply"],
        "restart",
        "moving the store is a page's to do, and nothing of it happens until the server starts again"
    );
    assert_eq!(
        named("content_dir")["writable"],
        false,
        "the command line holds it, and the file cannot outrank the command line"
    );
    assert_eq!(
        named("clients")["apply"],
        "never",
        "a device profile is a table, and the page does not edit tables"
    );
    assert_eq!(
        named("menus.album_threshold")["as_written"],
        serde_json::json!(24),
        "a page edits the value the file writes, not the text a reader is shown"
    );
}

#[tokio::test]
async fn a_rescan_is_asked_for_once_and_the_second_request_says_so() {
    let tree = library_with_a_broken_playlist("rescan-route");
    let server = serving(&tree, None);
    let post = || {
        Request::builder()
            .method(Method::POST)
            .uri("/api/rescan")
            // Every HTTP/1.1 request carries one, and a pass is refused without it.
            .header("Host", "192.0.2.42:8200")
            .body(Body::empty())
            .expect("a request")
    };
    let first = ask(&server, post()).await;
    assert_eq!(first.status(), StatusCode::ACCEPTED);
    assert!(body_of(first).await.contains("started = true"));

    // Nobody is reading the queue in this test, so the one slot is still full.
    let second = ask(&server, post()).await;
    assert_eq!(
        second.status(),
        StatusCode::CONFLICT,
        "a second request while one waits is one pass, not two"
    );
    assert!(
        body_of(second).await.contains("already waiting"),
        "the answer says which of the two it is, since only one of them is refusable"
    );
}

#[tokio::test]
async fn a_pass_asked_for_from_another_site_is_refused() {
    let tree = library_with_a_broken_playlist("rescan-origin");
    let server = serving(&tree, None);
    let post = |origin: Option<&str>| {
        let mut request = Request::builder()
            .method(Method::POST)
            .uri("/api/rescan")
            .header(header::HOST, "192.0.2.1:8200");
        if let Some(origin) = origin {
            request = request.header(header::ORIGIN, origin);
        }
        request.body(Body::empty()).expect("a request")
    };

    let elsewhere = ask(&server, post(Some("https://example.invalid"))).await;
    assert_eq!(elsewhere.status(), StatusCode::FORBIDDEN);

    let sandboxed = ask(&server, post(Some("null"))).await;
    assert_eq!(
        sandboxed.status(),
        StatusCode::FORBIDDEN,
        "a form post from a page this server did not serve"
    );

    let page = ask(&server, post(Some("http://192.0.2.1:8200"))).await;
    assert_eq!(
        page.status(),
        StatusCode::ACCEPTED,
        "this server's own page"
    );

    let shell = ask(&server, post(None)).await;
    assert_eq!(
        shell.status(),
        StatusCode::CONFLICT,
        "a shell sends no origin, and this one is behind the pass the page asked for"
    );
}

#[tokio::test]
async fn the_page_opens_without_credentials_and_is_html() {
    let tree = library_with_a_broken_playlist("page-route");
    let server = serving(&tree, None);
    let response = ask(&server, get("/config")).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok()),
        Some("text/html; charset=utf-8"),
        "an icon in a NAS application menu has to open something a browser renders"
    );
    let body = body_of(response).await;
    assert!(body.contains("<title>Kantele</title>"), "not the page");
    for route in [
        "/api/status",
        "/api/progress",
        "/api/config",
        "/api/folders",
    ] {
        assert!(
            body.contains(route),
            "the page renders what the interface answers rather than reimplementing it, and it \
             does not ask for {route}"
        );
    }
}

/// Absolute URLs that reach nothing: a namespace the DOM is built with, and the address in a
/// warning string. Anything else in a page is a font, a stylesheet or a script from elsewhere.
const INERT: &[&str] = &["http://www.w3.org/", "https://svelte.dev/"];

/// A NAS may have no route out at all.
#[tokio::test]
async fn the_page_asks_nothing_of_the_internet() {
    let tree = library_with_a_broken_playlist("page-offline");
    let server = serving(&tree, None);

    let body = body_of(ask(&server, get("/config")).await).await;
    for fetching in ["<script src", "<link rel=\"stylesheet\"", "@import", "url("] {
        assert!(
            !body.contains(fetching),
            "the page carries {fetching:?}, so a NAS with no route out renders it broken"
        );
    }
    for absolute in absolute_urls(&body) {
        assert!(
            INERT.iter().any(|inert| absolute.starts_with(inert)),
            "the page names {absolute}, which a NAS with no route out cannot reach"
        );
    }
    assert!(
        body.contains("fetch("),
        "the page reads the interface rather than being a static picture of it"
    );
    assert!(!body.contains("DELETE"), "nothing is deleted from a page");
}

/// Every `http://` or `https://` in the page, to the character that cannot be in a URL.
fn absolute_urls(body: &str) -> Vec<String> {
    let mut found = Vec::new();
    for (at, _) in body.match_indices("//") {
        let before = &body[..at];
        if !before.ends_with("http:") && !before.ends_with("https:") {
            continue;
        }
        let from = before.rfind("http").unwrap_or(at);
        let rest = &body[from..];
        let end = rest
            .find(|c: char| c.is_whitespace() || "\"'`<>)".contains(c))
            .unwrap_or(rest.len());
        found.push(rest[..end].to_owned());
    }
    found.sort();
    found.dedup();
    found
}

#[tokio::test]
async fn a_pass_that_published_nothing_is_still_the_record_of_what_it_refused() {
    let tree = Tree::new("refusals-outlive-a-quiet-pass");
    tree.album("Kremerata", &["01.wav"], false);
    tree.text("Kremerata/02.wav", "this is not audio");
    let mut store = Some(Store::in_memory().expect("a store"));
    let indexing = Indexing::of(&tree.0);
    let indexed = service::index(&indexing, &mut store, Pass::Whole).expect("a pass");
    assert_eq!(indexed.pass.refusals.total_of(Cause::UnreadableFile), 1);

    let server = serving(&tree, Some(indexed.pass));
    let status: serde_json::Value =
        serde_json::from_str(&body_of(ask(&server, get_json("/api/status")).await).await)
            .expect("json out");
    assert_eq!(status["refused"]["walked"], true);
    assert_eq!(
        status["last_pass"]["published"], false,
        "a pass that changed nothing swaps no index, and its record is the only one there is"
    );
    let causes = status["refused"]["causes"].as_array().expect("a list");
    let skipped = causes
        .iter()
        .find(|cause| cause["cause"] == "unreadable-file")
        .expect("the file that would not read");
    assert_eq!(skipped["origin"], "pass");
    assert_eq!(skipped["shown"][0]["subject"], "Kremerata/02.wav");
    let detail = skipped["shown"][0]["detail"]
        .as_str()
        .expect("why it would not read");
    assert!(
        !detail.contains("Kremerata/02.wav"),
        "the detail repeats the path the subject already names: {detail}"
    );
}

/// A name whose bytes are not text can only exist where the file system allows it, which rules
/// out APFS and so this developer's Mac; the Linux CI is where this one runs.
#[cfg(target_os = "linux")]
#[test]
fn a_file_whose_name_is_not_text_is_left_out_and_said_so() {
    use std::os::unix::ffi::OsStrExt;

    let tree = Tree::new("refusals-unnameable");
    tree.album("Kremerata", &["01.wav"], false);
    let name = std::ffi::OsStr::from_bytes(b"Kremerata/02\xff.wav");
    std::fs::write(tree.0.join(name), fixtures::wav(2)).expect("a file the name is bytes of");

    let indexed = service::index(&Indexing::of(&tree.0), &mut None, Pass::Whole).expect("a pass");
    assert_eq!(
        indexed.library.len(),
        1,
        "the file it could not name is out"
    );
    assert!(
        indexed.pass.complete,
        "one file left out is not a folder half read, and an incomplete pass publishes nothing"
    );
    let held = indexed.pass.refusals.held(Cause::UnreadableFile);
    let named = held.first().expect("the file whose name is not text");
    assert!(named.subject.starts_with("Kremerata/02"), "{named:?}");
    assert!(
        named
            .detail
            .as_deref()
            .is_some_and(|detail| detail.contains("not text")),
        "{named:?}"
    );
}

/// What the library names is what it serves: minidlna refuses a link out of its media folders
/// unless `wide_links` says otherwise, and nothing here is authenticated either.
#[cfg(unix)]
#[test]
fn a_link_out_of_the_music_folder_is_refused_and_named() {
    let tree = Tree::new("refusals-linked-outside");
    tree.album("Kremerata", &["01.wav"], false);
    let elsewhere = Tree::new("refusals-linked-outside-elsewhere");
    elsewhere.album("Private", &["01.wav", "02.wav"], false);

    std::os::unix::fs::symlink(elsewhere.path("Private"), tree.path("linked-folder"))
        .expect("a link to a folder outside");
    std::os::unix::fs::symlink(
        elsewhere.path("Private/01.wav"),
        tree.path("Kremerata/linked.wav"),
    )
    .expect("a link to a file outside");
    // A link that stays inside is not refused: it is a folder already walked.
    std::os::unix::fs::symlink(tree.path("Kremerata"), tree.path("linked-inside"))
        .expect("a link to a folder inside");

    let indexed = service::index(&Indexing::of(&tree.0), &mut None, Pass::Whole).expect("a pass");
    assert_eq!(
        indexed.library.len(),
        1,
        "only the track the music folder holds"
    );
    let held = indexed.pass.refusals.held(Cause::LinkedOutside);
    assert_eq!(held.len(), 2, "the folder and the file that left: {held:?}");
    assert!(
        held.iter()
            .any(|refusal| refusal.subject == "linked-folder"),
        "{held:?}"
    );
    // Named under whichever path reached it, since a folder two links name is walked once.
    assert!(
        held.iter()
            .any(|refusal| refusal.subject.ends_with("linked.wav")),
        "{held:?}"
    );
    assert!(
        held.iter().all(|refusal| refusal
            .detail
            .as_deref()
            .is_some_and(|why| why.contains("Private"))),
        "a refusal says where the link pointed: {held:?}"
    );
}
