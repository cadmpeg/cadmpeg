// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;
use crate::geometry::{CurveGeometry, SolvedCurveGeometry};
use crate::geometry::nurbs::NurbsCurve;
use crate::math::Point3;

fn curve() -> NurbsCurve {
    NurbsCurve::from_lanes(1, vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)], None, false)
        .expect("finite line spline")
}

#[test]
fn admitted_curve_point_refuses_each_scratch_collection() {
    let curve = curve();
    for (cap, operation) in [(1, "IR B-spline basis"), (3, "IR local NURBS poles")] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert!(matches!(super::nurbs_curve_point_at(&ctx, &curve, 0.5),
            Err(CodecError::ResourceLimit(resource)) if resource.dimension == ResourceDimension::CollectionItems
                && resource.operation == operation));
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(super::nurbs_curve_point_at(&ctx, &curve, 0.5).expect("exact scratch cap"),
        crate::eval::nurbs_curve_point_at(&curve, 0.5));
}

#[test]
fn admitted_curve_point_refuses_basis_work() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(matches!(super::nurbs_curve_point_at(&ctx, &curve(), 0.5),
        Err(CodecError::ResourceLimit(resource)) if resource.dimension == ResourceDimension::WorkUnits
            && resource.operation == "IR B-spline basis work"));
}

#[test]
fn admitted_curve_tangent_refuses_point_copy() {
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve()));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 1;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(matches!(super::curve_tangent(&ctx, &geometry, 0.5),
        Err(CodecError::ResourceLimit(resource)) if resource.operation == "IR NURBS derivative points"));
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(super::curve_tangent(&ctx, &geometry, 0.5).expect("service scratch"),
        crate::eval::curve_tangent(&geometry, 0.5));
}

#[test]
fn admitted_surface_point_refuses_both_axis_bases() {
    use crate::geometry::{SurfaceGeometry, SolvedSurfaceGeometry};
    use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    let surface = NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(vec![vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 1.0, 0.0)],
            vec![Point3::new(1.0, 0.0, 0.0), Point3::new(1.0, 1.0, 0.0)]],
        None), false).expect("finite plane spline");
    let geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(surface));
    for cap in [1, 3] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert!(matches!(super::surface_point(&ctx, &geometry, 0.5, 0.5),
            Err(CodecError::ResourceLimit(resource)) if resource.operation == "IR B-spline basis"));
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 4;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(super::surface_point(&ctx, &geometry, 0.5, 0.5).expect("exact cap"),
        crate::eval::surface_point(&geometry, 0.5, 0.5));
}

#[test]
fn admitted_pcurve_point_refuses_weights_poles_and_derivative_bases() {
    use crate::geometry::pcurve::{PcurveGeometry, PcurveNurbs};
    use crate::math::Point2;
    let pcurve = PcurveGeometry::Nurbs { nurbs: PcurveNurbs::from_lanes(1,
        vec![0.0, 0.0, 1.0, 1.0], vec![Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
        Some(vec![1.0, 1.0]), false).expect("finite rational line pcurve") };
    for (cap, operation) in [(1, "IR NURBS pcurve weights"), (3, "IR B-spline basis"),
        (5, "IR local NURBS poles"), (6, "IR B-spline basis"), (8, "IR B-spline derivative basis")] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_collection_items = cap;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert!(matches!(super::pcurve_uv(&ctx, &pcurve, 0.5),
            Err(CodecError::ResourceLimit(resource)) if resource.operation == operation));
    }
    let arena = DecodeArena::new();
    let policy = DecodePolicy::service();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert_eq!(super::pcurve_uv(&ctx, &pcurve, 0.5).expect("service scratch"),
        crate::eval::pcurve_uv(&pcurve, 0.5));
}

#[test]
fn admitted_curve_point_refuses_recursive_frame() {
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve()));
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    assert!(matches!(super::curve_point(&ctx, &geometry, 0.5),
        Err(CodecError::ResourceLimit(resource)) if resource.dimension == ResourceDimension::RecursionDepth
            && resource.operation == "geometry evaluation nesting"));
}
