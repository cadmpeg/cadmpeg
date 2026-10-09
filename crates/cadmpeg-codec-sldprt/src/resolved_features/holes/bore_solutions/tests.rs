// SPDX-License-Identifier: Apache-2.0
//! Actual bore-key replacement, shell lifetime and output handoff boundaries.

use super::{BoreSolution, BoreSolutionKey, BoreSolutions, Candidate, OUTPUT, RETAIN};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDirection3, FinitePoint3};
use cadmpeg_ir::features::holes::HolePlacement;
use cadmpeg_ir::math::{Point3, Vector3};

fn candidate(x: f64) -> Candidate {
    let zero = super::GridCoordinate::new(0.0, super::EPS_HOLE_POSITION);
    ([super::GridCoordinate::new(x, super::EPS_HOLE_POSITION), zero, zero],
        FinitePoint3::new(Point3::new(x, 0.0, 0.0)).unwrap(),
        FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap())
}
fn key_bytes() -> u64 {
    u64::try_from(4 * std::mem::size_of::<[super::GridCoordinate; 6]>()).unwrap()
}
fn placement_bytes() -> u64 {
    u64::try_from(4 * std::mem::size_of::<HolePlacement>()).unwrap()
}
fn shell_bytes() -> u64 {
    type K = BoreSolutionKey<'static>;
    type V = BoreSolution<'static>;
    let alignment = std::mem::align_of::<K>().max(std::mem::align_of::<V>())
        .max(std::mem::align_of::<usize>());
    u64::try_from(11 * (std::mem::size_of::<K>() + std::mem::size_of::<V>())
        + 16 * std::mem::size_of::<usize>() + 2 * alignment).unwrap()
}
fn peak() -> u64 { key_bytes() + placement_bytes() + shell_bytes() }
fn origin(placement: &HolePlacement) -> Point3 {
    let HolePlacement::Axis { origin, .. } = placement else { panic!("native axis placement"); };
    origin.get()
}

#[test]
fn first_solution_admits_each_actual_vector_and_typed_shell_before_allocation() {
    let candidates = [candidate(0.0)];
    let vector_pair = placement_bytes() + key_bytes();
    for (cap, used, additional, allocations) in [
        (placement_bytes() - 1, 0, placement_bytes(), 0),
        (vector_pair - 1, placement_bytes(), key_bytes(), 1),
        (peak() - 1, vector_pair, shell_bytes(), 2),
    ] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut solutions = BoreSolutions::new(&ctx).unwrap();
        let (result, actual_allocations) = crate::test_support::allocation::count_allocations(||
            solutions.retain(&candidates, std::iter::once(0)));
        let Err(CodecError::ResourceLimit(limit)) = result else { panic!("first solution storage refusal"); };
        assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!(limit.operation, RETAIN);
        assert_eq!((limit.used, limit.additional, limit.limit), (used, additional, cap));
        assert_eq!(actual_allocations, allocations);
        drop(solutions);
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = peak();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut solutions = BoreSolutions::new(&ctx).unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(||
        solutions.retain(&candidates, std::iter::once(0)));
    assert!(!result.unwrap());
    assert_eq!(allocations, 3);
    assert_eq!(solutions.entries.len(), 1);
    drop(solutions);
    drop(ctx.reserve_scoped(peak(), "test bore solution teardown").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn equal_keys_keep_the_original_key_backing_and_last_native_placement() {
    let candidates = [candidate(0.0), candidate(super::EPS_HOLE_POSITION * 0.1),
        candidate(super::EPS_HOLE_POSITION * 0.2)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let live = peak();
    let duplicate = key_bytes() + placement_bytes();
    policy.limits.max_materialized_bytes = live + duplicate;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 7; // Two vector slots per attempt and one distinct map slot.
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut solutions = BoreSolutions::new(&ctx).unwrap();
    assert!(!solutions.retain(&candidates, std::iter::once(0)).unwrap());
    let first_key = solutions.entries.first_key_value().unwrap().0.coordinates.as_ptr();
    for index in [1, 2] {
        assert!(!solutions.retain(&candidates, std::iter::once(index)).unwrap());
        let (key, value) = solutions.entries.first_key_value().unwrap();
        assert_eq!(key.coordinates.as_ptr(), first_key);
        assert_eq!(origin(&value.placements[0]), candidates[index].1.get());
        drop(ctx.reserve_scoped(duplicate, "test discarded duplicate owners").unwrap());
    }
    drop(solutions);
    drop(ctx.reserve_scoped(live + duplicate, "test duplicate table teardown").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn distinct_solution_ambiguity_discards_both_actual_keys_values_and_shell() {
    let candidates = [candidate(0.0), candidate(1.0)];
    let bytes = shell_bytes() + 2 * (key_bytes() + placement_bytes());
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = bytes;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut solutions = BoreSolutions::new(&ctx).unwrap();
    assert!(!solutions.retain(&candidates, std::iter::once(0)).unwrap());
    assert!(solutions.retain(&candidates, std::iter::once(1)).unwrap());
    assert!(solutions.into_single().unwrap().is_none());
    drop(ctx.reserve_scoped(bytes, "test ambiguous bore owners teardown").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn selected_value_outlives_its_key_and_table_shell_without_copying() {
    let candidates = [candidate(1.0)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = peak();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut solutions = BoreSolutions::new(&ctx).unwrap();
    solutions.retain(&candidates, std::iter::once(0)).unwrap();
    let pointer = solutions.entries.first_key_value().unwrap().1.placements.as_ptr();
    let value = solutions.into_single().unwrap().unwrap();
    assert_eq!(value.placements.as_ptr(), pointer);
    assert_eq!(origin(&value.placements[0]), candidates[0].1.get());
    drop(ctx.reserve_scoped(key_bytes() + shell_bytes(), "test selected key and shell expiry").unwrap());
    drop(value);
    drop(ctx.reserve_scoped(peak(), "test selected value expiry").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn standalone_publication_keeps_the_original_exact_output_refusal() {
    let candidates = [candidate(1.0)];
    let output = placement_bytes();
    for accepted in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = peak();
        policy.limits.max_retained_bytes = output - u64::from(!accepted);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut solutions = BoreSolutions::new(&ctx).unwrap();
        solutions.retain(&candidates, std::iter::once(0)).unwrap();
        let value = solutions.into_single().unwrap().unwrap();
        let pointer = value.placements.as_ptr();
        let (result, allocations) = crate::test_support::allocation::count_allocations(|| value.into_output());
        assert_eq!(allocations, 0);
        if accepted {
            let placements = result.unwrap();
            assert_eq!(placements.as_ptr(), pointer);
            assert_eq!(origin(&placements[0]), candidates[0].1.get());
            drop(ctx.reserve_scoped(peak(), "test retained output uses no scratch").unwrap());
            drop(placements);
            ctx.finish_session().unwrap();
        } else {
            let Err(CodecError::ResourceLimit(limit)) = result else { panic!("publication retained bytes"); };
            assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
            assert_eq!(limit.operation, OUTPUT);
            assert_eq!((limit.used, limit.additional, limit.limit), (0, output, output - 1));
            assert_eq!(ctx.resource_refusal(), Some(limit));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
        }
    }
}

#[test]
fn scoped_publication_transfers_only_the_surviving_placement_backing() {
    let candidates = [candidate(1.0)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = peak();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut storage = ctx.reserve_scoped(0, "test actual bore output owner").unwrap();
    let placements = storage.with_storage(|| {
        let mut solutions = BoreSolutions::new(&ctx)?;
        solutions.retain(&candidates, std::iter::once(0))?;
        solutions.into_single()?.unwrap().into_output()
    }).unwrap();
    assert_eq!(origin(&placements[0]), candidates[0].1.get());
    drop(ctx.reserve_scoped(key_bytes() + shell_bytes(), "test transferred output excludes key shell").unwrap());
    drop(placements);
    drop(storage);
    drop(ctx.reserve_scoped(peak(), "test actual scoped output teardown").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn original_fuse_refuses_solution_construction_without_allocation() {
    let candidates = [candidate(0.0)];
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(first)) = ctx.charge_work(1, "test original bore refusal") else { panic!("original refusal"); };
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| {
        let mut solutions = BoreSolutions::new(&ctx)?;
        solutions.retain(&candidates, std::iter::once(0))
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == first));
    assert_eq!(allocations, 0);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == first));
}

#[test]
fn empty_solution_table_uses_no_collection_storage_or_output() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| {
        BoreSolutions::new(&ctx)?.into_single()
    });
    assert!(result.unwrap().is_none());
    assert_eq!(allocations, 0);
    ctx.finish_session().unwrap();
}
