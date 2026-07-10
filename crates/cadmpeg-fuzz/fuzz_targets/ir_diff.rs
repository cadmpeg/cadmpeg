// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for `cadmpeg_ir::diff::diff`.
//!
//! Feeds arbitrary bytes through UTF-8 decoding and JSON deserialization of two
//! independent `CadIr` documents, then computes their structural diff.
//! Contract: no input may panic.

#![no_main]

use cadmpeg_ir::diff::diff;
use cadmpeg_ir::CadIr;
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.len() < 4 {
        return;
    }
    let payload = &data[1..];
    let split_point = payload
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or_else(|| (data[0] as usize) % payload.len());
    let (left_bytes, right_with_separator) = payload.split_at(split_point);
    let right_bytes = right_with_separator
        .strip_prefix(&[0])
        .unwrap_or(right_with_separator);

    let left_str = match std::str::from_utf8(left_bytes) {
        Ok(s) => s,
        Err(_) => return,
    };
    let right_str = match std::str::from_utf8(right_bytes) {
        Ok(s) => s,
        Err(_) => return,
    };

    let left_ir = match CadIr::from_json(left_str) {
        Ok(ir) => ir,
        Err(_) => return,
    };
    let right_ir = match CadIr::from_json(right_str) {
        Ok(ir) => ir,
        Err(_) => return,
    };

    let _ = diff(&left_ir, &right_ir);
});
