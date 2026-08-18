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
        member_offsets: vec![0; members.len()],
        members,
        lost_edge_references: Vec::new(),
        frame: crate::records::DesignConstructionOperandGroupFrame {
            member_count_offset: 0,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: Vec::new(),
            trailing_record_offsets: Vec::new(),
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
        extrude_face_role: None,
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
        "SplitFace",
        scope_record_index,
    );
    scope.class_tag = "277".into();
    scope.paired_class_tag = "258".into();
    scope.frame_length = 407;
    scope.reference_members = (100..112).collect();

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
