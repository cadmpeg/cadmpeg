// SPDX-License-Identifier: Apache-2.0
//! Fuzzes Rhino RawBrep framing and validation.

#![no_main]

use cadmpeg_codec_rhino::fuzzing;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| fuzzing::brep(data));
