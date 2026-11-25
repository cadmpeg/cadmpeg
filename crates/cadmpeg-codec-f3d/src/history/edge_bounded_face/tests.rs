// SPDX-License-Identifier: Apache-2.0
use super::unique_edge_bounded_face;
use crate::history::topology_cache::TopologyQueryCache;
use crate::history_records::{
    AsmHistoricalCoedge, AsmHistoricalEdge, AsmHistoricalRelation, AsmHistoricalTopology,
};
use crate::records::dimensions::DesignRecipeReference;
use crate::records::topology::face::DesignFaceOperand;

const WALL: i64 = 9;
const FAR_CAP: i64 = 13;
const NEAR_CAP: i64 = 42;
const NEAR_EDGE: i64 = 10;
const FAR_EDGE: i64 = 20;

/// A cylinder: the wall holds both circular edges; each cap holds one.
fn cylinder() -> AsmHistoricalTopology {
    let mut topology = AsmHistoricalTopology {
        faces: vec![WALL, FAR_CAP, NEAR_CAP],
        edges: vec![NEAR_EDGE, FAR_EDGE],
        vertices: vec![1, 2],
        edge_vertices: [(NEAR_EDGE, 1), (FAR_EDGE, 2)]
            .into_iter()
            .map(|(edge, vertex)| AsmHistoricalEdge {
                edge,
                start_vertex: vertex,
                end_vertex: vertex,
            })
            .collect(),
        ..Default::default()
    };
    for (face, loops) in [
        (WALL, vec![100, 200]),
        (FAR_CAP, vec![300]),
        (NEAR_CAP, vec![400]),
    ] {
        topology.face_loops.push(AsmHistoricalRelation {
            owner_ref: face,
            member_refs: loops,
        });
    }
    for (loop_, edge, radial_next) in [
        (100, NEAR_EDGE, 400),
        (200, FAR_EDGE, 300),
        (300, FAR_EDGE, 200),
        (400, NEAR_EDGE, 100),
    ] {
        topology.loop_coedges.push(AsmHistoricalRelation {
            owner_ref: loop_,
            member_refs: vec![loop_],
        });
        topology.coedge_topology.push(AsmHistoricalCoedge {
            coedge: loop_,
            owner_loop: loop_,
            edge,
            next: loop_,
            previous: loop_,
            radial_next,
        });
    }
    topology
}

fn reference(face: Option<i64>, edge: Option<i64>) -> DesignRecipeReference {
    DesignRecipeReference {
        selector: 1,
        selector_offset: 0,
        token: format!("t{}", face.or(edge).unwrap_or_default()),
        token_integer: 0,
        token_offset: 0,
        design_reference: 303,
        design_reference_offset: 0,
        candidate_faces: face.map(crate::ids::brep_face_id).into_iter().collect(),
        candidate_edges: edge
            .map(|edge| {
                cadmpeg_ir::ids::EdgeId::compose(
                    &cadmpeg_ir::identity_namespace!("f3d", "brep", "entity"),
                    edge,
                )
            })
            .into_iter()
            .collect(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
        operand_offset: None,
    }
}

/// A bounded-face operand with no parsed nodes that names `cap`, `edge` and
/// `wall`, whose history fallback offers `fallback`.
fn operand(cap: i64, edge: i64, wall: i64, fallback: i64) -> DesignFaceOperand {
    let mut operand: DesignFaceOperand = serde_json::from_value(serde_json::json!({
        "id": "f3d:test:face-operand#21", "scope_record_index": 10, "scope_reference_ordinal": 2,
        "group_record_index": 20, "group_member_ordinal": 0, "record_index": 21, "byte_offset": 0,
        "class_tag": "282", "paired_byte_offset": 12, "paired_class_tag": "259",
        "recipe_record_index": 24, "recipe_record_byte_offset": 24, "recipe_id": "f3d:test:recipe#22",
        "recipe_prefix_offset": 35, "recipe_prefix_bytes": "", "recipe_references": [],
        "recipe_kind": "bounded_face", "recipe_program_offset": 0,
        "recipe_program": [0, -1, 1, 1, -1, 2],
        "recipe_node_offsets": [], "recipe_nodes": [], "next_record_index": 23, "next_byte_offset": 36
    }))
    .unwrap();
    operand.recipe_references = vec![
        reference(Some(cap), None),
        reference(None, Some(edge)),
        reference(Some(wall), None),
    ];
    operand.candidate_faces = [wall, FAR_CAP, NEAR_CAP]
        .into_iter()
        .map(crate::ids::brep_face_id)
        .collect();
    operand.unreferenced_candidate_faces = vec![crate::ids::brep_face_id(fallback)];
    operand.preceding_candidate_faces = vec![crate::ids::brep_face_id(fallback)];
    operand
}

/// The slots of the faces that the history fallback offers.
fn fallback_of(operand: &DesignFaceOperand) -> Vec<i64> {
    operand
        .preceding_candidate_faces
        .iter()
        .filter_map(|face| crate::history::body_closures::stable_ref(face.as_str()))
        .collect()
}

#[test]
fn the_named_face_bounded_by_the_named_edge_replaces_a_fallback_that_shares_no_edge() {
    let topology = cylinder();
    let mut queries = TopologyQueryCache::default();
    let operand = operand(NEAR_CAP, NEAR_EDGE, WALL, FAR_CAP);
    assert_eq!(
        unique_edge_bounded_face(&operand, &fallback_of(&operand), &topology, &mut queries),
        Some(NEAR_CAP)
    );
    let operand = self::operand(FAR_CAP, FAR_EDGE, WALL, NEAR_CAP);
    assert_eq!(
        unique_edge_bounded_face(&operand, &fallback_of(&operand), &topology, &mut queries),
        Some(FAR_CAP)
    );
}

#[test]
fn a_fallback_face_on_a_named_edge_is_kept() {
    let topology = cylinder();
    let mut queries = TopologyQueryCache::default();
    // The fallback cap holds the named edge, so the fallback stands.
    let operand = operand(NEAR_CAP, NEAR_EDGE, WALL, NEAR_CAP);
    assert_eq!(
        unique_edge_bounded_face(&operand, &fallback_of(&operand), &topology, &mut queries),
        None
    );
    // The wall holds the named edge as well.
    let operand = self::operand(NEAR_CAP, NEAR_EDGE, WALL, WALL);
    assert_eq!(
        unique_edge_bounded_face(&operand, &fallback_of(&operand), &topology, &mut queries),
        None
    );
}

#[test]
fn no_face_bounded_by_exactly_the_named_edges_selects_nothing() {
    let topology = cylinder();
    let mut queries = TopologyQueryCache::default();
    // The wall's boundary is both edges, and no other named face holds only
    // the named edge.
    let mut operand = operand(WALL, NEAR_EDGE, WALL, FAR_CAP);
    assert_eq!(
        unique_edge_bounded_face(&operand, &fallback_of(&operand), &topology, &mut queries),
        None
    );
    // Without a named edge the recipe bounds no face.
    operand.recipe_references.remove(1);
    assert_eq!(
        unique_edge_bounded_face(&operand, &fallback_of(&operand), &topology, &mut queries),
        None
    );
}

#[test]
fn a_recipe_with_parsed_nodes_is_left_to_the_node_binding() {
    let topology = cylinder();
    let mut queries = TopologyQueryCache::default();
    let mut operand = operand(NEAR_CAP, NEAR_EDGE, WALL, FAR_CAP);
    operand
        .recipe_nodes
        .push(crate::records::topology::face::DesignFaceRecipeNode {
            byte_offset: 0,
            end_byte_offset: 1,
            program: Vec::new(),
            recipe_structure: None,
        });
    assert_eq!(
        unique_edge_bounded_face(&operand, &fallback_of(&operand), &topology, &mut queries),
        None
    );
}

#[test]
fn an_extrude_start_face_binds_the_edge_bounded_cap() {
    use crate::history::binding_histories::BindingHistories;
    use crate::history::face_operand_history::bind_face_operand_history_candidates;
    use crate::history_records::{
        AsmDeltaState, AsmEntityVersion, AsmHistoricalPersistentSubentityTag,
    };
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
    use std::collections::HashMap;

    let state = |state_id, mut topology: AsmHistoricalTopology, versions| {
        topology.faces.sort_unstable();
        topology.face_loops.sort_by_key(|row| row.owner_ref);
        topology
            .persistent_subentity_tags
            .sort_by_key(|row| row.entity_ref);
        (
            AsmDeltaState {
                id: format!("f3d:test:state#{state_id}"),
                parent: "f3d:test:history".into(),
                byte_offset: 0,
                state_id,
                version_flag: 1,
                state_flag: 0,
                previous_ref: None,
                next_ref: None,
                node_index: state_id,
                partner_ref: None,
                owner_ref: 0,
                bulletin_boards: Vec::new(),
                records: Vec::new(),
                transition: None,
            },
            Some(topology),
            versions,
        )
    };
    let versions = |with_new_face: bool| {
        let mut faces = vec![WALL, FAR_CAP, NEAR_CAP];
        if with_new_face {
            faces.push(77);
        }
        faces
            .into_iter()
            .map(|entity_ref| AsmEntityVersion {
                entity_ref,
                record_ref: entity_ref,
            })
            .collect::<Vec<_>>()
    };
    let mut preceding = cylinder();
    let recipe = operand(NEAR_CAP, NEAR_EDGE, WALL, FAR_CAP);
    for (reference, (kind, slot)) in recipe.recipe_references.iter().zip([
        (AsmHistoricalEntityKind::Face, NEAR_CAP),
        (AsmHistoricalEntityKind::Edge, NEAR_EDGE),
        (AsmHistoricalEntityKind::Face, WALL),
    ]) {
        preceding
            .persistent_subentity_tags
            .push(AsmHistoricalPersistentSubentityTag {
                entity_kind: kind,
                entity_ref: slot,
                selector: reference.selector,
                token: reference.token.clone(),
                token_integer: 0,
                design_references: vec![reference.design_reference],
                ordinal: 0,
            });
    }
    let mut after = cylinder();
    after.faces.push(77);
    let history = crate::test_support::history::history(
        "f3d:test:history".into(),
        0,
        None,
        false,
        vec![
            state(47, after, versions(true)),
            state(45, preceding, versions(false)),
        ],
    );
    let mut scope =
        DesignParameterScope::empty("f3d:test:scope#10", DesignFeatureKind::Extrude, 10);
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(47);
            draft.previous_history_state_id = Some(45);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let group = serde_json::from_value(serde_json::json!({
        "id": "f3d:test:construction-group#20", "scope_record_index": 10,
        "scope_reference_ordinal": 2, "record_index": 20, "byte_offset": 0, "class_tag": "338",
        "members": [21], "member_offsets": [0],
        "frame": {
            "member_count_offset": 0, "auxiliary_record_indices": [],
            "auxiliary_record_offsets": [], "auxiliary_paths": [],
            "trailing_record_indices": [], "trailing_record_offsets": [],
            "trailing_transforms": [], "trailing_dual_transforms": [], "trailing_flags": [],
            "opaque_index": 1, "opaque_index_offset": 18, "opaque_scalar": 0.0,
            "opaque_scalar_offset": 22, "variant": false
        },
        "role": crate::records::topology::extrude_selection::DesignOperandRole::FACES.raw(),
        "extrude_role": "faces", "extrude_face_role": "start", "role_offset": 0,
        "paired_class_tag": "261", "paired_byte_offset": 0
    }))
    .unwrap();
    let mut operands = vec![recipe];
    crate::test_support::with_decode_context(|ctx| {
        bind_face_operand_history_candidates(
            ctx,
            &mut operands,
            &[scope],
            &[group],
            &[],
            BindingHistories::Shared(&[history]),
            &HashMap::new(),
        )
    })
    .unwrap();
    assert_eq!(operands[0].resolved_face_slots, [NEAR_CAP]);
    assert_eq!(
        operands[0].preceding_candidate_faces,
        [crate::ids::brep_face_id(NEAR_CAP)]
    );
}
