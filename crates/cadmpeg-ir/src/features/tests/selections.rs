// SPDX-License-Identifier: Apache-2.0

use crate::features::{
    EdgeSelection, FaceSelection, GeneratedEdgeRef, GeneratedFaceRef, VertexSelection,
};
use serde_json::json;

#[test]
fn historical_selection_admission_preserves_partial_and_reference_semantics() {
    for kind in ["historical", "historical_partial"] {
        let mut edge = json!({"kind":kind,"value":{
            "state":"test:model:feature-input-topology#1",
            "edges":["test:model:historical-edge#1"],"native":" "
        }});
        let mut face = json!({"kind":kind,"value":{
            "state":"test:model:feature-input-topology#1",
            "faces":["test:model:historical-face#1"],"native":" "
        }});
        if kind == "historical_partial" {
            edge["value"]["unresolved"] = json!(["edge:unknown"]);
            face["value"]["unresolved"] = json!(["face:unknown"]);
        }
        for (wire, field) in [(&mut edge, "edges"), (&mut face, "faces")] {
            let valid = wire.clone();
            let accepts = |value: serde_json::Value| {
                if field == "edges" {
                    serde_json::from_value::<EdgeSelection>(value).is_ok()
                } else {
                    serde_json::from_value::<FaceSelection>(value).is_ok()
                }
            };
            assert!(accepts(valid.clone()));
            wire["value"]["native"] = json!("");
            assert!(!accepts(wire.clone()));
            *wire = valid.clone();
            wire["value"][field] = json!([valid["value"][field][0], valid["value"][field][0]]);
            assert!(!accepts(wire.clone()));
            *wire = valid.clone();
            wire["value"][field] = json!([]);
            assert_eq!(accepts(wire.clone()), kind == "historical_partial");
            if kind == "historical_partial" {
                for unresolved in [json!([]), json!([" "]), json!(["same", "same"])] {
                    *wire = valid.clone();
                    wire["value"]["unresolved"] = unresolved;
                    assert!(!accepts(wire.clone()));
                }
            }
        }
    }
}

#[test]
fn generated_selection_admission_rejects_blank_identities_and_preserves_duplicates() {
    let feature = crate::features::FeatureId::mint("test:model:feature#1").unwrap();
    assert!(GeneratedEdgeRef::new(feature.clone(), " ".into()).is_err());
    assert!(GeneratedFaceRef::new(feature.clone(), String::new()).is_err());
    let edge = GeneratedEdgeRef::new(feature.clone(), "edge".into()).unwrap();
    let face = GeneratedFaceRef::new(feature, "face".into()).unwrap();
    assert!(EdgeSelection::generated(vec![], "native".into()).is_err());
    assert!(FaceSelection::generated(vec![], "native".into()).is_err());
    assert!(EdgeSelection::generated(vec![edge.clone()], " ".into()).is_err());
    assert!(FaceSelection::generated(vec![face.clone()], " ".into()).is_err());
    assert!(EdgeSelection::generated(vec![edge.clone(), edge], "native".into()).is_ok());
    assert!(FaceSelection::generated(vec![face.clone(), face], "native".into()).is_ok());
    for (field, local_id) in [("edges", "edge"), ("faces", "face")] {
        let wire = json!({"kind":"generated","value":{
            field:[{"feature":"test:model:feature#1","local_id":local_id}],"native":"native"
        }});
        let accepts = |value: serde_json::Value| {
            if field == "edges" {
                serde_json::from_value::<EdgeSelection>(value).is_ok()
            } else {
                serde_json::from_value::<FaceSelection>(value).is_ok()
            }
        };
        assert!(accepts(wire.clone()));
        for bad in ["", " "] {
            let mut invalid = wire.clone();
            invalid["value"][field][0]["local_id"] = json!(bad);
            assert!(!accepts(invalid));
        }
    }
    for native in ["", " "] {
        assert!(VertexSelection::native(native.into()).is_err());
        assert!(
            serde_json::from_value::<VertexSelection>(json!({"kind":"native","value":native}))
                .is_err()
        );
    }
}

#[test]
fn selection_wire_errors_name_the_rejected_field() {
    for (wire, field) in [
        (
            json!({"kind":"historical","value":{"state":"test:model:state#1","edges":[],"native":"edge"}}),
            "edges",
        ),
        (
            json!({"kind":"historical","value":{"state":"test:model:state#1","edges":["edge"],"native":""}}),
            "native",
        ),
        (
            json!({"kind":"historical_partial","value":{"state":"test:model:state#1","edges":[],"unresolved":[],"native":"edge"}}),
            "unresolved",
        ),
        (
            json!({"kind":"generated","value":{"edges":[{"feature":"test:model:feature#1","local_id":""}],"native":"edge"}}),
            "local_id",
        ),
    ] {
        let error = serde_json::from_value::<EdgeSelection>(wire).unwrap_err();
        assert!(error.to_string().contains(field), "{error}");
    }
    let wire = json!({"kind":"generated","value":{"faces":[],"native":"face"}});
    assert!(serde_json::from_value::<FaceSelection>(wire)
        .unwrap_err()
        .to_string()
        .contains("faces"));
}

#[test]
fn vertex_selection_wire_admission_preserves_valid_forms() {
    let feature = "test:model:feature#1";
    for wire in [
        json!({"kind":"unresolved"}),
        json!({"kind":"native","value":"vertex"}),
        json!({"kind":"historical","value":{"state":"test:model:state#1","vertex":"vertex","native":" "}}),
        json!({"kind":"generated","value":{"vertex":{"feature":feature,"local_id":"vertex"},"native":"native"}}),
    ] {
        let admitted = serde_json::from_value::<VertexSelection>(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(admitted).unwrap(), wire);
    }
    for (native, local_id) in [
        ("", "vertex"),
        (" ", "vertex"),
        ("native", ""),
        ("native", " "),
    ] {
        let wire = json!({"kind":"generated","value":{"vertex":{"feature":feature,"local_id":local_id},"native":native}});
        assert!(serde_json::from_value::<VertexSelection>(wire).is_err());
    }
}
