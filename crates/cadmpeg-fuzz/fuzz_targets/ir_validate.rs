// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for `cadmpeg_ir::validate::validate`.
//!
//! Feeds arbitrary bytes through UTF-8 decoding, JSON deserialization into
//! `CadIr`, then validation. Contract: no input may panic. Malformed JSON must
//! surface as `serde_json::Error`; validation findings are discarded.
//!
//! Run: cargo +nightly fuzz run --fuzz-dir crates/cadmpeg-fuzz ir_validate

#![no_main]

use cadmpeg_ir::validate::validate;
use cadmpeg_ir::CadIr;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        if let Ok(ir) = CadIr::from_json(s) {
            let _ = validate(&ir, Vec::new());
        }
    }
});
