// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for SolidWorks Parasolid topology scanning.
//! No input may panic.

#![no_main]

use cadmpeg_codec_sldprt::fuzzing::topology;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| topology(data));
