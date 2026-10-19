// SPDX-License-Identifier: Apache-2.0
//! Fuzz decoded `.f3d` patch/replay fallback and immediate re-decode.
//!
//! Retained bytes cannot supply records absent from typed document arenas.
//! Sources with unrepresented content must synthesize from the document.
#![no_main]

use std::io::Cursor;

use cadmpeg_codec_f3d::F3dCodec;
use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::report::export::WritePath;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let codec = F3dCodec;
    let mut source = Cursor::new(data);
    let Ok(decoded) = codec.decode(&mut source, &DecodeOptions::default()) else {
        return;
    };
    let plan = codec
        .plan(
            EncodeInput::new(decoded.ir(), Some(decoded.source_fidelity())),
            TargetRequest::Inherit,
        )
        .expect("decoded document must have an export path");
    if !decoded.source_fidelity().unrepresented.is_empty() {
        assert!(matches!(
            plan.report().write_path(),
            WritePath::Synthesized { .. }
        ));
    }
    let mut encoded = Vec::new();
    plan.write_to(&mut encoded)
        .expect("planned output must write");
    let mut round_trip = Cursor::new(encoded);
    assert!(codec
        .decode(&mut round_trip, &DecodeOptions::default())
        .is_ok());
});
