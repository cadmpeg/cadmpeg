// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for source-less `.f3d` generation and immediate re-decode.
//!
//! Valid current-version IR may be unsupported by the bounded native writer;
//! ordinary codec errors are expected. Every successful encode must remain
//! safe to inspect and decode.
#![no_main]

use std::io::Cursor;

use cadmpeg_codec_f3d::F3dCodec;
use cadmpeg_ir::codec::{CodecEntry, DecodeOptions, Encoder};
use cadmpeg_ir::decode::InspectOptions;
use cadmpeg_ir::CadIr;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    let Ok(ir) = CadIr::from_json(text) else {
        return;
    };
    if ir.native_unknowns("f3d").is_ok_and(|records| {
        records
            .iter()
            .any(|record| record.id.0 == "f3d:file:source-image#0")
    }) {
        return;
    }
    let codec = F3dCodec;
    let mut encoded = Vec::new();
    if codec.encode(&ir, &mut encoded).is_ok() {
        let mut inspect = Cursor::new(encoded.as_slice());
        assert!(codec
            .inspect(&mut inspect, &InspectOptions::default())
            .is_ok());
        let mut decode = Cursor::new(encoded.as_slice());
        assert!(codec
            .decode(&mut decode, &DecodeOptions::default())
            .is_ok());
    }
});
