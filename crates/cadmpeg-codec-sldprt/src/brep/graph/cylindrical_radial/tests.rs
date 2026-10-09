// SPDX-License-Identifier: Apache-2.0
//! Radial projection lifetime and source-backed refusal boundaries.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::math::{Point2, Point3};
use cadmpeg_ir::scalar::PositiveLength;
use crate::brep::graph::cylindrical_radial::CylindricalRadialControls;

const SCRATCH_BYTES: u64 = 16 * 1024 * 1024;

fn curve(rational: bool) -> NurbsCurve {
    NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 2.0, 0.0), Point3::new(0.0, 2.0, 0.0)],
        rational.then(|| vec![1.0, std::f64::consts::FRAC_1_SQRT_2, 1.0]), false,
    ).unwrap().unwrap()
}

fn policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy.limits.max_retained_bytes = 0;
    policy
}

fn project(point: Point3) -> Point2 { Point2::new(point.x, point.y) }
fn radius() -> PositiveLength { PositiveLength::new(2.0).unwrap() }
fn radial_bytes() -> u64 { u64::try_from(4 * std::mem::size_of::<Point2>()).unwrap() }
fn weight_bytes() -> u64 { u64::try_from(4 * std::mem::size_of::<f64>()).unwrap() }

#[test]
fn radius_validation_releases_weights_before_polar_projection() {
    let curve = curve(true);
    let arena = DecodeArena::new();
    let mut policy = policy();
    // collect_vec starts at minimum capacity4 for each three-pole lane.
    policy.limits.max_materialized_bytes = radial_bytes() + weight_bytes();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let radial = CylindricalRadialControls::new(&ctx, &curve, project).unwrap();
    assert!(radial.has_constant_radius(radius()).unwrap());
    assert_eq!(radial.values(), [Point2::new(2.0, 0.0), Point2::new(2.0, 2.0), Point2::new(0.0, 2.0)]);
    drop(ctx.reserve_scoped(weight_bytes(), "test expired radius weights").unwrap());
    drop(radial);
    drop(ctx.reserve_scoped(policy.limits.max_materialized_bytes, "test expired radial controls").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn rejected_polynomial_radius_releases_controls_without_retained_storage() {
    let curve = curve(false);
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy()).unwrap();
    let radial = CylindricalRadialControls::new(&ctx, &curve, project).unwrap();
    assert!(!radial.has_constant_radius(radius()).unwrap());
    drop(radial);
    drop(ctx.reserve_scoped(SCRATCH_BYTES, "test rejected radial controls").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn radial_projection_refuses_before_the_first_backing_allocation() {
    let curve = curve(true);
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| CylindricalRadialControls::new(&ctx, &curve, project));
    let Err(CodecError::ResourceLimit(limit)) = result else { panic!("radial backing refusal"); };
    drop(result);
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "collect Parasolid cylinder radial poles");
    assert_eq!((limit.used, limit.additional), (0, radial_bytes()));
    assert_eq!(allocations, 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn radius_weight_refusal_keeps_live_radial_backing_and_original_fuse() {
    let curve = curve(true);
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_materialized_bytes = radial_bytes();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let radial = CylindricalRadialControls::new(&ctx, &curve, project).unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| radial.has_constant_radius(radius()));
    let Err(CodecError::ResourceLimit(limit)) = result else { panic!("weight backing refusal"); };
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "collect Parasolid cylinder pole weights");
    assert_eq!((limit.used, limit.additional), (radial_bytes(), weight_bytes()));
    assert_eq!(allocations, 0);
    drop(radial);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn radial_projection_preserves_prior_refusal_without_allocating() {
    let curve = curve(true);
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_collection_items(1, "test original radial refusal") else { panic!("original refusal"); };
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| CylindricalRadialControls::new(&ctx, &curve, project));
    assert!(matches!(&result, Err(CodecError::ResourceLimit(actual)) if *actual == limit));
    assert_eq!(allocations, 0);
    drop(result);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}
