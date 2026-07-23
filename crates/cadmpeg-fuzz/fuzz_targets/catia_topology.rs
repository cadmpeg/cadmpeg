// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA standard-nested and FBB topology parsing.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_catia::fuzz::topology_parse` to
//! exercise the counted spine and edge-row walks. Contract: no input may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cadmpeg_codec_catia::fuzz::topology_parse(data);
});
