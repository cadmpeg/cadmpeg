// SPDX-License-Identifier: Apache-2.0
use super::{curve, curve_is_supported};
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point3, Vector3};
use cadmpeg_ir::transform::Transform;

#[test]
fn rejects_transform_that_step_operator_cannot_represent() {
    let anisotropic = Transform::affine([
        [2.0, 0.0, 0.0, 0.0],
        [0.0, 3.0, 0.0, 0.0],
        [0.0, 0.0, 2.0, 0.0],
    ])
    .expect("affine transform");
    let curve = CurveGeometry::Solved(SolvedCurveGeometry::Transformed {
        basis: Box::new(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        transform: anisotropic,
    });

    assert!(!curve_is_supported(&curve));
}

#[test]
fn a_transformed_composite_curve_is_an_omitted_carrier() {
    let composite = CurveGeometry::Solved(SolvedCurveGeometry::Composite {
        segments: vec![cadmpeg_ir::geometry::CompositeCurveSegment {
            curve: cadmpeg_ir::ids::CurveId::mint("step:data:curve#1").expect("curve id"),
            same_sense: true,
            transition: cadmpeg_ir::geometry::CompositeCurveTransition::Continuous,
        }]
        .try_into()
        .expect("nonempty composite segments"),
        self_intersect: None,
    });
    let transformed = CurveGeometry::Solved(SolvedCurveGeometry::Transformed {
        basis: Box::new(composite.solved().expect("solved carrier").clone()),
        transform: Transform::identity(),
    });

    let mut emitter = crate::writer::Emitter::new();
    assert!(curve(&mut emitter, transformed.solved().expect("solved carrier")).is_none());
    assert!(!curve_is_supported(&transformed));
}

#[test]
fn numerical_audit_pcurve_keeps_large_finite_direction_and_magnitude() {
    use cadmpeg_ir::geometry::pcurve::{LinePcurve, PcurveGeometry};
    use cadmpeg_ir::math::Point2;
    let mut emitter = crate::writer::Emitter::new();
    let line = LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(1.0e200, 0.0)).unwrap();
    assert!(super::pcurve(&mut emitter, &PcurveGeometry::Line(line)).is_some());
    let lines = emitter.into_lines();
    assert!(lines
        .iter()
        .any(|line| line.contains("DIRECTION('',(1.,0.))")));
    assert!(lines
        .iter()
        .any(|line| line.contains(&format!("VECTOR('',#2,{})", crate::writer::real(1.0e200)))));
    let mut emitter = crate::writer::Emitter::new();
    let line = LinePcurve::try_new(Point2::new(0.0, 0.0), Point2::new(f64::MAX, f64::MAX)).unwrap();
    assert!(super::pcurve(&mut emitter, &PcurveGeometry::Line(line)).is_none());
}
