// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for Creo compact integer decoding.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_creo::psb::compact_int`
//! to exercise variable-length integer decoding. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_creo::psb::compact_int;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let offset = 0;
    let _ = compact_int(data, offset);
});
