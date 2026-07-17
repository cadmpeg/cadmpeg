// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for the CATIA Phase-4 strict/salvage decode contract.
//!
//! Feeds arbitrary bytes through `CatiaCodec::decode` in both strict and salvage
//! modes. Contract: no input may panic in either mode, and strict mode never
//! returns `Ok` carrying a blocking loss whose code rejects (`§10` Phase 4) —
//! that is the invariant `decode::enforce_strict` enforces at the typed-lossy
//! builder boundary. Salvage may return either a classified `CodecError` or a
//! partial model with the loss recorded.

#![no_main]

use cadmpeg_codec_catia::CatiaCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::decode::{DecodeMode, DecodePolicy};
use cadmpeg_ir::report::{Severity, StrictConsequence};
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
    let codec = CatiaCodec;

    // Salvage mode: must not panic; result kind is unconstrained here.
    let _ = codec.decode(&mut Cursor::new(data), &options(DecodeMode::Salvage));

    // Strict mode: must not panic, and any successful decode must be free of a
    // blocking Reject-coded loss — strict rejection would otherwise be silent.
    if let Ok(result) = codec.decode(&mut Cursor::new(data), &options(DecodeMode::Strict)) {
        assert!(
            !result.report.losses.iter().any(|loss| {
                loss.severity == Severity::Blocking
                    && loss.code.strict_consequence() == StrictConsequence::Reject
            }),
            "strict decode returned Ok with an unrepresentable mandatory-semantics loss"
        );
    }
});
