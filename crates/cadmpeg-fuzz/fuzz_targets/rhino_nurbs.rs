// SPDX-License-Identifier: Apache-2.0
//! Fuzzes Rhino NURBS curve, surface, and plane payloads.

#![no_main]

use cadmpeg_codec_rhino::fuzz;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| fuzz::nurbs(data));
