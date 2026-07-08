// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA E5 topology parsing.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_catia::e5::parse_topology`
//! to exercise E5 topology parsing. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_catia::e5::parse_topology;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = parse_topology(data);
});
