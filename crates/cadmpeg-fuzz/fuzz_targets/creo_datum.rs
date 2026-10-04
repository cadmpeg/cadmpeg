// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for Creo `ActDatums` model-space plane decoding.
//! No input may panic or read outside the input slice.

#![no_main]

use cadmpeg_codec_creo::fuzz::datum;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    if let Ok((ctx, _)) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
    {
        let _result = datum(&ctx, data);
    }
});
