// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for Creo PSB scalar decoding.
//! No input may panic or read outside the input slice.

#![no_main]

use cadmpeg_codec_creo::scalar::{decode, decode_in_lane, ScalarCache};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let cache = ScalarCache::from_section(data);
    let mut offset = 0usize;
    while offset < data.len() {
        match decode_in_lane(data, offset, &cache) {
            Some((_, next)) if next > offset => offset = next,
            _ => break,
        }
    }
    let _ = decode(data, 0);
});
