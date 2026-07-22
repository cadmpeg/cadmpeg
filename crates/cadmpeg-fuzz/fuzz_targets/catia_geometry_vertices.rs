// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA geometry vertex extraction.
//!
//! Feeds arbitrary bytes through
//! `cadmpeg_codec_catia::families::standard::records::scan_vertex_records`
//! to exercise vertex point extraction. Contract: no input may panic.

#![no_main]

use cadmpeg_codec_catia::families::standard::records::scan_vertex_records;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = scan_vertex_records(data);
});
