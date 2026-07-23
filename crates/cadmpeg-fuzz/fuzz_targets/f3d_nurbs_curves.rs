// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for F3D NURBS curve cache decoding.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_f3d::nurbs::core::decode_curve_cache`
//! to exercise NURBS binary parsing. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_f3d::nurbs::core::decode_curve_cache;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = decode_curve_cache(data);
});
