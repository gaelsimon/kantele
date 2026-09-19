//! What the network sees: the catalogue served, the identity, the update id and who is listening.

use std::sync::Arc;

use arc_swap::{ArcSwap, ArcSwapOption};
use tokio::sync::{Mutex, watch};

use crate::browse::{Served, Settings};
use crate::index::Library;
use crate::upnp::capture::Recorder;
use crate::upnp::client::Profiles;
use crate::upnp::description::DeviceIdentity;
use crate::upnp::gena::{Service, Subscriptions};
use crate::upnp::peers::Peers;

/// Zero is not a value this identifier takes.
const FIRST: u32 = 1;

/// The catalogue and the number that stamps it, held as one value: a client is never handed a
/// version that belongs to another catalogue.
pub struct Published {
    pub update_id: u32,
    pub served: Arc<Served>,
}

pub struct Device {
    published: ArcSwap<Published>,
    /// Held across the whole of a read, a rebuild and a write, or a view derived from the
    /// catalogue being served lands on top of a rescan that replaced it meanwhile, and no later
    /// pass notices: the store already holds the rows, so nothing says the tree changed.
    publishing: Mutex<()>,
    bumps: watch::Sender<u32>,
    pub identity: DeviceIdentity,
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
            published: ArcSwap::from_pointee(Published {
                update_id: FIRST,
                served: Arc::new(Served::new(library, settings)),
            }),
            publishing: Mutex::new(()),
            bumps: watch::Sender::new(FIRST),
            identity,
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

    /// The number a previous run reached, restored before anything is served. Nobody is told,
    /// and nothing needs persisting: this is the value the store already holds.
    pub fn resume_update_id(&self, id: u32) {
        let held = self.published();
        self.published.store(Arc::new(Published {
            update_id: id.max(FIRST),
            served: held.served.clone(),
        }));
    }

    /// The catalogue and its number together, for an answer that carries both.
    pub fn published(&self) -> Arc<Published> {
        self.published.load_full()
    }

    pub fn served(&self) -> Arc<Served> {
        self.published().served.clone()
    }

    pub fn system_update_id(&self) -> u32 {
        self.published().update_id
    }

    /// Every number this run reaches, for whoever holds the store to persist. A restart that
    /// resumes below one a client was told hands it a stale cache.
    pub fn bumps(&self) -> watch::Receiver<u32> {
        self.bumps.subscribe()
    }

    pub async fn publish(&self, served: Served) -> u32 {
        let _writing = self.publishing.lock().await;
        self.commit(Arc::new(served)).await
    }

    /// Rebuilds the menus over the catalogue being served. The lock is taken before that
    /// catalogue is read, not after the rebuild: the pair has to be one step.
    pub async fn apply_settings(&self, settings: Settings) -> u32 {
        let _writing = self.publishing.lock().await;
        let served = self.served();
        match tokio::task::spawn_blocking(move || served.with_settings(settings)).await {
            Ok(served) => self.commit(Arc::new(served)).await,
            Err(error) => {
                tracing::error!(%error, "the menus were not rebuilt");
                self.system_update_id()
            }
        }
    }

    async fn commit(&self, served: Arc<Served>) -> u32 {
        let update_id = self.published().update_id.wrapping_add(1).max(FIRST);
        self.published
            .store(Arc::new(Published { update_id, served }));
        let _ = self.bumps.send(update_id);
        self.subscriptions
            .notify(Service::ContentDirectory, &change(update_id))
            .await;
        update_id
    }

    /// What a fresh subscriber is told: every evented variable, whatever its value.
    pub fn content_directory_state(&self) -> Vec<(&'static str, String)> {
        vec![
            ("SystemUpdateID", self.system_update_id().to_string()),
            ("ContainerUpdateIDs", String::new()),
        ]
    }
}

/// What a change carries: the variables that changed, and no others. An empty
/// `ContainerUpdateIDs` beside a moved `SystemUpdateID` contradicts itself.
fn change(update_id: u32) -> Vec<(&'static str, String)> {
    vec![("SystemUpdateID", update_id.to_string())]
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::index::Scanned;
    use crate::tags::{AudioProperties, FileTags};

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

    fn empty(name: &str) -> Served {
        Served::new(Library::build(name.to_owned(), &[]), Settings::default())
    }

    /// A library named after the update id the publication that carries it will be given.
    fn numbered(update_id: u32) -> Library {
        Library::build(update_id.to_string(), &[])
    }

    /// Enough tracks that rebuilding the menus over them takes long enough for another
    /// publisher to reach the device while the rebuild is on its blocking thread.
    fn sized(name: &str, tracks: u32) -> Library {
        let files: Vec<Scanned> = (0..tracks)
            .map(|n| Scanned {
                path: PathBuf::from(format!("/music/{n:05}.flac")),
                relative: PathBuf::from(format!("{:03}/{n:05}.flac", n / 12)),
                tags: FileTags {
                    title: Some(format!("track {n}")),
                    album: Some(format!("album {}", n / 12)),
                    album_artists: vec![format!("artist {}", n % 211)],
                    artists: vec![format!("artist {}", n % 211)],
                    genres: vec![format!("genre {}", n % 17)],
                    track_number: Some(n % 12 + 1),
                    ..FileTags::default()
                },
                properties: AudioProperties::default(),
                size: 1,
                artwork: None,
            })
            .collect();
        Library::build(name.to_owned(), &files)
    }

    fn many(name: &str) -> Library {
        sized(name, 20_000)
    }

    #[tokio::test]
    async fn a_restart_carries_on_from_the_update_id_the_last_run_reached() {
        let device = device(Library::build("resumed".to_owned(), &[]));
        assert_eq!(device.system_update_id(), 1, "nothing to resume");
        device.resume_update_id(41);
        assert_eq!(device.system_update_id(), 41);
        assert_eq!(
            device.publish(empty("later")).await,
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

    #[tokio::test]
    async fn a_rescan_replaces_the_index_without_disturbing_a_request_already_reading_one() {
        let device = device(Library::build("before".to_owned(), &[]));
        assert_eq!(device.system_update_id(), 1);
        let in_flight = device.served();

        assert_eq!(
            device.publish(empty("after")).await,
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn the_number_a_client_is_handed_belongs_to_the_catalogue_it_is_handed() {
        let device = Arc::new(device(numbered(1)));
        let reading = {
            let device = device.clone();
            tokio::task::spawn_blocking(move || {
                for _ in 0..200_000 {
                    let handed = device.published();
                    assert_eq!(
                        handed.served.library.name(),
                        handed.update_id.to_string(),
                        "a client was told a version belonging to another catalogue"
                    );
                }
            })
        };
        for update_id in 2..=500 {
            device
                .publish(Served::new(numbered(update_id), Settings::default()))
                .await;
        }
        reading.await.expect("the reader");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn a_setting_written_while_a_rescan_publishes_does_not_put_the_old_library_back() {
        let device = Arc::new(device(many("before")));
        let rescanned = Served::new(many("after"), Settings::default());
        let writing = {
            let device = device.clone();
            tokio::spawn(async move {
                device
                    .apply_settings(Settings {
                        album_threshold: 3,
                        ..Settings::default()
                    })
                    .await
            })
        };
        // Long enough that the rebuild is on its blocking thread and nowhere near done, which
        // is where a publication used to be able to slip underneath it.
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        device.publish(rescanned).await;
        writing.await.expect("the settings task");

        assert_eq!(
            device.served().library.name(),
            "after",
            "the rescan was published and then dropped: the store holds its rows, so no later \
             pass reads the tree as changed and the catalogue stays behind for ever"
        );
    }

    #[tokio::test]
    async fn every_number_this_run_reaches_is_offered_for_the_store_to_keep() {
        let device = device(Library::build("kept".to_owned(), &[]));
        let mut bumps = device.bumps();

        device.resume_update_id(41);
        assert!(
            !bumps.has_changed().expect("the device is alive"),
            "a resumed number is the one the store already holds"
        );

        let published = device.publish(empty("rescanned")).await;
        assert_eq!(*bumps.borrow_and_update(), published);
        let applied = device.apply_settings(Settings::default()).await;
        assert_eq!(
            *bumps.borrow_and_update(),
            applied,
            "a setting moves the number too, and a restart below it hands a client a stale cache"
        );
    }
}
