// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for the NX Phase-4 strict/salvage decode contract.
//!
//! Feeds arbitrary bytes through `NxCodec::decode` in both strict and salvage
//! modes over the entity-decode entry (`container_only: false`). Contract: no
//! input may panic in either mode, and on that entry a successful strict decode
//! never carries a reject-consequence loss (`§10` Phase 4) — that is the
//! invariant `decode::reject_unrepresentable_in_strict` enforces. The
//! container-only strict path emits its own blocking `GeometryNotTransferred`
//! and is not exercised here. Salvage may return either a classified
//! `CodecError` or a partial model with the loss recorded.

#![no_main]

use cadmpeg_codec_nx::NxCodec;
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
    let codec = NxCodec;

    // Salvage mode: must not panic; result kind is unconstrained here.
    let _ = codec.decode(&mut Cursor::new(data), &options(DecodeMode::Salvage));

    // Strict mode: must not panic, and any successful decode must be free of a
    // Reject-coded loss — the gate rejects those regardless of severity, so a
    // strict Ok that kept one would be a silent bypass.
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
