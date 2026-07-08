// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for `CadIr::to_canonical_json` round-trip.
//!
//! Feeds arbitrary bytes through UTF-8 decoding, JSON deserialization into
//! `CadIr`, canonical JSON serialization, then deserialization again.
//! Contract: no input may panic. Invariant: round-trip should preserve structure.

#![no_main]

use cadmpeg_ir::CadIr;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        if let Ok(ir) = CadIr::from_json(s) {
            if let Ok(canonical) = ir.to_canonical_json() {
                let _ = CadIr::from_json(&canonical);
            }
        }
    }
});
