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

/// Refuse the first positive charge at an operation without pinning earlier work.
#[cfg(test)]
pub(crate) fn resource_limit_at<T>(
    bytes: &[u8],
    dimension: cadmpeg_core::decode::ResourceDimension,
    operation: &str,
    mut run: impl FnMut(&DecodeContext<'_>) -> Result<T, cadmpeg_core::CodecError>,
) -> cadmpeg_core::decode::ResourceLimit {
    use cadmpeg_core::decode::ResourceDimension;
    let error = cadmpeg_test_support::refusal::resource_limit_at(dimension, operation, |cap| {
        let mut policy = DecodePolicy::service();
        match dimension {
            ResourceDimension::WorkUnits => policy.limits.max_work_units = cap,
            ResourceDimension::RetainedBytes => policy.limits.max_retained_bytes = cap,
            ResourceDimension::MaterializedBytes => policy.limits.max_materialized_bytes = cap,
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = cap,
            ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = cap,
            _ => panic!("unsupported test dimension"),
        }
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(bytes, &arena, &policy)?;
        run(&ctx)
    });
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("expected resource refusal: {error:?}");
    };
    limit
}
