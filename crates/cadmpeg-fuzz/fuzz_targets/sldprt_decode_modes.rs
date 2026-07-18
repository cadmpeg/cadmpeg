// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for the `.sldprt` decode path under both policy modes.
//!
//! Neither mode may panic or abort. Losses have stable machine codes, a
//! successful strict decode contains no reject-consequence loss, and repeated
//! decodes in one mode produce the same ordered loss codes.
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
                assert!(!loss.code.as_str().is_empty());
                if mode == DecodeMode::Strict {
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
