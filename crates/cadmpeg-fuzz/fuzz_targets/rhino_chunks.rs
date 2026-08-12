// SPDX-License-Identifier: Apache-2.0
//! Fuzzes Rhino chunk offsets, bounds, and checksums.

#![no_main]

use cadmpeg_codec_rhino::fuzz;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| fuzz::chunks(data));
