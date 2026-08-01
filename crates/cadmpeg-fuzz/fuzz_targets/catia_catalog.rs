// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA `7C02` string catalog parsing.
//! No input may panic.

#![no_main]

use cadmpeg_codec_catia::fuzz::catalog;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    catalog(data);
});
