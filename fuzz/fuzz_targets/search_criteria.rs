#![no_main]
//! The search criteria a control point sends.
use libfuzzer_sys::fuzz_target;

fuzz_target!(|criteria: &str| {
    let _ = kantele::upnp::search::parse(criteria);
});
