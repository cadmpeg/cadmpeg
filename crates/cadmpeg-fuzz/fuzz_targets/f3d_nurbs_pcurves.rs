// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for F3D NURBS pcurve cache decoding.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_f3d::nurbs::pcurve::decode_pcurve_cache`
//! to exercise NURBS binary parsing. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_f3d::nurbs::pcurve::decode_pcurve_cache;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = decode_pcurve_cache(data);
});
