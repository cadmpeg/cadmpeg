// SPDX-License-Identifier: Apache-2.0
//! Fuzzes byte-oriented Part 21 tokenization.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cadmpeg_codec_step::fuzz::lex(data);
});
