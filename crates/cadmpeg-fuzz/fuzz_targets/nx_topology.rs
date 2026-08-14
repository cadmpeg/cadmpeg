// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for NX Parasolid topology parsing.
//! No input may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cadmpeg_codec_nx::fuzz::topology(data);
});
