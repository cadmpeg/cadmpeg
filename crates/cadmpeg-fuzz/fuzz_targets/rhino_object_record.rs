// SPDX-License-Identifier: Apache-2.0
//! Fuzzes Rhino object, class, userdata, and attribute framing.

#![no_main]

use cadmpeg_codec_rhino::fuzzing;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| fuzzing::object_record(data));
