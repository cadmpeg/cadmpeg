// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for the Alibre `SAB_DATA` stream: classification, identifier
//! table framing, extended-base normalization, and the shared ASM decoders over
//! raw stream bytes. No input may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    if let Ok((ctx, _root)) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
    {
        let _result = cadmpeg_codec_alibre::fuzz::sab_stream(&ctx, data);
        let _ = ctx.finish_session();
    }
});
