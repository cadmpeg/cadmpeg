// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic byte-fixture builders for the crate's `#[cfg(test)]` suites.
//!
//! Helpers hand-build `.prt` byte images and embedded-stream payloads. They
//! construct raw bytes only; no native record type crosses in here.
#![allow(clippy::unwrap_used)]

mod bytes;
mod deltas;
mod om;
mod prt;
mod streams;

pub(crate) use bytes::*;
pub(crate) use deltas::*;
pub(crate) use om::*;
pub(crate) use prt::*;
pub(crate) use streams::*;

pub(crate) fn extract_streams(bytes: &[u8]) -> Vec<crate::parasolid::Stream> {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, root) = cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes, &arena, &policy)
        .expect("bounded test input");
    let container = crate::container::scan_bytes(bytes.to_vec()).expect("test SPLMSSTR container");
    crate::parasolid::extract_streams(&ctx, root, &container).expect("test Parasolid streams")
}
