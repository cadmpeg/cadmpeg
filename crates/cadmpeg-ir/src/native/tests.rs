// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(clippy::disallowed_methods)]

use std::collections::BTreeMap;

use serde::Serialize;

use crate::diff;
use crate::examples::unit_cube;
use crate::native::NativeRecord;
use crate::validate::validate_neutral;

#[test]
fn typed_native_read_errors_identify_the_arena_and_stored_record() {
    use crate::native::NativeConvertError;
    use std::error::Error;

    #[derive(Debug, serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Record {
        id: crate::ids::Identity,
        value: u32,
    }

    let first = "test:native:record#first";
    let refused = "test:native:record#refused";
    let mut document = crate::CadIr::empty();
    document
        .native
        .namespace_mut("future")
        .set_arena(
            "records",
            &[
                serde_json::json!({"id": first, "value": 7}),
                serde_json::json!({"id": refused, "value": "not an integer"}),
            ],
        )
        .unwrap();
    let document: crate::CadIr =
        serde_json::from_value(serde_json::to_value(document).unwrap()).unwrap();
    let namespace = document.native.namespace("future").unwrap();
    // The iterator borrows the stored arena key, so a temporary lookup name
    // does not have to survive the iterator.
    let mut records = namespace.arena_iter_as::<Record>(&String::from("records"));
    let record = records.next().unwrap().unwrap();
    assert_eq!(record.id.as_str(), first);
    assert_eq!(record.value, 7);
    let error = records.next().unwrap().unwrap_err();
    assert!(error.to_string().contains("records"));
    assert!(error.to_string().contains(refused));
    assert!(error.source().unwrap().source().is_some());
    let NativeConvertError::Arena { arena, source } = error else {
        panic!("arena context")
    };
    assert_eq!(arena, "records");
    let NativeConvertError::ReadRecord { id, source } = *source else {
        panic!("record context")
    };
    assert_eq!(id.as_str(), refused);
    assert!(source.is_data());
    assert!(records.next().is_none());
    assert!(namespace.arena_iter_as::<Record>("absent").next().is_none());
}

#[test]
fn typed_native_write_errors_identify_input_ordinal_without_replacing_the_arena() {
    use crate::native::{NativeConvertError, NativeNamespace};
    let good = serde_json::json!({"id": "test:native:record#first", "value": 7});
    let mut namespace = NativeNamespace::default();
    namespace
        .set_arena("records", std::slice::from_ref(&good))
        .unwrap();
    let before = namespace.clone();
    let error = namespace
        .set_arena("records", &[good, serde_json::json!({"value": 8})])
        .unwrap_err();
    assert!(error.to_string().contains("records"));
    assert!(error.to_string().contains("ordinal 1"));
    let NativeConvertError::Arena { arena, source } = error else {
        panic!("arena context")
    };
    assert_eq!(arena, "records");
    let NativeConvertError::WriteRecord { ordinal, source } = *source else {
        panic!("input ordinal")
    };
    assert_eq!(ordinal, 1);
    assert!(matches!(*source, NativeConvertError::MissingId));
    assert_eq!(namespace, before);

    let source_error = NativeConvertError::InvalidOwner("producer record at offset 42".into());
    let result = crate::native::arena_from([Err::<serde_json::Value, _>(source_error)]);
    assert!(
        matches!(result, Err(NativeConvertError::InvalidOwner(message)) if message == "producer record at offset 42")
    );
}

#[test]
fn native_loss_counts_carry_a_nonempty_arena_population() {
    let mut native = crate::native::Native::default();
    native
        .namespace_mut("future")
        .arenas_mut()
        .insert("empty".into(), Vec::new());
    native.namespace_mut("future").arenas_mut().insert(
        "records".into(),
        vec![NativeRecord::new("test:native:record#counted", serde_json::Map::new()).unwrap()],
    );
    let counts = native.loss_counts();
    assert_eq!(counts.len(), 1);
    assert_eq!(counts[0].format, "future");
    assert_eq!(counts[0].kind, "records");
    assert_eq!(counts[0].count.get(), 1);
    let wire = serde_json::to_value(&counts[0]).unwrap();
    assert_eq!(
        serde_json::from_value::<crate::native::LossCount>(wire.clone()).unwrap(),
        counts[0]
    );
    let mut empty = wire;
    empty["count"] = serde_json::json!(0);
    assert!(serde_json::from_value::<crate::native::LossCount>(empty).is_err());
}

#[test]
fn complete_document_refuses_duplicate_native_keys_at_every_depth() {
    let empty = serde_json::to_string(&crate::CadIr::empty()).unwrap();
    assert_eq!(empty.matches("\"native\":{}").count(), 1);
    let document = |native: &str| empty.replace("\"native\":{}", &format!("\"native\":{native}"));
    let control = r#"{"future":{"records":[{"id":"test:native:record#first","fields":{"name":"value"}}]},"another_future":{"different_records":[]}}"#;
    let wire = document(control);
    let admitted = crate::CadIr::from_json(&wire).unwrap();
    assert_eq!(
        serde_json::to_value(&admitted).unwrap(),
        serde_json::from_str::<serde_json::Value>(&wire).unwrap(),
    );
    for (native, duplicate) in [
        (
            r#"{"future":{"records":[]},"future":{"other":[]}}"#,
            "future",
        ),
        (r#"{"future":{"records":[],"records":[]}}"#, "records"),
        (
            r#"{"future":{"records":[{"id":"test:native:record#first","id":"test:native:record#second"}]}}"#,
            "id",
        ),
        (
            r#"{"future":{"records":[{"id":"test:native:record#first","name":1,"name":2}]}}"#,
            "name",
        ),
        (
            r#"{"future":{"records":[{"id":"test:native:record#first","fields":{"name":1,"name":2}}]}}"#,
            "name",
        ),
        (
            r#"{"future":{"records":[{"id":"test:native:record#first","fields":[{"name":1,"name":2}]}]}}"#,
            "name",
        ),
    ] {
        let error = crate::CadIr::from_json(&document(native)).unwrap_err();
        assert!(
            error
                .to_string()
                .contains(&format!("duplicate key {duplicate}")),
            "{error}"
        );
    }
}

#[test]
fn deeply_nested_native_values_survive_every_stored_record_reader() {
    #[derive(Debug, PartialEq, Serialize, serde::Deserialize)]
    struct Record {
        id: String,
        nested: serde_json::Value,
    }

    let mut nested = serde_json::json!({"value": "retained", "absent": null});
    for _ in 0..140 {
        nested = serde_json::Value::Array(vec![nested]);
    }
    let id = "test:native:record#deep";
    let fields = serde_json::Map::from_iter([("nested".to_owned(), nested.clone())]);
    let constructed = NativeRecord::new(id, fields.clone()).unwrap();
    let typed = Record {
        id: id.into(),
        nested: nested.clone(),
    };
    assert_eq!(NativeRecord::from_typed(&typed).unwrap(), constructed);
    assert_eq!(constructed.to_typed::<Record>().unwrap(), typed);

    let mut document = crate::CadIr::empty();
    document
        .native
        .namespace_mut("future")
        .arenas_mut()
        .insert("records".into(), vec![constructed]);
    let wire = serde_json::to_value(&document).unwrap();
    let admitted = serde_json::from_value::<crate::CadIr>(wire).unwrap();
    let record = &admitted.native.namespace("future").unwrap().arenas()["records"][0];
    assert_eq!(record.fields(), fields);
    assert_eq!(record.field("nested"), Some(nested));
    assert_eq!(record.field("missing"), None);
    assert_eq!(record.field("id"), None);
    assert_eq!(record.to_typed::<Record>().unwrap(), typed);
    assert_eq!(
        serde_json::to_string(&admitted).unwrap(),
        serde_json::to_string(&document).unwrap()
    );
}

#[test]
fn native_identity_admission_is_shared_by_all_construction_paths() {
    #[derive(Serialize)]
    struct Record<'a> {
        id: &'a str,
    }

    for id in [
        "",
        "f3d:record#1",
        "f3d:test:record#",
        "f3d:test:record#1 space",
        "f3d:test:record#1#2",
    ] {
        assert!(matches!(
            NativeRecord::new(id, serde_json::Map::new()),
            Err(crate::native::NativeConvertError::InvalidIdentity(_))
        ));
        assert!(matches!(
            NativeRecord::from_typed(&Record { id }),
            Err(crate::native::NativeConvertError::InvalidIdentity(_))
        ));
        assert!(serde_json::from_value::<NativeRecord>(serde_json::json!({ "id": id })).is_err());
    }

    let id = "f3d:test:record#1";
    let record = NativeRecord::new(
        id,
        serde_json::Map::from_iter([(
            "id".to_owned(),
            serde_json::Value::String("ignored".into()),
        )]),
    )
    .unwrap();
    let typed = NativeRecord::from_typed(&Record { id }).unwrap();
    let decoded =
        serde_json::from_value::<NativeRecord>(serde_json::to_value(&record).unwrap()).unwrap();
    let admitted = NativeRecord::from_identity(
        crate::ids::UnknownId::mint(id).unwrap(),
        serde_json::Map::new(),
    );
    assert_eq!(record, admitted);
    assert_eq!(record, typed);
    assert_eq!(record, decoded);
    assert_eq!(record.id(), id);
}

#[test]
fn rejected_typed_identity_does_not_replace_an_existing_arena() {
    #[derive(Serialize)]
    struct Record<'a> {
        id: &'a str,
    }

    let mut namespace = crate::native::NativeNamespace::default();
    namespace
        .set_arena(
            "records",
            &[Record {
                id: "f3d:test:record#1",
            }],
        )
        .unwrap();
    let before = namespace.clone();
    assert!(namespace
        .set_arena("records", &[Record { id: "invalid" }])
        .is_err());
    assert_eq!(namespace, before);
}

#[test]
fn native_records_use_own_ids_for_counts_diff_and_validation() {
    let left = unit_cube().expect("valid unit cube fixture");
    let mut right = left.clone();
    right.native.namespace_mut("f3d").arenas_mut().insert(
        "act_guids".into(),
        vec![
            NativeRecord::new("f3d:test:act-guid#0", serde_json::Map::new())
                .expect("valid native identity"),
        ],
    );
    right.native.namespace_mut("sldprt").arenas_mut().insert(
        "configurations".into(),
        vec![
            NativeRecord::new("sldprt:test:configuration#0", serde_json::Map::new())
                .expect("valid native identity"),
        ],
    );
    right.native.finalize();

    let result = diff(&left, &right);
    assert_eq!(
        result
            .per_arena
            .iter()
            .find(|arena| arena.kind == "native.f3d.act_guids")
            .unwrap()
            .added,
        ["f3d:test:act-guid#0"]
    );
    assert_eq!(
        result
            .per_arena
            .iter()
            .find(|arena| arena.kind == "native.sldprt.configurations")
            .unwrap()
            .added,
        ["sldprt:test:configuration#0"]
    );
    let report = validate_neutral(&right, Vec::new());
    assert_eq!(report.entity_counts["native.f3d.act_guids"], 1);
    assert_eq!(report.entity_counts["native.sldprt.configurations"], 1);
    assert!(report.is_ok(), "{:?}", report.findings);

    right
        .native
        .namespace_mut("sldprt")
        .arenas_mut()
        .get_mut("configurations")
        .unwrap()[0] = NativeRecord::new("f3d:test:act-guid#0", serde_json::Map::new())
        .expect("valid native identity");
    right.native.finalize();
    assert!(validate_neutral(&right, Vec::new())
        .findings
        .iter()
        .any(|finding| finding.message == "entity id is not globally unique"));
}

/// The streaming canonical serializer must render the byte-exact text of the
/// `serde_json::to_value` route it replaced: recursively sorted object keys,
/// non-finite floats as `null`, `f32` widened to `f64`, and externally tagged
/// enum forms.
#[test]
fn from_typed_matches_value_tree_canonical_text() {
    #[derive(Serialize)]
    enum CanonShape {
        Unit,
        Newtype(u32),
        Tuple(i8, bool),
        Struct { zulu: f64, alpha: Option<String> },
    }

    #[derive(Serialize)]
    struct Shape<'a> {
        id: &'a str,
        #[serde(flatten)]
        fields: &'a serde_json::Map<String, serde_json::Value>,
    }

    #[derive(Serialize)]
    struct CanonRecord {
        id: String,
        zulu: f64,
        alpha: Vec<f64>,
        nested: BTreeMap<String, Vec<CanonShape>>,
        keyed: std::collections::HashMap<u32, char>,
        wide: f32,
        text: String,
        gone: Option<u8>,
        none_at_all: Option<u8>,
    }

    let record = CanonRecord {
        id: "f3d:test:canon#0-\"quotes\"-\u{1F980}".into(),
        zulu: -0.0,
        alpha: vec![f64::NAN, f64::INFINITY, 0.1, -1.5e300, 3.0],
        nested: BTreeMap::from([(
            "b\nkey".to_owned(),
            vec![
                CanonShape::Unit,
                CanonShape::Newtype(7),
                CanonShape::Tuple(-3, true),
                CanonShape::Struct {
                    zulu: f64::NEG_INFINITY,
                    alpha: Some("s".into()),
                },
            ],
        )]),
        keyed: std::collections::HashMap::from([(12, 'x')]),
        wide: 0.1_f32,
        text: "line\u{0}break\ttab".into(),
        gone: Some(9),
        none_at_all: None,
    };

    // The oracle is the replaced route itself: a `Value` tree flattened
    // behind a leading string `id`.
    let serde_json::Value::Object(mut fields) = serde_json::to_value(&record).unwrap() else {
        panic!("record serializes as an object");
    };
    let serde_json::Value::String(id) = fields.remove("id").unwrap() else {
        panic!("id serializes as a string");
    };
    let expected = serde_json::to_string(&Shape {
        id: &id,
        fields: &fields,
    })
    .unwrap();

    let native = NativeRecord::from_typed(&record).unwrap();
    assert_eq!(native.id(), id);
    assert_eq!(&*native.json, expected);
}
