// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for NX surface-intersection chart decoding.
//! No input may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cadmpeg_codec_nx::fuzz::intersection(data);
});
