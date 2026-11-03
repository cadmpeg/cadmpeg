// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for decoded USDA layers written as USDA 1.0 and decoded again.
//!
//! Malformed layers may return codec errors. A layer that decodes must write,
//! and the written layer must decode.
#![no_main]

use std::io::Cursor;

use cadmpeg_codec_usd::UsdCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let codec = UsdCodec;
    let mut source = Cursor::new(data);
    let Ok(decoded) = codec.decode(&mut source, &DecodeOptions::default()) else {
        return;
    };
    let mut encoded = Vec::new();
    assert!(codec
        .plan(
            EncodeInput::new(decoded.ir(), None),
            TargetRequest::Explicit("usda-1.0")
        )
        .and_then(|plan| plan.write_to(&mut encoded))
        .is_ok());
    let mut round_trip = Cursor::new(encoded);
    assert!(codec
        .decode(&mut round_trip, &DecodeOptions::default())
        .is_ok());
});
