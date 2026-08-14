// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for IGES physical-card scanning.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_iges::fuzz::cards`.
//! Contract: no input may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cadmpeg_codec_iges::fuzz::cards(data);
});
