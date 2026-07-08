// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for NX NURBS surface extraction.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_nx::nurbs::surfaces`
//! to exercise NURBS surface extraction from Parasolid streams. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_nx::nurbs::surfaces;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = surfaces(data);
});
