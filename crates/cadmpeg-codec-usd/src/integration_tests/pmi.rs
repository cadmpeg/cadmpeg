// SPDX-License-Identifier: Apache-2.0
//! PMI annotations survive a write as `CadPmi` prims (spec §18.9).

use cadmpeg_ir::pmi::{PmiAnnotation, PmiTarget};

use crate::loss::UsdLossCode;
use crate::test_support::{decode, encode};

fn fixture(name: &str) -> String {
    std::fs::read_to_string(format!(
        "{}/tests/golden/fixtures/{name}.usda",
        env!("CARGO_MANIFEST_DIR")
    ))
    .expect("fixture")
}

fn annotation(value: serde_json::Value) -> PmiAnnotation {
    serde_json::from_value(value).expect("an annotation")
}

#[test]
fn an_annotation_keeps_its_face_targets_and_definition() {
    let source = decode(&fixture("brep_array_box"));
    let mut ir = source.ir().clone();
    let face = ir.model.faces[0].id.to_string();
    ir.model.pmi = vec![
        annotation(serde_json::json!({
            "id": "test:model:pmi#A",
            "name": "Datum A",
            "visible": true,
            "targets": [{"kind": "face", "face": face}],
            "definition": {"kind": "datum", "identification": "A"}
        })),
        annotation(serde_json::json!({
            "id": "test:model:pmi#B",
            "targets": [{"kind": "shape_aspect", "source_id": "aspect 7"}],
            "definition": {"kind": "datum", "identification": "B"}
        })),
    ];
    let (text, _) = encode(&ir);
    assert!(text.contains("def CadPmi"), "{text}");
    let read = decode(&text);
    assert_eq!(read.ir().model.pmi, ir.model.pmi);
}

#[test]
fn a_target_the_stage_does_not_hold_is_dropped() {
    let source = decode(&fixture("brep_array_box"));
    let mut ir = source.ir().clone();
    let face = ir.model.faces[0].id.to_string();
    ir.model.pmi = vec![annotation(serde_json::json!({
        "id": "test:model:pmi#A",
        "targets": [
            {"kind": "face", "face": face},
            {"kind": "face", "face": "test:model:face#absent"}
        ],
        "definition": {"kind": "datum", "identification": "A"}
    }))];
    let (text, _) = encode(&ir);
    let read = decode(&text);
    let [found] = read.ir().model.pmi.as_slice() else {
        panic!("one annotation");
    };
    assert_eq!(
        found.targets,
        vec![PmiTarget::Face {
            face: ir.model.faces[0].id.clone()
        }]
    );
    assert!(read
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == UsdLossCode::PmiPropertyNotTransferred.kind()));
}
