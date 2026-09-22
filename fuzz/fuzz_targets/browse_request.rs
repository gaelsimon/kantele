#![no_main]
//! The SOAP body a control point sends, which any device on the network may shape.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|body: &str| {
    let _ = kantele::upnp::contentdirectory::soap::parse_browse(body);
});
