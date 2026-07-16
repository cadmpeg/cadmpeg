// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA standard-nested and FBB topology parsing.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_catia::topology::parse_standard`
//! and `parse_fbb` to exercise the counted spine and edge-row walks. Contract:
//! no input may panic.

#![no_main]

use cadmpeg_codec_catia::topology::{parse_fbb, parse_standard};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Some(topology) = parse_standard(data) {
        let _ = topology.edge_vertices();
    }
    let _ = parse_fbb(data);
});
