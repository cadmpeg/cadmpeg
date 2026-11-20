// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for SolidCraft design detection, file opening, document
//! parsing, classification, the blob audit, the model cache body slice
//! parsers and B-rep emitter, and the whole-codec inspect and decode paths.
//! No input may panic.

#![no_main]

use std::io::Cursor;

use cadmpeg_codec_solidcraft::SolidcraftCodec;
use cadmpeg_core::decode::InspectOptions;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    if let Ok((ctx, root)) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
    {
        let _detect = cadmpeg_codec_solidcraft::fuzz::detect(&ctx, data);
        let _source = cadmpeg_codec_solidcraft::fuzz::source(&ctx, root);
        let _body = cadmpeg_codec_solidcraft::fuzz::cache_body(&ctx, root);
        let _ = ctx.finish_session();
    }
    let _ = SolidcraftCodec.inspect(&mut Cursor::new(data), &InspectOptions::default());
    let _ = SolidcraftCodec.decode(&mut Cursor::new(data), &DecodeOptions::default());
});
