//! The built binary, started on a folder and asked over a socket what a device and the page ask.
//! A stop here is SIGTERM, the way a service manager asks, and Windows has no equivalent to send a
//! child, so the whole file is a Unix one.
#![cfg(unix)]

use std::io::{Read, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

mod fixtures;
use fixtures::{Tree, flac};

/// How long a start, a stop or a read is given before the test calls it a failure.
const PATIENCE: Duration = Duration::from_secs(30);

static STARTS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// A folder outside the library, since the store must never be written inside it.
struct Scratch(PathBuf);

impl Scratch {
    fn new(name: &str) -> Self {
        let path =
            std::env::temp_dir().join(format!("kantele-{}-{name}-state", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).expect("creating the state folder");
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The binary in its own process, on a port the system chose and a state folder of its own.
struct Running {
    child: Child,
    port: u16,
    log: PathBuf,
}

impl Running {
    fn start(tree: &Tree, state: &Scratch, arguments: &[&str]) -> Self {
        Self::start_on(&tree.0, state, arguments)
    }

    fn start_on(folder: &std::path::Path, state: &Scratch, arguments: &[&str]) -> Self {
        let state = &state.0;
        std::fs::create_dir_all(state).expect("creating the state folder");
        let nth = STARTS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let log = state.join(format!("run-{nth}.log"));
        let file = std::fs::File::create(&log).expect("creating the log");
        let also = file.try_clone().expect("the same log twice");
        let child = Command::new(env!("CARGO_BIN_EXE_kantele"))
            .arg(folder)
            .args(arguments)
            .env("KANTELE_STATE", state)
            .env("KANTELE_PORT", "0")
            .env("RUST_LOG", "kantele=info")
            .stdin(Stdio::null())
            .stdout(file)
            .stderr(also)
            .spawn()
            .expect("the built binary starts");
        let mut running = Self {
            child,
            port: 0,
            log,
        };
        running.port = running.wait_for_port();
        running
    }

    /// The port out of the line the server writes when it binds, which is the only way to know.
    fn wait_for_port(&mut self) -> u16 {
        let deadline = Instant::now() + PATIENCE;
        while Instant::now() < deadline {
            if let Some(port) = bound_port(&self.said()) {
                return port;
            }
            if let Some(status) = self.child.try_wait().expect("asking after the process") {
                panic!("the server left before it bound: {status}\n{}", self.said());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("no port after {PATIENCE:?}:\n{}", self.said());
    }

    /// The server binds before it walks, so what the page says about the library arrives a moment
    /// after the port does.
    fn wait_for_tracks(&self, tracks: u64) -> serde_json::Value {
        let deadline = Instant::now() + PATIENCE;
        let mut last = serde_json::Value::Null;
        while Instant::now() < deadline {
            let json = ask(
                self.port,
                "GET",
                "/api/status",
                &[("Accept", "application/json")],
                b"",
            );
            if json.status == 200 {
                last = serde_json::from_slice(&json.body).expect("json");
                if last["library"]["tracks"] == tracks {
                    return last;
                }
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!(
            "{tracks} tracks never published in {PATIENCE:?}: {last}\n{}",
            self.said()
        );
    }

    fn said(&self) -> String {
        std::fs::read_to_string(&self.log).unwrap_or_default()
    }

    /// A stop asked for the way a service manager asks, and how long the process took to go.
    fn stop(mut self) -> Duration {
        let asked = Instant::now();
        // SAFETY: the pid is this child's, and the process is still ours to signal.
        unsafe { libc::kill(self.child.id() as libc::pid_t, libc::SIGTERM) };
        let deadline = asked + PATIENCE;
        while Instant::now() < deadline {
            if self
                .child
                .try_wait()
                .expect("asking after the process")
                .is_some()
            {
                return asked.elapsed();
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        panic!("still running {PATIENCE:?} after SIGTERM:\n{}", self.said());
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The binary run to its end on one folder, and the log it wrote, byte for byte.
fn ran(tree: &Tree, state: &Scratch, arguments: &[&str], port: &str) -> String {
    let log = state.0.join("ran.log");
    let file = std::fs::File::create(&log).expect("creating the log");
    let also = file.try_clone().expect("the same log twice");
    let status = Command::new(env!("CARGO_BIN_EXE_kantele"))
        .arg(&tree.0)
        .args(arguments)
        .env("KANTELE_STATE", &state.0)
        .env("KANTELE_PORT", port)
        .env("RUST_LOG", "kantele=info")
        .stdin(Stdio::null())
        .stdout(file)
        .stderr(also)
        .status()
        .expect("the built binary runs");
    let said = std::fs::read_to_string(&log).expect("the log");
    assert!(status.success(), "{status}\n{said}");
    said
}

fn bound_port(log: &str) -> Option<u16> {
    let at = log.find("bound=")? + "bound=".len();
    let rest = &log[at..];
    let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
    rest[..end].rsplit(':').next()?.parse().ok()
}

/// What came back: the status, the headers, and the bytes.
struct Answer {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Answer {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(held, _)| held.eq_ignore_ascii_case(name))
            .map(|(_, value)| value.as_str())
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

/// One request over one connection, closed by the server, so the read to the end is the answer.
fn ask(port: u16, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Answer {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connecting");
    stream
        .set_read_timeout(Some(PATIENCE))
        .expect("a read deadline");
    let mut request = format!(
        "{method} {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\
         Content-Length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    stream.write_all(request.as_bytes()).expect("writing it");
    stream.write_all(body).expect("writing the body");
    stream.flush().expect("flushing");

    let mut read = Vec::new();
    stream.read_to_end(&mut read).expect("reading the answer");
    parse(&read)
}

fn parse(answer: &[u8]) -> Answer {
    let split = answer
        .windows(4)
        .position(|four| four == b"\r\n\r\n")
        .expect("headers end");
    let head = String::from_utf8_lossy(&answer[..split]).into_owned();
    let mut lines = head.split("\r\n");
    let status = lines
        .next()
        .and_then(|line| line.split(' ').nth(1))
        .and_then(|code| code.parse().ok())
        .expect("a status line");
    let headers = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(name, value)| (name.to_owned(), value.trim().to_owned()))
        .collect();
    Answer {
        status,
        headers,
        body: answer[split + 4..].to_vec(),
    }
}

/// A SOAP call shaped the way a control point sends one.
fn soap(port: u16, action: &str, arguments: &str) -> Answer {
    let body = format!(
        r#"<?xml version="1.0"?><s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/">
<s:Body><u:{action} xmlns:u="urn:schemas-upnp-org:service:ContentDirectory:1">{arguments}</u:{action}>
</s:Body></s:Envelope>"#
    );
    ask(
        port,
        "POST",
        "/control/ContentDirectory",
        &[
            ("Content-Type", "text/xml; charset=\"utf-8\""),
            (
                "SOAPAction",
                "\"urn:schemas-upnp-org:service:ContentDirectory:1#Browse\"",
            ),
        ],
        body.as_bytes(),
    )
}

fn browse(port: u16, object: &str, count: usize) -> Answer {
    soap(
        port,
        "Browse",
        &format!(
            "<ObjectID>{object}</ObjectID><BrowseFlag>BrowseDirectChildren</BrowseFlag>\
             <Filter>*</Filter><StartingIndex>0</StartingIndex>\
             <RequestedCount>{count}</RequestedCount><SortCriteria></SortCriteria>"
        ),
    )
}

/// An album of three tagged files, which is enough for every menu to hold something.
fn library(name: &str) -> Tree {
    let tree = Tree::new(name);
    let folder = tree.path("Sierra Maestra/Dundunbanza");
    std::fs::create_dir_all(&folder).expect("creating the album folder");
    for track in 1..=3 {
        let file = flac(
            &[
                ("ARTIST", "Sierra Maestra"),
                ("ALBUMARTIST", "Sierra Maestra"),
                ("ALBUM", "Dundunbanza"),
                ("GENRE", "Son"),
                ("DATE", "1994"),
                ("TITLE", &format!("Track {track}")),
                ("TRACKNUMBER", &track.to_string()),
            ],
            true,
        );
        std::fs::write(folder.join(format!("0{track}.flac")), file).expect("writing a flac");
    }
    tree
}

/// The media path of the first track a flat browse offers.
fn first_media_path(port: u16) -> String {
    let answered = browse(port, "music", 1).text();
    let at = answered.find("/media/").expect("a media url");
    let rest = &answered[at..];
    let end = rest.find(['&', '<', '"']).unwrap_or(rest.len());
    rest[..end].to_owned()
}

#[test]
fn a_control_point_is_answered_by_the_binary_over_a_socket() {
    let tree = library("e2e-wire");
    let state = Scratch::new("e2e-wire");
    let server = Running::start(&tree, &state, &[]);

    let description = ask(server.port, "GET", "/description.xml", &[], b"");
    assert_eq!(description.status, 200);
    let said = description.text();
    assert!(
        said.contains("urn:schemas-upnp-org:device:MediaServer:1"),
        "{said}"
    );
    assert!(said.contains("ContentDirectory"), "{said}");

    let root = browse(server.port, "0", 50);
    assert_eq!(root.status, 200);
    assert!(
        root.text().contains("&lt;container"),
        "the root offers menus"
    );

    // The port opens before the walk, so the tracks arrive a moment after the menus do.
    server.wait_for_tracks(3);
    let media = first_media_path(server.port);
    let whole = ask(server.port, "GET", &media, &[], b"");
    assert_eq!(whole.status, 200);
    assert_eq!(&whole.body[..4], b"fLaC", "the bytes of the file itself");
    assert_eq!(
        whole.header("content-length").and_then(|l| l.parse().ok()),
        Some(whole.body.len()),
        "a renderer sizes its buffer on this"
    );

    let ranged = ask(server.port, "GET", &media, &[("Range", "bytes=0-9")], b"");
    assert_eq!(ranged.status, 206, "a seek is a partial answer");
    assert_eq!(ranged.body.len(), 10);
    assert_eq!(ranged.body, whole.body[..10]);
    assert!(
        ranged
            .header("content-range")
            .is_some_and(|r| r.starts_with("bytes 0-9/")),
        "the answer says which bytes these are"
    );

    server.stop();
}

#[test]
fn the_page_and_the_control_interface_are_served_by_the_binary() {
    let tree = library("e2e-page");
    let state = Scratch::new("e2e-page");
    let server = Running::start(&tree, &state, &[]);

    let page = ask(server.port, "GET", "/config", &[], b"");
    assert_eq!(page.status, 200);
    assert!(
        page.header("content-type")
            .is_some_and(|t| t.contains("text/html")),
        "a browser opens this"
    );

    let answered = server.wait_for_tracks(3);
    assert_eq!(answered["library"]["albums"], 1);
    assert_eq!(answered["store"]["open"], true);

    // The pass that published is written down a moment after the library it published appears.
    let deadline = Instant::now() + PATIENCE;
    let last = loop {
        let status = ask(
            server.port,
            "GET",
            "/api/status",
            &[("Accept", "application/json")],
            b"",
        );
        let json: serde_json::Value = serde_json::from_slice(&status.body).expect("json");
        if !json["last_pass"].is_null() {
            break json["last_pass"].clone();
        }
        assert!(Instant::now() < deadline, "no pass written down: {json}");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(
        last["outcome"], "published",
        "the page says what became of the index, not only that a pass ran: {last}"
    );
    assert_eq!(last["published"], true);

    let plain = ask(server.port, "GET", "/api/status", &[], b"");
    assert_eq!(plain.status, 200);
    assert!(
        plain.text().lines().any(|line| line == "tracks = 3"),
        "the shell on a NAS greps this:\n{}",
        plain.text()
    );

    let asked = ask(server.port, "POST", "/api/rescan", &[], b"");
    assert_eq!(asked.status, 202, "a pass is queued rather than run here");

    server.stop();
}

#[test]
fn what_the_first_start_wrote_down_is_what_the_second_start_serves() {
    let tree = library("e2e-restart");
    let state = Scratch::new("e2e-restart");

    let first = Running::start(&tree, &state, &[]);
    let before = first.wait_for_tracks(3);
    let took = first.stop();
    assert!(
        took < PATIENCE,
        "a DSM stop script reads a slow stop as a hang: {took:?}"
    );

    let second = Running::start(&tree, &state, &["--no-scan"]);
    let after: serde_json::Value = serde_json::from_slice(
        &ask(
            second.port,
            "GET",
            "/api/status",
            &[("Accept", "application/json")],
            b"",
        )
        .body,
    )
    .expect("json");
    assert_eq!(
        after["library"], before["library"],
        "the store answers what the walk found"
    );
    assert_eq!(
        after["udn"], before["udn"],
        "a device that moves is a device every client forgets"
    );
    assert_eq!(
        after["system_update_id"], before["system_update_id"],
        "a restart that bumps this is every client reloading a library nobody touched"
    );

    second.stop();
}

#[test]
fn the_log_a_service_manager_collects_carries_no_colour() {
    let tree = library("e2e-colour");
    let state = Scratch::new("e2e-colour");
    let said = ran(&tree, &state, &["--report"], "0");
    assert!(
        !said.is_empty(),
        "nothing was logged, so nothing was checked"
    );
    assert!(
        !said.contains('\u{1b}'),
        "escape codes in a log file are in the way of every reader of it:\n{said}"
    );
}

#[test]
fn a_port_the_environment_names_badly_is_said_to_be_ignored() {
    let tree = library("e2e-port");
    let state = Scratch::new("e2e-port");
    let said = ran(&tree, &state, &["--report"], "82OO");
    assert!(
        said.contains("KANTELE_PORT is not a port number"),
        "a port nobody is listening on is not a silence: {said}"
    );
}

#[test]
fn a_music_folder_that_is_not_mounted_yet_does_not_stop_the_server_starting() {
    let tree = library("e2e-absent");
    let state = Scratch::new("e2e-absent");
    let absent = tree.0.join("not-mounted-yet");

    // Under launchd with KeepAlive, a start that exits here is a restart loop.
    let server = Running::start_on(&absent, &state, &[]);
    let status = ask(
        server.port,
        "GET",
        "/api/status",
        &[("Accept", "application/json")],
        b"",
    );
    assert_eq!(status.status, 200);
    let json: serde_json::Value = serde_json::from_slice(&status.body).expect("json");
    assert_eq!(json["library"]["tracks"], 0);
    assert!(
        server.said().contains("not there"),
        "and it says which folder it could not read:\n{}",
        server.said()
    );
    server.stop();
}

#[test]
fn a_server_under_a_service_manager_keeps_its_own_log_beside_the_index() {
    let tree = library("e2e-logfile");
    let state = Scratch::new("e2e-logfile");
    let server = Running::start(&tree, &state, &[]);
    server.wait_for_tracks(3);

    let kept = std::fs::read_to_string(kantele::log::path(&state.0)).expect("the server's own log");
    assert!(
        kept.contains("http listening"),
        "the line the port was announced on is in the file the page reads:\n{kept}"
    );
    assert!(!kept.contains('\u{1b}'), "no colour in a file:\n{kept}");

    let tail = ask(server.port, "GET", "/api/log?lines=5", &[], b"");
    assert_eq!(tail.status, 200);
    let shown = String::from_utf8_lossy(&tail.body);
    assert_eq!(shown.lines().count(), 5, "five asked, five given:\n{shown}");
    assert!(
        kept.ends_with(&*shown) || kept.contains(shown.lines().next().expect("a line")),
        "the tail is the end of the file"
    );
}
