// SPDX-License-Identifier: Apache-2.0
//! Fuzzes Rhino SubD framing, ID maps, and directed rings.

#![no_main]

use cadmpeg_codec_rhino::fuzz;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| fuzz::subd(data));
