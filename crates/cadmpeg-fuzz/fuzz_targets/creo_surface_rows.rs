// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for Creo surface row extraction.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_creo::surface::rows`
//! to exercise surface namespace row extraction. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_creo::surface::rows;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = rows(data);
});
