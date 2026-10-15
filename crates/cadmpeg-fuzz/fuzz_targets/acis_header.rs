// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for binary ACIS header and solved-partition parsing.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    if let Ok((ctx, _)) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        data,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    ) {
        drop(std::hint::black_box(cadmpeg_asm::acis_header::parse(
            &ctx, data,
        )));
        std::hint::black_box(cadmpeg_asm::acis_header::record_stream_start(data));
        std::hint::black_box(cadmpeg_asm::acis_header::solved_record_limit(data));
    }
});
