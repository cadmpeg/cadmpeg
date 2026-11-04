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
        reference_anchors: Vec::new(),
        text_style: cadmpeg_ir::semantic_annotations::AnnotationTextStyle::default(),
        format: None,
        position: Some(FiniteVector::new([10.0, 20.0, 30.0]).expect("position")),
        dimension_kind: None,
        text_override: None,
        upper_tolerance: None,
        lower_tolerance: None,
        parameters: BTreeMap::new(),
        assets: Vec::new(),
        native_ref: native_id.to_owned(),
    });
    let exported = records(
        &ir,
        &super::super::materials::MaterialPlan::new(&ir).expect("material plan"),
    )
    .expect("source-free annotation");
    let objects = exported.records;
    let losses = exported.losses;
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

#[test]
fn multiline_text_join_names_the_annotation() {
    let mut ir = CadIr::empty();
    let native_id = "rhino:document:annotation#multiline";
    ir.model.semantic_annotations.push(SemanticAnnotation {
        id: "test:model:annotation#multiline".try_into().expect("identity"),
        object: native_id.to_owned(),
        kind: SemanticAnnotationKind::Text,
        runtime_type: "rhino::annotations".to_owned(),
        order: 0,
        text: vec!["first".to_owned(), "second".to_owned()],
        references: BTreeMap::new(),
        value: None,
        reference_anchors: Vec::new(),
        text_style: cadmpeg_ir::semantic_annotations::AnnotationTextStyle::default(),
        format: None,
        position: Some(FiniteVector::new([10.0, 20.0, 30.0]).expect("position")),
        dimension_kind: None,
        text_override: None,
        upper_tolerance: None,
        lower_tolerance: None,
        parameters: BTreeMap::new(),
        assets: Vec::new(),
        native_ref: native_id.to_owned(),
    });
    let exported = records(
        &ir,
        &super::super::materials::MaterialPlan::new(&ir).expect("material plan"),
    )
    .expect("multiline annotation writes");
    assert!(
        exported.losses.iter().any(|loss| loss
            .message
            .contains("test:model:annotation#multiline")),
        "{:?}",
        exported.losses
    );
    assert!(
        exported
            .losses
            .iter()
            .all(|loss| loss.code.local_code() == "writer.annotation-text-joined"),
        "{:?}",
        exported.losses
    );
}

#[test]
fn v1_leader_writes_its_plane_and_points() {
    let mut ir = CadIr::empty();
    let native_id = "rhino:legacy:v1-record#leader";
    ir.native.namespace_mut("rhino").arenas_mut().insert(
        "legacy_v1_records".to_owned(),
        vec![cadmpeg_ir::NativeRecord::new(
            native_id.try_into().expect("native identity"),
            serde_json::from_value(serde_json::json!({
                "document_scale": 2.0,
                "payload": {"kind":"Annotation", "fields": {"kind":"Leader", "fields": {
                    "plane": {"origin":[10.0,20.0,30.0], "x_axis":[0.0,1.0,0.0], "y_axis":[0.0,0.0,1.0]},
                    "points": [[1.0,2.0,0.0],[3.0,4.0,0.0]]
                }}}
            })).expect("native fields"),
        ).expect("native record")],
    );
    let mut annotation = SemanticAnnotation::new(
        "test:model:annotation#leader"
            .try_into()
            .expect("annotation identity"),
        native_id.to_owned(),
        SemanticAnnotationKind::Leader,
        "v1_leader".to_owned(),
        0,
        native_id.to_owned(),
    );
    annotation.position = Some(FiniteVector::new([7.0, 8.0, 9.0]).expect("position"));
    ir.model.semantic_annotations.push(annotation);
    let export = records(
        &ir,
        &super::super::materials::MaterialPlan::new(&ir).expect("materials"),
    )
    .expect("leader writes");
    assert_eq!(export.records.len(), 1);
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
            crate::writer::tests::table(TCODE_OBJECT_TABLE, &export.records).expect("objects"),
        ],
    );
    let decoded = crate::RhinoCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect("leader decodes");
    let annotation = &decoded.ir().model.semantic_annotations[0];
    assert_eq!(
        annotation.position.expect("position").get(),
        [7.0, 8.0, 9.0]
    );
    let native = &decoded
        .ir()
        .native
        .namespace("rhino")
        .expect("namespace")
        .arenas()["annotations"][0];
    let fields = native.fields().expect("fields");
    assert_eq!(fields["plane_x_axis"], serde_json::json!([0.0, 1.0, 0.0]));
    assert_eq!(fields["plane_y_axis"], serde_json::json!([0.0, 0.0, 1.0]));
    assert_eq!(
        fields["leader_points"],
        serde_json::json!([[2.0, 4.0], [6.0, 8.0]])
    );
    ir.model.semantic_annotations[0].kind = SemanticAnnotationKind::Text;
    let mismatch = records(
        &ir,
        &super::super::materials::MaterialPlan::new(&ir).expect("materials"),
    )
    .expect("mismatched annotation plans");
    assert_eq!(mismatch.records.len(), 1);
    assert!(mismatch
        .losses
        .iter()
        .any(|loss| loss.message.contains(native_id)));
}

#[test]
fn v1_dimensions_write_class_objects_from_current_text() {
    for (kind, code, expected_type, expected_value) in [
        ("LinearDimension", 2, "linear_dimension", 4.0),
        ("RadialDimension", 8, "radius_dimension", 2.0),
        (
            "AngularDimension",
            6,
            "angular_dimension",
            std::f64::consts::FRAC_PI_2,
        ),
    ] {
        let mut ir = CadIr::empty();
        let native_id = "rhino:legacy:v1-record#dimension";
        ir.native.namespace_mut("rhino").arenas_mut().insert(
            "legacy_v1_records".to_owned(),
            vec![cadmpeg_ir::NativeRecord::new(
                native_id.try_into().expect("native identity"),
                serde_json::from_value(serde_json::json!({
                    "document_scale": 1.0,
                    "payload": {"kind":"Annotation", "fields": {"kind":kind, "fields": {
                        "annotation_type":code,
                        "plane": {"origin":[0.0,0.0,0.0], "x_axis":[1.0,0.0,0.0], "y_axis":[0.0,1.0,0.0]},
                        "points": [[0.0,0.0,0.0],[2.0,0.0,0.0],[4.0,0.0,0.0],[6.0,0.0,0.0],[1.0,1.0,0.0]],
                        "user_text": {"text":"old"}, "user_positioned_text": 0,
                        "angle": std::f64::consts::FRAC_PI_2, "radius": 2.0
                    }}}
                })).expect("native fields"),
            ).expect("native record")],
        );
        let mut annotation = SemanticAnnotation::new(
            "test:model:annotation#dimension"
                .try_into()
                .expect("annotation identity"),
            native_id.to_owned(),
            SemanticAnnotationKind::Dimension,
            "v1_dimension".to_owned(),
            0,
            native_id.to_owned(),
        );
        annotation.text = vec!["edited".to_owned()];
        annotation.position = Some(FiniteVector::new([0.0, 0.0, 0.0]).expect("position"));
        ir.model.semantic_annotations.push(annotation);
        let export = records(
            &ir,
            &super::super::materials::MaterialPlan::new(&ir).expect("materials"),
        )
        .expect("dimension writes");
        assert_eq!(export.records.len(), 1, "{kind}");
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
                crate::writer::tests::table(TCODE_OBJECT_TABLE, &export.records).expect("objects"),
            ],
        );
        let decoded = crate::RhinoCodec
            .decode(&mut Cursor::new(archive), &DecodeOptions::default())
            .expect("dimension decodes");
        assert_eq!(
            decoded.ir().model.semantic_annotations.len(),
            1,
            "{kind}: {:?}",
            decoded.report().losses
        );
        let annotation = &decoded.ir().model.semantic_annotations[0];
        assert_eq!(annotation.runtime_type, expected_type);
        assert_eq!(annotation.text, ["edited"]);
        assert!((annotation.value.expect("value").get() - expected_value).abs() < 1e-12);
        assert!(export
            .losses
            .iter()
            .any(|loss| loss.message.contains(native_id)));
        ir.model.semantic_annotations[0].value =
            cadmpeg_ir::scalar::FiniteReal::new(expected_value);
        let matching = records(
            &ir,
            &super::super::materials::MaterialPlan::new(&ir).expect("materials"),
        )
        .expect("matching dimension plans");
        assert!(matching
            .losses
            .iter()
            .all(|loss| loss.code != RhinoLossCode::WriterDimensionValueReduced.kind()));
        ir.model.semantic_annotations[0].value = cadmpeg_ir::scalar::FiniteReal::new(123.0);
        let edited = records(
            &ir,
            &super::super::materials::MaterialPlan::new(&ir).expect("materials"),
        )
        .expect("edited dimension plans");
        assert!(edited.losses.iter().any(|loss| loss.code
            == RhinoLossCode::WriterDimensionValueReduced.kind()
            && loss.message.contains("test:model:annotation#dimension")
            && loss.message.contains("independently")));
        ir.model.semantic_annotations[0].kind = SemanticAnnotationKind::Text;
        let text = records(
            &ir,
            &super::super::materials::MaterialPlan::new(&ir).expect("materials"),
        )
        .expect("dimension converted to text");
        assert_eq!(text.records.len(), 1);
        assert!(text.losses.iter().any(|loss| loss.code
            == RhinoLossCode::WriterV1AnnotationReduced.kind()
            && loss.message.contains(native_id)
            && loss.message.contains("dimension definition")));
    }
}

#[test]
fn v1_leader_and_dimension_round_trip_from_a_decoded_archive() {
    use cadmpeg_ir::codec::write::{target::TargetRequest, EncodeInput, Encoder};

    fn chunk(kind: u32, body: &[u8]) -> Vec<u8> {
        let mut out = kind.to_le_bytes().to_vec();
        out.extend(
            i32::try_from(body.len())
                .expect("fixture length")
                .to_le_bytes(),
        );
        out.extend(body);
        out
    }
    fn string(value: &str) -> Vec<u8> {
        let mut out = i32::try_from(value.len())
            .expect("fixture text length")
            .to_le_bytes()
            .to_vec();
        out.extend(value.as_bytes());
        out
    }
    let plane = [0.0_f64, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0]
        .into_iter()
        .flat_map(f64::to_le_bytes)
        .collect::<Vec<_>>();
    let mut leader = 1_i32.to_le_bytes().to_vec();
    leader.extend(0_i32.to_le_bytes());
    leader.extend(&plane);
    leader.extend(0_i32.to_le_bytes());
    leader.extend(1_i32.to_le_bytes());
    leader.extend(2_i32.to_le_bytes());
    for coordinate in [0.0_f64, 0.0, 0.0, 1.0, 2.0, 0.0] {
        leader.extend(coordinate.to_le_bytes());
    }
    let mut linear = 1_i32.to_le_bytes().to_vec();
    linear.extend(10_i32.to_le_bytes());
    linear.extend(&plane);
    for index in 0..11 {
        for coordinate in [f64::from(index), 0.0, 0.0] {
            linear.extend(coordinate.to_le_bytes());
        }
    }
    linear.extend(string("authored"));
    linear.extend(string("default"));
    linear.extend(0_i32.to_le_bytes());
    linear.extend(0_i32.to_le_bytes());
    linear.extend(1_i32.to_le_bytes());
    let mut bytes = crate::chunks::MAGIC.to_vec();
    bytes.extend(*b"       1");
    bytes.extend(chunk(1, b"legacy"));
    bytes.extend(chunk(0x0020_0005, &leader));
    bytes.extend(chunk(0x0020_0006, &linear));
    let source = crate::RhinoCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .expect("decode synthesized V1 archive");
    assert_eq!(source.ir().model.semantic_annotations.len(), 2);
    let plan = crate::RhinoCodec
        .plan(EncodeInput::new(source.ir(), None), TargetRequest::Inherit)
        .expect("plan converted annotations");
    let mut output = Vec::new();
    plan.write_to(&mut output)
        .expect("write converted annotations");
    let decoded = crate::RhinoCodec
        .decode(&mut Cursor::new(output), &DecodeOptions::default())
        .expect("decode converted annotations");
    let annotations = &decoded.ir().model.semantic_annotations;
    assert!(annotations
        .iter()
        .any(|annotation| annotation.kind == SemanticAnnotationKind::Leader));
    assert!(annotations
        .iter()
        .any(|annotation| annotation.runtime_type == "linear_dimension"
            && annotation.text == ["authored"]));
}
