// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for Creo PSB scalar decoding.
//!
//! Builds a `ScalarCache` from arbitrary section bytes, then feeds arbitrary
//! bytes through the migrated `scalar::decode` and `scalar::decode_in_lane`
//! primitive decoders. Contract: no input may panic and every read stays
//! within the caller-owned slice.

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
