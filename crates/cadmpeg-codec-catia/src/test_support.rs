// SPDX-License-Identifier: Apache-2.0
//! Shared synthetic CATPart byte-fixture builders for `#[cfg(test)]` suites.
//!
//! Helpers hand-build `.CATPart` byte images and embedded-stream payloads.
//! They construct raw bytes only; decode, native, and family tests own the
//! assertions.
#![allow(clippy::doc_markdown, clippy::unwrap_used)]

pub(crate) mod test_a5_bound;
pub(crate) mod test_a5a8;
pub(crate) mod test_annotations;
pub(crate) mod test_b2;
pub(crate) mod test_b5;
pub(crate) mod test_bytes;
pub(crate) mod test_container;
pub(crate) mod test_e5;
pub(crate) mod test_formula;
pub(crate) mod test_object_graph;
pub(crate) mod test_topology;
pub(crate) mod test_zero_entity;

pub(crate) fn with_service_context<T>(
    run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::service(),
    )
    .expect("empty test root fits the service profile");
    run(&ctx)
}

pub(crate) fn with_entity_limit<T>(
    max_entities: u64,
    run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_entities = max_entities;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root fits the service profile");
    run(&ctx)
}

pub(crate) fn with_collection_limit<T>(
    max_collection_items: u64,
    run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = max_collection_items;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root fits the collection limit");
    run(&ctx)
}

pub(crate) fn with_retained_limit<T>(
    max_retained_bytes: u64,
    run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = max_retained_bytes;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root fits the retained limit");
    run(&ctx)
}

pub(crate) fn with_materialized_limit<T>(
    max_materialized_bytes: u64,
    run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_materialized_bytes = max_materialized_bytes;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root fits the materialized limit");
    run(&ctx)
}

pub(crate) fn with_work_limit<T>(
    max_work_units: u64,
    run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = max_work_units;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty test root fits the work limit");
    run(&ctx)
}

pub(crate) fn with_depth_limit<T>(
    max_recursion_depth: u64,
    run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_recursion_depth = max_recursion_depth;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("empty root fits the depth limit");
    run(&ctx)
}

/// Reach the same named text refusal after admitting new result slot storage.
pub(crate) fn with_retained_refusal<T, E: std::fmt::Debug>(
    input: &[u8],
    operation: &str,
    run: impl Fn(&cadmpeg_core::decode::DecodeContext<'_>) -> Result<T, E>,
) -> Result<T, E> {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let mut cap = 0;
    for _ in 0..4096 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(input, &arena, &policy).expect("root");
        let result = run(&ctx);
        let Some(refusal) = ctx.resource_refusal() else {
            panic!(
                "named retained refusal {operation} was not reached: {:?}",
                result.err()
            );
        };
        assert!(result.is_err());
        assert_eq!(refusal.dimension, ResourceDimension::RetainedBytes);
        let need = refusal
            .used
            .checked_add(refusal.additional)
            .expect("retained need");
        assert!(need > cap);
        if refusal.operation == operation {
            policy.limits.max_retained_bytes = need - 1;
            let (ctx, _) = DecodeContext::from_root_bytes(input, &arena, &policy).expect("root");
            let result = run(&ctx);
            assert!(result.is_err());
            let below = ctx.resource_refusal().expect("named refusal sets fuse");
            assert_eq!(below.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(below.operation, operation);
            assert_eq!(below.used.checked_add(below.additional), Some(need));
            return result;
        }
        cap = need;
    }
    panic!("named retained refusal {operation} exceeded the boundary count");
}
