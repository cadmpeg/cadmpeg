// SPDX-License-Identifier: Apache-2.0
//! Fuzz Protein ZIP schema and paged instance-property decoding.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let split = data.len() / 2;
    let _schema_probe = cadmpeg_protein::has_schemas(&data[..split]);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    if let Ok((ctx, root)) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(data, &arena, &policy)
    {
        if let (Some(protein), Some(instance)) =
            (root.child(0, split), root.child(split, data.len()))
        {
            let _decode = cadmpeg_protein::decode_detailed(&ctx, protein, instance);
        }
    }
});
