// SPDX-License-Identifier: Apache-2.0
use super::identities;
use crate::ids::PointId;
use serde::ser::SerializeSeq;
use serde::{Serialize, Serializer};
use std::collections::BTreeMap;

#[derive(Serialize)]
struct Text(String);

#[derive(Serialize)]
enum Variant {
    Unit,
    Newtype(PointId),
    Tuple(PointId, String),
    Struct { reference: PointId, text: String },
}

#[derive(Serialize)]
struct Entity {
    id: PointId,
    label: String,
    optional: Option<PointId>,
    absent: Option<PointId>,
    tuple: (PointId, Text),
    variants: Vec<Variant>,
    references: BTreeMap<PointId, PointId>,
    extensions: BTreeMap<String, String>,
}

fn id(key: &str) -> PointId {
    PointId::mint(format!("test:model:point#{key}")).unwrap()
}

#[test]
fn generic_unknown_links_share_the_typed_identity_rewrite_boundary() {
    let record = crate::NativeUnknownRecord {
        id: crate::ids::UnknownId::mint("test:model:unknown#source").unwrap(),
        links: vec![crate::ids::Identity::new("test:model:point#a").unwrap()],
    };
    let rewritten = serde_json::to_value(identities(&record, |source| {
        source.replace("test:model:", "test:occurrence:")
    }))
    .unwrap();
    assert_eq!(
        rewritten,
        serde_json::json!({
            "id": "test:occurrence:unknown#source",
            "links": ["test:occurrence:point#a"]
        })
    );
    assert_eq!(record.links[0].as_str(), "test:model:point#a");
}

#[test]
fn rewriting_distinguishes_identities_from_identical_text_in_every_container() {
    let source = id("a").to_string();
    let entity = Entity {
        id: id("a"),
        label: source.clone(),
        optional: Some(id("a")),
        absent: None,
        tuple: (id("a"), Text(source.clone())),
        variants: vec![
            Variant::Unit,
            Variant::Newtype(id("a")),
            Variant::Tuple(id("a"), source.clone()),
            Variant::Struct {
                reference: id("a"),
                text: source.clone(),
            },
        ],
        references: BTreeMap::from([(id("a"), id("b"))]),
        extensions: BTreeMap::from([(source.clone(), source.clone())]),
    };
    let original = serde_json::to_value(&entity).unwrap();
    let calls = std::cell::Cell::new(0);
    let rewritten = serde_json::to_value(identities(&entity, |source| {
        calls.set(calls.get() + 1);
        source.replace("test:model:", "test:occurrence:")
    }))
    .unwrap();
    assert_eq!(calls.get(), 2);
    assert_eq!(
        rewritten,
        serde_json::json!({
            "id": "test:occurrence:point#a", "label": source,
            "optional": "test:occurrence:point#a", "absent": null,
            "tuple": ["test:occurrence:point#a", source],
            "variants": ["Unit", {"Newtype": "test:occurrence:point#a"},
                {"Tuple": ["test:occurrence:point#a", source]},
                {"Struct": {"reference": "test:occurrence:point#a", "text": source}}],
            "references": {"test:occurrence:point#a": "test:occurrence:point#b"},
            "extensions": {"test:model:point#a": "test:model:point#a"}
        })
    );
    assert_eq!(serde_json::to_value(&entity).unwrap(), original);
}

#[test]
fn colliding_map_keys_and_invalid_targets_fail_without_changing_the_source() {
    let source = BTreeMap::from([(id("a"), 1), (id("b"), 2)]);
    let before = serde_json::to_value(&source).unwrap();
    let error = serde_json::to_value(identities(&source, |_| "test:occurrence:point#same".into()))
        .unwrap_err();
    assert!(error.to_string().contains("test:model:point#b"), "{error}");
    assert!(error.to_string().contains("collides"), "{error}");
    let error = serde_json::to_value(identities(&source, |_| String::new())).unwrap_err();
    assert!(error.to_string().contains("invalid identity"), "{error}");
    assert_eq!(serde_json::to_value(&source).unwrap(), before);
}

struct SwallowsElementErrors([PointId; 2]);

impl Serialize for SwallowsElementErrors {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(2))?;
        for id in &self.0 {
            sequence.serialize_element(id).ok();
        }
        sequence.end()
    }
}

#[test]
fn a_serializer_cannot_swallow_a_rewrite_collision() {
    let source = SwallowsElementErrors([id("a"), id("b")]);
    let error = serde_json::to_value(identities(&source, |_| "test:occurrence:point#same".into()))
        .unwrap_err();
    assert!(error.to_string().contains("collides"), "{error}");
    assert_eq!(
        serde_json::to_value(&source).unwrap(),
        serde_json::json!(["test:model:point#a", "test:model:point#b"])
    );
}
