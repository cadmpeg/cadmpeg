// SPDX-License-Identifier: Apache-2.0

use super::super::{DecodeArena, OwnedBuffer};
use crate::decode::{u64_from_index, DecodeContext, DecodePolicy, ResourceDimension};
use crate::CodecError;

#[test]
fn provisional_concatenation_refuses_before_arena_payload_installation() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 2;
    let (ctx, root) = DecodeContext::from_root_bytes(b"ab", &arena, &policy).expect("context");
    let mut candidate = ctx
        .provisional_retained("caller candidate")
        .expect("candidate");
    let error = candidate
        .with_storage(|| ctx.concat_views(&[root]))
        .expect_err("arena payload needs session storage before installation");
    let CodecError::ResourceLimit(original) = error else {
        panic!("retained refusal")
    };
    assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(
        (original.limit, original.used, original.additional),
        (0, 0, 2)
    );
    assert_eq!(original.operation, "concat_views");
    assert!(arena.buffers.borrow().is_empty());
    assert_eq!(arena.buffers.borrow().capacity(), 0);
    assert_eq!(ctx.budget.materialized_used(), 0);
    assert_eq!(ctx.budget.retained_used(), 0);
    drop(candidate);
    assert_eq!(ctx.budget.retained_used(), 0);
    assert_eq!(ctx.resource_refusal(), Some(original));
    assert!(
        matches!(ctx.charge_work(0, "later"), Err(CodecError::ResourceLimit(found)) if found == original)
    );
}

#[test]
fn provisional_concatenation_keeps_actual_arena_backing_after_candidate_drop() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    // A two-byte payload and the first four-owner registry allocation.
    let retained = 2 + 4 * u64_from_index(std::mem::size_of::<OwnedBuffer>());
    policy.limits.max_retained_bytes = retained;
    policy.limits.max_materialized_bytes = 2;
    let (ctx, root) = DecodeContext::from_root_bytes(b"ab", &arena, &policy).expect("context");
    let mut candidate = ctx
        .provisional_retained("caller candidate")
        .expect("candidate");
    let view = candidate
        .with_storage(|| ctx.concat_views(&[root]))
        .expect("session arena backing");
    assert_eq!(view.window(), b"ab");
    assert_eq!(arena.buffers.borrow().len(), 1);
    assert_eq!(arena.buffers.borrow().capacity(), 4);
    assert_eq!(ctx.budget.materialized_used(), 0);
    assert_eq!(ctx.budget.retained_used(), retained);
    drop(candidate);
    assert_eq!(view.window(), b"ab");
    assert_eq!(arena.buffers.borrow().len(), 1);
    assert_eq!(ctx.budget.retained_used(), retained);
    assert_eq!(ctx.budget.materialized_used(), 0);
    let Err(CodecError::ResourceLimit(original)) = ctx.charge_retained(1, "live backing probe")
    else {
        panic!("exact session backing remains charged")
    };
    assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(
        (original.limit, original.used, original.additional),
        (retained, retained, 1)
    );
    assert_eq!(view.window(), b"ab");
}

#[test]
fn refused_registry_installation_releases_payload_in_every_storage_route() {
    let registry = 4 * u64_from_index(std::mem::size_of::<OwnedBuffer>());
    for retained in [2, 2 + registry - 1] {
        for route in 0..3 {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_retained_bytes = retained;
            policy.limits.max_materialized_bytes = 2;
            let (ctx, root) =
                DecodeContext::from_root_bytes(b"AB", &arena, &policy).expect("context");
            let inputs = [root.child(0, 1).expect("A"), root.child(1, 2).expect("B")];
            let result = match route {
                0 => ctx.concat_views(&inputs),
                1 => {
                    let mut owner = ctx.reserve_scoped(0, "caller scratch").expect("owner");
                    let result = owner.with_storage(|| ctx.concat_views(&inputs));
                    drop(owner);
                    result
                }
                _ => {
                    let mut owner = ctx.provisional_retained("caller candidate").expect("owner");
                    let result = owner.with_storage(|| ctx.concat_views(&inputs));
                    drop(owner);
                    result
                }
            };
            let Err(CodecError::ResourceLimit(original)) = result else {
                panic!("registry must refuse")
            };
            assert_eq!(original.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(
                (original.limit, original.used, original.additional),
                (retained, 2, registry)
            );
            assert_eq!(original.operation, "arena registry");
            assert_eq!(
                arena.allocation_state(),
                (0, 0),
                "route {route}, cap {retained}"
            );
            assert_eq!(
                ctx.budget.retained_used(),
                0,
                "destroyed payload, route {route}, cap {retained}"
            );
            assert_eq!(ctx.budget.materialized_used(), 0);
            assert_eq!(root.window(), b"AB");
            assert!(
                matches!(ctx.charge_work(0, "later"), Err(CodecError::ResourceLimit(found)) if found == original)
            );
        }
    }
}

#[test]
fn failed_registry_growth_preserves_prior_backing_and_borrows() {
    let old_backing = 4 * u64_from_index(std::mem::size_of::<OwnedBuffer>());
    for dimension in [
        ResourceDimension::WorkUnits,
        ResourceDimension::MaterializedBytes,
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 2 * old_backing;
        match dimension {
            ResourceDimension::WorkUnits => policy.limits.max_work_units = old_backing - 1,
            ResourceDimension::MaterializedBytes => {
                policy.limits.max_materialized_bytes = old_backing - 1;
            }
            _ => panic!("relocation dimension"),
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("context");
        let mut prior = Vec::new();
        for _ in 0..4 {
            prior.push(
                arena
                    .alloc(&ctx, Vec::new().into_boxed_slice())
                    .expect("first registry"),
            );
        }
        assert_eq!(arena.allocation_state(), (4, 4));
        let Err(CodecError::ResourceLimit(original)) =
            arena.alloc(&ctx, Vec::new().into_boxed_slice())
        else {
            panic!("registry relocation refuses")
        };
        assert_eq!(original.dimension, dimension);
        assert_eq!((original.used, original.additional), (0, old_backing));
        assert_eq!(arena.allocation_state(), (4, 4));
        assert_eq!(ctx.budget.retained_used(), old_backing);
        assert_eq!(ctx.budget.materialized_used(), 0);
        assert!(prior.iter().all(|bytes| bytes.is_empty()));
        assert!(
            matches!(arena.alloc(&ctx, Vec::new().into_boxed_slice()), Err(CodecError::ResourceLimit(found)) if found == original)
        );
        assert_eq!(arena.allocation_state(), (4, 4));
        assert_eq!(ctx.budget.retained_used(), old_backing);
    }
}

#[test]
fn installed_payload_and_grown_registry_survive_enclosing_unwind() {
    use std::panic::{catch_unwind, AssertUnwindSafe};
    let registry = 8 * u64_from_index(std::mem::size_of::<OwnedBuffer>());
    for route in 0..2 {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = registry + 5;
        policy.limits.max_materialized_bytes = registry / 2;
        let (ctx, root) = DecodeContext::from_root_bytes(b"x", &arena, &policy).expect("context");
        let mut prior = Vec::new();
        for _ in 0..4 {
            prior.push(ctx.concat_views(&[root]).expect("prior arena payload"));
        }
        let mut escaped = None;
        let mut publish = || -> Result<(), CodecError> {
            escaped = Some(ctx.concat_views(&[root])?);
            panic!("after actual installation")
        };
        if route == 0 {
            let mut owner = ctx.reserve_scoped(0, "caller scratch").expect("owner");
            assert!(catch_unwind(AssertUnwindSafe(|| owner.with_storage(&mut publish))).is_err());
            drop(owner);
        } else {
            let mut owner = ctx.provisional_retained("caller candidate").expect("owner");
            assert!(catch_unwind(AssertUnwindSafe(|| owner.with_storage(&mut publish))).is_err());
            drop(owner);
        }
        assert_eq!(escaped.expect("installed view").window(), b"x");
        assert!(prior.iter().all(|view| view.window() == b"x"));
        assert_eq!(arena.allocation_state(), (5, 8));
        assert_eq!(ctx.budget.retained_used(), registry + 5);
        assert_eq!(ctx.budget.materialized_used(), 0);
        assert_eq!(ctx.resource_refusal(), None);
    }
}

#[test]
fn refused_second_registry_slot_releases_only_new_payload() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let registry = 4 * u64_from_index(std::mem::size_of::<OwnedBuffer>());
    policy.limits.max_retained_bytes = registry + 4;
    policy.limits.max_materialized_bytes = 2;
    policy.limits.max_collection_items = 1;
    let (ctx, root) = DecodeContext::from_root_bytes(b"AB", &arena, &policy).expect("context");
    let prior = ctx.concat_views(&[root]).expect("first slot and payload");
    let Err(CodecError::ResourceLimit(original)) = ctx.concat_views(&[root]) else {
        panic!("second slot must refuse")
    };
    assert_eq!(original.dimension, ResourceDimension::CollectionItems);
    assert_eq!(
        (original.limit, original.used, original.additional),
        (1, 1, 1)
    );
    assert_eq!(arena.allocation_state(), (1, 4));
    assert_eq!(prior.window(), b"AB");
    assert_eq!(ctx.budget.retained_used(), registry + 2);
    assert_eq!(ctx.budget.materialized_used(), 0);
    assert!(
        matches!(ctx.concat_views(&[root]), Err(CodecError::ResourceLimit(found)) if found == original)
    );
    assert_eq!(prior.window(), b"AB");
    assert_eq!(arena.allocation_state(), (1, 4));
    assert_eq!(ctx.budget.retained_used(), registry + 2);
}
