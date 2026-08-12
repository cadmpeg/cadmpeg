// SPDX-License-Identifier: Apache-2.0
//! Fuzz Inventor OLE property sets.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| cadmpeg_codec_inventor::fuzz::property_set(data));
