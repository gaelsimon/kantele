//! GENA eventing.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(1800);
const MAX_TIMEOUT: Duration = Duration::from_secs(3600);
const MAX_CALLBACKS: usize = 2;
const MAX_PER_PEER: usize = 16;
/// Every device on the network together. Nothing else bounds the number of peers, and a round of
/// events walks all of them.
const MAX_SUBSCRIPTIONS: usize = 256;
/// Deliveries that failed in a row before a subscriber is dropped. One failure is a renderer on
/// wi-fi missing a connect, and dropping it there costs it every event until its own renewal.
const MISSED: u8 = 3;
const DELIVERIES_AT_ONCE: usize = 8;
/// A renderer on the same network answers in milliseconds, and one that never does holds a
/// delivery slot for this long.
const ANSWER_WITHIN: Duration = Duration::from_millis(300);
/// What a whole round of events may cost the pass that publishes the index it announces.
const ROUND_WITHIN: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Service {
    ContentDirectory,
    ConnectionManager,
}

impl Service {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ContentDirectory => "ContentDirectory",
            Self::ConnectionManager => "ConnectionManager",
        }
    }
}

#[derive(Clone, Debug)]
struct Subscription {
    service: Service,
    callbacks: Vec<String>,
    expires: Instant,
    seq: u32,
    user_agent: Option<String>,
    peer: IpAddr,
    since: u64,
    missed: u8,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Listed {
    pub sid: String,
    pub service: &'static str,
    pub callback: Option<String>,
    pub user_agent: Option<String>,
    pub renews_in_seconds: u64,
    pub events: u32,
}

#[derive(Clone, Default)]
pub struct Subscriptions {
    inner: Arc<Mutex<HashMap<String, Subscription>>>,
}

pub struct Granted {
    pub sid: String,
    pub timeout: Duration,
}

#[derive(Debug, PartialEq, Eq, thiserror::Error)]
pub enum Refused {
    #[error("CALLBACK names no HTTP URL")]
    NoCallback,
    #[error("CALLBACK names a host other than the one subscribing, and events go nowhere else")]
    CallbackElsewhere,
    #[error("the transport gave no address for the subscriber")]
    PeerUnknown,
    #[error("this server keeps as many subscriptions as it will")]
    TooMany,
}

impl Subscriptions {
    pub fn subscribe(
        &self,
        service: Service,
        callback: &str,
        peer: Option<IpAddr>,
        timeout: Option<&str>,
        user_agent: Option<String>,
    ) -> Result<Granted, Refused> {
        let peer = peer.ok_or(Refused::PeerUnknown)?;
        let callbacks = callbacks_at(callback, peer)?;
        let timeout = parse_timeout(timeout);
        let sid = format!("uuid:{}", uuid::Uuid::new_v4());
        let mut held = crate::held(&self.inner);
        let now = Instant::now();
        held.retain(|_, s| s.expires > now);
        make_room(&mut held, peer);
        if held.len() >= MAX_SUBSCRIPTIONS {
            tracing::warn!(
                held = held.len(), %peer,
                "as many subscriptions as this server keeps: refusing another"
            );
            return Err(Refused::TooMany);
        }
        let since = held.values().map(|s| s.since).max().map_or(0, |n| n + 1);
        held.insert(
            sid.clone(),
            Subscription {
                service,
                callbacks,
                expires: Instant::now() + timeout,
                // Zero is the initial event's, sent from a task that may run after a change.
                seq: 1,
                user_agent,
                peer,
                since,
                missed: 0,
            },
        );
        Ok(Granted { sid, timeout })
    }

    pub fn listed(&self) -> Vec<Listed> {
        let now = Instant::now();
        let mut listed: Vec<Listed> = crate::held(&self.inner)
            .iter()
            .map(|(sid, subscription)| Listed {
                sid: sid.clone(),
                service: subscription.service.as_str(),
                callback: subscription.callbacks.first().cloned(),
                user_agent: subscription.user_agent.clone(),
                renews_in_seconds: subscription
                    .expires
                    .saturating_duration_since(now)
                    .as_secs(),
                events: subscription.seq,
            })
            .collect();
        listed.sort_by(|left, right| left.sid.cmp(&right.sid));
        listed
    }

    /// `peer` is the address the renewal came from: a subscription is renewed by the device that
    /// took it and by nobody else.
    pub fn renew(&self, sid: &str, peer: Option<IpAddr>, timeout: Option<&str>) -> Option<Granted> {
        let timeout = parse_timeout(timeout);
        let mut guard = crate::held(&self.inner);
        let subscription = guard.get_mut(sid)?;
        if peer.is_some_and(|asking| asking != subscription.peer) {
            tracing::warn!(%sid, "a renewal from another address than the one that subscribed");
            return None;
        }
        subscription.expires = Instant::now() + timeout;
        subscription.missed = 0;
        Some(Granted {
            sid: sid.to_owned(),
            timeout,
        })
    }

    pub fn unsubscribe(&self, sid: &str) -> bool {
        crate::held(&self.inner).remove(sid).is_some()
    }

    pub fn expire(&self) {
        let now = Instant::now();
        crate::held(&self.inner).retain(|_, s| s.expires > now);
    }

    pub fn len(&self) -> usize {
        crate::held(&self.inner).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn next(&self, sid: &str) -> Option<(Vec<String>, u32)> {
        let mut guard = crate::held(&self.inner);
        let subscription = guard.get_mut(sid)?;
        let seq = subscription.seq;
        subscription.seq = subscription.seq.wrapping_add(1).max(1);
        Some((subscription.callbacks.clone(), seq))
    }

    fn subscribers_of(&self, service: Service) -> Vec<String> {
        crate::held(&self.inner)
            .iter()
            .filter(|(_, s)| s.service == service)
            .map(|(sid, _)| sid.clone())
            .collect()
    }

    pub async fn send_initial(&self, sid: &str, properties: &[(&str, String)]) {
        let callbacks = crate::held(&self.inner)
            .get(sid)
            .map(|subscription| subscription.callbacks.clone());
        if let Some(callbacks) = callbacks {
            self.deliver_once(&callbacks, sid, 0, &property_set(properties))
                .await;
        }
    }

    pub async fn notify(&self, service: Service, properties: &[(&str, String)]) {
        let body = property_set(properties);
        let permits = Arc::new(tokio::sync::Semaphore::new(DELIVERIES_AT_ONCE));
        let mut sending = tokio::task::JoinSet::new();
        for sid in self.subscribers_of(service) {
            let Some((callbacks, seq)) = self.next(&sid) else {
                continue;
            };
            // Taken inside the task: taken here, a slow subscriber stops the round being
            // handed out at all, and the deadline below starts after the wait it bounds.
            let permits = permits.clone();
            let (subscriptions, body) = (self.clone(), body.clone());
            sending.spawn(async move {
                let _held = permits.acquire_owned().await.expect("never closed");
                subscriptions
                    .deliver_once(&callbacks, &sid, seq, &body)
                    .await;
            });
        }
        // The pass that published the index waits on this, so slow subscribers are left to finish
        // on their own rather than holding it up.
        let round = tokio::time::timeout(ROUND_WITHIN, async {
            while sending.join_next().await.is_some() {}
        })
        .await;
        if round.is_err() {
            tracing::warn!("some subscribers had not taken the event before the round was up");
            sending.detach_all();
        }
    }

    /// The URLs a `CALLBACK` header holds are alternatives, not a list to fan out over: they are
    /// tried in the order the subscriber wrote them, one event reaches it once, and an event no
    /// URL carried counts against it once.
    async fn deliver_once(&self, callbacks: &[String], sid: &str, seq: u32, body: &str) {
        for callback in callbacks {
            match deliver(callback, sid, seq, body).await {
                Ok(()) => {
                    tracing::debug!(%sid, %callback, seq, "event delivered");
                    self.delivered(sid);
                    return;
                }
                Err(error) => {
                    tracing::warn!(%sid, %callback, seq, %error, "event not delivered")
                }
            }
        }
        let missed = self.missed(sid);
        tracing::warn!(%sid, seq, missed, "no callback of this subscriber took the event");
        if missed >= MISSED {
            tracing::warn!(%sid, "nothing reached this subscriber: it is dropped");
            self.unsubscribe(sid);
        }
    }

    fn delivered(&self, sid: &str) {
        if let Some(subscription) = crate::held(&self.inner).get_mut(sid) {
            subscription.missed = 0;
        }
    }

    /// The count after this failure, or the ceiling where the subscription is already gone.
    fn missed(&self, sid: &str) -> u8 {
        match crate::held(&self.inner).get_mut(sid) {
            Some(subscription) => {
                subscription.missed = subscription.missed.saturating_add(1);
                subscription.missed
            }
            None => MISSED,
        }
    }
}

fn property_set(properties: &[(&str, String)]) -> String {
    let mut body = String::from(
        r#"<?xml version="1.0" encoding="utf-8"?>
<e:propertyset xmlns:e="urn:schemas-upnp-org:event-1-0">"#,
    );
    for (name, value) in properties {
        body.push_str(&format!(
            "<e:property><{name}>{}</{name}></e:property>",
            escape(value)
        ));
    }
    body.push_str("</e:propertyset>");
    body
}

fn escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

async fn deliver(callback: &str, sid: &str, seq: u32, body: &str) -> std::io::Result<()> {
    let (host, port, path) = split_url(callback)
        .ok_or_else(|| std::io::Error::other(format!("callback not a URL: {callback}")))?;

    let request = format!(
        "NOTIFY {path} HTTP/1.1\r\n\
         HOST: {host}:{port}\r\n\
         CONTENT-TYPE: text/xml; charset=\"utf-8\"\r\n\
         NT: upnp:event\r\n\
         NTS: upnp:propchange\r\n\
         SID: {sid}\r\n\
         SEQ: {seq}\r\n\
         CONNECTION: close\r\n\
         CONTENT-LENGTH: {}\r\n\r\n{body}",
        body.len(),
    );

    let mut stream = tokio::time::timeout(
        Duration::from_secs(5),
        TcpStream::connect((host.as_str(), port)),
    )
    .await
    .map_err(|_| std::io::Error::other("connect timed out"))??;
    stream.write_all(request.as_bytes()).await?;
    stream.flush().await?;
    // Half closed, so a subscriber reading to the end sees one, and this end can still be answered.
    stream.shutdown().await?;
    refused_by(&mut stream).await
}

/// A subscriber that answers 404 or 500 holds no such subscription, whatever it asked for. One
/// that answers nothing at all is left alone: not every renderer writes a response back.
async fn refused_by(stream: &mut TcpStream) -> std::io::Result<()> {
    // Read to the end of the status line: TCP is free to hand over "HTTP/1.1 " and " 500" apart.
    let line = tokio::time::timeout(ANSWER_WITHIN, status_line(stream)).await;
    let Ok(Ok(line)) = line else {
        return Ok(());
    };
    match line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse::<u16>().ok())
    {
        Some(code) if !(200..300).contains(&code) => Err(std::io::Error::other(format!(
            "the subscriber answered {code}"
        ))),
        _ => Ok(()),
    }
}

/// The first line of the answer, up to the newline or to what a status line can hold.
async fn status_line(stream: &mut TcpStream) -> std::io::Result<String> {
    let mut line = Vec::new();
    let mut byte = [0u8; 1];
    while line.len() < 128 {
        match stream.read(&mut byte).await? {
            0 => break,
            _ if byte[0] == b'\n' => break,
            _ => line.push(byte[0]),
        }
    }
    Ok(String::from_utf8_lossy(&line).into_owned())
}

fn split_url(url: &str) -> Option<(String, u16, String)> {
    let rest = url.trim().trim_start_matches('<').trim_end_matches('>');
    let rest = rest.strip_prefix("http://")?;
    let (authority, path) = match rest.find('/') {
        Some(slash) => (&rest[..slash], &rest[slash..]),
        None => (rest, "/"),
    };
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) => (host.to_owned(), port.parse().ok()?),
        None => (authority.to_owned(), 80),
    };
    Some((host, port, path.to_owned()))
}

/// A `CALLBACK` header holds one or more angle-bracketed URLs.
fn parse_callbacks(header: &str) -> Vec<String> {
    header
        .split('>')
        .filter_map(|part| part.trim().strip_prefix('<').map(str::to_owned))
        .collect()
}

fn callbacks_at(header: &str, peer: IpAddr) -> Result<Vec<String>, Refused> {
    let named = parse_callbacks(header);
    if named.is_empty() {
        return Err(Refused::NoCallback);
    }
    let mut own: Vec<String> = named.into_iter().filter(|url| names(url, peer)).collect();
    if own.is_empty() {
        return Err(Refused::CallbackElsewhere);
    }
    own.truncate(MAX_CALLBACKS);
    Ok(own)
}

/// Only a literal address counts; a hostname is not resolved.
fn names(url: &str, peer: IpAddr) -> bool {
    split_url(url).is_some_and(|(host, _, _)| host.parse::<IpAddr>() == Ok(peer))
}

fn make_room(held: &mut HashMap<String, Subscription>, peer: IpAddr) {
    while held.values().filter(|s| s.peer == peer).count() >= MAX_PER_PEER {
        let Some(oldest) = held
            .iter()
            .filter(|(_, s)| s.peer == peer)
            .min_by_key(|(_, s)| s.since)
            .map(|(sid, _)| sid.clone())
        else {
            return;
        };
        tracing::info!(sid = %oldest, %peer, "a device at its share of subscriptions loses its oldest");
        held.remove(&oldest);
    }
}

/// `Second-1800`, `Second-infinite`, or absent.
fn parse_timeout(header: Option<&str>) -> Duration {
    let requested = header
        .and_then(|value| value.trim().strip_prefix("Second-"))
        .and_then(|seconds| seconds.parse::<u64>().ok())
        .map(Duration::from_secs)
        .unwrap_or(DEFAULT_TIMEOUT);
    requested.clamp(Duration::from_secs(60), MAX_TIMEOUT)
}

pub fn timeout_header(timeout: Duration) -> String {
    format!("Second-{}", timeout.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_callback_header_may_hold_several_urls() {
        assert_eq!(
            parse_callbacks("<http://192.0.2.132:49153/>"),
            vec!["http://192.0.2.132:49153/"]
        );
        assert_eq!(
            parse_callbacks("<http://a/1><http://b/2>"),
            vec!["http://a/1", "http://b/2"]
        );
    }

    #[test]
    fn timeouts_are_bounded_in_both_directions() {
        assert_eq!(
            parse_timeout(Some("Second-1800")),
            Duration::from_secs(1800)
        );
        assert_eq!(parse_timeout(Some("Second-5")), Duration::from_secs(60));
        assert_eq!(parse_timeout(Some("Second-99999")), MAX_TIMEOUT);
        assert_eq!(parse_timeout(Some("Second-infinite")), DEFAULT_TIMEOUT);
        assert_eq!(parse_timeout(None), DEFAULT_TIMEOUT);
    }

    #[test]
    fn callback_urls_are_split_into_something_connectable() {
        assert_eq!(
            split_url("<http://192.0.2.132:49153/notify>"),
            Some(("192.0.2.132".to_owned(), 49153, "/notify".to_owned()))
        );
        assert_eq!(
            split_url("http://host/"),
            Some(("host".to_owned(), 80, "/".to_owned()))
        );
        assert_eq!(split_url("ftp://host/"), None);
    }

    #[test]
    fn the_property_set_is_well_formed_and_escaped() {
        let body = property_set(&[("SystemUpdateID", "7".to_owned())]);
        assert!(body.contains("<e:property><SystemUpdateID>7</SystemUpdateID></e:property>"));
        let body = property_set(&[("SourceProtocolInfo", "a<b&c".to_owned())]);
        assert!(body.contains("a&lt;b&amp;c"));
    }

    fn at(address: &str) -> Option<IpAddr> {
        Some(address.parse().expect("an address"))
    }

    fn subscribed(subs: &Subscriptions, service: Service, port: u16) -> Granted {
        subs.subscribe(
            service,
            &format!("<http://192.0.2.1:{port}/>"),
            at("192.0.2.1"),
            None,
            None,
        )
        .expect("a callback naming the subscriber is granted")
    }

    #[test]
    fn a_callback_is_kept_only_where_it_names_the_subscriber() {
        let subs = Subscriptions::default();
        let refused = |callback: &str, peer: Option<IpAddr>| {
            subs.subscribe(Service::ContentDirectory, callback, peer, None, None)
                .err()
        };
        assert_eq!(
            refused("<http://192.0.2.7:1/>", at("192.0.2.9")),
            Some(Refused::CallbackElsewhere)
        );
        assert_eq!(
            refused("<http://renderer.local:1/>", at("192.0.2.9")),
            Some(Refused::CallbackElsewhere),
            "a name to look up is not the subscriber's address"
        );
        assert_eq!(refused("", at("192.0.2.9")), Some(Refused::NoCallback));
        assert_eq!(
            refused("<http://192.0.2.9:1/>", None),
            Some(Refused::PeerUnknown)
        );
        assert!(subs.is_empty(), "nothing is held for a refused subscriber");

        let granted = subs
            .subscribe(
                Service::ContentDirectory,
                "<http://192.0.2.7:1/><http://192.0.2.9:1/a><http://192.0.2.9:2/b><http://192.0.2.9:3/c>",
                at("192.0.2.9"),
                None,
                None,
            )
            .expect("one of them names the subscriber");
        let held = crate::held(&subs.inner);
        assert_eq!(
            held[&granted.sid].callbacks,
            vec!["http://192.0.2.9:1/a", "http://192.0.2.9:2/b"],
            "the subscriber's own, and no more of them than a control point needs"
        );
    }

    #[test]
    fn a_device_that_never_unsubscribes_is_held_to_its_share() {
        let subs = Subscriptions::default();
        let first = subscribed(&subs, Service::ContentDirectory, 1).sid;
        for _ in 0..MAX_PER_PEER {
            subscribed(&subs, Service::ContentDirectory, 1);
        }
        subs.subscribe(
            Service::ContentDirectory,
            "<http://192.0.2.8:1/>",
            at("192.0.2.8"),
            None,
            None,
        )
        .expect("another device is not held to the first one's share");
        assert_eq!(subs.len(), MAX_PER_PEER + 1);
        assert!(
            subs.renew(&first, None, None).is_none(),
            "the oldest went first"
        );
    }

    #[tokio::test]
    async fn a_subscriber_that_refuses_the_connection_is_dropped() {
        // Bound then dropped: a port known to refuse.
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("a port");
        let callback = format!("<http://{}/notify>", listener.local_addr().expect("bound"));
        drop(listener);
        let subs = Subscriptions::default();
        subs.subscribe(
            Service::ContentDirectory,
            &callback,
            at("127.0.0.1"),
            None,
            None,
        )
        .expect("granted");
        until_dropped(&subs).await;
        assert!(
            subs.is_empty(),
            "a subscriber nobody can reach is no longer one"
        );
    }

    /// A subscriber that takes the connection and never answers holds its delivery for
    /// `ANSWER_WITHIN`, so more of them than run at once makes the round's own wait visible.
    #[tokio::test]
    async fn every_subscriber_is_handed_its_event_before_the_round_waits_on_any_of_them() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("a port");
        let address = listener.local_addr().expect("bound");
        let quiet = tokio::spawn(async move {
            let mut taken = Vec::new();
            while let Ok((stream, _)) = listener.accept().await {
                taken.push(stream);
            }
        });

        let subs = Subscriptions::default();
        let slow = DELIVERIES_AT_ONCE * 2;
        held_by(&subs, slow, &format!("http://{address}/notify"));

        let round = tokio::spawn({
            let subs = subs.clone();
            async move {
                subs.notify(
                    Service::ContentDirectory,
                    &[("SystemUpdateID", "2".to_owned())],
                )
                .await;
            }
        });
        tokio::time::sleep(Duration::from_millis(100)).await;
        let handed = subs.listed().iter().filter(|held| held.events == 2).count();
        round.await.expect("the round ends");
        quiet.abort();

        assert_eq!(
            handed, slow,
            "a subscriber past the first {DELIVERIES_AT_ONCE} waited on the ones before it"
        );
    }

    /// Subscriptions put in place directly: this needs more of them than one device is granted.
    fn held_by(subs: &Subscriptions, count: usize, callback: &str) {
        let mut held = crate::held(&subs.inner);
        for nth in 0..count {
            held.insert(
                format!("uuid:{nth}"),
                Subscription {
                    service: Service::ContentDirectory,
                    callbacks: vec![callback.to_owned()],
                    expires: Instant::now() + DEFAULT_TIMEOUT,
                    seq: 1,
                    user_agent: None,
                    peer: "127.0.0.1".parse().expect("an address"),
                    since: nth as u64,
                    missed: 0,
                },
            );
        }
    }

    /// Sends the number of rounds it takes to give up on a subscriber, one short of it first.
    async fn until_dropped(subs: &Subscriptions) {
        for round in 1..u32::from(MISSED) {
            subs.notify(
                Service::ContentDirectory,
                &[("SystemUpdateID", "2".to_owned())],
            )
            .await;
            assert!(
                !subs.is_empty(),
                "round {round}: one failure is a renderer missing a connect, not a dead subscriber"
            );
        }
        subs.notify(
            Service::ContentDirectory,
            &[("SystemUpdateID", "2".to_owned())],
        )
        .await;
    }

    /// A subscriber listening on a port, answering each NOTIFY with the line it was given.
    async fn answering(with: Option<&'static str>) -> (Subscriptions, String) {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("a port");
        let callback = format!("<http://{}/notify>", listener.local_addr().expect("bound"));
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut seen = [0u8; 1024];
                let _ = stream.read(&mut seen).await;
                if let Some(line) = with {
                    let _ = stream.write_all(line.as_bytes()).await;
                    let _ = stream.flush().await;
                }
            }
        });
        let subs = Subscriptions::default();
        subs.subscribe(
            Service::ContentDirectory,
            &callback,
            at("127.0.0.1"),
            None,
            None,
        )
        .expect("granted");
        (subs, callback)
    }

    /// TCP is free to break a line in two, and a status read in one go would miss the code.
    #[tokio::test]
    async fn a_fault_split_across_two_packets_is_still_a_fault() {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("a port");
        let callback = format!("<http://{}/notify>", listener.local_addr().expect("bound"));
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut seen = [0u8; 1024];
                let _ = stream.read(&mut seen).await;
                let _ = stream.write_all(b"HTTP/1.1 ").await;
                let _ = stream.flush().await;
                tokio::time::sleep(Duration::from_millis(20)).await;
                let _ = stream.write_all(b"404 Not Found\r\n\r\n").await;
                let _ = stream.flush().await;
            }
        });
        let subs = Subscriptions::default();
        subs.subscribe(
            Service::ContentDirectory,
            &callback,
            at("127.0.0.1"),
            None,
            None,
        )
        .expect("granted");
        until_dropped(&subs).await;
        assert!(subs.is_empty(), "the code arrived, late and in two pieces");
    }

    #[tokio::test]
    async fn a_subscriber_that_answers_with_a_fault_is_dropped_and_one_that_accepts_is_kept() {
        for (answer, kept) in [
            (Some("HTTP/1.1 200 OK\r\n\r\n"), true),
            (Some("HTTP/1.1 404 Not Found\r\n\r\n"), false),
            (Some("HTTP/1.1 500 Internal Server Error\r\n\r\n"), false),
            // Not every renderer writes one back, and a silence is not a refusal.
            (None, true),
        ] {
            let (subs, callback) = answering(answer).await;
            until_dropped(&subs).await;
            assert_eq!(!subs.is_empty(), kept, "answering {answer:?} at {callback}");
        }
    }

    /// A port bound then let go, which refuses every connection after that.
    async fn refusing() -> String {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("a port");
        let address = listener.local_addr().expect("bound");
        drop(listener);
        format!("http://{address}/notify")
    }

    /// A callback that answers every NOTIFY, and the count of those it took.
    async fn counting() -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
            .await
            .expect("a port");
        let url = format!("http://{}/notify", listener.local_addr().expect("bound"));
        let taken = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = taken.clone();
        tokio::spawn(async move {
            while let Ok((mut stream, _)) = listener.accept().await {
                let mut seen = [0u8; 1024];
                let _ = stream.read(&mut seen).await;
                counted.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                let _ = stream.write_all(b"HTTP/1.1 200 OK\r\n\r\n").await;
                let _ = stream.flush().await;
            }
        });
        (url, taken)
    }

    fn took(counter: &std::sync::atomic::AtomicUsize) -> usize {
        counter.load(std::sync::atomic::Ordering::Relaxed)
    }

    async fn subscribed_to(callbacks: &[&str]) -> Subscriptions {
        let header: String = callbacks.iter().map(|url| format!("<{url}>")).collect();
        let subs = Subscriptions::default();
        subs.subscribe(
            Service::ContentDirectory,
            &header,
            at("127.0.0.1"),
            None,
            None,
        )
        .expect("granted");
        subs
    }

    async fn one_event(subs: &Subscriptions) {
        subs.notify(
            Service::ContentDirectory,
            &[("SystemUpdateID", "2".to_owned())],
        )
        .await;
    }

    #[tokio::test]
    async fn an_event_reaches_a_subscriber_once_though_it_gave_two_callback_urls() {
        let (first, to_first) = counting().await;
        let (second, to_second) = counting().await;
        let subs = subscribed_to(&[&first, &second]).await;

        one_event(&subs).await;
        assert_eq!(took(&to_first), 1);
        assert_eq!(
            took(&to_second),
            0,
            "the URLs are alternatives, so the second is not written to at all"
        );
    }

    #[tokio::test]
    async fn the_second_callback_url_takes_the_event_when_the_first_refuses_it() {
        let (alive, taken) = counting().await;
        let subs = subscribed_to(&[&refusing().await, &alive]).await;

        one_event(&subs).await;
        assert_eq!(took(&taken), 1, "the alternative took the event");
        assert!(
            !subs.is_empty(),
            "and the subscription is not held to blame"
        );
    }

    #[tokio::test]
    async fn a_subscriber_with_two_dead_callbacks_is_dropped_no_sooner_than_one_with_a_single_one()
    {
        let subs = subscribed_to(&[&refusing().await, &refusing().await]).await;
        until_dropped(&subs).await;
        assert!(
            subs.is_empty(),
            "an event nothing carried is one failure, whatever the number of URLs tried"
        );
    }

    #[test]
    fn a_renewal_names_the_address_that_took_the_subscription() {
        let subs = Subscriptions::default();
        let granted = subs
            .subscribe(
                Service::ContentDirectory,
                "<http://192.0.2.1:1/>",
                at("192.0.2.1"),
                None,
                None,
            )
            .expect("granted");
        assert!(
            subs.renew(&granted.sid, at("198.51.100.9"), None).is_none(),
            "another address does not hold this subscription alive"
        );
        assert!(
            subs.renew(&granted.sid, at("192.0.2.1"), None).is_some(),
            "and the one that took it does"
        );
    }

    #[test]
    fn this_server_keeps_as_many_subscriptions_as_it_will() {
        let subs = Subscriptions::default();
        // One peer may hold MAX_PER_PEER, so the ceiling is reached with enough of them.
        let peers = MAX_SUBSCRIPTIONS / MAX_PER_PEER + 1;
        for peer in 0..peers {
            for _ in 0..MAX_PER_PEER {
                let address = format!("198.51.100.{peer}");
                let _ = subs.subscribe(
                    Service::ContentDirectory,
                    &format!("<http://{address}:1/>"),
                    at(&address),
                    None,
                    None,
                );
            }
        }
        assert!(
            subs.len() <= MAX_SUBSCRIPTIONS,
            "nothing bounds the number of peers, and a round of events walks every one of them: \
             {} held",
            subs.len()
        );
    }

    #[test]
    fn subscribing_renewing_and_unsubscribing() {
        let subs = Subscriptions::default();
        let granted = subs
            .subscribe(
                Service::ContentDirectory,
                "<http://192.0.2.1:1/>",
                at("192.0.2.1"),
                Some("Second-120"),
                None,
            )
            .expect("granted");
        assert_eq!(subs.len(), 1);
        assert!(granted.sid.starts_with("uuid:"));
        assert_eq!(granted.timeout, Duration::from_secs(120));

        assert!(subs.renew(&granted.sid, None, Some("Second-600")).is_some());
        assert!(subs.renew("uuid:nobody", None, None).is_none());

        assert!(subs.unsubscribe(&granted.sid));
        assert!(subs.is_empty());
    }

    #[test]
    fn a_change_announced_before_the_initial_event_goes_out_does_not_take_its_sequence() {
        let subs = Subscriptions::default();
        let granted = subscribed(&subs, Service::ContentDirectory, 1);
        assert_eq!(
            subs.next(&granted.sid).unwrap().1,
            1,
            "zero is the event carrying every variable, and a subscriber reads a change as zero"
        );
    }

    #[test]
    fn the_events_after_the_initial_one_count_up_from_one() {
        let subs = Subscriptions::default();
        let granted = subscribed(&subs, Service::ContentDirectory, 1);
        assert_eq!(subs.next(&granted.sid).unwrap().1, 1);
        assert_eq!(subs.next(&granted.sid).unwrap().1, 2);
    }

    #[test]
    fn a_subscription_nobody_renewed_is_dropped() {
        let subs = Subscriptions::default();
        subs.subscribe(
            Service::ContentDirectory,
            "<http://192.0.2.1:1/>",
            at("192.0.2.1"),
            Some("Second-60"),
            None,
        )
        .expect("granted");
        subs.expire();
        assert_eq!(subs.len(), 1, "not yet due");

        subs.inner
            .lock()
            .unwrap()
            .values_mut()
            .for_each(|s| s.expires = Instant::now() - Duration::from_secs(1));
        subs.expire();
        assert!(subs.is_empty());
    }

    #[tokio::test]
    async fn a_subscriber_that_cannot_be_reached_does_not_hold_up_the_others() {
        let subscriptions = Subscriptions::default();
        for _ in 0..3 {
            subscribed(&subscriptions, Service::ContentDirectory, 9);
        }
        let started = Instant::now();
        subscriptions
            .notify(
                Service::ContentDirectory,
                &[("SystemUpdateID", "2".to_owned())],
            )
            .await;
        assert!(
            started.elapsed() < Duration::from_secs(12),
            "three unreachable subscribers took {:?}, which is one after another",
            started.elapsed()
        );
    }

    #[test]
    fn only_subscribers_of_a_service_hear_about_it() {
        let subs = Subscriptions::default();
        subscribed(&subs, Service::ContentDirectory, 1);
        subscribed(&subs, Service::ConnectionManager, 2);
        assert_eq!(subs.subscribers_of(Service::ContentDirectory).len(), 1);
        assert_eq!(subs.subscribers_of(Service::ConnectionManager).len(), 1);
    }
}
