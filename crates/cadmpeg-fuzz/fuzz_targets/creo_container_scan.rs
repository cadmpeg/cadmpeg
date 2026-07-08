// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for Creo container scanning.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_creo::container::scan_bytes`
//! to exercise PSB container scanning. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_creo::container::scan_bytes;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = scan_bytes(data.to_vec());
});
