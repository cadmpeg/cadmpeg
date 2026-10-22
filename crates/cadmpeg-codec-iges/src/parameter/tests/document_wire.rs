// SPDX-License-Identifier: Apache-2.0
//! Typed parameter values have no source-layout positions on the document wire.

use crate::parameter::{Token, TokenValue};

#[test]
fn token_span_is_not_part_of_typed_content() {
    let first = Token {
        value: TokenValue::Integer(42),
        span: 3..5,
    };
    let moved = Token {
        value: TokenValue::Integer(42),
        span: 300..302,
    };
    let expected = serde_json::json!({"value":{"kind": "integer", "value": 42}});
    assert_eq!(serde_json::to_value(first).unwrap(), expected);
    assert_eq!(serde_json::to_value(moved).unwrap(), expected);
}

#[test]
fn hollerith_source_bytes_keep_the_exact_native_value() {
    let token = Token {
        value: TokenValue::String(cadmpeg_ir::native::bytes::NativeBytes::from(
            b"part".to_vec(),
        )),
        span: 1..7,
    };
    assert_eq!(
        serde_json::to_value(token).unwrap(),
        serde_json::json!({"value":{"kind": "string", "value": "70617274"}})
    );
}

#[test]
fn absent_parameter_slots_are_null_without_becoming_format_defaults() {
    let mut record = super::super::ParameterRecord::from_test_tokens(
        1,
        1..2,
        Vec::new(),
        3,
        vec![
            Token {
                value: TokenValue::Integer(116),
                span: 0..3,
            },
            Token {
                value: TokenValue::Omitted,
                span: 4..4,
            },
            Token {
                value: TokenValue::Integer(7),
                span: 5..6,
            },
        ],
        Vec::new(),
    );
    assert_eq!(record.integer_or(1, 9), Some(9));
    assert_eq!(record.number_or(1, 9.0), Some(9.0));
    assert_eq!(record.string_or_empty(1), Some(&[][..]));
    record.tokens[1] = None;
    assert_eq!(record.integer_or(1, 9), None);
    assert_eq!(record.number_or(1, 9.0), None);
    assert_eq!(record.string_or_empty(1), None);
    let tokens = serde_json::to_value(record.tokens()).unwrap();
    assert_eq!(tokens.as_array().unwrap().len(), 3);
    assert!(tokens[1].is_null());
    assert_eq!(tokens[0]["value"]["value"], 116);
    assert_eq!(tokens[2]["value"]["value"], 7);
}
