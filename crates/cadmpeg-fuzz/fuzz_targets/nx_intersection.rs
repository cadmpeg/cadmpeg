// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for NX surface-intersection chart decoding.
//!
//! Feeds arbitrary bytes through the migrated `cadmpeg_codec_nx::intersection`
//! chart-backed curve reconstruction to exercise the point-count framing and
//! chord-length parameter accumulation on the committed decode path. Contract:
//! no input may panic.

#![no_main]

use cadmpeg_codec_nx::intersection;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = intersection::curves(data);
});
