// SPDX-License-Identifier: Apache-2.0
//! Source-index expiry and intact generated placement publication.

use super::{GeneratedHolePlacements, GridCoordinate, COLLECT, OUTPUT};
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension, ScopedReservation};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::{FeatureDirection3, FinitePoint3};
use cadmpeg_ir::features::holes::HolePlacement;
use cadmpeg_ir::math::{Point3, Vector3};
use std::collections::BTreeMap;

const INDEX: &str = "index SLDPRT generated hole axes";
fn vector_bytes() -> u64 { u64::try_from(4 * std::mem::size_of::<HolePlacement>()).unwrap() }
fn shell_bytes() -> u64 {
    type K = [GridCoordinate; 6];
    type V = HolePlacement;
    let alignment = std::mem::align_of::<K>().max(std::mem::align_of::<V>())
        .max(std::mem::align_of::<usize>());
    u64::try_from(11 * (std::mem::size_of::<K>() + std::mem::size_of::<V>())
        + 16 * std::mem::size_of::<usize>() + 2 * alignment).unwrap()
}
fn source<'ctx>(ctx: &'ctx DecodeContext<'_>) -> (ScopedReservation<'ctx>, BTreeMap<[GridCoordinate; 6], HolePlacement>) {
    let mut storage = ctx.reserve_scoped(0, INDEX).unwrap();
    let mut axes = BTreeMap::new();
    for x in [2.0, 1.0] {
        let zero = GridCoordinate::new(0.0, 1.0);
        let key = [GridCoordinate::new(x, 1.0), zero, zero, zero, zero, zero];
        let value = HolePlacement::Axis {
            origin: FinitePoint3::new(Point3::new(x, 0.0, 0.0)).unwrap(),
            axis: FeatureDirection3::new(Vector3::new(0.0, 0.0, 1.0)).unwrap(),
        };
        storage.with_storage(|| ctx.insert_btree_map(&mut axes, key, value, INDEX)).unwrap();
    }
    (storage, axes)
}
fn x(placement: &HolePlacement) -> f64 {
    let HolePlacement::Axis { origin, .. } = placement else { panic!("generated cylinder axis"); };
    origin.x
}

#[test]
fn consumed_axis_shell_expires_before_lane_comparison_without_copying_output() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let peak = shell_bytes() + vector_bytes();
    policy.limits.max_materialized_bytes = peak;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 4; // Two source nodes and two placement slots.
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (storage, axes) = source(&ctx);
    let (result, allocations) = crate::test_support::allocation::count_allocations(||
        GeneratedHolePlacements::from_axes(&ctx, storage, axes));
    let placements = result.unwrap();
    assert_eq!(allocations, 1);
    assert_eq!(placements.placements().iter().map(x).collect::<Vec<_>>(), [1.0, 2.0]);
    drop(ctx.reserve_scoped(shell_bytes(), "test consumed generated axis shell").unwrap());
    drop(placements);
    drop(ctx.reserve_scoped(peak, "test generated lane teardown").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn identical_lanes_discard_only_the_second_actual_placement_owner() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let peak = shell_bytes() + 2 * vector_bytes();
    policy.limits.max_materialized_bytes = peak;
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_collection_items = 8;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (storage, axes) = source(&ctx);
    let first = GeneratedHolePlacements::from_axes(&ctx, storage, axes).unwrap();
    let pointer = first.placements().as_ptr();
    let (storage, axes) = source(&ctx);
    let second = GeneratedHolePlacements::from_axes(&ctx, storage, axes).unwrap();
    assert!(ctx.equal(first.placements(), second.placements(), "sort SLDPRT generated hole lanes").unwrap());
    drop(ctx.reserve_scoped(shell_bytes(), "test equal generated lane shell expiry").unwrap());
    drop(second);
    assert_eq!(first.placements().as_ptr(), pointer);
    drop(ctx.reserve_scoped(shell_bytes() + vector_bytes(), "test discarded equal generated lane").unwrap());
    drop(first);
    drop(ctx.reserve_scoped(peak, "test all generated lanes destroyed").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn collection_refuses_before_allocating_the_generated_lane_vector() {
    for dimension in [ResourceDimension::MaterializedBytes, ResourceDimension::CollectionItems] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        if dimension == ResourceDimension::MaterializedBytes {
            policy.limits.max_materialized_bytes = shell_bytes() + vector_bytes() - 1;
        } else {
            policy.limits.max_collection_items = 2;
        }
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let (storage, axes) = source(&ctx);
        let (result, allocations) = crate::test_support::allocation::count_allocations(||
            GeneratedHolePlacements::from_axes(&ctx, storage, axes));
        let error = result.err().expect("generated placement collection refusal");
        let CodecError::ResourceLimit(limit) = error else { panic!("generated placement collection resource limit"); };
        assert_eq!(allocations, 0);
        assert_eq!((limit.dimension, limit.operation), (dimension, COLLECT));
        if dimension == ResourceDimension::MaterializedBytes {
            assert_eq!((limit.used, limit.additional, limit.limit), (shell_bytes(), vector_bytes(), shell_bytes() + vector_bytes() - 1));
        } else {
            assert_eq!((limit.used, limit.additional, limit.limit), (2, 1, 2));
        }
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
    }
}

#[test]
fn standalone_handoff_preserves_exact_original_retained_operation_and_native_order() {
    for accepted in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        let peak = shell_bytes() + vector_bytes();
        policy.limits.max_materialized_bytes = peak;
        policy.limits.max_retained_bytes = vector_bytes() - u64::from(!accepted);
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let (storage, axes) = source(&ctx);
        let value = GeneratedHolePlacements::from_axes(&ctx, storage, axes).unwrap();
        let pointer = value.placements().as_ptr();
        let (result, allocations) = crate::test_support::allocation::count_allocations(|| value.into_output());
        assert_eq!(allocations, 0);
        if accepted {
            let output = result.unwrap();
            assert_eq!(output.as_ptr(), pointer);
            assert_eq!(output.iter().map(x).collect::<Vec<_>>(), [1.0, 2.0]);
            drop(ctx.reserve_scoped(peak, "test published generated output has no scratch").unwrap());
            drop(output);
            ctx.finish_session().unwrap();
        } else {
            let Err(CodecError::ResourceLimit(limit)) = result else { panic!("generated output refusal"); };
            assert_eq!((limit.dimension, limit.operation), (ResourceDimension::RetainedBytes, OUTPUT));
            assert_eq!((limit.used, limit.additional, limit.limit), (0, vector_bytes(), vector_bytes() - 1));
            assert_eq!(ctx.resource_refusal(), Some(limit));
            assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
        }
    }
}

#[test]
fn scoped_handoff_transfers_only_actual_placement_backing() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    let peak = shell_bytes() + vector_bytes();
    policy.limits.max_materialized_bytes = peak;
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut parent = ctx.reserve_scoped(0, "test generated placement parent").unwrap();
    let output = parent.with_storage(|| {
        let (storage, axes) = source(&ctx);
        GeneratedHolePlacements::from_axes(&ctx, storage, axes)?.into_output()
    }).unwrap();
    assert_eq!(output.iter().map(x).collect::<Vec<_>>(), [1.0, 2.0]);
    drop(ctx.reserve_scoped(shell_bytes(), "test generated parent excludes source shell").unwrap());
    drop(output);
    drop(parent);
    drop(ctx.reserve_scoped(peak, "test generated output parent teardown").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn original_fuse_stops_axis_consumption_and_actual_output_publication() {
    for publish in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let (storage, axes) = source(&ctx);
        let mut source_storage = Some(storage);
        let mut source_axes = Some(axes);
        let value = if publish {
            Some(GeneratedHolePlacements::from_axes(&ctx, source_storage.take().unwrap(), source_axes.take().unwrap()).unwrap())
        } else { None };
        let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "test original generated refusal", None);
        let Err(CodecError::ResourceLimit(first)) = ctx.charge_work(1, "test original generated refusal") else { panic!("original generated fuse"); };
        drop(probe);
        let (result, allocations) = crate::test_support::allocation::count_allocations(|| {
            if let Some(value) = value { value.into_output().map(|_| ()) }
            else {
                GeneratedHolePlacements::from_axes(&ctx, source_storage.take().unwrap(), source_axes.take().unwrap()).map(|_| ())
            }
        });
        assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == first));
        assert_eq!(allocations, 0);
        drop(source_axes);
        drop(source_storage);
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == first));
    }
}
