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
