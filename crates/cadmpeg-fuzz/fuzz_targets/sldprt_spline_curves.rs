// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for SolidWorks spline curve carrier scanning.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_sldprt::brep::spline::scan_curve_carriers`
//! to exercise spline binary parsing. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_sldprt::brep::spline::scan_curve_carriers;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = scan_curve_carriers(data);
});
