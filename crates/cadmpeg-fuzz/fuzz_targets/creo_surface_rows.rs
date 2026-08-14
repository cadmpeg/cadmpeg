// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for Creo surface row extraction.
//! No input may panic.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data| cadmpeg_codec_creo::fuzz::surface_rows(data));
