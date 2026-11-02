// SPDX-License-Identifier: Apache-2.0
//! Hand-built Parasolid transmit-stream fixtures for unit and host-codec tests.

#![allow(clippy::unwrap_used)]

pub mod owned_deltas;
pub mod test_pmarks;
pub mod world;
pub mod xmt;

/// Run `f` with a decode context over an empty root.
#[cfg(test)]
pub(crate) fn with_decode_context<T>(
    f: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::service();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test root is admitted");
    f(&ctx)
}
