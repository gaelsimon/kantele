//! The library index.

pub mod artwork;
pub mod coverage;
pub mod credits;
pub mod derive;
pub mod fold;
pub mod identity;
pub mod library;
pub mod playlist;
pub mod refusals;
pub mod roots;
pub mod scan;
pub mod searchable;
pub mod store;
pub mod sweep;
pub mod watch;

pub use artwork::Artwork;
pub use coverage::{Coverage, Missing};
pub use fold::fold;
pub use identity::{IDENTITY_VERSION, Rule};
pub use library::{Album, Artist, Disc, Library, Run, Scan, Track};
pub use playlist::Playlist;
pub use refusals::{Cause, Refusal, Refusals};
pub use roots::Roots;
pub use scan::{Fingerprint, Found, ScanOptions, Scanned, Stopping, Walked};
pub use searchable::Searchables;
pub use store::Store;
pub use watch::Watcher;
