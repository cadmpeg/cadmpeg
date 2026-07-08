// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for F3D SAB record stream framing.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_f3d::sab::frame` to exercise
//! the token-by-token binary parser. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_f3d::sab::frame;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 16 {
        return;
    }
    let start = 0;
    let limit = data.len();
    let ref_width = 4;
    let _ = frame(data, start, limit, ref_width);
});
