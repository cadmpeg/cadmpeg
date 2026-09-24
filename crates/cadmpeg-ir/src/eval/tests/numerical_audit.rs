// SPDX-License-Identifier: Apache-2.0
use crate::math::Point3;
use crate::transform::Transform;

const EPS_NURBS_RATIONAL_DERIVATIVE: f64 = 1e-12;

#[test]
fn knot_span_refuses_oversized_degree_and_count_without_overflow() {
    let knots = [0.0, 1.0];
    assert_eq!(super::super::bspline_span(&knots, usize::MAX, 1, 0.5), None);
    assert_eq!(
        super::super::bspline_span(&knots, usize::MAX - 1, usize::MAX, 0.5),
        None
    );
}

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
    use super::super::{
        nurbs_curve_point, nurbs_curve_second_derivative, nurbs_curve_tangent,
        nurbs_surface_isocurve, nurbs_surface_second_partials, SurfaceParameterAxis,
    };
    use crate::math::Vector3;
    for weight in [1.0, -1.0, 1.0e200, 1.0e308, 1.0e-200, f64::from_bits(1)] {
        let poles = [Point3::new(2.0, 0.0, 0.0), Point3::new(4.0, 0.0, 0.0)];
        let knots = [0.0, 0.0, 1.0, 1.0];
        let weights = [weight; 2];
        assert_eq!(
            nurbs_curve_point(1, &knots, &poles, Some(&weights), 0.5)
                .map(crate::features::FinitePoint3::get),
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
    use super::super::{nurbs_surface_isocurve, SurfaceParameterAxis};
    let surface = bilinear_surface(vec![vec![1.0e308, 1.0e-308]; 2], [2.0, 4.0]);
    let curve = nurbs_surface_isocurve(&surface, SurfaceParameterAxis::U, 0.5).unwrap();
    assert_eq!(
        curve.control_points(),
        [Point3::new(3.0, 0.0, 0.0), Point3::new(3.0, 1.0, 0.0)]
    );
    assert_eq!(curve.pole_rows().weights(), Some(vec![1.0e308, 1.0e-308]));
    let surface = bilinear_surface(vec![vec![1.0e308; 2], vec![1.0e-308; 2]], [0.0, 1.0e308]);
    let curve = nurbs_surface_isocurve(&surface, SurfaceParameterAxis::U, 0.5).unwrap();
    for point in curve.control_points() {
        assert!((point.x / 1.0e-308 - 1.0).abs() <= 8.0 * f64::EPSILON);
    }
}

#[test]
fn numerical_audit_tiny_knot_spans_and_wide_periodic_offsets_stay_finite() {
    use super::super::{nurbs_curve_point, periodic_parameter};
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
    let wrapped = periodic_parameter(&knots, 1, 2, true, 1.0e308)
        .unwrap()
        .get();
    assert!(wrapped.is_finite() && (knots[1]..=knots[2]).contains(&wrapped));
}

#[test]
fn numerical_audit_affine_evaluation_keeps_cancelled_products() {
    use super::super::{curve_point, curve_tangent};
    use crate::geometry::{CurveGeometry, SolvedCurveGeometry};
    use crate::math::Vector3;
    let transform = Transform::affine([
        [1e308, -1e308, 1.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
    ])
    .unwrap();
    let curve = CurveGeometry::Solved(SolvedCurveGeometry::Transformed(
        crate::geometry::PlacedCurve::try_new(
            Box::new(SolvedCurveGeometry::Nurbs(
                crate::geometry::nurbs::NurbsCurve::from_lanes(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![Point3::new(0.0, 0.0, 0.0), Point3::new(2.0, 2.0, 3.0)],
                    None,
                    false,
                )
                .unwrap(),
            )),
            transform,
        )
        .expect("placed curve"),
    ));
    assert_eq!(
        curve_point(&curve, 1.0).map(crate::features::FinitePoint3::get),
        Some(Point3::new(3.0, 2.0, 3.0))
    );
    assert_eq!(
        curve_tangent(&curve, 1.0).map(crate::features::FiniteVector3::get),
        Some(Vector3::new(3.0, 2.0, 3.0))
    );
    for a in [1e-200, 1.0, 1e200] {
        let reflection =
            Transform::affine([[-a, 0.0, 0.0, 0.0], [0.0, a, 0.0, 0.0], [0.0, 0.0, a, 0.0]])
                .unwrap();
        assert_eq!(reflection.orientation(), Some(-1.0));
    }
}

#[test]
fn numerical_audit_rational_pcurve_preserves_finite_weighted_results() {
    use super::super::nurbs_pcurve_differential;
    use crate::math::Point2;
    let result = nurbs_pcurve_differential(
        1,
        &[0.0, 0.0, 1.0, 1.0],
        &[Point2::new(1e200, 0.0), Point2::new(2e200, 0.0)],
        Some(&[1e200, 1e200]),
        0.5,
    )
    .unwrap();
    assert!((result.point.u / 1e200 - 1.5).abs() <= 8.0 * f64::EPSILON);
    assert!((result.tangent.unwrap().u / 1e200 - 1.0).abs() <= 8.0 * f64::EPSILON);
    assert_eq!(result.acceleration, Some(Point2::new(0.0, 0.0)));
}

#[test]
fn numerical_audit_pcurve_keeps_finite_derivatives_on_a_narrow_knot_span() {
    use super::super::nurbs_pcurve_differential;
    use crate::math::Point2;

    let width = f64::from_bits(1_u64 << 44);
    let result = nurbs_pcurve_differential(
        1,
        &[0.0, 0.0, width, width],
        &[Point2::new(0.0, 0.0), Point2::new(width, 0.0)],
        None,
        width / 2.0,
    )
    .unwrap();
    assert_eq!(result.point, Point2::new(width / 2.0, 0.0));
    assert_eq!(result.tangent, Some(Point2::new(1.0, 0.0)));
    assert_eq!(result.acceleration, Some(Point2::new(0.0, 0.0)));

    let quadratic = nurbs_pcurve_differential(
        2,
        &[0.0, 0.0, 0.0, width, width, width],
        &[
            Point2::new(0.0, 0.0),
            Point2::new(width / 2.0, 0.0),
            Point2::new(width, 0.0),
        ],
        None,
        width / 2.0,
    )
    .unwrap();
    assert_eq!(quadratic.point, Point2::new(width / 2.0, 0.0));
    assert_eq!(quadratic.tangent, Some(Point2::new(1.0, 0.0)));
    assert_eq!(quadratic.acceleration, Some(Point2::new(0.0, 0.0)));
}

#[test]
fn numerical_audit_polar_derivatives_are_independent_of_radial_scale() {
    use super::super::pcurve_uv_differential;
    use crate::geometry::pcurve::PcurveGeometry;
    use crate::geometry::pcurve::{PolarHarmonicPcurve, SphericalGreatCirclePcurve};
    use crate::math::Point2;
    for radius in [1e-200, 1.0, 1e200] {
        let curve = PcurveGeometry::PolarHarmonic(
            PolarHarmonicPcurve::try_new(
                Point2::new(0.0, 0.0),
                Point2::new(radius, 0.0),
                Point2::new(0.0, radius),
                0.0,
                0.0,
                0.0,
            )
            .unwrap(),
        );
        let result = pcurve_uv_differential(&curve, 0.5).unwrap();
        assert!((result.point.u - 0.5).abs() <= 8.0 * f64::EPSILON);
        assert!((result.tangent.unwrap().u - 1.0).abs() <= 8.0 * f64::EPSILON);
        assert!(result.acceleration.unwrap().u.abs() <= 8.0 * f64::EPSILON);
    }
    let curve = PcurveGeometry::SphericalGreatCircle(
        SphericalGreatCirclePcurve::try_new(0.0, 1.0, 0.0, 1e200).unwrap(),
    );
    let result = pcurve_uv_differential(&curve, 0.5).unwrap();
    let (sin, cos) = 0.5_f64.sin_cos();
    let expected_first = -sin / (1e200 * cos * cos);
    let expected_second = -(1.0 + sin * sin) / (1e200 * cos * cos * cos);
    assert!((result.tangent.unwrap().v / expected_first - 1.0).abs() <= 16.0 * f64::EPSILON);
    assert!((result.acceleration.unwrap().v / expected_second - 1.0).abs() <= 16.0 * f64::EPSILON);
}

#[test]
fn numerical_audit_chain_rules_keep_finite_composed_derivatives() {
    use super::super::{scalar_unary_sweep_law_differential, ScalarSweepDifferential};
    for (operator, x, derivative, expected) in [
        ("LN", 1e-310, 1e-310, 1.0),
        ("COT", 1e-200, 1e-200, -1e200),
        ("CSC", 1e-200, 1e-200, -1e200),
        ("ARCSECH", 1e-310, 1e-310, -1.0),
        (
            "EXP",
            -750.0,
            1e300,
            (-375.0_f64).exp() * ((-375.0_f64).exp() * 1e300),
        ),
    ] {
        let result = scalar_unary_sweep_law_differential(
            operator,
            ScalarSweepDifferential {
                value: x,
                derivative,
            },
        )
        .unwrap();
        assert!(
            (result.derivative / expected - 1.0).abs() <= 16.0 * f64::EPSILON,
            "{operator}"
        );
    }
}

#[test]
fn numerical_audit_polyline_interpolation_spans_the_finite_range() {
    use super::super::{polyline_point, polyline_tangent};
    assert_eq!(
        polyline_point(
            &[Point3::new(-1e308, 0.0, 0.0), Point3::new(1e308, 0.0, 0.0)],
            &[0.0, 1.0],
            0.5
        ),
        Some(Point3::new(0.0, 0.0, 0.0))
    );
    let points = [Point3::new(0.0, 0.0, 0.0), Point3::new(1.0, 0.0, 0.0)];
    assert_eq!(
        polyline_point(&points, &[-1e308, 1e308], 0.0),
        Some(Point3::new(0.5, 0.0, 0.0))
    );
    let tangent = polyline_tangent(&points, &[-1e308, 1e308], 0.0).unwrap();
    assert!((tangent.x / 5e-309 - 1.0).abs() <= 8.0 * f64::EPSILON);
}

#[test]
fn audit_regression_surface_inversion_accepts_large_parameter_origins() {
    use crate::geometry::nurbs::{NurbsSurface, NurbsSurfaceAxis, NurbsSurfaceLanes};
    let axis = NurbsSurfaceAxis::new(1, vec![1e308, 1e308, 1.1e308, 1.1e308], false);
    let surface = NurbsSurface::from_lanes(
        axis.clone(),
        axis,
        NurbsSurfaceLanes::new(
            vec![
                vec![Point3::new(0., 0., 0.), Point3::new(0., 1., 0.)],
                vec![Point3::new(1., 0., 0.), Point3::new(1., 1., 0.)],
            ],
            None,
        ),
        false,
    )
    .unwrap();
    let target = Point3::new(0.5, 0.5, 0.);
    let uv = crate::eval::nurbs_surface_closest_parameter_with_budget(
        &surface,
        target,
        None,
        &cadmpeg_core::decode::WorkBudget::new(1_000_000),
    )
    .unwrap();
    let point = crate::eval::nurbs_surface_point(&surface, uv.u, uv.v).unwrap();
    assert!(point.distance(target) <= 64.0 * f64::EPSILON);
}

fn audit_rolling_ball_jet(
    radius: f64,
    end_knot: f64,
    second_derivative: f64,
) -> crate::geometry::ProceduralSurfaceDefinition {
    use crate::geometry::{
        ProceduralSurfaceDefinition, RollingBallJetDerivative, RollingBallJetSite,
        RollingBallJetStation, RollingBallJetStations,
    };
    use crate::math::Vector3;
    let zero = Vector3::new(0.0, 0.0, 0.0);
    let derivative = RollingBallJetDerivative {
        first_limit: zero,
        second_limit: zero,
        center: zero,
        angle: 0.0,
    };
    let site = RollingBallJetSite {
        first_limit: Point3::new(radius, 0.0, 0.0),
        second_limit: Point3::new(0.0, radius, 0.0),
        center: Point3::new(0.0, 0.0, 0.0),
        angle: std::f64::consts::FRAC_PI_2,
        first_derivative: derivative.clone(),
        second_derivative: RollingBallJetDerivative {
            first_limit: Vector3::new(second_derivative, 0.0, 0.0),
            ..derivative
        },
    };
    ProceduralSurfaceDefinition::RollingBallJet(
        RollingBallJetStations::try_new(
            5,
            vec![
                RollingBallJetStation {
                    knot: 0.0,
                    multiplicity: 6,
                    site: site.clone(),
                },
                RollingBallJetStation {
                    knot: end_knot,
                    multiplicity: 6,
                    site,
                },
            ],
        )
        .unwrap(),
    )
}

#[test]
fn numerical_audit_rolling_ball_keeps_small_nonzero_frame() {
    let radius = 1e-20;
    let jet = audit_rolling_ball_jet(radius, 1.0, 0.0);
    assert_eq!(
        super::super::rolling_ball_jet_point(&jet, 0.5, 0.0)
            .map(crate::features::FinitePoint3::get),
        Some(Point3::new(radius, 0.0, 0.0))
    );
    let point = super::super::rolling_ball_jet_point(&jet, 0.5, 1.0).unwrap();
    assert!(point.x.abs() <= radius * 8.0 * f64::EPSILON);
    assert_eq!(point.y, radius);
}

#[test]
fn numerical_audit_rolling_ball_keeps_endpoint_when_unused_product_overflows() {
    let jet = audit_rolling_ball_jet(1.0, 1e200, 1.0);
    assert_eq!(
        super::super::rolling_ball_jet_point(&jet, 0.0, 0.0)
            .map(crate::features::FinitePoint3::get),
        Some(Point3::new(1.0, 0.0, 0.0))
    );
}

#[test]
fn numerical_audit_small_nurbs_span_keeps_finite_curve_derivatives() {
    use crate::geometry::{nurbs::NurbsCurve, SolvedCurveGeometry};
    use crate::math::Vector3;
    let width = 1e-310;
    let line = SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, width, width],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(width, 0.0, 0.0)],
            None,
            false,
        )
        .unwrap(),
    );
    assert_eq!(
        super::super::curve_tangent_solved(&line, width / 2.0)
            .map(crate::features::FiniteVector3::get),
        Some(Vector3::new(1.0, 0.0, 0.0))
    );
    assert_eq!(
        super::super::curve_second_derivative_solved(&line, width / 2.0)
            .map(crate::features::FiniteVector3::get),
        Some(Vector3::new(0.0, 0.0, 0.0))
    );

    let width = 1e-155;
    let square = width * width;
    let quadratic = SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            2,
            vec![0.0, 0.0, 0.0, width, width, width],
            vec![
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(0.0, 0.0, 0.0),
                Point3::new(square, 0.0, 0.0),
            ],
            None,
            false,
        )
        .unwrap(),
    );
    let second = super::super::curve_second_derivative_solved(&quadratic, width / 2.0).unwrap();
    assert!(
        (second.x - 2.0).abs() <= 512.0 * f64::EPSILON,
        "second derivative {}",
        second.x
    );
    assert_eq!((second.y, second.z), (0.0, 0.0));
}

#[test]
fn numerical_audit_rational_linear_nurbs_keeps_subnormal_pole_derivatives() {
    use crate::geometry::{nurbs::NurbsCurve, SolvedCurveGeometry};
    let width = 1e-310;
    let pole = f64::from_bits(1);
    let curve = SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            1,
            vec![0.0, 0.0, width, width],
            vec![Point3::new(0.0, 0.0, 0.0), Point3::new(pole, 0.0, 0.0)],
            Some(vec![1.0, 2.0]),
            false,
        )
        .unwrap(),
    );
    let parameter = width / 2.0;
    let fraction = parameter / width;
    let base_weight = 1.0 + fraction;
    let expected_first = 2.0 * (pole / width) / (base_weight * base_weight);
    let expected_second = -4.0 * (pole / width) / width / (base_weight * base_weight * base_weight);
    let first = super::super::curve_tangent_solved(&curve, parameter).unwrap();
    let second = super::super::curve_second_derivative_solved(&curve, parameter).unwrap();
    assert!((first.x / expected_first - 1.0).abs() <= EPS_NURBS_RATIONAL_DERIVATIVE);
    assert!((second.x / expected_second - 1.0).abs() <= EPS_NURBS_RATIONAL_DERIVATIVE);
    assert_eq!((first.y, first.z, second.y, second.z), (0.0, 0.0, 0.0, 0.0));
}
