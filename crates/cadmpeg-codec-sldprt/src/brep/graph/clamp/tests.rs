// SPDX-License-Identifier: Apache-2.0
//! Each replacement generation owns its current knots and controls.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::brep::graph::clamp::clamp_nurbs_curve_to_domain_lanes;
use crate::brep::graph::clamp::insert_nurbs_homogeneous_knot;

const SCRATCH_BYTES: u64 = 16 * 1024 * 1024;
const KNOTS: [f64; 4] = [0.0, 0.0, 1.0, 1.0];
const CONTROLS: [[f64; 4]; 2] = [[0.0, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0]];

fn policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy
}

#[test]
fn homogeneous_replacement_releases_old_generations_and_preserves_numeric_lanes() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let mut current = insert_nurbs_homogeneous_knot(&ctx, 1, &KNOTS, &CONTROLS, 0.5).unwrap().unwrap();
    for value in [0.75, 0.25] {
        let next = insert_nurbs_homogeneous_knot(&ctx, 1, &current.knots, &current.controls, value).unwrap().unwrap();
        current = next;
    }
    assert_eq!(current.knots, [0.0, 0.0, 0.25, 0.5, 0.75, 1.0, 1.0]);
    assert_eq!(current.controls, [
        [0.0, 0.0, 0.0, 1.0], [0.25, 0.0, 0.0, 1.0],
        [0.5, 0.0, 0.0, 1.0], [0.75, 0.0, 0.0, 1.0], [1.0, 0.0, 0.0, 1.0],
    ]);
    // Core's minimum capacity4 and doubling leave capacity8 in each Vec.
    // The final controls have five elements; the final knots have seven.
    let expected = u64::try_from(8 * std::mem::size_of::<f64>() + 8 * std::mem::size_of::<[f64; 4]>()).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = ctx.reserve_scoped(SCRATCH_BYTES - expected + 1, "test final homogeneous generation") else { panic!("live generation probe"); };
    assert_eq!((limit.used, limit.additional), (expected, SCRATCH_BYTES - expected + 1));
    assert_eq!(limit.limit, SCRATCH_BYTES);
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    drop(current);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn unpublished_homogeneous_generation_drop_releases_all_backing() {
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let generation = insert_nurbs_homogeneous_knot(&ctx, 1, &KNOTS, &CONTROLS, 0.5).unwrap().unwrap();
    drop(generation);
    drop(ctx.reserve_scoped(SCRATCH_BYTES, "test released homogeneous generation").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn clamp_raw_segment_releases_initial_and_replacement_generations() {
    let curve = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 1,
        vec![0.0, 0.0, 2.0, 2.0],
        vec![cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0), cadmpeg_ir::math::Point3::new(2.0, 0.0, 0.0)],
        None, false,
    ).unwrap().unwrap();
    let arena = DecodeArena::new();
    let mut policy = policy();
    // Raw output: four exact knot slots and minimum capacity4 Point3 slots.
    // This pins only the raw-return boundary; converted IR storage is separate.
    let expected = u64::try_from(4 * std::mem::size_of::<f64>() + 4 * std::mem::size_of::<cadmpeg_ir::math::Point3>()).unwrap();
    policy.limits.max_retained_bytes = expected;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (knots, points, weights) = clamp_nurbs_curve_to_domain_lanes(&ctx, &curve, [0.5, 1.5]).unwrap().unwrap();
    assert_eq!(knots, [0.5, 0.5, 1.5, 1.5]);
    assert_eq!(points, [cadmpeg_ir::math::Point3::new(0.5, 0.0, 0.0), cadmpeg_ir::math::Point3::new(1.5, 0.0, 0.0)]);
    assert!(weights.is_none());
    drop(ctx.reserve_scoped(SCRATCH_BYTES, "test released clamp generations").unwrap());
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_retained(1, "test raw clamp output") else { panic!("raw segment storage boundary"); };
    assert_eq!((limit.used, limit.additional), (expected, 1));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn clamp_invalid_domain_preserves_the_original_refusal() {
    let curve = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0), cadmpeg_ir::math::Point3::new(1.0, 0.0, 0.0)],
        None, false,
    ).unwrap().unwrap();
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "test original clamp refusal") else { panic!("original refusal"); };
    let result = clamp_nurbs_curve_to_domain_lanes(&ctx, &curve, [1.0, 0.0]);
    assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == limit));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn homogeneous_insert_refuses_prefix_backing_before_allocation() {
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| {
        insert_nurbs_homogeneous_knot(&ctx, 1, &KNOTS, &CONTROLS, 0.5)
    });
    let CodecError::ResourceLimit(limit) = result.err().expect("prefix storage refusal") else { panic!("resource refusal"); };
    assert_eq!(allocations, 0);
    assert_eq!(limit.operation, "insert homogeneous NURBS knot lane");
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!((limit.used, limit.additional), (0, u64::try_from(4 * std::mem::size_of::<f64>()).unwrap()));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn homogeneous_insert_invalid_shape_is_free_before_a_refusal() {
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    assert!(insert_nurbs_homogeneous_knot(&ctx, 0, &KNOTS, &CONTROLS, 0.5).unwrap().is_none());
    ctx.finish_session().unwrap();
}

#[test]
fn homogeneous_insert_invalid_shape_preserves_the_original_refusal() {
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "test original homogeneous refusal") else { panic!("original refusal"); };
    let result = insert_nurbs_homogeneous_knot(&ctx, 0, &KNOTS, &CONTROLS, 0.5);
    assert!(matches!(&result, Err(CodecError::ResourceLimit(actual)) if *actual == limit));
    drop(result);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

fn conversion_source(rational: bool) -> cadmpeg_ir::geometry::nurbs::NurbsCurve {
    cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 1,
        vec![0.0, 0.0, 2.0, 2.0],
        vec![cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0), cadmpeg_ir::math::Point3::new(2.0, 0.0, 0.0)],
        rational.then(|| vec![2.0, 2.0]), false,
    ).unwrap().unwrap()
}

fn converted_bytes(rational: bool) -> u64 {
    let pole_size = if rational {
        std::mem::size_of::<cadmpeg_ir::geometry::nurbs::WeightedPole3<cadmpeg_ir::features::FinitePoint3>>()
    } else {
        std::mem::size_of::<cadmpeg_ir::features::FinitePoint3>()
    };
    // copy_slice retains four exact knots; IR collects two exact converted poles.
    u64::try_from(4 * std::mem::size_of::<f64>() + 2 * pole_size).unwrap()
}

#[test]
fn clamp_conversion_retains_only_knots_and_converted_poles() {
    for rational in [false, true] {
        let source = conversion_source(rational);
        let arena = DecodeArena::new();
        let mut policy = policy();
        let expected = converted_bytes(rational);
        policy.limits.max_retained_bytes = expected;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let curve = super::clamp_nurbs_curve_to_domain(&ctx, &source, [0.5, 1.5]).unwrap().unwrap();
        assert_eq!(curve.knots().as_slice(), [0.5, 0.5, 1.5, 1.5]);
        assert_eq!(curve.pole_count(), 2);
        assert_eq!(curve.pole_rows().point_at(0).unwrap().get(), cadmpeg_ir::math::Point3::new(0.5, 0.0, 0.0));
        assert_eq!(curve.pole_rows().point_at(1).unwrap().get(), cadmpeg_ir::math::Point3::new(1.5, 0.0, 0.0));
        assert_eq!(curve.pole_rows().weight_at(0), rational.then_some(2.0));
        assert_eq!(curve.pole_rows().weight_at(1), rational.then_some(2.0));
        drop(ctx.reserve_scoped(SCRATCH_BYTES, "test consumed clamp raw poles").unwrap());
        let Err(CodecError::ResourceLimit(limit)) = ctx.charge_retained(1, "test converted clamp backing") else { panic!("converted output boundary"); };
        assert_eq!((limit.used, limit.additional), (expected, 1));
        assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
    }
}

#[test]
fn clamp_conversion_transfers_only_surviving_backing_to_scoped_parent() {
    for rational in [false, true] {
        let source = conversion_source(rational);
        let arena = DecodeArena::new();
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
        let output_storage;
        let output;
        (output, output_storage) = ctx.with_scoped_storage("test clamp output parent", || {
            super::clamp_nurbs_curve_to_domain(&ctx, &source, [0.5, 1.5])
        }).unwrap();
        let curve = output.unwrap();
        assert_eq!(curve.pole_count(), 2);
        drop(ctx.reserve_scoped(SCRATCH_BYTES - converted_bytes(rational), "test live converted clamp backing").unwrap());
        drop(curve);
        drop(output_storage);
        drop(ctx.reserve_scoped(SCRATCH_BYTES, "test released converted clamp backing").unwrap());
        ctx.finish_session().unwrap();
    }
}

#[test]
fn clamp_conversion_keeps_original_knot_and_ir_pole_refusals() {
    let source = conversion_source(false);
    let knot_bytes = u64::try_from(4 * std::mem::size_of::<f64>()).unwrap();
    for (retained, operation, used, additional) in [
        (knot_bytes - 1, "copy clamped NURBS knots", 0, knot_bytes),
        (converted_bytes(false) - 1, "IR NURBS admitted poles", knot_bytes, converted_bytes(false) - knot_bytes),
    ] {
        let arena = DecodeArena::new();
        let mut policy = policy();
        policy.limits.max_retained_bytes = retained;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = super::clamp_nurbs_curve_to_domain(&ctx, &source, [0.5, 1.5]);
        let Err(crate::brep::graph::nurbs_pcurves::NurbsPcurveFailure::Resource(CodecError::ResourceLimit(limit))) = result else { panic!("original output refusal"); };
        assert_eq!(limit.operation, operation);
        assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        assert_eq!((limit.used, limit.additional), (used, additional));
        assert_eq!(limit.limit, retained);
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
    }
}

#[test]
fn clamp_raw_conversion_refuses_each_backing_before_allocation() {
    let point_bytes = u64::try_from(4 * std::mem::size_of::<cadmpeg_ir::math::Point3>()).unwrap();
    let weight_bytes = u64::try_from(4 * std::mem::size_of::<f64>()).unwrap();
    for (materialized, operation, used, additional, expected_allocations) in [
        (0, "collect clamped NURBS controls", 0, point_bytes, 0),
        (point_bytes, "collect clamped NURBS weights", point_bytes, weight_bytes, 1),
    ] {
        let arena = DecodeArena::new();
        let mut policy = policy();
        policy.limits.max_materialized_bytes = materialized;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let (result, allocations) = crate::test_support::allocation::count_allocations(|| {
            ctx.with_scoped_storage("test clamp raw conversion", || super::clamped_raw_poles(&ctx, &CONTROLS, true))
        });
        let CodecError::ResourceLimit(limit) = result.err().expect("raw backing refusal") else { panic!("resource refusal"); };
        assert_eq!(allocations, expected_allocations);
        assert_eq!(limit.operation, operation);
        assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
        assert_eq!((limit.used, limit.additional), (used, additional));
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
    }
}

#[test]
fn clamp_raw_conversion_refuses_first_slot_before_push() {
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| {
        ctx.with_scoped_storage("test clamp raw conversion", || super::clamped_raw_poles(&ctx, &CONTROLS, false))
    });
    let CodecError::ResourceLimit(limit) = result.err().expect("first raw slot refusal") else { panic!("resource refusal"); };
    assert_eq!(allocations, 1);
    assert_eq!(limit.operation, "collect clamped NURBS controls");
    assert_eq!(limit.dimension, ResourceDimension::CollectionItems);
    assert_eq!((limit.used, limit.additional), (0, 1));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn clamp_conversion_invalid_prefix_preserves_original_fuse_without_allocation() {
    let source = conversion_source(false);
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "test original clamp conversion refusal") else { panic!("original refusal"); };
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| {
        super::clamped_conversion_lanes(&ctx, &source, [1.0, 0.0])
    });
    let CodecError::ResourceLimit(actual) = result.err().expect("original conversion refusal") else { panic!("resource refusal"); };
    assert_eq!(actual, limit);
    assert_eq!(allocations, 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}
