// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for Creo `ActDatums` model-space plane decoding.
//! No input may panic or read outside the input slice.

#![no_main]

use cadmpeg_codec_creo::datum::{named_plane, planes};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = planes(data);
    let _ = named_plane(data);
});
