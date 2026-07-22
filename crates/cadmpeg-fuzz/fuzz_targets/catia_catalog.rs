// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA `7C02` string catalog parsing.
//! No input may panic.

#![no_main]

use cadmpeg_codec_catia::catalog::parse;
use cadmpeg_ir::decode::{DecodeArena, DecodeContext, DecodePolicy};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    if let Ok((ctx, root)) = DecodeContext::from_root_bytes(data, &arena, &policy) {
        let _ = parse(&ctx, root);
    }
});
