// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA B5 object-stream graph parsing.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_catia::fuzz::b5_parse`.
//! Contract: no input may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cadmpeg_codec_catia::fuzz::b5_parse(data);
});
