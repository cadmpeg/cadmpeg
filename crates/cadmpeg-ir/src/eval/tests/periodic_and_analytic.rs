use cadmpeg_test_support::edit;

use crate::examples::unit_cube;
use crate::geometry::sampled::PolylineSamples;
use crate::geometry::sampled::PolylineVertex;
use crate::geometry::CurveGeometry;
use crate::geometry::SolvedCurveGeometry;
use crate::geometry::SolvedSurfaceGeometry;
use crate::geometry::SurfaceGeometry;
use crate::math::Point3;
use crate::math::Vector3;
use crate::report::check::Check;
use crate::validate::validate_neutral;

#[test]
fn periodic_nurbs_parameters_preserve_phase_and_wrap_for_evaluation() {
    let nurbs = crate::geometry::nurbs::NurbsCurve::from_lanes(
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
    let geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs.clone()));
    assert_eq!(
        crate::eval::curve_point(&geometry, 0.5),
        crate::eval::curve_point(&geometry, 2.5)
    );

    let mut ir = unit_cube().expect("valid unit cube fixture");
    let curve_id = ir.model.edges[0].curve().cloned().unwrap();
    ir.model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .unwrap()
        .geometry = geometry;
    ir.model.edges[0].carrier =
        crate::topology::EdgeCarrier::new(ir.model.edges[0].curve().cloned(), Some([0.5, 2.5]))
            .unwrap();
    assert!(!validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ParameterDomain));

    ir.model.edges[0].carrier = crate::topology::EdgeCarrier::new(
        ir.model.edges[0].curve().cloned(),
        Some([0.5, 2.500_001]),
    )
    .unwrap();
    assert!(validate_neutral(&ir, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.check == Check::ParameterDomain));

    let CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) = &mut ir
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .unwrap()
        .geometry
    else {
        unreachable!()
    };
    {
        let replacement = false;
        edit::replace(nurbs, |previous| {
            crate::geometry::nurbs::NurbsCurve::new(
                previous.degree(),
                previous.knots().to_vec(),
                previous.pole_rows().clone(),
                replacement,
            )
        })
        .unwrap();
    };
    ir.model.edges[0].carrier =
        crate::topology::EdgeCarrier::new(ir.model.edges[0].curve().cloned(), Some([0.5, 2.5]))
            .unwrap();
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
            *interior.as_raw(),
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
    let parabola = CurveGeometry::Solved(SolvedCurveGeometry::Parabola(
        crate::geometry::analytic::ParabolaCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            axis,
            major,
            2.0,
        )
        .unwrap(),
    ));
    assert_eq!(
        crate::eval::curve_point(&parabola, 1.5).map(crate::features::FinitePoint3::get),
        Some(Point3::new(4.5, 6.0, 0.0))
    );

    let hyperbola = CurveGeometry::Solved(SolvedCurveGeometry::Hyperbola(
        crate::geometry::analytic::HyperbolaCurve::try_new(
            Point3::new(1.0, 2.0, 3.0),
            axis,
            major,
            2.0,
            3.0,
        )
        .unwrap(),
    ));
    let point = crate::eval::curve_point(&hyperbola, 0.5).unwrap();
    assert_eq!(point.x, 1.0 + 2.0 * 0.5_f64.cosh());
    assert_eq!(point.y, 2.0 + 3.0 * 0.5_f64.sinh());
    assert_eq!(point.z, 3.0);
}

#[test]
fn transformed_carriers_preserve_basis_parameters() {
    let transform = crate::transform::Transform::affine([
        [-2.0, 0.0, 0.0, 4.0],
        [0.0, 2.0, 0.0, 5.0],
        [0.0, 0.0, 2.0, 6.0],
    ])
    .expect("affine transform");
    let curve = CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
        crate::geometry::PlacedCurve::try_new(
            Box::new(SolvedCurveGeometry::Line(
                crate::geometry::analytic::LineCurve::try_new(
                    Point3::new(1.0, 0.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            transform,
        )
        .expect("placed curve"),
    ));
    assert_eq!(
        crate::eval::curve_point(&curve, 3.0).map(crate::features::FinitePoint3::get),
        Some(Point3::new(-4.0, 5.0, 6.0))
    );

    let surface = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Transformed(
        crate::geometry::PlacedSurface::try_new(
            Box::new(SolvedSurfaceGeometry::Plane(
                crate::geometry::analytic::PlaneSurface::try_new(
                    Point3::new(0.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap(),
            )),
            transform,
        )
        .expect("placed surface"),
    ));
    assert_eq!(
        crate::eval::surface_point(&surface, 2.0, 3.0).map(crate::features::FinitePoint3::get),
        Some(Point3::new(0.0, 11.0, 6.0))
    );
}

#[test]
fn polyline_carriers_evaluate_in_both_parameter_directions() {
    let increasing = CurveGeometry::Solved(SolvedCurveGeometry::Polyline(
        crate::geometry::sampled::PolylineCurve::new(
            PolylineSamples::Parameterized {
                vertices: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)]
                    .into_iter()
                    .zip(vec![1.0, 3.0])
                    .map(|(point, parameter)| PolylineVertex { parameter, point })
                    .collect::<Vec<_>>()
                    .try_into()
                    .expect("nonempty polyline fixture"),
            },
            0.01,
        )
        .unwrap(),
    ));
    assert_eq!(
        crate::eval::curve_point(&increasing, 2.0).map(crate::features::FinitePoint3::get),
        Some(Point3::new(1.0, 0.0, 0.0))
    );

    let decreasing = CurveGeometry::Solved(SolvedCurveGeometry::Polyline(
        crate::geometry::sampled::PolylineCurve::new(
            PolylineSamples::Parameterized {
                vertices: vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 0.0, 0.0)]
                    .into_iter()
                    .zip(vec![3.0, 1.0])
                    .map(|(point, parameter)| PolylineVertex { parameter, point })
                    .collect::<Vec<_>>()
                    .try_into()
                    .expect("nonempty polyline fixture"),
            },
            0.01,
        )
        .unwrap(),
    ));
    assert_eq!(
        crate::eval::curve_point(&decreasing, 2.5).map(crate::features::FinitePoint3::get),
        Some(Point3::new(0.5, 0.0, 0.0))
    );
}

#[test]
fn analytic_surface_points_are_absent_when_they_overflow() {
    use crate::eval::{surface_point, surface_point_with_budget};

    let plane = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        crate::geometry::analytic::PlaneSurface::try_new(
            Point3::new(f64::MAX, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    ));
    let budget = cadmpeg_core::decode::WorkBudget::new(64);
    // A finite origin and a finite in-plane displacement sum past the finite
    // range on every surface route.
    assert!(surface_point(&plane, f64::MAX, 0.0).is_none());
    assert!(surface_point_with_budget(&plane, f64::MAX, 0.0, &budget).is_none());
    assert_eq!(
        surface_point(&plane, -f64::MAX, 0.0).map(crate::features::FinitePoint3::get),
        Some(Point3::new(0.0, 0.0, 0.0))
    );
    assert_eq!(
        surface_point_with_budget(&plane, -f64::MAX, 0.0, &budget)
            .map(crate::features::FinitePoint3::get),
        Some(Point3::new(0.0, 0.0, 0.0))
    );
}

#[test]
fn conic_arms_refuse_points_and_derivatives_that_overflow() {
    use crate::eval::{curve_point, curve_second_derivative, curve_tangent};
    use crate::geometry::analytic::{CircleCurve, EllipseCurve, HyperbolaCurve, ParabolaCurve};

    let axis = Vector3::new(0.0, 0.0, 1.0);
    let reference = Vector3::new(1.0, 0.0, 0.0);
    let edge = Point3::new(f64::MAX, 0.0, 0.0);
    let origin = Point3::new(0.0, 0.0, 0.0);
    let solved = CurveGeometry::Solved;

    // A finite center and a finite radial term sum past the finite range.
    let circle = solved(SolvedCurveGeometry::Circle(
        CircleCurve::try_new(edge, axis, reference, 1.0e308).unwrap(),
    ));
    assert!(curve_point(&circle, 0.0).is_none());
    assert!(curve_point(&circle, std::f64::consts::PI).is_some());
    let ellipse = solved(SolvedCurveGeometry::Ellipse(
        EllipseCurve::try_new(edge, axis, reference, 1.0e308, 1.0).unwrap(),
    ));
    assert!(curve_point(&ellipse, 0.0).is_none());
    assert!(curve_point(&ellipse, std::f64::consts::PI).is_some());
    let parabola = solved(SolvedCurveGeometry::Parabola(
        ParabolaCurve::try_new(edge, axis, reference, 1.0).unwrap(),
    ));
    assert!(curve_point(&parabola, 1.0e154).is_none());
    assert!(curve_point(&parabola, 0.0).is_some());
    let hyperbola = solved(SolvedCurveGeometry::Hyperbola(
        HyperbolaCurve::try_new(edge, axis, reference, 1.0e308, 1.0).unwrap(),
    ));
    assert!(curve_point(&hyperbola, 0.0).is_none());
    let centered = solved(SolvedCurveGeometry::Hyperbola(
        HyperbolaCurve::try_new(origin, axis, reference, 1.0e308, 1.0).unwrap(),
    ));
    assert!(curve_point(&centered, 0.0).is_some());

    // A frame direction admitted within the unit tolerance but longer than
    // one carries a largest-radius derivative term past the finite range.
    let stretched = Vector3::new(1.0 + 5.0e-10, 0.0, 0.0);
    let circle = solved(SolvedCurveGeometry::Circle(
        CircleCurve::try_new(origin, axis, stretched, f64::MAX).unwrap(),
    ));
    assert!(curve_tangent(&circle, 0.0).is_none());
    assert!(curve_second_derivative(&circle, 0.0).is_none());
    assert!(curve_tangent(&circle, std::f64::consts::FRAC_PI_4).is_some());
    assert!(curve_second_derivative(&circle, std::f64::consts::FRAC_PI_4).is_some());
    let ellipse = solved(SolvedCurveGeometry::Ellipse(
        EllipseCurve::try_new(origin, axis, stretched, f64::MAX, 1.0).unwrap(),
    ));
    assert!(curve_tangent(&ellipse, std::f64::consts::FRAC_PI_2).is_none());
    assert!(curve_second_derivative(&ellipse, 0.0).is_none());
    assert!(curve_tangent(&ellipse, std::f64::consts::FRAC_PI_4).is_some());
    let parabola = solved(SolvedCurveGeometry::Parabola(
        ParabolaCurve::try_new(origin, axis, stretched, 0.5 * f64::MAX).unwrap(),
    ));
    assert!(curve_tangent(&parabola, 1.0).is_none());
    assert!(curve_second_derivative(&parabola, 0.0).is_none());
    let parabola = solved(SolvedCurveGeometry::Parabola(
        ParabolaCurve::try_new(origin, axis, reference, 0.5 * f64::MAX).unwrap(),
    ));
    assert!(curve_tangent(&parabola, 0.5).is_some());
    assert!(curve_second_derivative(&parabola, 0.0).is_some());
    let hyperbola = solved(SolvedCurveGeometry::Hyperbola(
        HyperbolaCurve::try_new(origin, axis, stretched, 1.0, f64::MAX).unwrap(),
    ));
    assert!(curve_tangent(&hyperbola, 0.0).is_none());
    let hyperbola = solved(SolvedCurveGeometry::Hyperbola(
        HyperbolaCurve::try_new(origin, axis, stretched, f64::MAX, 1.0).unwrap(),
    ));
    assert!(curve_second_derivative(&hyperbola, 0.0).is_none());
    let hyperbola = solved(SolvedCurveGeometry::Hyperbola(
        HyperbolaCurve::try_new(origin, axis, reference, 1.0, f64::MAX).unwrap(),
    ));
    assert!(curve_tangent(&hyperbola, 0.0).is_some());
    assert!(curve_second_derivative(&hyperbola, 0.0).is_some());
}

#[test]
fn curve_evaluators_hand_back_admitted_values_and_refuse_overflow() {
    use crate::features::{FinitePoint3, FiniteVector3};

    let line = |origin: Point3| {
        SolvedCurveGeometry::Line(
            crate::geometry::analytic::LineCurve::try_new(origin, Vector3::new(1.0, 0.0, 0.0))
                .unwrap(),
        )
    };
    let near_edge = CurveGeometry::Solved(line(Point3::new(f64::MAX, 0.0, 0.0)));
    assert_eq!(
        crate::eval::curve_point(&near_edge, 0.0),
        FinitePoint3::new(Point3::new(f64::MAX, 0.0, 0.0))
    );
    assert_eq!(crate::eval::curve_point(&near_edge, f64::MAX), None);
    assert_eq!(
        crate::eval::curve_tangent(&near_edge, 0.0),
        FiniteVector3::new(Vector3::new(1.0, 0.0, 0.0))
    );

    let stretch = crate::transform::Transform::affine([
        [f64::MAX, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .expect("affine transform");
    let placed = CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
        crate::geometry::PlacedCurve::try_new(Box::new(line(Point3::new(0.0, 1.0, 0.0))), stretch)
            .expect("placed curve"),
    ));
    let budget = cadmpeg_core::decode::WorkBudget::new(64);
    assert_eq!(
        crate::eval::curve_point_with_budget(&placed, 0.0, &budget),
        FinitePoint3::new(Point3::new(0.0, 1.0, 0.0))
    );
    assert_eq!(
        crate::eval::curve_tangent_with_budget(&placed, 0.0, &budget),
        FiniteVector3::new(Vector3::new(f64::MAX, 0.0, 0.0))
    );
    assert_eq!(
        crate::eval::curve_point_with_budget(&placed, 2.0, &budget),
        None
    );

    let plane = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
        crate::geometry::analytic::PlaneSurface::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
    ));
    assert_eq!(
        crate::eval::analytic_surface_parameters(&plane, Point3::new(2.0, -3.0, 5.0)),
        crate::units::FinitePoint2::new(crate::math::Point2::new(2.0, -3.0))
    );
    assert_eq!(
        crate::eval::analytic_surface_parameters(&plane, Point3::new(f64::MAX, 0.0, 0.0)),
        crate::units::FinitePoint2::new(crate::math::Point2::new(f64::MAX, 0.0))
    );
}
