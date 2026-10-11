// SPDX-License-Identifier: Apache-2.0

use super::super::{Cur, TypeFailure};

fn length(value: f64) {
    crate::test_support::with_zero_context(|ctx| {
        let mut cur = Cur { prims: &[], pos: 0, scale: 10.0, failure: None, resource: None, ctx };
        let result = cur.length(value);
        assert_eq!(cur.pos, 0);
        
                assert!(cur.resource.is_none());
                if value.is_finite() {
                    assert_eq!(result, Some(value));
                    assert!(cur.failure.is_none());
                } else {
                    assert!(result.is_none());
                    assert_eq!(cur.failure, Some(TypeFailure::UnrepresentableLength));
                }
            
    });
}

#[test]
fn sat_finite_length_keeps_value_seven() { length(7.0); }

#[test]
fn sat_infinite_length_sets_unrepresentable_length() { length(f64::INFINITY); }

fn invalid_spline_count(previous: Option<TypeFailure>) {
    crate::test_support::with_zero_context(|ctx| {
        let mut cur = Cur { prims: &[], pos: 0, scale: 10.0, failure: previous, resource: None, ctx };
        assert_eq!(cur.invalid_spline_count::<usize>(), None);
        assert_eq!(cur.pos, 0);
        
                assert!(cur.resource.is_none());
                assert_eq!(cur.failure, Some(TypeFailure::InvalidSplineCount));
            
    });
}

#[test]
fn sat_invalid_spline_count_sets_type_failure() { invalid_spline_count(None); }

#[test]
fn sat_invalid_spline_count_replaces_previous_type_failure() {
    invalid_spline_count(Some(TypeFailure::UnrepresentableLength));
}
