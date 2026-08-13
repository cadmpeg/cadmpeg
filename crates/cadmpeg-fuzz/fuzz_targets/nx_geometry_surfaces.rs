// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for NX geometry surface extraction.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_nx::fuzz::geometry_surfaces`
//! to exercise analytic surface extraction from Parasolid streams. Contract: no input may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cadmpeg_codec_nx::fuzz::geometry_surfaces(data);
});
