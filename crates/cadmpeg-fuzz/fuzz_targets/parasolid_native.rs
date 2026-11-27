// SPDX-License-Identifier: Apache-2.0
//! Fuzz the native-binary Parasolid rewrite and the neutral node reader on
//! its output.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(neutral) = cadmpeg_parasolid::native::to_neutral(data) {
        drop(cadmpeg_parasolid::records::PrimaryRecords::read(&neutral));
    }
});
