// SPDX-License-Identifier: Apache-2.0
//! Fuzz Inventor Protein package framing.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| cadmpeg_codec_inventor::fuzzing::protein_envelope(data));
