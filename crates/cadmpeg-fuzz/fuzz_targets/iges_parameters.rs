// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for IGES parameter-section assembly.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_iges::fuzz::parameters`.
//! Contract: no input may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cadmpeg_codec_iges::fuzz::parameters(data);
});
