// SPDX-License-Identifier: Apache-2.0
//! Fuzzes byte-oriented Part 21 tokenization.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = cadmpeg_step::lex::lex(data);
});
