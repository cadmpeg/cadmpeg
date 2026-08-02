// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA zero-entity topology parsing.
//! No input may panic.

#![no_main]

use cadmpeg_codec_catia::fuzz::zero_entity;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    zero_entity(data);
});
