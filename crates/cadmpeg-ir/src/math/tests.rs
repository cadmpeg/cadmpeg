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
