// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for explicit previous-version CADIR migration.

#![no_main]

use cadmpeg_ir::CadIr;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        let _ = CadIr::migrate_json(s);
    }
});
