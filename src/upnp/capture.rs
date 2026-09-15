//! Control exchanges written to disk verbatim, one folder per client, for replaying later.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

pub struct Recorder {
    dir: PathBuf,
    next: AtomicU32,
}

pub struct Exchange<'a> {
    pub peer: &'a str,
    pub method: &'a str,
    pub path: &'a str,
    pub headers: Vec<(String, String)>,
    pub body: &'a [u8],
    pub status: u16,
    pub response: &'a [u8],
}

impl Recorder {
    pub fn new(dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        let next = AtomicU32::new(last_sequence_under(&dir)? + 1);
        Ok(Self { dir, next })
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Writes `NNNNNN-<action>.request.txt` and `.response.xml` under the peer's folder.
    pub fn record(&self, exchange: &Exchange<'_>) -> std::io::Result<PathBuf> {
        let folder = self.dir.join(safe(exchange.peer));
        std::fs::create_dir_all(&folder)?;
        let sequence = self.next.fetch_add(1, Ordering::Relaxed);
        let stem = format!("{sequence:06}-{}", safe(&action_of(&exchange.headers)));

        let mut request = format!("{} {}\n", exchange.method, exchange.path).into_bytes();
        for (name, value) in &exchange.headers {
            request.extend_from_slice(format!("{name}: {value}\n").as_bytes());
        }
        request.push(b'\n');
        request.extend_from_slice(exchange.body);
        std::fs::write(folder.join(format!("{stem}.request.txt")), request)?;

        let response = folder.join(format!("{stem}.{}.response.xml", exchange.status));
        std::fs::write(&response, exchange.response)?;
        Ok(response)
    }
}

/// The highest sequence any peer's folder holds, so a restart carries on rather than overwriting.
fn last_sequence_under(dir: &Path) -> std::io::Result<u32> {
    let mut last = 0;
    for peer in std::fs::read_dir(dir)? {
        let peer = peer?.path();
        if !peer.is_dir() {
            continue;
        }
        for file in std::fs::read_dir(&peer)? {
            let name = file?.file_name();
            let sequence = name
                .to_str()
                .and_then(|name| name.split('-').next())
                .and_then(|digits| digits.parse::<u32>().ok());
            last = last.max(sequence.unwrap_or(0));
        }
    }
    Ok(last)
}

fn action_of(headers: &[(String, String)]) -> String {
    headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("soapaction"))
        .and_then(|(_, value)| value.trim().trim_matches('"').rsplit('#').next())
        .filter(|action| !action.is_empty())
        .unwrap_or("unknown")
        .to_owned()
}

fn safe(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("kantele-capture-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn browse<'a>(peer: &'a str, status: u16) -> Exchange<'a> {
        Exchange {
            peer,
            method: "POST",
            path: "/control/ContentDirectory",
            headers: vec![
                ("user-agent".to_owned(), "Denon-Heos".to_owned()),
                (
                    "soapaction".to_owned(),
                    "\"urn:schemas-upnp-org:service:ContentDirectory:1#Browse\"".to_owned(),
                ),
            ],
            body: b"<Browse/>",
            status,
            response: b"<Envelope/>",
        }
    }

    #[test]
    fn a_request_and_its_answer_land_under_the_peer_numbered_and_named_after_the_action() {
        let dir = scratch("names");
        let recorder = Recorder::new(&dir).expect("a folder");
        let written = recorder
            .record(&browse("192.0.2.219", 200))
            .expect("written");
        assert_eq!(
            written,
            dir.join("192.0.2.219")
                .join("000001-Browse.200.response.xml")
        );
        let request = std::fs::read_to_string(dir.join("192.0.2.219/000001-Browse.request.txt"))
            .expect("the request");
        assert_eq!(
            request,
            "POST /control/ContentDirectory\nuser-agent: Denon-Heos\n\
             soapaction: \"urn:schemas-upnp-org:service:ContentDirectory:1#Browse\"\n\n<Browse/>"
        );
        assert_eq!(
            std::fs::read(&written).expect("the response"),
            b"<Envelope/>"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_sequence_runs_across_peers_and_a_fault_shows_its_status_in_the_name() {
        let dir = scratch("sequence");
        let recorder = Recorder::new(&dir).expect("a folder");
        recorder.record(&browse("a", 200)).expect("first");
        let second = recorder.record(&browse("b", 500)).expect("second");
        assert_eq!(second, dir.join("b").join("000002-Browse.500.response.xml"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_restart_carries_the_sequence_on_rather_than_writing_over_the_last_run() {
        let dir = scratch("restart");
        Recorder::new(&dir)
            .expect("a folder")
            .record(&browse("a", 200))
            .expect("first run");
        let again = Recorder::new(&dir).expect("the same folder");
        let written = again.record(&browse("b", 200)).expect("second run");
        assert_eq!(
            written,
            dir.join("b").join("000002-Browse.200.response.xml")
        );
        assert!(
            dir.join("a/000001-Browse.request.txt").exists(),
            "what the first run captured is still there"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn without_a_soapaction_header_the_action_is_unknown_and_an_odd_peer_is_made_safe() {
        let dir = scratch("unknown");
        let recorder = Recorder::new(&dir).expect("a folder");
        let mut exchange = browse("fe80::1%en0", 200);
        exchange.headers.retain(|(name, _)| name != "soapaction");
        let written = recorder.record(&exchange).expect("written");
        assert_eq!(
            written,
            dir.join("fe80__1_en0")
                .join("000001-unknown.200.response.xml")
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
