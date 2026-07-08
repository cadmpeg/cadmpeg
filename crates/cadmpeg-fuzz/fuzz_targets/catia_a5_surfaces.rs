// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA A5 freeform surface extraction.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_catia::geometry::a5_surfaces`
//! to exercise freeform surface extraction. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_catia::geometry::a5_surfaces;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = a5_surfaces(data);
});
