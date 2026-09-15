//! The index layer must not name the browse layer.

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
