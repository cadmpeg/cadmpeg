// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA `7C08` outer object-graph parsing.
//! No input may panic.

#![no_main]

use cadmpeg_codec_catia::fuzz::object_graph;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    object_graph(data);
});
