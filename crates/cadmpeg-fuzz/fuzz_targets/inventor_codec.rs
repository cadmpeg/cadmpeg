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
    let arena = cadmpeg_core::decode::DecodeArena::new();
        if let Ok((ctx, root)) = cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &cadmpeg_core::decode::DecodePolicy::service()) {
            let _ = codec.detect(&ctx, root);
            let _ = ctx.finish_session();
        }
    let _ = codec.inspect(&mut Cursor::new(data), &InspectOptions::default());
    let _ = codec.decode(&mut Cursor::new(data), &options);
});
