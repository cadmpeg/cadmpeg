// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for the `.f3d` container parser.
//!
//! Runs arbitrary bytes through detection, container inspection, and decoding.
//! Codec errors are expected for malformed input; panics and aborts are
//! failures.
#![no_main]

use std::io::Cursor;

use cadmpeg_codec_f3d::F3dCodec;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use cadmpeg_core::decode::InspectOptions;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let codec = F3dCodec;

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
