// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for NX object-model section framing.
//! No input may panic.

#![no_main]

use cadmpeg_codec_nx::fuzz;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| fuzz::om(data));
