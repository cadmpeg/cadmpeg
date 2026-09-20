// SPDX-License-Identifier: Apache-2.0
use crate::math::Point3;
use crate::transform::Transform;

#[test]
fn numerical_audit_inverse_point_uses_scale_safe_affine_inverse() {
    let scale = 1.0e110;
    let matrix = Transform::affine([
        [scale, 0.0, 0.0, 0.0],
        [0.0, scale, 0.0, 0.0],
        [0.0, 0.0, scale, 0.0],
    ])
    .unwrap();
    let (point, tolerance) =
        super::super::inverse_affine_point(matrix, Point3::new(scale, scale, scale)).unwrap();
    for coordinate in [point.x, point.y, point.z] {
        assert!((coordinate - 1.0).abs() <= 8.0 * f64::EPSILON);
    }
    assert!((tolerance / (3.0_f64.sqrt() / scale) - 1.0).abs() <= 8.0 * f64::EPSILON);
}

fn bilinear_surface(weights: Vec<Vec<f64>>, x: [f64; 2]) -> crate::geometry::nurbs::NurbsSurface {
    use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    let axis = NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false);
    NurbsSurface::from_lanes(
        axis.clone(),
        axis,
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(x[0], 0.0, 0.0), Point3::new(x[0], 1.0, 0.0)],
                vec![Point3::new(x[1], 0.0, 0.0), Point3::new(x[1], 1.0, 0.0)],
            ],
            Some(weights),
        ),
        false,
    )
    .unwrap()
}

#[test]
fn numerical_audit_rational_points_and_derivatives_ignore_common_weight_scale() {
    use super::super::*;
    for weight in [1.0, -1.0, 1.0e200, 1.0e308, 1.0e-200, f64::from_bits(1)] {
        let poles = [Point3::new(2.0, 0.0, 0.0), Point3::new(4.0, 0.0, 0.0)];
        let knots = [0.0, 0.0, 1.0, 1.0];
        let weights = [weight; 2];
        assert_eq!(
            nurbs_curve_point(1, &knots, &poles, Some(&weights), 0.5),
            Some(Point3::new(3.0, 0.0, 0.0))
        );
        assert_eq!(
            nurbs_curve_tangent(1, &knots, &poles, Some(&weights), 0.5),
            Some(Vector3::new(2.0, 0.0, 0.0))
        );
        assert_eq!(
            nurbs_curve_second_derivative(1, &knots, &poles, Some(&weights), 0.5),
            Some(Vector3::new(0.0, 0.0, 0.0))
        );
        let surface = bilinear_surface(vec![vec![weight; 2]; 2], [2.0, 4.0]);
        let partials = nurbs_surface_second_partials(&surface, 0.5, 0.5).unwrap();
        assert_eq!(partials.point, Point3::new(3.0, 0.5, 0.0));
        assert_eq!(partials.du, Vector3::new(2.0, 0.0, 0.0));
        assert_eq!(partials.dv, Vector3::new(0.0, 1.0, 0.0));
        assert_eq!(partials.duu, Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(partials.duv, Vector3::new(0.0, 0.0, 0.0));
        assert_eq!(partials.dvv, Vector3::new(0.0, 0.0, 0.0));
        let curve = nurbs_surface_isocurve(&surface, SurfaceParameterAxis::U, 0.5).unwrap();
        assert_eq!(
            curve.control_points(),
            [Point3::new(3.0, 0.0, 0.0), Point3::new(3.0, 1.0, 0.0)]
        );
    }
}

#[test]
fn numerical_audit_isocurves_keep_mixed_magnitude_weights_and_contributions() {
    use super::super::*;
    let surface = bilinear_surface(vec![vec![1.0e308, 1.0e-308]; 2], [2.0, 4.0]);
    let curve = nurbs_surface_isocurve(&surface, SurfaceParameterAxis::U, 0.5).unwrap();
    assert_eq!(
        curve.control_points(),
        [Point3::new(3.0, 0.0, 0.0), Point3::new(3.0, 1.0, 0.0)]
    );
    assert_eq!(curve.weights(), Some(vec![1.0e308, 1.0e-308]));
    let surface = bilinear_surface(vec![vec![1.0e308; 2], vec![1.0e-308; 2]], [0.0, 1.0e308]);
    let curve = nurbs_surface_isocurve(&surface, SurfaceParameterAxis::U, 0.5).unwrap();
    for point in curve.control_points() {
        assert!((point.x / 1.0e-308 - 1.0).abs() <= 8.0 * f64::EPSILON);
    }
}

#[test]
fn numerical_audit_tiny_knot_spans_and_wide_periodic_offsets_stay_finite() {
    use super::super::*;
    let tiny = 1.0e-310;
    let parameter = tiny * 0.5;
    let point = nurbs_curve_point(
        1,
        &[0.0, 0.0, tiny, tiny],
        &[Point3::new(2.0, 0.0, 0.0), Point3::new(4.0, 0.0, 0.0)],
        None,
        parameter,
    )
    .unwrap();
    // The subnormal parameter can round away from the mathematical midpoint.
    let expected = 2.0 + 2.0 * (parameter / tiny);
    assert!((point.x - expected).abs() <= 8.0 * f64::EPSILON * expected);
    assert_eq!((point.y, point.z), (0.0, 0.0));
    let knots = [-1.0e308, -1.0e308, -9.0e307, -9.0e307];
    let wrapped = periodic_parameter(&knots, 1, 2, true, 1.0e308).unwrap();
    assert!(wrapped.is_finite() && (knots[1]..=knots[2]).contains(&wrapped));
}
