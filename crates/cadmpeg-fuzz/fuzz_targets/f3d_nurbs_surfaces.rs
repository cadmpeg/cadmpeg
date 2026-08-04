// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for F3D NURBS surface cache decoding.
//!
//! Feeds arbitrary bytes through `cadmpeg_asm::nurbs::core::decode_surface_cache`
//! to exercise NURBS binary parsing. Contract: no input may panic.

#![no_main]

use cadmpeg_asm::nurbs::core::decode_surface_cache;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = decode_surface_cache(data);
});
