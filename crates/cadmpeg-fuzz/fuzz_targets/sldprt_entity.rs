// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for SolidWorks Parasolid entity scanning.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_sldprt::brep::entity::scan`
//! to exercise entity facts scanning. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_sldprt::brep::entity::scan;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = scan(data);
});
