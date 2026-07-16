// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for Creo `ActDatums` model-space plane decoding.
//!
//! Feeds arbitrary bytes through the migrated `datum::planes` and
//! `datum::named_zero_plane` primitive decoders. Contract: no input may panic
//! and every read stays within the caller-owned slice.

#![no_main]

use cadmpeg_codec_creo::datum::{named_zero_plane, planes};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = planes(data);
    let _ = named_zero_plane(data);
});
