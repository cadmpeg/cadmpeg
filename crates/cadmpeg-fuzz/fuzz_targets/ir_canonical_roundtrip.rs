// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for `CadIr::to_canonical_json` round-trip.
//!
//! Feeds arbitrary bytes through UTF-8 decoding, JSON deserialization into
//! `CadIr`, canonical JSON serialization, then deserialization again.
//! Reparse failures and structural changes fail the target.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        if let Ok(ir) = cadmpeg_ir::CadIr::from_json(s) {
            if let Ok(canonical) = ir.to_canonical_json() {
                let result = cadmpeg_fuzz::check_ir_canonical_roundtrip(ir, &canonical);
                assert!(result.is_ok(), "{result:?}");
            }
        }
    }
});
