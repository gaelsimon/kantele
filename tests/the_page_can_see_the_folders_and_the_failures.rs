//! The routes the configuration page needs: the folder tree, one folder read again, how far a
//! pass has got, the bounded listing of the shares, and whether the library is answered for.

use axum::body::{Body, to_bytes};
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

fn get_json(path: &str) -> Request<Body> {
    Request::builder()
        .uri(path)
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
        ["Playlist link broken", "Playlist empty", "No cover art"],
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
        Pass::Within(kantele::index::scan::Scope::of([std::path::PathBuf::from(
            "Autechre"
        )])),
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
    assert_eq!(server.passes.taken(), Pass::Whole);

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

    let elsewhere = ask(&server, get_json("/api/shares?under=/etc")).await;
    assert_eq!(
        elsewhere.status(),
        StatusCode::FORBIDDEN,
        "the page has no password, so it never reads the filesystem"
    );
}
