// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for decoded `.f3d` replay and immediate re-decode.
//!
//! Malformed containers and unsupported semantic writes may return codec
//! errors. A successful decode/replay chain must not panic or abort.
#![no_main]

use std::io::Cursor;

use cadmpeg_codec_f3d::F3dCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::codec::write::{EncodeInput, Encoder, TargetRequest};

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let codec = F3dCodec;
    let mut source = Cursor::new(data);
    let Ok(decoded) = codec.decode(&mut source, &DecodeOptions::default()) else {
        return;
    };
    let mut encoded = Vec::new();
    assert!(codec.plan(EncodeInput::new(decoded.ir(), None), TargetRequest::Inherit).and_then(|plan| plan.write_to(&mut encoded)).is_ok());
    let mut round_trip = Cursor::new(encoded);
    assert!(codec
        .decode(&mut round_trip, &DecodeOptions::default())
        .is_ok());
});
