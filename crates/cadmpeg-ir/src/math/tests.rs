// SPDX-License-Identifier: Apache-2.0
use super::{Point2, Point3, Vector3};

const EPS_UNIT_RESULT: f64 = 8.0 * f64::EPSILON;

#[test]
fn point_is_finite_rejects_a_nonfinite_coordinate_in_any_slot() {
    assert!(Point3::new(1.0, -2.0, 3.0).is_finite());
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(!Point3::new(value, 1.0, 1.0).is_finite());
        assert!(!Point3::new(1.0, value, 1.0).is_finite());
        assert!(!Point3::new(1.0, 1.0, value).is_finite());
    }
}

#[test]
fn vector_is_finite_rejects_a_nonfinite_component_in_any_slot() {
    assert!(Vector3::new(1.0, -2.0, 3.0).is_finite());
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(!Vector3::new(value, 1.0, 1.0).is_finite());
        assert!(!Vector3::new(1.0, value, 1.0).is_finite());
        assert!(!Vector3::new(1.0, 1.0, value).is_finite());
    }
}

#[test]
fn parameter_point_is_finite_rejects_a_nonfinite_coordinate_in_either_slot() {
    assert!(Point2::new(1.0, -2.0).is_finite());
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(!Point2::new(value, 1.0).is_finite());
        assert!(!Point2::new(1.0, value).is_finite());
    }
}

#[test]
fn unit_vector_preserves_finite_direction_without_squared_length_overflow() {
    for scale in [1.0, 2.0_f64.powi(600), f64::MAX] {
        for (input, expected) in [
            (Vector3::new(scale, 0.0, 0.0), Vector3::new(1.0, 0.0, 0.0)),
            (Vector3::new(0.0, -scale, 0.0), Vector3::new(0.0, -1.0, 0.0)),
            (Vector3::new(0.0, 0.0, scale), Vector3::new(0.0, 0.0, 1.0)),
            (
                Vector3::new(scale, -scale, scale),
                Vector3::new(1.0, -1.0, 1.0).scale(1.0 / 3.0_f64.sqrt()),
            ),
        ] {
            let actual = input.unit().expect("finite nondegenerate direction");
            assert!((actual.x - expected.x).abs() <= EPS_UNIT_RESULT);
            assert!((actual.y - expected.y).abs() <= EPS_UNIT_RESULT);
            assert!((actual.z - expected.z).abs() <= EPS_UNIT_RESULT);
            assert!((actual.norm() - 1.0).abs() <= EPS_UNIT_RESULT);
        }
    }
}

#[test]
fn unit_vector_preserves_the_epsilon_degeneracy_boundary() {
    for x in [0.0, f64::from_bits(1), f64::MIN_POSITIVE, f64::EPSILON] {
        assert!(Vector3::new(x, 0.0, 0.0).unit().is_none());
        assert!(Vector3::new(0.0, -x, 0.0).unit().is_none());
    }
    let above = f64::from_bits(f64::EPSILON.to_bits() + 1);
    assert_eq!(
        Vector3::new(above, 0.0, 0.0).unit(),
        Some(Vector3::new(1.0, 0.0, 0.0))
    );
    let diagonal = Vector3::new(f64::EPSILON, f64::EPSILON, 0.0)
        .unit()
        .unwrap();
    assert!((diagonal.x - 1.0 / 2.0_f64.sqrt()).abs() <= EPS_UNIT_RESULT);
    assert!((diagonal.y - diagonal.x).abs() <= EPS_UNIT_RESULT);
    assert_eq!(diagonal.z, 0.0);
}

#[test]
fn unit_vector_refuses_each_nonfinite_component() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for input in [
            Vector3::new(value, 1.0, 1.0),
            Vector3::new(1.0, value, 1.0),
            Vector3::new(1.0, 1.0, value),
        ] {
            assert!(input.unit().is_none());
        }
    }
}

#[test]
fn numerical_audit_lengths_and_nonzero_directions_keep_the_finite_range() {
    for magnitude in [f64::from_bits(1), 1.0e-200, 1.0, 1.0e200, f64::MAX] {
        let vector = Vector3::new(magnitude, 0.0, 0.0);
        assert_eq!(vector.norm(), magnitude);
        assert_eq!(
            Point3::new(magnitude, 0.0, 0.0).distance(Point3::new(0.0, 0.0, 0.0)),
            magnitude
        );
        assert_eq!(vector.unit_nonzero(), Some(Vector3::new(1.0, 0.0, 0.0)));
    }
    let diagonal = Vector3::new(f64::MAX, f64::MAX, 0.0)
        .unit_nonzero()
        .unwrap();
    assert!((diagonal.norm() - 1.0).abs() <= EPS_UNIT_RESULT);
    assert!(Vector3::new(0.0, 0.0, 0.0).unit_nonzero().is_none());
    assert!(Vector3::new(f64::INFINITY, 0.0, 0.0)
        .unit_nonzero()
        .is_none());
}

#[test]
fn numerical_audit_parameter_reflection_preserves_shifted_endpoints() {
    for [start, end] in [
        [1.0e16, 1.0e16 + 2.0],
        [1.0e308, 1.1e308],
        [-f64::MAX, f64::MAX],
    ] {
        assert_eq!(super::reflect_parameter(start, start, end), Some(end));
        assert_eq!(super::reflect_parameter(end, start, end), Some(start));
    }
    assert_eq!(
        super::reflect_parameter(0.0, -f64::MAX, f64::MAX),
        Some(0.0)
    );
    assert!(super::reflect_parameter(-f64::MAX, 0.0, f64::MAX).is_none());
}

#[test]
fn numerical_audit_product_quotient_preserves_representable_results() {
    assert_eq!(super::multiply_divide(0.0, 1.0e200, 1.0e-200), Some(0.0));
    for value in [f64::from_bits(1), 1.0e-200, 1.0, 1.0e200, f64::MAX] {
        let result = super::multiply_divide(value, value, value).unwrap();
        assert!((result / value - 1.0).abs() <= 4.0 * f64::EPSILON);
    }
    assert!(super::multiply_divide(1.0, 1.0, 0.0).is_none());
}

#[test]
fn numerical_ranges_products_keep_intermediate_exponents() {
    use super::{product_quotient, scaled_sinh_cosh};
    assert_eq!(
        product_quotient([1e200, 1e200], [4.0, 1e200]),
        Some(2.5e199)
    );
    assert_eq!(product_quotient([2.0, 1e308, 0.5], []), Some(1e308));
    assert_eq!(product_quotient([1.0], [0.0]), None);
    assert_eq!(product_quotient([f64::NAN], [1.0]), None);
    assert_eq!(product_quotient([f64::MAX, 2.0], []), None);
    for parameter in [-720.0_f64, 720.0] {
        let expected = (parameter.abs() + 1e-10_f64.ln()).exp() * 0.5;
        let (sinh, cosh) = scaled_sinh_cosh(1e-10, parameter).unwrap();
        assert!((cosh / expected - 1.0).abs() < 1024.0 * f64::EPSILON);
        assert_eq!(sinh, parameter.signum() * cosh);
    }
    assert_eq!(scaled_sinh_cosh(0.0, 2000.0), Some((0.0, 0.0)));
    assert_eq!(scaled_sinh_cosh(1.0, 2000.0), None);
    assert_eq!(scaled_sinh_cosh(2.0, 0.0), Some((0.0, 2.0)));
}
#[test]
fn numerical_ranges_interpolation_and_wrapping_avoid_endpoint_subtraction_overflow() {
    use super::{interpolate, wrap_parameter};
    assert_eq!(interpolate(-1e308, 1e308, 0.5), Some(0.0));
    assert_eq!(interpolate(-1e308, 1e308, 0.0), Some(-1e308));
    assert_eq!(interpolate(-1e308, 1e308, 1.0), Some(1e308));
    assert_eq!(interpolate(0.0, 1.0, f64::NAN), None);
    assert_eq!(wrap_parameter(1e308, -1e308, 0.0), Some(-1e308));
    assert_eq!(wrap_parameter(1.5e308, -1e308, 1e308), Some(-5e307));
    assert_eq!(wrap_parameter(1.0, -1.0, 1.0), Some(-1.0));
    assert_eq!(wrap_parameter(0.5, -1.0, 1.0), Some(0.5));
    assert_eq!(wrap_parameter(1.0, 0.0, 0.0), None);
}

#[test]
fn a_power_of_two_bound_normalises_without_moving_a_significand() {
    use super::{power_of_two_bound, scale_power_of_two};
    for value in [
        f64::from_bits(1),
        -1.0e-200,
        0.5,
        1.0,
        -6.0,
        1.0e200,
        f64::MAX,
    ] {
        let exponent = power_of_two_bound(value).unwrap();
        let normalised = scale_power_of_two(value, -exponent).unwrap();
        assert!((0.5..1.0).contains(&normalised.abs()));
        assert_eq!(scale_power_of_two(normalised, exponent), Some(value));
    }
    assert_eq!(power_of_two_bound(1.0), Some(1));
    assert_eq!(power_of_two_bound(-6.0), Some(3));
    for value in [0.0, -0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(power_of_two_bound(value), None);
    }
}

#[test]
fn audit_regression_power_scaling_preserves_extreme_finite_results() {
    use super::scale_power_of_two;
    let minimum = f64::from_bits(1);
    let maximum_power = 2.0_f64.powi(1023);
    assert_eq!(scale_power_of_two(minimum, 2097), Some(maximum_power));
    assert_eq!(scale_power_of_two(maximum_power, -2097), Some(minimum));
    assert_eq!(scale_power_of_two(1.5, -1074), Some(2.0 * minimum));
    assert_eq!(scale_power_of_two(1.0, i32::MAX), None);
    assert_eq!(scale_power_of_two(1.0, i32::MIN), Some(0.0));
    assert_eq!(
        scale_power_of_two(-0.0, i32::MAX).unwrap().to_bits(),
        (-0.0_f64).to_bits()
    );
    assert_eq!(scale_power_of_two(f64::NAN, 0), None);
}

#[test]
fn numerical_0922b_parameter_fraction_preserves_finite_charts() {
    for (parameter, start, end, expected) in [
        (0.0, -1e308, 1e308, 0.5),
        (-1e308, -1e308, 1e308, 0.0),
        (1e308, -1e308, 1e308, 1.0),
        (-2.0, 0.0, 1.0, -2.0),
        (3.0, 0.0, 1.0, 3.0),
    ] {
        assert_eq!(
            super::parameter_fraction(parameter, start, end),
            Some(expected)
        );
    }
}

#[test]
fn numerical_audit_domains_do_not_admit_disjoint_parameter_ranges() {
    for domain in [[0.0_f64, 1.], [0., 1e-16], [-1e308, 1e308]] {
        for value in [domain[0], domain[0].midpoint(domain[1]), domain[1]] {
            assert!(super::parameter_in_domain(
                value,
                domain,
                64. * f64::EPSILON
            ));
        }
    }
    for d in [1., 1e-16] {
        assert!(!super::parameter_in_domain(
            2. * d,
            [0., d],
            64. * f64::EPSILON
        ));
        assert!(!super::parameter_in_domain(-d, [0., d], 64. * f64::EPSILON));
        assert!(super::parameter_in_domain(
            d.next_up(),
            [0., d],
            64. * f64::EPSILON
        ));
    }
    assert!(!super::parameter_in_domain(f64::NAN, [0., 1.], 0.));
}
