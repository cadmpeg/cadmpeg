// SPDX-License-Identifier: Apache-2.0
use super::bilinear_surface;
use crate::eval::{
    nurbs_surface_parameter_segment_chord_bound, nurbs_surface_point,
    rational_patch_parameter_segment, RationalBezierSurfacePatch,
};
use crate::math::{Point2, Point3};
use crate::topology::IncreasingParameterInterval;
use crate::units::FinitePoint2;

#[test]
fn rational_patch_rejects_out_of_domain_endpoint_instead_of_moving_it() {
    let patch = RationalBezierSurfacePatch {
        u_domain: IncreasingParameterInterval::new([0.0, 1.0]).unwrap(),
        v_domain: IncreasingParameterInterval::new([0.0, 1.0]).unwrap(),
        u_degree: 1,
        v_degree: 1,
        controls: [[0.0, 0.0, 0.0, 1.0]; 4].to_vec(),
    };
    let start = FinitePoint2::new(Point2::new(-0.5, 0.0)).unwrap();
    let end = FinitePoint2::new(Point2::new(0.5, 1.0)).unwrap();
    assert!(rational_patch_parameter_segment(&patch, start, end).is_none());
}

#[test]
fn nurbs_surface_parameter_segment_bound_contains_curved_diagonal() {
    let mut surface = bilinear_surface();
    let mut visited = 0;
    surface
        .edit_control_points(|point| {
            if visited == 3 {
                point.z = 1.0;
            }
            visited += 1;
            Ok(())
        })
        .unwrap();
    let parameters = [Point2::new(0.0, 0.0), Point2::new(1.0, 1.0)];
    let chord = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 1.0, 1.0)];
    let bound = nurbs_surface_parameter_segment_chord_bound(&surface, parameters, chord)
        .expect("rational Bézier residual bound");

    assert!(bound >= 1.0 / 3.0);
    assert!(bound < 1.0 / 3.0 + 1.0e-12);
    let reverse_bound = nurbs_surface_parameter_segment_chord_bound(
        &surface,
        [parameters[1], parameters[0]],
        [chord[1], chord[0]],
    )
    .expect("reversed rational Bézier residual bound");
    assert!((reverse_bound - bound).abs() < 1.0e-12);
    for index in 0..=100 {
        let parameter = f64::from(index) / 100.0;
        let point = nurbs_surface_point(&surface, parameter, parameter).expect("surface point");
        let target = Point3::new(parameter, parameter, parameter);
        let distance = (point.x - target.x)
            .hypot(point.y - target.y)
            .hypot(point.z - target.z);
        assert!(distance <= bound);
    }
}
