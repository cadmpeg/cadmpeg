// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA `7C0B` value block parsing.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_catia::fuzz::value_block_parse`.
//! Contract: no input may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cadmpeg_codec_catia::fuzz::value_block_parse(data);
});
