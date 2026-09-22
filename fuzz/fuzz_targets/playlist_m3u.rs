#![no_main]
//! An M3U playlist, from any file in the music folder.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|text: &str| {
    let _ = kantele::index::playlist::parse_m3u(
        text,
        std::path::Path::new("/music"),
        std::path::Path::new("/music/lists"),
        "list".to_owned(),
    );
});
