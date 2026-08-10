// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for binary ACIS header and solved-partition parsing.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = cadmpeg_asm::acis_header::parse(data);
    let _ = cadmpeg_asm::acis_header::record_stream_start(data);
    let _ = cadmpeg_asm::acis_header::solved_record_limit(data);
});
