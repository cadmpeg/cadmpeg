// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA geometry vertex extraction.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_catia::geometry::vertices`
//! to exercise vertex point extraction. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_catia::geometry::vertices;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = vertices(data);
});
