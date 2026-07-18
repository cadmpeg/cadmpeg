// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for NX Parasolid stream extraction.
//! No input may panic.

#![no_main]

use cadmpeg_codec_nx::{container, parasolid};
use cadmpeg_ir::decode::{DecodeArena, DecodeContext, DecodePolicy};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let Ok((ctx, root)) = DecodeContext::from_root_bytes(data, &arena, &policy) else {
        return;
    };
    let Ok(container) = container::scan_bytes(data.to_vec()) else {
        return;
    };
    let _ = parasolid::extract_streams(&ctx, root, &container);
});
