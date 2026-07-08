// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for SolidWorks Parasolid stream extraction.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_sldprt::parasolid::extract_streams`
//! to exercise DEFLATE decompression and stream location. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_sldprt::parasolid::extract_streams;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = extract_streams(data);
});
