// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for SolidWorks container scanning.
//!
//! Feeds arbitrary bytes through the charged SolidWorks container scan
//! to exercise block-framed container parsing with CRC validation. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_sldprt::fuzz::container;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| drop(container(data)));
