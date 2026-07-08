// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for Creo short-form float decoding.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_creo::psb::short_form_float`
//! to exercise 3-byte compact float decoding. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_creo::psb::short_form_float;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let offset = 0;
    let _ = short_form_float(data, offset);
});
