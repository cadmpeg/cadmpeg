// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for NX geometry point extraction.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_nx::fuzz::geometry_points`
//! to exercise POINT vertex extraction from Parasolid streams. Contract: no input may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cadmpeg_codec_nx::fuzz::geometry_points(data);
});
