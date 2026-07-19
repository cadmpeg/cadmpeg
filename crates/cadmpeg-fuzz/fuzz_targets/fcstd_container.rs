// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for FCStd detection, inspection, and container decoding.
#![no_main]

use std::io::Cursor;

use cadmpeg_codec_freecad::FcstdCodec;
use cadmpeg_ir::codec::{Codec, CodecEntry, DecodeOptions};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let codec = FcstdCodec;
    let _ = codec.detect(data);
    let _ = codec.inspect(
        &mut Cursor::new(data),
        &cadmpeg_ir::decode::InspectOptions::default(),
    );
    let _ = codec.decode(
        &mut Cursor::new(data),
        &DecodeOptions {
            container_only: true,
            ..DecodeOptions::default()
        },
    );
});
