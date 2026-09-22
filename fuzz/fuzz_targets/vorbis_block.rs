#![no_main]
//! The FLAC metadata blocks, from any file in the music folder.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let mut file = std::io::Cursor::new(bytes);
    let _ = kantele::tags::vorbis::block(&mut file);
});
