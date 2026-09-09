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
    let curve = CurveGeometry::Transformed {
        basis: Box::new(CurveGeometry::Line(
            cadmpeg_ir::geometry::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
        transform: anisotropic,
    };

    assert!(!curve_is_supported(&curve));
}
