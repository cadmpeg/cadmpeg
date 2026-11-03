// SPDX-License-Identifier: Apache-2.0

use super::{dictionary_value, half, real, shaped, Datum, Shape};
use crate::text::syntax::{DictionaryValue, Value};

#[test]
fn half_values() {
    assert_eq!(half(0x3C00), 1.0);
    assert_eq!(half(0xC000), -2.0);
    assert_eq!(half(0x3555), 0.333_251_953_125);
    assert_eq!(half(0x0001), 2.0_f64.powi(-24));
    assert_eq!(half(0x7BFF), 65_504.0);
    assert_eq!(half(0x7C00), f64::INFINITY);
    assert!(half(0x7E00).is_nan());
}

#[test]
fn quaternions_state_the_real_part_first() {
    let stored = vec![real(1.0), real(2.0), real(3.0), real(4.0)];
    assert_eq!(
        shaped(Shape::Quaternion, stored),
        Value::Tuple(vec![real(4.0), real(1.0), real(2.0), real(3.0)])
    );
}

#[test]
fn matrices_are_tuples_of_rows() {
    let stored = (1..=4).map(|value| real(f64::from(value))).collect();
    assert_eq!(
        shaped(Shape::Matrix(2), stored),
        Value::Tuple(vec![
            Value::Tuple(vec![real(1.0), real(2.0)]),
            Value::Tuple(vec![real(3.0), real(4.0)]),
        ])
    );
}

#[test]
fn token_vectors_are_token_arrays_in_dictionaries() {
    assert_eq!(
        dictionary_value(Datum::Tokens(vec!["a".to_owned()])),
        Ok(DictionaryValue::Typed {
            type_name: "token".to_owned(),
            array: true,
            value: Value::List(vec![Value::String("a".to_owned())]),
        })
    );
    assert_eq!(dictionary_value(Datum::Unread("spline")), Err("spline"));
}
