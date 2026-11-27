// SPDX-License-Identifier: Apache-2.0
//! Fuzz the IronCAD record tree, `Public Access` part records, and
//! `Document Data` unit entries. The first byte selects the parser.

#![no_main]

use cadmpeg_codec_ironcad::fuzz;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Some((&selector, rest)) = data.split_first() else {
        return;
    };
    match selector % 3 {
        0 => fuzz::record_tree(rest),
        1 => fuzz::public_access(rest),
        _ => fuzz::document_units(rest),
    }
});
