// SPDX-License-Identifier: Apache-2.0
//! Wire forms of the attribute targets and values: each reads the bytes it
//! prints, and a reader refuses an unknown key, a `null` optional key, a
//! `value` on a unit arm, and a restated object key.

use cadmpeg_core::decode::ResourceLimits;
use cadmpeg_core::CodecError;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::{json, Value};

use super::{AttributeJson, AttributeTarget, AttributeValue, SourceAttribute};
use crate::design::dimension::ParameterRef;
use crate::design::expr::{Constant, Expression, Neutral, Node};
use crate::features::ParameterId;

fn read<T: DeserializeOwned>(wire: &Value) -> Result<T, CodecError> {
    crate::design::read::from_str(&wire.to_string(), &ResourceLimits::desktop())
}

/// `wire` reads, and prints back as itself.
fn reads_back<T: DeserializeOwned + Serialize + std::fmt::Debug>(wire: &Value) -> T {
    let value = read::<T>(wire).unwrap_or_else(|error| panic!("{wire} refused: {error}"));
    assert_eq!(&serde_json::to_value(&value).unwrap(), wire);
    value
}

fn refused<T: DeserializeOwned + std::fmt::Debug>(wire: &Value) {
    if let Ok(value) = read::<T>(wire) {
        panic!("{wire} read as {value:?}");
    }
}

#[test]
fn every_target_arm_reads_back() {
    for (kind, id) in [
        ("region", "test:model:region#1"),
        ("surface", "test:model:surface#1"),
        ("curve", "test:model:curve#1"),
        ("point", "test:model:point#1"),
        ("feature", "test:design:feature#1"),
        ("sketch", "test:design:sketch#1"),
        ("parameter", "test:design:parameter#1"),
        ("product_definition", "test:model:definition#1"),
        ("occurrence", "test:model:occurrence#1"),
        ("tessellation", "test:model:tessellation#1"),
        ("pmi", "test:model:pmi#1"),
        ("semantic_annotation", "test:model:annotation#1"),
        ("drawing", "test:model:drawing#1"),
        ("view_presentation", "test:model:view#1"),
    ] {
        reads_back::<AttributeTarget>(&json!({"kind": kind, "value": id}));
    }
    let record =
        json!({"kind": "record", "value": {"arena": "point_clouds", "id": "test:model:cloud#1"}});
    reads_back::<AttributeTarget>(&record);
    refused::<AttributeTarget>(
        &json!({"kind": "record", "value": {"arena": " ", "id": "test:model:cloud#1"}}),
    );
    refused::<AttributeTarget>(&json!({"kind": "record", "value": {"arena": "point_clouds"}}));
    refused::<AttributeTarget>(
        &json!({"kind": "record", "value": {"arena": "point_clouds", "id": "x", "extra": 1}}),
    );
    refused::<AttributeTarget>(&json!({"kind": "document", "value": null}));
}

#[test]
fn target_order_agrees_with_equality() {
    let targets = [
        AttributeTarget::Document,
        AttributeTarget::Tessellation("test:model:tessellation#1".into()),
        AttributeTarget::Tessellation("test:model:tessellation#2".into()),
        AttributeTarget::Record {
            arena: cadmpeg_core::text::NonBlankString::new("point_clouds").unwrap(),
            id: "test:model:cloud#1".into(),
        },
        AttributeTarget::Record {
            arena: cadmpeg_core::text::NonBlankString::new("point_clouds").unwrap(),
            id: "test:model:cloud#2".into(),
        },
        AttributeTarget::Record {
            arena: cadmpeg_core::text::NonBlankString::new("assets").unwrap(),
            id: "test:model:cloud#1".into(),
        },
    ];
    for a in &targets {
        for b in &targets {
            assert_eq!(a.cmp(b).is_eq(), a == b, "{a:?} {b:?}");
        }
    }
}

#[test]
fn every_value_arm_reads_back() {
    for wire in [
        json!({"kind": "map", "value": [{"key": "a", "value": "1"}, {"key": "a", "value": "2"}]}),
        json!({"kind": "sub_reference", "value": {"object": "Box", "subelements": ["Edge3"]}}),
        json!({"kind": "sub_reference", "value": {"subelements": []}}),
        json!({"kind": "external_reference", "value": {"document": "part.FCStd", "object": "Box", "subelements": ["Face1"]}}),
        json!({"kind": "external_reference", "value": {"document": "part.FCStd", "subelements": []}}),
        json!({"kind": "empty_reference"}),
        json!({"kind": "json", "value": {"kind": "null"}}),
        json!({"kind": "json", "value": {"kind": "unsigned", "value": u64::MAX}}),
        json!({"kind": "json", "value": {"kind": "object", "value": {
            "a": {"kind": "array", "value": [{"kind": "integer", "value": -1}, {"kind": "float", "value": 0.5}]},
            "b": {"kind": "string", "value": "x"},
            "c": {"kind": "boolean", "value": true}
        }}}),
    ] {
        reads_back::<AttributeValue>(&wire);
    }
    refused::<AttributeValue>(&json!({"kind": "empty_reference", "value": null}));
    refused::<AttributeValue>(
        &json!({"kind": "sub_reference", "value": {"object": null, "subelements": []}}),
    );
    refused::<AttributeValue>(
        &json!({"kind": "external_reference", "value": {"object": "Box", "subelements": []}}),
    );
    refused::<AttributeValue>(
        &json!({"kind": "map", "value": [{"key": "a", "value": "1", "extra": 1}]}),
    );
    refused::<AttributeValue>(&json!({"kind": "json", "value": {"kind": "null", "value": null}}));
    refused::<AttributeValue>(&json!({"kind": "json", "value": {"kind": "unsigned", "value": -1}}));
    refused::<AttributeValue>(&json!({"kind": "json", "value": {"kind": "float", "value": null}}));
    assert!(crate::design::read::from_str::<AttributeJson>(
        r#"{"kind": "object", "value": {"a": {"kind": "null"}, "a": {"kind": "boolean", "value": true}}}"#,
        &ResourceLimits::desktop(),
    )
    .is_err());
}

#[test]
fn a_parameter_value_reads_back_and_an_expression_value_is_refused() {
    let value = AttributeValue::Parameter(ParameterId::mint("test:design:parameter#mass").unwrap());
    let wire = serde_json::to_value(&value).unwrap();
    assert_eq!(reads_back::<AttributeValue>(&wire), value);
    let expression = Expression::Neutral(Neutral(Node::<ParameterRef>::Constant(Constant::Pi)));
    read::<Expression>(&serde_json::to_value(&expression).unwrap()).unwrap();
    refused::<AttributeValue>(&json!({
        "kind": "expression",
        "value": {"path": "Length", "expression": expression}
    }));
}

#[test]
fn an_attribute_states_an_absent_value_as_null_and_a_native_ref() {
    let wire = json!({
        "id": "test:model:attribute#1",
        "target": {"entity": {"kind": "document"}},
        "name": "Material",
        "values": [{"kind": "integer", "value": 1}, null, {"kind": "empty_reference"}],
        "native_ref": "test:native:record#1"
    });
    let attribute = reads_back::<SourceAttribute>(&wire);
    assert_eq!(attribute.values[1], None);
    let mut null_ref = wire.clone();
    null_ref["native_ref"] = Value::Null;
    refused::<SourceAttribute>(&null_ref);
}

/// Runs `work` on a thread whose stack is 2 MiB, the default for a spawned
/// thread.
fn on_a_default_thread<T: Send + 'static>(work: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(2 << 20)
        .spawn(work)
        .unwrap()
        .join()
        .unwrap()
}

/// A JSON tree has no ceiling of its own; the JSON depth limit bounds it.
/// One level past the deepest nesting the reader admits is refused by that
/// limit, and the deepest reads, prints the same bytes, clones, compares,
/// and drops on a default thread.
#[test]
fn json_trees_nest_to_the_json_depth_limit_on_a_default_thread() {
    fn nested(depth: usize) -> String {
        let tree = (0..depth).fold(AttributeJson::Null, |tree, level| {
            if level % 2 == 0 {
                AttributeJson::Array(vec![tree])
            } else {
                AttributeJson::Object([("k".to_owned(), tree)].into())
            }
        });
        serde_json::to_string(&AttributeValue::Json(tree)).unwrap()
    }
    let read = |text: &str| {
        crate::design::read::from_str::<AttributeValue>(text, &ResourceLimits::desktop())
    };
    let past = (1..128)
        .find(|&depth| read(&nested(depth)).is_err())
        .expect("the reader refuses some depth below the JSON limit");
    assert!(past > 1, "a single level is refused");
    let refusal = on_a_default_thread({
        let text = nested(past);
        move || read(&text).err()
    });
    assert!(
        matches!(&refusal, Some(CodecError::Malformed(message)) if message.contains("recursion limit exceeded")),
        "depth {past}: {refusal:?}"
    );
    let deepest = nested(past - 1);
    let printed = on_a_default_thread({
        let deepest = deepest.clone();
        move || {
            let value = read(&deepest).unwrap();
            let printed = serde_json::to_string(&value).unwrap();
            let cloned = value.clone();
            assert_eq!(cloned, value);
            drop(cloned);
            drop(value);
            printed
        }
    });
    assert_eq!(printed, deepest);
}
