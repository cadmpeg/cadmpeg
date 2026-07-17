// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for the `.f3d` decode path under both policy modes.
//!
//! Complements `f3d_container` (which decodes only under the default salvage
//! policy) by driving the full decode in strict mode as well, exercising the
//! Phase-4B typed-builder loss channel from both entry points. Contract: no
//! input may panic or abort in either mode; every surfaced loss carries a
//! stable machine code (never an untyped or message-only loss); and decode is
//! deterministic within a mode.
#![no_main]

use std::io::Cursor;

use cadmpeg_codec_f3d::F3dCodec;
use cadmpeg_ir::codec::{CodecEntry, DecodeOptions};
use cadmpeg_ir::decode::{DecodeMode, DecodePolicy, ResourceLimits};
use libfuzzer_sys::fuzz_target;

fn options(mode: DecodeMode) -> DecodeOptions {
    DecodeOptions {
        container_only: false,
        policy: DecodePolicy {
            mode,
            limits: ResourceLimits::default(),
        },
    }
}

fuzz_target!(|data: &[u8]| {
    let codec = F3dCodec;

    for mode in [DecodeMode::Strict, DecodeMode::Salvage] {
        let first = codec.decode(&mut Cursor::new(data), &options(mode));
        let second = codec.decode(&mut Cursor::new(data), &options(mode));

        if let (Ok(a), Ok(b)) = (&first, &second) {
            // Every loss the typed builder resolved must carry a stable code;
            // gates key on it, so an untyped loss would be a silent drop.
            for loss in &a.report.losses {
                assert!(!loss.code.as_str().is_empty());
            }
            // Decode is deterministic within a mode: the same input yields the
            // same ordered loss codes.
            let codes = |report: &cadmpeg_ir::report::DecodeReport| {
                report
                    .losses
                    .iter()
                    .map(|loss| loss.code)
                    .collect::<Vec<_>>()
            };
            assert_eq!(codes(&a.report), codes(&b.report));
        }
    }
});
