#![no_main]

include!("../harness.rs");

libfuzzer_sys::fuzz_target!(|data: &[u8]| document_json(data));
