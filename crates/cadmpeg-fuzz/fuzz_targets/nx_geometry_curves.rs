// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for NX geometry curve extraction.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_nx::geometry::curves`
//! to exercise analytic curve extraction from Parasolid streams. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_nx::geometry::curves;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = curves(data);
});
