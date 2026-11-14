use super::*;
use crate::records::feature::scope::{DesignFeatureKind, DesignSweepScope};
use crate::records::identity::ReferenceRun;
use crate::records::topology::construction::{
    DesignConstructionOperandGroupDraft, DesignConstructionOperandGroupFrame,
    DesignConstructionOperandGroupFrameDraft, DesignConstructionOperandRole,
};
use crate::records::topology::sketch_profile::{
    DesignSketchProfileOperand, DesignSketchProfileOperandDraft,
};
use cadmpeg_ir::features::{DistinctMembers, FeatureEvaluation};
use cadmpeg_ir::ids::BodyId;

const STREAM: &str = "f3d:Design/BulkStream.dat";

fn fixture() -> (Vec<u8>, DesignParameterScope) {
    let mut bytes = vec![0; 500];
    bytes[21..25].copy_from_slice(&1u32.to_le_bytes());
    for (record, ordinal, value) in [
        (20u32, 0u8, 0.75f64),
        (21, 1, 0.25),
        (22, 2, 1.0),
        (23, 3, 0.0),
        (24, 4, 0.0),
        (25, 5, -0.125),
    ] {
        let mut scalar = vec![0; 104];
        scalar[0..4].copy_from_slice(&3u32.to_le_bytes());
        scalar[4..7].copy_from_slice(b"277");
        scalar[7..11].copy_from_slice(&record.to_le_bytes());
        scalar[24] = 1;
        scalar[25..29].copy_from_slice(&10u32.to_le_bytes());
        scalar[35] = ordinal;
        scalar[40..48].copy_from_slice(&value.to_le_bytes());
        bytes.extend_from_slice(&scalar);
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"261");
        bytes.extend_from_slice(&record.to_le_bytes());
    }
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#10"),
        DesignFeatureKind::Sweep,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.byte_offset = 0;
            draft.reference_count_offset = 80;
            draft.reference_members = ReferenceRun::unlocated(vec![20, 21, 22, 23, 24, 25]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = 500;
            draft.frame_length = 500;
            draft.layout_fixture_tail();
        })
        .unwrap();
    (bytes, scope)
}

fn group(record: u32, ordinal: u32, role: DesignOperandRole) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("{STREAM}:operand-group#{record}"),
        scope_record_index: 10,
        scope_reference_ordinal: ordinal,
        record_index: record,
        byte_offset: 1000,
        class_tag: "332".to_owned().try_into().unwrap(),
        members: vec![crate::records::identity::Located {
            value: record + 100,
            offset: 1026,
        }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 1021,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: Vec::new(),
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 1,
                opaque_index_offset: 1072,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 1076,
                variant: false,
                closing_flag: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::Other(role),
        role_offset: 1054,
        paired_class_tag: "259".to_owned().try_into().unwrap(),
        paired_byte_offset: 1125,
    })
    .unwrap()
}

fn feature(scope: &DesignParameterScope) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint("test:model:feature#sweep").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: Default::default(),
        source_properties: Default::default(),
        source_tag: Some("Sweep".into()),
        source_text: None,
        source_content: Default::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "Sweep".into(),
                parameters: Default::default(),
            }),
            Default::default(),
        ),
        native_ref: Some(scope.id.clone()),
    }
}

fn projected(
    bytes: &[u8],
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
    outputs: &OutputFacts,
) -> (Feature, Vec<LossNote>) {
    let (_, source) = fixture();
    let mut feature = feature(&source);
    let archive = crate::test_support::zip_test::f3d_with_smbh(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
    );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        crate::design::test_support::with_test_decode_context(|ctx| {
            let inputs = Inputs {
                scan,
                ctx,
                scopes,
                groups,
                owners: &[],
                parameters: &[],
                edges: &[],
                edge_identities: &[],
                faces: &[],
            };
            let records = IndexedRecordOffsets::build(bytes);
            let observations = scopes
                .iter()
                .map(|scope| (scope.id.clone(), vec![read_scope(bytes, &records, scope)]))
                .collect();
            let notes = project_one(&mut feature, &inputs, outputs, &observations).unwrap();
            let once = feature.clone();
            assert!(
                project(std::slice::from_mut(&mut feature), &inputs, outputs)
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(feature, once);
            (feature.clone(), notes)
        })
    })
}

#[test]
fn altered_compact_control_keeps_join_and_all_six_independent_scalars() {
    let (bytes, scope) = fixture();
    let mut outputs = OutputFacts::default();
    outputs.0.insert(feature(&scope).id, BodyKind::Solid);
    let (feature, notes) = projected(
        &bytes,
        &[scope],
        &[
            group(30, 0, DesignOperandRole::PROFILE),
            group(31, 2, DesignOperandRole::ROLE_0X5),
        ],
        &outputs,
    );
    let FeatureDefinition::Operation(FeatureOperation::Sweep {
        shape,
        path_extent,
        twist,
        taper,
        path_tangent,
        linearize,
        ..
    }) = feature.evaluation.definition()
    else {
        panic!("Sweep")
    };
    assert_eq!(
        shape.mode(),
        SweepMode::Solid {
            op: SolidSweepOperation::Join
        }
    );
    assert_eq!(path_extent.as_ref().unwrap().along_fraction.get(), 0.75);
    assert_eq!(path_extent.as_ref().unwrap().against_fraction.get(), 0.25);
    assert_eq!(twist.unwrap().get(), 0.0);
    assert_eq!(taper.unwrap().get(), -0.125);
    assert!(!path_tangent && !linearize);
    assert!(notes
        .iter()
        .any(|note| note.code == F3dLossCode::FeatureSweepOperationPrefixAssumed.kind()));
    let evidence: serde_json::Value =
        serde_json::from_str(feature.source_properties.get("sweep:source").unwrap()).unwrap();
    assert_eq!(
        evidence["readouts"][0]["scalars"].as_array().unwrap().len(),
        6
    );
    assert_eq!(evidence["readouts"][0]["scalars"][4]["value_offset"], 1000);
}

#[test]
fn role43_section_is_independent_of_missing_scalar_and_unknown_operation() {
    let (mut bytes, scope) = fixture();
    bytes[21..25].copy_from_slice(&0u32.to_le_bytes());
    bytes[540..548].copy_from_slice(&f64::NAN.to_le_bytes());
    let mut outputs = OutputFacts::default();
    outputs.0.insert(feature(&scope).id, BodyKind::Solid);
    let (feature, notes) = projected(
        &bytes,
        &[scope],
        &[
            group(30, 0, DesignOperandRole::ROLE_0X43),
            group(31, 2, DesignOperandRole::ROLE_0X5),
        ],
        &outputs,
    );
    let FeatureDefinition::Operation(FeatureOperation::Sweep {
        shape,
        path,
        path_extent,
        twist,
        ..
    }) = feature.evaluation.definition()
    else {
        panic!("Sweep")
    };
    assert_eq!(shape.mode(), SweepMode::Unresolved {});
    assert!(path.is_some());
    assert!(path_extent.is_none());
    assert_eq!(twist.unwrap().get(), 0.0);
    assert!(notes
        .iter()
        .any(|note| note.code == F3dLossCode::FeatureSweepSectionRoleAssumed.kind()));
    assert!(notes
        .iter()
        .any(|note| note.code == F3dLossCode::FeatureSweepOperationUnresolved.kind()));
    assert!(serde_json::to_value(shape)
        .unwrap()
        .to_string()
        .contains("operand-group#30"));
}

#[test]
fn ambiguous_scalar_frames_do_not_erase_other_lanes_or_duplicate_intervening_bytes() {
    let (mut bytes, scope) = fixture();
    bytes.extend_from_within(500..615);
    let readout = read_scope(&bytes, &IndexedRecordOffsets::build(&bytes), &scope);
    assert_eq!(readout.assumed_join_offset, Some(21));
    assert!(!readout.scalars.iter().any(|scalar| scalar.record == 20));
    assert!(readout
        .scalars
        .iter()
        .any(|scalar| scalar.ordinal == 4 && scalar.value == 0.0));
    assert!(readout
        .fragments
        .iter()
        .all(|fragment| fragment.bytes.len() <= 104));
}

#[test]
fn missing_twist_is_reported_without_erasing_taper_or_extent() {
    let (mut bytes, scope) = fixture();
    bytes[1000..1008].copy_from_slice(&f64::NAN.to_le_bytes());
    let (feature, notes) = projected(&bytes, &[scope], &[], &OutputFacts::default());
    let FeatureDefinition::Operation(FeatureOperation::Sweep {
        twist,
        taper,
        path_extent,
        ..
    }) = feature.evaluation.definition()
    else {
        panic!("Sweep")
    };
    assert!(twist.is_none());
    assert_eq!(taper.unwrap().get(), -0.125);
    assert_eq!(path_extent.as_ref().unwrap().along_fraction.get(), 0.75);
    assert!(notes.iter().any(
        |note| note.code == F3dLossCode::FeatureSweepScalarUnresolved.kind()
            && note.message.contains("twist or taper")
    ));
}

#[test]
fn known_compact_operation_survives_an_unreadable_scalar_without_assumption() {
    let (mut bytes, scope) = fixture();
    bytes[21..25].copy_from_slice(&2u32.to_le_bytes());
    bytes[25] = 1;
    bytes[540..548].copy_from_slice(&f64::NAN.to_le_bytes());
    let mut outputs = OutputFacts::default();
    outputs.0.insert(feature(&scope).id, BodyKind::Solid);
    let (feature, notes) = projected(&bytes, &[scope], &[], &outputs);
    let FeatureDefinition::Operation(FeatureOperation::Sweep {
        shape,
        path_extent,
        taper,
        ..
    }) = feature.evaluation.definition()
    else {
        panic!("Sweep")
    };
    assert_eq!(
        shape.mode(),
        SweepMode::Solid {
            op: SolidSweepOperation::Cut
        }
    );
    assert!(path_extent.is_none());
    assert_eq!(taper.unwrap().get(), -0.125);
    assert!(!notes.iter().any(|note| note.code
        == F3dLossCode::FeatureSweepOperationPrefixAssumed.kind()
        || note.code == F3dLossCode::FeatureSweepOperationUnresolved.kind()));
}

#[test]
fn sheet_mode_requires_known_new_body_operation() {
    let (bytes, mut scope) = fixture();
    let mut outputs = OutputFacts::default();
    outputs.0.insert(feature(&scope).id, BodyKind::Sheet);
    for (operation, expected) in [
        (DesignExtrudeOperation::NewBody, SweepMode::Surface {}),
        (DesignExtrudeOperation::Cut, SweepMode::Unresolved {}),
    ] {
        scope
            .try_edit(|draft| {
                draft.payload = DesignScopePayload::Sweep(Some(DesignSweepScope {
                    construction: Some(
                        crate::records::feature::path_features::DesignSweepConstruction {
                            operation,
                            operation_offset: 21,
                            values: crate::test_support::reals([0.75, 0.25, 1.0, 0.0, 0.0, -0.125])
                                .to_vec(),
                            record_indexes: vec![20, 21, 22, 23, 24, 25],
                            value_offsets: vec![540, 655, 770, 885, 1000, 1115],
                        },
                    ),
                    sweep_profile: None,
                }))
            })
            .unwrap();
        let (feature, _) = projected(&bytes, &[scope.clone()], &[], &outputs);
        let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) =
            feature.evaluation.definition()
        else {
            panic!("Sweep")
        };
        assert_eq!(shape.mode(), expected);
    }
}

#[test]
fn profile_carrier_is_preserved_without_widening_to_whole_sketch() {
    let (bytes, mut scope) = fixture();
    let carrier = DesignSketchProfileOperand::try_new(DesignSketchProfileOperandDraft {
        scope_reference_ordinal: 0,
        record_index: 130,
        byte_offset: 2000,
        class_tag: "326".to_owned().try_into().unwrap(),
        asset_id: "00000000-0000-0000-0000-000000000001"
            .to_owned()
            .try_into()
            .unwrap(),
        asset_id_offset: 2020,
        entity_id: crate::records::identity::DesignEntityId::from_parts("SourceSketch", 201),
        entity_reference_offset: 2100,
        compact_tail_flags: None,
        region_selection: None,
        paired_class_tag: "261".to_owned().try_into().unwrap(),
        paired_byte_offset: 2200,
    })
    .unwrap();
    scope
        .try_edit(|draft| {
            draft.payload = DesignScopePayload::Sweep(Some(DesignSweepScope {
                construction: None,
                sweep_profile: Some(carrier),
            }))
        })
        .unwrap();
    let (feature, notes) = projected(
        &bytes,
        &[scope],
        &[
            group(30, 0, DesignOperandRole::PROFILE),
            group(31, 2, DesignOperandRole::ROLE_0X5),
            group(32, 4, DesignOperandRole::FACES),
        ],
        &OutputFacts::default(),
    );
    let value = serde_json::to_value(feature.evaluation.definition()).unwrap();
    assert!(value.to_string().contains("design-record-header#2000"));
    assert!(value.to_string().contains("operand-group#32"));
    assert!(notes
        .iter()
        .any(|note| note.code == F3dLossCode::FeatureSweepProfileCarrierAssumed.kind()));
}

#[test]
fn absent_or_competing_scopes_still_project_with_unresolved_fields() {
    let (bytes, scope) = fixture();
    for scopes in [Vec::new(), vec![scope.clone(), scope.clone()]] {
        let (feature, notes) = projected(&bytes, &scopes, &[], &OutputFacts::default());
        let evidence: serde_json::Value =
            serde_json::from_str(feature.source_properties.get("sweep:source").unwrap()).unwrap();
        if scopes.len() == 2 {
            assert_eq!(
                evidence["readouts"][0]["scalars"].as_array().unwrap().len(),
                6
            );
        }
        assert!(matches!(
            feature.evaluation.definition(),
            FeatureDefinition::Operation(FeatureOperation::Sweep { .. })
        ));
        assert!(notes
            .iter()
            .any(|note| note.code == F3dLossCode::FeatureSweepSourceUnavailable.kind()));
    }
}

#[test]
fn complete_output_facts_reject_partial_mixed_and_duplicate_bodies() {
    let (_, scope) = fixture();
    let mut feature = feature(&scope);
    let body = Body {
        id: BodyId::mint("test:model:body#one").unwrap(),
        kind: BodyKind::Solid,
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    };
    feature
        .evaluation
        .set_outputs(DistinctMembers::try_from(vec![body.id.clone()]).unwrap());
    let mut facts = OutputFacts::default();
    facts.record(&feature, std::slice::from_ref(&body), false);
    assert!(facts.0.is_empty());
    facts.record(&feature, &[body.clone(), body.clone()], true);
    assert!(facts.0.is_empty());
    facts.record(&feature, std::slice::from_ref(&body), true);
    assert_eq!(facts.0.get(&feature.id), Some(&BodyKind::Solid));
    let sheet = Body {
        id: BodyId::mint("test:model:body#two").unwrap(),
        kind: BodyKind::Sheet,
        ..body.clone()
    };
    feature
        .evaluation
        .set_outputs(DistinctMembers::try_from(vec![body.id.clone(), sheet.id.clone()]).unwrap());
    let mut facts = OutputFacts::default();
    facts.record(&feature, &[body, sheet], true);
    assert!(facts.0.is_empty());
}
