//! A setting written from the page is saved with the file's comments kept and applied at once.

use std::path::Path;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use kantele::api::Operation;
use kantele::config::{Config, Resolved};
use kantele::index::Library;
use kantele::server::{self, Server};
use kantele::upnp::client::Profiles;
use kantele::upnp::description::DeviceIdentity;
use tower::ServiceExt;

fn serving_from(file: &Path) -> Server {
    let read = Config::load(file).expect("the file parses");
    let server = Server::new(
        Library::build("Music".to_owned(), &[]),
        read.menu_settings(),
        DeviceIdentity {
            friendly_name: "Kantele Test".to_owned(),
            udn: "3d5d1cbe-8f2a-4d1e-9a9c-7c2f0a1b2c3d".to_owned(),
            icon: Default::default(),
        },
        Profiles::default(),
    );
    server.control.set_operation(Operation {
        store: None,
        config: Resolved::load(Some(file), Some("/music".into())).expect("the layers resolve"),
    });
    server
}

async fn put(server: &Server, body: &str, headers: &[(&str, &str)]) -> (StatusCode, String) {
    let mut request = Request::builder()
        .method("PUT")
        .uri("/api/config")
        .header("Content-Type", "application/json");
    // Every HTTP/1.1 request carries one, and a write is refused without it.
    if !headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("host"))
    {
        request = request.header("Host", "192.0.2.42:8200");
    }
    for (name, value) in headers {
        request = request.header(*name, *value);
    }
    let response = server::router(server)
        .oneshot(
            request
                .body(Body::from(body.to_owned()))
                .expect("a request"),
        )
        .await
        .expect("the router answers");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("a body");
    (status, String::from_utf8_lossy(&bytes).into_owned())
}

#[tokio::test]
async fn a_setting_is_written_with_the_comments_kept_and_applied_at_once() {
    let dir = std::env::temp_dir().join(format!("kantele-write-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder");
    let file = dir.join("kantele.toml");
    std::fs::write(
        &file,
        "# Mine.\nfriendly_name = \"Salon\"\n\n[menus]\n# Kept too.\nalbum_threshold = 24\n",
    )
    .expect("writing the file");
    let server = serving_from(&file);
    let device = &server.device;
    let before = device.system_update_id();

    let (status, body) = put(
        &server,
        r#"{"menus.album_threshold": 3, "menus.alpha_group": 50}"#,
        &[],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    let written = std::fs::read_to_string(&file).expect("the file is still there");
    assert!(written.contains("# Mine.") && written.contains("# Kept too."));
    assert!(written.contains("friendly_name = \"Salon\""));
    assert!(written.contains("album_threshold = 3") && written.contains("alpha_group = 50"));
    assert_eq!(device.served().view.settings.album_threshold, 3);
    assert_eq!(device.served().view.settings.alpha_group, Some(50));
    assert!(
        device.system_update_id() != before,
        "the menus changed, so every client is told to look again"
    );
    let source = kantele::api::describe::effective(&server.control.operation().config)
        .settings
        .iter()
        .find(|setting| setting.key == "menus.album_threshold")
        .map(|setting| setting.source.as_str())
        .expect("the key is answered for");
    assert_eq!(source, "file", "and the answer says the file now holds it");

    let (status, body) = put(&server, r#"{"clients": 1}"#, &[]).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a key the file alone holds: {body}"
    );
    let (status, body) = put(&server, r#"{"content_dir": "/elsewhere"}"#, &[]).await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "a key the command line holds: {body}"
    );
    assert!(
        body.contains("command line"),
        "and the answer names the layer that holds it: {body}"
    );
    let (status, _) = put(&server, r#"{"menus.album_threshold": "many"}"#, &[]).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "a value the file could not read back"
    );
    let (status, _) = put(
        &server,
        r#"{"menus.album_threshold": 5}"#,
        &[("Origin", "http://elsewhere.example")],
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "another site's page");
    // A page on a name its author points at this server's address sends an Origin that matches
    // the Host it sent, so comparing the two says yes.
    let (status, _) = put(
        &server,
        r#"{"menus.album_threshold": 5}"#,
        &[
            ("Host", "elsewhere.example:8200"),
            ("Origin", "http://elsewhere.example:8200"),
        ],
    )
    .await;
    assert_eq!(
        status,
        StatusCode::FORBIDDEN,
        "a name the world resolves is not this network's"
    );
    assert_eq!(
        device.served().view.settings.album_threshold,
        3,
        "and nothing refused was applied"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The three modes a change can reach the server by, each written and each answered for.
#[tokio::test]
async fn every_written_key_says_how_its_change_reaches_the_server() {
    let dir = std::env::temp_dir().join(format!("kantele-modes-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder");
    let file = dir.join("kantele.toml");
    std::fs::write(&file, "[menus]\nalbum_threshold = 24\n").expect("writing the file");
    let server = serving_from(&file);

    let json = [("Accept", "application/json")];
    let answered = |body: &str| -> serde_json::Value {
        serde_json::from_str(body).expect("a write answers json")
    };
    let (status, body) = put(&server, r#"{"menus.album_threshold": 30}"#, &json).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(answered(&body)["apply"], "immediate");
    assert_eq!(answered(&body)["needs_restart"], false);

    let (status, body) = put(&server, r#"{"scan.threads": 2}"#, &json).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(answered(&body)["apply"], "next-pass");
    assert_eq!(
        server.control.indexing.now().options.threads,
        2,
        "and the next pass runs under it rather than under the one the server started with"
    );

    let (status, body) = put(&server, r#"{"friendly_name": "Salon"}"#, &json).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(answered(&body)["apply"], "restart");
    assert_eq!(answered(&body)["needs_restart"], true);

    // One save of keys in two modes is answered by the stronger of them.
    let (status, body) = put(
        &server,
        r#"{"menus.album_threshold": 32, "friendly_name": "Cuisine"}"#,
        &json,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(answered(&body)["apply"], "restart");
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_list_is_written_and_read_back_as_a_list() {
    let dir = std::env::temp_dir().join(format!("kantele-lists-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder");
    let file = dir.join("kantele.toml");
    std::fs::write(&file, "[menus]\nalbum_threshold = 24\n").expect("writing the file");
    let server = serving_from(&file);

    let (status, body) = put(&server, r#"{"menus.axes": ["genre", "date"]}"#, &[]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        server.device.served().view.settings.axes,
        vec![kantele::browse::Facet::Genre, kantele::browse::Facet::Date],
        "a list reaches the menus the way a number does"
    );
    let written = std::fs::read_to_string(&file).expect("the file is still there");
    assert!(written.contains(r#"axes = ["genre", "date"]"#), "{written}");

    let (status, body) = put(&server, r#"{"menus.axes": ["genre", "genre"]}"#, &[]).await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "an axis named twice is a menu offered twice: {body}"
    );
    assert!(
        std::fs::read_to_string(&file)
            .expect("still there")
            .contains(r#"["genre", "date"]"#),
        "and the file is left as it was"
    );

    let (status, body) = put(&server, r#"{"capture_dir": null}"#, &[]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert!(
        !std::fs::read_to_string(&file)
            .expect("still there")
            .contains("capture_dir"),
        "nothing is how a page unsets a key"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The reason the indexing settings are shared rather than copied at startup.
#[tokio::test]
async fn a_menu_setting_written_is_the_one_the_next_pass_publishes() {
    let dir = std::env::temp_dir().join(format!("kantele-survives-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder");
    let file = dir.join("kantele.toml");
    std::fs::write(&file, "[menus]\nalbum_threshold = 24\n").expect("writing the file");
    let server = serving_from(&file);

    let (status, body) = put(&server, r#"{"menus.album_threshold": 3}"#, &[]).await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        server.control.indexing.now().menus.album_threshold,
        3,
        "a pass that ran now would rebuild the view with the setting that was just written"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_server_started_without_a_file_has_nothing_to_write_and_says_so() {
    let server = Server::new(
        Library::build("Music".to_owned(), &[]),
        kantele::browse::Settings::default(),
        DeviceIdentity {
            friendly_name: "Kantele Test".to_owned(),
            udn: "3d5d1cbe-8f2a-4d1e-9a9c-7c2f0a1b2c3d".to_owned(),
            icon: Default::default(),
        },
        Profiles::default(),
    );
    server.control.set_operation(Operation {
        config: Resolved::load(None, Some("/music".into())).expect("the layers resolve"),
        ..Operation::default()
    });
    let (status, body) = put(&server, r#"{"menus.album_threshold": 3}"#, &[]).await;
    assert_eq!(status, StatusCode::CONFLICT, "{body}");
    assert!(body.contains("--config"), "{body}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_saves_at_once_both_land_and_the_menus_agree_with_the_file() {
    let dir = std::env::temp_dir().join(format!("kantele-write-race-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder");
    let file = dir.join("kantele.toml");
    std::fs::write(&file, "[menus]\nalbum_threshold = 24\n").expect("writing the file");
    let server = serving_from(&file);

    let first = tokio::spawn({
        let server = server.clone();
        async move { put(&server, r#"{"menus.album_threshold": 7}"#, &[]).await }
    });
    let second = tokio::spawn({
        let server = server.clone();
        async move { put(&server, r#"{"menus.alpha_group": 70}"#, &[]).await }
    });
    let (first, second) = (
        first.await.expect("the first write finishes"),
        second.await.expect("the second write finishes"),
    );
    assert_eq!(first.0, StatusCode::OK, "{}", first.1);
    assert_eq!(second.0, StatusCode::OK, "{}", second.1);

    let written = std::fs::read_to_string(&file).expect("the file is still there");
    assert!(
        written.contains("album_threshold = 7") && written.contains("alpha_group = 70"),
        "one save overwrote the other: {written}"
    );
    assert_eq!(server.device.served().view.settings.album_threshold, 7);
    assert_eq!(server.device.served().view.settings.alpha_group, Some(70));
    let _ = std::fs::remove_dir_all(&dir);
}

#[tokio::test]
async fn a_menu_setting_saved_beside_one_for_the_next_check_still_applies_at_once() {
    let dir = std::env::temp_dir().join(format!("kantele-mixed-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("a folder");
    let file = dir.join("kantele.toml");
    std::fs::write(&file, "[menus]\nalbum_threshold = 24\n").expect("writing the file");
    let server = serving_from(&file);

    let (status, body) = put(
        &server,
        r#"{"menus.album_threshold": 3, "scan.threads": 2}"#,
        &[("Accept", "application/json")],
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{body}");
    assert_eq!(
        server.device.served().view.settings.album_threshold,
        3,
        "the menus change now, and waiting for a check that finds nothing new is waiting for ever"
    );
    assert!(
        body.contains("the menus were applied"),
        "and the answer says so rather than only the slower key's news: {body}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
