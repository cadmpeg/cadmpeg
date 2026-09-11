// SPDX-License-Identifier: Apache-2.0
#![allow(
    clippy::default_trait_access,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]

use super::prelude::*;
use super::project_split_face;
use crate::records::topology::DesignConstructionOperandGroupFrame;
use crate::records::topology::DesignOperandRole;
use cadmpeg_ir::features::FeatureOperation;

fn group(
    scope_record_index: u32,
    scope_reference_ordinal: u32,
    record_index: u32,
    members: Vec<u32>,
    role: DesignOperandRole,
) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(
        crate::records::topology::DesignConstructionOperandGroupDraft {
            id: format!("f3d:Design/BulkStream.dat:group#{record_index}"),
            scope_record_index,
            scope_reference_ordinal,
            record_index,
            byte_offset: 0,
            class_tag: crate::records::DesignClassTag::try_from("262".to_owned()).unwrap(),

            members: members
                .into_iter()
                .enumerate()
                .map(|(index, value)| crate::records::Located {
                    value,
                    offset: index as u64 * 11,
                })
                .collect(),
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 0,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 18,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 22,
                    variant: false,
                },
            )
            .unwrap(),
            operand_role: crate::records::topology::DesignConstructionOperandRole::Other(role),
            role_offset: 0,
            paired_class_tag: crate::records::DesignClassTag::try_from("258".to_owned()).unwrap(),
            paired_byte_offset: 0,
        },
    )
    .unwrap()
}

#[test]
fn class_277_258_compact_split_face_frame_projects() {
    let scope_record_index = 77;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#77",
        crate::records::feature::DesignFeatureKind::SplitFace,
        scope_record_index,
    );
    scope.class_tag = crate::records::DesignClassTag::try_from("277".to_owned()).unwrap();
    scope.paired_class_tag = crate::records::DesignClassTag::try_from("258".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 407;
            draft.reference_members = crate::records::ReferenceRun::unlocated((100..112).collect());
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();

    let groups = [
        group(
            scope_record_index,
            0,
            100,
            vec![101],
            DesignOperandRole::ROLE_0X21,
        ),
        group(
            scope_record_index,
            2,
            102,
            (103..112).collect(),
            DesignOperandRole::ROLE_0X10,
        ),
    ];
    let definition = project_split_face(&scope, &[scope.clone()], &groups, &[], &[], &[])
        .expect("class-277 SplitFace frame");
    assert!(matches!(
        definition,
        FeatureDefinition::Operation(FeatureOperation::SplitFace {
            targets: FaceSelection::Native(targets),
            tool: cadmpeg_ir::features::SplitFaceTool::Path(
                cadmpeg_ir::features::PathRef::Native(tool)
            ),
        }) if targets.ends_with("group#102") && tool.ends_with("group#100")
    ));

    scope.class_tag = crate::records::DesignClassTag::try_from("418".to_owned()).unwrap();
    scope.paired_class_tag = crate::records::DesignClassTag::try_from("266".to_owned()).unwrap();
    assert!(project_split_face(&scope, &[scope.clone()], &groups, &[], &[], &[]).is_some());

    scope.class_tag = crate::records::DesignClassTag::try_from("277".to_owned()).unwrap();
    scope.paired_class_tag = crate::records::DesignClassTag::try_from("266".to_owned()).unwrap();
    assert!(project_split_face(&scope, &[scope.clone()], &groups, &[], &[], &[]).is_none());
}

#[test]
fn direct_single_identity_split_face_member_projects_historical_edge_path() {
    let scope_record_index = 77;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#77",
        crate::records::feature::DesignFeatureKind::SplitFace,
        scope_record_index,
    );
    scope.class_tag = crate::records::DesignClassTag::try_from("277".to_owned()).unwrap();
    scope.paired_class_tag = crate::records::DesignClassTag::try_from("258".to_owned()).unwrap();
    scope
        .try_edit(|draft| {
            draft.frame_length = 407;
            draft.previous_history_state_id = Some(7);
            draft.reference_members = crate::records::ReferenceRun::unlocated((100..112).collect());
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();

    let groups = [
        group(
            scope_record_index,
            0,
            100,
            vec![101],
            DesignOperandRole::ROLE_0X21,
        ),
        group(
            scope_record_index,
            2,
            102,
            (103..112).collect(),
            DesignOperandRole::ROLE_0X10,
        ),
    ];
    let selections = [
        crate::records::topology::DesignEntitySelectionOperand::try_new(
            crate::records::topology::DesignEntitySelectionOperandDraft {
                id: "f3d:Design/BulkStream.dat:entity-selection#101".into(),
                scope_record_index,
                group_record_index: 100,
                group_member_ordinal: 0,
                record_index: 101,
                byte_offset: 0,
                class_tag: crate::records::DesignClassTag::try_from("277".to_owned()).unwrap(),
                asset_id: crate::records::DesignRelaxedGuidText::try_from(
                    "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
                )
                .unwrap(),
                asset_id_offset: 0,
                context_id: crate::records::DesignRelaxedGuidText::try_from(
                    "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
                )
                .unwrap(),
                context_id_offset: 0,
                identity_record_index: 104,
                identity_record_offset: 0,
                primary_identity: 225,
                primary_identity_offset: 21,
                secondary: None,
                historical_edge_candidates: Vec::new(),
                historical_face_candidates: Vec::new(),
                resolved_edge_slot: Some(42),
                next_record_index: 103,
                next_byte_offset: 29,
            },
        )
        .unwrap(),
    ];

    let definition = project_split_face(&scope, &[scope.clone()], &groups, &selections, &[], &[])
        .expect("class-277 direct edge path");
    let FeatureDefinition::Operation(FeatureOperation::SplitFace {
        tool:
            cadmpeg_ir::features::SplitFaceTool::Path(cadmpeg_ir::features::PathRef::HistoricalEdges {
                state,
                edges,
                native,
            }),
        ..
    }) = definition
    else {
        panic!("expected historical edge path");
    };
    let feature = crate::ids::neutral_feature_id(&scope);
    let prefix = crate::ids::history_input_prefix(
        feature
            .as_str()
            .split_once('#')
            .map_or(feature.as_str(), |(_, key)| key),
        7,
    );
    assert_eq!(state, feature_input_topology_id(&feature, 7),);
    assert_eq!(
        edges.as_slice(),
        vec![crate::ids::history_input_edge_id(&prefix, 42)],
    );
    assert_eq!(native.as_str(), groups[0].id);
}
