// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for Creo curve prototype extraction.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_creo::curve::prototypes`
//! to exercise curve prototype extraction. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_creo::curve::prototypes;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = prototypes(data);
});
