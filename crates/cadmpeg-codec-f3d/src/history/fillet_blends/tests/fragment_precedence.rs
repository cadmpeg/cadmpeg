// SPDX-License-Identifier: Apache-2.0

use super::*;

fn fragment_blend_operand(
    input: &AsmHistoricalTopology,
) -> (
    crate::records::topology::edge_identity::DesignEdgeOperand,
    Vec<crate::records::topology::historical_context::AsmFaceBoundary>,
) {
    let program = [
        -1, -1, 2, 0, -1, 1, -1, 2, -1, 3, 1, -1, 0, -1, 2, -1, 0, 0, -1, 3, 0, -1, 0, -1, 0, -1,
        0, 0, -1,
    ];
    let mut operand: crate::records::topology::edge_identity::DesignEdgeOperand =
        serde_json::from_value(serde_json::json!({
            "id": "f3d:Design/BulkStream.dat:edge-operand#7", "scope_record_index":42,
            "scope_reference_ordinal":0, "record_index":7, "byte_offset":0, "class_tag":"297",
            "paired_byte_offset":16, "paired_class_tag":"259", "recipe_record_index":10,
            "recipe_record_byte_offset":32, "recipe_id":"f3d:test:recipe", "recipe_prefix_offset":43,
            "recipe_prefix_bytes":"", "recipe_references":[], "recipe_program_offset":0,
            "recipe_program":[], "next_record_index":11, "next_byte_offset":160,
        })).unwrap();
    operand.recipe_structure = crate::design::decode::operands::edge_recipe_structure(
        &cadmpeg_test_support::service_decode_context(),
        &program,
    )
    .unwrap();
    operand.recipe_state_id = Some(1);
    let boundaries = crate::history::face_boundary_contexts_for_slots(
        &[10, 13, 11],
        input,
        &mut crate::history::topology_cache::TopologyQueryCache::default(),
    )
    .into_iter()
    .map(
        |b| crate::records::topology::historical_context::AsmFaceBoundary {
            id: format!("boundary-{}", b.face_slot),
            parent: "state-1".into(),
            face_slot: b.face_slot,
            loops: b.loops,
        },
    )
    .collect();
    operand.recipe_reference_contexts = [vec![10,13],vec![11]].into_iter().enumerate().map(|(ordinal,faces)| {
        serde_json::from_value(serde_json::json!({
            "reference_ordinal":ordinal,"result_faces":[],"result_shared_edge_slots":[],
            "preceding_faces":faces.iter().map(|f| format!("f3d:brep:entity#{f}")).collect::<Vec<_>>(),
            "preceding_face_boundary_ids":faces.iter().map(|f| format!("boundary-{f}")).collect::<Vec<_>>(),
            "shared_edge_slots":[],"changed_shared_edge_slots":[],
        })).unwrap()
    }).collect();
    (operand, boundaries)
}

#[test]
fn blend_transition_selection_precedes_fragment_member_singletons() {
    let mut input = input(z());
    let extra = faces(&[(13, &[50])]);
    input.faces.extend(extra.faces);
    input.surfaces.extend(extra.surfaces);
    input.face_surfaces.extend(extra.face_surfaces);
    input.face_loops.extend(extra.face_loops);
    input.loop_coedges.extend(extra.loop_coedges);
    input.coedge_topology.extend(extra.coedge_topology);
    input.edges.push(50);
    let (operand, boundaries) = fragment_blend_operand(&input);
    let index = boundaries.iter().map(|b| (b.id.as_str(), b)).collect();
    assert_eq!(
        crate::history::fragment_edge::context_edge(&operand, &index).map(|edge| edge.slot),
        Some(17)
    );
    let feature = FeatureId::mint("f3d:test:feature#42").unwrap();
    let group = group(42, &[7]);
    let mut definition = fillet(native(), three_mm());
    let mut notes = Vec::new();
    bind_fillet_groups(
        &mut definition,
        &feature,
        &scope(42),
        std::slice::from_ref(&group),
        1,
        (&transition(&[20, 21], &[17, 18, 19]), &input, &two_blends()),
        &mut notes,
    );
    let expected = fillet(
        EdgeSelection::historical(
            crate::ids::feature_input_topology_id(&feature, 1),
            [17, 18]
                .into_iter()
                .map(|slot| {
                    crate::ids::history_input_edge_id(
                        &crate::ids::feature_input_prefix(&feature, 1),
                        slot,
                    )
                })
                .collect(),
            GROUP_ID.into(),
        )
        .unwrap(),
        three_mm(),
    );
    assert_eq!(definition, expected);
    crate::design::edge_resolve::fragment_fallback::apply(
        &mut definition,
        &feature,
        &[group],
        &[operand],
        &[],
        &boundaries,
        &mut notes,
    );
    assert_eq!(definition, expected);
    assert!(notes.is_empty());
}
