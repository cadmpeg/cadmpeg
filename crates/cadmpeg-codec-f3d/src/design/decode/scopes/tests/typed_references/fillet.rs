use super::{parse, typed_scope_frame};
use crate::design::decode::operands::{
    parse_construction_operand_group, ConstructionOperandGroupParse, RecordFrame,
};
use crate::design::decode::parameters::{parse_design_parameter_record, parse_parameter_owner};
use crate::design::feature_project::{
    project_parameter_design_with_edge_identities, ProjectInputs,
};
use crate::design::test_support::{
    parameter_owner_frame, parameter_record, synthetic_feature_timelines,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::parameters::{DesignParameter, DesignParameterOwner};
use crate::records::references::DesignClassTag;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::test_support::{indexed_header, push_marked_reference};
use cadmpeg_ir::features::edge_treatments::RadiusSpec;
use cadmpeg_ir::features::{EdgeSelection, FeatureDefinition, FeatureOperation};

const BULK: &str = "FusionAssetName[Active]/Design1/BulkStream.dat";

fn scope() -> DesignParameterScope {
    let mut scope = parse(&typed_scope_frame(5, "Fillet")).unwrap();
    scope.id = format!("{}:scope#226", crate::ids::native_scope(BULK));
    assert_eq!(scope.kind(), DesignFeatureKind::Fillet);
    assert!(scope.fixed_fillet_parameters().is_none());
    assert!(scope.fillet_sets().is_none());
    scope
}

fn group(
    scope: &DesignParameterScope,
    role: u64,
    full_round: bool,
) -> DesignConstructionOperandGroup {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"332", 222);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    push_marked_reference(&mut bytes, 234);
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&u32::from(full_round).to_le_bytes());
    if full_round {
        push_marked_reference(&mut bytes, 225);
    }
    bytes.extend_from_slice(&role.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&0.125f64.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    push_marked_reference(&mut bytes, 224);
    bytes.extend_from_slice(&[0, 0]);
    push_marked_reference(&mut bytes, 223);
    bytes.push(0);
    push_marked_reference(&mut bytes, 226);
    indexed_header(&mut bytes, *b"259", 222);
    let header = RecordFrame {
        record_index: 222,
        class_tag: DesignClassTag::try_from("332".to_owned()).unwrap(),
        byte_offset: 0,
    };
    let ConstructionOperandGroupParse::Complete(mut group) =
        parse_construction_operand_group(&bytes, scope, 0, &header)
    else {
        panic!("synthetic construction group");
    };
    assert_eq!(group.record_index, 222);
    assert_eq!(group.members()[0].value, 234);
    group.id = crate::ids::native_scoped_id(BULK, "design-construction-operand-group", 0);
    *group
}

fn owner() -> DesignParameterOwner {
    let mut bytes = parameter_owner_frame();
    for offset in [25, 68, 94] {
        bytes[offset..offset + 4].copy_from_slice(&226u32.to_le_bytes());
    }
    parse_parameter_owner(&bytes)
        .unwrap()
        .into_record(BULK, 0)
        .unwrap()
}

fn parameter(index: u32, owner: u32, expression: &str) -> DesignParameter {
    let mut bytes = parameter_record(Some(owner), expression, "Radius", Some("mm"), "d1", 0.7);
    bytes[7..11].copy_from_slice(&index.to_le_bytes());
    let mut parameter = parse_design_parameter_record(&bytes).unwrap();
    parameter.id = format!("{}:parameter#{index}", crate::ids::native_scope(BULK));
    parameter
}

#[test]
fn typed_fillet_without_radius_retains_group_and_missing_member() {
    let scope = scope();
    let group = group(&scope, 0x0000_0010_0000_0000, false);
    let scopes = std::slice::from_ref(&scope);
    let timelines = synthetic_feature_timelines(scopes);
    let projected = crate::test_support::with_decode_context(|ctx| {
        project_parameter_design_with_edge_identities(
            ctx,
            &ProjectInputs {
                scopes,
                timelines: &timelines,
                construction_groups: std::slice::from_ref(&group),
                ..Default::default()
            },
        )
        .unwrap()
    });
    assert_eq!(projected.features.len(), 1);
    let feature = &projected.features[0];
    let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) =
        feature.evaluation.definition()
    else {
        panic!("partial Fillet");
    };
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].radius, RadiusSpec::Unresolved { form: None });
    assert_eq!(groups[0].edges, EdgeSelection::Native(group.id.clone()));
    assert_eq!(groups[0].tangency_weight, None);
    assert_eq!(feature.source_properties["reference:0"], "222");
    assert_eq!(feature.source_properties["reference:1"], "234");
    let source: serde_json::Value =
        serde_json::from_str(&feature.source_properties["fillet:source"]).unwrap();
    assert_eq!(source["scope"]["record_index"], 226);
    assert_eq!(source["groups"][0], serde_json::to_value(&group).unwrap());
    assert!(source["owners"].as_array().unwrap().is_empty());
    assert!(projected
        .projection_losses
        .iter()
        .any(|note| note.code.local_code() == "feature.fillet-radius-unresolved"));
    assert!(projected
        .projection_losses
        .iter()
        .any(|note| note.code.local_code() == "feature.fillet-references-unresolved"));
}

#[test]
fn typed_fillet_without_group_keeps_scope_selection_and_reference_order() {
    let scope = scope();
    let scopes = std::slice::from_ref(&scope);
    let timelines = synthetic_feature_timelines(scopes);
    let projected = crate::test_support::with_decode_context(|ctx| {
        project_parameter_design_with_edge_identities(
            ctx,
            &ProjectInputs {
                scopes,
                timelines: &timelines,
                ..Default::default()
            },
        )
        .unwrap()
    });
    let feature = &projected.features[0];
    let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) =
        feature.evaluation.definition()
    else {
        panic!("partial Fillet");
    };
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].edges, EdgeSelection::Native(scope.id.clone()));
    assert_eq!(groups[0].radius, RadiusSpec::Unresolved { form: None });
    assert_eq!(feature.source_properties["reference:0"], "222");
    assert_eq!(feature.source_properties["reference:1"], "234");
}

#[test]
fn typed_fillet_keeps_missing_parameter_owner_and_source_only_expressions() {
    let scope = scope();
    let owner = owner();
    assert_eq!(owner.parameter_record_index(), 45);
    let mut other_stream = parameter(71, 234, "99 mm");
    other_stream.id = "f3d:other/BulkStream.dat:parameter#71".into();
    let parameters = [
        parameter(71, 234, "4 mm + 3 mm"),
        parameter(72, 234, "8 mm - 1 mm"),
        other_stream,
    ];
    let scopes = std::slice::from_ref(&scope);
    let timelines = synthetic_feature_timelines(scopes);
    let projected = crate::test_support::with_decode_context(|ctx| {
        project_parameter_design_with_edge_identities(
            ctx,
            &ProjectInputs {
                source_parameters: &parameters,
                scopes,
                owners: std::slice::from_ref(&owner),
                timelines: &timelines,
                ..Default::default()
            },
        )
        .unwrap()
    });
    let feature = &projected.features[0];
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Fillet { .. })
    ));
    let source: serde_json::Value =
        serde_json::from_str(&feature.source_properties["fillet:source"]).unwrap();
    assert_eq!(source["owners"][0], serde_json::to_value(&owner).unwrap());
    assert_eq!(source["owners"][0]["record_index"], 44);
    assert_eq!(source["owners"][0]["parameter_record_index"], 45);
    assert_eq!(source["owners"][0]["evaluated_value"], 6.0);
    assert_eq!(source["parameters"].as_array().unwrap().len(), 2);
    assert_eq!(source["parameters"][0]["expression"], "4 mm + 3 mm");
    assert_eq!(source["parameters"][1]["expression"], "8 mm - 1 mm");
    assert_eq!(source["parameters"][0]["name"], "d1");
    assert_eq!(source["parameters"][1]["name"], "d1");
    assert_eq!(source["parameters"][0]["owner_record_index"], 234);
    assert_eq!(source["parameters"][0]["unit"], "mm");
    assert_eq!(source["parameters"][0]["evaluated_value"], 0.7);
    assert_eq!(
        source["parameters"][0],
        serde_json::to_value(&parameters[0]).unwrap()
    );
    assert_eq!(
        source["parameters"][1],
        serde_json::to_value(&parameters[1]).unwrap()
    );
    assert!(projected
        .projection_losses
        .iter()
        .any(
            |note| note.code.local_code() == "feature.fillet-references-unresolved"
                && note.message.contains("45")
        ));
}

#[test]
fn typed_radius_fillet_keeps_existing_output_without_promotion_properties() {
    let scope = scope();
    let owner = owner();
    let parameter = parameter(45, 44, "7 mm");
    let scopes = std::slice::from_ref(&scope);
    let timelines = synthetic_feature_timelines(scopes);
    let projected = crate::test_support::with_decode_context(|ctx| {
        project_parameter_design_with_edge_identities(
            ctx,
            &ProjectInputs {
                native: std::slice::from_ref(&parameter),
                source_parameters: std::slice::from_ref(&parameter),
                owners: std::slice::from_ref(&owner),
                scopes,
                timelines: &timelines,
                ..Default::default()
            },
        )
        .unwrap()
    });
    let feature = &projected.features[0];
    let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) =
        feature.evaluation.definition()
    else {
        panic!("dimensioned Fillet");
    };
    assert_eq!(
        groups[0].radius,
        RadiusSpec::Constant {
            radius: cadmpeg_ir::scalar::PositiveLength::new(7.0).unwrap()
        }
    );
    assert_eq!(groups[0].edges, EdgeSelection::Native(scope.id.clone()));
    assert!(feature.source_properties.is_empty());
    assert!(projected.projection_losses.is_empty());
}

fn full_round_group(scope: &DesignParameterScope) -> DesignConstructionOperandGroup {
    use std::io::{Cursor, Write};

    let mut group = group(scope, 0x0000_0004_0000_0000, true);
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"321", 225);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&[1, 1, 0]);
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    crate::test_support::manifest_test::write_synthetic_manifests(&mut zip, stored);
    zip.start_file(BULK, stored).unwrap();
    zip.write_all(&bytes).unwrap();
    let archive = zip.finish().unwrap().into_inner();
    let header = crate::records::decal::DesignRecordHeader {
        id: format!("{}:record-header#225", crate::ids::native_scope(BULK)),
        record_index: 225,
        class_tag: DesignClassTag::try_from("321".to_owned()).unwrap(),
        byte_offset: 0,
    };
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        crate::design::decode::operands::bind_construction_operand_trailing_records(
            scan,
            std::slice::from_mut(&mut group),
            &[header],
        )
        .unwrap();
    });
    assert_eq!(group.frame.trailing_flags().len(), 1);
    assert!(group.frame.trailing_flags()[0].value);
    group
}

fn bounded_face(scope: &DesignParameterScope) -> crate::records::topology::face::DesignFaceOperand {
    use crate::records::recipes::{ConstructionRecipe, ConstructionRecipeKind};

    let mut bytes = Vec::new();
    for index in [234, 234, 235, 236, 237] {
        indexed_header(&mut bytes, *b"317", index);
    }
    indexed_header(&mut bytes, *b"281", 0);
    bytes.extend_from_slice(&24u32.to_le_bytes());
    let name_at = bytes.len();
    bytes.extend_from_slice(b"bounded_face_recipe_data");
    for value in [0i32, -1] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    indexed_header(&mut bytes, *b"325", 300);
    let header = crate::records::decal::DesignRecordHeader {
        id: format!("{}:record-header#234", crate::ids::native_scope(BULK)),
        record_index: 234,
        class_tag: DesignClassTag::try_from("317".to_owned()).unwrap(),
        byte_offset: 0,
    };
    let recipes = [ConstructionRecipe {
        id: format!("{}:recipe#237", crate::ids::native_scope(BULK)),
        byte_offset: u64::try_from(name_at).unwrap(),
        kind: ConstructionRecipeKind::BoundedFace,
        design: None,
        recipe_index: 0,
        record_index: None,
    }];
    crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::operands::parse_face_operand(
            ctx,
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            crate::design::decode::operands::FaceOperandFrame {
                scope,
                scope_reference_ordinal: 1,
                group_ownership: Some((222, 0)),
                next_byte_offset: None,
                header: &header,
            },
            &recipes,
        )
        .unwrap()
        .unwrap()
    })
}

#[test]
fn typed_fillet_full_round_requires_bounded_face_evidence() {
    use cadmpeg_ir::features::edge_treatments::FullRoundSideSelection;
    use cadmpeg_ir::features::FaceSelection;

    let scope = scope();
    let group = full_round_group(&scope);
    let face = bounded_face(&scope);
    let owner = owner();
    assert!(face.resolved_face_slots.is_empty());
    let scopes = std::slice::from_ref(&scope);
    let timelines = synthetic_feature_timelines(scopes);
    for (faces, owners, full_round) in [
        (std::slice::from_ref(&face), &[][..], true),
        (&[][..], &[][..], false),
        (
            std::slice::from_ref(&face),
            std::slice::from_ref(&owner),
            false,
        ),
    ] {
        let projected = crate::test_support::with_decode_context(|ctx| {
            project_parameter_design_with_edge_identities(
                ctx,
                &ProjectInputs {
                    scopes,
                    timelines: &timelines,
                    construction_groups: std::slice::from_ref(&group),
                    face_operands: faces,
                    owners,
                    ..Default::default()
                },
            )
            .unwrap()
        });
        let feature = &projected.features[0];
        if !full_round {
            assert!(matches!(
                feature.evaluation.definition(),
                FeatureDefinition::Operation(FeatureOperation::Fillet { .. })
            ));
        } else {
            let FeatureDefinition::Operation(FeatureOperation::FullRoundFillet { groups }) =
                feature.evaluation.definition()
            else {
                panic!("partial full-round Fillet");
            };
            assert_eq!(
                groups[0].center_faces(),
                &FaceSelection::Native(group.id.clone())
            );
            assert_eq!(
                groups[0].side_one_faces(),
                &FullRoundSideSelection::Automatic
            );
            assert_eq!(
                groups[0].side_two_faces(),
                &FullRoundSideSelection::Automatic
            );
            assert!(projected.projection_losses.iter().any(
                |note| note.code.local_code() == "feature.fillet-full-round-center-unresolved"
            ));
        }
        assert!(feature.source_properties.contains_key("fillet:source"));
    }
}
