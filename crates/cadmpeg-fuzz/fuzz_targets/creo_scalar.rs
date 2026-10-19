// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for Creo PSB scalar decoding.
//! No input may panic or read outside the input slice.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data| cadmpeg_codec_creo::fuzz::scalar(data));
