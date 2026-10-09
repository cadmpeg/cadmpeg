// SPDX-License-Identifier: Apache-2.0

use std::cell::Cell;

use serde::ser::SerializeMap;
use serde::{Serialize, Serializer};
use serde_json::Value;

use super::{NativeConvertError, NativeNamespace, MAX_NATIVE_NESTING_DEPTH};

#[derive(Serialize)]
struct Record<'a> {
    id: &'a str,
    value: u32,
}

#[test]
fn standard_native_write_sorts_records_and_keeps_equal_identity_order() {
    let records = [
        Record { id: "test:native:record#z", value: 3 },
        Record { id: "test:native:record#a", value: 1 },
        Record { id: "test:native:record#a", value: 2 },
    ];
    let mut namespace = NativeNamespace::default();
    namespace.set_arena_standard("records", &records).expect("Standard records");
    let stored = &namespace.arenas()["records"];
    assert_eq!(stored.iter().map(|record| record.id()).collect::<Vec<_>>(), [
        "test:native:record#a", "test:native:record#a", "test:native:record#z",
    ]);
    assert_eq!(stored.iter().map(|record| record.field("value")).collect::<Vec<_>>(), [
        Some(Value::from(1)), Some(Value::from(2)), Some(Value::from(3)),
    ]);
}

#[test]
fn standard_native_write_stops_at_the_original_producer_error() {
    struct Producer(usize);
    impl Serialize for Producer {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            if self.0 == 1 {
                Err(serde::ser::Error::custom("producer refuses this record"))
            } else {
                Record { id: "test:native:record#offered", value: 9 }.serialize(serializer)
            }
        }
    }
    let mut namespace = NativeNamespace::default();
    namespace.set_arena_standard("records", &[Record { id: "test:native:record#kept", value: 7 }]).expect("initial arena");
    let before = namespace.clone();
    let visited = Cell::new(0);
    let records = (0..3).map(|ordinal| { visited.set(visited.get() + 1); Producer(ordinal) });
    let error = namespace.set_arena_from_standard("records", records).expect_err("producer error");
    let NativeConvertError::Arena { arena, source } = error else { panic!("arena location"); };
    assert_eq!(arena, "records");
    let NativeConvertError::WriteRecord { ordinal, source } = *source else { panic!("record ordinal"); };
    assert_eq!(ordinal, 1);
    assert_eq!(source.to_string(), "native record conversion failed: producer refuses this record");
    assert_eq!(visited.get(), 2);
    assert_eq!(namespace, before);
}

#[test]
fn standard_native_write_keeps_decode_conversion_errors_and_existing_arena() {
    let mut namespace = NativeNamespace::default();
    namespace.set_arena_standard("records", &[Record { id: "test:native:record#kept", value: 7 }]).expect("initial arena");
    for value in [
        Value::from(7),
        serde_json::json!({"value":7}),
        serde_json::json!({"id":7}),
        serde_json::json!({"id":"invalid identity"}),
    ] {
        let before = namespace.clone();
        let mut decoded = before.clone();
        let ordinary = namespace.set_arena_standard("records", std::slice::from_ref(&value)).expect_err("conversion refuses");
        let contextual = decoded.set_arena(&super::test_ctx(), "records", &[value]).expect_err("decode conversion refuses");
        assert_eq!(ordinary.to_string(), contextual.to_string());
        assert_eq!(namespace, before);
        assert_eq!(decoded, before);
    }
}

#[test]
fn standard_native_write_reports_the_same_nonfinite_member_path() {
    #[derive(Serialize)]
    struct Number { value: f64 }
    #[derive(Serialize)]
    struct Numbers { id: &'static str, nested: Vec<Number> }
    let record = Numbers { id: "test:native:record#number", nested: vec![Number { value: f64::NAN }] };
    let mut namespace = NativeNamespace::default();
    let error = namespace.set_arena_standard("records", &[record]).expect_err("NaN cannot enter a stored field");
    let NativeConvertError::Arena { source, .. } = error else { panic!("arena location"); };
    let NativeConvertError::WriteRecord { ordinal, source } = *source else { panic!("record ordinal"); };
    assert_eq!(ordinal, 0);
    let NativeConvertError::NonFiniteNumber { field } = *source else { panic!("finite number rule"); };
    assert_eq!(field, "nested[0].value");
    assert!(namespace.arenas().is_empty());
}

#[test]
fn standard_native_write_refuses_duplicate_keys_before_reading_their_values() {
    struct Probe<'a>(&'a Cell<usize>);
    impl Serialize for Probe<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            self.0.set(self.0.get() + 1);
            serializer.serialize_u32(7)
        }
    }
    struct Duplicate<'a>(&'a Cell<usize>);
    impl Serialize for Duplicate<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut map = serializer.serialize_map(Some(3))?;
            map.serialize_entry("id", "test:native:record#duplicate")?;
            map.serialize_entry("value", &Probe(self.0))?;
            map.serialize_entry("value", &Probe(self.0))?;
            map.end()
        }
    }
    let calls = Cell::new(0);
    let mut namespace = NativeNamespace::default();
    let error = namespace.set_arena_standard("records", &[Duplicate(&calls)]).expect_err("duplicate field");
    assert!(error.to_string().contains("duplicate key value"));
    assert_eq!(calls.get(), 1);
    assert!(namespace.arenas().is_empty());
}

#[test]
fn standard_native_write_keeps_the_stored_container_bound() {
    #[derive(Serialize)]
    struct Nested { id: &'static str, value: Value }
    for containers in [MAX_NATIVE_NESTING_DEPTH, MAX_NATIVE_NESTING_DEPTH + 1] {
        let mut value = Value::from(7);
        for _ in 0..containers { value = Value::Array(vec![value]); }
        let mut namespace = NativeNamespace::default();
        let result = namespace.set_arena_standard("records", &[Nested { id: "test:native:record#nested", value }]);
        if containers == MAX_NATIVE_NESTING_DEPTH {
            result.expect("the native field bound is legal in Standard mode");
            assert_eq!(namespace.arenas()["records"].len(), 1);
        } else {
            let error = result.expect_err("one extra field container");
            assert!(error.to_string().contains(&format!("native value nests deeper than {MAX_NATIVE_NESTING_DEPTH} containers")));
            assert!(namespace.arenas().is_empty());
        }
    }
}

#[test]
fn standard_native_write_replays_raw_values_with_duplicate_and_depth_rules() {
    #[derive(Serialize)]
    struct Raw { id: &'static str, value: Box<serde_json::value::RawValue> }
    let mut namespace = NativeNamespace::default();
    let json = format!("{}7{}", "[".repeat(140), "]".repeat(140));
    namespace.set_arena_standard("records", &[Raw {
        id: "test:native:record#raw",
        value: serde_json::value::RawValue::from_string(json.clone()).expect("raw syntax"),
    }]).expect("raw fields can exceed the text reader's aggregate recursion limit");
    assert_eq!(namespace.arenas()["records"][0].field("value").expect("stored raw field").to_string(), json);
    let before = namespace.clone();
    let error = namespace.set_arena_standard("records", &[Raw {
        id: "test:native:record#raw",
        value: serde_json::value::RawValue::from_string(r#"{"key":1,"key":2}"#.into()).expect("raw syntax retains duplicate keys"),
    }]).expect_err("raw duplicate keys");
    assert!(error.to_string().contains("duplicate key key"));
    assert_eq!(namespace, before);
}
