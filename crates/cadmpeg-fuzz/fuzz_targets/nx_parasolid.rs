// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for NX Parasolid stream extraction.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_nx::parasolid::extract_streams`
//! to exercise zlib inflation and stream location. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_nx::parasolid::extract_streams;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = extract_streams(data);
});
