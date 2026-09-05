// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::default_trait_access,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]

use super::prelude::*;
use super::project_split_face;

fn group(
    scope_record_index: u32,
    scope_reference_ordinal: u32,
    record_index: u32,
    members: Vec<u32>,
    role: u64,
) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup {
        id: format!("f3d:Design/BulkStream.dat:group#{record_index}"),
        scope_record_index,
        scope_reference_ordinal,
        record_index,
        byte_offset: 0,
        class_tag: "262".into(),


        members: members.into_iter().map(|value| crate::records::Located { value, offset: 0 }).collect(),
        lost_edge_references: Vec::new(),
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 0,
            auxiliary_records: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_records: Vec::new(),
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 1,
            opaque_index_offset: 0,
            opaque_scalar: 0.0,
            opaque_scalar_offset: 0,
            variant: false,
        },
        role,
        extrude_role: None,
        role_offset: 0,
        paired_class_tag: "258".into(),
        paired_byte_offset: 0,
    }
}

#[test]
fn class_277_258_compact_split_face_frame_projects() {
    let scope_record_index = 77;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#77",
        crate::records::DesignFeatureKind::SplitFace,
        scope_record_index,
    );
    scope.class_tag = "277".into();
    scope.paired_class_tag = "258".into();
    scope.frame_length = 407;
    scope.reference_members = crate::records::ReferenceRun::Unlocated((100..112).collect());

    let groups = [
        group(scope_record_index, 0, 100, vec![101], 0x0000_0021_0000_0000),
        group(
            scope_record_index,
            2,
            102,
            (103..112).collect(),
            0x0000_0010_0000_0000,
        ),
    ];
    let definition = project_split_face(&scope, &[scope.clone()], &groups, &[], &[], &[])
        .expect("class-277 SplitFace frame");
    assert!(matches!(
        definition,
        FeatureDefinition::SplitFace {
            targets: FaceSelection::Native(targets),
            tool: cadmpeg_ir::features::SplitFaceTool::Path(
                cadmpeg_ir::features::PathRef::Native(tool)
            ),
        } if targets.ends_with("group#102") && tool.ends_with("group#100")
    ));

    scope.class_tag = "418".into();
    scope.paired_class_tag = "266".into();
    assert!(project_split_face(&scope, &[scope.clone()], &groups, &[], &[], &[]).is_some());

    scope.class_tag = "277".into();
    scope.paired_class_tag = "266".into();
    assert!(project_split_face(&scope, &[scope.clone()], &groups, &[], &[], &[]).is_none());
}

#[test]
fn direct_single_identity_split_face_member_projects_historical_edge_path() {
    let scope_record_index = 77;
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#77",
        crate::records::DesignFeatureKind::SplitFace,
        scope_record_index,
    );
    scope.class_tag = "277".into();
    scope.paired_class_tag = "258".into();
    scope.frame_length = 407;
    scope.previous_history_state_id = Some(7);
    scope.reference_members = crate::records::ReferenceRun::Unlocated((100..112).collect());

    let groups = [
        group(scope_record_index, 0, 100, vec![101], 0x0000_0021_0000_0000),
        group(
            scope_record_index,
            2,
            102,
            (103..112).collect(),
            0x0000_0010_0000_0000,
        ),
    ];
    let selections = [crate::records::DesignEntitySelectionOperand {
        id: "f3d:Design/BulkStream.dat:entity-selection#101".into(),
        scope_record_index,
        group_record_index: 100,
        group_member_ordinal: 0,
        record_index: 101,
        byte_offset: 0,
        class_tag: "277".into(),
        asset_id: "asset".into(),
        asset_id_offset: 0,
        context_id: "context".into(),
        context_id_offset: 0,
        identity_record_index: 102,
        identity_record_offset: 0,
        primary_identity: 225,
        primary_identity_offset: 0,
        secondary: None,
        historical_edge_candidates: Vec::new(),
        historical_face_candidates: Vec::new(),
        resolved_edge_slot: Some(42),
        next_record_index: 103,
        next_byte_offset: 0,
    }];

    let definition = project_split_face(&scope, &[scope.clone()], &groups, &selections, &[], &[])
        .expect("class-277 direct edge path");
    let FeatureDefinition::SplitFace {
        tool:
            cadmpeg_ir::features::SplitFaceTool::Path(cadmpeg_ir::features::PathRef::HistoricalEdges {
                state,
                edges,
                native,
            }),
        ..
    } = definition
    else {
        panic!("expected historical edge path");
    };
    let feature = crate::ids::neutral_feature_id(&scope);
    let prefix = crate::ids::history_input_prefix(
        feature
            .0
            .split_once('#')
            .map_or(feature.0.as_str(), |(_, key)| key),
        7,
    );
    assert_eq!(state, feature_input_topology_id(&feature, 7),);
    assert_eq!(edges, vec![crate::ids::history_input_edge_id(&prefix, 42)],);
    assert_eq!(native, groups[0].id);
}
