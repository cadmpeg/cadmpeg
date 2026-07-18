// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for `cadmpeg_step::write_step`.
//!
//! Feeds arbitrary bytes through UTF-8 decoding, JSON deserialization into
//! `CadIr`, then STEP export. Contract: no input may panic. Malformed JSON must
//! surface as `serde_json::Error`; STEP export errors are discarded.
//!

#![no_main]

use std::io::Cursor;

use cadmpeg_ir::CadIr;
use cadmpeg_step::{write_step, StepWriteOptions};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        if let Ok(ir) = CadIr::from_json(s) {
            let mut out = Cursor::new(Vec::new());
            let _ = write_step(&ir, &mut out, &StepWriteOptions::default());
        }
    }
});
