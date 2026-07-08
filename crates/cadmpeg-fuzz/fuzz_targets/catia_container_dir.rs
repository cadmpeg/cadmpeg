// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA container stream directory parsing.
//!
//! Feeds arbitrary bytes through `cadmpeg_codec_catia::container::parse_stream_directory`
//! to exercise V5_CFV2 directory parsing. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_catia::container::parse_stream_directory;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = parse_stream_directory(data);
});
