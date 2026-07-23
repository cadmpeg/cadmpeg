// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA `7C08` outer object-graph parsing.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_catia::fuzz::object_graph_parse`,
//! which drives object-graph parsing and surface-alias extraction.
//! Contract: no input may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cadmpeg_codec_catia::fuzz::object_graph_parse(data);
});
