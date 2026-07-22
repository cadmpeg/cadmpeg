// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for NX Parasolid topology parsing.
//! No input may panic.

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
