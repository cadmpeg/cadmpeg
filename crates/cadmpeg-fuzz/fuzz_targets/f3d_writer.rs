// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for source-less `.f3d` generation and immediate re-decode.
//!
//! Invalid document state and format capacity limits may refuse generation.
//! Content without an exact encoding is degraded and reported. Every successful
//! encode must remain safe to inspect and decode.
#![no_main]

use std::io::Cursor;

use cadmpeg_codec_f3d::F3dCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::CadIr;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(ir) = CadIr::from_json(text) else {
        return;
    };
    let codec = F3dCodec;
    let mut encoded = Vec::new();
    if codec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .is_ok()
    {
        let mut inspect = Cursor::new(encoded.as_slice());
        assert!(codec
            .inspect(&mut inspect, &InspectOptions::default())
            .is_ok());
        let mut decode = Cursor::new(encoded.as_slice());
        assert!(codec.decode(&mut decode, &DecodeOptions::default()).is_ok());
    }
});
