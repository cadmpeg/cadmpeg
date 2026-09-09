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
    let left = unit_cube();
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
