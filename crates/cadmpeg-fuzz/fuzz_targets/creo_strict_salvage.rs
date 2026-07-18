// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for the Creo strict/salvage decode contract.
//!
//! Neither mode may panic. A successful strict decode contains no loss whose
//! strict consequence is rejection.

#![no_main]

use cadmpeg_codec_creo::CreoCodec;
use cadmpeg_ir::codec::{CodecEntry, DecodeOptions};
use cadmpeg_ir::decode::{DecodeMode, DecodePolicy};
use cadmpeg_ir::report::StrictConsequence;
use libfuzzer_sys::fuzz_target;
use std::io::Cursor;

fn options(mode: DecodeMode) -> DecodeOptions {
    DecodeOptions {
        container_only: false,
        policy: DecodePolicy {
            mode,
            ..Default::default()
        },
    }
}

fuzz_target!(|data: &[u8]| {
    let codec = CreoCodec;

    let _ = codec.decode(&mut Cursor::new(data), &options(DecodeMode::Salvage));

    if let Ok(result) = codec.decode(&mut Cursor::new(data), &options(DecodeMode::Strict)) {
        assert!(
            !result
                .report
                .losses
                .iter()
                .any(|loss| loss.code.strict_consequence() == StrictConsequence::Reject),
            "strict decode returned Ok with an unrepresentable mandatory-semantics loss"
        );
    }
});
