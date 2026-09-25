//! The library index.

pub mod artwork;
pub mod checks;
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

pub use artwork::{Artwork, Prefer, Source};
pub use checks::{Check, Checks};
pub use coverage::{Coverage, Missing};
pub use fold::fold;
pub use identity::{IDENTITY_VERSION, Rule};
pub use library::{Album, Artist, Disc, Library, Run, Scan, Track};
pub use playlist::Playlist;
pub use refusals::{Cause, Origin, Refusal, Refusals, Reported, Tally};
pub use roots::Roots;
pub use scan::{Fingerprint, Found, ScanOptions, Scanned, Stopping, Walked};
pub(crate) use scan::{is_image, is_skipped};
pub use searchable::Searchables;
pub use store::Store;
pub use store::copy_path as store_copy_path;
pub use watch::Watcher;
