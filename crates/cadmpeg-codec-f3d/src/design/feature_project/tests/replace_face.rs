// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]
use super::prelude::*;

use crate::records::{
    ConstructionRecipeKind, DesignBodyRecipeOperandOwner, DesignBodyRecipeReference,
    DesignConstructionOperandGroupFrame,
};
use cadmpeg_ir::features::{FaceSelection, FeatureDefinition};

fn group(
    scope_record_index: u32,
    scope_reference_ordinal: u32,
    record_index: u32,
    member: u32,
    role: u64,
) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup {
        id: format!("f3d:Design/BulkStream.dat:group#{record_index}"),
        scope_record_index,
        scope_reference_ordinal,
        record_index,
        byte_offset: 0,
        class_tag: "277".into(),
        members: vec![member],
        lost_edge_references: Vec::new(),
        member_offsets: vec![0],
        frame: DesignConstructionOperandGroupFrame {
            member_count_offset: 0,
            auxiliary_record_indices: Vec::new(),
            auxiliary_record_offsets: Vec::new(),
            auxiliary_paths: Vec::new(),
            trailing_record_indices: Vec::new(),
            trailing_record_offsets: Vec::new(),
            trailing_transforms: Vec::new(),
            trailing_dual_transforms: Vec::new(),
            trailing_flags: Vec::new(),
            opaque_index: 88,
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
fn replace_face_projects_role_order_and_historical_inputs() {
    let mut scope =
        DesignParameterScope::empty("f3d:Design/BulkStream.dat:scope#1129", "ReplaceFace", 1129);
    scope.class_tag = "301".into();
    scope.paired_class_tag = "258".into();
    scope.frame_length = 290;
    scope.previous_history_state_id = Some(254);
    scope.reference_members = vec![1130, 1133, 1137, 1140];

    let replacement_group = group(1129, 0, 1130, 1133, 0x0000_0009_0000_0000);
    let target_group = group(1129, 2, 1137, 1140, 0x0000_0010_0000_0000);
    let replacement = DesignBodyRecipeOperand {
        id: "f3d:Design/BulkStream.dat:body-recipe#1133".into(),
        scope_record_index: 1129,
        owner: DesignBodyRecipeOperandOwner::Group {
            group_record_index: 1130,
            group_member_ordinal: 0,
        },
        record_index: 1133,
        byte_offset: 0,
        class_tag: "316".into(),
        asset_id: "asset".into(),
        asset_id_offset: 0,
        context_id: "context".into(),
        context_id_offset: 0,
        references: vec![DesignBodyRecipeReference {
            design_reference: 326,
            design_reference_offset: 0,
            form: 33,
            form_offset: 0,
            candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            preceding_body_slots: Vec::new(),
        }],
        nested_record_index: 1136,
        nested_record_index_offset: 0,
        recipe_id: "f3d:Design/BulkStream.dat:recipe#1135".into(),
        resolved_face_slot: Some(20),
        resolved_body_state_id: Some(254),
        resolved_body_slot: Some(3),
        resolved_body_face_slots: vec![20],
        next_record_index: 1137,
        next_byte_offset: 0,
    };
    let target = DesignFaceOperand {
        id: "f3d:Design/BulkStream.dat:face-operand#1140".into(),
        scope_record_index: 1129,
        scope_reference_ordinal: 3,
        group_record_index: Some(1137),
        group_member_ordinal: Some(0),
        record_index: 1140,
        byte_offset: 0,
        class_tag: "272".into(),
        paired_byte_offset: 0,
        paired_class_tag: "258".into(),
        recipe_record_index: 1143,
        recipe_record_byte_offset: 0,
        recipe_id: "f3d:Design/BulkStream.dat:recipe#1142".into(),
        recipe_prefix_offset: 0,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_kind: ConstructionRecipeKind::BoundedFace,
        recipe_program_offset: 0,
        recipe_program: Vec::new(),
        recipe_node_offsets: Vec::new(),
        recipe_nodes: Vec::new(),
        candidate_faces: Vec::new(),
        unreferenced_candidate_faces: Vec::new(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: Vec::new(),
        resolved_face_slots: vec![622],
        resolved_active_face: None,
        next_record_index: 1144,
        next_byte_offset: 0,
    };

    let definition = super::project_replace_face(
        &scope,
        &[replacement_group.clone(), target_group.clone()],
        std::slice::from_ref(&target),
        std::slice::from_ref(&replacement),
    )
    .expect("typed ReplaceFace");
    assert!(matches!(
        definition,
        FeatureDefinition::ReplaceFace {
            targets: FaceSelection::Historical { ref faces, ref native, .. },
            replacements: FaceSelection::Historical {
                faces: ref replacement_faces,
                native: ref replacement_native,
                ..
            },
        } if faces.len() == 1
            && replacement_faces.len() == 1
            && native == &target_group.id
            && replacement_native == &replacement_group.id
    ));

    let mut invalid_scope = scope;
    invalid_scope.frame_length = 291;
    assert!(super::project_replace_face(
        &invalid_scope,
        &[replacement_group, target_group],
        std::slice::from_ref(&target),
        std::slice::from_ref(&replacement),
    )
    .is_none());
}
