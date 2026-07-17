// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for the `.sldprt` decode path under both policy modes.
//!
//! Complements `sldprt_container` (which decodes only under the default salvage
//! policy) by driving the full decode in strict mode as well, exercising the
//! Phase-4B typed-builder loss channel and the strict-mode semantic rejection
//! from both entry points. Contract: no input may panic or abort in either
//! mode; every surfaced loss carries a stable machine code (never an untyped or
//! message-only loss); a successful strict decode never keeps a
//! reject-consequence loss; and decode is deterministic within a mode.
#![no_main]

use std::io::Cursor;

use cadmpeg_codec_sldprt::SldprtCodec;
use cadmpeg_ir::codec::{CodecEntry, DecodeOptions};
use cadmpeg_ir::decode::{DecodeMode, DecodePolicy, ResourceLimits};
use cadmpeg_ir::report::{DecodeReport, StrictConsequence};
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
    let codec = SldprtCodec;

    for mode in [DecodeMode::Strict, DecodeMode::Salvage] {
        let first = codec.decode(&mut Cursor::new(data), &options(mode));
        let second = codec.decode(&mut Cursor::new(data), &options(mode));

        if let (Ok(a), Ok(b)) = (&first, &second) {
            for loss in &a.report.losses {
                // Every loss the typed builder resolved carries a stable code.
                assert!(!loss.code.as_str().is_empty());
                if mode == DecodeMode::Strict {
                    // A strict decode that returned Ok cannot keep a loss whose
                    // consequence is Reject: those are refused, not tolerated.
                    assert_ne!(
                        loss.code.strict_consequence(),
                        StrictConsequence::Reject,
                        "strict decode retained a reject-consequence loss"
                    );
                }
            }
            let codes = |report: &DecodeReport| {
                report.losses.iter().map(|loss| loss.code).collect::<Vec<_>>()
            };
            assert_eq!(codes(&a.report), codes(&b.report));
        }
    }
});
