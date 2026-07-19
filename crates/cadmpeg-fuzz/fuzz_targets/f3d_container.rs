// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for the `.f3d` container parser.
//!
//! Runs arbitrary bytes through detection, container inspection, and decoding.
//! Codec errors are expected for malformed input; panics and aborts are
//! failures.
#![no_main]

use std::io::Cursor;

use cadmpeg_codec_f3d::F3dCodec;
use cadmpeg_ir::codec::{Codec, CodecEntry, DecodeOptions};
use cadmpeg_ir::decode::InspectOptions;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let codec = F3dCodec;

    let _ = codec.detect(data);

    let mut inspect_cur = Cursor::new(data);
    let _ = codec.inspect(&mut inspect_cur, &InspectOptions::default());

    let mut decode_cur = Cursor::new(data);
    let _ = codec.decode(&mut decode_cur, &DecodeOptions::default());
});
