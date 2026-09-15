//! SSDP discovery.

use std::io;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::time::Duration;

use socket2::{Domain, Protocol, Socket, Type};
use tokio::net::UdpSocket;

use crate::upnp::peers::{Peers, Step};

const MULTICAST: Ipv4Addr = Ipv4Addr::new(239, 255, 255, 250);
const PORT: u16 = 1900;
const MULTICAST_ADDR: SocketAddrV4 = SocketAddrV4::new(MULTICAST, PORT);

/// How long a client may cache an advertisement.
const MAX_AGE: Duration = Duration::from_secs(1800);

const LOOK_AGAIN: Duration = Duration::from_secs(15);

const SALVO: Duration = Duration::from_secs(1);

fn notification_types(udn: &str) -> Vec<(String, String)> {
    let uuid = format!("uuid:{udn}");
    vec![
        (
            "upnp:rootdevice".to_owned(),
            format!("{uuid}::upnp:rootdevice"),
        ),
        (uuid.clone(), uuid.clone()),
        (
            "urn:schemas-upnp-org:device:MediaServer:1".to_owned(),
            format!("{uuid}::urn:schemas-upnp-org:device:MediaServer:1"),
        ),
        (
            "urn:schemas-upnp-org:service:ContentDirectory:1".to_owned(),
            format!("{uuid}::urn:schemas-upnp-org:service:ContentDirectory:1"),
        ),
        (
            "urn:schemas-upnp-org:service:ConnectionManager:1".to_owned(),
            format!("{uuid}::urn:schemas-upnp-org:service:ConnectionManager:1"),
        ),
    ]
}

pub struct Advertiser {
    udn: String,
    http_port: u16,
    /// UPnP 1.1 requires `BOOTID` to increase every boot.
    boot_id: u32,
    peers: Peers,
}

impl Advertiser {
    pub fn new(udn: impl Into<String>, http_port: u16, peers: Peers) -> Self {
        Self {
            udn: udn.into(),
            http_port,
            boot_id: boot_id(),
            peers,
        }
    }

    pub async fn run(&self, shutdown: tokio::sync::oneshot::Receiver<()>) -> anyhow::Result<()> {
        let listener = bind_listener()?;
        let serving = std::sync::Mutex::new(Vec::new());

        let announce = self.announce_loop(&listener, &serving);
        let respond = self.respond_loop(&listener);

        tokio::select! {
            result = announce => result?,
            result = respond => result?,
            _ = shutdown => {}
        }

        let interfaces = serving.lock().map(|held| held.clone()).unwrap_or_default();
        self.byebye(&interfaces).await;
        Ok(())
    }

    async fn announce_loop(
        &self,
        listener: &UdpSocket,
        serving: &std::sync::Mutex<Vec<Ipv4Addr>>,
    ) -> anyhow::Result<()> {
        let mut known: Vec<Ipv4Addr> = Vec::new();
        let mut refreshed = tokio::time::Instant::now();
        loop {
            let (fresh, gone) = changes(&known, &local_addresses());
            if !gone.is_empty() {
                tracing::info!(interfaces = ?gone, "these interfaces went away");
                known.retain(|address| !gone.contains(address));
            }
            if !fresh.is_empty() {
                for address in &fresh {
                    if let Err(error) = join_group(listener, *address) {
                        tracing::warn!(%address, %error, "could not join the multicast group here");
                    }
                }
                known.extend_from_slice(&fresh);
                tracing::info!(interfaces = ?fresh, "advertising here");
                for salvo in 0..3 {
                    self.alive(&fresh).await;
                    tokio::time::sleep(self.spread(SALVO, salvo)).await;
                }
            }
            if let Ok(mut held) = serving.lock() {
                held.clone_from(&known);
            }
            if refreshed.elapsed() >= self.spread(MAX_AGE / 2, 3) {
                self.alive(&known).await;
                refreshed = tokio::time::Instant::now();
            }
            tokio::time::sleep(LOOK_AGAIN).await;
        }
    }

    async fn alive(&self, interfaces: &[Ipv4Addr]) {
        for address in interfaces {
            let location = self.location(*address);
            for (nt, usn) in notification_types(&self.udn) {
                send_from(
                    *address,
                    &self.notification("ssdp:alive", &nt, &usn, Some(&location)),
                )
                .await;
            }
        }
    }

    async fn byebye(&self, interfaces: &[Ipv4Addr]) {
        for address in interfaces {
            for (nt, usn) in notification_types(&self.udn) {
                send_from(*address, &self.notification("ssdp:byebye", &nt, &usn, None)).await;
            }
        }
        tracing::info!("ssdp byebye sent");
    }

    fn spread(&self, base: Duration, salvo: u32) -> Duration {
        let widest = base.as_millis() as u64 / 4;
        let of = u64::from(self.boot_id.rotate_right(salvo * 8) % 251);
        base + Duration::from_millis(widest * of / 250)
    }

    fn notification(&self, nts: &str, nt: &str, usn: &str, location: Option<&str>) -> String {
        let offered = match location {
            Some(location) => format!(
                "CACHE-CONTROL: max-age={age}\r\n\
                 LOCATION: {location}\r\n\
                 SERVER: {server}\r\n",
                age = MAX_AGE.as_secs(),
                server = server_header(),
            ),
            None => String::new(),
        };
        format!(
            "NOTIFY * HTTP/1.1\r\n\
             HOST: {MULTICAST}:{PORT}\r\n\
             {offered}\
             NT: {nt}\r\n\
             NTS: {nts}\r\n\
             USN: {usn}\r\n\
             BOOTID.UPNP.ORG: {boot}\r\n\
             CONFIGID.UPNP.ORG: 1\r\n\r\n",
            boot = self.boot_id,
        )
    }

    async fn respond_loop(&self, listener: &UdpSocket) -> anyhow::Result<()> {
        let mut buffer = vec![0_u8; 2048];
        loop {
            let (read, from) = match listener.recv_from(&mut buffer).await {
                Ok(received) => received,
                Err(error) => {
                    tracing::warn!(%error, "ssdp receive failed; listening on");
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
            };
            let request = String::from_utf8_lossy(&buffer[..read]);
            if !request.starts_with("M-SEARCH") {
                continue;
            }
            let Some(st) = header_value(&request, "ST") else {
                continue;
            };
            let answers = self.answers_to(&st);
            if answers.is_empty() {
                continue;
            }
            // Noted before the route back is known: a search this server cannot answer is the
            // case the table exists to show.
            self.peers.note(
                from.ip(),
                Step::Searched,
                header_value(&request, "USER-AGENT").as_deref(),
            );
            let Some(replies) = self.replies_to(answers, from) else {
                continue;
            };
            let delay = response_delay(header_value(&request, "MX").as_deref());
            tracing::debug!(%from, %st, replies = replies.len(), "answering M-SEARCH");
            tokio::spawn(async move {
                tokio::time::sleep(delay).await;
                if let Ok(socket) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).await {
                    for reply in &replies {
                        let _ = socket.send_to(reply.as_bytes(), from).await;
                    }
                }
            });
        }
    }

    fn replies_to(&self, answers: Vec<(String, String)>, from: SocketAddr) -> Option<Vec<String>> {
        let Some(address) = local_address_towards(from) else {
            tracing::warn!(%from, "no route back: not answering");
            return None;
        };
        let location = self.location(address);
        Some(
            answers
                .into_iter()
                .map(|(st, usn)| {
                    format!(
                        "HTTP/1.1 200 OK\r\n\
                         CACHE-CONTROL: max-age={age}\r\n\
                         EXT:\r\n\
                         LOCATION: {location}\r\n\
                         SERVER: {server}\r\n\
                         ST: {st}\r\n\
                         USN: {usn}\r\n\
                         BOOTID.UPNP.ORG: {boot}\r\n\
                         CONFIGID.UPNP.ORG: 1\r\n\r\n",
                        age = MAX_AGE.as_secs(),
                        server = server_header(),
                        boot = self.boot_id,
                    )
                })
                .collect(),
        )
    }

    fn answers_to(&self, search_target: &str) -> Vec<(String, String)> {
        let types = notification_types(&self.udn);
        if search_target == "ssdp:all" {
            return types;
        }
        types
            .into_iter()
            .filter(|(nt, _)| nt == search_target)
            .collect()
    }

    fn location(&self, address: Ipv4Addr) -> String {
        format!("http://{address}:{}/description.xml", self.http_port)
    }
}

fn bind_listener() -> io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    #[cfg(unix)]
    socket.set_reuse_port(true)?;
    socket.bind(&SocketAddr::from((Ipv4Addr::UNSPECIFIED, PORT)).into())?;
    socket.set_nonblocking(true)?;
    UdpSocket::from_std(socket.into())
}

fn join_group(listener: &UdpSocket, interface: Ipv4Addr) -> io::Result<()> {
    listener.join_multicast_v4(MULTICAST, interface)
}

async fn send_from(interface: Ipv4Addr, message: &str) {
    match sender_from(interface) {
        Ok(socket) => {
            if let Err(error) = socket.send_to(message.as_bytes(), MULTICAST_ADDR).await {
                tracing::warn!(%interface, %error, "ssdp send failed");
            }
        }
        Err(error) => tracing::warn!(%interface, %error, "cannot send from this interface"),
    }
}

/// A BSD kernel routes multicast by the default route unless the interface is set.
fn sender_from(interface: Ipv4Addr) -> io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.bind(&SocketAddr::from((interface, 0)).into())?;
    socket.set_multicast_if_v4(&interface)?;
    socket.set_multicast_ttl_v4(4)?;
    socket.set_nonblocking(true)?;
    UdpSocket::from_std(socket.into())
}

fn changes(known: &[Ipv4Addr], now: &[Ipv4Addr]) -> (Vec<Ipv4Addr>, Vec<Ipv4Addr>) {
    let missing = |from: &[Ipv4Addr], within: &[Ipv4Addr]| -> Vec<Ipv4Addr> {
        from.iter()
            .copied()
            .filter(|address| !within.contains(address))
            .collect()
    };
    (missing(now, known), missing(known, now))
}

pub fn local_addresses() -> Vec<Ipv4Addr> {
    if_addrs::get_if_addrs()
        .into_iter()
        .flatten()
        .filter(|interface| !interface.is_loopback())
        .filter_map(|interface| match interface.ip() {
            std::net::IpAddr::V4(address) => Some(address),
            std::net::IpAddr::V6(_) => None,
        })
        .collect()
}

fn local_address_towards(peer: SocketAddr) -> Option<Ipv4Addr> {
    let socket = std::net::UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect(peer).ok()?;
    match socket.local_addr().ok()? {
        SocketAddr::V4(address) if !address.ip().is_loopback() => Some(*address.ip()),
        _ => None,
    }
}

fn boot_id() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|since| since.as_secs() as u32)
        .unwrap_or(1)
}

fn server_header() -> String {
    format!(
        "{}/{} UPnP/1.0 Kantele/{}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        env!("CARGO_PKG_VERSION")
    )
}

fn header_value(message: &str, name: &str) -> Option<String> {
    message.lines().find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.trim()
            .eq_ignore_ascii_case(name)
            .then(|| value.trim().to_owned())
    })
}

fn response_delay(mx: Option<&str>) -> Duration {
    let seconds: u64 = mx.and_then(|v| v.trim().parse().ok()).unwrap_or(1);
    Duration::from_millis(seconds.clamp(1, 5) * 100)
}

#[cfg(test)]
mod tests {
    #[test]
    fn two_servers_do_not_announce_in_step_and_neither_waits_longer_than_it_should() {
        let one = Advertiser::new("uuid:one", 8200, Peers::default());
        let mut two = Advertiser::new("uuid:two", 8200, Peers::default());
        two.boot_id = one.boot_id.wrapping_add(7919);

        let base = Duration::from_secs(1);
        assert_ne!(one.spread(base, 0), two.spread(base, 0));
        for salvo in 0..4 {
            let waited = one.spread(base, salvo);
            assert!(waited >= base, "a repeat never comes sooner than intended");
            assert!(
                waited <= base + base / 4,
                "and never late enough to matter: {waited:?}"
            );
        }
    }

    #[test]
    fn an_interface_appearing_late_is_noticed_and_one_going_away_is_dropped() {
        let first = Ipv4Addr::new(192, 0, 2, 128);
        let second = Ipv4Addr::new(10, 0, 0, 4);

        let (fresh, gone) = changes(&[], &[first]);
        assert_eq!(fresh, vec![first]);
        assert!(gone.is_empty());

        let (fresh, gone) = changes(&[first], &[first, second]);
        assert_eq!(fresh, vec![second], "only the new one is announced on");
        assert!(gone.is_empty());

        let (fresh, gone) = changes(&[first, second], &[second]);
        assert!(fresh.is_empty());
        assert_eq!(gone, vec![first]);

        assert_eq!(changes(&[first], &[first]), (Vec::new(), Vec::new()));
    }

    use super::*;

    const UDN: &str = "3d5d1cbe-8f2a-4d1e-9a9c-7c2f0a1b2c3d";

    #[tokio::test]
    async fn a_sender_names_the_interface_its_multicast_leaves_by() {
        let socket = sender_from(Ipv4Addr::LOCALHOST).expect("a socket on the loopback");
        assert_eq!(
            socket.local_addr().expect("bound").ip(),
            std::net::IpAddr::V4(Ipv4Addr::LOCALHOST)
        );
    }

    #[test]
    fn headers_are_matched_regardless_of_case() {
        let message =
            "M-SEARCH * HTTP/1.1\r\nHost: 239.255.255.250:1900\r\nst: ssdp:all\r\nMX: 3\r\n\r\n";
        assert_eq!(header_value(message, "ST").as_deref(), Some("ssdp:all"));
        assert_eq!(header_value(message, "mx").as_deref(), Some("3"));
        assert_eq!(header_value(message, "Absent"), None);
    }

    #[test]
    fn a_media_server_advertises_five_things() {
        let types = notification_types(UDN);
        assert_eq!(types.len(), 5);
        assert!(types.iter().any(|(nt, _)| nt == "upnp:rootdevice"));
        assert!(
            types
                .iter()
                .any(|(nt, _)| nt == "urn:schemas-upnp-org:device:MediaServer:1")
        );
    }

    #[test]
    fn searches_for_us_are_answered_and_others_are_not() {
        let advertiser = Advertiser::new(UDN, 8200, Peers::default());
        for target in [
            "upnp:rootdevice",
            "urn:schemas-upnp-org:device:MediaServer:1",
            "urn:schemas-upnp-org:service:ContentDirectory:1",
        ] {
            assert_eq!(
                advertiser.answers_to(target).len(),
                1,
                "{target} should be answered once"
            );
        }
        assert!(
            advertiser
                .answers_to("urn:schemas-upnp-org:device:MediaRenderer:1")
                .is_empty(),
            "we are not a renderer"
        );
    }

    #[test]
    fn a_search_for_everything_is_answered_once_per_notification_type() {
        let advertiser = Advertiser::new(UDN, 8200, Peers::default());
        let answers = advertiser.answers_to("ssdp:all");
        assert_eq!(answers.len(), notification_types(UDN).len());
        assert!(
            answers.iter().all(|(st, _)| st != "ssdp:all"),
            "a response names the target it answers for, never the wildcard"
        );
    }

    #[test]
    fn the_usn_always_carries_the_uuid() {
        let advertiser = Advertiser::new(UDN, 8200, Peers::default());
        for (_, usn) in advertiser.answers_to("ssdp:all") {
            assert!(usn.starts_with(&format!("uuid:{UDN}")), "{usn}");
        }
    }

    #[test]
    fn the_advertised_location_is_never_loopback() {
        assert!(local_addresses().iter().all(|a| !a.is_loopback()));
    }

    #[test]
    fn an_advertisement_carries_what_a_control_point_needs_to_fetch_the_description() {
        let advertiser = Advertiser::new(UDN, 8200, Peers::default());
        let alive = advertiser.notification(
            "ssdp:alive",
            "upnp:rootdevice",
            &format!("uuid:{UDN}::upnp:rootdevice"),
            Some("http://192.0.2.1:8200/description.xml"),
        );
        for expected in [
            "NOTIFY * HTTP/1.1\r\n",
            "HOST: 239.255.255.250:1900\r\n",
            "NTS: ssdp:alive\r\n",
            "NT: upnp:rootdevice\r\n",
            "LOCATION: http://192.0.2.1:8200/description.xml\r\n",
            "CACHE-CONTROL: max-age=1800\r\n",
            "CONFIGID.UPNP.ORG: 1\r\n",
        ] {
            assert!(alive.contains(expected), "missing {expected:?} in {alive}");
        }
        assert!(
            alive.contains(&format!("BOOTID.UPNP.ORG: {}\r\n", advertiser.boot_id)),
            "UPnP 1.1 requires it to increase per boot: {alive}"
        );
        assert!(alive.ends_with("\r\n\r\n"), "the headers end: {alive:?}");
    }

    #[test]
    fn a_withdrawal_offers_nothing_to_fetch() {
        let advertiser = Advertiser::new(UDN, 8200, Peers::default());
        let byebye = advertiser.notification(
            "ssdp:byebye",
            "upnp:rootdevice",
            &format!("uuid:{UDN}::upnp:rootdevice"),
            None,
        );
        assert!(byebye.contains("NTS: ssdp:byebye\r\n"), "{byebye}");
        assert!(!byebye.contains("LOCATION"), "{byebye}");
        assert!(!byebye.contains("CACHE-CONTROL"), "{byebye}");
    }

    #[test]
    fn the_advertised_location_names_the_port_the_server_bound() {
        let advertiser = Advertiser::new(UDN, 8201, Peers::default());
        assert_eq!(
            advertiser.location(Ipv4Addr::new(192, 0, 2, 1)),
            "http://192.0.2.1:8201/description.xml"
        );
    }

    #[test]
    fn the_server_header_is_honest_about_what_this_is() {
        let header = server_header();
        assert!(header.contains("UPnP/1.0"), "{header}");
        assert!(header.contains("Kantele/"), "{header}");
    }

    #[test]
    fn the_response_delay_stays_inside_the_searchers_window() {
        assert_eq!(response_delay(Some("3")), Duration::from_millis(300));
        assert_eq!(response_delay(None), Duration::from_millis(100));
        assert_eq!(response_delay(Some("120")), Duration::from_millis(500));
        assert_eq!(response_delay(Some("nonsense")), Duration::from_millis(100));
    }
}
