// SPDX-License-Identifier: Apache-2.0

use crate::design::feature_project::{
    project_parameter_design_with_edge_identities, ProjectInputs,
};
use crate::records::feature::scope::DesignParameterScope;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::edge_identity::DesignEdgeOperand;
use cadmpeg_ir::features::{EdgeSelection, FeatureDefinition, FeatureOperation};

#[test]
fn edge_resolve_deleted_set_assumption_reaches_projection_losses() {
    let scope: DesignParameterScope = serde_json::from_value(serde_json::json!({
        "id": "f3d:test/Design/BulkStream.dat:scope", "byte_offset": 0, "class_tag": "300",
        "record_index": 1, "frame_length": 200, "kind": "Fillet", "kind_offset": 32,
        "feature_ordinal": 1, "feature_ordinal_offset": 128,
        "history_state_id": 8, "history_state_id_offset": 24,
        "previous_history_state_id": 7, "previous_history_state_id_offset": 158,
        "reference_count_offset": 9, "reference_members": [2], "reference_member_offsets": [14],
        "fixed_fillet_parameters": { "groups": [{
            "radii": [0.3], "radius_record_indexes": [5], "radius_offsets": [0],
            "intermediate_parameters": [], "intermediate_parameter_record_indexes": [],
            "intermediate_parameter_offsets": []
        }] },
        "paired_class_tag": "261", "paired_byte_offset": 200
    }))
    .unwrap();
    let group: DesignConstructionOperandGroup = serde_json::from_value(serde_json::json!({
        "id": "f3d:test/Design/BulkStream.dat:group-2", "scope_record_index": 1, "scope_reference_ordinal": 0,
        "record_index": 2, "byte_offset": 0, "class_tag": "300",
        "members": [10, 11], "member_offsets": [0, 11],
        "frame": { "layout": "counted", "member_count_offset": 0,
            "identity_record_index": 9, "identity_record_offset": 0,
            "opaque_index": 1, "opaque_index_offset": 18,
            "opaque_scalar": 1.0, "opaque_scalar_offset": 22, "variant": false },
        "role": 0x10_0000_0000u64, "role_offset": 0,
        "paired_class_tag": "258", "paired_byte_offset": 0
    }))
    .unwrap();
    let reference_contexts = [1, 2].map(|face| {
        serde_json::json!({
            "reference_ordinal": face - 1,
            "preceding_faces": [format!("f3d:brep:entity#{face}")],
            "result_faces": [], "result_shared_edge_slots": [],
            "shared_edge_slots": [], "changed_shared_edge_slots": [],
            "changed_reference_edge_slots": [10, 11]
        })
    });
    let recipe_sides = [0, 1].map(|_| {
        serde_json::json!({
            "field_count": 1, "header_value": 0, "scalars": [], "payload_prefix": [], "payload_entry_count": 0, "entries": []
        })
    });
    let operands: [DesignEdgeOperand; 2] = [10, 11].map(|index| {
        serde_json::from_value(serde_json::json!({
            "id": format!("f3d:test/Design/BulkStream.dat:edge-operand#{index}"),
            "scope_record_index": 1, "scope_reference_ordinal": 0,
            "record_index": index, "byte_offset": 0, "class_tag": "297",
            "paired_byte_offset": 16, "paired_class_tag": "259",
            "recipe_record_index": index + 3, "recipe_record_byte_offset": 32,
            "recipe_id": "f3d:test/Design/BulkStream.dat:recipe", "recipe_prefix_offset": 43, "recipe_prefix_bytes": "",
            "recipe_references": [], "recipe_program_offset": 0, "recipe_program": [],
            "recipe_structure": { "root": 2, "sides": recipe_sides },
            "recipe_reference_contexts": reference_contexts,
            "transition_deleted_edge_slots": [10, 11],
            "next_record_index": index + 4, "next_byte_offset": 160
        }))
        .unwrap()
    });
    for (resolved, expected_notes) in [(None, 1), (Some(99), 0), (Some(10), 0)] {
        let mut operands = operands.clone();
        operands[0].resolved_edge_slot = resolved;
        let projected = crate::test_support::with_decode_context(|ctx| {
            let scopes = std::slice::from_ref(&scope);
            let timelines = crate::design::test_support::synthetic_feature_timelines(scopes);
            project_parameter_design_with_edge_identities(
                ctx,
                &ProjectInputs {
                    scopes,
                    timelines: &timelines,
                    construction_groups: std::slice::from_ref(&group),
                    edge_operands: &operands,
                    ..Default::default()
                },
            )
        })
        .unwrap();
        let FeatureDefinition::Operation(FeatureOperation::Fillet { groups }) =
            projected.features[0].evaluation.definition()
        else {
            panic!("fixed Fillet projection");
        };
        if resolved == Some(99) {
            assert!(
                matches!(&groups[0].edges, EdgeSelection::HistoricalPartial { edges, unresolved, .. }
                if edges.len() == 1 && edges[0].as_str().ends_with(":99")
                    && unresolved.len() == 1 && unresolved[0] == operands[1].id)
            );
        } else {
            assert!(
                matches!(&groups[0].edges, EdgeSelection::Historical { edges, .. } if edges.len() == 2)
            );
        }
        assert_eq!(projected.projection_losses.len(), expected_notes);
        if expected_notes == 1 {
            assert_eq!(projected.projection_losses[0],
                crate::loss::F3dLossCode::HistoricalEdgeSelectionDeletedSetAssumed.note(
                    "Group f3d:test/Design/BulkStream.dat:group-2 is read with the assumption that the treated edge set equals the transition-deleted set."
                ));
        }
    }
}
