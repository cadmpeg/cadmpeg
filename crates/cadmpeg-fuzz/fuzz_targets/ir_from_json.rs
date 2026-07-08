// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for `CadIr::from_json`.
//!
//! Feeds arbitrary bytes through UTF-8 decoding and JSON deserialization of the
//! IR document. Contract: no input may panic. Malformed JSON must surface as
//! `serde_json::Error`.
//!
//! Run: cargo +nightly fuzz run --fuzz-dir crates/cadmpeg-fuzz ir_from_json

#![no_main]

use cadmpeg_ir::CadIr;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = CadIr::from_json(s);
    }
});
