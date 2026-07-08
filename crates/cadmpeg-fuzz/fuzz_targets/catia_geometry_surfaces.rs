// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA geometry surface extraction.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_catia::geometry::surface_prefixes`
//! to exercise surface prefix extraction. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_catia::geometry::surface_prefixes;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = surface_prefixes(data);
});
