// SPDX-License-Identifier: Apache-2.0

use crate::geometry::nurbs::NurbsCurve;
use crate::math::Point3;
use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

#[test]
fn raw_curve_constructor_refuses_weights_then_cardinality_then_poles_then_knots() {
    for (degree, knots, points, weights, expected) in [
        (
            1,
            vec![f64::NAN],
            vec![Point3::new(f64::NAN, 0.0, 0.0)],
            Some(vec![]),
            "poles: 1 pole(s) against 0 weight(s)",
        ),
        (
            1,
            vec![f64::NAN],
            vec![Point3::new(f64::NAN, 0.0, 0.0)],
            Some(vec![0.0]),
            "poles: weight 0 at index 0 is not a usable weight",
        ),
        (
            1,
            vec![f64::NAN],
            vec![Point3::new(f64::NAN, 0.0, 0.0)],
            None,
            "control_points must contain more than degree 1 poles, found 1",
        ),
        (
            1,
            vec![f64::NAN; 4],
            vec![Point3::new(f64::NAN, 0.0, 0.0); 2],
            None,
            "control_points contains a non-finite point",
        ),
        (
            1,
            vec![f64::NAN; 4],
            vec![Point3::new(0.0, 0.0, 0.0); 2],
            None,
            "knots contains a non-finite value",
        ),
        (
            1,
            vec![0.0, 1.0, 0.0, 1.0],
            vec![Point3::new(0.0, 0.0, 0.0); 2],
            None,
            "knots must be non-decreasing",
        ),
    ] {
        let actual = NurbsCurve::from_lanes(
            &cadmpeg_test_support::service_decode_context(),
            degree,
            knots,
            points,
            weights,
            false,
        )
        .expect("fixture constructor admission")
        .expect_err("invalid lanes")
        .to_string();
        assert_eq!(actual, expected);
    }
}

#[test]
fn raw_curve_constructor_converts_rational_poles() {
    let curve = NurbsCurve::from_lanes(
        &cadmpeg_test_support::service_decode_context(),
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0); 2],
        Some(vec![1.0; 2]),
        false,
    )
    .unwrap()
    .unwrap();
    assert_eq!(curve.pole_count(), 2);
}

#[test]
fn admitted_curve_constructor_keeps_polynomial_pole_storage() {
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let points = vec![crate::features::FinitePoint3::new(Point3::new(0.0, 0.0, 0.0)).unwrap(); 2];
    let address = points.as_ptr();
    let curve = NurbsCurve::from_lanes(&ctx, 1, vec![0.0, 0.0, 1.0, 1.0], points, None, false)
        .unwrap()
        .unwrap();
    let super::super::NurbsPoles3::Polynomial { points } = curve.into_parts().2 else {
        panic!("polynomial poles");
    };
    assert_eq!(points.as_ptr(), address);
}
