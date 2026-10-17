// SPDX-License-Identifier: Apache-2.0

use serde::ser::{SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant};
use serde::{Serialize, Serializer};

use super::{CanonText, Written};
use crate::native::NativeNamespace;

/// The text a serializer wrote into `out`, with the object it returned placed.
fn render(written: Written, mut out: Vec<u8>) -> String {
    written.place(&mut out);
    String::from_utf8(out).expect("canonical text")
}

#[test]
fn display_value_formats_once() {
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
fn display_map_key_formats_once() {
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
    let mut out = Vec::new();
    let mut pending =
        serde::Serializer::serialize_map(CanonText::for_record(&ctx, &mut out), None).expect("map");
    pending.serialize_key("first").expect("first key");
    assert!(pending.serialize_key("second").is_err());
    assert!(SerializeMap::end(pending).is_err());

    let mut no_key =
        serde::Serializer::serialize_map(CanonText::for_record(&ctx, &mut out), None).expect("map");
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
    let mut out = Vec::new();
    let mut sequence =
        serde::Serializer::serialize_seq(CanonText::for_record(&ctx, &mut out), None)
            .expect("sequence");
    sequence.serialize_element(&1).expect("first element");
    assert!(sequence.serialize_element(&Refused).is_err());
    sequence.serialize_element(&2).expect("second element");
    let written = sequence.end().expect("sequence");
    assert_eq!(render(written, out), "[1,2]");
}

#[test]
fn raw_value_is_stored_as_canonical_json() {
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
    fn make<'a, 'o>(
        ctx: &'a cadmpeg_core::decode::DecodeContext<'a>,
        out: &'o mut Vec<u8>,
    ) -> super::CanonStruct<'a, 'o> {
        CanonText::for_record(ctx, out)
            .serialize_struct(super::RAW_VALUE_STRUCT, 1)
            .expect("raw struct")
    }
    let ctx = crate::native::test_ctx();
    let mut out = Vec::new();
    assert!(make(&ctx, &mut out).end().is_err());
    assert!(make(&ctx, &mut out)
        .serialize_field("other", &"null")
        .is_err());
    assert!(make(&ctx, &mut out)
        .serialize_field(super::RAW_VALUE_STRUCT, &7)
        .is_err());
    assert!(make(&ctx, &mut out)
        .serialize_field(super::RAW_VALUE_STRUCT, &"invalid JSON")
        .is_err());
    assert!(out.is_empty(), "a refused payload leaves no text behind");

    let mut raw = make(&ctx, &mut out);
    raw.serialize_field(super::RAW_VALUE_STRUCT, &"null")
        .expect("one raw value");
    assert!(raw
        .serialize_field(super::RAW_VALUE_STRUCT, &"true")
        .is_err());
    let written = raw.end().expect("first payload remains intact");
    assert_eq!(render(written, out), "null");
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
fn canonical_native_root_and_transparent_wrappers_use_session_depth() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_recursion_depth = 0;
    for trigger in 0..4 {
        let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
        let mut out = Vec::new();
        let canon = CanonText::for_record(&ctx, &mut out);
        let result = match trigger {
            0 => canon.serialize_map(Some(0)).map(|_| ()),
            1 => canon.serialize_seq(Some(0)).map(|_| ()),
            2 => canon.serialize_some(&7).map(|_| ()),
            3 => canon.serialize_newtype_struct("Wrapped", &7).map(|_| ()),
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

/// Members are placed in key order at every depth whatever order they arrive
/// in, and the record's `id` member leads. The oracle is the `serde_json`
/// text of the record's `serde_json::to_value` tree behind a leading `id`.
#[test]
fn members_are_placed_in_key_order_however_they_arrive() {
    use std::collections::BTreeMap;

    use crate::native::NativeRecord;

    #[derive(Serialize)]
    struct Leading {
        id: &'static str,
        value: u8,
    }

    #[derive(Serialize)]
    struct Before {
        alpha: u8,
        id: &'static str,
    }

    #[derive(Serialize)]
    struct Reversed {
        zulu: u8,
        yankee: [u8; 2],
        alpha: Option<u8>,
    }

    #[derive(Serialize)]
    enum Tagged {
        Fields { zulu: u8, alpha: u8 },
    }

    struct Descending;

    impl Serialize for Descending {
        fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
            let mut map = serializer.serialize_map(None)?;
            for key in ["c", "b", "a"] {
                map.serialize_entry(
                    key,
                    &Reversed {
                        zulu: 1,
                        yankee: [2, 3],
                        alpha: None,
                    },
                )?;
            }
            map.end()
        }
    }

    #[derive(Serialize)]
    struct Nested {
        zulu: Vec<Reversed>,
        id: &'static str,
        map: Descending,
        tagged: Tagged,
        raw: Box<serde_json::value::RawValue>,
        sorted: BTreeMap<&'static str, Leading>,
    }

    fn oracle<T: Serialize>(record: &T) -> String {
        let serde_json::Value::Object(mut fields) = serde_json::to_value(record).unwrap() else {
            panic!("record serializes as an object");
        };
        let id = fields.remove("id").unwrap();
        let mut text = format!("{{\"id\":{id}");
        for (key, value) in fields {
            text.push(',');
            text.push_str(&serde_json::Value::String(key).to_string());
            text.push(':');
            text.push_str(&value.to_string());
        }
        text.push('}');
        text
    }

    fn stored<T: Serialize>(record: &T) -> String {
        serde_json::to_string(&NativeRecord::from_typed(record).unwrap()).unwrap()
    }

    let leading = Leading {
        id: "test:native:record#leading",
        value: 7,
    };
    assert_eq!(stored(&leading), oracle(&leading));
    let before = Before {
        alpha: 7,
        id: "test:native:record#before",
    };
    assert_eq!(stored(&before), oracle(&before));
    let nested = Nested {
        zulu: vec![
            Reversed {
                zulu: 1,
                yankee: [2, 3],
                alpha: Some(4),
            },
            Reversed {
                zulu: 5,
                yankee: [6, 7],
                alpha: None,
            },
        ],
        id: "test:native:record#nested",
        map: Descending,
        tagged: Tagged::Fields { zulu: 1, alpha: 2 },
        raw: serde_json::value::RawValue::from_string(
            r#"{"z":{"y":1,"x":[{"b":1,"a":2}]},"a":0}"#.to_owned(),
        )
        .unwrap(),
        sorted: BTreeMap::from([(
            "k",
            Leading {
                id: "inner",
                value: 1,
            },
        )]),
    };
    assert_eq!(stored(&nested), oracle(&nested));
}

/// Writes its text in pieces of `piece` characters.
struct Pieces<'a> {
    text: &'a str,
    piece: usize,
}

impl std::fmt::Display for Pieces<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut rest = self.text;
        while !rest.is_empty() {
            let end = rest
                .char_indices()
                .nth(self.piece)
                .map_or(rest.len(), |(at, _)| at);
            formatter.write_str(&rest[..end])?;
            rest = &rest[end..];
        }
        Ok(())
    }
}

/// `collect_str` writes the text `serde_json` writes for the same string,
/// for every `char` and however the `Display` value divides its text.
#[test]
fn display_text_is_escaped_as_serde_json_escapes_the_whole_string() {
    let every_char = (0..=u32::from(char::MAX))
        .filter_map(char::from_u32)
        .collect::<String>();
    let ctx = crate::native::test_ctx();
    for piece in [1, 2, 3, 7, 64, 512, usize::MAX] {
        let mut out = b"prefix".to_vec();
        let written = CanonText::for_record(&ctx, &mut out)
            .collect_str(&Pieces {
                text: &every_char,
                piece,
            })
            .expect("text writes");
        assert!(matches!(written, Written::Text));
        let mut expected = b"prefix".to_vec();
        serde_json::to_writer(&mut expected, &every_char).unwrap();
        assert!(out == expected, "pieces of {piece} characters");
    }
}

/// A `Display` value that fails leaves no text behind and states the
/// refusal the collected text stated.
#[test]
fn a_failing_display_value_leaves_no_text() {
    struct Failing;

    impl std::fmt::Display for Failing {
        fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("partial \"text\"")?;
            Err(std::fmt::Error)
        }
    }

    let ctx = crate::native::test_ctx();
    let mut out = b"prefix".to_vec();
    let Err(error) = CanonText::for_record(&ctx, &mut out).collect_str(&Failing) else {
        panic!("a failing Display value is refused");
    };
    assert_eq!(error.to_string(), "a formatting error occurred");
    assert_eq!(out, b"prefix");
}

/// The member rewrite as it was before it moved the longest member in place:
/// the body is split off and every member is copied back in order.
fn reorder_by_copy(out: &mut Vec<u8>, body_start: usize, order: &[std::ops::Range<usize>]) {
    let body = out.split_off(body_start);
    for (index, text) in order.iter().enumerate() {
        if index > 0 {
            out.push(b',');
        }
        out.extend_from_slice(&body[text.start - body_start..text.end - body_start]);
    }
}

/// Every arrival order of up to five members, each with lengths that make
/// each member in turn the longest, ties included, rewrites to the text the
/// split-off copy wrote.
#[test]
fn in_place_member_reorder_matches_the_copying_rewrite() {
    fn permutations(items: &mut Vec<usize>, at: usize, each: &mut impl FnMut(&[usize])) {
        if at == items.len() {
            each(items);
            return;
        }
        for index in at..items.len() {
            items.swap(at, index);
            permutations(items, at + 1, each);
            items.swap(at, index);
        }
    }

    let lengths = [1, 2, 5, 5, 40];
    for count in 1..=lengths.len() {
        for rotation in 0..lengths.len() {
            let sizes = (0..count)
                .map(|member| lengths[(member + rotation) % lengths.len()])
                .collect::<Vec<_>>();
            let mut arrival = (0..count).collect::<Vec<_>>();
            permutations(&mut arrival, 0, &mut |arrival| {
                // Member `m` in key order arrives at position `arrival[m]`.
                let prefix = b"pre{".to_vec();
                let mut out = prefix.clone();
                let mut texts = vec![0..0; count];
                for position in 0..count {
                    let member = arrival.iter().position(|at| *at == position).unwrap();
                    if position > 0 {
                        out.push(b',');
                    }
                    let start = out.len();
                    let byte = b'a' + u8::try_from(member).unwrap();
                    out.extend(std::iter::repeat_n(byte, sizes[member]));
                    texts[member] = start..out.len();
                }
                let mut expected = out.clone();
                reorder_by_copy(&mut expected, prefix.len(), &texts);
                let body = prefix.len()..out.len();
                super::reorder(&mut out, body, texts.iter());
                assert_eq!(out, expected, "sizes {sizes:?}, arrival {arrival:?}");
            });
        }
    }
}

/// A record whose long byte payload arrives before shorter members that
/// sort ahead of it keeps the text the value tree states.
#[test]
fn a_long_payload_member_keeps_canonical_order() {
    #[derive(Serialize)]
    struct Segment {
        id: &'static str,
        zulu: u8,
        data: crate::native::bytes::NativeBytes<Vec<u8>>,
        byte_offset: u64,
        alpha: &'static str,
    }

    let segment = Segment {
        id: "test:native:record#segment",
        zulu: 1,
        data: (0..=u8::MAX)
            .cycle()
            .take(100_000)
            .collect::<Vec<_>>()
            .into(),
        byte_offset: 7,
        alpha: "a",
    };
    let stored = crate::native::NativeRecord::from_typed(&segment).unwrap();
    let serde_json::Value::Object(mut fields) = serde_json::to_value(&segment).unwrap() else {
        panic!("record serializes as an object");
    };
    let id = fields.remove("id").unwrap();
    let mut expected = format!("{{\"id\":{id}");
    for (key, value) in fields {
        expected.push(',');
        expected.push_str(&serde_json::Value::String(key).to_string());
        expected.push(':');
        expected.push_str(&value.to_string());
    }
    expected.push('}');
    assert_eq!(serde_json::to_string(&stored).unwrap(), expected);
}
