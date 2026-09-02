// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for STEP encoder planning and writing.
//!
//! Feeds arbitrary bytes through UTF-8 decoding, JSON deserialization into
//! `CadIr`, then STEP export. Contract: no input may panic. Malformed JSON must
//! surface as `serde_json::Error`; STEP export errors are discarded.
//!

#![no_main]

use std::io::Cursor;

use cadmpeg_codec_step::{StepCodec, StepSchema};
use cadmpeg_ir::codec::write::{EncodeInput, Encoder, TargetRequest};
use cadmpeg_ir::CadIr;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(s) = std::str::from_utf8(data) {
        if let Ok(ir) = CadIr::from_json(s) {
            let mut out = Cursor::new(Vec::new());
            let _ = StepCodec::default()
                .plan(
                    EncodeInput::new(&ir, None),
        TargetRequest::Explicit(StepSchema::default().descriptor().id.as_str()),
                )
                .and_then(|plan| plan.write_to(&mut out));
        }
    }
});
