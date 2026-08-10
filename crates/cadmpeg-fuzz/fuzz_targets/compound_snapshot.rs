// SPDX-License-Identifier: Apache-2.0
//! Fuzz CFB prefix probing, snapshot construction, and selected stream opening.

#![no_main]

use cadmpeg_container::compound::{CompoundEntry, CompoundPrefixProbe, CompoundSnapshot};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = CompoundPrefixProbe::inspect(data);
    let arena = DecodeArena::new();
    let Ok((ctx, root)) = DecodeContext::from_root_bytes(data, &arena, &DecodePolicy::service())
    else {
        return;
    };
    let Ok(snapshot) = CompoundSnapshot::new(&ctx, root) else {
        return;
    };
    if let Some(entry) = snapshot.entries().iter().find_map(|entry| match entry {
        CompoundEntry::Stream(stream) => Some(stream),
        CompoundEntry::Storage(_) => None,
    }) {
        let _ = snapshot.open(&ctx, entry);
    }
});
