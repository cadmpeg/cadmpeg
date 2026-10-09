// SPDX-License-Identifier: Apache-2.0
//! Generated profiles and raw grids have distinct storage lifetimes.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use cadmpeg_ir::features::FinitePoint3;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsPoleGrid, NurbsSurface, WeightedPole3};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::units::SumSquaresUnitVector3;

use super::{profile_nurbs, spun_nurbs, swept_nurbs};
use crate::lane_refusal::LaneRefusals;

const SCRATCH_BYTES: u64 = 16 * 1024 * 1024;

fn circle() -> CurveGeometry {
    CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        cadmpeg_ir::geometry::analytic::CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0).unit().unwrap(),
            Vector3::new(1.0, 0.0, 0.0).unit().unwrap(),
            2.0,
        ).unwrap(),
    ))
}

fn line() -> NurbsCurve {
    NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(2.0, 0.0, 0.0), Point3::new(2.0, 0.0, 1.0)],
        None, false,
    ).unwrap().unwrap()
}

fn policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy
}

fn build_surface(ctx: &DecodeContext<'_>, profile: &NurbsCurve, spun: bool)
    -> Result<Option<NurbsSurface>, CodecError>
{
    let direction = SumSquaresUnitVector3::new(Vector3::new(0.0, 1.0, 0.0)).unwrap();
    if spun {
        spun_nurbs(ctx, profile, Point3::new(0.0, 0.0, 0.0), direction,
            &"ownership spin", &mut LaneRefusals::new())
    } else {
        swept_nurbs(ctx, profile, direction, -2.0, 3.0,
            &"ownership sweep", &mut LaneRefusals::new())
    }
}

fn surface_bytes(spun: bool) -> u64 {
    // Two u rows. Raw knot vectors move unchanged into the surface. IR maps
    // exact-size input Vecs to exact-size converted rows and outer storage.
    let knots = (4 + if spun { 12 } else { 4 }) * std::mem::size_of::<f64>();
    let poles = if spun {
        2 * std::mem::size_of::<Vec<WeightedPole3<FinitePoint3>>>()
            + 2 * 9 * std::mem::size_of::<WeightedPole3<FinitePoint3>>()
    } else {
        2 * std::mem::size_of::<Vec<FinitePoint3>>()
            + 2 * 2 * std::mem::size_of::<FinitePoint3>()
    };
    u64::try_from(knots + poles).unwrap()
}

#[test]
fn generated_profile_owns_only_final_knots_and_converted_poles() {
    let geometry = circle();
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let profile = profile_nurbs(&ctx, &geometry, &"ownership circle", &mut LaneRefusals::new())
        .unwrap().unwrap();
    assert_eq!(profile.curve().pole_count(), 9);
    assert_eq!(profile.curve().knots().as_slice(), super::FULL_RING_KNOTS);
    let expected = u64::try_from(12 * std::mem::size_of::<f64>()
        + 9 * std::mem::size_of::<WeightedPole3<FinitePoint3>>()).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = ctx.reserve_scoped(SCRATCH_BYTES - expected + 1, "test live generated profile") else {
        panic!("live profile probe");
    };
    assert_eq!((limit.used, limit.additional), (expected, SCRATCH_BYTES - expected + 1));
    assert_eq!(limit.limit, SCRATCH_BYTES);
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    drop(profile);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn generated_profile_drop_releases_all_backing_without_retained_publication() {
    let geometry = circle();
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let profile = profile_nurbs(&ctx, &geometry, &"ownership circle", &mut LaneRefusals::new())
        .unwrap().unwrap();
    drop(profile);
    drop(ctx.reserve_scoped(SCRATCH_BYTES, "test released generated profile").unwrap());
    ctx.finish_session().unwrap();
}

#[test]
fn borrowed_profile_is_free_and_preserves_the_original_fuse() {
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(line()));
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let profile = profile_nurbs(&ctx, &geometry, &"borrowed profile", &mut LaneRefusals::new())
        .unwrap().unwrap();
    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(source)) = &geometry else { unreachable!() };
    assert!(std::ptr::eq(profile.curve(), source));
    drop(profile);
    assert!(ctx.resource_refusal().is_none());
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "test borrowed profile fuse") else {
        panic!("work cap");
    };
    let result = profile_nurbs(&ctx, &geometry, &"borrowed profile", &mut LaneRefusals::new());
    assert!(matches!(&result, Err(CodecError::ResourceLimit(actual)) if *actual == limit));
    drop(result);
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn generated_profile_refuses_raw_storage_before_allocation() {
    let geometry = circle();
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_retained_bytes = 0;
    policy.limits.max_materialized_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| {
        profile_nurbs(&ctx, &geometry, &"ownership circle", &mut LaneRefusals::new())
    });
    let error = result.err().expect("raw profile storage refusal");
    let CodecError::ResourceLimit(limit) = error else { panic!("raw profile resource limit") };
    assert_eq!(allocations, 0, "raw profile backing is admitted before allocation");
    assert_eq!(limit.dimension, ResourceDimension::MaterializedBytes);
    assert_eq!(limit.operation, "construct SLDPRT sweep profile poles");
    assert_eq!((limit.used, limit.additional), (0, u64::try_from(9 * std::mem::size_of::<Point3>()).unwrap()));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn sweep_surface_retains_only_final_knots_and_converted_grid() {
    let profile = line();
    for spun in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = policy();
        let expected = surface_bytes(spun);
        policy.limits.max_retained_bytes = expected;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let surface = build_surface(&ctx, &profile, spun).unwrap().unwrap();
        assert_eq!((surface.u_count(), surface.v_count()), (2, if spun { 9 } else { 2 }));
        assert_eq!(surface.u_knots().as_slice(), profile.knots().as_slice());
        assert_eq!(matches!(surface.pole_grid(), NurbsPoleGrid::Rational { .. }), spun);
        // All raw Point3/weight grids and IR pairing storage have ended.
        drop(ctx.reserve_scoped(SCRATCH_BYTES, "test released sweep raw grid").unwrap());
        let Err(CodecError::ResourceLimit(limit)) = ctx.charge_retained(1, "test final sweep bytes") else {
            panic!("exact final surface storage");
        };
        assert_eq!((limit.used, limit.additional), (expected, 1));
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
    }
}

#[test]
fn sweep_surface_publication_refuses_exact_surviving_backing() {
    let profile = line();
    for spun in [false, true] {
        let arena = DecodeArena::new();
        let mut policy = policy();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let Err(CodecError::ResourceLimit(limit)) = build_surface(&ctx, &profile, spun) else {
            panic!("surface publication refusal");
        };
        assert_eq!(limit.operation, "retain SLDPRT sweep surface candidate");
        assert_eq!(limit.dimension, ResourceDimension::RetainedBytes);
        assert_eq!((limit.used, limit.additional), (0, surface_bytes(spun)));
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
    }
}

#[test]
fn rejected_sweep_grid_releases_candidate_and_raw_storage_after_note() {
    let profile = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(), 1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(f64::MAX, 1.0, 0.0)],
        None, false,
    ).unwrap().unwrap();
    let expected_message = "sldprt swept ruled surface patch: ownership rejection: control_points contains a non-finite point";
    let expected = u64::try_from(expected_message.len() + 4 * std::mem::size_of::<String>()).unwrap();
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_retained_bytes = expected;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut refusals = LaneRefusals::new();
    let direction = SumSquaresUnitVector3::new(Vector3::new(1.0, 0.0, 0.0)).unwrap();
    assert!(swept_nurbs(&ctx, &profile, direction, 0.0, f64::MAX,
        &"ownership rejection", &mut refusals).unwrap().is_none());
    assert_eq!(refusals.take_records(), [expected_message]);
    drop(ctx.reserve_scoped(SCRATCH_BYTES, "test released rejected sweep grid").unwrap());
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_retained(1, "test sweep note-only bytes") else {
        panic!("exact retained refusal record");
    };
    assert_eq!((limit.used, limit.additional), (expected, 1));
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}


#[test]
fn invalid_sweep_bounds_preserve_the_original_sticky_refusal() {
    let profile = line();
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "test original sweep refusal") else {
        panic!("original work refusal");
    };
    let direction = SumSquaresUnitVector3::new(Vector3::new(0.0, 1.0, 0.0)).unwrap();
    let mut refusals = LaneRefusals::new();
    let result = swept_nurbs(&ctx, &profile, direction, f64::NAN, 1.0,
        &"invalid travel", &mut refusals);
    assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == limit));
    assert!(refusals.take_records().is_empty());
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}
