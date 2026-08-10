// SPDX-License-Identifier: Apache-2.0
//! Fuzz Protein ZIP schema and paged instance-property decoding.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let split = data.len() / 2;
    let _ = cadmpeg_protein::has_schemas(&data[..split]);
    let _ = cadmpeg_protein::decode(&data[..split], &data[split..]);
});
