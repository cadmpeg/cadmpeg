// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for IGES global-section parsing.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_iges::fuzz::global`.
//! Contract: no input may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cadmpeg_codec_iges::fuzz::global(data);
});
