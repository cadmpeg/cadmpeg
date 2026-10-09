// SPDX-License-Identifier: Apache-2.0
//! Exact backing and publication bounds for planar placement vectors.

use super::{PlanarHolePlacements, OPERATION};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDirection3, FeatureId, FinitePoint3};
use cadmpeg_ir::features::holes::HolePlacement;
use cadmpeg_ir::math::{Point3, Vector3};

fn backing() -> u64 { u64::try_from(4 * std::mem::size_of::<HolePlacement>()).unwrap() }
fn dependency() -> FeatureId { FeatureId::mint("test:model:feature#position").unwrap() }
fn origin(x: f64) -> FinitePoint3 { FinitePoint3::new(Point3::new(x, 0.0, 0.0)).unwrap() }
fn axis() -> FeatureDirection3 { FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap() }

#[test]
fn only_actual_axis_slots_and_backing_survive_planar_publication() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = backing();
    policy.limits.max_retained_bytes = backing();
    policy.limits.max_collection_items = 2;
    policy.limits.max_work_units = 0; // Fixed admitted axis values require no clone walk.
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let dependency = dependency();
    let mut value = PlanarHolePlacements::new(&ctx, &dependency).unwrap();
    let (_, allocations) = crate::test_support::allocation::count_allocations(|| {
        value.push_axis(origin(2.0), axis()).unwrap();
        value.push_axis(origin(1.0), axis()).unwrap();
    });
    assert_eq!(allocations, 1);
    let pointer = value.placements().as_ptr();
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| value.into_output());
    assert_eq!(allocations, 0);
    let (placements, actual_dependency) = result.unwrap();
    assert_eq!(placements.as_ptr(), pointer);
    assert!(std::ptr::eq(actual_dependency, &dependency));
    assert!(matches!(placements.as_slice(), [HolePlacement::Axis { origin: first, .. }, HolePlacement::Axis { origin: second, .. }] if first.x == 2.0 && second.x == 1.0));
    drop(ctx.reserve_scoped(backing(), "test planar publication has no scratch").unwrap());
    drop(placements);
    ctx.finish_session().unwrap();
}

#[test]
fn planar_first_axis_refuses_before_allocating_at_exact_materialized_or_slot_bound() {
    for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        if dimension == ResourceDimension::MaterializedBytes { policy.limits.max_materialized_bytes = backing() - 1; }
        else { policy.limits.max_collection_items = 0; }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let dependency = dependency();
        let mut value = PlanarHolePlacements::new(&ctx, &dependency).unwrap();
        let (result, allocations) = crate::test_support::allocation::count_allocations(|| value.push_axis(origin(1.0), axis()));
        let Err(CodecError::ResourceLimit(limit)) = result else { panic!("first planar axis resource limit"); };
        assert_eq!(allocations, 0);
        assert!(value.placements().is_empty());
        assert_eq!((limit.dimension, limit.operation), (dimension, OPERATION));
        assert_eq!((limit.used, limit.additional), (0, if dimension == ResourceDimension::MaterializedBytes { backing() } else { 1 }));
        assert_eq!(ctx.resource_refusal(), Some(limit));
        drop(value);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
    }
}

#[test]
fn planar_retained_handoff_keeps_original_exact_refusal_and_native_dependency() {
    for accepted in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = backing();
        policy.limits.max_retained_bytes = backing() - u64::from(!accepted);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let dependency = dependency();
        let mut value = PlanarHolePlacements::new(&ctx, &dependency).unwrap();
        value.push_axis(origin(1.0), axis()).unwrap();
        let pointer = value.placements().as_ptr();
        let (result, allocations) = crate::test_support::allocation::count_allocations(|| value.into_output());
        assert_eq!(allocations, 0);
        if accepted {
            let (placements, actual_dependency) = result.unwrap();
            assert_eq!(placements.as_ptr(), pointer);
            assert!(std::ptr::eq(actual_dependency, &dependency));
            drop(placements);
            ctx.finish_session().unwrap();
        } else {
            let error = result.err().expect("planar retained refusal");
            let CodecError::ResourceLimit(limit) = error else { panic!("planar retained resource limit"); };
            assert_eq!((limit.dimension, limit.operation), (ResourceDimension::RetainedBytes, OPERATION));
            assert_eq!((limit.used, limit.additional, limit.limit), (0, backing(), backing() - 1));
            assert_eq!(ctx.resource_refusal(), Some(limit));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
        }
    }
}

#[test]
fn rejected_planar_prefix_releases_its_actual_capacity_on_owner_teardown() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = backing();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let dependency = dependency();
    let mut value = PlanarHolePlacements::new(&ctx, &dependency).unwrap();
    value.push_axis(origin(1.0), axis()).unwrap();
    let pointer = value.placements().as_ptr();
    value.clear().unwrap();
    assert!(value.placements().is_empty());
    value.push_axis(origin(2.0), axis()).unwrap();
    assert_eq!(value.placements().as_ptr(), pointer);
    drop(value);
    drop(ctx.reserve_scoped(backing(), "test rejected planar prefix expiry").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn planar_scoped_parent_receives_only_the_surviving_vector() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = backing() + 64;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let dependency = dependency();
    let mut parent = ctx.reserve_scoped(0, "test planar output parent").unwrap();
    let (placements, actual_dependency) = parent.with_storage(|| {
        let scratch = ctx.reserve_scoped(64, "test planar input scratch").unwrap();
        let mut value = PlanarHolePlacements::new(&ctx, &dependency)?;
        value.push_axis(origin(1.0), axis())?;
        drop(scratch);
        value.into_output()
    }).unwrap();
    assert!(std::ptr::eq(actual_dependency, &dependency));
    drop(ctx.reserve_scoped(64, "test planar parent excludes input scratch").unwrap());
    drop(placements);
    drop(parent);
    drop(ctx.reserve_scoped(backing() + 64, "test planar output parent teardown").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn original_planar_fuse_stops_value_creation_and_publication_without_allocation() {
    for publish in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let dependency = dependency();
        let mut value = if publish { Some(PlanarHolePlacements::new(&ctx, &dependency).unwrap()) } else { None };
        if let Some(value) = &mut value { value.push_axis(origin(1.0), axis()).unwrap(); }
        let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "test original planar refusal", None);
        let Err(CodecError::ResourceLimit(first)) = ctx.charge_work(1, "test original planar refusal") else { panic!("original planar fuse"); };
        drop(probe);
        let (result, allocations) = crate::test_support::allocation::count_allocations(|| {
            if let Some(value) = value { value.into_output().map(|_| ()) }
            else { PlanarHolePlacements::new(&ctx, &dependency).map(|_| ()) }
        });
        assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == first));
        assert_eq!(allocations, 0);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == first));
    }
}
