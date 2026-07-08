// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA zero-entity topology parsing.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_catia::zero_entity::parse`
//! to exercise zero-entity topology parsing. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_catia::zero_entity::parse;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = parse(data);
});
