// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for the eDrawings model stream: record walker, scene
//! collection, shell decoding, and B-rep emission over raw inflated bytes. No
//! input may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    if let Ok((ctx, root)) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
    {
        let _result = cadmpeg_codec_edrawings::fuzz::model(&ctx, root);
        let _ = ctx.finish_session();
    }
});
