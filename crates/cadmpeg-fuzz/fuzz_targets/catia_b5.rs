// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA B5 topology parsing.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_catia::families::b5::graph::parse`
//! to exercise B5 graph parsing. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_catia::families::b5::graph::parse;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = parse(data);
});
