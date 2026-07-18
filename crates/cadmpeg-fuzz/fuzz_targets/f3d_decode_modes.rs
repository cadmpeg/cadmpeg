// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for the `.f3d` decode path under both policy modes.
//!
//! Neither mode may panic or abort. Repeated decodes in one mode produce the
//! same ordered loss codes.
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
