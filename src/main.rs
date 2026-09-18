//! Wiring only: everything that does work lives in the library.

use std::net::{Ipv4Addr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use kantele::api::Operation;
use kantele::config::{Config, Resolved};
use kantele::index::{Roots, Store};
use kantele::server::{self, Server};
use kantele::service::{self, Indexing, Pass, Start};
use kantele::upnp::client::Profiles;
use kantele::upnp::description::DeviceIdentity;
use kantele::upnp::icon::Icon;
use kantele::upnp::{capture, ssdp};
use kantele::{config, mdns, report, state};

const USAGE: &str = "usage: kantele [<music folder>] [options]";

const HELP: &str = "\
A UPnP/DLNA music server for amplifiers and streamers.

The folder holds the music to serve. Without one the server still starts and serves its page, where
the folder is chosen.

  --config <file>  the configuration file to read
  --no-scan        serve the saved index without reading the disk
  --reread         read every file again, whatever the saved index holds
  --report         print what the index made of the library, then stop
  --albums <file>  write one line per album to a file, then stop
  --version        print the version, then stop
  --help           print this, then stop

Precedence is the file, then the environment, then the command line.

  KANTELE_CONFIG   the same file, where the option is not given
  KANTELE_STATE    where the index and the device identity are kept
  KANTELE_PORT     the port
  KANTELE_NAME     the name devices show
  KANTELE_CAPTURE  a folder to write every client exchange to
  RUST_LOG         the log level

kantele.example.toml documents every key of the file beside its default.";

/// What the command line asked to be told, where it asked for that instead of a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Asked {
    Help,
    Version,
}

/// What the command line asked for, which is the last word on all of it.
#[derive(Debug, Default)]
struct Arguments {
    content_dir: Option<PathBuf>,
    config: Option<PathBuf>,
    album_list: Option<PathBuf>,
    report_only: bool,
    start: Start,
    asked: Option<Asked>,
}

impl Arguments {
    fn parse(arguments: &[String]) -> Result<Self> {
        let mut parsed = Self::default();
        let mut rest = arguments.iter();
        while let Some(argument) = rest.next() {
            match argument.as_str() {
                "--help" | "-h" => parsed.asked = Some(Asked::Help),
                "--version" | "-V" => parsed.asked = Some(Asked::Version),
                "--report" => parsed.report_only = true,
                "--no-scan" => parsed.start = Start::Remembered,
                "--reread" => parsed.start = Start::Reread,
                "--config" => parsed.config = Some(path(&mut rest, "--config")?),
                "--albums" => {
                    parsed.album_list = Some(path(&mut rest, "--albums")?);
                    parsed.report_only = true;
                }
                other if other.starts_with("--") => {
                    anyhow::bail!("unknown option {other}\nkantele --help lists the options")
                }
                folder if parsed.content_dir.is_none() => {
                    parsed.content_dir = Some(PathBuf::from(folder));
                }
                extra => {
                    anyhow::bail!(
                        "the command line names one folder, and {extra} is a second; several go \
                         in the configuration file as a list under content_dir"
                    )
                }
            }
        }
        Ok(parsed)
    }
}

/// The path an option was given, or the error naming the option that wanted one.
fn path<'a>(rest: &mut impl Iterator<Item = &'a String>, option: &str) -> Result<PathBuf> {
    rest.next()
        .map(PathBuf::from)
        .with_context(|| format!("{option} needs a path"))
}

/// The configuration this run uses: the file named here or by the environment, then the rest.
fn configure(arguments: &Arguments, from_env: Option<PathBuf>) -> Result<Resolved> {
    let path = arguments.config.clone().or(from_env);
    Resolved::load(path.as_deref(), arguments.content_dir.clone())
}

#[tokio::main]
async fn main() -> Result<()> {
    let arguments = Arguments::parse(&std::env::args().skip(1).collect::<Vec<_>>())?;
    match arguments.asked {
        Some(Asked::Help) => {
            println!("{USAGE}\n\n{HELP}");
            return Ok(());
        }
        Some(Asked::Version) => {
            println!("kantele {}", env!("CARGO_PKG_VERSION"));
            return Ok(());
        }
        None => {}
    }

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "kantele=info".into()),
        )
        .init();

    let resolved = configure(&arguments, env_config_path())?;
    let config = resolved.config.clone();
    let indexing = Indexing::of_config(&config, Arc::default())?;
    let roots = indexing.roots.clone();
    // Nothing chosen yet is a state to serve, not one to refuse: the page opens on Settings and
    // the folder picker is there. A report has nothing to report on, so it still refuses.
    if roots.is_empty() {
        if arguments.report_only || arguments.album_list.is_some() {
            anyhow::bail!(
                "no music folder: name one on the command line or in a configuration file\n\
                 {USAGE}\nkantele --help lists the options"
            );
        }
        tracing::warn!("no music folder: open the page and choose one");
    }

    // Before the store, because two servers on one state folder share the persisted device
    // identity and then write the same index. Held for the whole run.
    let state_dir = config.state_dir();
    let _state_lock = match state::lock(&state_dir) {
        state::Lock::Taken(held) => Some(held),
        state::Lock::Busy(path) => anyhow::bail!(
            "another kantele is serving {}: two of them share a device identity and write the \
             same index. Give this one its own state folder, with state_dir or KANTELE_STATE.\n\
             The lock is {}",
            state_dir.display(),
            path.display()
        ),
        state::Lock::Skipped(error) => {
            tracing::warn!(
                %error, dir = %state_dir.display(),
                "the state folder was not locked: nothing stops a second server sharing it"
            );
            None
        }
    };

    let store_path = config::store_path(&state_dir);
    let mut store = match Store::open_or_replace(&store_path) {
        Ok(mut store) => {
            // With no folder chosen the store is left saying which library it holds, or a start
            // before anyone has chosen would drop what the last one wrote.
            if !roots.is_empty()
                && let Err(error) = store.serving(&roots)
            {
                tracing::warn!(%error, "the store was not told which library it is serving");
            }
            Some(store)
        }
        Err(error) => {
            tracing::warn!(
                path = %store_path.display(), %error,
                "no store: every file will be read on every start, and dates added are not kept"
            );
            None
        }
    };
    let remembered = match arguments.start {
        Start::Reread => None,
        _ if arguments.report_only => None,
        _ if roots.is_empty() => None,
        _ => service::remembered(&indexing, &mut store),
    };
    let verify = remembered.is_some() && arguments.start != Start::Remembered;
    let mut first_pass = None;
    let library = match remembered {
        Some(library) => library,
        None if roots.is_empty() => kantele::index::Library::default(),
        None => {
            if arguments.start == Start::Remembered {
                tracing::warn!(
                    "nothing is remembered about this folder, so it is walked despite --no-scan"
                );
            }
            let pass = if arguments.start == Start::Reread {
                Pass::Reread
            } else {
                Pass::Whole
            };
            let indexed = service::index(&indexing, &mut store, pass)
                .with_context(|| format!("scanning {}", roots.describe()))?;
            // This index is the one served from here, which nothing else is left to decide.
            first_pass = Some(kantele::service::PassReport {
                outcome: kantele::service::Outcome::Published,
                ..indexed.pass
            });
            indexed.library
        }
    };
    if library.is_empty() && !roots.is_empty() {
        tracing::warn!("nothing playable found: a player will show an empty folder");
    }
    if let Some(path) = &arguments.album_list {
        report::write_album_list(&library, path)
            .with_context(|| format!("writing {}", path.display()))?;
        println!(
            "wrote {} albums to {}",
            library.albums().len(),
            path.display()
        );
    }
    if arguments.report_only {
        report::report(&library);
        return Ok(());
    }

    let operation = Operation {
        store: store.is_some().then(|| store_path.clone()),
        config: resolved,
    };
    serve(
        library,
        indexing,
        store,
        &config,
        Started {
            operation,
            first_pass,
            verify,
            reread: arguments.start == Start::Reread,
        },
    )
    .await
}

/// What a start knows that the index does not, handed to the server in one piece.
struct Started {
    operation: Operation,
    /// The pass that built the index, where one ran rather than the store answering.
    first_pass: Option<kantele::service::PassReport>,
    verify: bool,
    reread: bool,
}

/// Everything from the index being ready to the process leaving.
async fn serve(
    library: kantele::index::Library,
    indexing: Indexing,
    mut store: Option<Store>,
    config: &Config,
    started: Started,
) -> Result<()> {
    let identity = DeviceIdentity {
        friendly_name: config.friendly_name(),
        icon: match config.icon() {
            Some(path) => Icon::read(&path),
            None => Icon::default(),
        },
        udn: store
            .as_mut()
            .and_then(|store| match store.device_udn() {
                Ok(udn) => Some(udn),
                Err(error) => {
                    tracing::warn!(%error, "the device identity was not persisted");
                    None
                }
            })
            .unwrap_or_else(|| stable_udn(&indexing.roots)),
    };
    tracing::info!(udn = %identity.udn, name = %identity.friendly_name, "device identity");

    let clients = Profiles::new(config.client_profiles());
    let server = Server::new(library, indexing.menus.clone(), identity, clients);
    server.control.set_operation(started.operation);
    if let Some(dir) = &config.capture_dir {
        let recorder = capture::Recorder::new(dir)
            .with_context(|| format!("creating the capture folder {}", dir.display()))?;
        tracing::info!(dir = %dir.display(), "capturing every control exchange");
        server.device.capture_to(recorder);
    }
    if let Some(pass) = started.first_pass {
        server.passes.record(pass);
    }
    server
        .device
        .resume_update_id(service::resumed_update_id(&mut store));
    let boot_id = service::next_boot_id(&mut store);
    let udn = server.device.identity.udn.clone();

    let address = SocketAddr::from((
        Ipv4Addr::UNSPECIFIED,
        config.http_port.unwrap_or(config::DEFAULT_PORT),
    ));
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .with_context(|| format!("binding {address}"))?;
    let bound = listener.local_addr()?;
    tracing::info!(%bound, "http listening");
    for interface in ssdp::local_addresses() {
        tracing::info!(
            "description at http://{interface}:{}/description.xml",
            bound.port()
        );
    }

    let underway = indexing.underway.clone();
    server.control.indexing.replace(indexing);
    tokio::spawn(service::keep_fresh(
        server.device.clone(),
        server.passes.clone(),
        server.control.indexing.clone(),
        store,
        started.verify,
        started.reread,
    ));

    let subscriptions = server.device.subscriptions.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            subscriptions.expire();
        }
    });

    // The page is easier to reach by name than by address, and a network answering no mDNS costs
    // the server nothing.
    let announced = match mdns::Announced::new(
        &server.device.identity.friendly_name,
        &hostname(),
        bound.port(),
        &ssdp::local_addresses(),
    ) {
        Ok(announced) => {
            tracing::info!(name = announced.fullname(), "the page is announced on mdns");
            Some(announced)
        }
        Err(error) => {
            tracing::warn!(%error, "the page is not announced on mdns");
            None
        }
    };

    let (stop_ssdp, ssdp_stopped) = tokio::sync::oneshot::channel();
    let advertiser = ssdp::Advertiser::new(udn, bound.port(), server.device.peers.clone(), boot_id);
    let discovery = tokio::spawn(async move {
        if let Err(error) = advertiser.run(ssdp_stopped).await {
            tracing::error!(%error, "ssdp stopped");
        }
    });

    let (leaving, left) = tokio::sync::oneshot::channel::<()>();
    let serving = axum::serve(
        listener,
        server::router(&server).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async move {
        let signal = asked_to_stop().await;
        // Before anything else.
        underway.stopping.stop();
        let _ = stop_ssdp.send(());
        let _ = leaving.send(());
        tracing::info!(signal, "shutting down");
    });
    tokio::select! {
        served = serving => served?,
        () = drained_or_left(left) => {}
    }

    if let Some(announced) = announced {
        announced.withdraw();
    }
    let _ = tokio::time::timeout(Duration::from_secs(2), discovery).await;
    Ok(())
}

/// How long a stop waits for open connections.
const DRAIN: Duration = Duration::from_secs(5);

/// Resolves once the stop signal has been followed by the drain allowance, and never otherwise.
async fn drained_or_left(left: tokio::sync::oneshot::Receiver<()>) {
    if left.await.is_err() {
        std::future::pending::<()>().await;
    }
    tokio::time::sleep(DRAIN).await;
    tracing::warn!(
        seconds = DRAIN.as_secs(),
        "connections still open after the drain allowance: leaving them"
    );
}

/// SIGTERM as well as an interrupt, so a service manager can stop this.
async fn asked_to_stop() -> &'static str {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(stream) => stream,
            Err(error) => {
                tracing::warn!(%error, "cannot listen for SIGTERM; only an interrupt will stop this");
                let _ = tokio::signal::ctrl_c().await;
                return "interrupt";
            }
        };
        tokio::select! {
            _ = tokio::signal::ctrl_c() => "interrupt",
            _ = terminate.recv() => "terminate",
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        "interrupt"
    }
}

/// A device identifier for a server with no store, derived rather than remembered.
fn stable_udn(roots: &Roots) -> String {
    let host = hostname();
    let seed = format!("kantele:{host}:{}", roots.describe());
    let digest = <sha2::Sha256 as sha2::Digest>::digest(seed.as_bytes());
    let bytes: [u8; 16] = digest[..16].try_into().expect("sha256 is longer than 16");
    uuid::Uuid::from_bytes(bytes).to_string()
}

/// The configuration file the environment names, which the command line outranks.
fn env_config_path() -> Option<PathBuf> {
    std::env::var_os("KANTELE_CONFIG")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

/// The machine's name, asked of the system: no service manager passes it in the environment.
fn hostname() -> String {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .or_else(|| {
            let output = std::process::Command::new("hostname").output().ok()?;
            String::from_utf8(output.stdout).ok()
        })
        .map(|name| name.trim().to_owned())
        .filter(|name| !name.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok())
        .unwrap_or_else(|| "kantele".to_owned())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    fn parse(arguments: &[&str]) -> Result<Arguments> {
        Arguments::parse(
            &arguments
                .iter()
                .map(|a| (*a).to_owned())
                .collect::<Vec<_>>(),
        )
    }

    #[test]
    fn a_folder_and_the_three_start_states() {
        let plain = parse(&["/music"]).expect("a folder is enough");
        assert_eq!(plain.content_dir.as_deref(), Some(Path::new("/music")));
        assert_eq!(plain.start, Start::Cached);
        assert!(!plain.report_only);

        assert_eq!(
            parse(&["/music", "--no-scan"]).expect("it parses").start,
            Start::Remembered
        );
        assert_eq!(
            parse(&["/music", "--reread"]).expect("it parses").start,
            Start::Reread
        );
    }

    #[test]
    fn a_question_about_the_binary_is_answered_instead_of_serving() {
        for spelling in ["--help", "-h"] {
            assert_eq!(
                parse(&[spelling]).expect("it parses").asked,
                Some(Asked::Help),
                "{spelling}"
            );
        }
        for spelling in ["--version", "-V"] {
            assert_eq!(
                parse(&[spelling]).expect("it parses").asked,
                Some(Asked::Version),
                "{spelling}"
            );
        }
        assert_eq!(
            parse(&["/music"]).expect("it parses").asked,
            None,
            "a folder is a server"
        );
    }

    #[test]
    fn the_help_names_every_option_the_command_line_takes() {
        for option in [
            "--config",
            "--no-scan",
            "--reread",
            "--report",
            "--albums",
            "--version",
            "--help",
        ] {
            assert!(
                HELP.contains(option),
                "{option} is taken and nowhere in the help"
            );
        }
    }

    #[test]
    fn writing_an_album_list_serves_nothing() {
        let parsed = parse(&["/music", "--albums", "/tmp/list.tsv"]).expect("it parses");
        assert_eq!(
            parsed.album_list.as_deref(),
            Some(Path::new("/tmp/list.tsv"))
        );
        assert!(
            parsed.report_only,
            "a run that writes a list is a report rather than a server"
        );
    }

    #[test]
    fn an_option_that_needs_a_path_says_so_rather_than_swallowing_the_folder() {
        assert!(parse(&["--config"]).is_err());
        assert!(parse(&["--albums"]).is_err());
        let ordered = parse(&["--config", "/etc/kantele.toml", "/music"]).expect("it parses");
        assert_eq!(
            ordered.config.as_deref(),
            Some(Path::new("/etc/kantele.toml"))
        );
        assert_eq!(ordered.content_dir.as_deref(), Some(Path::new("/music")));
    }

    #[test]
    fn what_this_server_will_not_be_asked_to_do() {
        assert!(parse(&["--nonsense"]).is_err(), "an unknown option");
        assert!(
            parse(&["/music", "/other"]).is_err(),
            "one folder is served at a time"
        );
    }

    #[test]
    fn the_command_line_has_the_last_word_on_which_folder_is_served() {
        let file = std::env::temp_dir().join("kantele-main-config.toml");
        std::fs::write(&file, "content_dir = \"/from/the/file\"\n").expect("writing it");
        let named = configure(
            &parse(&["--config", &file.display().to_string()]).expect("it parses"),
            None,
        )
        .expect("the file loads");
        assert_eq!(
            named.config.content_dirs(),
            [PathBuf::from("/from/the/file")]
        );
        assert_eq!(named.file_path(), Some(file.as_path()));
        assert_eq!(source_of(&named, "content_dir").as_str(), "file");

        let overridden = configure(
            &parse(&[
                "--config",
                &file.display().to_string(),
                "/from/the/command/line",
            ])
            .expect("it parses"),
            None,
        )
        .expect("the file loads");
        assert_eq!(
            overridden.config.content_dirs(),
            [PathBuf::from("/from/the/command/line")]
        );
        assert_eq!(
            source_of(&overridden, "content_dir").as_str(),
            "command line",
            "and the answer says which layer won"
        );
        let _ = std::fs::remove_file(&file);
    }

    fn source_of(resolved: &Resolved, key: &str) -> kantele::config::Source {
        kantele::api::describe::effective(resolved)
            .settings
            .iter()
            .find(|setting| setting.key == key)
            .map(|setting| setting.source.clone())
            .expect("every key is answered for")
    }

    #[test]
    fn a_value_nobody_set_says_it_came_from_this_server() {
        let said = configure(&parse(&["/music"]).expect("it parses"), None).expect("no file");
        assert_eq!(said.file_path(), None);
        assert_eq!(source_of(&said, "scan.threads").as_str(), "default");
        assert_eq!(source_of(&said, "content_dir").as_str(), "command line");
    }

    #[test]
    fn a_configuration_file_that_is_not_there_is_an_error_rather_than_a_default() {
        let missing = parse(&["--config", "/no/such/kantele.toml"]).expect("it parses");
        assert!(configure(&missing, None).is_err());
    }

    #[test]
    fn the_machine_names_itself_without_help_from_the_environment() {
        let name = hostname();
        assert!(!name.is_empty());
        assert_ne!(
            name, "kantele",
            "a name of its own, rather than the one every storeless server would share"
        );
    }

    #[test]
    fn a_server_with_no_store_keeps_its_identity_across_restarts() {
        let one = stable_udn(&Roots::one("/music"));
        assert_eq!(one, stable_udn(&Roots::one("/music")));
        assert_ne!(
            one,
            stable_udn(&Roots::one("/other")),
            "two instances serving different folders must not collide"
        );
        assert_eq!(one.len(), 36, "a uuid: {one}");
    }
}
