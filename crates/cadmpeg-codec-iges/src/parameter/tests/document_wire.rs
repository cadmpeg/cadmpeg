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
    let expected = serde_json::json!({"kind": "integer", "value": 42});
    assert_eq!(serde_json::to_value(first).unwrap(), expected);
    assert_eq!(serde_json::to_value(moved).unwrap(), expected);
}

#[test]
fn hollerith_source_bytes_become_a_decoded_string_value() {
    let token = Token {
        value: TokenValue::String(cadmpeg_ir::native::bytes::NativeBytes::from(
            b"part".to_vec(),
        )),
        span: 1..7,
    };
    assert_eq!(
        serde_json::to_value(token).unwrap(),
        serde_json::json!({"kind": "string", "value": "part"})
    );
}
