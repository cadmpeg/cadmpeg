// SPDX-License-Identifier: Apache-2.0
//! Fuzz complete FCStd decode on arbitrary bounded bytes.
#![no_main]

use cadmpeg_codec_freecad::FcstdCodec;
use cadmpeg_ir::codec::{CodecEntry, DecodeOptions};
use libfuzzer_sys::fuzz_target;
use std::io::Cursor;

fuzz_target!(|data: &[u8]| {
    let _ = FcstdCodec.decode(
        &mut Cursor::new(&data[..data.len().min(1 << 20)]),
        &DecodeOptions::default(),
    );
});
