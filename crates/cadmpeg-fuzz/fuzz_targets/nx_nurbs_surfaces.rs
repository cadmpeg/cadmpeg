// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for NX NURBS surface extraction.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_nx::fuzz::nurbs_surfaces`
//! to exercise NURBS surface extraction from Parasolid streams. Contract: no input may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cadmpeg_codec_nx::fuzz::nurbs_surfaces(data);
});
