//! What the server has seen of every address on the network, and when.

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

/// Addresses remembered at most; a scanner on the LAN must not grow this.
const REMEMBERED: usize = 64;

/// One step a device took, the last time and how often.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Seen {
    /// Seconds since the epoch, so a reader can subtract its own clock.
    pub last: i64,
    pub times: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// An M-SEARCH this server answered.
    Searched,
    /// `description.xml` read.
    Described,
    /// A new GENA subscription, not a renewal.
    Subscribed,
    /// A ContentDirectory or ConnectionManager action.
    Browsed,
    /// A media fetch.
    Played,
}

impl Step {
    fn said(self) -> &'static str {
        match self {
            Self::Searched => "a device is looking for this server",
            Self::Described => "a device is reading what this server is",
            Self::Subscribed => "a device asked to be told about changes",
            Self::Browsed => "a device is browsing",
            Self::Played => "a device is playing",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct Peer {
    name: Option<String>,
    searched: Option<Seen>,
    described: Option<Seen>,
    subscribed: Option<Seen>,
    browsed: Option<Seen>,
    played: Option<Seen>,
}

impl Peer {
    fn slot(&mut self, step: Step) -> &mut Option<Seen> {
        match step {
            Step::Searched => &mut self.searched,
            Step::Described => &mut self.described,
            Step::Subscribed => &mut self.subscribed,
            Step::Browsed => &mut self.browsed,
            Step::Played => &mut self.played,
        }
    }

    fn last_active(&self) -> i64 {
        [
            self.searched,
            self.described,
            self.subscribed,
            self.browsed,
            self.played,
        ]
        .into_iter()
        .flatten()
        .map(|seen| seen.last)
        .max()
        .unwrap_or(0)
    }
}

/// One address as the interface lists it.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Listed {
    pub address: String,
    pub name: Option<String>,
    pub searched: Option<Seen>,
    pub described: Option<Seen>,
    pub subscribed: Option<Seen>,
    pub browsed: Option<Seen>,
    pub played: Option<Seen>,
}

#[derive(Clone, Default)]
pub struct Peers {
    seen: Arc<Mutex<HashMap<IpAddr, Peer>>>,
}

impl Peers {
    pub fn note(&self, address: IpAddr, step: Step, agent: Option<&str>) {
        self.note_at(address, step, agent, epoch_seconds());
    }

    fn note_at(&self, address: IpAddr, step: Step, agent: Option<&str>, at: i64) {
        let mut seen = crate::held(&self.seen);
        if !seen.contains_key(&address) {
            tracing::info!(%address, agent = agent.unwrap_or("unnamed"), "{}", step.said());
            forget_the_quietest(&mut seen);
        }
        let peer = seen.entry(address).or_default();
        if peer.name.is_none()
            && let Some(agent) = agent.map(str::trim).filter(|agent| !agent.is_empty())
        {
            peer.name = Some(agent.to_owned());
        }
        let slot = peer.slot(step);
        let times = slot.map_or(0, |seen| seen.times).saturating_add(1);
        *slot = Some(Seen { last: at, times });
    }

    /// Every address, the most recently active first.
    pub fn listed(&self) -> Vec<Listed> {
        let seen = crate::held(&self.seen);
        let mut peers: Vec<(&IpAddr, &Peer)> = seen.iter().collect();
        peers.sort_by(|left, right| {
            right
                .1
                .last_active()
                .cmp(&left.1.last_active())
                .then_with(|| left.0.cmp(right.0))
        });
        peers
            .into_iter()
            .map(|(address, peer)| Listed {
                address: address.to_string(),
                name: peer.name.clone(),
                searched: peer.searched,
                described: peer.described,
                subscribed: peer.subscribed,
                browsed: peer.browsed,
                played: peer.played,
            })
            .collect()
    }
}

fn forget_the_quietest(seen: &mut HashMap<IpAddr, Peer>) {
    if seen.len() < REMEMBERED {
        return;
    }
    let quietest = seen
        .iter()
        .min_by_key(|(address, peer)| (peer.last_active(), **address))
        .map(|(address, _)| *address);
    if let Some(address) = quietest {
        seen.remove(&address);
    }
}

pub(crate) fn epoch_seconds() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn ip(last: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(192, 0, 2, last))
    }

    #[test]
    fn every_step_is_counted_and_the_first_name_a_device_gives_is_kept() {
        let peers = Peers::default();
        peers.note_at(ip(9), Step::Searched, None, 100);
        peers.note_at(ip(9), Step::Searched, Some("Denon-Heos"), 110);
        peers.note_at(ip(9), Step::Described, Some("Other"), 120);
        let listed = peers.listed();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].address, "192.0.2.9");
        assert_eq!(listed[0].name.as_deref(), Some("Denon-Heos"));
        assert_eq!(
            listed[0].searched,
            Some(Seen {
                last: 110,
                times: 2
            })
        );
        assert_eq!(
            listed[0].described,
            Some(Seen {
                last: 120,
                times: 1
            })
        );
        assert_eq!(listed[0].browsed, None, "it has not browsed");
    }

    #[test]
    fn the_most_recently_active_address_comes_first() {
        let peers = Peers::default();
        peers.note_at(ip(1), Step::Browsed, None, 500);
        peers.note_at(ip(2), Step::Searched, None, 900);
        peers.note_at(ip(3), Step::Played, None, 700);
        let order: Vec<String> = peers.listed().into_iter().map(|p| p.address).collect();
        assert_eq!(order, ["192.0.2.2", "192.0.2.3", "192.0.2.1"]);
    }

    #[test]
    fn a_scanner_walking_the_subnet_pushes_out_the_quietest_rather_than_growing_the_table() {
        let peers = Peers::default();
        for last in 0..REMEMBERED as i64 {
            peers.note_at(ip(last as u8), Step::Searched, None, last);
        }
        // Outside the block the walk filled, so the newcomer is not one of the addresses it evicts.
        peers.note_at(
            IpAddr::V4(Ipv4Addr::new(198, 51, 100, 1)),
            Step::Searched,
            None,
            1_000,
        );
        let listed = peers.listed();
        assert_eq!(listed.len(), REMEMBERED);
        assert!(
            listed.iter().all(|p| p.address != "192.0.2.0"),
            "the address not heard from longest is the one forgotten"
        );
        assert_eq!(listed[0].address, "198.51.100.1");
    }

    #[test]
    fn a_blank_agent_is_no_name() {
        let peers = Peers::default();
        peers.note_at(ip(4), Step::Described, Some("  "), 1);
        assert_eq!(peers.listed()[0].name, None);
    }
}
