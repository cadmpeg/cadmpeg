// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for USDZ package admission and its root layer reader over raw bytes. No input may
//! panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    if let Ok((ctx, root)) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
    {
        let _layer = cadmpeg_codec_usd::fuzz::package(&ctx, root);
        let _ = ctx.finish_session();
    }
});
