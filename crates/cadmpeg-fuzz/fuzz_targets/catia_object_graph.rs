// SPDX-License-Identifier: Apache-2.0
//! Fuzz target for CATIA `7C08` outer object-graph parsing.
//! No input may panic.

#![no_main]

use cadmpeg_codec_catia::object_graph::{markers_7cd9, parse, surface_aliases};
use cadmpeg_ir::decode::{DecodeArena, DecodeContext, DecodePolicy};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    if let Ok((ctx, root)) = DecodeContext::from_root_bytes(data, &arena, &policy) {
        let _ = parse(root);
        let _ = surface_aliases(root);
        let _ = markers_7cd9(root, data.len());
    }
});
