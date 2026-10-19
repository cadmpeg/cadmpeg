// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for F3D ASM header parsing.
//!
//! Feeds arbitrary bytes through `cadmpeg_asm::asm_header::parse` to
//! exercise magic detection and header field parsing. Contract: no input may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    drop(std::hint::black_box(cadmpeg_asm::asm_header::parse(data)));
});
