//! Configuration: a file on bare metal, environment variables in a container.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::browse::{Facet, Settings};
use crate::index::ScanOptions;
use crate::index::scan::Exclusions;
use crate::upnp::client::Profile;

/// Port for content and control; nothing is standard, it just has to be free.
pub const DEFAULT_PORT: u16 = 8200;

/// The music folder, or several, each told apart by its name, which every relative path under it
/// then starts with.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum ContentDir {
    One(PathBuf),
    Several(Vec<PathBuf>),
}

impl ContentDir {
    pub fn paths(&self) -> Vec<PathBuf> {
        match self {
            Self::One(path) => vec![path.clone()],
            Self::Several(paths) => paths.clone(),
        }
    }

    fn display(&self) -> String {
        self.paths()
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Everything the server needs to start.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// The music folder or folders, which the command line may name instead.
    pub content_dir: Option<ContentDir>,
    /// The name control points display, which is the user's to choose.
    pub friendly_name: Option<String>,
    /// Port for content and the SOAP endpoint; zero asks the system for a free one.
    pub http_port: Option<u16>,
    /// Where the store lives, or nothing to let [`state_dir`] decide.
    pub state_dir: Option<PathBuf>,
    /// A PNG or a JPEG to publish instead of the icon this server ships.
    pub icon: Option<PathBuf>,
    /// A folder every control exchange is written to, so a client's requests can be replayed.
    pub capture_dir: Option<PathBuf>,
    pub scan: Scan,
    pub menus: Menus,
    /// One per control point that needs an answer differing from the conservative one.
    pub clients: Vec<Profile>,
}

/// Bounds on a scan; beyond a handful of threads an ARM NAS goes slower.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Scan {
    pub threads: Option<usize>,
    /// Minutes between looks at the tree for changes nothing announced; zero never looks.
    pub sweep_minutes: Option<u64>,
    /// Names and paths under the music folder that are never walked.
    pub exclude: Vec<String>,
    /// Which cover wins where a folder image and an embedded picture both exist.
    pub cover_art: Option<crate::index::artwork::Prefer>,
}

/// What the menus are allowed to do.
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Menus {
    /// How many albums a selection may hold before the menus stop narrowing and show them.
    pub album_threshold: Option<usize>,
    /// How many values a listing must hold before it offers the way into its letter index.
    pub alpha_group: Option<usize>,
    /// How many of the newest files the recently added menu is built from; zero offers none.
    pub recent: Option<usize>,
    /// The axes a menu offers, in this order.
    pub axes: Option<Vec<Facet>>,
    /// Words a listing looks past when ordering a value that has no sort tag.
    pub sort_ignore: Option<Vec<String>>,
}

impl Config {
    /// Reads a configuration file.
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("parsing {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let config: Self = toml::from_str(text)?;
        config.check()?;
        Ok(config)
    }

    /// What the types cannot refuse: a repeated axis, and a word or a pattern that is nothing.
    fn check(&self) -> Result<()> {
        if let Some(ContentDir::Several(paths)) = &self.content_dir
            && paths.is_empty()
        {
            anyhow::bail!("content_dir names no folder at all");
        }
        if let Some(axes) = &self.menus.axes {
            for (at, axis) in axes.iter().enumerate() {
                if axes[..at].contains(axis) {
                    anyhow::bail!("menus.axes names {} twice", axis.title());
                }
            }
        }
        if self
            .scan
            .exclude
            .iter()
            .any(|pattern| pattern.trim().is_empty())
        {
            anyhow::bail!("scan.exclude holds an empty pattern, which would mean nothing");
        }
        if let Some(words) = &self.menus.sort_ignore
            && words.iter().any(|word| word.trim().is_empty())
        {
            anyhow::bail!("menus.sort_ignore holds an empty word, which would mean nothing");
        }
        Ok(())
    }

    pub fn with_env(mut self) -> Self {
        if let Some(port) = std::env::var("KANTELE_PORT")
            .ok()
            .and_then(|value| value.parse().ok())
        {
            self.http_port = Some(port);
        }
        if let Ok(name) = std::env::var("KANTELE_NAME") {
            self.friendly_name = Some(name);
        }
        if let Some(state) = env_path("KANTELE_STATE") {
            self.state_dir = Some(state);
        }
        if let Some(dir) = env_path("KANTELE_CAPTURE") {
            self.capture_dir = Some(dir);
        }
        self
    }

    pub fn friendly_name(&self) -> String {
        self.friendly_name
            .clone()
            .unwrap_or_else(|| "Kantele".to_owned())
    }

    /// The music folders named, in the order they were named.
    pub fn content_dirs(&self) -> Vec<PathBuf> {
        self.content_dir
            .as_ref()
            .map(ContentDir::paths)
            .unwrap_or_default()
    }

    /// Where this configuration puts the store.
    pub fn state_dir(&self) -> PathBuf {
        self.state_dir.clone().unwrap_or_else(state_dir)
    }

    pub fn scan_options(&self) -> ScanOptions {
        let defaults = ScanOptions::default();
        ScanOptions {
            threads: self.scan.threads.unwrap_or(defaults.threads),
            sweep: match self.scan.sweep_minutes {
                None => defaults.sweep,
                Some(0) => None,
                Some(minutes) => Some(std::time::Duration::from_secs(minutes * 60)),
            },
            exclude: Exclusions::new(self.scan.exclude.iter().map(String::as_str)),
            cover_art: self.scan.cover_art.unwrap_or(defaults.cover_art),
        }
    }

    /// The picture an owner wants published instead of the shipped one, if they named one.
    pub fn icon(&self) -> Option<PathBuf> {
        self.icon.clone()
    }

    /// The client profiles this configuration ships.
    pub fn client_profiles(&self) -> Vec<Profile> {
        self.clients.clone()
    }

    /// The menu settings this configuration asks for, defaulted axis by axis.
    pub fn menu_settings(&self) -> Settings {
        let mut settings = Settings::default();
        if let Some(threshold) = self.menus.album_threshold {
            settings.album_threshold = threshold;
        }
        if let Some(least) = self.menus.alpha_group {
            // Zero is how a file says never, since a listing always holds at least none.
            settings.alpha_group = (least > 0).then_some(least);
        }
        if let Some(most) = self.menus.recent {
            settings.recent = (most > 0).then_some(most);
        }
        if let Some(axes) = &self.menus.axes {
            settings.axes = axes.clone();
        }
        if let Some(words) = &self.menus.sort_ignore {
            settings.sort_ignore = words.clone();
        }
        settings
    }
}

/// The file's text with `changes` applied, comments and layout intact, or the reason it was refused.
pub fn rewrite(text: &str, changes: &serde_json::Map<String, serde_json::Value>) -> Result<String> {
    let mut document: toml_edit::DocumentMut = text.parse().context("parsing the file")?;
    for (key, value) in changes {
        let mut parts = key.split('.').peekable();
        let mut table = document.as_table_mut();
        while let Some(part) = parts.next() {
            if parts.peek().is_none() {
                write_key(table, part, value, key)?;
                break;
            }
            table = table
                .entry(part)
                .or_insert(toml_edit::Item::Table(toml_edit::Table::new()))
                .as_table_mut()
                .with_context(|| format!("{part} in {key} is not a table"))?;
        }
    }
    let written = document.to_string();
    Config::parse(&written).context("the file would not read back")?;
    Ok(written)
}

/// One key written into the table that holds it, or removed where the value is nothing.
fn write_key(
    table: &mut toml_edit::Table,
    part: &str,
    value: &serde_json::Value,
    key: &str,
) -> Result<()> {
    if value.is_null() {
        table.remove(part);
        return Ok(());
    }
    let mut fresh = toml_value(value, key)?;
    // In place where the key exists, so the comment above it and beside it stay.
    match table.get_mut(part).and_then(|item| item.as_value_mut()) {
        Some(existing) => {
            *fresh.decor_mut() = existing.decor().clone();
            *existing = fresh;
        }
        None => {
            table.insert(part, toml_edit::Item::Value(fresh));
        }
    }
    Ok(())
}

/// A JSON value as the TOML the file writes it as.
fn toml_value(value: &serde_json::Value, key: &str) -> Result<toml_edit::Value> {
    match value {
        serde_json::Value::Bool(flag) => Ok(toml_edit::Value::from(*flag)),
        serde_json::Value::Number(number) => number
            .as_i64()
            .map(toml_edit::Value::from)
            .or_else(|| number.as_f64().map(toml_edit::Value::from))
            .with_context(|| format!("{key}: {number} is not a number the file can hold")),
        serde_json::Value::String(text) => Ok(toml_edit::Value::from(text.as_str())),
        serde_json::Value::Array(values) => {
            let mut list = toml_edit::Array::new();
            for value in values {
                list.push(toml_value(value, key)?);
            }
            Ok(toml_edit::Value::Array(list))
        }
        other => anyhow::bail!("{key}: a setting is a value or a list, not {other}"),
    }
}

/// Where state that must survive a restart is kept.
pub fn state_dir() -> PathBuf {
    if let Some(explicit) = env_path("KANTELE_STATE") {
        return explicit;
    }
    if let Some(xdg) = env_path("XDG_DATA_HOME") {
        return xdg.join("kantele");
    }
    match env_path("HOME") {
        Some(home) => home.join(".local/share/kantele"),
        None => PathBuf::from(".kantele"),
    }
}

/// The name of the store inside the state folder.
pub fn store_path(state_dir: &Path) -> PathBuf {
    state_dir.join("index.sqlite")
}

/// A list as a page shows it, with `none` for an empty one.
fn listed(values: &[String]) -> String {
    match values.is_empty() {
        true => "none".to_owned(),
        false => values.join(", "),
    }
}

fn env_path(name: &str) -> Option<PathBuf> {
    let value = std::env::var_os(name)?;
    (!value.is_empty()).then(|| PathBuf::from(value))
}

/// Which of the three layers a value in force came from.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "layer", rename_all = "kebab-case")]
pub enum Source {
    /// The file, named, because an owner running two of them needs to know which one to edit.
    File { path: PathBuf },
    /// An environment variable, named.
    Environment { variable: &'static str },
    /// An argument, which is the last word on it.
    CommandLine,
    /// This server, because nothing else said anything.
    Default,
}

impl Source {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::File { .. } => "file",
            Self::Environment { .. } => "environment",
            Self::CommandLine => "command line",
            Self::Default => "default",
        }
    }

    /// The file or the variable this value came from.
    pub fn names(&self) -> Option<String> {
        match self {
            Self::File { path } => Some(path.display().to_string()),
            Self::Environment { variable } => Some((*variable).to_owned()),
            Self::CommandLine | Self::Default => None,
        }
    }
}

/// How a change reaches the running server. Variant order is increasing cost, so one save takes
/// the greatest mode among the keys it writes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Apply {
    /// Not a setting a page writes.
    Never,
    /// The menus are rebuilt off-thread and the update id bumps.
    Immediate,
    /// Saved, and the next pass over the library uses it.
    NextPass,
    /// Saved, then a whole pass runs while the old index keeps serving.
    Reread,
    /// Saved, and nothing changes until the process starts again.
    Restart,
}

impl Apply {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Never => "never",
            Self::Immediate => "immediate",
            Self::NextPass => "next-pass",
            Self::Reread => "reread",
            Self::Restart => "restart",
        }
    }

    /// What a page shows beside the field.
    pub fn says(self) -> &'static str {
        match self {
            Self::Never => "not written from this page",
            Self::Immediate => "applies at once",
            Self::NextPass => "applies at the next check",
            Self::Reread => "reads the library again",
            Self::Restart => "needs a restart",
        }
    }
}

/// One value a setting may take, where the set of them is closed.
#[derive(Clone, Debug, serde::Serialize)]
pub struct Choice {
    pub value: String,
    pub label: &'static str,
}

/// One setting in force.
#[derive(Clone, Debug, serde::Serialize)]
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
    /// Whether the page may write it now, which the layer holding the value decides.
    pub writable: bool,
    /// Every value this setting may take, where they are countable; empty where they are not.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<Choice>,
}

/// Every setting in force, with the file that was read, if one was.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct Effective {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<PathBuf>,
    pub settings: Vec<Setting>,
}

/// What was read before the environment and the command line had their say.
pub struct Layers<'a> {
    /// The file that was read and what it said.
    pub file: Option<(&'a Path, &'a Config)>,
    /// Whether the folder to serve was named as an argument, which outranks both other layers.
    pub content_dir_on_command_line: bool,
}

impl Config {
    /// Where each value in force came from, per key.
    pub fn effective(&self, layers: &Layers<'_>) -> Effective {
        let from_file = |named: bool| -> Option<Source> {
            let (path, _) = layers.file?;
            named.then(|| Source::File {
                path: path.to_path_buf(),
            })
        };
        let said = |pick: fn(&Config) -> bool| -> bool {
            layers.file.map(|(_, file)| pick(file)).unwrap_or(false)
        };
        let from_env = |variable: &'static str| -> Option<Source> {
            env_path(variable).map(|_| Source::Environment { variable })
        };
        let layered = |env: Option<Source>, named: bool| -> Source {
            env.or_else(|| from_file(named)).unwrap_or(Source::Default)
        };

        let mut settings = Vec::new();
        let mut add = |key, label, value: String, as_written: serde_json::Value, source, apply| {
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
                says: apply.says(),
                writable,
                choices: choices_for(key),
            })
        };
        let menus = self.menu_settings();
        let scan = self.scan_options();

        add(
            "content_dir",
            "Music folder",
            self.content_dir
                .as_ref()
                .map(ContentDir::display)
                .unwrap_or_default(),
            written(&self.content_dir),
            if layers.content_dir_on_command_line {
                Source::CommandLine
            } else {
                layered(None, said(|file| file.content_dir.is_some()))
            },
            Apply::Reread,
        );
        add(
            "friendly_name",
            "Server name",
            self.friendly_name(),
            written(&self.friendly_name()),
            layered(
                std::env::var("KANTELE_NAME")
                    .ok()
                    .map(|_| Source::Environment {
                        variable: "KANTELE_NAME",
                    }),
                said(|file| file.friendly_name.is_some()),
            ),
            Apply::Restart,
        );
        add(
            "http_port",
            "Port",
            self.http_port.unwrap_or(DEFAULT_PORT).to_string(),
            written(&self.http_port.unwrap_or(DEFAULT_PORT)),
            layered(
                std::env::var("KANTELE_PORT")
                    .ok()
                    .map(|_| Source::Environment {
                        variable: "KANTELE_PORT",
                    }),
                said(|file| file.http_port.is_some()),
            ),
            Apply::Restart,
        );
        add(
            "state_dir",
            "Index folder",
            self.state_dir().display().to_string(),
            written(&self.state_dir()),
            layered(
                from_env("KANTELE_STATE"),
                said(|file| file.state_dir.is_some()),
            ),
            Apply::Restart,
        );
        add(
            "capture_dir",
            "Capture folder",
            self.capture_dir
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "off".to_owned()),
            written(&self.capture_dir),
            layered(
                from_env("KANTELE_CAPTURE"),
                said(|file| file.capture_dir.is_some()),
            ),
            Apply::Restart,
        );
        add(
            "icon",
            "Icon",
            self.icon
                .as_ref()
                .map(|path| path.display().to_string())
                .unwrap_or_else(|| "built in".to_owned()),
            written(&self.icon),
            layered(None, said(|file| file.icon.is_some())),
            Apply::Restart,
        );
        add(
            "scan.threads",
            "Files read at once",
            scan.threads.to_string(),
            written(&scan.threads),
            layered(None, said(|file| file.scan.threads.is_some())),
            Apply::NextPass,
        );
        add(
            "scan.sweep_minutes",
            "Checks for changes",
            scan.sweep
                .map(|every| (every.as_secs() / 60).to_string())
                .unwrap_or_else(|| "off".to_owned()),
            // Zero is how the file says never, and the page needs a number to show either way.
            written(&scan.sweep.map_or(0, |every| every.as_secs() / 60)),
            layered(None, said(|file| file.scan.sweep_minutes.is_some())),
            Apply::NextPass,
        );
        add(
            "scan.exclude",
            "Left out of the library",
            listed(&self.scan.exclude),
            written(&self.scan.exclude),
            layered(None, said(|file| !file.scan.exclude.is_empty())),
            Apply::Reread,
        );
        add(
            "scan.cover_art",
            "Cover art",
            match scan.cover_art {
                crate::index::artwork::Prefer::Folder => "the folder's image".to_owned(),
                crate::index::artwork::Prefer::Embedded => "the one in the file".to_owned(),
            },
            written(&scan.cover_art),
            layered(None, said(|file| file.scan.cover_art.is_some())),
            Apply::Reread,
        );
        add(
            "menus.album_threshold",
            "Show albums directly up to",
            menus.album_threshold.to_string(),
            written(&menus.album_threshold),
            layered(None, said(|file| file.menus.album_threshold.is_some())),
            Apply::Immediate,
        );
        add(
            "menus.alpha_group",
            "A-Z index on lists of at least",
            menus
                .alpha_group
                .map(|least| least.to_string())
                .unwrap_or_else(|| "off".to_owned()),
            written(&menus.alpha_group.unwrap_or(0)),
            layered(None, said(|file| file.menus.alpha_group.is_some())),
            Apply::Immediate,
        );
        add(
            "menus.recent",
            "Recently added reaches back over",
            menus
                .recent
                .map(|most| format!("{most} files"))
                .unwrap_or_else(|| "off".to_owned()),
            written(&menus.recent.unwrap_or(0)),
            layered(None, said(|file| file.menus.recent.is_some())),
            Apply::Immediate,
        );
        add(
            "menus.axes",
            "Menus, in order",
            menus
                .axes
                .iter()
                .map(|axis| axis.title())
                .collect::<Vec<_>>()
                .join(", "),
            written(&menus.axes),
            layered(None, said(|file| file.menus.axes.is_some())),
            Apply::Immediate,
        );
        add(
            "menus.sort_ignore",
            "Ignored when sorting",
            listed(&menus.sort_ignore),
            written(&menus.sort_ignore),
            layered(None, said(|file| file.menus.sort_ignore.is_some())),
            // The words order the albums and the artists inside the library, which only a pass
            // builds; the axes alone would follow a change at once.
            Apply::Reread,
        );
        add(
            "clients",
            "Device profiles",
            match self.clients.len() {
                0 => "none".to_owned(),
                count => count.to_string(),
            },
            written(&self.clients),
            layered(None, said(|file| !file.clients.is_empty())),
            Apply::Never,
        );
        add(
            "log level",
            "Log level",
            std::env::var("RUST_LOG").unwrap_or_else(|_| "kantele=info".to_owned()),
            serde_json::Value::Null,
            std::env::var("RUST_LOG")
                .ok()
                .map(|_| Source::Environment {
                    variable: "RUST_LOG",
                })
                .unwrap_or(Source::Default),
            Apply::Never,
        );

        Effective {
            file: layers.file.map(|(path, _)| path.to_path_buf()),
            settings,
        }
    }
}

/// Every value a key may take, where the set is closed.
fn choices_for(key: &str) -> Vec<Choice> {
    let named = |value: &crate::index::artwork::Prefer, label| Choice {
        value: written(value).as_str().unwrap_or_default().to_owned(),
        label,
    };
    match key {
        "scan.cover_art" => vec![
            named(&crate::index::artwork::Prefer::Folder, "The folder's image"),
            named(
                &crate::index::artwork::Prefer::Embedded,
                "The one in the file",
            ),
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

/// A value as JSON, or nothing where it cannot be rendered, which nothing here can fail at.
fn written<T: Serialize>(value: &T) -> serde_json::Value {
    serde_json::to_value(value).unwrap_or(serde_json::Value::Null)
}

/// The file, the environment and the command line, resolved in that order.
#[derive(Clone, Debug, Default)]
pub struct Resolved {
    pub config: Config,
    pub effective: Effective,
    /// The folder the command line named, which outranks the file on every load.
    command_line_folder: Option<PathBuf>,
}

impl Resolved {
    pub fn load(file: Option<&Path>, command_line_folder: Option<PathBuf>) -> Result<Self> {
        let read = match file {
            Some(path) => Some(Config::load(path)?),
            None => None,
        };
        let mut config = read.clone().unwrap_or_default().with_env();
        if let Some(folder) = &command_line_folder {
            config.content_dir = Some(ContentDir::One(folder.clone()));
        }
        let effective = config.effective(&Layers {
            file: file.zip(read.as_ref()),
            content_dir_on_command_line: command_line_folder.is_some(),
        });
        Ok(Self {
            config,
            effective,
            command_line_folder,
        })
    }

    /// The same layers read again, after the file changed.
    pub fn reload(&self) -> Result<Self> {
        let file = self
            .effective
            .file
            .as_deref()
            .context("no configuration file was read at startup")?;
        Self::load(Some(file), self.command_line_folder.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_example_configuration_file_parses_and_says_what_the_defaults_say() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("kantele.example.toml");
        let example = Config::load(&path).expect("the example in the repository parses");
        assert_eq!(example.friendly_name(), Config::default().friendly_name());
        assert_eq!(
            example.scan_options().threads,
            ScanOptions::default().threads
        );
        assert_eq!(example.scan_options().sweep, ScanOptions::default().sweep);
        assert_eq!(
            example.menu_settings(),
            Settings::default(),
            "an example that documents something other than the defaults documents nothing"
        );
    }

    #[test]
    fn a_rewrite_keeps_the_owners_comments_and_refuses_what_would_not_read_back() {
        let text = "# The owner's note.\nfriendly_name = \"Salon\"\n\n[menus]\n# Kept too.\nalbum_threshold = 24\n";
        let change = |key: &str, value: serde_json::Value| {
            let mut changes = serde_json::Map::new();
            changes.insert(key.to_owned(), value);
            changes
        };
        let written =
            rewrite(text, &change("menus.album_threshold", 30.into())).expect("it writes");
        assert!(written.contains("# The owner's note.") && written.contains("# Kept too."));
        assert!(written.contains("album_threshold = 30"), "{written}");
        assert!(written.contains("friendly_name = \"Salon\""));

        let created =
            rewrite(text, &change("scan.sweep_minutes", 5.into())).expect("a table is created");
        assert_eq!(
            toml::from_str::<Config>(&created)
                .expect("it reads back")
                .scan
                .sweep_minutes,
            Some(5)
        );

        assert!(rewrite(text, &change("menus.album_threshold", "many".into())).is_err());
        assert!(rewrite(text, &change("menus.nonsense", 1.into())).is_err());
    }

    #[test]
    fn the_sweep_is_read_in_minutes_and_zero_turns_it_off() {
        let read = |text: &str| {
            toml::from_str::<Config>(text)
                .expect("it parses")
                .scan_options()
                .sweep
        };
        assert_eq!(
            read("[scan]\nsweep_minutes = 5\n"),
            Some(std::time::Duration::from_secs(300))
        );
        assert_eq!(read("[scan]\nsweep_minutes = 0\n"), None);
        assert_eq!(read("[scan]\n"), ScanOptions::default().sweep);
    }

    #[test]
    fn a_client_profile_is_data_and_every_key_the_example_documents_parses() {
        let config: Config = toml::from_str(
            r#"
            [[clients]]
            name = "example"
            matches = ["Example-Renderer"]
            roles = "both"
            join = ", "
            rate_cap = 2.0

            [clients.formats."audio/x-dsf"]
            mime = "audio/dsf"
            dlna_profile = ""
            "#,
        )
        .expect("it parses");
        let profiles = config.client_profiles();
        let [profile] = profiles.as_slice() else {
            panic!("one profile");
        };
        assert_eq!(profile.roles, crate::upnp::client::Roles::Both);
        assert_eq!(profile.join.as_deref(), Some(", "));
        assert_eq!(profile.rate_cap, Some(2.0));
        assert_eq!(profile.mime_for("audio/x-dsf"), "audio/dsf");
        assert_eq!(profile.claim_for("audio/x-dsf"), Some(None));
    }

    #[test]
    fn several_music_folders_are_a_list_and_an_empty_list_is_refused() {
        let several = Config::parse(r#"content_dir = ["/volume1/music", "/volume2/more"]"#)
            .expect("a list parses");
        assert_eq!(
            several.content_dirs(),
            [
                PathBuf::from("/volume1/music"),
                PathBuf::from("/volume2/more")
            ]
        );
        assert_eq!(
            Config::parse(r#"content_dir = "/music""#)
                .expect("one folder parses")
                .content_dirs(),
            [PathBuf::from("/music")]
        );
        assert!(Config::parse("content_dir = []").is_err());
    }

    #[test]
    fn a_file_that_names_only_a_folder_gets_every_default() {
        let config: Config = toml::from_str(r#"content_dir = "/music""#).expect("it parses");
        assert_eq!(config.content_dirs(), [PathBuf::from("/music")]);
        assert_eq!(config.friendly_name(), "Kantele");
        assert_eq!(
            config.scan_options().threads,
            ScanOptions::default().threads
        );
        assert_eq!(config.menu_settings(), Settings::default());
    }

    #[test]
    fn the_letter_split_is_read_from_the_file_and_absent_by_default() {
        let read = |text: &str| {
            toml::from_str::<Config>(text)
                .expect("it parses")
                .menu_settings()
                .alpha_group
        };
        assert_eq!(read("[menus]\nalpha_group = 250\n"), Some(250));
        assert_eq!(
            read("[menus]\nalpha_group = 0\n"),
            None,
            "zero is how a file says never"
        );
        let default = Some(crate::browse::ALPHA_GROUP);
        assert_eq!(read("[menus]\n"), default);
        assert_eq!(
            read(""),
            default,
            "and a server with no configuration file at all still offers the way in"
        );
    }

    #[test]
    fn the_album_threshold_is_configured_and_the_rest_keeps_its_default() {
        let config: Config = toml::from_str(
            r#"
            [menus]
            album_threshold = 40
            "#,
        )
        .expect("it parses");
        let settings = config.menu_settings();
        assert_eq!(settings.album_threshold, 40);
        assert_eq!(
            settings.alpha_group,
            Some(crate::browse::ALPHA_GROUP),
            "a file naming one menu setting keeps the default of the other"
        );
    }

    #[test]
    fn the_axes_the_exclusions_and_the_ignored_words_are_read_from_the_file() {
        let config = Config::parse(
            r#"
            [scan]
            exclude = ["*.iso", "Podcasts"]

            [menus]
            axes = ["date", "genre"]
            sort_ignore = ["The", "Les", "L'"]
            "#,
        )
        .expect("it parses");
        let menus = config.menu_settings();
        assert_eq!(
            menus.axes,
            vec![crate::browse::Facet::Date, crate::browse::Facet::Genre]
        );
        assert_eq!(menus.sort_ignore, ["The", "Les", "L'"]);
        assert!(
            config
                .scan_options()
                .exclude
                .excludes(Path::new("Rips/disc.iso"))
        );
        assert_eq!(
            Config::parse("[menus]\nsort_ignore = []\n")
                .expect("it parses")
                .menu_settings()
                .sort_ignore,
            Vec::<String>::new(),
            "an empty list is how a file says look past nothing"
        );
    }

    #[test]
    fn a_repeated_axis_and_an_empty_pattern_are_refused() {
        let refused = Config::parse("[menus]\naxes = [\"genre\", \"date\", \"genre\"]\n")
            .expect_err("a menu offered twice");
        assert!(refused.to_string().contains("Genre"), "{refused}");
        assert!(Config::parse("[scan]\nexclude = [\"*.iso\", \" \"]\n").is_err());
        assert!(Config::parse("[menus]\nsort_ignore = [\"\"]\n").is_err());
        assert!(
            Config::parse("[menus]\naxes = [\"nonsense\"]\n").is_err(),
            "an axis nobody has"
        );
    }

    #[test]
    fn a_setting_with_a_closed_set_of_values_carries_them_so_the_page_holds_no_copy() {
        let config = Config::parse("[scan]\ncover_art = \"embedded\"\n").expect("it parses");
        assert_eq!(
            config.scan_options().cover_art,
            crate::index::artwork::Prefer::Embedded
        );

        let effective = config.effective(&Layers {
            file: None,
            content_dir_on_command_line: false,
        });
        let setting = effective
            .settings
            .iter()
            .find(|setting| setting.key == "scan.cover_art")
            .expect("the page is offered it");
        assert_eq!(setting.apply, Apply::Reread, "the store holds the choice");
        assert_eq!(setting.value, "the one in the file");
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
    fn a_key_nobody_recognises_is_refused_rather_than_ignored() {
        assert!(toml::from_str::<Config>(r#"content_dirs = "/music""#).is_err());
        assert!(toml::from_str::<Config>("[menus]\nalbum_treshold = 40").is_err());
        assert!(
            toml::from_str::<Config>("[[menus.buckets]]\naxis = \"nonsense\"\nabove = 1").is_err()
        );
    }
}
