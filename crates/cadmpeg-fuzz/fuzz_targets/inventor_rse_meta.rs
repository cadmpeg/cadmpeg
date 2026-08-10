// SPDX-License-Identifier: Apache-2.0
//! Fuzz Inventor RSe metadata envelopes and tables.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| cadmpeg_codec_inventor::fuzzing::meta_stream(data));
