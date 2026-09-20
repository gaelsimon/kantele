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

    pub(crate) fn display(&self) -> String {
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
        if let Ok(named) = std::env::var("KANTELE_PORT") {
            match env_port() {
                Some(port) => self.http_port = Some(port),
                None => tracing::warn!(
                    value = %named,
                    "KANTELE_PORT is not a port number: the port it names is not used"
                ),
            }
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

/// The port `KANTELE_PORT` names, and nothing where it names something that is not a port.
fn env_port() -> Option<u16> {
    std::env::var("KANTELE_PORT").ok()?.trim().parse().ok()
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
}

/// The file, the environment and the command line, resolved in that order.
#[derive(Clone, Debug, Default)]
pub struct Resolved {
    pub config: Config,
    /// The file that was read and what it said, before the other two layers had their say.
    pub file: Option<(PathBuf, Config)>,
    /// The folder the command line named, which outranks the file on every load.
    pub command_line_folder: Option<PathBuf>,
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
        Ok(Self {
            config,
            file: file.map(Path::to_path_buf).zip(read),
            command_line_folder,
        })
    }

    /// The file that was read, if one was.
    pub fn file_path(&self) -> Option<&Path> {
        self.file.as_ref().map(|(path, _)| path.as_path())
    }

    /// The same layers read again, after the file changed.
    pub fn reload(&self) -> Result<Self> {
        let file = self
            .file_path()
            .context("no configuration file was read at startup")?;
        Self::load(Some(file), self.command_line_folder.clone())
    }

    /// Which layer holds the value in force for a key, or nothing for a key this server has not.
    pub fn source_of(&self, key: &str) -> Option<Source> {
        let file = |named: fn(&Config) -> bool| -> Option<Source> {
            let (path, read) = self.file.as_ref()?;
            named(read).then(|| Source::File { path: path.clone() })
        };
        let set = |variable: &'static str| -> Option<Source> {
            std::env::var(variable)
                .ok()
                .map(|_| Source::Environment { variable })
        };
        let set_path = |variable: &'static str| -> Option<Source> {
            env_path(variable).map(|_| Source::Environment { variable })
        };
        let layered = match key {
            "content_dir" if self.command_line_folder.is_some() => Some(Source::CommandLine),
            "content_dir" => file(|read| read.content_dir.is_some()),
            "friendly_name" => {
                set("KANTELE_NAME").or_else(|| file(|read| read.friendly_name.is_some()))
            }
            "http_port" => env_port()
                .map(|_| Source::Environment {
                    variable: "KANTELE_PORT",
                })
                .or_else(|| file(|read| read.http_port.is_some())),
            "state_dir" => {
                set_path("KANTELE_STATE").or_else(|| file(|read| read.state_dir.is_some()))
            }
            "capture_dir" => {
                set_path("KANTELE_CAPTURE").or_else(|| file(|read| read.capture_dir.is_some()))
            }
            "icon" => file(|read| read.icon.is_some()),
            "scan.threads" => file(|read| read.scan.threads.is_some()),
            "scan.sweep_minutes" => file(|read| read.scan.sweep_minutes.is_some()),
            "scan.exclude" => file(|read| !read.scan.exclude.is_empty()),
            "scan.cover_art" => file(|read| read.scan.cover_art.is_some()),
            "menus.album_threshold" => file(|read| read.menus.album_threshold.is_some()),
            "menus.alpha_group" => file(|read| read.menus.alpha_group.is_some()),
            "menus.recent" => file(|read| read.menus.recent.is_some()),
            "menus.axes" => file(|read| read.menus.axes.is_some()),
            "menus.sort_ignore" => file(|read| read.menus.sort_ignore.is_some()),
            "clients" => file(|read| !read.clients.is_empty()),
            "log level" => set("RUST_LOG"),
            _ => return None,
        };
        Some(layered.unwrap_or(Source::Default))
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
    fn the_preferred_cover_is_read_from_the_file() {
        let config = Config::parse("[scan]\ncover_art = \"embedded\"\n").expect("it parses");
        assert_eq!(
            config.scan_options().cover_art,
            crate::index::artwork::Prefer::Embedded
        );
    }

    #[test]
    fn the_layer_holding_a_value_is_named_and_the_command_line_outranks_the_file() {
        let read = Config::parse("content_dir = \"/music\"\n[menus]\nalbum_threshold = 24\n")
            .expect("it parses");
        let path = PathBuf::from("/etc/kantele.toml");
        let mut resolved = Resolved {
            config: read.clone(),
            file: Some((path.clone(), read)),
            command_line_folder: None,
        };
        assert_eq!(
            resolved.source_of("menus.album_threshold"),
            Some(Source::File { path: path.clone() })
        );
        assert_eq!(resolved.source_of("menus.recent"), Some(Source::Default));
        assert_eq!(
            resolved.source_of("content_dir"),
            Some(Source::File { path })
        );
        resolved.command_line_folder = Some(PathBuf::from("/elsewhere"));
        assert_eq!(resolved.source_of("content_dir"), Some(Source::CommandLine));
        assert_eq!(
            resolved.source_of("nonsense"),
            None,
            "a key this server has not"
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
