// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for STEP writer with degenerate IR geometry.
//!
//! Constructs CadIr documents with NaN, infinity, zero-length vectors,
//! and other degenerate geometry, then exports to STEP.
//! Contract: no input may panic.

#![no_main]

use std::io::Cursor;

use cadmpeg_codec_step::{StepCodec, StepSchema};
use cadmpeg_ir::codec::write::{EncodeInput, Encoder, TargetRequest};
use cadmpeg_ir::CadIr;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 100 {
        return;
    }

    // Parse as JSON IR
    let json_str = match std::str::from_utf8(data) {
        Ok(s) => s,
        Err(_) => return,
    };

    let ir = match CadIr::from_json(json_str) {
        Ok(ir) => ir,
        Err(_) => return,
    };

    // Write to STEP - should handle degenerate geometry gracefully
    let mut out = Cursor::new(Vec::new());
    let _ = StepCodec::default()
        .plan(
            EncodeInput::new(&ir, None),
            TargetRequest::Explicit(StepSchema::default().descriptor().id.as_str()),
        )
        .and_then(|plan| plan.write_to(&mut out));
});
