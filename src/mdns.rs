//! The page, announced on mDNS, so an owner opens it by name instead of by address.

use std::net::{IpAddr, Ipv4Addr};

use anyhow::Result;
use mdns_sd::{IfKind, ServiceDaemon, ServiceInfo};

/// What a Bonjour browser lists. The page is plain HTTP on the port the wire already uses.
const SERVICE: &str = "_http._tcp.local.";

/// A registration held for as long as the server runs.
pub struct Announced {
    daemon: ServiceDaemon,
    fullname: String,
}

/// The label an mDNS host name is built from. A system may call itself by a qualified name, and
/// `.local` is this responder's own domain, so only the first part of it is ours to publish.
fn label(host: &str) -> &str {
    let first = host.trim().trim_end_matches('.').split('.').next();
    match first {
        Some(label) if !label.is_empty() => label,
        _ => "kantele",
    }
}

impl Announced {
    /// Announces the page on the interfaces given. An address that changes on one of them is
    /// followed; an interface appearing later is announced on at the next start.
    pub fn new(name: &str, host: &str, port: u16, interfaces: &[Ipv4Addr]) -> Result<Self> {
        // A registration with no address is one nobody can act on, and mdns-sd takes it anyway.
        if interfaces.is_empty() {
            anyhow::bail!("no address to announce it on");
        }
        let addresses: Vec<IpAddr> = interfaces.iter().copied().map(IpAddr::V4).collect();
        let daemon = ServiceDaemon::new()?;
        let mut service = ServiceInfo::new(
            SERVICE,
            name,
            &format!("{}.local.", label(host)),
            addresses.as_slice(),
            port,
            &[("path", "/config")][..],
        )?
        .enable_addr_auto();
        service.set_interfaces(addresses.iter().copied().map(IfKind::Addr).collect());
        let fullname = service.get_fullname().to_owned();
        daemon.register(service)?;
        Ok(Self { daemon, fullname })
    }

    /// The name a browser lists, which is also what a withdrawal names.
    pub fn fullname(&self) -> &str {
        &self.fullname
    }

    /// Withdraws the registration, so a browser drops the entry instead of waiting for it to age.
    pub fn withdraw(&self) {
        if let Err(error) = self.daemon.unregister(&self.fullname) {
            tracing::debug!(%error, "the mdns registration could not be withdrawn");
        }
        if let Err(error) = self.daemon.shutdown() {
            tracing::debug!(%error, "the mdns daemon did not stop cleanly");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_host_name_gives_up_its_domain_before_it_is_published() {
        assert_eq!(label("gamac"), "gamac");
        assert_eq!(label("gamac.local"), "gamac");
        assert_eq!(label("gamac.local."), "gamac");
        assert_eq!(label("nas.home.arpa"), "nas");
        assert_eq!(label("DiskStation"), "DiskStation");
        assert_eq!(label("  gamac.lan \n"), "gamac");
        assert_eq!(label(""), "kantele");
        assert_eq!(label("."), "kantele");
    }

    #[test]
    fn a_machine_with_no_address_is_not_announced_as_though_it_had_one() {
        let error = Announced::new("Salon", "kantele", 8200, &[])
            .err()
            .expect("a registration with no address is refused");
        assert!(error.to_string().contains("no address"), "{error}");
    }

    #[test]
    fn the_page_is_announced_under_the_name_a_device_shows() {
        let announced = Announced::new("Salon", "kantele", 8200, &[Ipv4Addr::LOCALHOST])
            .expect("a registration on the loopback");
        assert_eq!(announced.fullname(), "Salon._http._tcp.local.");
        announced.withdraw();
    }

    #[test]
    fn a_name_with_a_dot_in_it_is_still_one_instance() {
        let announced = Announced::new("Kantele 0.1", "kantele", 8200, &[Ipv4Addr::LOCALHOST])
            .expect("a registration on the loopback");
        assert!(
            announced.fullname().ends_with("._http._tcp.local."),
            "{}",
            announced.fullname()
        );
        announced.withdraw();
    }
}
