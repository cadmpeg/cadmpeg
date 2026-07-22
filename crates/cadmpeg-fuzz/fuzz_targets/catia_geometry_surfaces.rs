// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA geometry surface-prefix extraction.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_catia::fuzz::geometry_surface_prefixes`.
//! Contract: no input may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cadmpeg_codec_catia::fuzz::geometry_surface_prefixes(data);
});
