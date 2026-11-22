// SPDX-License-Identifier: Apache-2.0
use super::super::FaceRecipeIndex;
use crate::history_records::{
    AsmHistoricalCoedge, AsmHistoricalEdge, AsmHistoricalRelation, AsmHistoricalTopology,
};
use crate::records::topology::face::{DesignFaceOperand, DesignFaceRecipeNode};

fn cylinder(cap: i64, wall: i64, opposite: i64) -> AsmHistoricalTopology {
    let mut topology = AsmHistoricalTopology {
        faces: vec![wall, opposite, cap],
        edges: vec![10, 20],
        vertices: vec![1, 2],
        edge_vertices: [(10, 1), (20, 2)]
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
        (wall, vec![100, 200]),
        (opposite, vec![300]),
        (cap, vec![400]),
    ] {
        topology.face_loops.push(AsmHistoricalRelation {
            owner_ref: face,
            member_refs: loops,
        });
    }
    for (loop_, edge, radial_next) in [
        (100, 10, 400),
        (200, 20, 300),
        (300, 20, 200),
        (400, 10, 100),
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

fn cap_operand(cap: i64, wall: i64) -> DesignFaceOperand {
    let mut operand = super::operand();
    operand.recipe_references.truncate(2);
    for (reference, face) in operand.recipe_references.iter_mut().zip([cap, wall]) {
        reference.candidate_faces = vec![crate::ids::brep_face_id(face)];
    }
    operand.candidate_faces = vec![crate::ids::brep_face_id(wall)];
    operand.preceding_candidate_faces = operand.candidate_faces.clone();
    operand.changed_candidate_faces = operand.candidate_faces.clone();
    let fields = [1, -1, 0, -1, 1, -1, 2, 1, -1, 0, -1, 0, 0, -1];
    let structure = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::operands::recipe_structure::face_recipe_structure(ctx, &fields)
            .unwrap()
            .unwrap()
    });
    let mut program = vec![-1, -1, 2];
    program.extend(fields);
    operand.recipe_program = vec![0, -1, 1];
    operand.recipe_program.extend(&program);
    operand.recipe_nodes = vec![DesignFaceRecipeNode {
        byte_offset: 12,
        end_byte_offset: 80,
        program,
        recipe_structure: Some(structure),
    }];
    operand
}

#[test]
fn closed_profile_boundary_selects_cap_with_changed_wall_candidate() {
    for (cap, wall, opposite) in [(42, 9, 13), (7, 90, 130)] {
        let mut topology = cylinder(cap, wall, opposite);
        let operand = cap_operand(cap, wall);
        assert_eq!(
            crate::design::face_resolve::resolve_face_operand_history_candidates(&operand),
            Some(wall)
        );
        assert_eq!(FaceRecipeIndex::new(&topology).resolve(&operand), Some(cap));
        topology.faces.reverse();
        assert_eq!(FaceRecipeIndex::new(&topology).resolve(&operand), Some(cap));
        assert_eq!(FaceRecipeIndex::new(&topology).resolve_loop(&operand), None);
    }
}

#[test]
fn closed_profile_boundary_requires_the_complete_cap_and_incident_pair() {
    let mut topology = cylinder(42, 9, 13);
    let operand = cap_operand(42, 9);
    topology
        .face_loops
        .last_mut()
        .unwrap()
        .member_refs
        .push(300);
    assert_eq!(FaceRecipeIndex::new(&topology).resolve(&operand), None);

    let topology = cylinder(42, 9, 13);
    let operand = cap_operand(13, 9);
    assert_eq!(FaceRecipeIndex::new(&topology).resolve(&operand), Some(13));
    let operand = cap_operand(42, 13);
    assert_eq!(FaceRecipeIndex::new(&topology).resolve(&operand), None);
}

#[test]
fn extrude_history_binding_replaces_changed_wall_with_selected_cap() {
    use crate::history::{bind_face_operand_history_candidates, BindingHistories};
    use crate::history_records::{
        AsmDeltaState, AsmEntityVersion, AsmHistoricalPersistentSubentityTag,
    };
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
    use std::collections::HashMap;

    let mut operand = cap_operand(42, 9);
    let mut preceding = cylinder(42, 9, 13);
    for (reference, face) in operand.recipe_references.iter_mut().zip([42, 9]) {
        reference.design_reference += 401;
        preceding
            .persistent_subentity_tags
            .push(AsmHistoricalPersistentSubentityTag {
                entity_kind: AsmHistoricalEntityKind::Face,
                entity_ref: face,
                selector: 1,
                token: reference.token.clone(),
                token_integer: 0,
                design_references: vec![reference.design_reference],
                ordinal: 0,
            });
    }
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
    let versions = |wall_record| {
        [
            AsmEntityVersion {
                entity_ref: 9,
                record_ref: wall_record,
            },
            AsmEntityVersion {
                entity_ref: 13,
                record_ref: 13,
            },
        ]
        .to_vec()
    };
    let mut preceding_versions = versions(9);
    preceding_versions.push(AsmEntityVersion {
        entity_ref: 42,
        record_ref: 42,
    });
    let mut result_versions = versions(90);
    result_versions.push(AsmEntityVersion {
        entity_ref: 7,
        record_ref: 7,
    });
    let history = crate::test_support::history::history(
        "f3d:test:history".into(),
        0,
        None,
        false,
        vec![
            state(47, cylinder(7, 9, 13), result_versions),
            state(45, preceding, preceding_versions),
        ],
    );
    let transition = history.states()[0].transition.as_ref().unwrap();
    assert_eq!(transition.topology.faces.deleted, [42]);
    assert_eq!(transition.topology.faces.updated, [9]);
    let mut scope =
        DesignParameterScope::empty("f3d:test:scope#10", DesignFeatureKind::Extrude, 10);
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(47);
            draft.previous_history_state_id = Some(45);
            draft.layout_fixture_tail();
        })
        .unwrap();
    let mut operands = vec![operand];
    crate::test_support::with_decode_context(|ctx| {
        bind_face_operand_history_candidates(
            ctx,
            &mut operands,
            &[scope],
            &[],
            &[],
            BindingHistories::Shared(&[history]),
            &HashMap::new(),
        )
    })
    .unwrap();
    assert_eq!(
        operands[0].preceding_candidate_faces,
        [crate::ids::brep_face_id(9)]
    );
    assert_eq!(
        operands[0].changed_candidate_faces,
        [crate::ids::brep_face_id(9)]
    );
    assert_eq!(operands[0].resolved_face_slots, [42]);
}
