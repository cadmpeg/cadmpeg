// SPDX-License-Identifier: Apache-2.0
//! Each conic curve's NURBS form equals the conic at sample parameters.

use cadmpeg_ir::eval::admission::EvaluationAdmission;
use cadmpeg_ir::eval::decode::{curve_point_solved, nurbs_curve_point_at};
use cadmpeg_ir::geometry::analytic::{CircleCurve, EllipseCurve, HyperbolaCurve, ParabolaCurve};
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::math::{Point3, Vector3};

use super::super::exact::ArcAxis;
use super::curve_nurbs;

/// Relative distance within which a conic and its NURBS form agree at a
/// sample parameter.
const EXACT_FORM_AGREEMENT: f64 = 1e-9;

fn assert_nurbs_equals(curve: &SolvedCurveGeometry, range: [f64; 2], map: impl Fn(f64) -> f64) {
    let nurbs = curve_nurbs(curve, range).expect("the conic has a NURBS form");
    for step in 0..=8 {
        let parameter = range[0] + (range[1] - range[0]) * f64::from(step) / 8.0;
        let expected = curve_point_solved(EvaluationAdmission::Standard, curve, parameter)
            .expect("the conic has a point")
            .get();
        let actual = nurbs_curve_point_at(EvaluationAdmission::Standard, &nurbs, map(parameter))
            .expect("the NURBS form has a point");
        let scale = 1.0 + expected.x.abs().max(expected.y.abs()).max(expected.z.abs());
        assert!(
            actual.distance(expected) <= EXACT_FORM_AGREEMENT * scale,
            "{curve:?} at {parameter}"
        );
    }
}

fn arc_map(range: [f64; 2], hyperbolic: bool) -> impl Fn(f64) -> f64 {
    let axis = ArcAxis::new(range[0], range[1], hyperbolic).expect("the range has arcs");
    move |value| axis.parameter(value)
}

#[test]
fn conic_curves_equal_their_nurbs_forms() {
    let center = Point3::new(2.0, -1.0, 0.5);
    let axis = Vector3::new(0.0, 0.6, 0.8);
    let major = Vector3::new(1.0, 0.0, 0.0);

    let circle = SolvedCurveGeometry::Circle(
        CircleCurve::try_new(center, axis, major, 1.5).expect("circle fixture"),
    );
    let range = [0.25, 4.0];
    assert_nurbs_equals(&circle, range, arc_map(range, false));

    let ellipse = SolvedCurveGeometry::Ellipse(
        EllipseCurve::try_new(center, axis, major, 3.0, 1.0).expect("ellipse fixture"),
    );
    let range = [0.0, std::f64::consts::TAU];
    assert_nurbs_equals(&ellipse, range, arc_map(range, false));

    let hyperbola = SolvedCurveGeometry::Hyperbola(
        HyperbolaCurve::try_new(center, axis, major, 2.0, 0.5).expect("hyperbola fixture"),
    );
    let range = [-1.5, 2.0];
    assert_nurbs_equals(&hyperbola, range, arc_map(range, true));

    let parabola = SolvedCurveGeometry::Parabola(
        ParabolaCurve::try_new(center, axis, major, 0.75).expect("parabola fixture"),
    );
    assert_nurbs_equals(&parabola, [-2.0, 3.0], |value| value);
}
