// SPDX-License-Identifier: Apache-2.0
//! Borrowed pole traversal has one progressive coordinate-scale admission.

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::decode::refusal_probe::RefusalProbe;
use cadmpeg_core::CodecError;
use cadmpeg_ir::geometry::nurbs::{NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use cadmpeg_ir::math::Point3;
use crate::brep::graph::inverse::inverse_coordinate_tolerance;
use crate::brep::graph::inverse::nurbs_parameter_at_point;
use crate::brep::graph::inverse::surface_poles;
use crate::brep::graph::inverse::InverseResolution;

const SCRATCH_BYTES: u64 = 16 * 1024 * 1024;

fn surface(rational: bool) -> NurbsSurface {
    NurbsSurface::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(vec![
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 7.0, 0.0)],
            vec![Point3::new(7.0, 0.0, 0.0), Point3::new(7.0, 7.0, 0.0)],
        ], rational.then(|| vec![vec![1.0, 1.0], vec![1.0, 1.0]])), false,
    ).unwrap().unwrap()
}

fn policy() -> DecodePolicy {
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = SCRATCH_BYTES;
    policy.limits.max_retained_bytes = 0;
    policy
}

#[test]
fn borrowed_grid_scale_uses_one_visit_per_pole() {
    for rational in [false, true] {
        let surface = surface(rational);
        let arena = DecodeArena::new();
        let mut policy = policy();
        // Four in-bounds rectangular poles, followed by the inherited core
        // next_charged exhaustion request. No second whole-grid prepayment.
        policy.limits.max_work_units = 5;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let (result, allocations) = crate::test_support::allocation::count_allocations(|| {
            inverse_coordinate_tolerance(&ctx, surface_poles(&ctx, &surface).unwrap().map(|point| point.get()))
        });
        assert_eq!(result.unwrap(), crate::brep::graph::inverse::looser_tolerance(crate::brep::graph::inverse::INVERSE_ABSOLUTE_TOLERANCE_MM, 7.0 * crate::brep::graph::inverse::INVERSE_RELATIVE_TOLERANCE));
        assert_eq!(allocations, 0);
        let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "test consumed scale budget") else { panic!("single-pass budget"); };
        assert_eq!(limit.dimension, ResourceDimension::WorkUnits);
        assert_eq!((limit.used, limit.additional, limit.limit), (5, 1, 5));
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
    }
}

#[test]
fn pole_scale_refuses_before_the_first_projection() {
    let surface = surface(false);
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut visits = 0;
    let poles = surface_poles(&ctx, &surface).unwrap();
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| {
        inverse_coordinate_tolerance(&ctx, poles.map(|point| { visits += 1; point.get() }))
    });
    let Err(CodecError::ResourceLimit(limit)) = result else { panic!("first scale visit"); };
    assert_eq!(visits, 0);
    assert_eq!(allocations, 0);
    assert_eq!(limit.operation, "measure Parasolid inverse coordinate scale");
    assert_eq!((limit.used, limit.additional, limit.limit), (0, 1, 0));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}

#[test]
fn curve_inverse_scale_avoids_an_extent_prepayment() {
    for rational in [false, true] {
        let curve = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(), 1,
            vec![0.0, 0.0, 1.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
            rational.then(|| vec![1.0, 1.0]), false,
        ).unwrap().unwrap();
        let arena = DecodeArena::new();
        let mut policy = policy();
        policy.limits.max_work_units = u64::MAX;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let probe = RefusalProbe::arm(ResourceDimension::WorkUnits, "scan SLDPRT nurbs_parameter_at_point values", None);
        let InverseResolution::Unique(parameter) = nurbs_parameter_at_point(&ctx, &curve, Point3::new(1.0, 0.0, 0.0)).unwrap()
        else { panic!("unique midpoint inverse"); };
        assert!((parameter - 0.5).abs() <= crate::brep::graph::inverse::INVERSE_PARAMETER_TOLERANCE);
        let Err(CodecError::ResourceLimit(limit)) = ctx.charge_work(1, "scan SLDPRT nurbs_parameter_at_point values") else { panic!("unvisited extent prepayment"); };
        drop(probe);
        assert_eq!(limit.additional, 1);
        assert_eq!(ctx.resource_refusal(), Some(limit));
        assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
    }
}

#[test]
fn borrowed_scale_preserves_prior_refusal_before_projection() {
    let surface = surface(true);
    let arena = DecodeArena::new();
    let mut policy = policy();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let Err(CodecError::ResourceLimit(limit)) = ctx.charge_collection_items(1, "test original scale refusal") else { panic!("original refusal"); };
    let poles = surface_poles(&ctx, &surface).unwrap();
    let mut visits = 0;
    let (result, allocations) = crate::test_support::allocation::count_allocations(|| {
        inverse_coordinate_tolerance(&ctx, poles.map(|point| { visits += 1; point.get() }))
    });
    assert!(matches!(result, Err(CodecError::ResourceLimit(actual)) if actual == limit));
    assert_eq!(visits, 0);
    assert_eq!(allocations, 0);
    assert_eq!(ctx.resource_refusal(), Some(limit));
    assert!(matches!(ctx.finish_session(), Err(CodecError::ResourceLimit(actual)) if actual == limit));
}
