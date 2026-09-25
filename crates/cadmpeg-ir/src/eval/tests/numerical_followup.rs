// SPDX-License-Identifier: Apache-2.0
use super::super::{
    direct_curve_parameter_near_point, map_nurbs_curve_parameter, nurbs_curve_parameter_near_point,
    nurbs_curve_parameter_near_point_with_nonnegative_tolerance, nurbs_curve_speed_bound,
    nurbs_pcurve_contains_point, nurbs_surface_parameter_near_point, scalar_sweep_law_differential,
    scalar_unary_sweep_law_differential,
};
use super::law_operand;
use crate::geometry::nurbs::{NurbsSurfaceAxis, NurbsSurfaceLanes};
use crate::geometry::{
    nurbs::{NurbsCurve, NurbsSurface},
    LawExpression, SolvedCurveGeometry,
};
use crate::math::{Point2, Point3, Vector3};

#[test]
fn numerical_followup_surface_projection_is_independent_of_scale() {
    for scale in [1.0, 1e-5, 1e-100, 1e100] {
        let axis = NurbsSurfaceAxis::new(1, vec![0., 0., 1., 1.], false);
        let surface = NurbsSurface::from_lanes(
            axis.clone(),
            axis,
            NurbsSurfaceLanes::new(
                vec![
                    vec![Point3::new(0., 0., 0.), Point3::new(0., scale, 0.)],
                    vec![Point3::new(scale, 0., 0.), Point3::new(scale, scale, 0.)],
                ],
                None,
            ),
            false,
        )
        .unwrap();
        let target = Point3::new(0.3 * scale, 0.4 * scale, 0.);
        let uv = nurbs_surface_parameter_near_point(&surface, target, Some(Point2::new(0., 0.)))
            .unwrap();
        assert!((uv.u - 0.3).abs() <= 8.0 * f64::EPSILON);
        assert!((uv.v - 0.4).abs() <= 8.0 * f64::EPSILON);
    }
}

#[test]
fn numerical_followup_curve_search_rejects_a_nonzero_zero_tolerance_residual() {
    let curve = NurbsCurve::from_lanes(
        1,
        vec![0., 0., 1., 1.],
        vec![Point3::new(0., 0., 0.), Point3::new(1., 0., 0.)],
        None,
        false,
    )
    .unwrap();
    assert_eq!(
        nurbs_curve_parameter_near_point(&curve, Point3::new(0., 1e-200, 0.), 0., 0.),
        None
    );
}

#[test]
fn curve_search_admits_its_tolerance_before_the_search() {
    let curve = NurbsCurve::from_lanes(
        1,
        vec![0., 0., 1., 1.],
        vec![Point3::new(0., 0., 0.), Point3::new(1., 0., 0.)],
        None,
        false,
    )
    .unwrap();
    let point = Point3::new(0.25, 1e-3, 0.);
    for tolerance in [-1e-3, f64::NAN, f64::INFINITY] {
        assert_eq!(
            nurbs_curve_parameter_near_point(&curve, point, tolerance, 0.5),
            None
        );
    }
    let tolerance = crate::scalar::NonNegativeLength::new(2e-3).unwrap();
    let seed = crate::scalar::FiniteReal::new(0.5).unwrap();
    let parameter =
        nurbs_curve_parameter_near_point_with_nonnegative_tolerance(&curve, point, tolerance, seed);
    assert!(parameter.is_some());
    assert_eq!(
        parameter,
        nurbs_curve_parameter_near_point(&curve, point, tolerance.get(), 0.5)
    );
}

#[test]
fn numerical_followup_rational_search_retains_common_weight_scaling() {
    let poles = vec![Point3::new(0., 0., 0.), Point3::new(1., 0., 0.)];
    let points = [Point2::new(0., 0.), Point2::new(1., 0.)];
    for w in [1.0, 1e-200, 1e200, 1e308] {
        let curve = NurbsCurve::from_lanes(
            1,
            vec![0., 0., 1., 1.],
            poles.clone(),
            Some(vec![w, w]),
            false,
        )
        .unwrap();
        assert_eq!(
            nurbs_curve_speed_bound(&curve).map(crate::scalar::FiniteReal::get),
            Some(1.0)
        );
        assert_eq!(
            nurbs_curve_parameter_near_point(&curve, poles[0], 0., 0.)
                .map(crate::scalar::FiniteReal::get),
            Some(0.0)
        );
        assert_eq!(
            nurbs_pcurve_contains_point(
                1,
                &[0., 0., 1., 1.],
                &points,
                Some(&[w, w]),
                Point2::new(0.5, 0.),
                0.
            ),
            Some(true)
        );
    }
}

#[test]
fn numerical_followup_periodic_mapping_stays_finite_and_canonical() {
    let curve = NurbsCurve::from_lanes(
        1,
        vec![-1e308, -1e308, -9e307, -9e307],
        vec![Point3::new(0., 0., 0.), Point3::new(1., 0., 0.)],
        None,
        true,
    )
    .unwrap();
    let mapped = map_nurbs_curve_parameter(&curve, crate::scalar::FiniteReal::new(1e308).unwrap())
        .unwrap()
        .get();
    assert!((-1e308..-9e307).contains(&mapped));
    assert_eq!(
        map_nurbs_curve_parameter(&curve, crate::scalar::FiniteReal::new(-9e307).unwrap())
            .map(crate::scalar::FiniteReal::get),
        Some(-1e308)
    );
}

#[test]
fn numerical_followup_sweep_quotient_retains_finite_derivatives() {
    for denominator in [1e200, 1e-200] {
        let expression = LawExpression::Algebraic {
            operator: "DIV".into(),
            operands: vec![
                LawExpression::Text {
                    value: cadmpeg_core::text::NonBlankString::new("X").unwrap(),
                },
                LawExpression::Double { value: denominator },
            ],
        };
        let value = scalar_sweep_law_differential(
            &expression.admit().unwrap(),
            crate::scalar::FiniteReal::new(1.).unwrap(),
        )
        .unwrap();
        assert_eq!(value.value.get(), 1. / denominator);
        assert_eq!(value.derivative.unwrap().get(), 1. / denominator);
    }
}

#[test]
fn numerical_followup_hyperbolic_laws_preserve_values_and_chain_derivatives() {
    let tanh = scalar_unary_sweep_law_differential("TANH", law_operand(20., 1e20)).unwrap();
    let expected = 1e20 / 20.0_f64.cosh().powi(2);
    assert!((tanh.derivative.unwrap().get() / expected - 1.).abs() <= 8. * f64::EPSILON);
    for operator in ["ARCSINH", "ARCCOSH"] {
        let result =
            scalar_unary_sweep_law_differential(operator, law_operand(1e200, 1e200)).unwrap();
        assert!((result.derivative.unwrap().get() - 1.).abs() <= 4. * f64::EPSILON);
    }
    let coth = scalar_unary_sweep_law_differential("ARCOTH", law_operand(1e20, 1.)).unwrap();
    assert!((coth.value.get() / 1e-20 - 1.).abs() <= 4. * f64::EPSILON);
    let tail = scalar_unary_sweep_law_differential("TANH", law_operand(400., 1e300)).unwrap();
    let tail = tail.derivative.unwrap().get();
    assert!(tail > 0.0 && tail.is_finite());
}

#[test]
fn analytic_line_search_preserves_subnormal_scale_residuals() {
    let line = SolvedCurveGeometry::Line(
        crate::geometry::analytic::LineCurve::try_new(
            Point3::new(0., 0., 0.),
            Vector3::new(1., 0., 0.),
        )
        .unwrap(),
    );
    assert_eq!(
        direct_curve_parameter_near_point(
            &line,
            Point3::new(0., 1e-200, 0.),
            crate::scalar::FiniteReal::ZERO,
            crate::scalar::NonNegativeLength::ZERO,
        ),
        None
    );
}

#[test]
fn inverse_laws_preserve_scaled_chain_derivatives() {
    for (operator, sign) in [
        ("ARCTAN", 1.),
        ("ARCOT", -1.),
        ("ARCSEC", 1.),
        ("ARCCSC", -1.),
        ("ARCCSCH", -1.),
    ] {
        for x in [-1e200, 1e200] {
            let result =
                scalar_unary_sweep_law_differential(operator, law_operand(x, 1e300)).unwrap();
            assert!(
                (result.derivative.unwrap().get() / (sign * 1e-100) - 1.).abs()
                    <= 8. * f64::EPSILON,
                "{operator}"
            );
        }
    }
}

#[test]
fn reciprocal_hyperbolic_laws_preserve_exponential_tails() {
    for x in [-720.0_f64, 720.] {
        for operator in ["SECH", "CSCH"] {
            let result =
                scalar_unary_sweep_law_differential(operator, law_operand(x, 1e300)).unwrap();
            let expected_magnitude = 2. * (-360.0_f64).exp() * (1e300 * (-360.0_f64).exp());
            let sign = if operator == "SECH" { -x.signum() } else { -1. };
            assert!(
                (result.derivative.unwrap().get() / (sign * expected_magnitude) - 1.).abs()
                    <= 8. * f64::EPSILON
            );
            assert!(result.value.get() != 0. && result.value.get().is_finite());
        }
    }
    let result = scalar_unary_sweep_law_differential("COTH", law_operand(400., 1e300)).unwrap();
    let expected = -4. * (-400.0_f64).exp() * (1e300 * (-400.0_f64).exp());
    assert!((result.derivative.unwrap().get() / expected - 1.).abs() <= 8. * f64::EPSILON);
}

#[test]
fn inverse_cosecant_hyperbolic_retains_subnormal_arguments() {
    for x in [-1e-320_f64, 1e-320] {
        let result =
            scalar_unary_sweep_law_differential("ARCCSCH", law_operand(x, x.abs())).unwrap();
        let expected = (std::f64::consts::LN_2 - x.abs().ln()).copysign(x);
        assert!((result.value.get() / expected - 1.).abs() <= 4. * f64::EPSILON);
        assert_eq!(result.derivative.unwrap().get(), -1.);
    }
}
#[test]
fn reciprocal_hyperbolic_laws_preserve_ordinary_values() {
    for x in [-1.0_f64, 1.] {
        for (op, value, derivative) in [
            ("COTH", 1. / x.tanh(), -3. / x.sinh().powi(2)),
            ("SECH", 1. / x.cosh(), -3. * x.tanh() / x.cosh()),
            ("CSCH", 1. / x.sinh(), -3. * x.cosh() / x.sinh().powi(2)),
        ] {
            let result = scalar_unary_sweep_law_differential(op, law_operand(x, 3.)).unwrap();
            assert!((result.value.get() / value - 1.).abs() <= 8. * f64::EPSILON);
            assert!(
                (result.derivative.unwrap().get() / derivative - 1.).abs() <= 8. * f64::EPSILON
            );
        }
    }
}

#[test]
fn a_law_at_its_domain_boundary_keeps_its_value_without_a_derivative() {
    for (operator, x, value) in [
        ("ABS", 0.0, 0.0),
        ("SQRT", 0.0, 0.0),
        ("ARCCOS", 1.0, 0.0),
        ("ARCSIN", -1.0, -std::f64::consts::FRAC_PI_2),
        ("ARCSEC", 1.0, 0.0),
        ("ARCCSC", -1.0, -std::f64::consts::FRAC_PI_2),
        ("ARCCOSH", 1.0, 0.0),
        ("ARCSECH", 1.0, 0.0),
    ] {
        let result = scalar_unary_sweep_law_differential(operator, law_operand(x, 1.0))
            .unwrap_or_else(|failure| panic!("{operator} at {x}: {failure:?}"));
        assert_eq!(result.value.get(), value, "{operator}");
        assert_eq!(
            result.derivative,
            Err(crate::eval::EvaluationFailure::NoValue),
            "{operator}"
        );
    }
}

#[test]
fn an_inverse_hyperbolic_cotangent_whose_derivative_overflows_keeps_its_value() {
    // Next to x = 1 the chain factor 1 / (1 - x^2) is about -2^51, so the
    // derivative 1e300 / (1 - x^2) leaves the finite range; the value is
    // atanh(1 / x).
    let x = 1.0 + f64::EPSILON;
    let result = scalar_unary_sweep_law_differential("ARCOTH", law_operand(x, 1.0e300))
        .expect("the value is finite");
    let expected = 0.5 * ((x + 1.0) / (x - 1.0)).ln();
    assert!((result.value.get() / expected - 1.0).abs() <= 8.0 * f64::EPSILON);
    assert_eq!(
        result.derivative,
        Err(crate::eval::EvaluationFailure::NonFinite(()))
    );
}
