// SPDX-License-Identifier: Apache-2.0

use crate::eval::admission::EvaluationAdmission;
use crate::eval::{pcurve_tangent, EvaluationFailure};
use crate::geometry::pcurve::{
    LinePcurve, OffsetPcurve, ParabolaPcurve, PcurveGeometry, PlacedPcurve, TrimmedPcurve,
};
use crate::math::Point2;
use crate::transform::Transform2;

fn placed_trimmed(basis: PcurveGeometry) -> PcurveGeometry {
    PcurveGeometry::Transformed(
        PlacedPcurve::try_new(
            Box::new(PcurveGeometry::Trimmed(
                TrimmedPcurve::try_new([0.0, 1.0], true, Box::new(basis)).unwrap(),
            )),
            Transform2::identity(),
        )
        .unwrap(),
    )
}

fn nested_offset() -> PcurveGeometry {
    let basis = PcurveGeometry::Line(
        LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(0.0, 1.0)).unwrap(),
    );
    let inner = PcurveGeometry::Offset(OffsetPcurve::try_new(1.0, Box::new(basis)).unwrap());
    PcurveGeometry::Offset(OffsetPcurve::try_new(1.0, Box::new(placed_trimmed(inner))).unwrap())
}

#[test]
fn placed_trimmed_offset_preserves_unstated_acceleration() {
    let curve = nested_offset();
    assert_eq!(
        pcurve_tangent(EvaluationAdmission::Standard, &curve, 0.0),
        Err(EvaluationFailure::NoValue)
    );
    assert_eq!(
        crate::eval::decode::pcurve_uv(EvaluationAdmission::Standard, &curve, 0.0)
            .map(crate::units::FinitePoint2::get),
        Ok(Point2::new(-2.0, 0.0)),
    );
}

#[test]
fn placed_trimmed_offset_preserves_nonfinite_declared_acceleration() {
    // At zero the parabola has point (0,0) and tangent (0,1). Its axial
    // second derivative is 1/(2*focal), beyond the finite f64 range.
    let basis = PcurveGeometry::Parabola(
        ParabolaPcurve::try_new(
            Point2::new(0.0, 0.0),
            Point2::new(1.0, 0.0),
            Point2::new(0.0, 1.0),
            f64::from_bits(1),
        )
        .unwrap(),
    );
    let curve = PcurveGeometry::Offset(
        OffsetPcurve::try_new(1.0, Box::new(placed_trimmed(basis))).unwrap(),
    );
    let Err(EvaluationFailure::NonFinite(tangent)) =
        pcurve_tangent(EvaluationAdmission::Standard, &curve, 0.0)
    else {
        panic!("the declared acceleration cannot form a finite offset tangent");
    };
    assert!(tangent.u.is_nan() && tangent.v.is_nan());
    assert_eq!(
        crate::eval::decode::pcurve_uv(EvaluationAdmission::Standard, &curve, 0.0)
            .map(crate::units::FinitePoint2::get),
        Ok(Point2::new(-1.0, 0.0)),
    );
}
