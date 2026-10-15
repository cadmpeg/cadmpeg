// SPDX-License-Identifier: Apache-2.0

use crate::geometry::nurbs::{BsplineSurface, NurbsError};
use crate::math::Point3;

fn points() -> Vec<Vec<Point3>> {
    vec![
        vec![Point3::new(1.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
        vec![Point3::new(3.0, 0.0, 0.0), Point3::new(4.0, 0.0, 0.0)],
    ]
}

#[test]
fn bspline_constructor_preserves_axis_shape_and_scalar_error_precedence() {
    let knots = vec![0.0, 0.0, 1.0, 1.0];
    let mut invalid = points();
    invalid[1].pop();
    invalid[0][0].x = f64::NAN;
    assert_eq!(
        BsplineSurface::new(2, 1, knots.clone(), knots.clone(), invalid.clone()),
        Err(NurbsError::Structure(
            "control_points u count must exceed degree 2, found 2".into()
        ))
    );
    assert_eq!(
        BsplineSurface::new(1, 1, knots.clone(), knots.clone(), invalid.clone()),
        Err(NurbsError::Structure(
            "control_points row must contain 2 values, found 1".into()
        ))
    );
    invalid[1].push(Point3::new(4.0, 0.0, 0.0));
    assert_eq!(
        BsplineSurface::new(1, 1, knots.clone(), knots, invalid),
        Err(NurbsError::Structure(
            "control_points contains a non-finite point".into()
        ))
    );
}

#[test]
fn bspline_constructor_keeps_poles_moves_knots_and_preserves_wire() {
    let knots = vec![0.0, 0.0, 1.0, 1.0];
    let storage = knots.as_ptr();
    let surface = BsplineSurface::new(1, 1, knots, vec![0.0, 0.0, 1.0, 1.0], points())
        .expect("valid surface");
    assert_eq!(surface.u_knots.as_ptr(), storage);
    assert_eq!(
        surface
            .control_points
            .iter()
            .map(|row| row.iter().map(|point| point.get()).collect::<Vec<_>>())
            .collect::<Vec<_>>(),
        points()
    );
    let wire = serde_json::json!({
        "u_degree": 1,
        "v_degree": 1,
        "u_knots": [0.0, 0.0, 1.0, 1.0],
        "v_knots": [0.0, 0.0, 1.0, 1.0],
        "control_points": [
            [{"x": 1.0, "y": 0.0, "z": 0.0}, {"x": 2.0, "y": 0.0, "z": 0.0}],
            [{"x": 3.0, "y": 0.0, "z": 0.0}, {"x": 4.0, "y": 0.0, "z": 0.0}]
        ]
    });
    assert_eq!(serde_json::to_value(&surface).expect("wire"), wire);
    assert_eq!(
        serde_json::from_value::<BsplineSurface>(wire).expect("context-free reconstruction"),
        surface
    );
}
