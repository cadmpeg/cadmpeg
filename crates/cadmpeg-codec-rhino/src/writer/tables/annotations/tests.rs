// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::writer::{TCODE_OBJECT_TABLE, TCODE_SETTINGS_TABLE};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::units::FiniteVector;
use std::collections::BTreeMap;
use std::io::Cursor;

#[test]
fn edited_neutral_text_and_position_regenerate_the_annotation_object() {
    let mut ir = CadIr::empty();
    let native_id = "rhino:document:annotation#source";
    ir.native.namespace_mut("rhino").arenas_mut().insert("annotations".to_owned(), vec![cadmpeg_ir::NativeRecord::new(cadmpeg_ir::ids::Identity::new(native_id).expect("identity"), serde_json::from_value(serde_json::json!({"rich_text":"stale native text","plane_origin":[100.0,200.0,300.0]})).expect("fields")).expect("typed record")]);
    ir.model.semantic_annotations.push(SemanticAnnotation {
        id: "test:model:annotation#edited".try_into().expect("identity"),
        object: native_id.to_owned(),
        kind: SemanticAnnotationKind::Text,
        runtime_type: "rhino::annotations".to_owned(),
        order: 0,
        text: vec!["Current text".to_owned()],
        references: BTreeMap::new(),
        value: None,
        format: None,
        position: Some(FiniteVector::new([10.0, 20.0, 30.0]).expect("position")),
        parameters: BTreeMap::new(),
        assets: Vec::new(),
        native_ref: native_id.to_owned(),
    });
    let (objects, losses) = records(&ir).expect("source-free annotation");
    assert!(losses.is_empty());
    let archive = crate::test_support::test_dump::minimal_document(
        "80",
        &[
            crate::writer::tests::table(
                TCODE_SETTINGS_TABLE,
                &[super::super::super::units_record(
                    ir.tolerances.linear.get(),
                    ir.tolerances.angular.get(),
                )
                .expect("units")],
            )
            .expect("settings"),
            crate::writer::tests::table(TCODE_OBJECT_TABLE, &objects).expect("objects"),
        ],
    );
    let decoded = crate::RhinoCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect("decode authored text");
    assert_eq!(
        decoded.ir().model.semantic_annotations.len(),
        1,
        "{:?}",
        decoded.report().losses
    );
    let annotation = &decoded.ir().model.semantic_annotations[0];
    assert_eq!(annotation.text, ["Current text"]);
    assert_eq!(
        annotation.position.expect("position").get(),
        [10.0, 20.0, 30.0]
    );
    assert!(
        decoded
            .report()
            .losses
            .iter()
            .all(|loss| loss.code != crate::loss::RhinoLossCode::IntegrityFailure.kind()),
        "{:?}",
        decoded.report().losses
    );
}
