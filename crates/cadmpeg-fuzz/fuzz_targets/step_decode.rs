// SPDX-License-Identifier: Apache-2.0
//! Fuzzes the public STEP decode path, including semantic IR construction.

#![no_main]

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = cadmpeg_codec_step::StepCodec::default()
        .decode(&mut Cursor::new(data), &DecodeOptions::default());
});
