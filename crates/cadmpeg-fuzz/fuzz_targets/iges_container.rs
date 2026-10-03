// SPDX-License-Identifier: Apache-2.0
//! Fuzzes IGES representation detection, physical framing, and full decode.

#![no_main]

use std::io::Cursor;

use cadmpeg_codec_iges::IgesCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let codec = IgesCodec;
    let arena = cadmpeg_core::decode::DecodeArena::new();
    if let Ok((ctx, root)) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        data,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    ) {
        let _ = codec.detect(&ctx, root);
        let _ = ctx.finish_session();
    }
    let _ = codec.inspect(
        &mut Cursor::new(data),
        &cadmpeg_core::decode::InspectOptions::default(),
    );
    let _ = codec.decode(&mut Cursor::new(data), &DecodeOptions::default());
});
