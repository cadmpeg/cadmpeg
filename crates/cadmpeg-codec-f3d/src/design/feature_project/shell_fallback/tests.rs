use super::*;
use crate::records::topology::construction::{
    DesignConstructionOperandGroupDraft, DesignConstructionOperandGroupFrame,
    DesignConstructionOperandGroupFrameDraft, DesignConstructionOperandRole,
};
use crate::test_support::shell_slots::{fixture, Prefix};
use cadmpeg_ir::features::{DistinctMembers, FeatureContent, FeatureEvaluation, FeatureId};
use std::collections::BTreeMap;

fn group(record: u32, ordinal: u32, role: DesignOperandRole) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("f3d:Design/BulkStream.dat:operand-group#{record}"),
        scope_record_index: 90,
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

fn feature() -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint("test:model:feature#shell").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: Some(false),
        dependencies: DistinctMembers::default(),
        source_properties: BTreeMap::from([(
            cadmpeg_core::nonblank_literal!("reference:0"),
            "91".to_owned(),
        )]),
        source_tag: Some("Shell".into()),
        source_text: None,
        source_content: FeatureContent::default(),
        evaluation: FeatureEvaluation::new(
            FeatureDefinition::Operation(FeatureOperation::Native {
                kind: "Shell".into(),
                parameters: BTreeMap::from([(
                    cadmpeg_core::nonblank_literal!("Wall"),
                    "q + 1 mm".to_owned(),
                )]),
            }),
            DistinctMembers::default(),
        ),
        native_ref: Some("f3d:Design/BulkStream.dat:design-parameter-scope#90".to_owned()),
    }
}

fn projected(
    bytes: &[u8],
    scopes: &[DesignParameterScope],
    groups: &[DesignConstructionOperandGroup],
) -> (Feature, Vec<LossNote>) {
    let archive = crate::test_support::zip_test::f3d_with_smbh(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
    );
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        crate::test_support::with_decode_context(|ctx| {
            let inputs = Inputs {
                scan,
                ctx,
                scopes,
                groups,
                owners: &[],
                parameters: &[],
                faces: &[],
            };
            let records = IndexedRecordOffsets::build(bytes);
            let mut readouts: HashMap<String, Vec<Readout>> = HashMap::new();
            for scope in scopes {
                readouts
                    .entry(scope.id.clone())
                    .or_default()
                    .push(shell_observation::read(ctx, bytes, &records, scope).unwrap());
            }
            let mut feature = feature();
            let notes = project_one(&mut feature, &inputs, &readouts).unwrap();
            let once = feature.clone();
            assert!(project(std::slice::from_mut(&mut feature), &inputs)
                .unwrap()
                .is_empty());
            assert_eq!(feature, once);
            assert_eq!(feature.source_properties.get("reference:0").unwrap(), "91");
            let evidence: serde_json::Value =
                serde_json::from_str(feature.source_properties.get("shell:source").unwrap())
                    .unwrap();
            assert_eq!(evidence["native"]["parameters"]["Wall"], "q + 1 mm");
            (feature, notes)
        })
    })
}

fn has(notes: &[LossNote], code: &str) -> bool {
    notes.iter().any(|note| note.code.local_code() == code)
}

#[test]
fn counted_nonzero_control_projects_the_inside_wall_with_named_assumptions() {
    let (bytes, scope) = fixture(Prefix::Counted(1), Some((93, 0.25)), None, &[91]);
    let (feature, notes) = projected(
        &bytes,
        &[scope],
        &[group(91, 0, DesignOperandRole::ROLE_0X10)],
    );
    let FeatureDefinition::Operation(FeatureOperation::Shell {
        thickness,
        outward,
        removed_faces,
        mode,
        join,
        ..
    }) = feature.evaluation.definition()
    else {
        panic!()
    };
    assert_eq!(thickness.unwrap().get(), 2.5);
    assert_eq!(*outward, Some(false));
    assert!(matches!(removed_faces, FaceSelection::Native(_)));
    assert!(mode.is_none() && join.is_none());
    assert!(has(&notes, "feature.shell-counted-control-assumed"));
    assert!(has(&notes, "feature.shell-slot-direction-assumed"));
    assert!(has(&notes, "feature.shell-prefix-flag-assumed"));
}

#[test]
fn cornered_face_and_body_groups_both_survive_projection() {
    let (bytes, scope) = fixture(Prefix::Cornered(true), Some((93, 0.5)), None, &[91, 92]);
    let (feature, notes) = projected(
        &bytes,
        &[scope],
        &[
            group(91, 0, DesignOperandRole::ROLE_0X10),
            group(92, 1, DesignOperandRole::BODIES_A),
        ],
    );
    let FeatureDefinition::Operation(FeatureOperation::Shell {
        thickness,
        removed_faces,
        bodies,
        join,
        ..
    }) = feature.evaluation.definition()
    else {
        panic!()
    };
    assert_eq!(thickness.unwrap().get(), 5.0);
    assert!(matches!(removed_faces, FaceSelection::Native(_)));
    assert!(matches!(bodies, Some(BodySelection::Native(_))));
    assert_eq!(*join, Some(ShellJoin::Arc));
    assert!(has(&notes, "feature.shell-group-roles-assumed"));
}

#[test]
fn missing_wall_definition_keeps_a_shell_and_the_wall_reference() {
    let (mut bytes, scope) = fixture(Prefix::Counted(1), Some((93, 0.25)), None, &[91]);
    bytes.truncate(500);
    let (feature, notes) = projected(&bytes, &[scope], &[]);
    let FeatureDefinition::Operation(FeatureOperation::Shell {
        thickness,
        outward,
        removed_faces,
        ..
    }) = feature.evaluation.definition()
    else {
        panic!()
    };
    assert!(thickness.is_none());
    assert_eq!(*outward, Some(false));
    assert!(matches!(removed_faces, FaceSelection::Unresolved));
    assert!(has(&notes, "feature.shell-wall-value-unresolved"));
    assert!(has(&notes, "feature.shell-selection-unresolved"));
    let source: serde_json::Value =
        serde_json::from_str(feature.source_properties.get("shell:source").unwrap()).unwrap();
    assert!(source["readouts"][0]["frames"]
        .as_array()
        .unwrap()
        .iter()
        .any(|frame| frame["form"] == "counted" && frame["slots"][0]["record"] == 93));
}

#[test]
fn unequal_partial_walls_keep_both_values_and_both_sides() {
    let (bytes, scope) = fixture(Prefix::Counted(1), Some((93, 0.25)), Some((94, 0.5)), &[91]);
    let (feature, notes) = projected(
        &bytes,
        &[scope],
        &[group(91, 0, DesignOperandRole::ROLE_0X10)],
    );
    let FeatureDefinition::Operation(FeatureOperation::Shell {
        thickness,
        outward,
        mode,
        ..
    }) = feature.evaluation.definition()
    else {
        panic!()
    };
    assert!(thickness.is_none() && outward.is_none());
    assert_eq!(*mode, Some(ShellMode::BothSides));
    assert!(has(&notes, "feature.shell-independent-walls-native"));
    let source: serde_json::Value =
        serde_json::from_str(feature.source_properties.get("shell:source").unwrap()).unwrap();
    assert_eq!(source["readouts"][0]["scalars"][0]["value"], 0.25);
    assert_eq!(source["readouts"][0]["scalars"][1]["value"], 0.5);
}

#[test]
fn outside_slot_and_competing_face_groups_keep_independent_fields() {
    let (bytes, scope) = fixture(Prefix::Cornered(false), None, Some((93, 0.75)), &[91, 92]);
    let (feature, notes) = projected(
        &bytes,
        &[scope],
        &[
            group(91, 0, DesignOperandRole::ROLE_0X10),
            group(92, 1, DesignOperandRole::ROLE_0X10),
        ],
    );
    let FeatureDefinition::Operation(FeatureOperation::Shell {
        thickness,
        outward,
        removed_faces,
        join,
        ..
    }) = feature.evaluation.definition()
    else {
        panic!()
    };
    assert_eq!(thickness.unwrap().get(), 7.5);
    assert_eq!(*outward, Some(true));
    assert_eq!(*join, Some(ShellJoin::Intersection));
    assert!(matches!(removed_faces, FaceSelection::Unresolved));
    assert!(has(&notes, "feature.shell-selection-unresolved"));
}

#[test]
fn missing_source_still_projects_and_preserves_native_parameters() {
    let (feature, notes) = projected(&[], &[], &[]);
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Shell {
            thickness: None,
            removed_faces: FaceSelection::Unresolved,
            ..
        })
    ));
    assert!(has(&notes, "feature.shell-source-unavailable"));
}

#[test]
fn mismatched_scalar_owner_cannot_supply_neutral_thickness() {
    let (mut bytes, scope) = fixture(Prefix::Counted(1), Some((93, 0.25)), None, &[91]);
    bytes[525..529].copy_from_slice(&91u32.to_le_bytes());
    let (feature, notes) = projected(&bytes, &[scope], &[]);
    assert!(matches!(
        feature.evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Shell {
            thickness: None,
            outward: Some(false),
            ..
        })
    ));
    assert!(has(&notes, "feature.shell-wall-value-unresolved"));
}

#[test]
fn bound_scalar_selects_partial_counted_prefix_without_losing_the_wall() {
    let (mut bytes, scope) = fixture(Prefix::Counted(1), Some((93, 0.25)), None, &[91]);
    bytes[51..55].copy_from_slice(&u32::MAX.to_le_bytes());
    let (feature, notes) = projected(&bytes, &[scope], &[]);
    let FeatureDefinition::Operation(FeatureOperation::Shell {
        thickness, outward, ..
    }) = feature.evaluation.definition()
    else {
        panic!()
    };
    assert_eq!(thickness.unwrap().get(), 2.5);
    assert_eq!(*outward, Some(false));
    assert!(has(&notes, "feature.shell-envelope-assumed"));
}

#[test]
fn ambiguous_partial_slot_order_retains_the_common_magnitude() {
    let (mut bytes, scope) = fixture(Prefix::Counted(0), Some((93, 0.25)), None, &[91]);
    bytes[51..55].copy_from_slice(&u32::MAX.to_le_bytes());
    let (feature, notes) = projected(&bytes, &[scope], &[]);
    let FeatureDefinition::Operation(FeatureOperation::Shell {
        thickness,
        outward,
        mode,
        ..
    }) = feature.evaluation.definition()
    else {
        panic!()
    };
    assert_eq!(thickness.unwrap().get(), 2.5);
    assert!(outward.is_none() && mode.is_none());
    assert!(has(&notes, "feature.shell-wall-value-unresolved"));
}

#[test]
fn source_retention_refusal_leaves_the_feature_unchanged() {
    let archive = crate::test_support::zip_test::f3d_with_smbh(
        &crate::test_support::smbh_header_test::synthetic_smbh(),
    );
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = 0;
    crate::test_support::zip_test::with_scan(&archive, |scan| {
        crate::test_support::with_decode_policy(&policy, |ctx| {
            let inputs = Inputs {
                scan,
                ctx,
                scopes: &[],
                groups: &[],
                owners: &[],
                parameters: &[],
                faces: &[],
            };
            let mut feature = feature();
            let before = feature.clone();
            let result = project_one(&mut feature, &inputs, &HashMap::new());
            assert!(matches!(result, Err(CodecError::ResourceLimit(_))));
            assert_eq!(feature, before);
        });
    });
}
