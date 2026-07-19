// SPDX-License-Identifier: Apache-2.0
//! Fuzzes IGES representation detection, physical framing, and full decode.

#![no_main]

use std::io::Cursor;

use cadmpeg_codec_iges::IgesCodec;
use cadmpeg_ir::codec::{Codec, CodecEntry, DecodeOptions};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let codec = IgesCodec;
    let _ = codec.detect(data);
    let _ = codec.inspect(
        &mut Cursor::new(data),
        &cadmpeg_ir::decode::InspectOptions::default(),
    );
    let _ = codec.decode(&mut Cursor::new(data), &DecodeOptions::default());
});
