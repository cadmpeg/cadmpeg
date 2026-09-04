use super::super::*;
use super::*;

#[test]
fn periodic_nurbs_parameters_preserve_phase_and_wrap_for_evaluation() {
    let nurbs = crate::geometry::NurbsCurve::new(
        1,
        vec![0.0, 0.0, 1.0, 2.0, 2.0],
        vec![
            Point3::new(0.0, 0.0, 0.0),
            Point3::new(1.0, 0.0, 0.0),
            Point3::new(0.0, 0.0, 0.0),
        ],
        None,
        true,
    )
    .unwrap();
    let geometry = CurveGeometry::Nurbs(nurbs.clone());
    assert_eq!(
        crate::eval::curve_point(&geometry, 0.5),
        crate::eval::curve_point(&geometry, 2.5)
    );

    let mut ir = unit_cube();
    let curve_id = ir.model.edges[0].curve.clone().unwrap();
    ir.model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .unwrap()
        .geometry = geometry;
    ir.model.edges[0].param_range = Some([0.5, 2.5]);
    assert!(!validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ParameterDomain));

    ir.model.edges[0].param_range = Some([0.5, 2.500_001]);
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ParameterDomain));

    let CurveGeometry::Nurbs(nurbs) = &mut ir
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .unwrap()
        .geometry
    else {
        unreachable!()
    };
    nurbs.set_periodic(false);
    ir.model.edges[0].param_range = Some([0.5, 2.5]);
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ParameterDomain));
}

#[test]
fn rational_quadratic_arc_evaluates_on_the_circle() {
    // Quarter circle of radius 5 as a rational quadratic Bezier.
    let weight = 0.5_f64.sqrt();
    let point = crate::eval::nurbs_curve_point(
        2,
        &[0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        &[
            Point3::new(5.0, 0.0, 0.0),
            Point3::new(5.0, 5.0, 0.0),
            Point3::new(0.0, 5.0, 0.0),
        ],
        Some(&[1.0, weight, 1.0]),
        0.5,
    )
    .unwrap();
    let radius = (point.x * point.x + point.y * point.y).sqrt();
    assert!((radius - 5.0).abs() < 1.0e-12, "mid-span radius {radius}");
}

#[test]
fn rational_pcurve_membership_finds_interior_points_without_sampling() {
    use crate::math::Point2;

    let weight = 0.5_f64.sqrt();
    let knots = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0];
    let controls = [
        Point2::new(5.0, 0.0),
        Point2::new(5.0, 5.0),
        Point2::new(0.0, 5.0),
    ];
    let weights = [1.0, weight, 1.0];
    let interior =
        crate::eval::nurbs_pcurve_uv(2, &knots, &controls, Some(&weights), 0.375).unwrap();
    assert_eq!(
        crate::eval::nurbs_pcurve_contains_point(
            2,
            &knots,
            &controls,
            Some(&weights),
            interior,
            1.0e-9,
        ),
        Some(true)
    );
    assert_eq!(
        crate::eval::nurbs_pcurve_contains_point(
            2,
            &knots,
            &controls,
            Some(&weights),
            Point2::new(4.0, 4.0),
            1.0e-6,
        ),
        Some(false)
    );
}

#[test]
fn analytic_parabola_and_hyperbola_use_step_parameterization() {
    let axis = Vector3::new(0.0, 0.0, 1.0);
    let major = Vector3::new(1.0, 0.0, 0.0);
    let parabola = CurveGeometry::Parabola {
        vertex: Point3::new(0.0, 0.0, 0.0),
        axis,
        major_direction: major,
        focal_distance: 2.0,
    };
    assert_eq!(
        crate::eval::curve_point(&parabola, 1.5),
        Some(Point3::new(4.5, 6.0, 0.0))
    );

    let hyperbola = CurveGeometry::Hyperbola {
        center: Point3::new(1.0, 2.0, 3.0),
        axis,
        major_direction: major,
        major_radius: 2.0,
        minor_radius: 3.0,
    };
    let point = crate::eval::curve_point(&hyperbola, 0.5).unwrap();
    assert_eq!(point.x, 1.0 + 2.0 * 0.5_f64.cosh());
    assert_eq!(point.y, 2.0 + 3.0 * 0.5_f64.sinh());
    assert_eq!(point.z, 3.0);
}

#[test]
fn transformed_carriers_preserve_basis_parameters() {
    let transform = crate::transform::Transform {
        rows: [
            [-2.0, 0.0, 0.0, 4.0],
            [0.0, 2.0, 0.0, 5.0],
            [0.0, 0.0, 2.0, 6.0],
            [0.0, 0.0, 0.0, 1.0],
        ],
    };
    let curve = CurveGeometry::Transformed {
        basis: Box::new(CurveGeometry::Line {
            origin: Point3::new(1.0, 0.0, 0.0),
            direction: Vector3::new(1.0, 0.0, 0.0),
        }),
        transform,
    };
    assert_eq!(
        crate::eval::curve_point(&curve, 3.0),
        Some(Point3::new(-4.0, 5.0, 6.0))
    );

    let surface = SurfaceGeometry::Transformed {
        basis: Box::new(SurfaceGeometry::Plane {
            origin: Point3::new(0.0, 0.0, 0.0),
            normal: Vector3::new(0.0, 0.0, 1.0),
            u_axis: Vector3::new(1.0, 0.0, 0.0),
        }),
        transform,
    };
    assert_eq!(
        crate::eval::surface_point(&surface, 2.0, 3.0),
        Some(Point3::new(0.0, 11.0, 6.0))
    );
}

#[test]
fn polyline_carriers_evaluate_in_both_parameter_directions() {
    let increasing = CurveGeometry::Polyline {
        points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
        parameters: Some(vec![1.0, 3.0]),
        chordal_deflection: 0.01,
    };
    assert_eq!(
        crate::eval::curve_point(&increasing, 2.0),
        Some(Point3::new(1.0, 0.0, 0.0))
    );

    let decreasing = CurveGeometry::Polyline {
        points: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)],
        parameters: Some(vec![3.0, 1.0]),
        chordal_deflection: 0.01,
    };
    assert_eq!(
        crate::eval::curve_point(&decreasing, 2.5),
        Some(Point3::new(0.5, 0.0, 0.0))
    );
}
