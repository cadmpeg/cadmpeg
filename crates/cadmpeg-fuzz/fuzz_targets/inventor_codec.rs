// SPDX-License-Identifier: Apache-2.0
//! Fuzz Inventor detection, inspection, and bounded full decode.

#![no_main]

use std::io::Cursor;

use cadmpeg_codec_inventor::InventorCodec;
use cadmpeg_core::decode::{DecodePolicy, InspectOptions};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let codec = InventorCodec;
    let options = DecodeOptions {
        container_only: false,
        policy: DecodePolicy::service(),
    };
    let _ = codec.detect(data);
    let _ = codec.inspect(&mut Cursor::new(data), &InspectOptions::default());
    let _ = codec.decode(&mut Cursor::new(data), &options);
});
