// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for F3D ASM header parsing.
//!
//! Feeds arbitrary bytes through `cadmpeg_asm::asm_header::parse` to
//! exercise magic detection and header field parsing. Contract: no input may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    if let Ok((ctx, _)) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        data,
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    ) {
        drop(std::hint::black_box(cadmpeg_asm::asm_header::parse(
            &ctx, data,
        )));
    }
});
