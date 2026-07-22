// SPDX-License-Identifier: Apache-2.0
//! Fuzzes the public STEP inspection path.

#![no_main]

use std::io::Cursor;

use cadmpeg_ir::codec::Codec;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = cadmpeg_step::StepCodec::default().inspect(&mut Cursor::new(data));
});
