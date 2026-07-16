// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for NX Parasolid deltas-stream walking.
//!
//! Feeds arbitrary bytes through the migrated `cadmpeg_codec_nx::deltas`
//! status-byte-framed record walk and point extraction to exercise the
//! record-boundary scan on the committed decode path. Contract: no input may
//! panic.

#![no_main]

use cadmpeg_codec_nx::deltas;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = deltas::walk(data);
    let _ = deltas::points(data);
});
