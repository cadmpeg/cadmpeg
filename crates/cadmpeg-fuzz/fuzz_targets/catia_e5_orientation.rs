// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA E5 topology parsing and orientation solving.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_catia::fuzz::e5_topology`.
//! Contract: no input may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cadmpeg_codec_catia::fuzz::e5_topology(data);
});
