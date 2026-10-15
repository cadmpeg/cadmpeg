// SPDX-License-Identifier: Apache-2.0

use crate::features::FinitePoint3;
use crate::geometry::nurbs::{
    KnotVector, NurbsCurve, NurbsPoleGrid, NurbsPoles3, NurbsSurface, NurbsSurfaceAxis,
};
use crate::math::Point3;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

fn knots() -> Vec<f64> {
    vec![0.0, 0.0, 1.0, 1.0]
}
fn raw_points() -> Vec<Point3> {
    vec![Point3::new(1.0, 2.0, 3.0), Point3::new(4.0, 5.0, 6.0)]
}
fn finite_points() -> Vec<FinitePoint3> {
    raw_points()
        .into_iter()
        .map(|point| FinitePoint3::new(point).expect("finite"))
        .collect()
}

#[test]
fn final_nurbs_construction_moves_admitted_storage_without_copy_or_scalar_readmission() {
    let setup = cadmpeg_test_support::service_decode_context();
    let curve_knots = KnotVector::new(&setup, knots())
        .expect("setup")
        .expect("knots");
    let knot_address = curve_knots.as_ptr();
    let points = finite_points();
    let point_address = points.as_ptr();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let curve = NurbsCurve::new(
        &ctx,
        1,
        curve_knots,
        NurbsPoles3::Polynomial { points },
        false,
    )
    .expect("owned admitted storage")
    .expect("valid curve");
    assert_eq!(curve.knots().as_ptr(), knot_address);
    let NurbsPoles3::Polynomial { points } = curve.pole_rows() else {
        panic!("polynomial");
    };
    assert_eq!(points.as_ptr(), point_address);
    ctx.finish_session()
        .expect("no input-sized operation needed");
    let u_knots = KnotVector::new(&setup, knots())
        .expect("setup")
        .expect("knots");
    let v_knots = KnotVector::new(&setup, knots())
        .expect("setup")
        .expect("knots");
    let rows = vec![finite_points(), finite_points()];
    let outer = rows.as_ptr();
    let inner = rows[0].as_ptr();
    let u_address = u_knots.as_ptr();
    let v_address = v_knots.as_ptr();
    let arena = DecodeArena::new();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).expect("root");
    let surface = NurbsSurface::new(
        &ctx,
        NurbsSurfaceAxis::new(1, u_knots, false),
        NurbsSurfaceAxis::new(1, v_knots, false),
        NurbsPoleGrid::Polynomial { rows },
        true,
    )
    .expect("two row-width visits")
    .expect("valid surface");
    let NurbsPoleGrid::Polynomial { rows } = surface.pole_grid() else {
        panic!("polynomial");
    };
    assert_eq!(rows.as_ptr(), outer);
    assert_eq!(rows[0].as_ptr(), inner);
    assert_eq!(surface.u_knots().as_ptr(), u_address);
    assert_eq!(surface.v_knots().as_ptr(), v_address);
    ctx.finish_session().expect("exact row-width work");
    let wire = serde_json::to_value(&surface).expect("wire");
    assert_eq!(
        serde_json::from_value::<NurbsSurface>(wire).expect("context-free reconstruction"),
        surface
    );
}

#[test]
fn raw_curve_construction_moves_raw_knot_storage() {
    let ctx = cadmpeg_test_support::service_decode_context();
    let knots = knots();
    let address = knots.as_ptr();
    let curve = NurbsCurve::from_lanes(&ctx, 1, knots, raw_points(), None, false)
        .expect("construction admission")
        .expect("valid curve");
    assert_eq!(curve.knots().as_ptr(), address);
    assert_eq!(curve.knots().as_slice(), &[0.0, 0.0, 1.0, 1.0]);
}
