// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for NX object-model section framing.
//! No input may panic.

#![no_main]

use cadmpeg_codec_nx::om::indexed_sections;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    for section in indexed_sections(data) {
        let _ = section.numeric_expressions();
    }
});
