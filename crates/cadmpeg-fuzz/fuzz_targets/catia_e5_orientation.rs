// SPDX-License-Identifier: Apache-2.0
//! Exercises CATIA E5 topology parsing and orientation solving with arbitrary
//! graph data. Parse errors are expected; panics are failures.

#![no_main]

use cadmpeg_codec_catia::e5::parse_topology;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = parse_topology(data);
});
