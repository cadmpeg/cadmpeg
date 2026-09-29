// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
use cadmpeg_core::CodecError;
use crate::geometry::nurbs::{NurbsCurve, NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
use crate::math::Point3;

fn with_limit<T>(cap: u64, f: impl FnOnce(&DecodeContext<'_>) -> T) -> T {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = cap;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    f(&ctx)
}

#[test]
fn raw_curve_constructor_refuses_pairing_and_admission() {
    for (cap, operation) in [(1, "IR NURBS paired poles"), (3, "IR NURBS admitted poles")] {
        let result = with_limit(cap, |ctx| NurbsCurve::from_lanes_admitted(ctx, 1,
            vec![0.0, 0.0, 1.0, 1.0], vec![Point3::new(0.0, 0.0, 0.0); 2], Some(vec![1.0; 2]), false));
        assert!(matches!(result, Err(CodecError::ResourceLimit(resource)) if resource.operation == operation));
    }
    let points = vec![Point3::new(0.0, 0.0, 0.0); 2];
    let expected = NurbsCurve::from_lanes(1, vec![0.0, 0.0, 1.0, 1.0], points.clone(), Some(vec![1.0; 2]), false);
    assert_eq!(with_limit(4, |ctx| NurbsCurve::from_lanes_admitted(ctx, 1,
        vec![0.0, 0.0, 1.0, 1.0], points, Some(vec![1.0; 2]), false)).expect("exact cap"), expected);
}

#[test]
fn raw_surface_constructor_refuses_each_nested_collection() {
    let make = |ctx: &DecodeContext<'_>| NurbsSurface::from_lanes_admitted(ctx,
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(vec![vec![Point3::new(0.0, 0.0, 0.0); 2]; 2], Some(vec![vec![1.0; 2]; 2])), false);
    for (cap, operation) in [(0, "IR NURBS paired grid rows"), (2, "IR NURBS paired poles"),
        (3, "IR NURBS paired grid rows"), (5, "IR NURBS paired poles"),
        (6, "IR NURBS admitted grid rows"), (8, "IR NURBS admitted poles"),
        (9, "IR NURBS admitted grid rows"), (11, "IR NURBS admitted poles")] {
        assert!(matches!(with_limit(cap, make), Err(CodecError::ResourceLimit(resource)) if resource.operation == operation));
    }
    let expected = NurbsSurface::from_lanes(
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false),
        NurbsSurfaceLanes::new(vec![vec![Point3::new(0.0, 0.0, 0.0); 2]; 2], Some(vec![vec![1.0; 2]; 2])), false);
    assert_eq!(with_limit(12, make).expect("exact cap"), expected);
}

#[test]
fn raw_constructor_refusal_text_is_admitted_and_keeps_order() {
    for (degree, knots, points, weights) in [
        (1, vec![f64::NAN], vec![Point3::new(f64::NAN, 0.0, 0.0)], Some(vec![])),
        (1, vec![f64::NAN], vec![Point3::new(f64::NAN, 0.0, 0.0)], Some(vec![0.0])),
        (1, vec![f64::NAN], vec![Point3::new(f64::NAN, 0.0, 0.0)], None),
        (1, vec![f64::NAN; 4], vec![Point3::new(f64::NAN, 0.0, 0.0); 2], None),
        (1, vec![f64::NAN; 4], vec![Point3::new(0.0, 0.0, 0.0); 2], None),
        (1, vec![0.0, 1.0, 0.0, 1.0], vec![Point3::new(0.0, 0.0, 0.0); 2], None),
    ] {
        let expected = NurbsCurve::from_lanes(degree, knots.clone(), points.clone(), weights.clone(), false)
            .expect_err("invalid lanes").to_string();
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_retained_bytes = 0;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
        assert!(matches!(NurbsCurve::from_lanes_admitted(&ctx, degree, knots.clone(), points.clone(), weights.clone(), false),
            Err(CodecError::ResourceLimit(resource)) if resource.dimension == cadmpeg_core::decode::ResourceDimension::RetainedBytes));
        let actual = with_limit(u64::MAX, |ctx| NurbsCurve::from_lanes_admitted(ctx, degree, knots, points, weights, false))
            .expect("service refusal text").expect_err("same geometry refusal").to_string();
        assert_eq!(actual, expected);
    }
}
