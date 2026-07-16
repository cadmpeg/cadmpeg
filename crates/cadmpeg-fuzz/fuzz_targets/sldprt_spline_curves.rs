// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for SolidWorks spline curve carrier scanning.
//!
//! Drives `cadmpeg_codec_sldprt::fuzzing::spline_curves` to exercise spline
//! binary parsing. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_sldprt::fuzzing::spline_curves;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| spline_curves(data));
