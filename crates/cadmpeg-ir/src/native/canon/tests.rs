// SPDX-License-Identifier: Apache-2.0

use serde::ser::{SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant};
use serde::{Serialize, Serializer};

use super::CanonValue;
use crate::native::NativeNamespace;

enum ObjectShape {
    Map,
    Struct,
    Variant,
}

struct Object<'a> {
    shape: &'a ObjectShape,
    second_key: &'static str,
}

impl Serialize for Object<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self.shape {
            ObjectShape::Map => {
                let mut map = serializer.serialize_map(Some(2))?;
                map.serialize_entry("first", &1)?;
                map.serialize_entry(self.second_key, &2)?;
                map.end()
            }
            ObjectShape::Struct => {
                let mut fields = serializer.serialize_struct("Object", 2)?;
                fields.serialize_field("first", &1)?;
                fields.serialize_field(self.second_key, &2)?;
                fields.end()
            }
            ObjectShape::Variant => {
                let mut fields = serializer.serialize_struct_variant("Object", 0, "Variant", 2)?;
                fields.serialize_field("first", &1)?;
                fields.serialize_field(self.second_key, &2)?;
                fields.end()
            }
        }
    }
}

#[test]
fn duplicate_typed_fields_cannot_replace_a_native_arena() {
    #[derive(Serialize)]
    struct Record<'a> {
        id: &'static str,
        nested: Vec<Object<'a>>,
    }

    for shape in [ObjectShape::Map, ObjectShape::Struct, ObjectShape::Variant] {
        let record = |second_key| Record {
            id: "test:native:record#duplicate",
            nested: vec![Object {
                shape: &shape,
                second_key,
            }],
        };
        let mut namespace = NativeNamespace::default();
        namespace
            .set_arena("records", &[record("second")])
            .expect("distinct keys are legal");
        let before = namespace.clone();
        let error = namespace
            .set_arena("records", &[record("first")])
            .expect_err("duplicate fields must not collapse to their last value");
        assert!(error.to_string().contains("duplicate key first"), "{error}");
        assert_eq!(namespace, before);
    }
}

#[test]
fn malformed_map_protocol_is_reported() {
    let mut pending =
        serde::Serializer::serialize_map(CanonValue::for_record(), None).expect("map");
    pending.serialize_key("first").expect("first key");
    assert!(pending.serialize_key("second").is_err());
    assert!(SerializeMap::end(pending).is_err());

    let mut no_key = serde::Serializer::serialize_map(CanonValue::for_record(), None).expect("map");
    assert!(no_key.serialize_value(&1).is_err());
}

#[test]
fn a_rejected_sequence_element_does_not_corrupt_rendered_json() {
    struct Refused;
    impl Serialize for Refused {
        fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("fixture rejects this element"))
        }
    }

    let mut sequence =
        serde::Serializer::serialize_seq(CanonValue::for_record(), None).expect("sequence");
    sequence.serialize_element(&1).expect("first element");
    assert!(sequence.serialize_element(&Refused).is_err());
    sequence.serialize_element(&2).expect("second element");
    assert_eq!(sequence.end().expect("sequence").render(), "[1,2]");
}

#[test]
fn raw_json_values_use_the_same_canonical_native_admission() {
    use serde_json::{value::RawValue, Value};

    #[derive(Serialize, serde::Deserialize)]
    struct Record {
        id: crate::ids::Identity,
        raw: Box<RawValue>,
    }

    let mut deep = serde_json::json!(7);
    for _ in 0..140 {
        deep = Value::Array(vec![deep]);
    }
    let mut document = crate::CadIr::empty();
    for (json, expected) in [
        (
            r#"{ "z": [1, true], "a": "text" }"#.to_owned(),
            serde_json::json!({"a": "text", "z": [1, true]}),
        ),
        (format!("{}7{}", "[".repeat(140), "]".repeat(140)), deep),
    ] {
        let record = Record {
            id: crate::ids::Identity::new("test:native:record#raw").expect("fixture identity"),
            raw: RawValue::from_string(json).expect("legal raw JSON"),
        };
        document
            .native
            .namespace_mut("future")
            .set_arena("records", &[record])
            .expect("raw fields have ordinary JSON semantics");
        let wire = serde_json::to_value(&document).expect("document writes");
        let admitted: crate::CadIr = serde_json::from_value(wire.clone()).expect("document reads");
        assert_eq!(
            serde_json::to_value(&admitted).expect("document writes"),
            wire
        );
        let namespace = admitted.native.namespace("future").expect("namespace");
        let read = namespace
            .arena_as::<Record>("records")
            .expect("typed raw reader")
            .remove(0);
        assert_eq!(read.id.as_str(), "test:native:record#raw");
        assert_eq!(read.raw.get(), expected.to_string());
        let stored = &namespace.arenas()["records"][0];
        assert_eq!(stored.field("raw"), Some(expected));
    }

    let raw = RawValue::from_string(r#"{"value":7,"id":"test:native:record#raw"}"#.to_owned())
        .expect("object fixture");
    let mut namespace = NativeNamespace::default();
    namespace
        .set_arena("records", &[raw])
        .expect("raw object record");
    assert_eq!(
        namespace.arenas()["records"][0].field("value"),
        Some(serde_json::json!(7))
    );

    let before = namespace.clone();
    for json in [r#"{"a":1,"a":2}"#, r#"[{"a":1,"a":2}]"#] {
        let record = Record {
            id: crate::ids::Identity::new("test:native:record#raw").expect("fixture identity"),
            raw: RawValue::from_string(json.to_owned()).expect("raw JSON retains duplicate keys"),
        };
        let error = namespace
            .set_arena("records", &[record])
            .expect_err("duplicate raw keys");
        assert!(error.to_string().contains("duplicate key a"), "{error}");
        assert_eq!(namespace, before);
    }

    let ordinary = serde_json::json!({"$serde_json::private::RawValue": "null"});
    namespace
        .set_arena(
            "records",
            &[serde_json::json!({
                "id": "test:native:record#ordinary", "value": ordinary.clone(),
            })],
        )
        .expect("an ordinary map is not the RawValue struct protocol");
    assert_eq!(
        namespace.arenas()["records"][0].field("value"),
        Some(ordinary)
    );
}

/// The canonical serializer recurses one frame per container of the record it
/// is handed, so a `serde_json::Value` field states the descent. One budget
/// spans the whole record, and a `RawValue` nested inside it continues that
/// budget instead of starting a fresh one.
#[test]
fn one_budget_spans_the_whole_canonical_write() {
    use crate::native::{NativeRecord, MAX_NATIVE_NESTING_DEPTH};
    use serde_json::{value::RawValue, Value};

    #[derive(Serialize)]
    struct Record {
        id: &'static str,
        nested: Value,
    }

    #[derive(Serialize)]
    struct Wrapped {
        id: &'static str,
        nested: Vec<Box<RawValue>>,
    }

    let chain = |containers: usize| {
        let mut nested = serde_json::json!(7);
        for _ in 0..containers {
            nested = Value::Array(vec![nested]);
        }
        nested
    };
    let text = |containers: usize| format!("{}7{}", "[".repeat(containers), "]".repeat(containers));

    let admitted = chain(MAX_NATIVE_NESTING_DEPTH);
    let record = NativeRecord::from_typed(&Record {
        id: "test:native:record#canon",
        nested: admitted.clone(),
    })
    .expect("the bound admits its own depth");
    assert_eq!(record.field("nested"), Some(admitted));

    let refused = NativeRecord::from_typed(&Record {
        id: "test:native:record#canon",
        nested: chain(MAX_NATIVE_NESTING_DEPTH + 1),
    })
    .expect_err("one container past the bound");
    assert!(
        refused.to_string().contains(&format!(
            "native value nests deeper than {MAX_NATIVE_NESTING_DEPTH} containers"
        )),
        "{refused}"
    );

    // The field is an array, so the raw payload may enter one container fewer.
    let wrapped = |containers: usize| Wrapped {
        id: "test:native:record#canon",
        nested: vec![RawValue::from_string(text(containers)).expect("legal raw JSON")],
    };
    NativeRecord::from_typed(&wrapped(MAX_NATIVE_NESTING_DEPTH - 1))
        .expect("the array plus the payload reach the bound");
    let refused = NativeRecord::from_typed(&wrapped(MAX_NATIVE_NESTING_DEPTH))
        .expect_err("a raw payload continues the record's budget");
    assert!(
        refused.to_string().contains(&format!(
            "native value nests deeper than {MAX_NATIVE_NESTING_DEPTH} containers"
        )),
        "{refused}"
    );
}

#[test]
fn malformed_raw_json_protocol_cannot_produce_a_native_value() {
    let make = || {
        CanonValue::for_record()
            .serialize_struct(super::RAW_VALUE_STRUCT, 1)
            .expect("raw struct")
    };
    assert!(make().end().is_err());
    assert!(make().serialize_field("other", &"null").is_err());
    assert!(make().serialize_field(super::RAW_VALUE_STRUCT, &7).is_err());
    assert!(make()
        .serialize_field(super::RAW_VALUE_STRUCT, &"invalid JSON")
        .is_err());

    let mut raw = make();
    raw.serialize_field(super::RAW_VALUE_STRUCT, &"null")
        .expect("one raw value");
    assert!(raw
        .serialize_field(super::RAW_VALUE_STRUCT, &"true")
        .is_err());
    assert_eq!(
        raw.end().expect("first payload remains intact").render(),
        "null"
    );
}
