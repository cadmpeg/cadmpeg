// SPDX-License-Identifier: Apache-2.0
//! Fuzz Inventor metadata-table and bulk-record framing together.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    cadmpeg_codec_inventor::fuzz::bulk_stream(data);
    cadmpeg_codec_inventor::fuzz::record_tables(data);
});
