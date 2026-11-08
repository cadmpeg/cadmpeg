// SPDX-License-Identifier: Apache-2.0
//! Writer-domain synthetic tests.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use std::io::{Cursor, Read};

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::native_test::{f3d_native, f3d_native_mut};
use crate::F3dCodec;
#[test]
fn source_less_extrude_keeps_profile_frame() {
    let bytes = include_bytes!("../../../tests/golden/fixtures/design_extrude_depth.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut edited = decoded.ir().clone();
    {
        let mut native = f3d_native_mut(&mut edited);
        let group = &mut native.design_construction_operand_groups[0];
        let mut frame = serde_json::to_value(&group.frame).unwrap();
        frame["opaque_index"] = serde_json::json!(7);
        frame["opaque_scalar"] = serde_json::json!(2.5);
        group.frame = serde_json::from_value(frame).unwrap();
    }
    let (written, _) = super::source_less_analytic::synthesize_document(&edited, None);
    let input = &written.ir().model.feature_input_topologies[0];
    let native = f3d_native(written.ir());
    let group = native
        .design_construction_operand_groups
        .iter()
        .find(|group| {
            input
                .operands
                .iter()
                .any(|operand| operand.native_ref.as_str() == group.id)
        })
        .unwrap();
    assert_eq!(group.frame.opaque_index.get(), 7);
    assert!((group.frame.opaque_scalar().get() - 2.5).abs() < 1e-12);
}
#[test]
fn source_less_extrude_keeps_profile_record_indices() {
    let bytes = include_bytes!("../../../tests/golden/fixtures/design_extrude_depth.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let indices = |ir: &cadmpeg_ir::document::CadIr| {
        let feature = ir
            .model
            .features
            .iter()
            .find(|feature| {
                matches!(
                    feature.evaluation.definition(),
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Extrude { .. }
                    )
                )
            })
            .unwrap();
        let native = f3d_native(ir);
        let scope = native
            .design_parameter_scopes
            .iter()
            .find(|scope| feature.native_ref.as_deref() == Some(scope.id.as_str()))
            .unwrap();
        let group = *scope.reference_members().values().next().unwrap();
        (group, scope.extrude_profile().unwrap().record_index)
    };
    let expected = indices(decoded.ir());
    let (written, _) = super::source_less_analytic::synthesize_document(decoded.ir(), None);
    assert_eq!(indices(written.ir()), expected);
}
#[test]
fn source_less_extrude_targets_generated_design_asset() {
    let bytes = include_bytes!("../../../tests/golden/fixtures/design_extrude_depth.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let asset = |ir: &cadmpeg_ir::document::CadIr| {
        let feature = ir
            .model
            .features
            .iter()
            .find(|feature| {
                matches!(
                    feature.evaluation.definition(),
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Extrude { .. }
                    )
                )
            })
            .unwrap();
        let native = f3d_native(ir);
        let scope = native
            .design_parameter_scopes
            .iter()
            .find(|scope| feature.native_ref.as_deref() == Some(scope.id.as_str()))
            .unwrap();
        scope
            .extrude_profile()
            .unwrap()
            .asset_id
            .as_str()
            .to_owned()
    };
    assert_ne!(
        asset(decoded.ir()),
        crate::manifest::GENERATED_ASSET_FOLDER_GUID
    );
    let (written, _) = super::source_less_analytic::synthesize_document(decoded.ir(), None);
    assert_eq!(
        asset(written.ir()),
        crate::manifest::GENERATED_ASSET_FOLDER_GUID
    );
}
#[test]
fn source_less_extrude_keeps_scope_and_parameter_indices() {
    let bytes = include_bytes!("../../../tests/golden/fixtures/design_extrude_depth.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let indices = |ir: &cadmpeg_ir::document::CadIr| {
        let feature = ir
            .model
            .features
            .iter()
            .find(|feature| {
                matches!(
                    feature.evaluation.definition(),
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Extrude { .. }
                    )
                )
            })
            .unwrap();
        let native = f3d_native(ir);
        let scope = native
            .design_parameter_scopes
            .iter()
            .find(|scope| feature.native_ref.as_deref() == Some(scope.id.as_str()))
            .unwrap();
        let parameter = ir
            .model
            .parameters
            .iter()
            .find(|parameter| parameter.owner.as_ref() == Some(&feature.id))
            .unwrap();
        let parameter = native
            .design_parameters
            .iter()
            .find(|record| parameter.native_ref.as_deref() == Some(record.id.as_str()))
            .unwrap();
        (scope.record_index, parameter.record_index)
    };
    let expected = indices(decoded.ir());
    let (written, _) = super::source_less_analytic::synthesize_document(decoded.ir(), None);
    assert_eq!(indices(written.ir()), expected);
}
#[test]
fn source_less_sketch_keeps_feature_scope_index() {
    let bytes = include_bytes!("../../../tests/golden/fixtures/design_extrude_depth.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let scope_index = |ir: &cadmpeg_ir::document::CadIr| {
        let feature = ir
            .model
            .features
            .iter()
            .find(|feature| {
                matches!(
                    feature.evaluation.definition(),
                    cadmpeg_ir::features::FeatureDefinition::Operation(
                        cadmpeg_ir::features::FeatureOperation::Sketch { .. }
                    )
                )
            })
            .unwrap();
        f3d_native(ir)
            .design_parameter_scopes
            .iter()
            .find(|scope| feature.native_ref.as_deref() == Some(scope.id.as_str()))
            .unwrap()
            .record_index
    };
    let expected = scope_index(decoded.ir());
    let (written, _) = super::source_less_analytic::synthesize_document(decoded.ir(), None);
    assert_eq!(scope_index(written.ir()), expected);
}
#[test]
fn source_less_sketch_keeps_relation_record_indices() {
    let bytes = include_bytes!("../../../tests/golden/fixtures/design_extrude_depth.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let expected: Vec<_> = f3d_native(decoded.ir())
        .sketch_relations
        .iter()
        .map(|relation| relation.record_index)
        .collect();
    assert!(!expected.is_empty());
    let (written, _) = super::source_less_analytic::synthesize_document(decoded.ir(), None);
    let actual: Vec<_> = written
        .ir()
        .model
        .sketch_constraints
        .iter()
        .map(|constraint| {
            f3d_native(written.ir())
                .sketch_relations
                .iter()
                .find(|relation| constraint.native_ref.as_deref() == Some(relation.id.as_str()))
                .unwrap()
                .record_index
        })
        .collect();
    assert_eq!(actual, expected);
}
#[test]
fn source_less_decal_keeps_edited_image_and_target() {
    let bytes = include_bytes!("../../../tests/golden/fixtures/design_decal.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut edited = decoded.ir().clone();
    edited.model.assets[0].content = cadmpeg_ir::assets::AssetContent::Embedded {
        data: cadmpeg_ir::assets::AssetData::new(b"edited decal".to_vec()).unwrap(),
    };
    let (written, mut report) = super::source_less_analytic::synthesize_document(&edited, None);
    report.coverage = cadmpeg_ir::codec::write::ArenaCoverage::Complete;
    cadmpeg_test_support::contract::assert_written_arenas_agree(&report, &edited, written.ir());
}
#[test]
fn source_less_canvas_keeps_edited_image_bytes() {
    let bytes = include_bytes!("../../../tests/golden/fixtures/design_canvas.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut edited = decoded.ir().clone();
    edited.model.assets[0].content = cadmpeg_ir::assets::AssetContent::Embedded {
        data: cadmpeg_ir::assets::AssetData::new(b"edited image".to_vec()).unwrap(),
    };
    let mut definition = edited.model.features[0].evaluation.definition().clone();
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::ReferenceImage { frame, .. },
    ) = &mut definition
    else {
        panic!("Canvas projects an image")
    };
    *frame = frame.with_origin(
        cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(13.0, 27.0, 41.0))
            .unwrap(),
    );
    edited.model.features[0]
        .evaluation
        .set_definition(definition);
    let (written, mut report) = super::source_less_analytic::synthesize_document(&edited, None);
    let cadmpeg_ir::codec::write::ArenaCoverage::Declared(coverage) = &report.coverage else {
        panic!("synthesis declares arena coverage")
    };
    assert_eq!(
        coverage.assets,
        cadmpeg_ir::codec::write::ArenaDisposition::Written
    );
    report.coverage = cadmpeg_ir::codec::write::ArenaCoverage::Complete;
    cadmpeg_test_support::contract::assert_written_arenas_agree(&report, &edited, written.ir());
}
#[test]
fn source_less_canvas_mirrored_reports_omission() {
    let bytes = include_bytes!("../../../tests/golden/fixtures/design_canvas.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut edited = decoded.ir().clone();
    let mut definition = edited.model.features[0].evaluation.definition().clone();
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::ReferenceImage { mirror_u, .. },
    ) = &mut definition
    else {
        panic!("Canvas projects an image")
    };
    *mirror_u = true;
    edited.model.features[0]
        .evaluation
        .set_definition(definition);
    let (written, report) = super::source_less_analytic::synthesize_document(&edited, None);
    assert!(written.ir().model.assets.is_empty());
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::WriterAssetOmitted.kind()));
}
#[test]
fn source_less_form_keeps_native_scope_and_cage() {
    let bytes = include_bytes!("../../../tests/golden/fixtures/design_form.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let (written, mut report) =
        super::source_less_analytic::synthesize_document(decoded.ir(), None);
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::WriterDesignRecordsOmitted.kind()));
    report.coverage = cadmpeg_ir::codec::write::ArenaCoverage::Complete;
    cadmpeg_test_support::contract::assert_written_arenas_agree(
        &report,
        decoded.ir(),
        written.ir(),
    );
}
#[test]
fn source_less_copy_paste_keeps_edited_occurrence_placement() {
    let bytes = include_bytes!("../../../tests/golden/fixtures/design_copy_paste.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let mut edited = decoded.ir().clone();
    let occurrence = edited
        .model
        .occurrences
        .iter_mut()
        .find(|occurrence| occurrence.ordinal == 1)
        .unwrap();
    let translation =
        cadmpeg_ir::features::FiniteVector3::new(cadmpeg_ir::math::Vector3::new(13.0, 27.0, 41.0))
            .unwrap();
    occurrence.transform = Some(occurrence.transform.unwrap().with_translation(translation));
    let (written, mut report) = super::source_less_analytic::synthesize_document(&edited, None);
    let cadmpeg_ir::codec::write::ArenaCoverage::Declared(coverage) = &report.coverage else {
        panic!("synthesis declares arena coverage");
    };
    assert_eq!(
        coverage.product_definitions,
        cadmpeg_ir::codec::write::ArenaDisposition::Written
    );
    assert_eq!(
        coverage.occurrences,
        cadmpeg_ir::codec::write::ArenaDisposition::Written
    );
    report.coverage = cadmpeg_ir::codec::write::ArenaCoverage::Complete;
    cadmpeg_test_support::contract::assert_written_arenas_agree(&report, &edited, written.ir());
}
#[test]
fn source_less_surface_stitch_keeps_its_input_and_gap() {
    let bytes = include_bytes!("../../../tests/golden/fixtures/design_surface_stitch.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let (written, mut report) =
        super::source_less_analytic::synthesize_document(decoded.ir(), None);
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::WriterDesignRecordsOmitted.kind()));
    report.coverage = cadmpeg_ir::codec::write::ArenaCoverage::Complete;
    cadmpeg_test_support::contract::assert_written_arenas_agree(
        &report,
        decoded.ir(),
        written.ir(),
    );
}
#[test]
fn source_less_remove_body_keeps_group_without_untyped_member() {
    let bytes = include_bytes!("../../../tests/golden/fixtures/design_remove_body.f3d");
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    assert_eq!(
        decoded.ir().model.feature_input_topologies[0]
            .operands
            .len(),
        1
    );
    let (written, mut report) =
        super::source_less_analytic::synthesize_document(decoded.ir(), None);
    assert!(report
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::WriterDesignRecordsOmitted.kind()));
    report.coverage = cadmpeg_ir::codec::write::ArenaCoverage::Complete;
    cadmpeg_test_support::contract::assert_written_arenas_agree(
        &report,
        decoded.ir(),
        written.ir(),
    );
}
#[test]
fn source_less_remove_body_keeps_its_input_group() {
    use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
    use crate::test_support::zip_test::f3d_with_smbh_and_protein_with_generated_remove_body;

    let bytes = f3d_with_smbh_and_protein_with_generated_remove_body(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let (written, mut report) =
        super::source_less_analytic::synthesize_document(decoded.ir(), None);
    report.coverage = cadmpeg_ir::codec::write::ArenaCoverage::Complete;
    cadmpeg_test_support::contract::assert_written_arenas_agree(
        &report,
        decoded.ir(),
        written.ir(),
    );
}
#[test]
fn source_less_native_base_flange_keeps_its_references() {
    use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
    use crate::test_support::zip_test::f3d_with_smbh_and_protein_with_generated_base_flange;

    let bytes = f3d_with_smbh_and_protein_with_generated_base_flange(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let (written, mut report) =
        super::source_less_analytic::synthesize_document(decoded.ir(), None);
    report.coverage = cadmpeg_ir::codec::write::ArenaCoverage::Complete;
    cadmpeg_test_support::contract::assert_written_arenas_agree(
        &report,
        decoded.ir(),
        written.ir(),
    );
}
#[test]
fn source_less_empty_base_feature_keeps_its_selection() {
    use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
    use crate::test_support::zip_test::f3d_with_smbh_and_protein_with_generated_base_feature;

    let bytes = f3d_with_smbh_and_protein_with_generated_base_feature(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let (written, mut report) =
        super::source_less_analytic::synthesize_document(decoded.ir(), None);
    report.coverage = cadmpeg_ir::codec::write::ArenaCoverage::Complete;
    cadmpeg_test_support::contract::assert_written_arenas_agree(
        &report,
        decoded.ir(),
        written.ir(),
    );
}
#[test]
fn source_less_typed_sketch_text_survives_synthesis() {
    use crate::records::entity_header::{DesignEntityHeader, SegmentType, SegmentTypeData};
    use crate::records::identity::ReferenceRun;

    let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
    let mut native = f3d_native_mut(&mut ir);
    native.design_types = [
        ("44A64366-4BD3-4B24-881A-F94C206E8F2D", "MSketch", 16, 42),
        ("E0618268-3A06-450E-9E94-7CF4C2E66802", "Geometry", 0, 20),
    ]
    .into_iter()
    .enumerate()
    .map(|(ordinal, (guid, module, version, entity))| {
        SegmentType::try_new(
            format!("f3d:generated/MetaStream.dat:design-type#{ordinal}"),
            SegmentTypeData {
                class_tag: (256 + ordinal).to_string().try_into().unwrap(),
                byte_offset: Some(u64::try_from(ordinal).unwrap()),
                module: module.into(),
                entities: ReferenceRun::unlocated(vec![entity]),
                type_guid: guid.to_owned().try_into().unwrap(),
                type_guid_offset: Some(0),
                base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
                version,
                version_offset: Some(0),
            },
        )
        .unwrap()
    })
    .collect();
    native.design_entity_headers = vec![DesignEntityHeader {
        id: "f3d:generated/BulkStream.dat:design-entity-header#0".into(),
        byte_offset: Some(0),
        entity_id: "0_42".to_owned().try_into().unwrap(),
        class_tag: "256".to_owned().try_into().unwrap(),
        optional_slot_present: Some(true),
        registration: crate::records::entity_header::DesignEntityRegistration::new(
            Some("MSketch".into()),
            Some(crate::records::entity_header::SketchHeaderReferences {
                record_reference: None,
                record_reference_offset: None,
                references: vec![crate::records::identity::Located {
                    value: 20,
                    offset: Some(0),
                }],
            }),
            ReferenceRun::unlocated(Vec::new()),
        )
        .unwrap(),
    }];
    let mut text = crate::test_support::sketch_text::neutral_text_record();
    text.id = "f3d:generated/BulkStream.dat:sketch-text#0".into();
    text.class_tag = "257".to_owned().try_into().unwrap();
    native.sketch_texts.push(text.clone());
    drop(native);
    let mut bytes = Vec::new();
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    assert!(plan.report().losses.iter().any(|loss| {
        loss.message.contains(&text.id) && loss.code.local_code() == "record.residual-unavailable"
    }));
    plan.write_to(&mut bytes).unwrap();
    let output = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let written = f3d_native(output.ir());
    assert_eq!(written.sketch_texts.len(), 1);
    assert_eq!(written.sketch_texts[0].text, text.text);
    assert_eq!(written.sketch_texts[0].layout, text.layout);
    assert_eq!(
        written.sketch_texts[0].owner_reference,
        text.owner_reference
    );

    let mut edited = ir.clone();
    let sketch = cadmpeg_ir::sketches::Sketch {
        id: "authored:model:sketch#42".try_into().unwrap(),
        name: Some("Sketch_42".into()),
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0),
            cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: Default::default(),
        native_ref: None,
    };
    let geometry = cadmpeg_ir::sketches::SketchGeometry::try_from(
        cadmpeg_ir::sketches::SketchGeometryDefinition::Text {
            text: cadmpeg_core::text::NonBlankString::new("Edited").unwrap(),
            font_family: cadmpeg_core::text::NonBlankString::new("Arial").unwrap(),
            font_weight: cadmpeg_ir::sketches::SketchFontWeight::Regular,
            height: cadmpeg_ir::scalar::Length::new(10.0).unwrap(),
            width_factor: Some(1.0),
            placement: Some(cadmpeg_ir::sketches::TextPlacement {
                anchor: cadmpeg_ir::math::Point2::new(0.0, 0.0),
                rotation: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
            }),
            horizontal_alignment: Some(cadmpeg_ir::sketches::SketchTextHorizontalAlignment::Left),
            vertical_alignment: Some(cadmpeg_ir::sketches::SketchTextVerticalAlignment::Top),
        },
    )
    .unwrap();
    edited.model.sketch_entities.push(
        cadmpeg_ir::sketches::SketchEntity::new(
            "authored:model:sketch-entity#text".try_into().unwrap(),
            sketch.id.clone(),
            geometry.clone(),
        )
        .with_native_ref(Some(text.id.clone())),
    );
    edited.model.sketches.push(sketch);
    let mut bytes = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&edited, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut bytes))
        .unwrap();
    let edited_output = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let edited_native = f3d_native(edited_output.ir());
    assert_eq!(edited_native.sketch_texts.len(), 1);
    assert_eq!(edited_native.sketch_texts[0].text, "Edited");
    assert_eq!(edited_output.ir().model.sketch_entities.len(), 1);
    assert_eq!(
        edited_output.ir().model.sketch_entities[0].geometry,
        geometry
    );

    let mut native = f3d_native_mut(&mut ir);
    native.design_types[1].set_entities(ReferenceRun::unlocated(vec![20, 21]));
    native.design_entity_headers[0].registration =
        crate::records::entity_header::DesignEntityRegistration::new(
            Some("MSketch".into()),
            Some(crate::records::entity_header::SketchHeaderReferences {
                record_reference: None,
                record_reference_offset: None,
                references: [20, 21]
                    .into_iter()
                    .map(|value| crate::records::identity::Located {
                        value,
                        offset: Some(0),
                    })
                    .collect(),
            }),
            ReferenceRun::unlocated(Vec::new()),
        )
        .unwrap();
    let mut rotated = text;
    rotated.id = "f3d:generated/BulkStream.dat:sketch-text#rotated".into();
    rotated.record_index = 21;
    let crate::records::sketch_geometry::SketchTextLayout::TextexTag {
        placement: Some(placement),
        ..
    } = &mut rotated.layout
    else {
        panic!("expected frame text");
    };
    placement.rotation = cadmpeg_ir::scalar::Angle::new(0.25).unwrap();
    let expected_layout = rotated.layout.clone();
    native.sketch_texts.push(rotated);
    drop(native);
    let plan = F3dCodec
        .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
        .unwrap();
    let mut bytes = Vec::new();
    plan.write_to(&mut bytes).unwrap();
    let output = F3dCodec
        .decode(&mut Cursor::new(bytes), &DecodeOptions::default())
        .unwrap();
    let written = f3d_native(output.ir());
    assert_eq!(written.sketch_texts.len(), 2);
    assert_eq!(
        written
            .sketch_texts
            .iter()
            .find(|text| text.record_index == 21)
            .unwrap()
            .layout,
        expected_layout
    );
}
#[test]
fn generated_source_less_writes_design_type_metastream() {
    use crate::records::entity_header::SegmentType;

    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let mut native = f3d_native_mut(&mut source_less);
    native.design_types = vec![
        SegmentType::try_new(
            "f3d:generated/MetaStream.dat:design-type#0".into(),
            crate::records::entity_header::SegmentTypeData {
                class_tag: (256u32).to_string().try_into().expect("fixture class tag"),
                byte_offset: Some(0),
                module: "Fusion".to_owned(),
                entities: crate::records::identity::ReferenceRun::unlocated(vec![1, 2]),
                type_guid: "11111111-2222-3333-4444-555555555555"
                    .to_owned()
                    .try_into()
                    .expect("type GUID"),
                type_guid_offset: Some(0),
                base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
                version: 7,
                version_offset: Some(0),
            },
        )
        .unwrap(),
        SegmentType::try_new(
            "f3d:generated/MetaStream.dat:design-type#1".into(),
            crate::records::entity_header::SegmentTypeData {
                class_tag: (257u32).to_string().try_into().expect("fixture class tag"),
                byte_offset: Some(1),
                module: crate::records::entity_header::DESIGN_MODULE_SKETCH.to_owned(),
                entities: crate::records::identity::ReferenceRun::unlocated(vec![277]),
                type_guid: "22222222-3333-4444-5555-666666666666"
                    .to_owned()
                    .try_into()
                    .expect("type GUID"),
                type_guid_offset: Some(0),
                base_type_guid: crate::records::entity_header::BaseTypeGuid::Guid {
                    value: "11111111-2222-3333-4444-555555555555"
                        .to_owned()
                        .try_into()
                        .expect("base GUID"),
                    offset: Some(0),
                },
                version: 9,
                version_offset: Some(0),
            },
        )
        .unwrap(),
        SegmentType::try_new(
            "f3d:generated/MetaStream.dat:design-type#2".into(),
            crate::records::entity_header::SegmentTypeData {
                class_tag: (258u32).to_string().try_into().expect("fixture class tag"),
                byte_offset: Some(2),
                module: "FutureFeature".to_owned(),
                entities: crate::records::identity::ReferenceRun::unlocated(vec![999]),
                type_guid: "33333333-4444-5555-6666-777777777777"
                    .to_owned()
                    .try_into()
                    .expect("type GUID"),
                type_guid_offset: Some(0),
                base_type_guid: crate::records::entity_header::BaseTypeGuid::Guid {
                    value: "11111111-2222-3333-4444-555555555555"
                        .to_owned()
                        .try_into()
                        .expect("base GUID"),
                    offset: Some(0),
                },
                version: 11,
                version_offset: Some(0),
            },
        )
        .unwrap(),
    ];

    drop(native);
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less Design MetaStream encode");
    let mut guid_module = source_less.clone();
    f3d_native_mut(&mut guid_module).design_types[2]
        .set_module("11111111-2222-3333-4444-555555555555".into());
    for scope in [
        cadmpeg_ir::document::ContentScope::full(),
        cadmpeg_ir::document::ContentScope::geometry(),
    ] {
        guid_module.set_scope(scope).expect("empty neutral design");
        let error = F3dCodec
            .plan(EncodeInput::new(&guid_module, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut Vec::new()))
            .expect_err("a GUID-shaped Design module name must not be emitted at any scope");
        assert!(error
            .to_string()
            .contains("Design type module name is GUID-shaped"));
    }
    f3d_native_mut(&mut source_less).design_types[0].set_base_type_guid(
        crate::records::entity_header::BaseTypeGuid::Guid {
            value: "22222222-3333-4444-5555-666666666666"
                .to_owned()
                .try_into()
                .expect("base GUID"),
            offset: Some(0),
        },
    );
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("a cyclic Design type hierarchy must not be emitted");
    assert!(error
        .to_string()
        .contains("Design type hierarchy contains a cycle"));
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less Design MetaStream round trip");
    let types = &f3d_native(round_trip.ir()).design_types;
    assert_eq!(types.len(), 3);
    let fusion = types
        .iter()
        .find(|design_type| {
            design_type.type_guid.as_str() == "11111111-2222-3333-4444-555555555555"
        })
        .expect("Fusion type");
    assert_eq!(fusion.module, "Fusion");
    assert_eq!(
        fusion.entities.values().copied().collect::<Vec<_>>(),
        [1, 2]
    );
    assert_eq!(fusion.version, 7);
    assert_eq!(
        fusion.base_type_guid,
        crate::records::entity_header::BaseTypeGuid::Absent
    );
    let sketch = types
        .iter()
        .find(|design_type| {
            design_type.module == crate::records::entity_header::DESIGN_MODULE_SKETCH
        })
        .expect("sketch-module type");
    assert_eq!(sketch.entities.values().copied().collect::<Vec<_>>(), [277]);
    assert_eq!(
        sketch
            .base_type_guid
            .value()
            .map(crate::records::mesh::DesignRelaxedGuidText::as_str),
        Some("11111111-2222-3333-4444-555555555555")
    );
    assert_eq!(sketch.version, 9);
    let future = types
        .iter()
        .find(|design_type| design_type.module == "FutureFeature")
        .expect("forward-compatible module");
    assert_eq!(future.entities.values().copied().collect::<Vec<_>>(), [999]);
    assert_eq!(future.version, 11);
}
#[test]
fn generated_source_less_writes_design_recipes_and_persistent_references() {
    use crate::records::{
        recipes::{ConstructionRecipe, ConstructionRecipeKind},
        references::{LostEdgeReference, PersistentReference, PersistentReferenceKind},
    };

    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let mut native = f3d_native_mut(&mut source_less);
    native.design_types = (0..=163_u32)
        .map(|ordinal| {
            crate::records::entity_header::SegmentType::try_new(
                format!("f3d:generated/MetaStream.dat:design-type#{}", ordinal + 100),
                crate::records::entity_header::SegmentTypeData {
                    class_tag: (256 + ordinal).to_string().try_into().unwrap(),
                    byte_offset: Some(u64::from(ordinal + 100)),
                    module: "Geometry".into(),
                    entities: crate::records::identity::ReferenceRun::unlocated(
                        if ordinal == 163 {
                            vec![4645, 4646]
                        } else {
                            Vec::new()
                        },
                    ),
                    type_guid: format!("12345678-1234-1234-1234-{ordinal:012x}")
                        .try_into()
                        .unwrap(),
                    type_guid_offset: Some(0),
                    base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
                    version: 1,
                    version_offset: Some(0),
                },
            )
            .unwrap()
        })
        .collect();
    native.construction_recipes = [
        ConstructionRecipeKind::Body,
        ConstructionRecipeKind::Face,
        ConstructionRecipeKind::BoundedFace,
        ConstructionRecipeKind::Edge,
        ConstructionRecipeKind::Vertex,
    ]
    .into_iter()
    .enumerate()
    .map(|(ordinal, kind)| ConstructionRecipe {
        id: format!("f3d:generated:recipe#{ordinal}"),
        byte_offset: Some(0),
        kind,
        design: Some(crate::records::recipes::ConstructionRecipeDesign {
            id: crate::records::identity::RecordedValue {
                value: format!("{}", 320 + ordinal),
                offset: None,
            },
            selector: Some(crate::records::recipes::ConstructionRecipeSelector {
                value: 0,
                byte_offset: None,
            }),
        }),
        recipe_index: 0,
        record_index: Some(crate::records::identity::RecordedValue {
            value: 100 + i32::try_from(ordinal).unwrap(),
            offset: None,
        }),
    })
    .collect();
    native.persistent_references = vec![
        PersistentReference {
            id: "f3d:generated:persistent-reference#0".into(),
            byte_offset: None,
            value_offset: None,
            kind: PersistentReferenceKind::Point,
            value: 900,
        },
        PersistentReference {
            id: "f3d:generated:persistent-reference#1".into(),
            byte_offset: None,
            value_offset: None,
            kind: PersistentReferenceKind::CurvePrimary,
            value: 100,
        },
        PersistentReference {
            id: "f3d:generated:persistent-reference#2".into(),
            byte_offset: None,
            value_offset: None,
            kind: PersistentReferenceKind::CurveSecondary,
            value: 500,
        },
    ];
    native.lost_edge_references = vec![LostEdgeReference::new(
        "f3d:generated:lost-edge-reference#0".into(),
        0,
        "419".into(),
        4645,
        "419".into(),
        4646,
    )
    .expect("valid lost-edge record layout")];

    drop(native);
    let mut reordered = source_less.clone();
    {
        let mut native = f3d_native_mut(&mut reordered);
        let original = native.design_types.remove(163);
        let mut data = (*original).clone();
        data.byte_offset = Some(10);
        native.design_types.push(
            crate::records::entity_header::SegmentType::try_new(
                "f3d:generated/MetaStream.dat:design-type#10".into(),
                data,
            )
            .unwrap(),
        );
    }
    let mut reordered_bytes = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&reordered, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut reordered_bytes))
        .expect("reordered Design registry encode");
    let reordered_output = F3dCodec
        .decode(&mut Cursor::new(reordered_bytes), &DecodeOptions::default())
        .expect("reordered Design registry decode");
    let reordered_native = f3d_native(reordered_output.ir());
    assert_eq!(reordered_native.lost_edge_references.len(), 1);
    assert_eq!(
        reordered_native.lost_edge_references[0].class_tag.as_str(),
        "256"
    );
    assert_eq!(
        reordered_native.lost_edge_references[0]
            .next_class_tag
            .as_str(),
        "256"
    );
    assert_eq!(reordered_native.persistent_references.len(), 3);
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less Design BulkStream encode");
    let mut absent_selector = source_less.clone();
    f3d_native_mut(&mut absent_selector).construction_recipes[0]
        .design
        .as_mut()
        .unwrap()
        .selector = None;
    let selector_plan = F3dCodec
        .plan(
            EncodeInput::new(&absent_selector, None),
            TargetRequest::Inherit,
        )
        .unwrap();
    let selector_id = &f3d_native(&absent_selector).construction_recipes[0].id;
    assert_eq!(
        selector_plan
            .report()
            .native_dispositions
            .record_disposition("f3d", "construction_recipes", selector_id),
        cadmpeg_ir::codec::write::NativeDisposition::Dropped
    );
    assert!(selector_plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(selector_id)));
    f3d_native_mut(&mut source_less).construction_recipes[0].recipe_index = 1;
    let plan = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .expect("an unrepresentable recipe is dropped");
    let dropped = &f3d_native(&source_less).construction_recipes[0].id;
    assert_eq!(
        plan.report().native_dispositions.record_disposition(
            "f3d",
            "construction_recipes",
            dropped
        ),
        cadmpeg_ir::codec::write::NativeDisposition::Dropped
    );
    assert!(plan
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains(dropped)));
    let mut reduced = Vec::new();
    plan.write_to(&mut reduced).unwrap();
    let reduced = F3dCodec
        .decode(&mut Cursor::new(reduced), &DecodeOptions::default())
        .unwrap();
    assert_eq!(f3d_native(reduced.ir()).construction_recipes.len(), 4);
    let mut archive = zip::ZipArchive::new(Cursor::new(&encoded)).expect("generated F3D ZIP");
    let mut bulkstream = Vec::new();
    archive
        .by_name("FusionAssetName[Active]/Design1/BulkStream.dat")
        .expect("generated Design BulkStream")
        .read_to_end(&mut bulkstream)
        .expect("read generated Design BulkStream");
    for name in [
        b"body_recipe_data".as_slice(),
        b"face_recipe_data".as_slice(),
        b"bounded_face_recipe_data".as_slice(),
        b"edge_recipe_data".as_slice(),
        b"vertex_recipe_data".as_slice(),
    ] {
        let offset = bulkstream
            .windows(name.len())
            .position(|window| window == name)
            .expect("generated recipe name");
        assert_eq!(
            u32::from_le_bytes(bulkstream[offset - 4..offset].try_into().unwrap()),
            u32::try_from(name.len()).unwrap()
        );
        let payload = offset + name.len();
        assert_eq!(
            i64::from_le_bytes(bulkstream[payload..payload + 8].try_into().unwrap()),
            -1
        );
        assert_eq!(
            (0..5)
                .map(|ordinal| {
                    let at = payload + 8 + ordinal * 4;
                    i32::from_le_bytes(bulkstream[at..at + 4].try_into().unwrap())
                })
                .collect::<Vec<_>>(),
            [2, 0, -1, 1, -1]
        );
    }
    for name in [
        b"pt_tag".as_slice(),
        b"crv_primary_id".as_slice(),
        b"crv_secondary_id".as_slice(),
    ] {
        let offset = bulkstream
            .windows(name.len())
            .position(|window| window == name)
            .expect("generated persistent-reference name");
        let payload = offset + name.len();
        assert_eq!(
            &bulkstream[payload..payload + 8],
            &[2, 0, 0, 0, 14, 0, 0, 0]
        );
        assert_eq!(&bulkstream[payload + 8..payload + 22], &[0; 14]);
        assert_eq!(
            u32::from_le_bytes(bulkstream[payload + 22..payload + 26].try_into().unwrap()),
            23
        );
        assert_eq!(
            &bulkstream[payload + 26..payload + 49],
            b"IntrinsicMetaTypeuint64"
        );
    }
    crate::test_support::zip_test::with_scan(&encoded, |scan| {
        let meta = scan
            .parsed_metastream("FusionAssetName[Active]/Design1/MetaStream.dat")
            .unwrap();
        assert!(meta.records.iter().any(|record| record.entity_id == 4645));
        assert!(meta.records.iter().any(|record| record.entity_id == 4646));
    });
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less Design BulkStream round trip");
    let native = f3d_native(round_trip.ir());
    assert_eq!(native.construction_recipes.len(), 5);
    let body_recipe = native
        .construction_recipes
        .iter()
        .find(|recipe| recipe.kind == ConstructionRecipeKind::Body)
        .expect("body recipe");
    assert_eq!(body_recipe.record_index.map(|index| index.value), Some(100));
    assert_eq!(
        body_recipe
            .design
            .as_ref()
            .map(|design| design.id.value.as_str()),
        Some("320")
    );
    assert!(native
        .construction_recipes
        .iter()
        .any(|recipe| recipe.kind == ConstructionRecipeKind::BoundedFace));
    let bounded = native
        .construction_recipes
        .iter()
        .find(|recipe| recipe.kind == ConstructionRecipeKind::BoundedFace)
        .expect("bounded-face recipe");
    assert_eq!(
        bounded
            .design
            .as_ref()
            .map(|design| design.id.value.as_str()),
        Some("322")
    );
    assert_eq!(bounded.record_index.map(|index| index.value), Some(102));
    assert_eq!(native.persistent_references.len(), 3);
    assert_eq!(
        native
            .persistent_references
            .iter()
            .map(|reference| reference.value)
            .collect::<Vec<_>>(),
        [900, 100, 500]
    );
    assert_eq!(
        native.persistent_references[1].kind,
        PersistentReferenceKind::CurvePrimary
    );
    assert_eq!(native.lost_edge_references.len(), 1);
    assert_eq!(native.lost_edge_references[0].class_tag.as_str(), "419");
    assert_eq!(native.lost_edge_references[0].record_index, 4645);
    assert_eq!(
        native.lost_edge_references[0].next_class_tag.as_str(),
        "419"
    );
    assert_eq!(native.lost_edge_references[0].next_record_index, 4646);
}
#[test]
fn generated_source_less_writes_design_ownership_and_record_headers() {
    use crate::records::{
        bodies::DesignBodyMember,
        decal::DesignRecordHeader,
        entity_header::{DesignEntityHeader, SegmentType},
    };

    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let mut native = f3d_native_mut(&mut source_less);
    native.design_types = vec![SegmentType::try_new(
        "f3d:generated/MetaStream.dat:design-type#0".into(),
        crate::records::entity_header::SegmentTypeData {
            class_tag: (256u32).to_string().try_into().expect("fixture class tag"),
            byte_offset: Some(0),
            module: crate::records::entity_header::DESIGN_MODULE_SKETCH.to_owned(),
            entities: crate::records::identity::ReferenceRun::unlocated(vec![277]),
            type_guid: "22222222-3333-4444-5555-666666666666"
                .to_owned()
                .try_into()
                .expect("type GUID"),
            type_guid_offset: Some(0),
            base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
            version: 4,
            version_offset: Some(0),
        },
    )
    .unwrap()];
    for ordinal in 1..=95_u32 {
        let entities = match ordinal {
            94 => vec![33],
            95 => vec![44],
            _ => Vec::new(),
        };
        native.design_types.push(
            SegmentType::try_new(
                format!("f3d:generated/MetaStream.dat:design-type#{ordinal}"),
                crate::records::entity_header::SegmentTypeData {
                    class_tag: (256 + ordinal).to_string().try_into().unwrap(),
                    byte_offset: Some(u64::from(ordinal)),
                    module: "Geometry".into(),
                    entities: crate::records::identity::ReferenceRun::unlocated(entities),
                    type_guid: format!("12345678-1234-1234-1234-{ordinal:012x}")
                        .try_into()
                        .unwrap(),
                    type_guid_offset: Some(0),
                    base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
                    version: 1,
                    version_offset: Some(0),
                },
            )
            .unwrap(),
        );
    }
    native.design_body_members = vec![
        DesignBodyMember::try_from(crate::records::bodies::DesignBodyMemberContent {
            id: "f3d:generated:design-body-member#0".into(),
            byte_offset: None,
            entity_suffix: 985,
            flags: 0,
        })
        .unwrap(),
        DesignBodyMember::try_from(crate::records::bodies::DesignBodyMemberWire {
            id: "f3d:generated:design-body-member#1".into(),
            byte_offset: 1,
            entity_suffix: 8422,
            flags: 3,
        })
        .unwrap(),
    ];
    assert!(native.design_body_members[0].byte_offset().is_none());
    native.design_entity_headers = vec![DesignEntityHeader {
        id: "f3d:generated:entity-header#0".into(),
        byte_offset: Some(0),

        entity_id: crate::records::identity::DesignEntityId::try_from("0_277".to_owned())
            .expect("valid entity ID"),
        class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        optional_slot_present: Some(true),
        registration: crate::records::entity_header::DesignEntityRegistration::new(
            Some(crate::records::entity_header::DESIGN_MODULE_SKETCH.to_owned()),
            Some(crate::records::entity_header::SketchHeaderReferences {
                record_reference: Some(584),
                record_reference_offset: None,
                references: vec![33, 44]
                    .into_iter()
                    .map(|value| crate::records::identity::Located {
                        value,
                        offset: Some(0),
                    })
                    .collect(),
            }),
            crate::records::identity::ReferenceRun::unlocated(Vec::new()),
        )
        .expect("valid module registration"),
    }];
    native.design_record_headers = vec![
        DesignRecordHeader {
            id: "f3d:generated:record-header#0".into(),
            record_index: 33,
            class_tag: crate::records::references::DesignClassTag::try_from("350".to_owned())
                .unwrap(),
            byte_offset: None,
        },
        DesignRecordHeader {
            id: "f3d:generated:record-header#1".into(),
            record_index: 44,
            class_tag: crate::records::references::DesignClassTag::try_from("351".to_owned())
                .unwrap(),
            byte_offset: None,
        },
    ];

    drop(native);
    let mut unregistered = source_less.clone();
    f3d_native_mut(&mut unregistered).design_types[94]
        .set_entities(crate::records::identity::ReferenceRun::unlocated(Vec::new()));
    let plan = F3dCodec
        .plan(
            EncodeInput::new(&unregistered, None),
            TargetRequest::Inherit,
        )
        .expect("unregistered header degrades");
    assert!(plan.report().losses.iter().any(|loss| {
        loss.message.contains("f3d:generated:record-header#0")
            && loss.code.local_code() == "writer.native-records-omitted"
    }));
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less Design ownership encode");
    {
        let mut native = f3d_native_mut(&mut source_less);
        native.design_entity_headers[0].registration =
            crate::records::entity_header::DesignEntityRegistration::new(
                Some("Body".to_owned()),
                None,
                crate::records::identity::ReferenceRun::unlocated(Vec::new()),
            )
            .expect("body registration");
    }
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("cross-stream modules must not diverge");
    assert!(error
        .to_string()
        .contains("module conflicts with MetaStream ownership"));
    crate::test_support::zip_test::with_scan(&encoded, |scan| {
        let bulk_name = "FusionAssetName[Active]/Design1/BulkStream.dat";
        let bulk = scan.entry_bytes(bulk_name).unwrap();
        let meta = scan
            .parsed_metastream(&bulk_name.replace("BulkStream.dat", "MetaStream.dat"))
            .unwrap();
        assert!(scan.unindexed_record_runs.is_empty());
        assert!(meta.secondary_records.is_empty());
        assert_eq!(meta.records.len(), 4);
        assert_eq!(meta.records[0].entity_id, 278);
        assert_eq!(meta.records[0].bulk_offset, 0);
        assert_eq!(meta.records[1].entity_id, 33);
        assert_eq!(meta.records[1].bulk_offset, 72);
        assert_eq!(meta.records[2].entity_id, 44);
        assert_eq!(meta.records[2].bulk_offset, 83);
        assert_eq!(meta.records[3].entity_id, 277);
        assert_eq!(meta.records[3].bulk_offset, 94);
        assert_eq!(&bulk[72..83], &[3, 0, 0, 0, b'3', b'5', b'0', 33, 0, 0, 0]);
        assert_eq!(&bulk[83..94], &[3, 0, 0, 0, b'3', b'5', b'1', 44, 0, 0, 0]);
        assert_eq!(&bulk[94..105], &[3, 0, 0, 0, b'2', b'5', b'6', 21, 1, 0, 0]);
        assert_eq!(bulk.len(), 164);
    });
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less Design ownership round trip");
    let native = f3d_native(round_trip.ir());
    assert_eq!(native.design_body_members.len(), 2);
    assert_eq!(native.design_body_members[0].entity_suffix, 985);
    assert_eq!(native.design_body_members[1].flags, 3);
    assert_eq!(native.design_entity_headers.len(), 1);
    assert_eq!(native.design_entity_headers[0].entity_id.as_str(), "0_277");
    assert_eq!(
        native.design_entity_headers[0].declared_reference_count(),
        Some(2)
    );
    assert_eq!(
        native.design_entity_headers[0]
            .sketch_references()
            .and_then(|list| list.record_reference),
        Some(584)
    );
    assert_eq!(
        native.design_entity_headers[0]
            .reference_values()
            .copied()
            .collect::<Vec<_>>(),
        [33, 44]
    );
    assert_eq!(native.design_record_headers.len(), 2);
    assert_eq!(native.design_record_headers[0].record_index, 33);
    assert_eq!(native.design_record_headers[1].class_tag.as_str(), "351");
}

#[test]
fn generated_source_less_writes_sketch_points_curves_and_constraints() {
    use crate::records::{
        entity_header::{DesignEntityHeader, SegmentType},
        sketch_geometry::{SketchCurveGeometry, SketchCurveIdentity, SketchPoint},
        sketch_relations::{SketchRelation, SketchRelationMember, SketchRelationReturnMember},
    };
    use cadmpeg_ir::math::{Point2, Point3, Vector3};

    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let mut native = f3d_native_mut(&mut source_less);
    native.design_types = vec![
        SegmentType::try_new(
            "f3d:generated/MetaStream.dat:design-type#0".into(),
            crate::records::entity_header::SegmentTypeData {
                class_tag: (256u32).to_string().try_into().expect("fixture class tag"),
                byte_offset: Some(0),
                module: crate::records::entity_header::DESIGN_MODULE_SKETCH.to_owned(),
                entities: crate::records::identity::ReferenceRun::unlocated(vec![277]),
                type_guid: crate::design::decode::sketch::SKETCH_CONTAINER_TYPE_GUID
                    .to_owned()
                    .try_into()
                    .expect("type GUID"),
                type_guid_offset: Some(0),
                base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
                version: 1,
                version_offset: Some(0),
            },
        )
        .unwrap(),
        SegmentType::try_new(
            "f3d:generated/MetaStream.dat:design-type#1".into(),
            crate::records::entity_header::SegmentTypeData {
                class_tag: (257u32).to_string().try_into().expect("fixture class tag"),
                byte_offset: Some(1),
                module: crate::records::entity_header::DESIGN_MODULE_SKETCH.to_owned(),
                entities: crate::records::identity::ReferenceRun::unlocated(vec![33]),
                type_guid: "60403D47-0C49-49B0-BDE8-1679608164A2"
                    .to_owned()
                    .try_into()
                    .expect("type GUID"),
                type_guid_offset: Some(0),
                base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
                version: 1,
                version_offset: Some(0),
            },
        )
        .unwrap(),
        SegmentType::try_new(
            "f3d:generated/MetaStream.dat:design-type#2".into(),
            crate::records::entity_header::SegmentTypeData {
                class_tag: (258u32).to_string().try_into().expect("fixture class tag"),
                byte_offset: Some(2),
                module: "Geometry".into(),
                entities: crate::records::identity::ReferenceRun::unlocated(vec![100]),
                type_guid: "C2CEDAE7-1716-47C1-B7B1-07B70081D0FB"
                    .to_owned()
                    .try_into()
                    .expect("type GUID"),
                type_guid_offset: Some(0),
                base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
                version: 11,
                version_offset: Some(0),
            },
        )
        .unwrap(),
        SegmentType::try_new(
            "f3d:generated/MetaStream.dat:design-type#3".into(),
            crate::records::entity_header::SegmentTypeData {
                class_tag: (259u32).to_string().try_into().expect("fixture class tag"),
                byte_offset: Some(3),
                module: "Geometry".into(),
                entities: crate::records::identity::ReferenceRun::unlocated(vec![600]),
                type_guid: "DCA267ED-D615-4934-B64F-AD805E8003E2"
                    .to_owned()
                    .try_into()
                    .expect("type GUID"),
                type_guid_offset: Some(0),
                base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
                version: 2,
                version_offset: Some(0),
            },
        )
        .unwrap(),
        SegmentType::try_new(
            "f3d:generated/MetaStream.dat:design-type#4".into(),
            crate::records::entity_header::SegmentTypeData {
                class_tag: (260u32).to_string().try_into().expect("fixture class tag"),
                byte_offset: Some(4),
                module: "Geometry".into(),
                entities: crate::records::identity::ReferenceRun::unlocated(vec![601]),
                type_guid: "F0130424-8B7E-4092-93C9-1CA807482534"
                    .to_owned()
                    .try_into()
                    .expect("type GUID"),
                type_guid_offset: Some(0),
                base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
                version: 0,
                version_offset: Some(0),
            },
        )
        .unwrap(),
        SegmentType::try_new(
            "f3d:generated/MetaStream.dat:design-type#5".into(),
            crate::records::entity_header::SegmentTypeData {
                class_tag: (261u32).to_string().try_into().expect("fixture class tag"),
                byte_offset: Some(5),
                module: crate::records::entity_header::DESIGN_MODULE_SKETCH.to_owned(),
                entities: crate::records::identity::ReferenceRun::unlocated(vec![602]),
                type_guid: "D82E012F-6DDD-4AED-BDE1-C0F7F9100B9B"
                    .to_owned()
                    .try_into()
                    .expect("type GUID"),
                type_guid_offset: Some(0),
                base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
                version: 3,
                version_offset: Some(0),
            },
        )
        .unwrap(),
        SegmentType::try_new(
            "f3d:generated/MetaStream.dat:design-type#6".into(),
            crate::records::entity_header::SegmentTypeData {
                class_tag: (262u32).to_string().try_into().expect("fixture class tag"),
                byte_offset: Some(6),
                module: "Geometry".into(),
                entities: crate::records::identity::ReferenceRun::unlocated(vec![101]),
                type_guid: crate::design::decode::sketch::SKETCH_POINT_COMPANION_TYPE
                    .0
                    .to_owned()
                    .try_into()
                    .expect("type GUID"),
                type_guid_offset: Some(0),
                base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
                version: crate::design::decode::sketch::SKETCH_POINT_COMPANION_TYPE.1,
                version_offset: Some(0),
            },
        )
        .unwrap(),
    ];
    native.design_entity_headers = vec![DesignEntityHeader {
        id: "f3d:generated:sketch-header#0".into(),
        byte_offset: Some(0),

        entity_id: crate::records::identity::DesignEntityId::try_from("0_277".to_owned())
            .expect("valid entity ID"),
        class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        optional_slot_present: Some(true),
        registration: crate::records::entity_header::DesignEntityRegistration::new(
            Some(crate::records::entity_header::DESIGN_MODULE_SKETCH.to_owned()),
            Some(crate::records::entity_header::SketchHeaderReferences {
                record_reference: Some(584),
                record_reference_offset: None,
                references: vec![33]
                    .into_iter()
                    .map(|value| crate::records::identity::Located {
                        value,
                        offset: Some(0),
                    })
                    .collect(),
            }),
            crate::records::identity::ReferenceRun::unlocated(Vec::new()),
        )
        .expect("valid module registration"),
    }];
    native.sketch_points =
        vec![
            SketchPoint::try_from(crate::records::sketch_geometry::SketchPointDraft {
                id: "f3d:generated:sketch-point#0".into(),
                record_index: 100,
                byte_offset: None,
                coordinate_offset: Some(89),
                owner_reference: Some(277),
                class_tag: crate::records::references::DesignClassTag::try_from("258".to_owned())
                    .unwrap(),
                companion: crate::records::sketch_geometry::SketchPointCompanion {
                    incident_curves: Vec::new(),
                },
                record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
                    500,
                    crate::records::sketch_geometry::SketchPointClosure::Selector0State1,
                    Some(900),
                    0.0,
                ),
                paired_reference: 101,
                coordinates: Point2::new(12.5, -25.0),
            })
            .unwrap(),
        ];
    native.sketch_curve_identities = vec![
        SketchCurveIdentity {
            id: "f3d:generated:sketch-curve#0".into(),
            record_index: 600,
            owner_reference: Some(277),
            class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned())
                .unwrap(),
            byte_offset: None,
            geometry_offset: None,
            entity_genesis: Some(901),
            primary_id: std::num::NonZeroU64::new(700).unwrap(),
            secondary_id: 701,
            geometry: Some(
                SketchCurveGeometry::line(
                    Point3::new(10.0, 20.0, 0.0),
                    Point3::new(40.0, 20.0, 0.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                )
                .unwrap(),
            ),
            planar_z_noise: None,
        },
        SketchCurveIdentity {
            id: "f3d:generated:sketch-curve#1".into(),
            record_index: 601,
            owner_reference: Some(277),
            class_tag: crate::records::references::DesignClassTag::try_from("260".to_owned())
                .unwrap(),
            byte_offset: None,
            geometry_offset: None,
            entity_genesis: None,
            primary_id: std::num::NonZeroU64::new(702).unwrap(),
            secondary_id: 703,
            geometry: Some(
                SketchCurveGeometry::arc(
                    Point3::new(5.0, 6.0, 0.0),
                    Vector3::new(0.0, 0.0, 1.0),
                    Vector3::new(1.0, 0.0, 0.0),
                    30.0,
                    0.25,
                    2.5,
                )
                .unwrap(),
            ),
            planar_z_noise: None,
        },
        SketchCurveIdentity {
            id: "f3d:generated:sketch-curve#2".into(),
            record_index: 602,
            owner_reference: Some(277),
            class_tag: crate::records::references::DesignClassTag::try_from("261".to_owned())
                .unwrap(),
            byte_offset: None,
            geometry_offset: None,
            entity_genesis: None,
            primary_id: std::num::NonZeroU64::new(704).unwrap(),
            secondary_id: 705,
            geometry: Some(SketchCurveGeometry::nurbs_from_parts(
                None,
                crate::records::references::DesignClassTag::try_from("365".to_owned()).unwrap(),
                602,
                crate::records::sketch_geometry::SketchNurbsGeometry::from_parts(
                    2,
                    1.0e-8,
                    8,
                    vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                    crate::records::sketch_geometry::SketchNurbsPoles::from_wire(
                        vec![
                            Point3::new(0.0, 0.0, 0.0),
                            Point3::new(10.0, 20.0, 0.0),
                            Point3::new(30.0, 10.0, 0.0),
                        ],
                        vec![1.0, 0.8, 1.0],
                    )
                    .unwrap(),
                )
                .unwrap(),
            )),
            planar_z_noise: None,
        },
    ];
    native.sketch_relations = vec![SketchRelation::authored(
        crate::records::sketch_relations::SketchRelationContent {
            id: "f3d:generated:sketch-relation#0".into(),
            record_index: 33,
            class_tag: crate::records::references::DesignClassTag::try_from("257".to_owned())
                .unwrap(),
            owner_reference: 277,
            owner_entity_id: None,
            auxiliary_references: crate::records::identity::ReferenceRun::located(Vec::new()),
            rectangular_counted_reference_count: None,
            members: (vec![
                SketchRelationMember {
                    relation_ordinal: Some(0),
                    ..SketchRelationMember::from_index(100)
                },
                SketchRelationMember {
                    relation_ordinal: Some(0),
                    ..SketchRelationMember::from_index(600)
                },
            ])
            .try_into()
            .expect("uniform member resolution"),
            definition: crate::records::sketch_relations::SketchRelationDefinition::new(0x11, None)
                .expect("valid relation definition"),
            entity_genesis: None,
            return_members: (vec![
                SketchRelationReturnMember::from_index(600),
                SketchRelationReturnMember::from_index(100),
            ])
            .try_into()
            .expect("uniform member resolution"),
        },
    )];
    assert!(native.sketch_relations[0].byte_offset.is_none());
    assert!(native.sketch_relations[0].state_offset.is_none());
    assert!(native.sketch_relations[0].raw_bytes().is_none());

    let expected_geometries = native
        .sketch_curve_identities
        .iter()
        .map(|curve| curve.geometry.clone().unwrap())
        .collect::<Vec<_>>();
    drop(native);
    let mut encoded = Vec::new();
    crate::writer::generate::write_new(&source_less, &mut encoded)
        .expect("source-less sketch BulkStream encode");
    let mut extended_source_less = source_less.clone();
    {
        let mut archive = zip::ZipArchive::new(Cursor::new(&encoded)).expect("generated F3D ZIP");
        let mut bulkstream = Vec::new();
        archive
            .by_name("FusionAssetName[Active]/Design1/BulkStream.dat")
            .expect("generated Design BulkStream")
            .read_to_end(&mut bulkstream)
            .expect("read generated Design BulkStream");
        let mut companion = Vec::new();
        companion.extend_from_slice(&3u32.to_le_bytes());
        companion.extend_from_slice(b"262");
        companion.extend_from_slice(&101u32.to_le_bytes());
        companion.extend_from_slice(&[0; 15]);
        companion.push(1);
        companion.extend_from_slice(&100u64.to_le_bytes());
        companion.extend_from_slice(&[0; 2]);
        assert_eq!(companion.len(), 37);
        assert!(bulkstream
            .windows(companion.len())
            .any(|window| window == companion));
    }
    f3d_native_mut(&mut source_less).sketch_points[0].owner_reference = None;
    let (partial, report) = super::source_less_analytic::synthesize_document(&source_less, None);
    assert_eq!(
        partial.ir().model.bodies.len(),
        source_less.model.bodies.len()
    );
    assert!(f3d_native(partial.ir()).sketch_points.is_empty());
    super::source_less_analytic::assert_native_omission(&report);
    f3d_native_mut(&mut source_less).sketch_points[0].owner_reference = Some(277);
    f3d_native_mut(&mut source_less).design_types[6]
        .set_entities(crate::records::identity::ReferenceRun::unlocated(Vec::new()));
    // #239 §5.0: writer-detected input/precondition violations are invariant errors.
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut Vec::new()))
    }));
    let message = match &result {
        Ok(Err(error)) => error.to_string(),
        Err(panic) => panic.downcast_ref::<String>().cloned().unwrap_or_else(|| {
            panic
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .unwrap_or_default()
        }),
        Ok(Ok(_)) => panic!("source-less points require a registered inverse companion"),
    };
    assert!(
        message.contains("inverse companion")
            || message.contains("InvalidInput")
            || message.contains("unmapped F3D encode error"),
        "{message}"
    );
    f3d_native_mut(&mut source_less).design_types[6]
        .set_entities(crate::records::identity::ReferenceRun::unlocated(vec![101]));
    f3d_native_mut(&mut source_less).design_types[2].set_version(10);
    let error = crate::writer::generate::write_new(&source_less, &mut Vec::new())
        .expect_err("the native serializer requires its current writable class version");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
    f3d_native_mut(&mut source_less).design_types[2].set_version(11);
    {
        let relation = &mut f3d_native_mut(&mut source_less).sketch_relations[0];
        relation.edit_content(|content| {
            content.members = ([100, 600, 100, 600, 100, 600, 100, 600]
                .into_iter()
                .map(|record_index| SketchRelationMember {
                    relation_ordinal: Some(0),
                    ..SketchRelationMember::from_index(record_index)
                })
                .collect::<Vec<_>>())
            .try_into()
            .expect("uniform member resolution");
        });
        relation.edit_content(|content| {
            content.return_members = (content
                .members
                .iter()
                .rev()
                .map(|member| {
                    SketchRelationReturnMember::from_index(member.reference.record_index())
                })
                .collect::<Vec<_>>())
            .try_into()
            .expect("uniform member resolution");
        });
    }
    let mut variable_relation = Vec::new();
    crate::writer::generate::write_new(&source_less, &mut variable_relation)
        .expect("source-less variable-width sketch relation encode");
    let variable_round_trip = F3dCodec
        .decode(
            &mut Cursor::new(variable_relation),
            &DecodeOptions::default(),
        )
        .expect("source-less variable-width sketch relation round trip");
    assert_eq!(
        f3d_native(variable_round_trip.ir()).sketch_relations[0].member_indices(),
        vec![100, 600, 100, 600, 100, 600, 100, 600]
    );
    // Retained source bytes never serialize into the native document (§2.1),
    // so the variable width is asserted as content: all eight members and
    // all eight return members survived the round trip.
    assert_eq!(
        f3d_native(variable_round_trip.ir()).sketch_relations[0]
            .return_member_indices()
            .len(),
        8
    );
    {
        let relation = &mut f3d_native_mut(&mut source_less).sketch_relations[0];
        relation.edit_content(|content| {
            content.members = (vec![
                SketchRelationMember::from_index(100),
                SketchRelationMember::from_index(600),
            ])
            .try_into()
            .expect("uniform member resolution");
        });
        relation.edit_content(|content| {
            content.return_members = (vec![
                SketchRelationReturnMember::from_index(600),
                SketchRelationReturnMember::from_index(100),
            ])
            .try_into()
            .expect("uniform member resolution");
        });
    }
    f3d_native_mut(&mut source_less).sketch_relations[0].owner_reference = 999;
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("relations with missing sketch owners must not disappear");
    assert!(error
        .to_string()
        .contains("references missing sketch owner"));
    {
        let mut native = f3d_native_mut(&mut source_less);
        native.sketch_relations[0].owner_reference = 277;
        native.sketch_points[0].record_index = 600;
    }
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("duplicate typed sketch indices must not be deduplicated");
    assert!(error.to_string().contains("share record index 600"));
    f3d_native_mut(&mut source_less).sketch_points[0].record_index = 100;
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less sketch BulkStream round trip");
    let native = f3d_native(round_trip.ir());
    assert_eq!(native.sketch_points.len(), 1);
    assert_eq!(native.sketch_points[0].persistent_id(), Some(500));
    assert_eq!(native.sketch_points[0].entity_genesis(), Some(900));
    assert_eq!(native.sketch_points[0].coordinate_offset, Some(141));
    assert_eq!(native.sketch_points[0].owner_reference, Some(277));
    assert_eq!(native.sketch_points[0].depth(), 0.0);
    assert_eq!(
        native.sketch_points[0].closure(),
        Some(crate::records::sketch_geometry::SketchPointClosure::Selector0State1)
    );
    assert_eq!(
        native.sketch_points[0].companion(),
        crate::records::sketch_geometry::SketchPointCompanionRef {
            prefix_present_zero: false,
            incident_curves: &[],
        }
    );
    assert_eq!(
        native.sketch_points[0].coordinates(),
        Point2::new(12.5, -25.0)
    );
    assert_eq!(native.sketch_curve_identities.len(), 3);
    let genesis_curve = native
        .sketch_curve_identities
        .iter()
        .find(|curve| curve.primary_id.get() == 700)
        .expect("genesis curve");
    assert_eq!(genesis_curve.entity_genesis, Some(901));
    // A decoded curve with geometry records its geometry payload position;
    // authored content carries no positions.
    assert!(genesis_curve.geometry_offset.is_some());
    assert_eq!(genesis_curve.owner_reference, Some(277));
    assert!(native
        .sketch_curve_identities
        .iter()
        .all(|curve| curve.owner_reference == Some(277)));
    for expected in expected_geometries {
        assert!(native
            .sketch_curve_identities
            .iter()
            .any(|curve| curve.geometry.as_ref() == Some(&expected)));
    }
    assert_eq!(native.sketch_relations.len(), 1);
    assert_eq!(native.sketch_relations[0].member_indices(), vec![100, 600]);
    assert!(native.sketch_relations[0].auxiliary_references().is_empty());
    assert_eq!(native.sketch_relations[0].owner_reference, 277);
    assert_eq!(
        native.sketch_relations[0]
            .owner_entity_id
            .as_ref()
            .map(cadmpeg_core::text::NonBlankString::as_str),
        Some("0_277")
    );
    assert_eq!(native.sketch_relations[0].definition.state(), 0x11);
    assert_eq!(
        native.sketch_relations[0].return_member_indices(),
        vec![600, 100]
    );
    assert_eq!(
        native.sketch_relations[0].resolved_members(),
        [
            crate::records::sketch_relations::SketchRelationOperand::Point {
                record_index: 100,
                persistent_id: Some(500),
            },
            crate::records::sketch_relations::SketchRelationOperand::Curve {
                record_index: 600,
                primary_id: 700,
                secondary_id: 701,
            },
        ]
    );
    assert_eq!(
        native.sketch_relations[0].resolved_return_members(),
        [
            crate::records::sketch_relations::SketchRelationOperand::Curve {
                record_index: 600,
                primary_id: 700,
                secondary_id: 701,
            },
            crate::records::sketch_relations::SketchRelationOperand::Point {
                record_index: 100,
                persistent_id: Some(500),
            },
        ]
    );
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, round_trip.ir())
            .expect("validate the native namespace")
    })
    .is_empty());

    {
        let point = &mut f3d_native_mut(&mut extended_source_less).sketch_points[0];
        let persistent_id = std::num::NonZeroU64::new(point.persistent_id().unwrap()).unwrap();
        point
            .try_set_record_form(
                crate::records::sketch_geometry::SketchPointRecordForm::Version11 {
                    depth: 7.5,
                    entity_genesis: point.entity_genesis(),
                    padded_paired_reference: true,
                    derived_type: false,
                    companion_prefix_present_zero: true,
                    persistent_id,
                    flags: [true, false, false, true, true, false, true, false],
                    closure: crate::records::sketch_geometry::SketchPointClosure::Selector4State0,
                },
            )
            .unwrap();
        point
            .try_set_companion(crate::records::sketch_geometry::SketchPointCompanion {
                incident_curves: vec![600],
            })
            .unwrap();
    }
    let mut extended_encoded = Vec::new();
    crate::writer::generate::write_new(&extended_source_less, &mut extended_encoded)
        .expect("source-less extended sketch point encode");
    let extended_round_trip = F3dCodec
        .decode(
            &mut Cursor::new(extended_encoded),
            &DecodeOptions::default(),
        )
        .expect("source-less extended sketch point round trip");
    let extended_native = f3d_native(extended_round_trip.ir());
    let extended_point = &extended_native.sketch_points[0];
    assert_eq!(extended_point.depth(), 7.5);
    assert_eq!(extended_point.flags(), [1, 0, 0, 1, 1, 0, 1, 0]);
    assert_eq!(
        extended_point.closure(),
        Some(crate::records::sketch_geometry::SketchPointClosure::Selector4State0)
    );
    assert_eq!(
        extended_point.companion(),
        crate::records::sketch_geometry::SketchPointCompanionRef {
            prefix_present_zero: true,
            incident_curves: &[600],
        }
    );
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, extended_round_trip.ir())
            .expect("validate the native namespace")
    })
    .is_empty());

    let mut invalid_zero_marker = extended_source_less.clone();
    {
        let mut zero_native = f3d_native_mut(&mut invalid_zero_marker);
        let point = &mut zero_native.sketch_points[0];
        point
            .try_set_record_form(
                crate::records::sketch_geometry::SketchPointRecordForm::Version11 {
                    depth: 7.5,
                    entity_genesis: point.entity_genesis(),
                    padded_paired_reference: true,
                    derived_type: false,
                    companion_prefix_present_zero: true,
                    persistent_id: std::num::NonZeroU64::new(point.persistent_id().unwrap())
                        .unwrap(),
                    flags: [false, false, false, true, true, true, false, false],
                    closure: crate::records::sketch_geometry::SketchPointClosure::Selector4State0,
                },
            )
            .unwrap();
    }
    let mut refused_output = Vec::new();
    let error = F3dCodec
        .plan(
            EncodeInput::new(&invalid_zero_marker, None),
            TargetRequest::Inherit,
        )
        .and_then(|plan| plan.write_to(&mut refused_output))
        .expect_err("a nonzero point cannot carry the zero-coordinate marker");
    assert!(error.to_string().contains("flag lane 5"));

    let mut inconsistent = round_trip.ir().clone();
    f3d_native_mut(&mut inconsistent).sketch_relations[0].resolve_members(|record_index| {
        crate::records::sketch_relations::SketchRelationOperand::Point {
            record_index,
            persistent_id: Some(u64::MAX),
        }
    });
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &inconsistent).expect("validate the native namespace")
    })
    .iter()
    .any(|finding| {
        finding.check == cadmpeg_ir::report::check::Check::NativeLinks
            && finding.message.contains("typed operands disagree")
    }));

    let mut points = native.sketch_points.clone();
    let mut curves = native.sketch_curve_identities.clone();
    let mut relations = native.sketch_relations.clone();
    let mut conflicting_relation = relations[0].clone();
    let relation_scope = relations[0]
        .id
        .rsplit_once(':')
        .expect("generated relation identity has a stream")
        .0;
    conflicting_relation.id = format!("{relation_scope}:sketch-relation-conflict#1");
    conflicting_relation.owner_reference = 278;
    relations.push(conflicting_relation);
    let mut entities = native.design_entity_headers.clone();
    let mut second_owner = entities[0].clone();
    let entity_scope = entities[0]
        .id
        .rsplit_once(':')
        .expect("generated entity identity has a stream")
        .0;
    second_owner.id = format!("{entity_scope}:sketch-header-conflict#1");
    second_owner.entity_id = crate::records::identity::DesignEntityId::from_parts("0", 278);
    entities.push(second_owner);
    let error = crate::design::decode::sketch::bind_sketch_graph(
        &entities,
        &mut points,
        &mut curves,
        &mut [],
        &mut relations,
    )
    .expect_err("typed sketch geometry cannot belong to two sketches");
    assert!(error.to_string().contains("belongs to multiple sketches"));
}
