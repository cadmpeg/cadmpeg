// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for `cadmpeg_step::write_step` with non-default options.
//!
//! Feeds arbitrary bytes through UTF-8 decoding, JSON deserialization into
//! `CadIr`, then STEP export with custom options. Contract: no input may panic.
//! Exercises string escaping in STEP HEADER with arbitrary metadata.

#![no_main]

use std::io::Cursor;

use cadmpeg_ir::CadIr;
use cadmpeg_step::{write_step, StepWriteOptions};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 8 {
        return;
    }

    let json_bytes = &data[8..];
    let s = match std::str::from_utf8(json_bytes) {
        Ok(s) => s,
        Err(_) => return,
    };

    let ir = match CadIr::from_json(s) {
        Ok(ir) => ir,
        Err(_) => return,
    };

    let options = StepWriteOptions {
        product_name: format!("Product {}", data[0]),
        author: format!("Author {}", data[1]),
        organization: format!("Org {}", data[2]),
        timestamp: format!("2024-01-{:02}T{:02}:00:00", (data[3] % 28) + 1, data[4] % 24),
        originating_system: format!("System {}", data[5]),
    };

    let mut out = Cursor::new(Vec::new());
    let _ = write_step(&ir, &mut out, &options);
});
