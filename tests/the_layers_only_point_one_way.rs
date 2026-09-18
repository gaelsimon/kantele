//! The index layer must not name the browse layer, and the server must not name the page's API.

use std::path::Path;

fn sources(dir: &str) -> Vec<(String, String)> {
    fn walk(at: &Path, into: &mut Vec<(String, String)>) {
        for entry in std::fs::read_dir(at).expect("reading the source tree") {
            let path = entry.expect("a directory entry").path();
            if path.is_dir() {
                walk(&path, into);
            } else if path.extension().is_some_and(|kind| kind == "rs") {
                into.push((
                    path.display().to_string(),
                    std::fs::read_to_string(&path).expect("reading a source file"),
                ));
            }
        }
    }
    let mut found = Vec::new();
    walk(Path::new(dir), &mut found);
    assert!(!found.is_empty(), "no sources under {dir}");
    found
}

#[test]
fn the_index_never_names_the_browse_layer() {
    let offenders: Vec<String> = sources("src/index")
        .into_iter()
        .filter(|(_, body)| body.contains("crate::browse"))
        .map(|(path, _)| path)
        .collect();
    assert!(
        offenders.is_empty(),
        "the index derives and does not present; these name the browse layer: {offenders:?}"
    );
}

/// The index mints identifiers and the wire publishes them. Both name `crate::object`, which holds
/// the identifier and the class names, and neither has to name the other.
#[test]
fn the_index_never_names_the_wire() {
    let offenders: Vec<String> = sources("src/index")
        .into_iter()
        .filter(|(_, body)| body.contains("crate::upnp"))
        .map(|(path, _)| path)
        .collect();
    assert!(
        offenders.is_empty(),
        "the index derives and does not transport; these name the wire: {offenders:?}"
    );
}

#[test]
fn the_shared_vocabulary_is_named_by_both_sides() {
    for layer in ["src/index", "src/upnp"] {
        assert!(
            sources(layer)
                .iter()
                .any(|(_, body)| body.contains("crate::object")),
            "{layer} names nothing in the shared module, so the cut it was made for is gone"
        );
    }
}

#[test]
fn the_browse_layer_is_allowed_to_name_the_index() {
    assert!(
        sources("src/browse")
            .iter()
            .any(|(_, body)| body.contains("crate::index")),
        "the direction this asserts would be vacuous if nothing pointed that way"
    );
}

/// The API is a client of the server. `server.rs` is where the wire router and the page router
/// meet, and `main.rs` hands it what a start knows; nothing else may reach for it.
#[test]
fn only_the_assembled_server_names_the_api() {
    let sources = sources("src");
    let offenders: Vec<&String> = sources
        .iter()
        .filter(|(path, _)| !path.starts_with("src/api/"))
        .filter(|(path, _)| path != "src/server.rs" && path != "src/main.rs")
        .filter(|(_, body)| body.contains("crate::api"))
        .map(|(path, _)| path)
        .collect();
    assert!(
        offenders.is_empty(),
        "the API reads the server and not the other way round; these name it: {offenders:?}"
    );
    assert!(
        sources
            .iter()
            .any(|(path, body)| path == "src/server.rs" && body.contains("crate::api")),
        "the direction this asserts would be vacuous if the server did not mount the API"
    );
}

/// The API reads the index at its root, where what the index publishes is chosen; a submodule is
/// how the index is built, and the page has no business there.
#[test]
fn the_api_never_reaches_into_the_index() {
    let listing = std::fs::read_to_string("src/index/mod.rs").expect("the index's module file");
    let modules: Vec<&str> = listing
        .lines()
        .filter_map(|line| line.strip_prefix("pub mod ")?.strip_suffix(';'))
        .collect();
    assert!(
        !modules.is_empty(),
        "the index has no submodules to keep out of"
    );
    let offenders: Vec<String> = sources("src/api")
        .into_iter()
        .filter(|(_, body)| {
            modules
                .iter()
                .any(|module| body.contains(&format!("crate::index::{module}::")))
        })
        .map(|(path, _)| path)
        .collect();
    assert!(
        offenders.is_empty(),
        "these name how the index is built rather than what it publishes: {offenders:?}"
    );
}
