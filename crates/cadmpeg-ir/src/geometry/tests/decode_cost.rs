// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::cost::DecodeCost;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
use cadmpeg_core::CodecError;

use crate::geometry::analytic::LineCurve;
use crate::geometry::{PlacedCurve, SolvedCurveGeometry};
use crate::math::{Point3, Vector3};
use crate::transform::Transform;

#[test]
fn nurbs_cost_counts_every_knot_pole_weight_and_scalar() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let curve = crate::test_support::nurbs::curve();
    let surface = crate::test_support::nurbs::surface();
    // Curve: degree 4, knots 32, pole-form tag 1, weighted poles 64, periodic flag 1.
    assert_eq!(curve.decode_cost(&ctx, "curve field cost").unwrap(), 102);
    // Surface: degrees 8, knots 64, pole-form tag 1, weighted grid 128, flags 3.
    assert_eq!(surface.decode_cost(&ctx, "surface field cost").unwrap(), 204);
}

#[test]
fn nurbs_cost_row_visit_refusal_reaches_caller() {
    let surface = crate::test_support::nurbs::surface();
    let before = surface.clone();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = surface.decode_cost(&ctx, "surface row cost").unwrap_err();
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("row measurement must preserve its resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::WorkUnits);
    assert_eq!(refusal.operation, "surface row cost");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
    assert_eq!(surface, before);
}

#[test]
fn recursive_geometry_cost_refuses_depth_before_descent() {
    let line = SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0))
            .unwrap(),
    );
    let placed = SolvedCurveGeometry::Transformed(
        PlacedCurve::try_new(Box::new(line), Transform::identity()).unwrap(),
    );
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = placed.decode_cost(&ctx, "recursive geometry cost").unwrap_err();
    let CodecError::ResourceLimit(refusal) = error else {
        panic!("cost descent must preserve its resource refusal");
    };
    assert_eq!(refusal.dimension, ResourceDimension::RecursionDepth);
    assert_eq!(refusal.operation, "recursive geometry cost");
    assert_eq!(ctx.resource_refusal(), Some(refusal));
}
