// SPDX-License-Identifier: Apache-2.0
//! Owner tests over already acquired input, with complete session finalization.

use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_ir::codec::{Codec, DecodeFailure, DecodeOptions, DecodeResult};

/// Runs full decoding on a borrowed root so parser limits exclude input acquisition.
pub fn full(
    codec: &dyn Codec,
    bytes: &[u8],
    policy: &DecodePolicy,
) -> Result<DecodeResult, DecodeFailure> {
    let arena = DecodeArena::new();
    let (ctx, root) = DecodeContext::from_root_bytes(bytes, &arena, policy)?;
    let options = DecodeOptions {
        policy: *policy,
        container_only: false,
    };
    let result = codec.decode_with_context(&ctx, root, &options);
    ctx.finish(result)
}

/// Backing bytes of the arena registry's initial four-buffer capacity.
/// Each owned buffer is one slice pointer; collection slots are separate.
pub fn arena_registry_bytes() -> u64 {
    u64_from_index(4 * std::mem::size_of::<&[u8]>())
}
