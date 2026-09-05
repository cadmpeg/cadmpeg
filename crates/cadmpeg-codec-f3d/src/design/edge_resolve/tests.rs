// SPDX-License-Identifier: Apache-2.0
#![allow(
    unused_imports,
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::trivially_copy_pass_by_ref,
    clippy::uninlined_format_args,
    clippy::wildcard_imports
)]

use super::*;
use crate::records::{
    DesignConstructionOperandGroup, DesignEdgeIdentityOperand, DesignEdgeOperand,
    DesignParameterScope, DesignRecipeReference,
};
use cadmpeg_ir::ids::EdgeId;

fn identity(record_index: u32, candidates: &[(i64, f64)]) -> DesignEdgeIdentityOperand {
    serde_json::from_value(serde_json::json!({
        "id": format!("f3d:test:identity#{record_index}"),
        "scope_record_index": 1,
        "group_record_index": 2,
        "group_member_ordinal": record_index,
        "record_index": record_index,
        "byte_offset": 0,
        "class_tag": "277",
        "compact_layout": true,
        "local_id": record_index,
        "local_id_offset": 0,
        "asset_id": "asset",
        "asset_id_offset": 0,
        "context_id": "context",
        "context_id_offset": 0,
        "transition_edge_candidates": candidates
            .iter()
            .map(|(edge, _)| *edge)
            .collect::<Vec<_>>(),
        "treatment_radius_candidates": candidates
            .iter()
            .map(|(edge, radius)| serde_json::json!({
                "edge_slot": edge,
                "radius": radius
            }))
            .collect::<Vec<_>>()
    }))
    .expect("edge identity")
}

#[test]
fn identity_radius_candidates_require_member_local_evidence() {
    let mut first = identity(10, &[(17, 3.0), (18, 5.0)]);
    let mut second = identity(11, &[(19, 3.0), (20, 5.0)]);
    assert_eq!(
        radius_edge_identity_group_candidates(&[&first], 3.0),
        Some(vec![17])
    );
    assert_eq!(
        radius_edge_identity_group_candidates(&[&first, &second], 3.0),
        None
    );
    let copied = identity(11, &[(17, 3.0), (18, 5.0)]);
    assert_eq!(
        radius_edge_identity_group_candidates(&[&first, &copied], 3.0),
        None
    );

    first.resolved_edge_slot = Some(17);
    second.resolved_edge_slots = vec![19, 20];
    assert_eq!(
        radius_edge_identity_group_candidates(&[&first, &second], 3.0),
        Some(vec![17, 19, 20])
    );
}

fn fixed_scope() -> DesignParameterScope {
    serde_json::from_value(serde_json::json!({
        "id": "f3d:test:scope",
        "byte_offset": 0,
        "class_tag": "300",
        "record_index": 1,
        "frame_length": 200,
        "kind": "Fillet",
        "kind_offset": 0,
        "feature_ordinal": 1,
        "feature_ordinal_offset": 0,
        "history_state_id": 8,
        "history_state_id_offset": 0,
        "previous_history_state_id": 7,
        "previous_history_state_id_offset": 0,
        "reference_count_offset": 0,
        "reference_members": [2],
        "reference_member_offsets": [0],
        "fixed_fillet_parameters": {
            "groups": [{
                "tangency_weight": {
                    "value": 1.0,
                    "record_index": 4,
                    "value_offset": 0
                },
                "radii": [0.3],
                "radius_record_indexes": [5],
                "radius_offsets": [0],
                "intermediate_parameters": [],
                "intermediate_parameter_record_indexes": [],
                "intermediate_parameter_offsets": []
            }]
        },
        "paired_class_tag": "261",
        "paired_byte_offset": 200
    }))
    .expect("fixed Fillet scope")
}

fn group(record_index: u32, member: u32) -> DesignConstructionOperandGroup {
    serde_json::from_value(serde_json::json!({
        "id": format!("f3d:test:group-{record_index}"),
        "scope_record_index": 1,
        "scope_reference_ordinal": 0,
        "record_index": record_index,
        "byte_offset": 0,
        "class_tag": "300",
        "members": [member],
        "member_offsets": [0],
        "frame": {
            "layout": "counted",
            "member_count_offset": 0,
            "identity_record_index": 9,
            "identity_record_offset": 0,
            "opaque_index": 1,
            "opaque_index_offset": 0,
            "opaque_scalar": 1.0,
            "opaque_scalar_offset": 0,
            "variant": false
        },
        "role": 0x10_0000_0000u64,
        "role_offset": 0,
        "paired_class_tag": "258",
        "paired_byte_offset": 0
    }))
    .expect("construction operand group")
}

#[test]
fn sole_compact_identity_group_projects_fixed_fillet_transition_chain() {
    let scope = fixed_scope();
    let group = group(2, 10);
    let identity = identity(10, &[(17, 3.0), (18, 5.0), (19, 3.0)]);
    let definition = project_fixed_fillet(&scope, &[group], &[], &[identity])
        .expect("fixed Fillet from sole compact identity group");
    let cadmpeg_ir::features::FeatureDefinition::Fillet { groups } = definition else {
        panic!("expected Fillet");
    };
    assert!(matches!(
        groups.as_slice(),
        [cadmpeg_ir::features::FilletGroup {
            edges: cadmpeg_ir::features::EdgeSelection::Historical { edges, .. },
            radius: cadmpeg_ir::features::RadiusSpec::Constant {
                radius: cadmpeg_ir::features::Length(3.0)
            },
            tangency_weight: Some(1.0),
        }] if edges.len() == 2
    ));
}

#[test]
fn full_layout_identity_does_not_assign_the_fixed_fillet_edge_role() {
    let scope = fixed_scope();
    let group = group(2, 10);
    let mut identity = identity(10, &[(17, 3.0), (19, 3.0)]);
    identity.compact_layout = false;

    assert!(project_fixed_fillet(&scope, &[group], &[], &[identity]).is_none());
}

#[test]
fn only_edge_treatments_use_single_member_transition_chains() {
    let group = group(2, 10);
    let mut generic_identity = identity(10, &[(17, 3.0), (19, 3.0)]);
    generic_identity.compact_layout = false;
    generic_identity.treatment_radius_candidates.clear();
    let generic_feature_id =
        cadmpeg_ir::features::FeatureId("f3d:model:feature#ruled-surface".into());

    assert!(matches!(
        resolved_edge_group(
            &group,
            std::slice::from_ref(&group),
            &[],
            &[generic_identity],
            Some(7),
            &generic_feature_id,
        ),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
    let treatment_feature_id = cadmpeg_ir::features::FeatureId("f3d:model:feature#fillet".into());
    let mut identity = identity(10, &[(17, 3.0), (19, 3.0)]);
    identity.compact_layout = false;
    identity.treatment_radius_candidates.clear();
    assert!(matches!(
        resolved_edge_treatment_group(
            &group,
            std::slice::from_ref(&group),
            &[],
            &[identity],
            Some(7),
            &treatment_feature_id,
            None,
        ),
        cadmpeg_ir::features::EdgeSelection::Historical { edges, .. }
            if edges == [
                cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#6:fillet:7:17").expect("identity grammar"),
                cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#6:fillet:7:19").expect("identity grammar"),
            ]
    ));
}

#[test]
fn multiple_full_layout_members_do_not_use_the_operation_transition_chain() {
    let mut selection_group = group(2, 10);
    selection_group.members = vec![10, 11];
    selection_group.member_offsets = vec![0, 0];
    let mut first = identity(10, &[(17, 0.0), (18, 0.0), (19, 0.0)]);
    first.compact_layout = false;
    let mut second = identity(11, &[(17, 0.0), (18, 0.0), (19, 0.0)]);
    second.compact_layout = false;
    second.group_member_ordinal = 1;
    let feature_id = cadmpeg_ir::features::FeatureId("f3d:model:feature#fillet".into());

    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &[],
            &[first, second],
            Some(7),
            &feature_id,
            None,
        ),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
}

fn recipe_edge_operand(
    record_index: u32,
    changed_boundary_edge_slots: &[i64],
    deleted_boundary_edge_slots: &[i64],
) -> DesignEdgeOperand {
    serde_json::from_value(serde_json::json!({
        "id": format!("f3d:test:edge-operand#{record_index}"),
        "scope_record_index": 1,
        "scope_reference_ordinal": 0,
        "record_index": record_index,
        "byte_offset": 0,
        "class_tag": "297",
        "paired_byte_offset": 0,
        "paired_class_tag": "259",
        "recipe_record_index": record_index + 3,
        "recipe_record_byte_offset": 0,
        "recipe_id": "f3d:test:recipe",
        "recipe_prefix_offset": 0,
        "recipe_prefix_bytes": "",
        "recipe_references": [],
        "recipe_program_offset": 0,
        "recipe_program": [],
        "changed_boundary_edge_slots": changed_boundary_edge_slots,
        "deleted_boundary_edge_slots": deleted_boundary_edge_slots,
        "next_record_index": record_index + 4,
        "next_byte_offset": 0,
    }))
    .expect("edge recipe operand")
}

#[test]
fn unresolved_standard_recipe_is_not_replaced_by_identity_or_transition_context() {
    let selection_group = group(2, 10);
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.recipe_state_id = Some(7);
    operand.recipe_structure = Some(crate::records::DesignEdgeRecipeStructure {
        root: 1,
        sides: vec![crate::records::DesignTopologyRecipeSide {
            field_count: std::num::NonZeroU32::new(1).expect("one recipe field"),
            header_value: 1,
            scalars: Vec::new(),
            payload_prefix: Vec::new(),
            payload_entry_count: 0,
            entries: Vec::new(),
        }],
    });
    let mut persistent_identity = identity(10, &[]);
    persistent_identity.resolved_edge_slot = Some(19);
    let feature_id = cadmpeg_ir::features::FeatureId("f3d:model:feature#fillet".into());

    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            std::slice::from_ref(&operand),
            std::slice::from_ref(&persistent_identity),
            Some(7),
            &feature_id,
            None,
        ),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));

    operand.changed_boundary_edge_slots = vec![17];
    operand.deleted_boundary_edge_slots = vec![17];
    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &[operand],
            &[],
            Some(7),
            &feature_id,
            None,
        ),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
}

#[test]
fn unstructured_recipe_is_not_replaced_by_identity_or_transition_context() {
    let selection_group = group(2, 10);
    let mut operand = recipe_edge_operand(10, &[17], &[17]);
    operand.recipe_program = vec![1];
    operand.recipe_state_id = Some(7);
    let mut persistent_identity = identity(10, &[(17, 0.0)]);
    persistent_identity.resolved_edge_slot = Some(17);
    let feature_id = cadmpeg_ir::features::FeatureId("f3d:model:feature#fillet".into());

    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            std::slice::from_ref(&operand),
            std::slice::from_ref(&persistent_identity),
            Some(7),
            &feature_id,
            None,
        ),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
}

#[test]
fn treatment_corner_context_admits_only_edge_endpoints_and_collapses_recipe_repeats() {
    use crate::history_records::{
        AsmDeltaState, AsmHistoricalEdge, AsmHistoricalTopology, AsmHistory,
    };
    use crate::records::{DesignEdgeTreatmentVertexOperand, DesignVertexRecipe};

    let mut selection_group = group(2, 10);
    selection_group.members = vec![10, 11, 12, 13];
    selection_group.member_offsets = vec![0; 4];
    let mut first_edge = recipe_edge_operand(11, &[], &[]);
    first_edge.recipe_state_id = Some(7);
    first_edge.resolved_edge_slot = Some(17);
    let mut repeated_edge = recipe_edge_operand(13, &[], &[]);
    repeated_edge.recipe_state_id = Some(7);
    repeated_edge.resolved_edge_slot = Some(17);
    let corner = |record_index, group_member_ordinal, vertex| DesignEdgeTreatmentVertexOperand {
        id: format!("f3d:test:edge-treatment-vertex-operand#{record_index}"),
        scope_record_index: 1,
        scope_reference_ordinal: group_member_ordinal,
        group_record_index: 2,
        group_member_ordinal,
        recipe: DesignVertexRecipe {
            record_index,
            byte_offset: u64::from(record_index),
            class_tag: "306".into(),
            paired_byte_offset: 1,
            paired_class_tag: "261".into(),
            recipe_record_index: record_index + 3,
            recipe_record_byte_offset: 2,
            recipe_id: format!("f3d:test:construction-recipe#{record_index}"),
            recipe_prefix_offset: 3,
            recipe_prefix_bytes: Vec::new(),
            recipe_references: Vec::new(),
            recipe_program_offset: 4,
            recipe_program: vec![0],
            recipe_state_id: Some(7),
            resolved_vertex_slot: Some(vertex),
            next_record_index: record_index + 5,
            next_byte_offset: 5,
        },
    };
    let state = AsmDeltaState {
        id: "f3d:test:state#7".into(),
        parent: "f3d:test:history".into(),
        byte_offset: 0,
        state_id: 7,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: None,
        node_index: 7,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: Vec::new(),
        record_table_complete: true,
        topology: Some(AsmHistoricalTopology {
            edges: vec![17],
            vertices: vec![3, 4, 5],
            edge_vertices: vec![AsmHistoricalEdge {
                edge: 17,
                start_vertex: 3,
                end_vertex: 4,
            }],
            ..AsmHistoricalTopology::default()
        }),
        transition: None,
    };
    let history = AsmHistory {
        id: "f3d:test:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        projection_finalized: false,
        states: vec![state],
    };
    let feature_id = cadmpeg_ir::features::FeatureId("f3d:model:feature#fillet".into());
    let corners = [corner(10, 0, 3), corner(12, 2, 4)];
    let edges = [first_edge.clone(), repeated_edge.clone()];

    assert!(matches!(
        resolved_edge_treatment_group_with_corners(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &edges,
            &[],
            &corners,
            std::slice::from_ref(&history),
            Some(7),
            &feature_id,
            None,
        ),
        cadmpeg_ir::features::EdgeSelection::Historical { edges, .. }
            if edges.len() == 1 && edges[0].0.ends_with(":17")
    ));

    let invalid_corners = [corner(10, 0, 3), corner(12, 2, 5)];
    assert!(matches!(
        resolved_edge_treatment_group_with_corners(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &[first_edge, repeated_edge],
            &[],
            &invalid_corners,
            std::slice::from_ref(&history),
            Some(7),
            &feature_id,
            None,
        ),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
}

fn recipe_reference(candidate_edges: &[i64]) -> DesignRecipeReference {
    DesignRecipeReference {
        selector: 1,
        selector_offset: 0,
        token: "97".into(),
        token_offset: 0,
        design_reference: 1,
        design_reference_offset: 0,
        candidate_faces: Vec::new(),
        candidate_edges: candidate_edges
            .iter()
            .map(|edge| EdgeId::mint(format!("f3d:edge#{edge}")).expect("identity grammar"))
            .collect(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    }
}

#[test]
fn grouped_surface_patch_recipe_requires_agreeing_exact_references() {
    let mut first = recipe_edge_operand(10, &[], &[]);
    first.recipe_references = vec![
        recipe_reference(&[]),
        recipe_reference(&[17]),
        recipe_reference(&[17]),
    ];
    let mut second = recipe_edge_operand(11, &[], &[]);
    second.recipe_references = vec![recipe_reference(&[18]), recipe_reference(&[18])];

    assert_eq!(
        surface_patch_grouped_recipe_edges(&[&first, &second]),
        SurfacePatchRecipeEdges::Resolved(vec![
            EdgeId::mint("f3d:edge#17").expect("identity grammar"),
            EdgeId::mint("f3d:edge#18").expect("identity grammar")
        ])
    );
}

#[test]
fn grouped_surface_patch_recipe_rejects_ambiguous_or_repeated_edges() {
    let mut ambiguous = recipe_edge_operand(10, &[], &[]);
    ambiguous.recipe_references = vec![recipe_reference(&[17, 18])];
    let mut contradictory = recipe_edge_operand(11, &[], &[]);
    contradictory.recipe_references = vec![recipe_reference(&[18]), recipe_reference(&[19])];
    let absent = recipe_edge_operand(12, &[], &[]);
    let mut repeated = recipe_edge_operand(11, &[], &[]);
    repeated.recipe_references = vec![recipe_reference(&[17])];

    assert_eq!(
        surface_patch_grouped_recipe_edges(&[&ambiguous]),
        SurfacePatchRecipeEdges::Inconclusive
    );
    assert_eq!(
        surface_patch_grouped_recipe_edges(&[&contradictory]),
        SurfacePatchRecipeEdges::Inconclusive
    );
    assert_eq!(
        surface_patch_grouped_recipe_edges(&[&absent, &contradictory]),
        SurfacePatchRecipeEdges::Inconclusive
    );
    assert_eq!(
        surface_patch_grouped_recipe_edges(&[&repeated, &repeated]),
        SurfacePatchRecipeEdges::Inconclusive
    );
}

#[test]
fn grouped_surface_patch_recipe_projects_historical_edges() {
    let mut group = group(2, 10);
    group.members = vec![10, 11];
    group.member_offsets = vec![0, 0];
    let mut first = recipe_edge_operand(10, &[], &[]);
    first.recipe_references = vec![recipe_reference(&[17])];
    first.surface_patch_recipe_structure =
        Some(crate::records::DesignSurfacePatchRecipeStructure {
            root: 2,
            clauses: Vec::new(),
        });
    let mut second = recipe_edge_operand(11, &[], &[]);
    second.recipe_references = vec![recipe_reference(&[18])];
    let feature_id = cadmpeg_ir::features::FeatureId("f3d:model:feature#surface-patch".into());

    let selection = resolved_surface_patch_edge_group(
        &group,
        std::slice::from_ref(&group),
        &[first, second],
        &[],
        Some(7),
        &feature_id,
    );
    let prefix = crate::ids::history_input_prefix("surface-patch", 7);
    assert!(matches!(
        selection,
        cadmpeg_ir::features::EdgeSelection::Historical { edges, .. }
            if edges == [
                crate::ids::history_input_edge_id(&prefix, 17),
                crate::ids::history_input_edge_id(&prefix, 18),
            ]
    ));
}

#[test]
fn contradictory_surface_patch_references_suppress_generic_resolution() {
    let group = group(2, 10);
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.recipe_references = vec![recipe_reference(&[17]), recipe_reference(&[18])];
    operand.resolved_edge_slot = Some(17);
    operand.recipe_structure = Some(crate::records::DesignEdgeRecipeStructure {
        root: 1,
        sides: Vec::new(),
    });
    let feature_id = cadmpeg_ir::features::FeatureId("f3d:model:feature#surface-patch".into());

    assert!(matches!(
        resolved_surface_patch_edge_group(
            &group,
            std::slice::from_ref(&group),
            std::slice::from_ref(&operand),
            &[],
            Some(7),
            &feature_id,
        ),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
}

#[test]
fn unstructured_local_support_face_references_do_not_resolve_an_edge() {
    let context = |reference_ordinal, shared_edge_slots: &[i64]| {
        serde_json::from_value(serde_json::json!({
            "reference_ordinal": reference_ordinal,
            "result_faces": [],
            "result_shared_edge_slots": shared_edge_slots,
            "preceding_faces": [],
            "shared_edge_slots": shared_edge_slots,
            "changed_shared_edge_slots": []
        }))
        .expect("edge recipe reference context")
    };
    let mut operand = recipe_edge_operand(10, &[63, 106, 109, 164], &[]);
    operand.recipe_program = vec![1];
    operand.resolved_edge_slot = Some(180);
    operand.local_topology_references = Some(
        [2, 1, 3, 1]
            .into_iter()
            .map(|ordinal| std::num::NonZeroU32::new(ordinal).expect("nonzero ordinal"))
            .collect(),
    );
    operand.recipe_reference_contexts = vec![
        context(0, &[]),
        context(1, &[164, 180, 183, 210]),
        context(2, &[106, 139, 180, 195]),
        context(3, &[109, 142, 183, 197]),
    ];
    operand.preceding_boundary_edge_slots =
        vec![63, 106, 109, 139, 142, 164, 168, 180, 183, 195, 197, 210];

    assert_eq!(resolved_edge_operand(&operand), None);
}

#[test]
fn primary_terminal_support_references_resolve_one_shared_edge() {
    let side = |header_value, scalars: [i32; 2]| {
        serde_json::from_value(serde_json::json!({
            "field_count": 3,
            "header_value": header_value,
            "scalars": scalars,
            "payload_prefix": [0],
            "payload_entry_count": 0,
            "entries": []
        }))
        .expect("terminal edge recipe side")
    };
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.recipe_structure = Some(crate::records::DesignEdgeRecipeStructure {
        root: 2,
        sides: vec![side(0, [2, 1]), side(0, [1, 3])],
    });
    operand.terminal_reference_edge_slots = vec![
        vec![25, 38, 39, 62],
        vec![25, 35, 55, 69],
        vec![35, 39, 57, 83],
    ];
    operand.terminal_boundary_edge_slots = vec![25, 35, 38, 39, 55, 57, 62, 69, 83];

    assert_eq!(resolved_edge_operand(&operand), Some(25));

    operand.terminal_reference_edge_slots[0].push(69);
    assert_eq!(resolved_edge_operand(&operand), None);

    operand.terminal_reference_edge_slots[0].pop();
    operand.recipe_structure.as_mut().unwrap().sides[0] = side(0, [2, 0]);
    assert_eq!(resolved_edge_operand(&operand), None);

    operand.recipe_structure.as_mut().unwrap().sides[0] = side(0, [2, 1]);
    operand.recipe_reference_contexts = vec![serde_json::from_value(serde_json::json!({
        "reference_ordinal": 0,
        "result_faces": [],
        "result_shared_edge_slots": [],
        "preceding_faces": [],
        "shared_edge_slots": [],
        "changed_shared_edge_slots": []
    }))
    .expect("historical edge recipe reference context")];
    assert_eq!(resolved_edge_operand(&operand), None);
}

#[test]
fn edge_flange_uses_one_updated_edge_without_recipe_context() {
    let group = group(2, 10);
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.preceding_boundary_edge_slots = vec![17, 18, 19];
    operand.changed_boundary_edge_slots = vec![17, 18];
    operand.updated_boundary_edge_slots = vec![17];
    operand.result_boundary_edge_slots = vec![17, 20];
    let feature_id = cadmpeg_ir::features::FeatureId("f3d:model:feature#edge-flange".into());

    let selection = resolved_edge_flange_group(
        &group,
        std::slice::from_ref(&group),
        &[operand],
        &[],
        Some(7),
        &feature_id,
    );
    assert!(matches!(
        selection,
        cadmpeg_ir::features::EdgeSelection::Historical { edges, .. }
            if edges == [cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#11:edge-flange:7:17").expect("identity grammar")]
    ));
}

#[test]
fn edge_flange_does_not_choose_an_ambiguous_updated_boundary() {
    let group = group(2, 10);
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.preceding_boundary_edge_slots = vec![17, 18, 19];
    operand.changed_boundary_edge_slots = vec![17, 18];
    operand.updated_boundary_edge_slots = vec![17, 18];
    operand.result_boundary_edge_slots = vec![17, 18, 20];
    let feature_id = cadmpeg_ir::features::FeatureId("f3d:model:feature#edge-flange".into());

    assert!(matches!(
        resolved_edge_flange_group(
            &group,
            std::slice::from_ref(&group),
            &[operand],
            &[],
            Some(7),
            &feature_id,
        ),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
}

#[test]
fn edge_treatment_chain_requires_complete_recipe_boundary_coverage() {
    let first = recipe_edge_operand(10, &[17], &[17]);
    let second = recipe_edge_operand(11, &[], &[]);
    assert!(!transition_chain_is_supported_by_recipe(
        &[17, 18],
        2,
        [&first, &second],
    ));

    let context = |changed_reference_edge_slots| {
        serde_json::from_value(serde_json::json!({
            "reference_ordinal": 0,
            "result_faces": [],
            "result_shared_edge_slots": [],
            "preceding_faces": [],
            "shared_edge_slots": [],
            "changed_shared_edge_slots": [],
            "changed_reference_edge_slots": changed_reference_edge_slots,
        }))
        .expect("historical edge recipe reference context")
    };
    let mut first = recipe_edge_operand(10, &[], &[17]);
    first.local_topology_references = Some(vec![
        std::num::NonZeroU32::new(1).expect("nonzero reference ordinal")
    ]);
    first.recipe_reference_contexts = vec![context(vec![18])];
    let mut second = recipe_edge_operand(11, &[], &[]);
    second.local_topology_references = Some(vec![
        std::num::NonZeroU32::new(1).expect("nonzero reference ordinal")
    ]);
    second.recipe_reference_contexts = vec![context(vec![17, 18])];
    assert!(transition_chain_is_supported_by_recipe(
        &[17, 18],
        2,
        [&first, &second],
    ));

    let first = recipe_edge_operand(10, &[17, 18], &[17]);
    assert!(transition_chain_is_supported_by_recipe(
        &[17, 18],
        2,
        [&first, &second],
    ));
}

#[test]
fn compact_identity_group_uses_selected_recipe_context_boundaries() {
    let mut selection_group = group(2, 10);
    selection_group.members = vec![10, 11];
    selection_group.member_offsets = vec![0, 0];
    let first_identity = identity(10, &[(17, 0.0), (18, 0.0)]);
    let mut second_identity = identity(11, &[(17, 0.0), (18, 0.0)]);
    second_identity.group_member_ordinal = 1;
    let context = |changed_reference_edge_slots| {
        serde_json::from_value(serde_json::json!({
            "reference_ordinal": 0,
            "result_faces": [],
            "result_shared_edge_slots": [],
            "preceding_faces": [],
            "shared_edge_slots": [],
            "changed_shared_edge_slots": [],
            "changed_reference_edge_slots": changed_reference_edge_slots,
        }))
        .expect("historical edge recipe reference context")
    };
    let mut first = recipe_edge_operand(10, &[], &[17]);
    first.local_topology_references = Some(vec![
        std::num::NonZeroU32::new(1).expect("nonzero reference ordinal")
    ]);
    first.recipe_reference_contexts = vec![context(vec![18])];
    let mut second = recipe_edge_operand(11, &[], &[]);
    second.local_topology_references = Some(vec![
        std::num::NonZeroU32::new(1).expect("nonzero reference ordinal")
    ]);
    second.recipe_reference_contexts = vec![context(vec![17, 18])];
    let feature_id = cadmpeg_ir::features::FeatureId("f3d:model:feature#chamfer".into());

    let selection = resolved_edge_treatment_group(
        &selection_group,
        std::slice::from_ref(&selection_group),
        &[first, second],
        &[first_identity, second_identity],
        Some(7),
        &feature_id,
        None,
    );
    assert!(matches!(
        selection,
        cadmpeg_ir::features::EdgeSelection::Historical { edges, .. }
            if edges == [
                cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#7:chamfer:7:17").expect("identity grammar"),
                cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#7:chamfer:7:18").expect("identity grammar"),
            ]
    ));
}

#[test]
fn lost_references_preserve_a_complete_compact_transition_chain() {
    let mut selection_group = group(2, 10);
    selection_group.members = vec![10, 11];
    selection_group.member_offsets = vec![0, 0];
    let first_identity = identity(10, &[(17, 0.0), (18, 0.0)]);
    let mut second_identity = identity(11, &[(17, 0.0), (18, 0.0)]);
    second_identity.group_member_ordinal = 1;
    let recipe_operands = [
        recipe_edge_operand(10, &[19], &[19]),
        recipe_edge_operand(11, &[19], &[19]),
    ];
    let feature_id = cadmpeg_ir::features::FeatureId("f3d:model:feature#chamfer".into());

    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &recipe_operands,
            &[first_identity.clone(), second_identity.clone()],
            Some(7),
            &feature_id,
            None,
        ),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));

    selection_group.lost_edge_references = vec!["f3d:test:lost#0".into()];
    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &recipe_operands,
            &[first_identity.clone(), second_identity.clone()],
            Some(7),
            &feature_id,
            None,
        ),
        cadmpeg_ir::features::EdgeSelection::Unresolved
    ));

    selection_group
        .lost_edge_references
        .push("f3d:test:lost#1".into());
    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &recipe_operands,
            &[first_identity, second_identity],
            Some(7),
            &feature_id,
            None,
        ),
        cadmpeg_ir::features::EdgeSelection::Historical { edges, .. }
            if edges == [
                cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#7:chamfer:7:17").expect("identity grammar"),
                cadmpeg_ir::ids::HistoricalEdgeId::mint("f3d:history-input:edge#7:chamfer:7:18").expect("identity grammar"),
            ]
    ));
}

#[test]
fn compact_identity_group_does_not_displace_a_possible_support_group() {
    let scope = fixed_scope();
    let first = group(2, 10);
    let second = group(3, 11);
    let first_identity = identity(10, &[(17, 3.0)]);
    let mut second_identity = identity(11, &[(19, 3.0)]);
    second_identity.group_record_index = 3;
    assert!(project_fixed_fillet(
        &scope,
        &[first, second],
        &[],
        &[first_identity, second_identity],
    )
    .is_none());
}

#[test]
fn compact_edge_treatment_group_selects_exact_deleted_edge_cardinality() {
    let mut selection_group = group(2, 10);
    selection_group.members = vec![10, 11];
    selection_group.member_offsets = vec![0, 0];
    let first = identity(10, &[(17, 5.0), (19, 5.0)]);
    let mut second = identity(11, &[(17, 5.0), (19, 5.0)]);
    second.group_member_ordinal = 1;
    let feature_id = cadmpeg_ir::features::FeatureId("feature".into());

    assert!(matches!(
        resolved_edge_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &[],
            &[first.clone(), second.clone()],
            Some(7),
            &feature_id,
        ),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &[],
            &[first.clone(), second.clone()],
            Some(7),
            &feature_id,
            Some(3.0),
        ),
        cadmpeg_ir::features::EdgeSelection::Historical { edges, .. }
            if edges.len() == 2
    ));

    let first = identity(10, &[(17, 5.0), (18, 5.0), (19, 5.0)]);
    let mut second = identity(11, &[(17, 5.0), (18, 5.0), (19, 5.0)]);
    second.group_member_ordinal = 1;
    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &[],
            &[first, second],
            Some(7),
            &feature_id,
            Some(3.0),
        ),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));

    let recipe_operands = [
        recipe_edge_operand(10, &[17, 18, 19], &[17]),
        recipe_edge_operand(11, &[17, 18, 19], &[18]),
    ];
    let mut recipe_second_identity = identity(11, &[(17, 5.0), (18, 5.0), (19, 5.0)]);
    recipe_second_identity.group_member_ordinal = 1;
    let recipe_identities = [
        identity(10, &[(17, 5.0), (18, 5.0), (19, 5.0)]),
        recipe_second_identity,
    ];
    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &recipe_operands,
            &recipe_identities,
            Some(7),
            &feature_id,
            None,
        ),
        cadmpeg_ir::features::EdgeSelection::Historical { edges, .. }
            if edges.len() == 3
    ));

    let incomplete_recipe_operands = [
        recipe_edge_operand(10, &[17, 18], &[17]),
        recipe_edge_operand(11, &[17, 18], &[18]),
    ];
    assert!(matches!(
        resolved_edge_treatment_group(
            &selection_group,
            std::slice::from_ref(&selection_group),
            &incomplete_recipe_operands,
            &recipe_identities,
            Some(7),
            &feature_id,
            None,
        ),
        cadmpeg_ir::features::EdgeSelection::Native(_)
    ));
}

#[test]
fn hem_transition_edge_is_the_unique_non_support_boundary() {
    let support = [[167], [106], [110]];
    assert_eq!(
        unique_hem_transition_edge_candidate(
            &[63, 106, 110, 167],
            [
                &[][..],
                support[0].as_slice(),
                support[1].as_slice(),
                support[2].as_slice()
            ]
        ),
        Some(63)
    );
    assert_eq!(
        unique_hem_transition_edge_candidate(
            &[63, 106, 110, 167],
            [
                &[][..],
                &[63][..],
                support[1].as_slice(),
                support[0].as_slice()
            ]
        ),
        Some(110)
    );
    assert_eq!(
        unique_hem_transition_edge_candidate(
            &[63, 106, 110, 167],
            [
                support[0].as_slice(),
                support[1].as_slice(),
                support[2].as_slice()
            ]
        ),
        None
    );
    assert_eq!(
        unique_hem_transition_edge_candidate(
            &[63, 106, 110, 167],
            [&[][..], &[106, 111][..], support[1].as_slice()]
        ),
        None
    );
}

#[test]
fn partial_historical_edge_selection_retains_proofs_and_unresolved_operands() {
    use cadmpeg_ir::features::EdgeSelection;
    use cadmpeg_ir::ids::FeatureInputTopologyId;

    let state =
        FeatureInputTopologyId::mint("f3d:history-input:state#feature").expect("identity grammar");
    let selection = partial_historical_edge_selection(
        [
            ("operand-a", Some(17)),
            ("operand-b", None),
            ("operand-c", Some(17)),
        ],
        41,
        "feature",
        state.clone(),
        "group",
    )
    .expect("mixed proof state");
    assert_eq!(
        selection,
        EdgeSelection::HistoricalPartial {
            state,
            edges: vec![cadmpeg_ir::ids::HistoricalEdgeId::mint(
                "f3d:history-input:edge#7:feature:41:17"
            )
            .expect("identity grammar")],
            unresolved: vec!["operand-b".into()],
            native: "group".into(),
        }
    );
    assert!(partial_historical_edge_selection(
        [("operand-a", Some(17)), ("operand-b", Some(18))],
        41,
        "feature",
        FeatureInputTopologyId::mint("state").expect("identity grammar"),
        "group",
    )
    .is_none());
    assert_eq!(
        partial_historical_edge_selection(
            [("operand-a", None), ("operand-b", None)],
            41,
            "feature",
            FeatureInputTopologyId::mint("state").expect("identity grammar"),
            "group",
        ),
        None
    );
}

#[test]
fn edge_recipe_candidate_intersection_must_be_uniquely_corroborated() {
    use crate::records::{
        DesignEdgeRecipeSelectorContext, DesignTopologyIncidentSide, DesignTopologyRecipeEntry,
        DesignTopologyRecipeTriplet,
    };

    let selector = |selector, edges: &[i64]| DesignEdgeRecipeSelectorContext {
        selector,
        clause_entries: vec![None, None],
        clause_triplet_edge_slots: vec![None, None],
        incidence_matching_edge_slots: edges.to_vec(),
        unique_incidence_edge_slot: (edges.len() == 1).then(|| edges[0]),
        boundary_count_matching_edge_slots: Vec::new(),
    };
    let selector_with_counts = |ordinal: i32, incidence: &[i64], counts: &[i64]| {
        let mut context = selector(ordinal, incidence);
        context.boundary_count_matching_edge_slots = counts.to_vec();
        context
    };
    assert_eq!(
        resolved_edge_candidate_intersection(
            &[selector(0, &[17, 18]), selector(1, &[17, 19])],
            [&[17, 20][..], &[15, 17][..]],
        ),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(
            &[selector(0, &[17, 18]), selector(1, &[17, 18])],
            [&[17, 18][..]],
        ),
        None
    );
    assert_eq!(
        resolved_edge_candidate_intersection(
            &[selector(0, &[17]), selector(1, &[18])],
            [&[17, 18][..]],
        ),
        None
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[selector(0, &[17]), selector(1, &[])], [&[17][..]],),
        None
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[selector(0, &[17])], [&[][..]]),
        None
    );
    assert_eq!(resolved_edge_candidate_intersection(&[], [&[17][..]]), None);
    assert_eq!(
        resolved_edge_candidate_intersection(
            &[
                selector_with_counts(0, &[17, 18], &[17, 19]),
                selector_with_counts(1, &[17, 20], &[17, 21]),
            ],
            std::iter::empty::<&[i64]>(),
        ),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(
            &[
                selector_with_counts(0, &[17, 18], &[17, 18]),
                selector_with_counts(1, &[17, 18], &[17, 18]),
            ],
            std::iter::empty::<&[i64]>(),
        ),
        None
    );
    assert_eq!(
        resolved_edge_candidate_intersection(
            &[
                selector_with_counts(0, &[17], &[18]),
                selector_with_counts(1, &[17], &[18]),
            ],
            std::iter::empty::<&[i64]>(),
        ),
        None
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[], [&[17, 18][..], &[17, 19][..]]),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[], [&[][..], &[17, 18][..], &[][..], &[17, 19][..]],),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[], [&[17, 18][..], &[17, 18][..]]),
        None
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[selector(0, &[18])], [&[17, 18][..], &[17, 19][..]],),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(
            &[
                selector_with_counts(0, &[], &[17, 18]),
                selector_with_counts(1, &[], &[17, 19]),
            ],
            [&[17, 20][..]],
        ),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(
            &[selector_with_counts(0, &[17], &[18])],
            [&[17, 18][..]],
        ),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::edge_assignment_candidates(
            &[selector_with_counts(0, &[], &[17, 18])],
            [&[17][..]],
        ),
        Some(vec![17])
    );
    assert_eq!(
        crate::design::edge_resolve::edge_assignment_candidates(
            &[selector_with_counts(0, &[18], &[17, 18])],
            [&[17, 18][..]],
        ),
        Some(vec![18])
    );
    assert_eq!(
        crate::design::edge_resolve::edge_assignment_candidates(
            &[selector_with_counts(0, &[18], &[17, 18])],
            [&[17][..]],
        ),
        None
    );
    let assignment_candidates = [
        crate::design::edge_resolve::edge_assignment_candidates(
            &[selector_with_counts(0, &[], &[17, 18])],
            [&[17, 18][..]],
        )
        .unwrap(),
        crate::design::edge_resolve::edge_assignment_candidates(
            &[selector_with_counts(0, &[18], &[17, 18])],
            [&[17, 18][..]],
        )
        .unwrap(),
    ];
    assert_eq!(
        crate::design::edge_resolve::unique_bipartite_assignment(&assignment_candidates),
        Some(vec![17, 18])
    );
    let triplet = DesignTopologyRecipeTriplet {
        outer: std::num::NonZeroU32::new(3).unwrap(),
        middle: 2,
        vertex_ordinal: 2,
        incident_edge_ordinal: Some(1),
        incident_side: Some(DesignTopologyIncidentSide::Preceding),
    };
    let mut common = selector(0, &[]);
    common.clause_entries[0] = Some(DesignTopologyRecipeEntry {
        selector: 0,
        boundary_edge_count: std::num::NonZeroU32::new(4).unwrap(),
        topology_triplets: [triplet.clone(), triplet.clone()],
        common_incident_edge_ordinal: Some(1),
    });
    common.clause_triplet_edge_slots[0] = Some([vec![17, 18], vec![17]]);
    assert_eq!(
        resolved_edge_candidate_intersection(&[common.clone()], [&[17, 18][..]]),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[common], [&[][..]]),
        Some(17)
    );
    let mut common = selector(0, &[]);
    common.clause_entries[0] = Some(DesignTopologyRecipeEntry {
        selector: 0,
        boundary_edge_count: std::num::NonZeroU32::new(4).unwrap(),
        topology_triplets: [triplet.clone(), triplet],
        common_incident_edge_ordinal: Some(1),
    });
    common.clause_triplet_edge_slots[0] = Some([vec![17, 18, 19], vec![17, 18]]);
    assert_eq!(
        resolved_edge_candidate_intersection(&[common.clone()], [&[17][..]]),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[common], [&[19][..]]),
        None
    );
    let mut cross_clause = selector(0, &[]);
    cross_clause.clause_triplet_edge_slots =
        vec![Some([vec![18], vec![17, 19]]), Some([vec![20], vec![17]])];
    assert_eq!(
        resolved_edge_candidate_intersection(&[cross_clause.clone()], std::iter::empty::<&[i64]>(),),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[cross_clause.clone()], [&[17, 21][..]],),
        Some(17)
    );
    assert_eq!(
        resolved_edge_candidate_intersection(&[cross_clause.clone()], [&[18][..]]),
        None
    );
    cross_clause.clause_triplet_edge_slots =
        vec![Some([vec![18], vec![17]]), Some([vec![18], vec![17]])];
    assert_eq!(
        resolved_edge_candidate_intersection(&[cross_clause], std::iter::empty::<&[i64]>(),),
        None
    );
}

#[test]
fn edge_group_cardinality_resolves_one_common_deleted_candidate_set() {
    let selector = |candidates: &[i64]| crate::records::DesignEdgeRecipeSelectorContext {
        selector: 0,
        clause_entries: vec![None, None],
        clause_triplet_edge_slots: vec![None, None],
        incidence_matching_edge_slots: Vec::new(),
        unique_incidence_edge_slot: None,
        boundary_count_matching_edge_slots: candidates.to_vec(),
    };
    let first = [selector(&[19, 17, 18])];
    let context = [selector(&[])];
    let last = [selector(&[18, 19, 17])];
    assert_eq!(
        crate::design::edge_resolve::changed_boundary_count_edge_group_candidates([
            first.as_slice(),
            context.as_slice(),
            last.as_slice(),
        ]),
        Some(vec![17, 18, 19])
    );
    assert_eq!(
        crate::design::edge_resolve::changed_boundary_count_edge_group_candidates([
            first.as_slice(),
            last.as_slice(),
        ]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::changed_boundary_count_edge_group_candidates([
            first.as_slice(),
            context.as_slice(),
            &[],
        ]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::common_deleted_edge_group_candidates([
            (true, &[19, 17, 18, 17][..]),
            (true, &[18, 19, 17][..]),
            (true, &[17, 18, 19][..]),
        ],),
        Some(vec![17, 18, 19])
    );
    assert_eq!(
        crate::design::edge_resolve::common_deleted_edge_group_candidates([
            (true, &[17, 18, 19][..]),
            (true, &[17, 18][..]),
            (true, &[17, 18, 19][..]),
        ],),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::common_deleted_edge_group_candidates([
            (true, &[17, 18, 19][..]),
            (true, &[17, 18, 19][..]),
        ]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::common_deleted_edge_group_candidates([
            (true, &[17, 18][..]),
            (false, &[][..]),
            (true, &[18, 17][..]),
        ]),
        Some(vec![17, 18])
    );
    assert_eq!(
        crate::design::edge_resolve::common_deleted_edge_group_candidates(std::iter::empty::<(
            bool,
            &[i64]
        )>()),
        None
    );
    let deleted = vec![17, 18, 19, 20];
    let groups = vec![
        vec![
            (10, Some(17), deleted.clone()),
            (11, Some(19), deleted.clone()),
        ],
        vec![(12, None, deleted.clone()), (13, None, deleted.clone())],
    ];
    assert_eq!(
        crate::design::edge_resolve::partition_unique_incomplete_edge_group(1, &groups),
        Some(vec![18, 20])
    );
    assert_eq!(
        crate::design::edge_resolve::partition_unique_incomplete_edge_group(0, &groups),
        None
    );
    let mut two_incomplete = groups.clone();
    two_incomplete[0][0].1 = None;
    assert_eq!(
        crate::design::edge_resolve::partition_unique_incomplete_edge_group(1, &two_incomplete),
        None
    );
    let mut duplicate_identity = groups;
    duplicate_identity[1][0].0 = 11;
    assert_eq!(
        crate::design::edge_resolve::partition_unique_incomplete_edge_group(1, &duplicate_identity),
        None
    );
}

#[test]
fn deleted_boundary_group_requires_complete_contextual_group_cardinality() {
    let context = |changed_reference_edge_slots: &[i64]| {
        serde_json::from_value(serde_json::json!({
            "reference_ordinal": 0,
            "result_faces": [],
            "result_shared_edge_slots": [],
            "preceding_faces": [],
            "shared_edge_slots": [],
            "changed_shared_edge_slots": [],
            "changed_reference_edge_slots": changed_reference_edge_slots,
        }))
        .expect("edge recipe reference context")
    };
    let operand = |record_index: u32, deleted: &[i64]| {
        let mut operand = recipe_edge_operand(record_index, deleted, deleted);
        operand.preceding_boundary_edge_slots = deleted.to_vec();
        operand.recipe_structure = Some(crate::records::DesignEdgeRecipeStructure {
            root: 2,
            sides: Vec::new(),
        });
        operand.recipe_references = vec![recipe_reference(&[])];
        operand.recipe_reference_contexts = vec![context(deleted)];
        operand
    };

    let first = operand(10, &[17, 18]);
    let second = operand(11, &[17, 18]);
    let third = operand(12, &[19, 20]);
    let fourth = operand(13, &[19, 20]);
    assert_eq!(
        deleted_boundary_edge_group_candidates(&[&first, &second, &third, &fourth]),
        Some(vec![17, 18, 19, 20])
    );

    let too_many_edges = operand(11, &[17, 18, 19]);
    assert_eq!(
        deleted_boundary_edge_group_candidates(&[&first, &too_many_edges]),
        None
    );

    let mut unreferenced = operand(12, &[19, 20]);
    unreferenced.recipe_reference_contexts = vec![context(&[21])];
    assert_eq!(
        deleted_boundary_edge_group_candidates(&[&first, &second, &unreferenced, &fourth]),
        None
    );
}

#[test]
fn contextual_deleted_group_assigns_a_consolidated_legacy_member() {
    let context = |changed_reference_edge_slots: &[i64]| {
        serde_json::from_value(serde_json::json!({
            "reference_ordinal": 0,
            "result_faces": [],
            "result_shared_edge_slots": [],
            "preceding_faces": [],
            "shared_edge_slots": [],
            "changed_shared_edge_slots": [],
            "changed_reference_edge_slots": changed_reference_edge_slots,
        }))
        .expect("edge recipe reference context")
    };
    let structure = || crate::records::DesignEdgeRecipeStructure {
        root: 2,
        sides: Vec::new(),
    };

    let mut consolidated = recipe_edge_operand(10, &[], &[]);
    consolidated.preceding_boundary_edge_slots = vec![5630, 5675];
    consolidated.recipe_structure = Some(structure());
    consolidated.recipe_references = vec![recipe_reference(&[])];
    consolidated.recipe_reference_contexts = vec![context(&[5630])];

    let mut deleted = recipe_edge_operand(11, &[5630, 5675], &[5630, 5675]);
    deleted.preceding_boundary_edge_slots = vec![5630, 5675];
    deleted.recipe_structure = Some(structure());
    deleted.recipe_references = vec![recipe_reference(&[]), recipe_reference(&[])];
    deleted.recipe_reference_contexts = vec![context(&[5630]), context(&[5675])];

    assert_eq!(
        contextual_deleted_edge_group_candidates(&[&consolidated, &deleted]),
        Some(vec![5630, 5675])
    );

    deleted.recipe_reference_contexts[1] = context(&[5630]);
    assert_eq!(
        contextual_deleted_edge_group_candidates(&[&consolidated, &deleted]),
        None
    );
}

#[test]
fn result_boundary_reference_group_requires_one_persistent_contextual_edge() {
    let context = |changed_reference_edge_slots: &[i64]| {
        serde_json::from_value(serde_json::json!({
            "reference_ordinal": 0,
            "result_faces": [],
            "result_shared_edge_slots": [],
            "preceding_faces": [],
            "shared_edge_slots": [],
            "changed_shared_edge_slots": [],
            "changed_reference_edge_slots": changed_reference_edge_slots,
        }))
        .expect("edge recipe reference context")
    };
    let mut operand = recipe_edge_operand(10, &[], &[]);
    operand.recipe_structure = Some(crate::records::DesignEdgeRecipeStructure {
        root: 2,
        sides: (0..2)
            .map(|_| crate::records::DesignTopologyRecipeSide {
                field_count: std::num::NonZeroU32::new(3).expect("field count"),
                header_value: 2,
                scalars: vec![1, 0],
                payload_prefix: vec![0],
                payload_entry_count: 0,
                entries: Vec::new(),
            })
            .collect(),
    });
    operand.recipe_references = vec![
        recipe_reference(&[]),
        recipe_reference(&[]),
        recipe_reference(&[]),
    ];
    operand.recipe_reference_contexts = vec![context(&[19, 21]), context(&[19]), context(&[22])];
    operand.preceding_boundary_edge_slots = vec![17, 18];
    operand.result_boundary_edge_slots = vec![19, 20];
    assert_eq!(
        result_boundary_reference_edge_group_candidates(&[&operand]),
        Some(vec![19])
    );

    operand.recipe_reference_contexts[1] = context(&[20]);
    assert_eq!(
        result_boundary_reference_edge_group_candidates(&[&operand]),
        None
    );

    operand.recipe_reference_contexts[1] = context(&[19]);
    operand.preceding_boundary_edge_slots.push(19);
    assert_eq!(
        result_boundary_reference_edge_group_candidates(&[&operand]),
        None
    );
}

#[test]
fn edge_group_ignores_members_without_changed_edge_candidates() {
    assert_eq!(
        crate::design::edge_resolve::context_only_edge_group_candidates([
            (None, &[][..]),
            (Some(17), &[17, 18][..]),
            (Some(17), &[17][..]),
            (None, &[][..]),
        ]),
        Some(vec![17])
    );
    assert_eq!(
        crate::design::edge_resolve::context_only_edge_group_candidates([
            (Some(17), &[17][..]),
            (None, &[18][..]),
        ]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::context_only_edge_group_candidates([(None, &[][..])]),
        None
    );
}

#[test]
fn edge_group_resolves_only_one_perfect_candidate_assignment() {
    assert_eq!(
        crate::design::edge_resolve::edge_group_assignment_candidates(
            &[],
            [&[17, 18][..], &[18, 19][..], &[20][..]],
        ),
        Some(crate::design::edge_resolve::EdgeAssignmentCandidates::Edges(vec![18]))
    );
    assert_eq!(
        crate::design::edge_resolve::edge_group_assignment_candidates(&[], [&[][..], &[18][..]]),
        Some(crate::design::edge_resolve::EdgeAssignmentCandidates::Context)
    );
    assert_eq!(
        crate::design::edge_resolve::edge_group_assignment_candidates(&[], [&[17][..], &[18][..]]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::edge_group_assignment_candidates(&[], [&[17][..]]),
        Some(crate::design::edge_resolve::EdgeAssignmentCandidates::Context)
    );
    assert_eq!(
        crate::design::edge_resolve::unique_bipartite_assignment(&[
            vec![17, 18],
            vec![18, 19],
            vec![19],
        ]),
        Some(vec![17, 18, 19])
    );
    assert_eq!(
        crate::design::edge_resolve::unique_bipartite_assignment(&[vec![17, 18], vec![17, 18]]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::unique_bipartite_assignment(&[vec![17], vec![17]]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::unique_bipartite_assignment(&[vec![17], Vec::new()]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::unique_bipartite_assignment(&[]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::unique_edge_assignment_with_context(&[
            crate::design::edge_resolve::EdgeAssignmentCandidates::Edges(vec![17, 18]),
            crate::design::edge_resolve::EdgeAssignmentCandidates::Context,
            crate::design::edge_resolve::EdgeAssignmentCandidates::Edges(vec![18]),
        ]),
        Some(vec![17, 18])
    );
    assert_eq!(
        crate::design::edge_resolve::unique_edge_assignment_with_context(&[
            crate::design::edge_resolve::EdgeAssignmentCandidates::Context,
            crate::design::edge_resolve::EdgeAssignmentCandidates::Context,
        ]),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::unique_deleted_reference_assignment(
            &[vec![16, 17, 18], vec![19, 20, 21]],
            &[vec![17, 20, 22], vec![17, 20, 22]],
        ),
        Some(vec![17, 20])
    );
    assert_eq!(
        crate::design::edge_resolve::unique_deleted_reference_assignment(
            &[vec![16, 17, 18], vec![16, 17, 18]],
            &[vec![17, 18], vec![17, 18]],
        ),
        None
    );
    assert_eq!(
        crate::design::edge_resolve::unique_deleted_reference_assignment(&[vec![17]], &[vec![]],),
        None
    );
}

#[test]
fn sweep_recipe_edge_requires_incidence_and_two_reference_faces() {
    use crate::design::edge_resolve::unique_incidence_edge_shared_by_reference_faces;

    let selector = |edges| crate::records::DesignEdgeRecipeSelectorContext {
        selector: 0,
        clause_entries: Vec::new(),
        clause_triplet_edge_slots: Vec::new(),
        incidence_matching_edge_slots: edges,
        unique_incidence_edge_slot: None,
        boundary_count_matching_edge_slots: Vec::new(),
    };
    let selectors = [selector(vec![11, 12]), selector(vec![13])];
    assert_eq!(
        unique_incidence_edge_shared_by_reference_faces(
            &selectors,
            [&[10, 11][..], &[11, 12][..], &[13, 14][..]],
        ),
        Some(11)
    );
    assert_eq!(
        unique_incidence_edge_shared_by_reference_faces(&selectors, [&[11, 13][..], &[11, 13][..]],),
        None
    );
    assert_eq!(
        unique_incidence_edge_shared_by_reference_faces(&[selector(vec![11])], [&[10, 11][..]],),
        None
    );
    assert_eq!(
        unique_incidence_edge_shared_by_reference_faces(
            &[selector(vec![11])],
            [&[10, 11][..], &[10, 11][..]],
        ),
        None
    );
}
