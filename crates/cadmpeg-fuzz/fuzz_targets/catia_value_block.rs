// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA `7C0B` value block parsing.
//! No input may panic.

#![no_main]

use cadmpeg_codec_catia::fuzz::value_blocks;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    value_blocks(data);
});
