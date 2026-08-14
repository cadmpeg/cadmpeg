// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for IGES directory-section parsing.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_iges::fuzz::directory`.
//! Contract: no input may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cadmpeg_codec_iges::fuzz::directory(data);
});
