// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA A8 NURBS surface extraction.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_catia::families::a5a8::records::a8_surfaces`
//! to exercise NURBS surface extraction. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_catia::families::a5a8::records::a8_surfaces;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = a8_surfaces(data);
});
