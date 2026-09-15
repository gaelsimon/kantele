//! UPnP: discovery, description, control and eventing.

pub mod capture;
pub mod client;
pub mod contentdirectory;
pub mod description;
pub mod device;
pub mod didl;
pub mod gena;
pub mod http;
pub mod icon;
pub mod peers;
pub mod search;
pub mod ssdp;

pub use crate::object::{ObjectId, ObjectIdError};

/// The protocol info `ConnectionManager` reports.
pub const PROTOCOL_INFO: &[&str] = &[
    "http-get:*:audio/x-flac:*",
    "http-get:*:audio/mpeg:*",
    "http-get:*:audio/mp4:*",
    "http-get:*:audio/ogg:*",
    "http-get:*:audio/opus:*",
    "http-get:*:audio/x-wav:*",
    "http-get:*:audio/x-aiff:*",
    "http-get:*:audio/x-wavpack:*",
    "http-get:*:audio/x-monkeys-audio:*",
    "http-get:*:audio/x-dsf:*",
    "http-get:*:audio/x-dff:*",
];
