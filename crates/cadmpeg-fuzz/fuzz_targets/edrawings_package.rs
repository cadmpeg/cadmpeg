// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for eDrawings package admission, the `eModel` wrapper, and
//! dialect classification over raw archive bytes. No input may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    if let Ok((ctx, root)) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
    {
        let _result = cadmpeg_codec_edrawings::fuzz::package(&ctx, root);
        let _ = ctx.finish_session();
    }
});
