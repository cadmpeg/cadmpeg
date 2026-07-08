// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA topology orientation solving.
//!
//! Tests the orientation solving algorithm in e5.rs with malformed
//! topology graphs that could cause expect() calls to panic.
//! Contract: no input may panic.

#![no_main]

use cadmpeg_codec_catia::e5::parse_topology;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // The orientation solver can panic if the graph traversal doesn't visit all nodes
    // or if there are inconsistencies in the topology graph.
    // Feed arbitrary bytes to exercise these code paths.
    let _ = parse_topology(data);
});
