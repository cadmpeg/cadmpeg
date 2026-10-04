// SPDX-License-Identifier: Apache-2.0
//! Fixture helpers for ASM and its codec consumers.

pub mod sab;

#[cfg(test)]
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

/// Run a fixture with the service decode policy.
#[cfg(test)]
pub(crate) fn with_service_context<T>(
    bytes: &[u8],
    use_context: impl FnOnce(&DecodeContext<'_>) -> T,
) -> Result<T, cadmpeg_core::CodecError> {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &DecodePolicy::service())?;
    Ok(use_context(&ctx))
}
