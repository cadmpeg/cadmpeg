// SPDX-License-Identifier: Apache-2.0
//! Presentation layers and source attributes survive a write (spec §18.10
//! and §18.11).

use crate::loss::UsdLossCode;
use crate::test_support::{decode, encode};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/golden/fixtures/{name}.usda",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture")
}

fn record<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> T {
    serde_json::from_value(value).expect("a record")
}

#[test]
fn layers_and_attributes_survive_a_write() {
    let source = decode(&fixture("brep_array_box"));
    let mut ir = source.ir().clone();
    let face = ir.model.faces[0].id.to_string();
    let body = ir.model.bodies[0].id.to_string();
    let occurrence = ir.model.occurrences[0].id.to_string();
    ir.model.presentation_layers = vec![record(serde_json::json!({
        "id": "test:presentation:layer#1",
        "name": "machined faces",
        "description": "primary face",
        "visible": false,
        "items": [
            {"kind": "face", "face": face},
            {"kind": "body", "body": body},
            {"kind": "occurrence", "occurrence": occurrence},
            {"kind": "source", "source_id": "layer item 9"},
            {"kind": "face", "face": "test:model:face#absent"}
        ]
    }))];
    ir.model.attributes = vec![
        record(serde_json::json!({
            "id": "test:model:attribute#1",
            "target": {"kind": "face", "id": face},
            "name": "finish",
            "values": [
                {"kind": "string", "value": "anodized"},
                {"kind": "integer", "value": 3},
                {"kind": "vector", "value": [1.0, 0.5]}
            ]
        })),
        record(serde_json::json!({
            "id": "test:model:attribute#2",
            "target": {"kind": "document"},
            "name": "author",
            "values": [{"kind": "string", "value": "jo"}]
        })),
    ];
    let (text, _) = encode(&ir);
    assert!(text.contains("CollectionAPI:members"), "{text}");
    let read = decode(&text);
    let after = &read.ir().model;
    let mut expected = ir.model.presentation_layers.clone();
    expected[0].items.pop();
    assert_eq!(after.presentation_layers, expected);
    assert_eq!(after.attributes, ir.model.attributes);
    assert!(read
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == UsdLossCode::PresentationPropertyNotTransferred.kind()));
}
