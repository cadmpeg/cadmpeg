// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for Creo PSB token stream parsing.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_creo::psb::tokens`
//! to exercise PSB token stream parsing. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_creo::psb::tokens;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = tokens(data);
});
