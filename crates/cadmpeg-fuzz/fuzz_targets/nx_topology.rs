// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for NX Parasolid topology parsing.
//!
//! Feeds arbitrary bytes through the migrated `cadmpeg_codec_nx::topology`
//! record scanners — the `Graph` index plus the composite, intersection,
//! blend, offset, surface, and trimmed-curve extractors — to exercise the
//! fixed-record framing, XMT reference sequences, and large-index shift
//! accumulation on the committed decode path. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_nx::topology;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = topology::Graph::parse(data);
    let _ = topology::composite_curves(data);
    let _ = topology::intersection_data_curves(data);
    let _ = topology::blend_surfaces(data);
    let _ = topology::offset_surfaces(data);
    let _ = topology::surface_curves(data);
    let _ = topology::trimmed_curves(data);
});
