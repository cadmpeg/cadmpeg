// SPDX-License-Identifier: Apache-2.0

use serde::ser::{SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant};
use serde::{Serialize, Serializer};

use super::CanonValue;
use crate::native::NativeNamespace;

#[test]
fn display_value_charges_formatted_chunks_and_formats_once() {
    use std::cell::Cell;
    use std::fmt::Write as _;

    struct DisplayText<'a>(&'a Cell<usize>);

    impl std::fmt::Display for DisplayText<'_> {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            self.0.set(self.0.get() + 1);
            formatter.write_str("line\n")?;
            formatter.write_str("quote\"")?;
            formatter.write_char('\u{1}')
        }
    }

    impl Serialize for DisplayText<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.collect_str(self)
        }
    }

    #[derive(Serialize)]
    struct Record<'a> {
        id: &'static str,
        value: DisplayText<'a>,
    }

    let calls = Cell::new(0);
    let ctx = crate::native::test_ctx();
    let record = super::super::NativeRecord::from_typed_for_decode(
        &ctx,
        &Record {
            id: "test:native:record#display",
            value: DisplayText(&calls),
        },
    )
    .expect("valid display record");
    assert_eq!(calls.get(), 1);
    assert_eq!(
        record.field("value"),
        Some(serde_json::json!("line\nquote\"\u{1}"))
    );
}

#[test]
fn display_map_key_charges_formatted_chunks_and_formats_once() {
    use std::cell::Cell;

    struct Key<'a>(&'a Cell<usize>);

    impl std::fmt::Display for Key<'_> {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            self.0.set(self.0.get() + 1);
            formatter.write_str("key\n")
        }
    }

    impl Serialize for Key<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.collect_str(self)
        }
    }

    struct Record<'a>(&'a Cell<usize>);

    impl Serialize for Record<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut map = serializer.serialize_map(Some(2))?;
            map.serialize_entry("id", "test:native:record#key")?;
            map.serialize_entry(&Key(self.0), &7)?;
            map.end()
        }
    }

    let calls = Cell::new(0);
    let ctx = crate::native::test_ctx();
    let record = super::super::NativeRecord::from_typed_for_decode(&ctx, &Record(&calls))
        .expect("valid key record");
    assert_eq!(calls.get(), 1);
    assert_eq!(record.field("key\n"), Some(serde_json::json!(7)));
}

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
            .set_arena(&crate::native::test_ctx(), "records", &[record("second")])
            .expect("distinct keys are legal");
        let before = namespace.clone();
        let error = namespace
            .set_arena(&crate::native::test_ctx(), "records", &[record("first")])
            .expect_err("duplicate fields must not collapse to their last value");
        assert!(error.to_string().contains("duplicate key first"), "{error}");
        assert_eq!(namespace, before);
    }
}

#[test]
fn malformed_map_protocol_is_reported() {
    let ctx = crate::native::test_ctx();
    let mut pending =
        serde::Serializer::serialize_map(CanonValue::for_record(&ctx), None).expect("map");
    pending.serialize_key("first").expect("first key");
    assert!(pending.serialize_key("second").is_err());
    assert!(SerializeMap::end(pending).is_err());

    let mut no_key =
        serde::Serializer::serialize_map(CanonValue::for_record(&ctx), None).expect("map");
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

    let ctx = crate::native::test_ctx();
    let mut sequence =
        serde::Serializer::serialize_seq(CanonValue::for_record(&ctx), None).expect("sequence");
    sequence.serialize_element(&1).expect("first element");
    assert!(sequence.serialize_element(&Refused).is_err());
    sequence.serialize_element(&2).expect("second element");
    assert_eq!(sequence.end().expect("sequence").render(), "[1,2]");
}

#[test]
fn raw_value_record_stores_the_parsed_json() {
    use serde_json::value::RawValue;

    #[derive(Serialize)]
    struct Record {
        id: &'static str,
        raw: Box<RawValue>,
    }

    let typed = Record {
        id: "test:native:record#raw-stream",
        raw: RawValue::from_string(r#"{ "text": "quoted \"value\"" }"#.to_owned())
            .expect("valid raw JSON"),
    };
    let ctx = crate::native::test_ctx();
    let stored =
        super::super::NativeRecord::from_typed_for_decode(&ctx, &typed).expect("valid raw record");
    assert_eq!(
        stored.field("raw"),
        Some(serde_json::json!({"text": "quoted \"value\""}))
    );
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
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_recursion_depth =
        cadmpeg_core::decode::u64_from_index(super::super::MAX_NATIVE_NESTING_DEPTH + 2);
    let (ctx, _) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
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
            .set_arena(&ctx, "records", &[record])
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
        .set_arena(&crate::native::test_ctx(), "records", &[raw])
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
            .set_arena(&ctx, "records", &[record])
            .expect_err("duplicate raw keys");
        assert!(error.to_string().contains("duplicate key a"), "{error}");
        assert_eq!(namespace, before);
    }

    let ordinary = serde_json::json!({"$serde_json::private::RawValue": "null"});
    namespace
        .set_arena(
            &crate::native::test_ctx(),
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

/// A member written through `collect_str` carries its `Display` text, as a
/// value and as a map key, and a `char` member carries its one character.
///
/// The whole record is compared against `serde_json::to_value` of the same
/// typed record, which is the value this serializer states it builds.
#[test]
fn a_display_member_writes_its_text_through_collect_str() {
    use crate::native::NativeRecord;

    struct Displayed(&'static str);

    impl std::fmt::Display for Displayed {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(formatter, "<{}>", self.0)
        }
    }

    impl Serialize for Displayed {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            serializer.collect_str(self)
        }
    }

    /// One member whose key arrives through `collect_str`.
    struct Keyed;

    impl Serialize for Keyed {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut map = serializer.serialize_map(Some(1))?;
            map.serialize_entry(&Displayed("key"), &1)?;
            map.end()
        }
    }

    #[derive(Serialize)]
    struct Record {
        id: &'static str,
        text: Displayed,
        escaped: Displayed,
        letter: char,
        keyed: Keyed,
    }

    let record = Record {
        id: "test:native:record#display",
        text: Displayed("plain"),
        escaped: Displayed("a \"quoted\" \\ and\ttab"),
        letter: '"',
        keyed: Keyed,
    };
    let stored = NativeRecord::from_typed(&record).expect("a Display member is an ordinary string");
    assert_eq!(stored.field("text"), Some(serde_json::json!("<plain>")));
    assert_eq!(
        stored.field("escaped"),
        Some(serde_json::json!("<a \"quoted\" \\ and\ttab>"))
    );
    assert_eq!(stored.field("letter"), Some(serde_json::json!("\"")));
    assert_eq!(stored.field("keyed"), Some(serde_json::json!({"<key>": 1})));
    assert_eq!(
        serde_json::to_value(&stored).expect("the stored record writes"),
        serde_json::to_value(&record).expect("the typed record writes")
    );
}

#[test]
fn malformed_raw_json_protocol_cannot_produce_a_native_value() {
    let ctx = crate::native::test_ctx();
    let make = || {
        CanonValue::for_record(&ctx)
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

#[test]
fn raw_native_resource_refusals_keep_the_caller_dimension() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use serde_json::value::RawValue;

    #[derive(Serialize)]
    struct Record {
        id: &'static str,
        raw: Box<RawValue>,
    }
    let id = "test:native:record#raw-limit";
    let json = r#"[["retained"]]"#;
    let record = Record {
        id,
        raw: RawValue::from_string(json.into()).unwrap(),
    };
    let arena = DecodeArena::new();
    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::default()).unwrap();
    let stored = super::super::NativeRecord::from_typed_for_decode(&service, &record).unwrap();
    assert_eq!(stored.field("raw"), Some(serde_json::json!([["retained"]])));
    for dimension in [
        ResourceDimension::CollectionItems,
        ResourceDimension::RetainedBytes,
        ResourceDimension::RecursionDepth,
    ] {
        let mut policy = DecodePolicy::default();
        match dimension {
            ResourceDimension::CollectionItems => policy.limits.max_collection_items = 2,
            ResourceDimension::RetainedBytes => {
                policy.limits.max_retained_bytes =
                    u64::try_from(2 + id.len() + 3 + json.len()).unwrap();
            }
            ResourceDimension::RecursionDepth => policy.limits.max_recursion_depth = 1,
            _ => panic!("unsupported raw-native test dimension"),
        }
        let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let error =
            super::super::NativeRecord::from_typed_for_decode(&limited, &record).unwrap_err();
        assert!(matches!(cadmpeg_core::CodecError::from(error),
            cadmpeg_core::CodecError::ResourceLimit(limit) if limit.dimension == dimension));
    }
}

#[test]
fn native_float_keys_keep_the_scalar_json_spelling() {
    struct Keyed<T>(T);
    impl<T: Serialize> Serialize for Keyed<T> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut map = serializer.serialize_map(Some(1))?;
            map.serialize_entry(&self.0, &7)?;
            map.end()
        }
    }
    #[derive(Serialize)]
    struct Record<T> {
        id: &'static str,
        keyed: Keyed<T>,
    }
    for value in [
        f64::MIN,
        -0.0,
        f64::MIN_POSITIVE,
        f64::from_bits(1),
        f64::MAX,
    ] {
        let record = Record {
            id: "test:native:record#float-key",
            keyed: Keyed(value),
        };
        let stored = super::super::NativeRecord::from_typed(&record).unwrap();
        assert_eq!(
            serde_json::to_value(&stored).unwrap(),
            serde_json::to_value(&record).unwrap()
        );
    }
    for value in [
        f32::MIN,
        -0.0,
        f32::MIN_POSITIVE,
        f32::from_bits(1),
        f32::MAX,
    ] {
        let record = Record {
            id: "test:native:record#float-key",
            keyed: Keyed(value),
        };
        let stored = super::super::NativeRecord::from_typed(&record).unwrap();
        assert_eq!(
            serde_json::to_value(&stored).unwrap(),
            serde_json::to_value(&record).unwrap()
        );
    }
}

#[test]
fn raw_native_replay_text_uses_scoped_storage() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use serde_json::value::RawValue;
    #[derive(Serialize)]
    struct Record {
        id: &'static str,
        raw: Box<RawValue>,
    }
    let json = r#"[["retained"]]"#;
    let record = Record {
        id: "test:native:record#raw-storage",
        raw: RawValue::from_string(json.into()).unwrap(),
    };
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_materialized_bytes = cadmpeg_core::decode::u64_from_index(json.len()) - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let error = super::super::NativeRecord::from_typed_for_decode(&limited, &record).unwrap_err();
    assert!(
        matches!(cadmpeg_core::CodecError::from(error), cadmpeg_core::CodecError::ResourceLimit(limit)
        if limit.dimension == ResourceDimension::MaterializedBytes && limit.operation == "serialize native record")
    );
    policy.limits.max_materialized_bytes += 1;
    let (exact, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let stored = super::super::NativeRecord::from_typed_for_decode(&exact, &record).unwrap();
    assert_eq!(stored.field("raw"), Some(serde_json::json!([["retained"]])));
    let _released = exact
        .reserve_scoped(
            policy.limits.max_materialized_bytes,
            "released raw replay text",
        )
        .unwrap();
}

#[test]
fn known_sequence_length_reserves_only_its_backing_slots() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes =
        cadmpeg_core::decode::u64_from_index(3 * std::mem::size_of::<serde_json::Value>());
    policy.limits.max_collection_items = 3;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let values = [1u32, 2, 3];
    let value = values
        .serialize(CanonValue::for_record(&ctx))
        .unwrap()
        .into_value();
    assert_eq!(value, serde_json::json!([1, 2, 3]));
    assert_eq!(value.as_array().unwrap().capacity(), 3);
}

#[test]
fn canonical_native_root_and_transparent_wrappers_use_session_depth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    for trigger in 0..4 {
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = match trigger {
            0 => CanonValue::for_record(&ctx)
                .serialize_map(Some(0))
                .map(|_| ()),
            1 => CanonValue::for_record(&ctx)
                .serialize_seq(Some(0))
                .map(|_| ()),
            2 => CanonValue::for_record(&ctx).serialize_some(&7).map(|_| ()),
            3 => CanonValue::for_record(&ctx)
                .serialize_newtype_struct("Wrapped", &7)
                .map(|_| ()),
            _ => unreachable!(),
        };
        let super::CanonError::Resource(cadmpeg_core::CodecError::ResourceLimit(first)) =
            result.err().unwrap()
        else {
            panic!("frame admission must refuse");
        };
        assert_eq!(first.dimension, ResourceDimension::RecursionDepth);
        assert_eq!(first.operation, super::WORK);
        assert_eq!(first.used, 0);
        assert_eq!(first.additional, 1);
        assert!(
            matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit == first)
        );
    }
}

#[test]
fn canonical_native_scalar_visits_preserve_the_first_work_refusal() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    for trigger in 0..4 {
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let result = match trigger {
            0 => CanonValue::for_record(&ctx).serialize_bool(true),
            1 => CanonValue::for_record(&ctx).serialize_f64(1.0),
            2 => CanonValue::for_record(&ctx).serialize_none(),
            3 => CanonValue::for_record(&ctx).serialize_unit(),
            _ => unreachable!(),
        };
        let super::CanonError::Resource(cadmpeg_core::CodecError::ResourceLimit(first)) =
            result.err().unwrap()
        else {
            panic!("scalar work admission must refuse");
        };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        assert_eq!(first.used, 0);
        assert_eq!(first.additional, 1);
        assert!(
            matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit == first)
        );
    }
}

#[test]
fn canonical_native_sequence_slots_are_admitted_before_allocation() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let super::CanonError::Resource(cadmpeg_core::CodecError::ResourceLimit(first)) =
        CanonValue::for_record(&ctx)
            .serialize_seq(Some(3))
            .err()
            .unwrap()
    else {
        panic!("sequence slots must refuse");
    };
    assert_eq!(first.dimension, ResourceDimension::CollectionItems);
    assert_eq!(first.additional, 3);
    assert!(
        matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit == first)
    );
}

#[test]
fn canonical_native_key_comparisons_admit_the_complete_key_bound() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use serde_json::{Map, Value};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_work_units = 0;
    for copied in [false, true] {
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut entries = Map::from_iter([("a".into(), Value::Null), ("b".into(), Value::Null)]);
        let error = if copied {
            let error = super::super::copy::insert(&ctx, &mut entries, String::new(), Value::Null)
                .unwrap_err();
            assert_eq!(entries.len(), 2);
            error
        } else {
            let mut map = super::CanonMap {
                ctx: &ctx,
                _nested: ctx.enter_nested(super::WORK).unwrap(),
                entries,
                key: None,
                depth: super::MAX_NATIVE_NESTING_DEPTH,
            };
            let super::CanonError::Resource(error) = map.insert(String::new(), &7).unwrap_err()
            else {
                panic!("scalar admission must refuse");
            };
            assert_eq!(map.entries.len(), 2);
            error
        };
        let cadmpeg_core::CodecError::ResourceLimit(first) = error else {
            panic!("work must refuse");
        };
        assert_eq!(first.dimension, ResourceDimension::WorkUnits);
        // Empty keys cost no comparison bytes. Canonical construction next
        // visits one scalar; copying next admits one insertion node pass.
        let node_bytes = 11 * (std::mem::size_of::<String>() + std::mem::size_of::<Value>())
            + 16 * std::mem::size_of::<usize>()
            + 2 * std::mem::align_of::<String>()
                .max(std::mem::align_of::<Value>())
                .max(std::mem::align_of::<usize>());
        assert_eq!(
            first.additional,
            if copied {
                cadmpeg_core::decode::u64_from_index(node_bytes)
            } else {
                1
            }
        );
        assert!(
            matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit == first)
        );
    }
}

#[test]
fn native_map_insertions_admit_search_paths_before_lookup() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use serde_json::{Map, Value};
    for copied in [false, true] {
        let run = |cap| {
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = cap;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
            let entries = (0..1000)
                .map(|i| (format!("key-{i:04}"), Value::Null))
                .collect::<Map<_, _>>();
            if copied {
                let mut entries = entries;
                super::super::copy::insert(&ctx, &mut entries, "key-0999".into(), Value::Null)
            } else {
                let mut map = super::CanonMap {
                    ctx: &ctx,
                    _nested: ctx.enter_nested(super::WORK)?,
                    entries,
                    key: None,
                    depth: super::MAX_NATIVE_NESTING_DEPTH,
                };
                match map.insert("key-0999".into(), &7) {
                    Err(super::CanonError::Resource(error)) => Err(error),
                    Err(super::CanonError::Message(message)) => {
                        assert_eq!(message, "duplicate key key-0999");
                        Ok(())
                    }
                    _ => panic!("duplicate key must refuse"),
                }
            }
        };
        let operation = if copied {
            "insert copied native field"
        } else {
            super::WORK
        };
        let error = cadmpeg_test_support::refusal::resource_limit_at(
            ResourceDimension::WorkUnits,
            operation,
            run,
        );
        let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
            panic!("search path refusal");
        };
        // ceil(1000/2)=500; 1+ilog6(500)=4 levels, 11 comparisons each.
        assert_eq!(limit.additional, 8 * 11 * 4);
        run(u64::MAX).unwrap();
    }
}

#[test]
fn canonical_native_unknown_sequence_slot_refuses_before_the_child_producer() {
    use std::cell::Cell;

    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    struct Child<'a>(&'a Cell<usize>);
    impl Serialize for Child<'_> {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            self.0.set(self.0.get() + 1);
            serializer.serialize_str("retained child")
        }
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 0;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let calls = Cell::new(0);
    let mut sequence = CanonValue::for_record(&ctx).serialize_seq(None).unwrap();
    let super::CanonError::Resource(cadmpeg_core::CodecError::ResourceLimit(original)) =
        sequence.serialize_element(&Child(&calls)).unwrap_err()
    else {
        panic!("destination slot must refuse");
    };
    assert_eq!(original.dimension, ResourceDimension::CollectionItems);
    assert_eq!(original.operation, super::STORAGE);
    assert_eq!(original.used, 0);
    assert_eq!(original.additional, 1);
    assert_eq!(calls.get(), 0);
    assert!(sequence.out.is_empty());
    assert_eq!(sequence.unfilled, 0);
    let super::CanonError::Resource(cadmpeg_core::CodecError::ResourceLimit(repeated)) =
        sequence.serialize_element(&Child(&calls)).unwrap_err()
    else {
        panic!("original refusal must remain");
    };
    assert_eq!(repeated, original);
    assert_eq!(calls.get(), 0);
    drop(sequence);
    assert!(
        matches!(ctx.finish_session(), Err(cadmpeg_core::CodecError::ResourceLimit(limit)) if limit == original)
    );
}

#[test]
fn canonical_native_unknown_sequence_keeps_the_reserved_slot_after_child_error() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    struct Refused;
    impl Serialize for Refused {
        fn serialize<S: Serializer>(&self, _: S) -> Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("fixture rejects this element"))
        }
    }
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_collection_items = 2;
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut sequence = CanonValue::for_record(&ctx).serialize_seq(None).unwrap();
    sequence.serialize_element(&1).unwrap();
    assert_eq!(
        sequence
            .serialize_element(&Refused)
            .unwrap_err()
            .to_string(),
        "fixture rejects this element"
    );
    assert_eq!(sequence.out, vec![serde_json::json!(1)]);
    assert_eq!(sequence.unfilled, 1);
    sequence
        .serialize_element(&2)
        .expect("retry reuses the admitted slot");
    assert_eq!(sequence.unfilled, 0);
    assert_eq!(sequence.end().unwrap().render(), "[1,2]");
    ctx.finish_session().unwrap();
}

#[test]
fn non_finite_path_steps_release_scoped_storage() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    use cadmpeg_core::CodecError;
    let key = "k".repeat(64);
    let run = |cap| {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        policy.limits.max_materialized_bytes = cap;
        // Only the final 64-byte path text remains retained.
        policy.limits.max_retained_bytes = 64;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy)?;
        let error = super::number(&ctx, f64::NAN)
            .err()
            .unwrap()
            .within(&ctx, || super::copy_text(&ctx, &key).map(super::Step::Key))
            .into_native(&ctx);
        match error {
            crate::native::NativeConvertError::NonFiniteNumber { field } => {
                assert_eq!(field, key);
                drop(ctx.reserve_scoped(cap, "released non-finite path steps")?);
                ctx.finish_session()
            }
            error => Err(CodecError::from(error)),
        }
    };
    cadmpeg_test_support::refusal::resource_limit_at(
        ResourceDimension::MaterializedBytes,
        "serialize native record",
        run,
    );
    run(1024).unwrap();
}

#[test]
fn raw_scalar_does_not_pay_for_unused_container_depth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};
    use serde::ser::SerializeStruct;
    for depth in [0, super::MAX_NATIVE_NESTING_DEPTH] {
        let arena = DecodeArena::new();
        let mut policy = DecodePolicy::service();
        // Serialize the one-byte text: one visit + one copy byte.
        // Replay: one text byte + one scalar visit. No container is entered.
        policy.limits.max_work_units = 1 + 1 + 1 + 1;
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut raw = super::CanonStruct::Raw {
            ctx: &ctx,
            depth,
            parsed: None,
        };
        raw.serialize_field(super::RAW_VALUE_STRUCT, &"7").unwrap();
        assert_eq!(raw.end().unwrap().render(), "7");
        ctx.finish_session().unwrap();
    }
}

#[test]
fn raw_replay_duplicate_does_not_admit_unread_object_or_array_members() {
    use cadmpeg_core::decode::{u64_from_index, DecodeArena, DecodeContext, DecodePolicy};
    use serde_json::Value;

    let node_bytes = 11 * (std::mem::size_of::<String>() + std::mem::size_of::<Value>())
        + 16 * std::mem::size_of::<usize>()
        + 2 * std::mem::align_of::<String>()
            .max(std::mem::align_of::<Value>())
            .max(std::mem::align_of::<usize>());
    // One map visit, two key copies, one scalar visit, the first insertion's
    // three node passes, one duplicate comparison and one duplicate-key scan,
    // then two text passes to format the duplicate message.
    let native_work = 1 + 2 + 1 + 3 * node_bytes + 1 + 1 + 2 * "duplicate key a".len();
    // Two three-byte raw-key scans; only the first one-byte value is replayed.
    // The duplicate refuses before its value's Replay serializer is called.
    let member_work = 2 * 3 + 1;
    for tail in ["7".to_owned(), "[7]".repeat(1024).replace("][", "],[")] {
        for array in [false, true] {
            let (json, parent_work) = if array {
                (
                    format!(r#"[{{"a":7,"a":8}},[{tail}]]"#),
                    r#"{"a":7,"a":8}"#.len() + 1 + 1,
                )
            } else {
                (format!(r#"{{"a":7,"a":8,"tail":[{tail}]}}"#), 0)
            };
            // The root text pass pays the initial parser. Additional replay
            // charges cover only members read before the duplicate refuses.
            let work = u64_from_index(json.len() + native_work + member_work + parent_work);
            let arena = DecodeArena::new();
            let mut policy = DecodePolicy::service();
            policy.limits.max_work_units = work;
            let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
            ctx.charge_work(u64_from_index(json.len()), super::WORK)
                .unwrap();
            let error = super::super::replay::emit(
                &json,
                super::CanonValue::for_record(&ctx),
                super::MAX_NATIVE_NESTING_DEPTH,
                &ctx,
            )
            .err()
            .expect("duplicate key must refuse");
            assert!(error.to_string().contains("duplicate key a"), "{error}");
            drop(error);
            ctx.finish_session().unwrap();
        }
    }
}
