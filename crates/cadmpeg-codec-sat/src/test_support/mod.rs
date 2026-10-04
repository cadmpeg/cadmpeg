// SPDX-License-Identifier: Apache-2.0
//! Synthetic SAT stream builders for crate tests.
#![allow(clippy::unwrap_used)]

pub(crate) mod test_streams;

/// Runs a test operation under the stated caller policy.
pub(crate) fn with_context<T>(
    bytes: &[u8],
    policy: &cadmpeg_core::decode::DecodePolicy,
    operation: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(bytes, &arena, policy)
        .expect("test input fits caller policy");
    operation(&ctx)
}
