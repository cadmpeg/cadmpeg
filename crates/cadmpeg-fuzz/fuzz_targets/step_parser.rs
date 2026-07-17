// SPDX-License-Identifier: Apache-2.0
//! Fuzzes complete Part 21 parsing and reference resolution.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = cadmpeg_step::parse::parse(data);
});
