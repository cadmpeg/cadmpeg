// SPDX-License-Identifier: Apache-2.0

use super::super::{run_shape, try_shape, Cur, Slot, TypeFailure, TypedRecordFailure};
use cadmpeg_core::CodecError;

fn length(value: f64) {
    crate::test_support::with_entry_context(|ctx, original| {
        let mut cur = Cur { prims: &[], pos: 0, scale: 10.0, failure: None, resource: None, ctx };
        let result = cur.length(value);
        assert_eq!(cur.pos, 0);
        match original {
            Some(first) => {
                assert!(result.is_none());
                assert!(matches!(cur.resource, Some(CodecError::ResourceLimit(last)) if last == first));
                assert!(cur.failure.is_none());
            }
            None => {
                assert!(cur.resource.is_none());
                if value.is_finite() {
                    assert_eq!(result, Some(value));
                    assert!(cur.failure.is_none());
                } else {
                    assert!(result.is_none());
                    assert_eq!(cur.failure, Some(TypeFailure::UnrepresentableLength));
                }
            }
        }
    });
}

#[test]
fn sat_fixed_length_preserves_original_refusal() { length(7.0); }

#[test]
fn sat_invalid_length_preserves_original_refusal() { length(f64::INFINITY); }

fn invalid_spline_count(previous: Option<TypeFailure>) {
    crate::test_support::with_entry_context(|ctx, original| {
        let mut cur = Cur { prims: &[], pos: 0, scale: 10.0, failure: previous, resource: None, ctx };
        assert_eq!(cur.invalid_spline_count::<usize>(), None);
        assert_eq!(cur.pos, 0);
        match original {
            Some(first) => {
                assert!(matches!(cur.resource, Some(CodecError::ResourceLimit(last)) if last == first));
                assert_eq!(cur.failure, previous);
            }
            None => {
                assert!(cur.resource.is_none());
                assert_eq!(cur.failure, Some(TypeFailure::InvalidSplineCount));
            }
        }
    });
}

#[test]
fn sat_invalid_spline_count_preserves_original_refusal() { invalid_spline_count(None); }

#[test]
fn sat_refused_spline_count_keeps_previous_type_failure() {
    invalid_spline_count(Some(TypeFailure::UnrepresentableLength));
}

fn shape(slots: &[Slot], fresh: Option<()>) {
    crate::test_support::with_entry_context(|ctx, original| {
        let mut cur = Cur { prims: &[], pos: 0, scale: 10.0, failure: None, resource: None, ctx };
        let mut out = Vec::new();
        let result = run_shape(&mut cur, slots, &mut out);
        match original {
            Some(first) => assert!(matches!(result, Err(CodecError::ResourceLimit(last)) if last == first)),
            None => assert_eq!(result.expect("fixed empty-source grammar is free"), fresh),
        }
        assert_eq!(cur.pos, 0);
        assert!(out.is_empty());
        assert!(cur.failure.is_none());
        assert!(cur.resource.is_none());
    });
}

#[test]
fn sat_empty_shape_preserves_original_refusal() { shape(&[], Some(())); }

#[test]
fn sat_absent_shape_primitive_preserves_original_refusal() { shape(&[Slot::D], None); }

#[test]
fn sat_empty_shape_candidate_preserves_original_refusal() {
    crate::test_support::with_entry_context(|ctx, original| {
        let result = try_shape(ctx, &[], 10.0, &[]);
        match original {
            Some(first) => assert!(matches!(result,
                Err(TypedRecordFailure::Resource(CodecError::ResourceLimit(last))) if last == first)),
            None => match result {
                Ok(Some(tokens)) => assert!(tokens.is_empty()),
                _ => panic!("empty shape matches empty source without resource work"),
            },
        }
    });
}
