// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for the Siemens NX `.prt` codec.
//! No input may panic. Malformed input must return `CodecError`.

#![no_main]

use cadmpeg_codec_nx::NxCodec;
use cadmpeg_ir::codec::{Codec, CodecEntry, DecodeOptions};
use cadmpeg_ir::decode::InspectOptions;
use libfuzzer_sys::fuzz_target;
use std::io::Cursor;

fuzz_target!(|data: &[u8]| {
    let codec = NxCodec;

    let _ = codec.detect(data);

    let mut inspect_cur = Cursor::new(data);
    let _ = codec.inspect(&mut inspect_cur, &InspectOptions::default());

    let mut decode_cur = Cursor::new(data);
    let _ = codec.decode(&mut decode_cur, &DecodeOptions::default());
});
