// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for SolidWorks Parasolid topology scanning.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_sldprt::brep::topology::scan`
//! to exercise magic-guided binary parsing. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_sldprt::brep::topology::scan;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = scan(data);
});
