// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::serde_erase::json;
use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{json, Value};

use super::{
    to_canonical_json_string, to_compact_canonical_json_string, to_json_value, CanonicalJsonError,
};

#[derive(Serialize)]
struct Scalars {
    negative_zero: f64,
    not_a_number: f64,
    infinity: f64,
    narrow: f32,
    subnormal: f64,
    integral_float: f64,
    largest: u64,
    smallest: i64,
    character: char,
    escaped: String,
    unit: (),
    absent: Option<f64>,
    keys: BTreeMap<String, Value>,
    tagged: Vec<Tagged>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Tagged {
    Point { x: f64 },
    Origin,
}

fn scalars() -> Scalars {
    Scalars {
        negative_zero: -0.0,
        not_a_number: f64::NAN,
        infinity: f64::NEG_INFINITY,
        narrow: 0.1,
        subnormal: f64::from_bits(1),
        integral_float: 3.0,
        largest: u64::MAX,
        smallest: i64::MIN,
        character: 'é',
        escaped: "quote \" slash \\ line\n tab\t nul\u{0}".to_owned(),
        unit: (),
        absent: None,
        keys: BTreeMap::from([
            ("$serde_json::private::RawValue".to_owned(), json!("[1]")),
            (
                "nested".to_owned(),
                json!({"$serde_json::private::RawValue": "{}", "b": [1, -2, 2.5]}),
            ),
        ]),
        tagged: vec![Tagged::Point { x: -0.0 }, Tagged::Origin],
    }
}

/// The value equals `expected`, and so does its text, which tells negative
/// zero from zero and an integer from an integral float.
fn assert_value<T: Serialize>(value: &T, expected: &Value) {
    let actual = to_json_value(value).unwrap();
    assert_eq!(&actual, expected);
    assert_eq!(actual.to_string(), expected.to_string());
}

#[test]
fn to_json_value_matches_serde_json_on_scalars_and_keys_with_one_zero() {
    let mut expected = json::to_value(scalars()).unwrap();
    assert!(expected["negative_zero"]
        .as_f64()
        .unwrap()
        .is_sign_negative());
    expected["negative_zero"] = json!(0.0);
    expected["tagged"][0]["x"] = json!(0.0);
    assert_value(&scalars(), &expected);
}

#[test]
fn to_json_value_matches_serde_json_on_a_document() {
    let document = crate::examples::unit_cube().unwrap();
    assert_value(&document, &json::to_value(&document).unwrap());
}

#[test]
fn every_writer_writes_negative_zero_as_zero() {
    let mut text = Vec::new();
    super::write_serde_json(
        super::admission::CanonicalAdmission::unlimited(),
        None,
        &mut text,
        &[-0.0_f64, 0.0],
    )
    .unwrap();
    let text = String::from_utf8(text).unwrap();
    assert!(!text.contains('-'), "{text}");
    assert_eq!(to_json_value(&-0.0_f32).unwrap().to_string(), "0.0");
    assert_eq!(to_canonical_json_string(&-0.0_f32).unwrap(), "0.0");
}

#[test]
fn to_json_value_refuses_a_key_that_is_not_a_string() {
    let map = BTreeMap::from([((1u8, 2u8), 3u8)]);
    let expected = json::to_value(&map).unwrap_err();
    let actual = to_json_value(&map).unwrap_err();
    assert_eq!(actual.to_string(), expected.to_string());
}

#[test]
fn canonical_text_still_refuses_a_non_finite_float_after_a_value_walk() {
    assert_eq!(to_json_value(&f64::NAN).unwrap(), Value::Null);
    assert!(matches!(
        to_canonical_json_string(&f64::NAN),
        Err(CanonicalJsonError::NonFinite { .. })
    ));
    assert_eq!(to_canonical_json_string(&-0.0f64).unwrap(), "0.0");
}

#[test]
fn compact_canonical_text_is_the_compact_serde_text() {
    let value = json!({
        "text": "a \"quoted\" word, a \\ and a line\nbreak: [ { } ]",
        "nested": [[1, 2.5, []], {"empty": {}, "key with space": null}],
        "flag": true
    });
    assert_eq!(
        to_compact_canonical_json_string(&value).unwrap(),
        json::to_string(&value).unwrap()
    );
    assert!(matches!(
        to_compact_canonical_json_string(&f64::NAN),
        Err(CanonicalJsonError::NonFinite { .. })
    ));
}
