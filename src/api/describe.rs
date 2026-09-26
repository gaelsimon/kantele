//! Every setting in force, described for a page and for a shell: its label, its layer, its cost.

use std::path::{Path, PathBuf};

use serde::Serialize;

use crate::config::{Apply, ContentDir, DEFAULT_PORT, Resolved, Source};
use crate::index::{Prefer, SKIPPED_FOLDERS};

/// What a page shows beside the field.
/// The same cost as a badge wears it: a label, never a sentence.
pub fn tag(apply: Apply) -> &'static str {
    match apply {
        Apply::Never => "read only",
        Apply::Immediate => "applies at once",
        Apply::NextPass => "next check",
        Apply::Reread => "reads the library again",
        Apply::Restart => "needs a restart",
    }
}

pub fn says(apply: Apply) -> &'static str {
    match apply {
        Apply::Never => "You cannot change this setting on this page.",
        Apply::Immediate => "This change applies now.",
        Apply::NextPass => "This change applies at the next check.",
        Apply::Reread => "The server reads the library again.",
        Apply::Restart => "You must restart the server.",
    }
}

/// One value a setting may take, where the set of them is closed.
#[derive(Clone, Debug, Serialize)]
pub struct Choice {
    pub value: String,
    pub label: &'static str,
}

/// One setting in force.
#[derive(Clone, Debug, Serialize)]
pub struct Setting {
    /// The key as a file writes it.
    pub key: &'static str,
    /// What a page shows beside the value. A label.
    pub label: &'static str,
    pub value: String,
    /// The value in the shape the file writes it. The rendering in `value` is for a reader and
    /// cannot be written back.
    pub as_written: serde_json::Value,
    pub source: Source,
    pub apply: Apply,
    /// The mode in words.
    pub says: &'static str,
    /// The badge beside a field, where a sentence does not fit.
    pub tag: &'static str,
    /// Whether the page may write it now, which the layer holding the value decides.
    pub writable: bool,
    /// Every value this setting may take, where they are countable; empty where they are not.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<Choice>,
    /// What the server does with it that the value does not say, shown under the field.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// Every setting in force, with the file that was read, if one was.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Effective {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,
    pub settings: Vec<Setting>,
}

/// Where each value in force came from, per key.
pub fn effective(resolved: &Resolved) -> Effective {
    let config = &resolved.config;
    let mut settings = Vec::new();
    let mut add = |key, label, value: String, as_written: serde_json::Value, apply| {
        let source = resolved.source_of(key).unwrap_or(Source::Default);
        // A value the environment or an argument holds is not the file's to change.
        let writable =
            apply != Apply::Never && matches!(source, Source::File { .. } | Source::Default);
        settings.push(Setting {
            key,
            label,
            value,
            as_written,
            source,
            apply,
            says: says(apply),
            tag: tag(apply),
            writable,
            choices: choices_for(key),
            note: note_for(key, resolved.file_path()),
        })
    };
    let menus = config.menu_settings();
    let scan = config.scan_options();

    add(
        "content_dir",
        "Music folders",
        config
            .content_dir
            .as_ref()
            .map(ContentDir::display)
            .unwrap_or_default(),
        written(&config.content_dir),
        Apply::Reread,
    );
    add(
        "friendly_name",
        "Server name",
        config.friendly_name(),
        written(&config.friendly_name()),
        Apply::Restart,
    );
    add(
        "http_port",
        "Port",
        config.http_port.unwrap_or(DEFAULT_PORT).to_string(),
        written(&config.http_port.unwrap_or(DEFAULT_PORT)),
        Apply::Restart,
    );
    add(
        "state_dir",
        "Index folder",
        config.state_dir().display().to_string(),
        written(&config.state_dir()),
        Apply::Restart,
    );
    add(
        "capture_dir",
        "Capture folder",
        config
            .capture_dir
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "off".to_owned()),
        written(&config.capture_dir),
        Apply::Restart,
    );
    add(
        "icon",
        "Icon",
        config
            .icon
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "built in".to_owned()),
        written(&config.icon),
        Apply::Restart,
    );
    add(
        "scan.threads",
        "Files read at the same time",
        scan.threads.to_string(),
        written(&scan.threads),
        Apply::NextPass,
    );
    add(
        "scan.sweep_minutes",
        "Scan interval",
        scan.sweep
            .map(|every| (every.as_secs() / 60).to_string())
            .unwrap_or_else(|| "off".to_owned()),
        // Zero is how the file says never, and the page needs a number to show either way.
        written(&scan.sweep.map_or(0, |every| every.as_secs() / 60)),
        Apply::NextPass,
    );
    add(
        "scan.exclude",
        "Exclusions",
        listed(&config.scan.exclude),
        written(&config.scan.exclude),
        Apply::Reread,
    );
    add(
        "scan.cover_art",
        "Preferred cover",
        match scan.cover_art {
            Prefer::Folder => "the image in the folder".to_owned(),
            Prefer::Embedded => "the image in the file".to_owned(),
        },
        written(&scan.cover_art),
        Apply::Reread,
    );
    add(
        "menus.album_threshold",
        "Albums shown in a list",
        menus.album_threshold.to_string(),
        written(&menus.album_threshold),
        Apply::Immediate,
    );
    add(
        "menus.alpha_group",
        "A to Z index",
        menus
            .alpha_group
            .map(|least| least.to_string())
            .unwrap_or_else(|| "off".to_owned()),
        written(&menus.alpha_group.unwrap_or(0)),
        Apply::Immediate,
    );
    add(
        "menus.recent",
        "Recently added",
        menus
            .recent
            .map(|most| format!("{most} files"))
            .unwrap_or_else(|| "off".to_owned()),
        written(&menus.recent.unwrap_or(0)),
        Apply::Immediate,
    );
    add(
        "menus.axes",
        "Menu sequence",
        menus
            .axes
            .iter()
            .map(|axis| axis.title())
            .collect::<Vec<_>>()
            .join(", "),
        written(&menus.axes),
        Apply::Immediate,
    );
    add(
        "menus.sort_ignore",
        "Words to ignore in the sort order",
        listed(&menus.sort_ignore),
        written(&menus.sort_ignore),
        // The words order the albums and the artists inside the library, which only a pass
        // builds; the axes alone would follow a change at once.
        Apply::Reread,
    );
    add(
        "clients",
        "Device profiles",
        match config.clients.len() {
            0 => "none".to_owned(),
            count => count.to_string(),
        },
        written(&config.clients),
        Apply::Never,
    );
    add(
        "log level",
        "Log level",
        std::env::var("RUST_LOG").unwrap_or_else(|_| "kantele=info".to_owned()),
        serde_json::Value::Null,
        Apply::Never,
    );

    Effective {
        file: resolved.file_path().map(Path::to_path_buf),
        settings,
    }
}

fn note_for(key: &str, file: Option<&Path>) -> Option<String> {
    match key {
        "scan.exclude" => Some(format!(
            "The server does not scan these names or paths, and never scans {}.",
            either(SKIPPED_FOLDERS)
        )),
        "clients" => Some(format!(
            "Edit the device profiles in {}.",
            file.map_or_else(
                || "kantele.toml".to_owned(),
                |file| file.display().to_string()
            )
        )),
        "log level" => Some("The server reads it from RUST_LOG when it starts.".to_owned()),
        _ => None,
    }
}

/// `a, b or c`.
fn either(names: &[&str]) -> String {
    match names.split_last() {
        Some((last, [])) => (*last).to_owned(),
        Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
        None => String::new(),
    }
}

/// Every value a key may take, where the set is closed.
fn choices_for(key: &str) -> Vec<Choice> {
    let named = |value: &Prefer, label| Choice {
        value: written(value).as_str().unwrap_or_default().to_owned(),
        label,
    };
    match key {
        "scan.cover_art" => vec![
            named(&Prefer::Folder, "The image in the folder"),
            named(&Prefer::Embedded, "The image in the file"),
        ],
        // The value is the name the file writes, not the letter an object identifier carries.
        "menus.axes" => crate::browse::FACETS
            .iter()
            .filter_map(|facet| {
                Some(Choice {
                    value: serde_json::to_value(facet).ok()?.as_str()?.to_owned(),
                    label: facet.title(),
                })
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// A list as a page shows it, with `none` for an empty one.
fn listed(values: &[String]) -> String {
    match values.is_empty() {
        true => "none".to_owned(),
        false => values.join(", "),
    }
}

/// A value as JSON, or nothing where it cannot be rendered, which nothing here can fail at.
fn written<T: Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;

    fn resolved(config: Config) -> Resolved {
        Resolved {
            config,
            file: None,
            command_line_folder: None,
        }
    }

    fn note(key: &str) -> String {
        effective(&resolved(Config::default()))
            .settings
            .into_iter()
            .find(|setting| setting.key == key)
            .and_then(|setting| setting.note)
            .unwrap_or_else(|| panic!("{key} carries a note"))
    }

    #[test]
    fn the_exclusions_name_every_folder_the_server_never_scans() {
        let said = note("scan.exclude");
        for name in SKIPPED_FOLDERS {
            assert!(said.contains(name), "{name} is left out of: {said}");
        }
    }

    #[test]
    fn the_log_level_says_where_it_is_read_and_it_is_not_the_file() {
        assert!(note("log level").contains("RUST_LOG"));
        assert!(!note("log level").contains("kantele.toml"));
    }

    #[test]
    fn a_setting_with_a_closed_set_of_values_carries_them_so_the_page_holds_no_copy() {
        let config = Config::parse("[scan]\ncover_art = \"embedded\"\n").expect("it parses");
        let effective = effective(&resolved(config));
        let setting = effective
            .settings
            .iter()
            .find(|setting| setting.key == "scan.cover_art")
            .expect("the page is offered it");
        assert_eq!(setting.apply, Apply::Reread, "the store holds the choice");
        assert_eq!(setting.value, "the image in the file");
        assert_eq!(setting.as_written, serde_json::json!("embedded"));
        assert_eq!(
            setting
                .choices
                .iter()
                .map(|choice| choice.value.as_str())
                .collect::<Vec<_>>(),
            vec!["folder", "embedded"]
        );
    }

    #[test]
    fn every_key_the_page_is_offered_has_a_layer_the_server_answers_for() {
        let resolved = resolved(Config::default());
        for setting in effective(&resolved).settings {
            assert!(
                resolved.source_of(setting.key).is_some(),
                "{} is described but no layer answers for it",
                setting.key
            );
        }
    }

    #[test]
    fn every_mode_is_said_in_one_of_the_page_s_four_phrases() {
        assert_eq!(says(Apply::Immediate), "This change applies now.");
        assert_eq!(tag(Apply::Immediate), "applies at once");
        assert_eq!(tag(Apply::Reread), "reads the library again");
        assert_eq!(says(Apply::Restart), "You must restart the server.");
        assert_eq!(
            says(Apply::Never),
            "You cannot change this setting on this page.",
            "and the one mode a page cannot write says so"
        );
    }
}
