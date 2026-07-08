// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for NX geometry point extraction.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_nx::geometry::points`
//! to exercise POINT vertex extraction from Parasolid streams. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_nx::geometry::points;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = points(data);
});
