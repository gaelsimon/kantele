//! A UPnP AV media server for music.

pub mod api;
pub mod browse;
pub mod config;
pub mod index;
pub mod mdns;
pub mod object;
pub mod report;
pub mod server;
pub mod service;
pub mod state;
pub mod tags;
pub mod tasks;
pub mod upnp;

/// A lock taken past a poisoning: every mutex here guards a table a panic cannot leave half-written.
pub(crate) fn held<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
