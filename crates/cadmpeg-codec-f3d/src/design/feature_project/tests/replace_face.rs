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
    ConstructionRecipeKind, DesignBodyRecipeReference, DesignConstructionOperandGroupFrame,
    DesignOperandOwner, DesignSurfaceTrimCellEntry, DesignSurfaceTrimOperation,
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
        role_offset: 0,
        paired_class_tag: "258".into(),
        paired_byte_offset: 0,
    }
}

#[test]
fn replace_face_projects_role_order_and_historical_inputs() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#1129",
        crate::records::DesignFeatureKind::ReplaceFace,
        1129,
    );
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
        owner: DesignOperandOwner::Group {
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
        selector_tail: None,
        selector_tail_offset: None,
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
        group: Some(crate::records::DesignOperandGroup {
            group_record_index: 1137,
            group_member_ordinal: 0,
        }),
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

#[test]
fn surface_trim_projects_body_target_and_curve_tool() {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#1200",
        crate::records::DesignFeatureKind::SurfaceTrim,
        1200,
    );
    scope.reference_members = vec![1201, 1202, 1203, 1204];
    let target_group = group(1200, 0, 1201, 1202, 0x0000_0004_0000_0000);
    let tool_group = group(1200, 2, 1203, 1204, 0x0000_0021_0000_0000);
    let body = DesignBodyRecipeOperand {
        id: "f3d:Design/BulkStream.dat:body-recipe#1202".into(),
        scope_record_index: 1200,
        owner: DesignOperandOwner::Group {
            group_record_index: 1201,
            group_member_ordinal: 0,
        },
        record_index: 1202,
        byte_offset: 0,
        class_tag: "316".into(),
        asset_id: "asset".into(),
        asset_id_offset: 0,
        context_id: "context".into(),
        context_id_offset: 0,
        selector_tail: None,
        selector_tail_offset: None,
        references: vec![DesignBodyRecipeReference {
            design_reference: 326,
            design_reference_offset: 0,
            form: 33,
            form_offset: 0,
            candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            preceding_body_slots: Vec::new(),
        }],
        nested_record_index: 1205,
        nested_record_index_offset: 0,
        recipe_id: "f3d:Design/BulkStream.dat:recipe#1206".into(),
        resolved_face_slot: Some(20),
        resolved_body_state_id: Some(254),
        resolved_body_slot: Some(3),
        resolved_body_face_slots: vec![20],
        next_record_index: 1207,
        next_byte_offset: 0,
    };
    let definition = super::project_surface_trim(
        &scope,
        &[target_group.clone(), tool_group.clone()],
        std::slice::from_ref(&body),
    )
    .expect("typed SurfaceTrim");
    assert!(matches!(
        definition,
        FeatureDefinition::TrimSurface {
            faces: FaceSelection::Historical { ref faces, ref native, .. },
            tool: cadmpeg_ir::features::PathRef::Native(ref tool),
            keep: cadmpeg_ir::features::TrimRegion::Unresolved,
        } if faces.len() == 1
            && native == &target_group.id
            && tool == &tool_group.id
    ));
}

#[test]
fn surface_trim_binds_selected_cells_without_inventing_a_side() {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#1200",
        crate::records::DesignFeatureKind::SurfaceTrim,
        1200,
    );
    let mut feature = cadmpeg_ir::features::Feature::new(
        cadmpeg_ir::features::FeatureId::from("f3d:feature#1200"),
        0,
        FeatureDefinition::TrimSurface {
            faces: FaceSelection::Unresolved,
            tool: cadmpeg_ir::features::PathRef::Unresolved("tool".into()),
            keep: cadmpeg_ir::features::TrimRegion::Unresolved,
        },
    );
    feature.native_ref = Some(scope.id.clone());
    let operation = DesignSurfaceTrimOperation {
        id: "f3d:Design/BulkStream.dat:design-surface-trim-operation#1200".into(),
        scope_record_index: 1200,
        selection_record_index: 1,
        selection_byte_offset: 0,
        selection_next_record_index: 2,
        selection_next_byte_offset: 0,
        chain_records: Vec::new(),
        cell_table_record_index: 3,
        cell_table_byte_offset: 0,
        cell_table_class_tag: "325".into(),
        cell_table_frame_length: 0,
        cell_table_paired_class_tag: "257".into(),
        cell_table_paired_byte_offset: 0,
        cell_count: 2,
        cell_count_offset: 0,
        cell_entries: vec![
            DesignSurfaceTrimCellEntry {
                record_index: 4,
                record_reference_offset: 0,
                ordinal: 1,
                ordinal_offset: 0,
            },
            DesignSurfaceTrimCellEntry {
                record_index: 5,
                record_reference_offset: 0,
                ordinal: 4,
                ordinal_offset: 0,
            },
        ],
        trailing_value: 5,
        trailing_value_offset: 0,
        trailing_zero_offset: 0,
    };

    super::bind_surface_trim_cell_selections(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(&scope),
        std::slice::from_ref(&operation),
    );

    assert!(matches!(
        feature.definition,
        FeatureDefinition::TrimSurface {
            keep: cadmpeg_ir::features::TrimRegion::Cells(ref selection),
            ..
        } if selection.removed() == [1, 4] && selection.total() == 5
    ));
}
