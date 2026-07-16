// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for NX object-model section framing.
//!
//! Feeds arbitrary bytes through the migrated `cadmpeg_codec_nx::om`
//! entity-index/object-id-table pairing and its numeric-expression decode to
//! exercise the count-framed boundary arrays whose reservations this module
//! migrated off `Vec::with_capacity`. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_nx::om::indexed_sections;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    for section in indexed_sections(data) {
        let _ = section.numeric_expressions();
    }
});
