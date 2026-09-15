//! What the network sees: the catalogue served, the identity, the update id and who is listening.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use arc_swap::{ArcSwap, ArcSwapOption};

use crate::browse::{Served, Settings};
use crate::index::Library;
use crate::upnp::capture::Recorder;
use crate::upnp::client::Profiles;
use crate::upnp::description::DeviceIdentity;
use crate::upnp::gena::{Service, Subscriptions};
use crate::upnp::peers::Peers;

pub struct Device {
    served: ArcSwap<Served>,
    pub identity: DeviceIdentity,
    update_id: AtomicU32,
    pub subscriptions: Subscriptions,
    pub clients: Profiles,
    /// Every address that searched, read, subscribed, browsed or played, and when.
    pub peers: Peers,
    capture: ArcSwapOption<Recorder>,
}

impl Device {
    pub fn new(
        library: Library,
        settings: Settings,
        identity: DeviceIdentity,
        clients: Profiles,
    ) -> Self {
        Self {
            served: ArcSwap::from_pointee(Served::new(library, settings)),
            identity,
            update_id: AtomicU32::new(1),
            subscriptions: Subscriptions::default(),
            clients,
            peers: Peers::default(),
            capture: ArcSwapOption::empty(),
        }
    }

    pub fn capture_to(&self, recorder: Recorder) {
        self.capture.store(Some(Arc::new(recorder)));
    }

    pub fn capture(&self) -> Option<Arc<Recorder>> {
        self.capture.load_full()
    }

    pub fn resume_update_id(&self, id: u32) {
        self.update_id.store(id.max(1), Ordering::Relaxed);
    }

    pub fn served(&self) -> Arc<Served> {
        self.served.load_full()
    }

    pub fn system_update_id(&self) -> u32 {
        self.update_id.load(Ordering::Relaxed)
    }

    pub async fn publish(&self, served: Served) -> u32 {
        let id = self.replace_served(served);
        self.subscriptions
            .notify(Service::ContentDirectory, &self.content_directory_change())
            .await;
        id
    }

    pub fn replace_library(&self, library: Library) -> u32 {
        let settings = self.served().view.settings.clone();
        self.replace_served(Served::new(library, settings))
    }

    pub async fn apply_settings(&self, settings: Settings) -> u32 {
        let served = self.served();
        let rebuilt = tokio::task::spawn_blocking(move || served.with_settings(settings)).await;
        match rebuilt {
            Ok(served) => self.publish(served).await,
            Err(error) => {
                tracing::error!(%error, "the menus were not rebuilt");
                self.system_update_id()
            }
        }
    }

    pub fn replace_served(&self, served: Served) -> u32 {
        self.served.store(Arc::new(served));
        let next = self
            .update_id
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some(current.wrapping_add(1).max(1))
            });
        match next {
            Ok(previous) => previous.wrapping_add(1).max(1),
            Err(previous) => previous,
        }
    }

    /// What a fresh subscriber is told: every evented variable, whatever its value.
    pub fn content_directory_state(&self) -> Vec<(&'static str, String)> {
        vec![
            ("SystemUpdateID", self.system_update_id().to_string()),
            ("ContainerUpdateIDs", String::new()),
        ]
    }

    /// What a change carries: the variables that changed, and no others. An empty
    /// `ContainerUpdateIDs` beside a moved `SystemUpdateID` contradicts itself.
    fn content_directory_change(&self) -> Vec<(&'static str, String)> {
        vec![("SystemUpdateID", self.system_update_id().to_string())]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(library: Library) -> Device {
        Device::new(
            library,
            Settings::default(),
            DeviceIdentity {
                friendly_name: "Kantele".to_owned(),
                udn: "uuid:test".to_owned(),
                icon: Default::default(),
            },
            Profiles::default(),
        )
    }

    #[test]
    fn a_restart_carries_on_from_the_update_id_the_last_run_reached() {
        let device = device(Library::build("resumed".to_owned(), &[]));
        assert_eq!(device.system_update_id(), 1, "nothing to resume");
        device.resume_update_id(41);
        assert_eq!(device.system_update_id(), 41);
        assert_eq!(
            device.replace_library(Library::build("later".to_owned(), &[])),
            42,
            "and the next change carries on past it"
        );
        device.resume_update_id(0);
        assert_eq!(
            device.system_update_id(),
            1,
            "zero is not a value this identifier takes"
        );
    }

    #[test]
    fn a_rescan_replaces_the_index_without_disturbing_a_request_already_reading_one() {
        let device = device(Library::build("before".to_owned(), &[]));
        assert_eq!(device.system_update_id(), 1);
        let in_flight = device.served();

        assert_eq!(
            device.replace_library(Library::build("after".to_owned(), &[])),
            2,
            "a swap is what a subscriber is told about"
        );
        assert_eq!(device.system_update_id(), 2);
        assert_eq!(device.served().library.name(), "after");
        assert_eq!(
            in_flight.library.name(),
            "before",
            "the index a request started on stays alive until it is done with it"
        );
    }
}
