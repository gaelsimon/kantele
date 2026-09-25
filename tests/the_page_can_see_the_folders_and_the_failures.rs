//! The routes the configuration page needs: the folder tree, one folder read again, how far a
//! pass has got, the bounded listing of the shares, and whether the library is answered for.

use std::net::SocketAddr;

use axum::body::{Body, to_bytes};
use axum::extract::ConnectInfo;
use axum::http::{Method, Request, StatusCode};
use axum::response::Response;
use kantele::api::Operation;
use kantele::index::{Library, Store};
use kantele::server::{self, Server};
use kantele::service::{self, Indexing, Outcome, Pass, PassReport};
use kantele::upnp::client::Profiles;
use kantele::upnp::description::DeviceIdentity;
use tower::ServiceExt;

mod fixtures;
use fixtures::Tree;

/// Two albums under one label folder, a folder holding nothing playable, and a broken playlist.
fn a_library(name: &str) -> Tree {
    let tree = Tree::new(name);
    tree.album("Blue Note/Sierra Maestra", &["01.wav", "02.wav"], true);
    tree.album("Blue Note/Kremerata", &["01.wav"], false);
    tree.album("Autechre", &["01.wav"], false);
    tree.text(
        "Blue Note/Sierra Maestra/gone.m3u",
        "#EXTM3U\nnowhere.wav\n",
    );
    tree
}

fn serving(tree: &Tree, pass: Option<PassReport>) -> Server {
    let library = Library::scan(&tree.0).expect("scanning the test tree");
    let server = Server::new(
        library,
        kantele::browse::Settings::default(),
        DeviceIdentity {
            friendly_name: "Kantele Test".to_owned(),
            udn: "3d5d1cbe-8f2a-4d1e-9a9c-7c2f0a1b2c3d".to_owned(),
            icon: Default::default(),
        },
        Profiles::default(),
    );
    server.control.set_operation(Operation {
        store: None,
        config: kantele::config::Resolved::load(None, Some(tree.0.clone()))
            .expect("the layers resolve"),
    });
    server.control.indexing.replace(Indexing::of(&tree.0));
    if let Some(pass) = pass {
        server.passes.record(pass);
    }
    server
}

/// A device on this network, as the listener would name it.
async fn ask(server: &Server, mut request: Request<Body>) -> Response {
    request
        .extensions_mut()
        .insert(ConnectInfo(SocketAddr::from(([192, 168, 1, 20], 50_000))));
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

fn get_json(path: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
        // Every HTTP/1.1 request carries one, and the listing of the shares is refused without it.
        .header("Host", "192.0.2.42:8200")
        .header("Accept", "application/json")
        .body(Body::empty())
        .expect("a request")
}

async fn json(server: &Server, path: &str) -> serde_json::Value {
    let body = body_of(ask(server, get_json(path)).await).await;
    serde_json::from_str(&body).unwrap_or_else(|error| panic!("json from {path}: {error}\n{body}"))
}

#[tokio::test]
async fn the_tree_is_read_one_level_at_a_time_with_what_each_folder_holds() {
    let tree = a_library("folder-tree");
    let server = serving(&tree, None);

    let top = json(&server, "/api/folders").await;
    let rows = top["folders"].as_array().expect("a list of folders");
    let named = |rows: &serde_json::Value, name: &str| -> serde_json::Value {
        rows.as_array()
            .expect("a list")
            .iter()
            .find(|row| row["name"] == name)
            .unwrap_or_else(|| panic!("{name} is listed"))
            .clone()
    };
    assert_eq!(rows.len(), 2, "the top of the library holds two folders");
    assert_eq!(top["here"]["tracks"], 4, "and four tracks below it in all");

    let label = named(&top["folders"], "Blue Note");
    assert_eq!(
        label["tracks"], 3,
        "a folder counts everything below it, not only what is directly in it"
    );
    assert_eq!(label["folders"], 2, "and says that it opens");
    assert_eq!(
        label["problems"], 2,
        "the playlist naming a file that is gone is refused twice, and both fall in its folder"
    );
    // A row says what it holds. Everything wrong with it is listed apart, all of it, since a row
    // naming only its worst trouble hides the rest and an owner cannot act on what is not shown.
    assert_eq!(label["says"], "3 tracks, no album");
    let listed: Vec<&str> = label["issues"]
        .as_array()
        .expect("a list of issues")
        .iter()
        .map(|issue| issue["label"].as_str().expect("a label"))
        .collect();
    assert_eq!(
        listed,
        ["Broken playlist link", "Empty playlist", "No cover art"],
        "a cover nobody has is not a refusal, and a listener still misses it"
    );

    let inside = json(&server, "/api/folders?under=Blue%20Note").await;
    assert_eq!(
        inside["folders"].as_array().expect("a list").len(),
        2,
        "one level down is the two albums and nothing from elsewhere in the tree"
    );
    assert_eq!(named(&inside["folders"], "Kremerata")["tracks"], 1);

    let searched = json(&server, "/api/folders?q=krem").await;
    let found = searched["folders"].as_array().expect("a list");
    assert_eq!(found.len(), 1, "a search reaches the whole tree");
    assert_eq!(found[0]["path"], "Blue Note/Kremerata");

    let nowhere = ask(&server, get_json("/api/folders?under=Nowhere")).await;
    assert_eq!(nowhere.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_folder_the_last_pass_read_again_is_marked_and_can_be_filtered_to() {
    let tree = a_library("folder-changed");
    let mut store = Some(Store::in_memory().expect("a store"));
    let indexing = Indexing::of(&tree.0);
    service::index(&indexing, &mut store, Pass::Whole).expect("the first pass");
    let within = service::index(
        &indexing,
        &mut store,
        Pass::Within(kantele::index::scan::Scope::of([std::path::PathBuf::from(
            "Autechre",
        )])),
    )
    .expect("a pass over one folder");
    let server = serving(&tree, Some(within.pass));

    let top = json(&server, "/api/folders").await;
    let marked: Vec<&str> = top["folders"]
        .as_array()
        .expect("a list")
        .iter()
        .filter(|row| row["changed"] == true)
        .map(|row| row["name"].as_str().expect("a name"))
        .collect();
    assert_eq!(marked, ["Autechre"]);

    let only = json(&server, "/api/folders?changed=true").await;
    assert_eq!(only["folders"].as_array().expect("a list").len(), 1);

    let whole = serving(&tree, None);
    let all = json(&whole, "/api/folders").await;
    assert!(
        all["folders"]
            .as_array()
            .expect("a list")
            .iter()
            .all(|row| row["changed"] == false),
        "with no pass to go on, nothing is marked out"
    );
}

#[tokio::test]
async fn one_folder_is_read_again_on_its_own_and_a_folder_nobody_has_is_refused() {
    let tree = a_library("rescan-one");
    let server = serving(&tree, None);
    let post = |uri: &str| {
        Request::builder()
            .method(Method::POST)
            .uri(uri)
            // Every HTTP/1.1 request carries one, and a pass is refused without it.
            .header("Host", "192.0.2.42:8200")
            .header("Accept", "application/json")
            .body(Body::empty())
            .expect("a request")
    };

    let one = ask(&server, post("/api/rescan?folder=Autechre")).await;
    assert_eq!(one.status(), StatusCode::ACCEPTED);
    let says = body_of(one).await;
    assert!(says.contains("Autechre"), "the answer names it: {says}");
    assert_eq!(
        server.passes.taken(),
        Some(Pass::Within(kantele::index::scan::Scope::of([
            std::path::PathBuf::from("Autechre")
        ]))),
        "and the pass waiting is over that folder rather than the library"
    );

    let both = ask(&server, post("/api/rescan?folder=Autechre")).await;
    assert_eq!(both.status(), StatusCode::ACCEPTED);
    let widened = ask(&server, post("/api/rescan")).await;
    assert_eq!(
        widened.status(),
        StatusCode::ACCEPTED,
        "asking for everything while one folder waits is a wider pass, not a repeat"
    );
    assert_eq!(server.passes.taken(), Some(Pass::Whole));
    assert_eq!(
        server.passes.taken(),
        None,
        "and nothing waits behind it: a second nudge would otherwise walk the whole library"
    );

    let nowhere = ask(&server, post("/api/rescan?folder=Nowhere")).await;
    assert_eq!(nowhere.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_pass_underway_is_answered_without_counting_the_whole_library() {
    let tree = a_library("progress");
    let server = serving(&tree, None);

    let idle = json(&server, "/api/progress").await;
    assert_eq!(idle["phase"], "idle");
    assert_eq!(idle["read"], 0);
    assert!(idle["began"].is_null(), "nothing is underway");

    let mut store = None;
    let indexing = server.control.indexing.now();
    service::index(&indexing, &mut store, Pass::Whole).expect("a pass");
    let after = json(&server, "/api/progress").await;
    assert_eq!(after["phase"], "idle", "and it is idle again when it ends");
    assert_eq!(
        after["found"], 4,
        "with what the walk found still readable after it"
    );
    assert_eq!(after["read"], 4);
    assert!(after["folders"].as_u64().expect("a count") >= 3);
}

#[tokio::test]
async fn a_background_task_that_stopped_is_named_on_the_page() {
    let tree = a_library("stopped-task");
    let server = serving(&tree, None);

    assert!(
        json(&server, "/api/status").await["stopped"].is_null(),
        "nothing has stopped, so nothing is said about it"
    );

    server.control.tasks.spawn("library sweeps", async {
        panic!("the pass loop fell over")
    });
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }

    assert_eq!(
        json(&server, "/api/status").await["stopped"][0],
        "library sweeps",
        "the sweeps stopping is otherwise visible only in what /api/rescan answers"
    );
}

#[tokio::test]
async fn the_status_says_whether_the_library_is_answered_for_and_since_when() {
    let tree = a_library("failing");
    let server = serving(&tree, None);

    let serving_well = json(&server, "/api/status").await;
    assert!(
        serving_well["failing"].is_null(),
        "nothing has failed yet, so nothing is said about it"
    );

    server.passes.record(PassReport {
        ended: 1_700_000_000,
        outcome: Outcome::Kept {
            why: "the music folder read as empty".to_owned(),
        },
        ..PassReport::failed(String::new())
    });
    let failing = json(&server, "/api/status").await;
    assert_eq!(failing["last_pass"]["outcome"], "kept");
    assert_eq!(failing["failing"]["since"], 1_700_000_000_i64);
    assert!(
        failing["failing"]["why"]
            .as_str()
            .expect("a reason")
            .contains("read as empty"),
        "and the page is told why rather than being left to guess"
    );

    server.passes.record(PassReport {
        ended: 1_700_000_900,
        outcome: Outcome::Failed {
            why: "the pass panicked".to_owned(),
        },
        ..PassReport::failed(String::new())
    });
    assert_eq!(
        json(&server, "/api/status").await["failing"]["since"],
        1_700_000_000_i64,
        "a second failure is the same run of them, and the page shows when it began"
    );

    server.passes.record(PassReport {
        ended: 1_700_001_000,
        outcome: Outcome::Published,
        ..PassReport::failed(String::new())
    });
    assert!(
        json(&server, "/api/status").await["failing"].is_null(),
        "a pass that answered for the library ends the run"
    );
}

#[tokio::test]
async fn the_folder_chooser_offers_the_shares_and_refuses_the_rest_of_the_disk() {
    let tree = a_library("shares");
    let server = serving(&tree, None);

    let shares = json(&server, "/api/shares").await;
    let entries = shares["entries"].as_array().expect("a list");
    let served = tree.0.canonicalize().expect("the test tree is there");
    assert!(
        entries
            .iter()
            .any(|entry| entry["path"] == served.display().to_string()),
        "the folder being served is one an owner can open: {entries:?}"
    );
    assert!(
        shares["above"].is_null(),
        "a share is as far up as this goes"
    );

    let under = json(&server, &format!("/api/shares?under={}", served.display())).await;
    let names: Vec<&str> = under["entries"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|entry| entry["name"].as_str().expect("a name"))
        .collect();
    assert_eq!(names, ["Autechre", "Blue Note"]);
    assert!(
        under["entries"][0]["folder"] == true,
        "and the chooser is offered folders"
    );

    // A folder that is there and is outside every share, on either platform.
    let outside = if cfg!(windows) { "C:/Windows" } else { "/etc" };
    let elsewhere = ask(&server, get_json(&format!("/api/shares?under={outside}"))).await;
    assert_eq!(
        elsewhere.status(),
        StatusCode::FORBIDDEN,
        "the page has no password, so it never reads the filesystem"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn a_folder_this_server_may_not_read_says_so_rather_than_looking_absent() {
    use std::os::unix::fs::PermissionsExt;

    let tree = a_library("denied");
    let server = serving(&tree, None);
    let served = tree.0.canonicalize().expect("the test tree is there");
    let shut = served.join("Autechre");
    std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o000)).expect("shutting it");

    let answer = ask(
        &server,
        get_json(&format!("/api/shares?under={}", shut.display())),
    )
    .await;
    let status = answer.status();
    let said = String::from_utf8(
        to_bytes(answer.into_body(), usize::MAX)
            .await
            .expect("a body")
            .to_vec(),
    )
    .expect("utf-8");
    std::fs::set_permissions(&shut, std::fs::Permissions::from_mode(0o755)).expect("opening it");

    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a folder that is there and shut is not a folder that is missing"
    );
    assert!(
        said.contains("may not read it"),
        "and it says what to do about it: {said}"
    );
}

#[tokio::test]
async fn the_count_behind_a_problem_is_what_the_folder_really_holds() {
    let tree = Tree::new("problems-past-the-hundred");
    tree.album("Blue Note/Sierra Maestra", &["01.wav"], false);
    let broken = 150;
    let mut list = String::from("#EXTM3U\n");
    for at in 0..broken {
        list.push_str(&format!("gone-{at}.wav\n"));
    }
    tree.text("Blue Note/Sierra Maestra/set.m3u", &list);
    let server = serving(&tree, None);

    let answered = json(
        &server,
        "/api/problems?folder=Blue%20Note&cause=missing-entry",
    )
    .await;
    assert_eq!(
        answered["total"], broken,
        "the page says how many there are, not how many the record kept"
    );
    assert_eq!(
        answered["shown"].as_array().expect("a list").len(),
        100,
        "and names the hundred it kept"
    );
}

#[tokio::test]
async fn the_root_menu_is_the_one_the_server_builds_rather_than_a_copy_of_the_rule() {
    let tree = a_library("root-menu");
    let server = serving(&tree, None);

    let root = json(&server, "/api/menu").await;
    assert_eq!(root["at"], "f");
    assert_eq!(root["kind"], "root");
    let entries = root["entries"].as_array().expect("a list of entries");
    let titled = |title: &str| {
        entries
            .iter()
            .find(|entry| entry["title"] == title)
            .cloned()
    };

    assert!(
        titled("4 items").is_some(),
        "every item leads: {entries:#?}"
    );
    assert!(
        titled("[folder view]").is_some(),
        "and the folder view closes"
    );
    for entry in entries {
        assert!(
            entry["at"].as_str().is_some_and(|at| !at.is_empty()),
            "every entry says what to browse: {entry:#?}"
        );
        assert!(
            entry["children"]
                .as_u64()
                .is_some_and(|children| children > 1)
                || entry["chosen"] == false,
            "an axis offering one value is no menu: {entry:#?}"
        );
    }
}

#[tokio::test]
async fn a_position_nothing_names_is_no_menu() {
    let tree = a_library("no-menu");
    let server = serving(&tree, None);

    let response = ask(&server, get_json("/api/menu?at=not-a-position")).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_file_says_what_its_tags_are_and_where_they_put_it_in_the_menus() {
    let tree = a_library("track-detail");
    let server = serving(&tree, None);

    let top = json(&server, "/api/folders").await;
    assert!(top["here"]["tracks"].as_u64().is_some_and(|n| n > 0));

    let path = "Autechre/01.wav";
    let found = json(&server, &format!("/api/track?path={path}")).await;
    assert_eq!(found["path"], path);
    let labels: Vec<String> = found["tags"]
        .as_array()
        .expect("the tag rows")
        .iter()
        .map(|tag| tag["label"].as_str().expect("a label").to_owned())
        .collect();
    assert_eq!(
        labels.first().map(String::as_str),
        Some("Title"),
        "{labels:?}"
    );
    assert!(labels.contains(&"Artist".to_owned()), "{labels:?}");
    assert!(labels.contains(&"Cover art".to_owned()), "{labels:?}");
    // The fixture writes bare wavs, so every tag row is there and every one of them is empty.
    for tag in found["tags"].as_array().expect("the tag rows") {
        assert!(
            tag.get("value").is_none(),
            "an untagged file carries no value: {tag:#?}"
        );
    }
    assert!(
        found["format"]
            .as_str()
            .is_some_and(|said| said.contains("WAV")),
        "{found:#?}"
    );
    for place in found["places"].as_array().expect("the places") {
        assert!(
            place["at"].as_str().is_some_and(|at| at.starts_with("f-")),
            "a place names the position that lists it: {place:#?}"
        );
    }
}

#[tokio::test]
async fn a_path_the_library_does_not_hold_is_no_track() {
    let tree = a_library("no-track");
    let server = serving(&tree, None);

    let response = ask(&server, get_json("/api/track?path=nowhere.flac")).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_cover_beside_the_files_is_reachable_and_one_that_is_absent_says_nothing() {
    let tree = a_library("cover-beside");
    let server = serving(&tree, None);

    // The fixture writes a cover.jpg beside this album and none beside the other.
    let with = json(
        &server,
        "/api/track?path=Blue%20Note/Sierra%20Maestra/01.wav",
    )
    .await;
    let at = with["artwork"]
        .as_str()
        .unwrap_or_else(|| panic!("a cover in the folder is offered: {with:#?}"));
    let cover = ask(&server, get_json(&format!("/art/{at}"))).await;
    assert_eq!(cover.status(), StatusCode::OK, "and the wire serves it");

    let without = json(&server, "/api/track?path=Autechre/01.wav").await;
    assert!(
        without.get("artwork").is_none(),
        "a file with no cover offers none: {without:#?}"
    );
}

#[tokio::test]
async fn a_folder_lists_what_is_directly_in_it_and_a_row_offers_a_cover() {
    let tree = a_library("folder-files");
    let server = serving(&tree, None);

    let files = json(&server, "/api/files?folder=Blue%20Note/Sierra%20Maestra").await;
    let listed = files["files"].as_array().expect("the files here");
    assert_eq!(listed.len(), 2, "the two tracks of that album: {listed:#?}");
    assert!(listed.iter().all(|file| file["path"].as_str().is_some()));
    assert_eq!(files["more"], false);

    // What the files belong to comes with them, because a folder tree cannot show it. These
    // fixtures carry no tags, so nothing names an album and the list is empty rather than absent.
    assert_eq!(
        files["albums"].as_array().expect("the albums here").len(),
        0
    );

    // A folder holding only folders has none of its own.
    let top = json(&server, "/api/files?folder=Blue%20Note").await;
    assert_eq!(top["files"].as_array().expect("a list").len(), 0);
    assert_eq!(top["albums"].as_array().expect("a list").len(), 0);

    // The tree's own rows carry the cover, so a listing draws one without asking per row.
    let rows = json(&server, "/api/folders?under=Blue%20Note").await;
    let named = rows["folders"]
        .as_array()
        .expect("a list")
        .iter()
        .find(|row| row["name"] == "Sierra Maestra")
        .expect("the album folder");
    assert!(named["artwork"].as_str().is_some(), "{named:#?}");

    let missing = ask(&server, get_json("/api/files?folder=nowhere")).await;
    assert_eq!(missing.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn the_end_of_the_log_is_read_on_the_page_rather_than_over_ssh() {
    let tree = a_library("log-tail");
    let server = serving(&tree, None);
    let state = tree.0.join("state");
    std::fs::create_dir_all(&state).expect("a state folder");
    let mut operation = (*server.control.operation()).clone();
    operation.config.config.state_dir = Some(state.clone());
    server.control.set_operation(operation);

    let missing = ask(&server, get_json("/api/log")).await;
    assert_eq!(
        missing.status(),
        StatusCode::NOT_FOUND,
        "a terminal run has no file, and the page is told so"
    );

    std::fs::write(kantele::log::path(&state), "one\ntwo\nthree\nfour\n").expect("a log");
    let plain = ask(
        &server,
        Request::builder()
            .uri("/api/log?lines=2")
            .body(Body::empty())
            .expect("a request"),
    )
    .await;
    assert_eq!(plain.status(), StatusCode::OK);
    assert_eq!(body_of(plain).await, "three\nfour\n");

    let tail = json(&server, "/api/log?lines=3").await;
    assert_eq!(tail["lines"], serde_json::json!(["two", "three", "four"]));
    assert_eq!(
        tail["path"],
        kantele::log::path(&state).display().to_string()
    );

    let whole = json(&server, "/api/log").await;
    assert_eq!(whole["lines"].as_array().expect("lines").len(), 4);
}
