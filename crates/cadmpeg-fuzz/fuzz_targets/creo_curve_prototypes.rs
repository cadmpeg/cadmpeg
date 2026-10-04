// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for Creo curve prototype extraction.
//! No input may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    if let Ok((ctx, _)) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
    {
        let _result = cadmpeg_codec_creo::fuzz::curve_prototypes(&ctx, data);
    }
});
