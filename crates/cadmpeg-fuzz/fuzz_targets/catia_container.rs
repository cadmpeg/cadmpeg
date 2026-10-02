// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for the CATIA V5 `.CATPart` codec.
//! No input may panic. Malformed input must return `CodecError`.

#![no_main]

use cadmpeg_codec_catia::CatiaCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use cadmpeg_core::decode::InspectOptions;
use libfuzzer_sys::fuzz_target;
use std::io::Cursor;

fuzz_target!(|data: &[u8]| {
    let codec = CatiaCodec;

    let arena = cadmpeg_core::decode::DecodeArena::new();
    if let Ok((ctx, root)) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        data,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    ) {
        let _ = codec.detect(&ctx, root);
        let _ = ctx.finish_session();
    }

    let mut inspect_cur = Cursor::new(data);
    let _ = codec.inspect(&mut inspect_cur, &InspectOptions::default());

    let mut decode_cur = Cursor::new(data);
    let _ = codec.decode(&mut decode_cur, &DecodeOptions::default());
});
