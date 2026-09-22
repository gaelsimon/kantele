#![no_main]
//! The DSF and DSDIFF headers, from any file in the music folder.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|bytes: &[u8]| {
    let mut file = std::io::Cursor::new(bytes);
    let _ = kantele::tags::dsd::read_from(&mut file);
});
