// SPDX-License-Identifier: Apache-2.0
use super::*;

#[test]
fn rejects_transform_that_step_operator_cannot_represent() {
    let anisotropic = Transform {
        rows: [
            [2.0, 0.0, 0.0, 0.0],
            [0.0, 3.0, 0.0, 0.0],
            [0.0, 0.0, 2.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };
    let curve = CurveGeometry::Transformed {
        basis: Box::new(CurveGeometry::Line {
            origin: Point3::new(0.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        }),
        transform: anisotropic,
    };

    assert!(!curve_is_supported(&curve));
}

#[test]
fn rejects_nurbs_surface_with_incomplete_control_grid() {
    let geometry = SurfaceGeometry::Nurbs(NurbsSurface {
        u_degree: 1,
        v_degree: 1,
        u_knots: vec![0.0, 0.0, 1.0, 1.0],
        v_knots: vec![0.0, 0.0, 1.0, 1.0],
        u_count: 2,
        v_count: 2,
        control_points: vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(0.0, 1.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
        ],
        weights: None,
        normal_reversed: false,
        u_periodic: false,
        v_periodic: false,
    });

    assert!(!surface_is_supported(&geometry));
    let mut emitter = Emitter::new();
    assert!(surface(&mut emitter, &geometry).is_none());
    assert!(emitter.into_lines().is_empty());
}
