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

/// Exercise a fixed recovery route in fresh and originally refused sessions.
#[cfg(test)]
pub(crate) fn with_entry_context(
    mut run: impl FnMut(&cadmpeg_core::decode::DecodeContext<'_>, Option<cadmpeg_core::decode::ResourceLimit>),
) {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    for dimension in [None, Some(ResourceDimension::WorkUnits), Some(ResourceDimension::CollectionItems),
        Some(ResourceDimension::MaterializedBytes), Some(ResourceDimension::RetainedBytes),
        Some(ResourceDimension::Entities), Some(ResourceDimension::RecursionDepth)] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = 0;
        policy.limits.max_collection_items = 0;
        policy.limits.max_materialized_bytes = 0;
        policy.limits.max_retained_bytes = 0;
        policy.limits.max_entities = 0;
        policy.limits.max_recursion_depth = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let original = dimension.map(|dimension| {
            let refused = match dimension {
                ResourceDimension::WorkUnits => ctx.charge_work(1, "test original entry refusal"),
                ResourceDimension::CollectionItems => ctx.charge_collection_items(1, "test original entry refusal"),
                ResourceDimension::MaterializedBytes => ctx.reserve_scoped(1, "test original entry refusal").map(|_| ()),
                ResourceDimension::RetainedBytes => ctx.charge_retained(1, "test original entry refusal"),
                ResourceDimension::Entities => ctx.charge_entities(1, "test original entry refusal"),
                ResourceDimension::RecursionDepth => ctx.enter_nested("test original entry refusal").map(|_| ()),
                _ => panic!("entry refusal dimension"),
            };
            let Err(CodecError::ResourceLimit(first)) = refused else { panic!("original refusal"); };
            assert_eq!(first.dimension, dimension);
            first
        });
        for _ in 0..64 { run(&ctx, original); }
        match original {
            Some(first) => assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(last)) if last == first)),
            None => ctx.finish_session().unwrap(),
        }
    }
}
