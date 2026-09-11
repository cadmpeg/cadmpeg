// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn rejects_transform_that_step_operator_cannot_represent() {
    let anisotropic = Transform::from_rows([
        [2.0, 0.0, 0.0, 0.0],
        [0.0, 3.0, 0.0, 0.0],
        [0.0, 0.0, 2.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
    .expect("affine transform");
    let curve = CurveGeometry::Solved(SolvedCurveGeometry::Transformed {
        basis: Box::new(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::LineCurve::try_new(
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
