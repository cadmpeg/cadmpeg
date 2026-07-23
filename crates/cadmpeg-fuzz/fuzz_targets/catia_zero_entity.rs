// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA zero-entity topology parsing.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_catia::fuzz::zero_entity_parse`.
//! Contract: no input may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cadmpeg_codec_catia::fuzz::zero_entity_parse(data);
});
