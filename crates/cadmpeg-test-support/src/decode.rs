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
    ctx.finish_session()?;
    result
}

/// Backing bytes of the arena registry's initial four-buffer capacity.
/// Each owned buffer is one slice pointer; collection slots are separate.
pub fn arena_registry_bytes() -> u64 {
    u64_from_index(4 * std::mem::size_of::<&[u8]>())
}

/// Find a codec charge and repeat its refusal with a normal policy ceiling.
///
/// `skip` selects a later reachable boundary of the same operation. The final
/// ceiling remains in `options` so a caller can inspect or repeat the refusal.
///
/// # Panics
///
/// Panics if the dimension is unsupported, the charge is absent, or the normal
/// policy replay differs from the probe.
pub fn resource_refusal_at(
    codec: &dyn Codec,
    bytes: &[u8],
    options: &mut DecodeOptions,
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: &str,
    skip: usize,
) -> cadmpeg_core::decode::ResourceLimit {
    use cadmpeg_core::decode::ResourceDimension;
    use cadmpeg_core::CodecError;

    let error = crate::refusal::resource_limit_at_nth(dimension, operation, skip, |cap| {
        match dimension {
            ResourceDimension::RetainedBytes => options.policy.limits.max_retained_bytes = cap,
            ResourceDimension::CollectionItems => options.policy.limits.max_collection_items = cap,
            ResourceDimension::WorkUnits => options.policy.limits.max_work_units = cap,
            ResourceDimension::RecursionDepth => options.policy.limits.max_recursion_depth = cap,
            ResourceDimension::MaterializedBytes => {
                options.policy.limits.max_materialized_bytes = cap;
            }
            _ => panic!("unsupported codec test dimension"),
        }
        codec
            .decode(&mut std::io::Cursor::new(bytes), options)
            .map_err(|failure| match failure {
                DecodeFailure::Codec(error) => error,
                other => panic!("expected codec resource refusal: {other:?}"),
            })
    });
    match error {
        CodecError::ResourceLimit(limit) => limit,
        other => panic!("expected resource refusal: {other:?}"),
    }
}
