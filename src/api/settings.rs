//! Reading the effective configuration, and writing the keys the page may write.

use std::path::Path;

use axum::extract::State;
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Serialize;
use tokio::io::AsyncWriteExt;

use crate::config::Apply;
use crate::service::{Asked, Pass};

use super::describe::{self, Effective, Setting};
use super::{Control, Operation, Peer, Shared, answer, refused};

pub(super) async fn configuration(State(control): State<Shared>, headers: HeaderMap) -> Response {
    let effective = describe::effective(&control.operation().config);
    answer(&headers, &effective, || flatten_config(&effective))
}

/// What a write answered with.
#[derive(Debug, Serialize)]
struct Written {
    written: Vec<String>,
    /// The strongest mode among the keys written, which is what the whole save came to.
    apply: Apply,
    needs_restart: bool,
    says: String,
}

pub(super) async fn write_configuration(
    State(control): State<Shared>,
    peer: Peer,
    headers: HeaderMap,
    body: String,
) -> Response {
    if let Some(refused) = refused(
        peer,
        &headers,
        "settings written",
        "settings may only be written from this server's own page\n",
    ) {
        return refused;
    }
    match write_settings(&control, &body).await {
        Ok(written) => answer(&headers, &written, || {
            vec![("written".to_owned(), written.written.join(" "))]
        }),
        Err((status, why)) => {
            tracing::warn!(%why, "settings not written");
            (status, format!("{why}\n")).into_response()
        }
    }
}

/// The write itself: only writable keys, through the file, read back, then applied.
async fn write_settings(control: &Control, body: &str) -> Result<Written, (StatusCode, String)> {
    let _one_at_a_time = control.settings_writes.lock().await;
    let operation = control.operation();
    let refuse = |status: StatusCode, why: String| (status, why);
    let changes: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(body).map_err(|error| {
            refuse(
                StatusCode::BAD_REQUEST,
                format!("the body is not a JSON object of settings: {error}"),
            )
        })?;
    let (mode, menus_now) = writable(&describe::effective(&operation.config), &changes)?;
    let path = operation.config.file_path().map(Path::to_path_buf).ok_or_else(|| {
        refuse(StatusCode::CONFLICT, "no configuration file was read at startup, so there is nothing to write: start with --config".to_owned())
    })?;
    let text = tokio::fs::read_to_string(&path).await.map_err(|error| {
        refuse(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("reading {}: {error}", path.display()),
        )
    })?;
    let rewritten = crate::config::rewrite(&text, &changes)
        .map_err(|error| refuse(StatusCode::BAD_REQUEST, format!("refused: {error:#}")))?;
    // The folders are checked against the disk before the file is replaced, or a name nothing
    // answers to would be saved and the library would go with it.
    let folders = match changes.contains_key("content_dir") {
        false => None,
        true => {
            let named = crate::config::Config::parse(&rewritten)
                .map_err(|error| refuse(StatusCode::BAD_REQUEST, format!("refused: {error:#}")))?;
            Some(
                crate::service::roots_of(&named).map_err(|error| {
                    refuse(StatusCode::BAD_REQUEST, format!("refused: {error:#}"))
                })?,
            )
        }
    };
    // Named for this process: two servers sharing a settings file must not stage over each other.
    let staged = path.with_extension(format!("toml.writing.{}", std::process::id()));
    let replaced = async {
        let mut file = tokio::fs::File::create(&staged).await?;
        file.write_all(rewritten.as_bytes()).await?;
        // Flushed here rather than left to `sync_all`, which drops the error the flush returns.
        file.flush().await?;
        // Before the rename, not after: a rename the disk keeps without the bytes behind it
        // leaves an empty file, and a settings file naming no music folder is a server with
        // no library until somebody chooses one again on the page.
        file.sync_all().await?;
        drop(file);
        tokio::fs::rename(&staged, &path).await
    };
    if let Err(error) = replaced.await {
        let _ = tokio::fs::remove_file(&staged).await;
        return Err(refuse(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("writing {}: {error}", path.display()),
        ));
    }
    let reloaded = operation.config.reload().map_err(|error| {
        refuse(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("reading the file back: {error:#}"),
        )
    })?;
    let settings = reloaded.config.menu_settings();
    // Every pass from here runs under what the file now says, whatever the mode does besides. The
    // folders are the ones checked above, so a write that does not name them resolves nothing.
    let held = control.indexing.now();
    control.indexing.replace(crate::service::Indexing {
        roots: folders.unwrap_or_else(|| held.roots.clone()),
        options: reloaded.config.scan_options(),
        menus: settings.clone(),
        underway: held.underway.clone(),
    });
    control.set_operation(Operation {
        store: operation.store.clone(),
        config: reloaded,
    });
    let written: Vec<String> = changes.keys().cloned().collect();
    // The cover a row remembers was chosen under the old preference, so no row can be trusted.
    let pass = if changes.contains_key("scan.cover_art") {
        Pass::Reread
    } else {
        Pass::Whole
    };
    let says = applied(control, mode, pass, menus_now, &path, settings).await;
    tracing::info!(keys = ?written, mode = mode.as_str(), "settings written");
    Ok(Written {
        written,
        apply: mode,
        needs_restart: mode == Apply::Restart,
        says,
    })
}

/// Every key the page may write now, the strongest mode among them and whether one of them applies
/// at once, or the refusal.
fn writable(
    effective: &Effective,
    changes: &serde_json::Map<String, serde_json::Value>,
) -> Result<(Apply, bool), (StatusCode, String)> {
    let mut mode = Apply::Never;
    let mut immediate = false;
    for key in changes.keys() {
        let Some(setting) = effective.settings.iter().find(|setting| setting.key == key) else {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("{key} is not a setting this server has"),
            ));
        };
        if setting.apply == Apply::Never {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("{key} is not written from this page"),
            ));
        }
        if !setting.writable {
            let held = match setting.source.names() {
                Some(named) => format!("{} {named}", setting.source.as_str()),
                None => setting.source.as_str().to_owned(),
            };
            return Err((
                StatusCode::CONFLICT,
                format!("{key} is held by the {held} and the file cannot outrank it"),
            ));
        }
        mode = mode.max(setting.apply);
        immediate |= setting.apply == Apply::Immediate;
    }
    Ok((mode, immediate))
}

/// Carries out what the mode asks for, and says what happened in the owner's words.
async fn applied(
    control: &Control,
    mode: Apply,
    pass: Pass,
    menus_now: bool,
    path: &Path,
    settings: crate::browse::Settings,
) -> String {
    let file = path.display();
    // A pass that finds nothing new publishes nothing, so a menu key saved beside a slower one
    // would wait for a change in the library.
    let menus_first = menus_now && mode != Apply::Immediate;
    if menus_first {
        control.device.apply_settings(settings.clone()).await;
    }
    let said = match mode {
        Apply::Immediate => {
            let id = control.device.apply_settings(settings).await;
            tracing::info!(system_update_id = id, "the menus were rebuilt");
            format!("saved to {file} and applied")
        }
        Apply::Reread => match control.passes.request(pass) {
            Asked::NobodyIsListening => {
                // No pass will carry it, so what the menus can take now they take now.
                if !menus_now {
                    control.device.apply_settings(settings).await;
                }
                format!(
                    "saved to {file}, but nothing in this process is watching the library, so it \
                     cannot be read again"
                )
            }
            _ => format!("saved to {file}; the library is being read again"),
        },
        Apply::NextPass => format!("saved to {file} and used at the next check"),
        Apply::Restart => format!("saved to {file}; it takes effect when the server starts again"),
        // Only an empty save gets here, and the file was written back as it was.
        Apply::Never => format!("saved to {file}"),
    };
    match menus_first {
        true => format!("{said}; the menus were applied now"),
        false => said,
    }
}

/// The configuration as lines, each carrying the layer its value came from.
fn flatten_config(effective: &Effective) -> Vec<(String, String)> {
    let mut lines = Vec::with_capacity(effective.settings.len() + 1);
    lines.push((
        "file".to_owned(),
        effective
            .file
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "none".to_owned()),
    ));
    for setting in &effective.settings {
        let Setting {
            key,
            value,
            source,
            label,
            apply,
            writable,
            ..
        } = setting;
        let from = match source.names() {
            Some(named) => format!("{} {named}", source.as_str()),
            None => source.as_str().to_owned(),
        };
        lines.push((
            (*key).to_owned(),
            format!(
                "{value}\t{from}\t{label}\t{}",
                if *writable {
                    apply.as_str()
                } else {
                    "read only"
                }
            ),
        ));
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_setting_names_the_layer_it_came_from() {
        let resolved =
            crate::config::Resolved::load(None, Some("/music".into())).expect("the layers resolve");
        let effective = describe::effective(&resolved);
        let lines = flatten_config(&effective);
        assert_eq!(lines[0], ("file".to_owned(), "none".to_owned()));
        assert!(
            lines.len() > effective.settings.len(),
            "the file it read is a line of its own"
        );
        assert!(
            effective
                .settings
                .iter()
                .any(|setting| setting.key == "content_dir"),
            "the one setting with no default is named"
        );
    }
}
