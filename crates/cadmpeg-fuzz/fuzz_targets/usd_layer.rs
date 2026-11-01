// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for the OpenUSD header classifier and the USDA grammar over
//! raw layer bytes. No input may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cadmpeg_codec_usd::fuzz::header(data);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    if let Ok((ctx, root)) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
    {
        let _layer = cadmpeg_codec_usd::fuzz::layer(&ctx, root);
        let _ = ctx.finish_session();
    }
});
