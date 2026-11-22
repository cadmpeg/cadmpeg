// SPDX-License-Identifier: Apache-2.0

use super::{FiniteSpeed, SPEED_COORDINATE_SEPARATION};
use cadmpeg_ir::geometry::nurbs::bounds::speed_bound_by;
use cadmpeg_ir::geometry::nurbs::NurbsCurve;
use cadmpeg_ir::math::Point3;

fn curve(knot_scale: f64, coordinate_scale: f64, weights: Option<Vec<f64>>) -> NurbsCurve {
    NurbsCurve::from_lanes(
        2,
        vec![0.0, 0.0, 0.0, knot_scale, knot_scale, knot_scale],
        vec![
            Point3::new(-coordinate_scale, 0.25 * coordinate_scale, 0.0),
            Point3::new(
                0.5 * coordinate_scale,
                coordinate_scale,
                -0.5 * coordinate_scale,
            ),
            Point3::new(coordinate_scale, 0.0, 0.0),
        ],
        weights,
        false,
    )
    .unwrap()
}

fn bound(curve: &NurbsCurve, origin: Point3) -> Option<f64> {
    speed_bound_by(
        curve.degree(),
        curve.knots(),
        curve.pole_count(),
        |index| {
            curve
                .pole_rows()
                .point_at(index)
                .map(|point| point.get().into())
        },
        |index| curve.pole_rows().weight_at(index).unwrap_or(1.0),
        origin.into(),
    )
    .map(cadmpeg_ir::scalar::FiniteReal::get)
}

#[test]
fn admitted_queries_keep_a_finite_original_speed_bound() {
    for weights in [
        None,
        Some(vec![1.0, 2.0, 1.0]),
        Some(vec![1e-10, 1e10, 1e-10]),
    ] {
        for knot_scale in [1e-50, 1.0, 1e50] {
            for coordinate_scale in [1e-50, 1.0, 1e50] {
                let curve = curve(knot_scale, coordinate_scale, weights.clone());
                let speed = FiniteSpeed::new(&curve).unwrap();
                for origin in [
                    Point3::new(0.0, 0.0, 0.0),
                    Point3::new(1e50, -1e50, 1e50),
                    Point3::new(-1e50, 0.0, 0.0),
                ] {
                    assert!(speed.admits(origin));
                    assert!(bound(&curve, origin).is_some_and(f64::is_finite));
                }
            }
        }
    }
}

#[test]
fn extreme_curve_ranges_keep_the_eager_bound() {
    for curve in [
        curve(1e-200, 1.0, None),
        curve(1e60, 1.0, None),
        curve(1.0, 1e60, None),
        curve(1.0, 1.0, Some(vec![1e-200, 1.0, 1.0])),
        curve(1.0, 1.0, Some(vec![-1.0, 1.0, 1.0])),
    ] {
        assert!(FiniteSpeed::new(&curve).is_none());
    }
}

#[test]
fn tiny_components_and_large_queries_keep_the_eager_bound() {
    let small = curve(1.0, 1e-200, Some(vec![1.0, 2.0, 1.0]));
    let speed = FiniteSpeed::new(&small).unwrap();
    assert!(!speed.admits(Point3::new(0.0, 0.0, 0.0)));
    let ordinary = curve(1.0, 1.0, None);
    let speed = FiniteSpeed::new(&ordinary).unwrap();
    assert!(!speed.admits(Point3::new(1e51, 0.0, 0.0)));
    assert!(!speed.admits(Point3::new(f64::NAN, 0.0, 0.0)));
}

#[test]
fn coincident_poles_admit_a_zero_speed_bound() {
    let curve = NurbsCurve::from_lanes(
        1,
        vec![0.0, 0.0, 1.0, 1.0],
        vec![Point3::new(0.0, 0.0, 0.0), Point3::new(0.0, 0.0, 0.0)],
        Some(vec![1.0, 2.0]),
        false,
    )
    .unwrap();
    let speed = FiniteSpeed::new(&curve).unwrap();
    let origin = Point3::new(0.0, 0.0, 0.0);
    assert!(speed.admits(origin));
    assert_eq!(bound(&curve, origin), Some(0.0));
}

#[test]
fn separated_coordinates_keep_the_component_guard() {
    let lower = SPEED_COORDINATE_SEPARATION;
    let curve = NurbsCurve::from_lanes(
        2,
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        vec![
            Point3::new(lower, 0.0, 0.0),
            Point3::new(2.0 * lower, lower, 0.0),
            Point3::new(3.0 * lower, 0.0, lower),
        ],
        Some(vec![1e-10, 1e10, 1e-10]),
        false,
    )
    .unwrap();
    let speed = FiniteSpeed::new(&curve).unwrap();
    assert!(speed.separated);
    let adjacent = f64::from_bits(lower.to_bits() + 1);
    for x in [0.0, lower, adjacent, -lower, 1.0, -1e50, 1e50] {
        let origin = Point3::new(x, -x, 0.0);
        assert!(speed.admits(origin));
        assert!(speed.components_admitted(origin.into()));
        assert!(bound(&curve, origin).is_some_and(f64::is_finite));
    }
    let origin = Point3::new(lower * 0.5, 0.0, 0.0);
    assert_eq!(
        speed.admits(origin),
        speed.components_admitted(origin.into())
    );
}
