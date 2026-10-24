// SPDX-License-Identifier: Apache-2.0
//! The canonical interval of one closed traversal of a curve carrier.

use std::f64::consts::TAU;

use crate::geometry::analytic::{CircleCurve, EllipseCurve, LineCurve};
use crate::geometry::nurbs::NurbsCurve;
use crate::geometry::{PlacedCurve, SolvedCurveGeometry};
use crate::math::{Point3, Vector3};
use crate::transform::Transform;

fn endpoints(geometry: &SolvedCurveGeometry) -> Option<[f64; 2]> {
    geometry
        .closed_traversal_range()
        .map(crate::topology::IncreasingParameterInterval::endpoints)
}

/// A degree-1 square loop over the clamped knot domain `[1, 5]`.
fn clamped_square() -> NurbsCurve {
    NurbsCurve::from_lanes(
        1,
        vec![1.0, 1.0, 2.0, 3.0, 4.0, 5.0, 5.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
        ],
        None,
        false,
    )
    .unwrap()
}

#[test]
fn conics_traverse_one_revolution() {
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    let circle = SolvedCurveGeometry::Circle(
        CircleCurve::try_new(Point3::new(0.0, 0.0, 0.0), axis, reference, 2.0).unwrap(),
    );
    let ellipse = SolvedCurveGeometry::Ellipse(
        EllipseCurve::try_new(Point3::new(0.0, 0.0, 0.0), axis, reference, 3.0, 1.0).unwrap(),
    );
    assert_eq!(endpoints(&circle), Some([0.0, TAU]));
    assert_eq!(endpoints(&ellipse), Some([0.0, TAU]));
}

#[test]
fn clamped_and_periodic_nurbs_traverse_their_knot_domain() {
    let clamped = SolvedCurveGeometry::Nurbs(clamped_square());
    assert_eq!(endpoints(&clamped), Some([1.0, 5.0]));
    let periodic = NurbsCurve::from_lanes(
        1,
        vec![0.0, 1.0, 2.0, 3.0, 4.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(1.0, 1.0, 0.0),
        ],
        None,
        true,
    )
    .unwrap();
    assert_eq!(
        endpoints(&SolvedCurveGeometry::Nurbs(periodic)),
        Some([1.0, 3.0])
    );
}

#[test]
fn placement_keeps_the_basis_interval() {
    let placed = SolvedCurveGeometry::Transformed(
        PlacedCurve::try_new(
            Box::new(SolvedCurveGeometry::Nurbs(clamped_square())),
            Transform::identity(),
        )
        .unwrap(),
    );
    assert_eq!(endpoints(&placed), Some([1.0, 5.0]));
}

#[test]
fn a_line_has_no_closed_traversal() {
    let line = SolvedCurveGeometry::Line(
        LineCurve::try_new(Point3::new(0.0, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)).unwrap(),
    );
    assert_eq!(endpoints(&line), None);
}
