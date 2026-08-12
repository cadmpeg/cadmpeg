// SPDX-License-Identifier: Apache-2.0
//! Fuzzes Rhino object, class, userdata, and attribute framing.

#![no_main]

use cadmpeg_codec_rhino::fuzz;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| fuzz::object_record(data));
