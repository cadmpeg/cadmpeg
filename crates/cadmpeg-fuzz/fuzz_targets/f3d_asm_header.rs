// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for F3D ASM header parsing.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_f3d::asm_header::parse` to
//! exercise magic detection and header field parsing. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_f3d::asm_header::parse;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = parse(data);
});
