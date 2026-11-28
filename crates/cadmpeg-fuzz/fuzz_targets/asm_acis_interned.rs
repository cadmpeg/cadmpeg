// SPDX-License-Identifier: Apache-2.0
//! Fuzz the interned ACIS binary framer over a bare record stream.

#![no_main]

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    std::hint::black_box(cadmpeg_asm::acis_interned::applies(data, 0));
    let arena = DecodeArena::new();
    let Ok((ctx, _)) = DecodeContext::from_root_bytes(data, &arena, &DecodePolicy::service())
    else {
        return;
    };
    drop(cadmpeg_asm::acis_interned::frame(&ctx, data, 0, data.len(), None));
    let _ = ctx.finish_session();
});
