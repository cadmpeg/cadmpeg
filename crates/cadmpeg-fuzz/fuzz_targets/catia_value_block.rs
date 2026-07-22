// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA `7C0B` value block parsing.
//! No input may panic.

#![no_main]

use cadmpeg_codec_catia::value_block::parse;
use cadmpeg_ir::decode::{DecodeArena, DecodeContext, DecodePolicy};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    if let Ok((ctx, root)) = DecodeContext::from_root_bytes(data, &arena, &policy) {
        let _ = parse(root);
    }
});
