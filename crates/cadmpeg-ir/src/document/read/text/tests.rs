// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use serde_json::json;

use super::{value, Found, Scanner, Unreadable};

#[test]
fn members_are_framed_up_to_the_point_the_text_stops_being_json() {
    let text = br#"{"a": [1, {"b": 2}], "c": "x", "d": [3, "#;
    let mut scanner = Scanner::new(text, 0);
    let (members, stop) = scanner.members();
    let keys: Vec<&str> = members.iter().map(|member| member.key.as_str()).collect();
    assert_eq!(keys, ["a", "c", "d"]);
    assert!(members[0].value.closed && members[1].value.closed);
    assert_eq!(members[0].value.depth, 2);
    assert!(!members[2].value.closed);
    assert_eq!(stop.unwrap().at, text.len());
}

#[test]
fn a_value_keeps_one_copy_of_a_key_restated_with_one_value_and_none_otherwise() {
    let text = br#"{"a": 1, "a": 1, "b": 2, "b": 3, "c": "x"}"#;
    let mut scanner = Scanner::new(text, 0);
    let span = scanner.value();
    let mut found = Vec::new();
    let read = value(text, span, &mut found).unwrap();
    assert_eq!(read, json!({"a": 1, "c": "x"}));
    assert_eq!(
        found,
        [
            Found::Restated("a".into()),
            Found::Conflicting {
                key: "b".into(),
                copies: vec![(17, 23), (25, 31)],
            },
        ]
    );
}

#[test]
fn an_invalid_utf8_sequence_in_a_string_is_replaced_and_in_a_key_or_an_id_is_unreadable() {
    let text = b"{\"a\": \"x\xFFy\"}";
    let span = Scanner::new(text, 0).value();
    let mut found = Vec::new();
    assert_eq!(
        value(text, span, &mut found).unwrap(),
        json!({"a": "x\u{FFFD}y"})
    );
    assert_eq!(found, [Found::Replaced(6)]);

    let text = b"{\"\xFF\": 1}";
    let span = Scanner::new(text, 0).value();
    assert_eq!(value(text, span, &mut Vec::new()), Err(Unreadable::Key(1)));

    let text = b"{\"id\": \"x\xFFy\"}";
    let span = Scanner::new(text, 0).value();
    assert_eq!(value(text, span, &mut Vec::new()), Err(Unreadable::Id(7)));

    let text = b"[\"a:b:c#\xFF\"]";
    let span = Scanner::new(text, 0).value();
    assert_eq!(value(text, span, &mut Vec::new()), Err(Unreadable::Id(1)));
}

#[test]
fn numbers_read_as_integers_where_they_are_and_past_range_numbers_are_unreadable() {
    let text = b"[1, -2, 0.5, 18446744073709551615]";
    let span = Scanner::new(text, 0).value();
    assert_eq!(
        value(text, span, &mut Vec::new()).unwrap(),
        json!([1, -2, 0.5, 18_446_744_073_709_551_615_u64])
    );
    let text = b"[1e999]";
    let span = Scanner::new(text, 0).value();
    assert_eq!(
        value(text, span, &mut Vec::new()),
        Err(Unreadable::Number(1))
    );
}
