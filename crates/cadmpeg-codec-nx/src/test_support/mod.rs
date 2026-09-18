// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic byte-fixture builders for the crate's `#[cfg(test)]` suites.
//!
//! Helpers hand-build `.prt` byte images and embedded-stream payloads.
//! Native projection fixtures use checked source tokens.
#![allow(clippy::unwrap_used)]

pub(crate) mod native_references;
pub(crate) mod test_bytes;
pub(crate) mod test_cfb;
pub(crate) mod test_deltas;
pub(crate) mod test_om;
pub(crate) mod test_prt;
pub(crate) mod test_streams;
pub(crate) mod test_wire;

pub(crate) fn extract_streams(bytes: &[u8]) -> Vec<crate::parasolid::Stream> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .expect("bounded test input");
    let container = crate::container::scan_bytes(bytes.to_vec()).expect("test SPLMSSTR container");
    crate::parasolid::extract_streams(&ctx, root, &container).expect("test Parasolid streams")
}
