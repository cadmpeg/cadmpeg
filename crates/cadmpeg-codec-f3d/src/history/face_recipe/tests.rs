// SPDX-License-Identifier: Apache-2.0
use super::{fan_matches, FaceRecipeIndex};
use crate::history_records::{
    AsmHistoricalCoedge, AsmHistoricalEdge, AsmHistoricalRelation, AsmHistoricalTopology,
};
use crate::records::topology::face::{DesignFaceOperand, DesignFaceRecipeNode};
use std::collections::BTreeSet;

mod profile_cap;

fn topology() -> AsmHistoricalTopology {
    let mut topology = AsmHistoricalTopology {
        faces: vec![100, 101, 102, 103],
        edges: vec![10, 11, 12, 13, 14, 15],
        vertices: vec![1, 2, 3, 4],
        edge_vertices: [
            (10, 1, 2),
            (11, 2, 3),
            (12, 3, 1),
            (13, 1, 4),
            (14, 4, 2),
            (15, 3, 4),
        ]
        .into_iter()
        .map(|(edge, start_vertex, end_vertex)| AsmHistoricalEdge {
            edge,
            start_vertex,
            end_vertex,
        })
        .collect(),
        ..Default::default()
    };
    for (ordinal, edges) in [[10, 11, 12], [10, 13, 14], [11, 14, 15], [12, 15, 13]]
        .into_iter()
        .enumerate()
    {
        let face = 100 + i64::try_from(ordinal).unwrap();
        let loop_ = face + 100;
        let first = face * 10;
        topology.face_loops.push(AsmHistoricalRelation {
            owner_ref: face,
            member_refs: vec![loop_],
        });
        let coedges: Vec<_> = (first..first + 3).collect();
        topology.loop_coedges.push(AsmHistoricalRelation {
            owner_ref: loop_,
            member_refs: coedges.clone(),
        });
        for (position, edge) in edges.into_iter().enumerate() {
            topology.coedge_topology.push(AsmHistoricalCoedge {
                coedge: coedges[position],
                owner_loop: loop_,
                edge,
                next: coedges[(position + 1) % 3],
                previous: coedges[(position + 2) % 3],
                radial_next: 0,
            });
        }
    }
    topology
}

fn operand() -> DesignFaceOperand {
    let mut operand: DesignFaceOperand = cadmpeg_core::serde_erase::json::from_value(serde_json::json!({
        "id": "f3d:test:face-operand#21", "scope_record_index": 10, "scope_reference_ordinal": 1,
        "group_record_index": 20, "group_member_ordinal": 0, "record_index": 21, "byte_offset": 0,
        "class_tag": "282", "paired_byte_offset": 12, "paired_class_tag": "259",
        "recipe_record_index": 24, "recipe_record_byte_offset": 24, "recipe_id": "f3d:test:recipe#22",
        "recipe_prefix_offset": 35, "recipe_prefix_bytes": "", "recipe_references": [],
        "recipe_kind": "bounded_face", "recipe_program_offset": 0, "recipe_program": [0, -1, 3],
        "recipe_node_offsets": [], "recipe_nodes": [], "next_record_index": 23, "next_byte_offset": 36
    })).unwrap();
    for ordinal in 0..4 {
        operand
            .recipe_references
            .push(crate::records::dimensions::DesignRecipeReference {
                selector: 1,
                selector_offset: ordinal,
                token: format!("face{ordinal}"),
                token_integer: 0,
                token_offset: ordinal,
                design_reference: i64::try_from(ordinal).unwrap(),
                design_reference_offset: ordinal,
                candidate_faces: vec![crate::ids::brep_face_id(
                    100 + i64::try_from(ordinal).unwrap(),
                )],
                alternate_selector_faces: Vec::new(),
                candidate_edges: Vec::new(),
                alternate_selector_edges: Vec::new(),
                operand_offset: None,
            });
    }
    for (other, fans) in [
        (1, [[0, 1, 3], [0, 1, 2]]),
        (2, [[0, 1, 2], [0, 2, 3]]),
        (3, [[0, 2, 3], [0, 1, 3]]),
    ] {
        let mut fields = vec![0, -1, other, -1, 2, -1];
        for fan in fans {
            fields.extend([3, fan[0], -1, fan[1], -1, fan[2], -1, 0, 0, -1]);
        }
        let structure = crate::test_support::with_decode_context(|ctx| {
            crate::design::decode::operands::recipe_structure::face_recipe_structure(ctx, &fields)
                .unwrap()
                .unwrap()
        });
        let mut program = vec![-1, -1, 2];
        program.extend(fields);
        operand.recipe_program.extend(&program);
        operand.recipe_nodes.push(DesignFaceRecipeNode {
            byte_offset: 0,
            end_byte_offset: 12,
            program,
            recipe_structure: Some(structure),
        });
    }
    operand
}

#[test]
fn bounded_face_nodes_resolve_the_complete_referenced_boundary() {
    let topology = topology();
    let operand = operand();
    assert_eq!(FaceRecipeIndex::new(&topology).resolve(&operand), Some(100));
    assert!(operand.preceding_candidate_faces.is_empty());
    assert!(operand.candidate_faces.is_empty());
}

fn topology_with_four_face_edge() -> AsmHistoricalTopology {
    let mut topology = topology();
    topology.edges.push(16);
    topology.edge_vertices.push(AsmHistoricalEdge {
        edge: 16,
        start_vertex: 1,
        end_vertex: 4,
    });
    for face in [104, 105] {
        let loop_ = face + 100;
        let first = face * 10;
        topology.faces.push(face);
        topology.face_loops.push(AsmHistoricalRelation {
            owner_ref: face,
            member_refs: vec![loop_],
        });
        topology.loop_coedges.push(AsmHistoricalRelation {
            owner_ref: loop_,
            member_refs: vec![first, first + 1],
        });
        for (coedge, edge, other) in [(first, 13, first + 1), (first + 1, 16, first)] {
            topology.coedge_topology.push(AsmHistoricalCoedge {
                coedge,
                owner_loop: loop_,
                edge,
                next: other,
                previous: other,
                radial_next: 0,
            });
        }
    }
    topology
}

fn set_node_fans(operand: &mut DesignFaceOperand, index: usize, fans: [&[i32]; 2]) {
    let node = &mut operand.recipe_nodes[index];
    let structure = node.recipe_structure.as_ref().unwrap();
    let mut fields = vec![structure.root, -1, structure.prelude[0], -1, 2, -1];
    for fan in fans {
        fields.push(i32::try_from(fan.len()).unwrap());
        for &ordinal in fan {
            fields.extend([ordinal, -1]);
        }
        fields.extend([0, 0, -1]);
    }
    node.recipe_structure = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::operands::recipe_structure::face_recipe_structure(ctx, &fields)
            .unwrap()
    });
    node.program = vec![-1, -1, 2];
    node.program.extend(fields);
    operand.recipe_program = vec![0, -1, i32::try_from(operand.recipe_nodes.len()).unwrap()];
    for node in &operand.recipe_nodes {
        operand.recipe_program.extend_from_slice(&node.program);
    }
}

#[test]
fn bounded_face_nodes_use_the_clause_to_cross_a_four_face_edge() {
    let index = FaceRecipeIndex::new(&topology_with_four_face_edge());
    let mut operand = operand();
    for ordinal in [4, 5] {
        let mut reference = operand.recipe_references[0].clone();
        reference.selector_offset = ordinal;
        reference.token_offset = ordinal;
        reference.design_reference = i64::try_from(ordinal).unwrap();
        reference.design_reference_offset = ordinal;
        reference.candidate_faces = vec![crate::ids::brep_face_id(
            100 + i64::try_from(ordinal).unwrap(),
        )];
        operand.recipe_references.push(reference);
    }
    set_node_fans(&mut operand, 0, [&[0, 1, 4, 5, 3], &[0, 1, 2]]);
    set_node_fans(&mut operand, 2, [&[0, 2, 3], &[0, 1, 4, 5, 3]]);
    assert_eq!(index.resolve(&operand), Some(100));
    assert_eq!(index.resolve_loop(&operand), Some(100));
    operand.recipe_references[1]
        .candidate_faces
        .push(crate::ids::brep_face_id(104));
    operand.recipe_references[3]
        .candidate_faces
        .push(crate::ids::brep_face_id(105));
    set_node_fans(&mut operand, 0, [&[0, 1, 1, 3, 3], &[0, 1, 2]]);
    set_node_fans(&mut operand, 2, [&[0, 2, 3], &[0, 1, 1, 3, 3]]);
    assert_eq!(index.resolve(&operand), Some(100));
    for invalid in [&[0, 1, 4, 3, 5][..], &[0, 1, 4, 3], &[0, 1, 4, 4, 3]] {
        set_node_fans(&mut operand, 0, [invalid, &[0, 1, 2]]);
        assert_eq!(index.resolve(&operand), None);
        assert_eq!(index.resolve_loop(&operand), None);
    }
}

/// Face 100 gains an inner loop whose one closed edge 20, at vertex 5, it
/// shares with face 104.
fn topology_with_inner_loop() -> AsmHistoricalTopology {
    let mut topology = topology();
    topology.faces.push(104);
    topology.edges.push(20);
    topology.vertices.push(5);
    topology.edge_vertices.push(AsmHistoricalEdge {
        edge: 20,
        start_vertex: 5,
        end_vertex: 5,
    });
    topology.face_loops[0].member_refs.push(300);
    topology.face_loops.push(AsmHistoricalRelation {
        owner_ref: 104,
        member_refs: vec![301],
    });
    for (loop_, coedge, radial_next) in [(300, 2000, 2001), (301, 2001, 2000)] {
        topology.loop_coedges.push(AsmHistoricalRelation {
            owner_ref: loop_,
            member_refs: vec![coedge],
        });
        topology.coedge_topology.push(AsmHistoricalCoedge {
            coedge,
            owner_loop: loop_,
            edge: 20,
            next: coedge,
            previous: coedge,
            radial_next,
        });
    }
    topology
}

#[test]
fn bounded_face_nodes_resolve_a_complete_loop_of_a_face_with_an_inner_loop() {
    let topology = topology_with_inner_loop();
    let index = FaceRecipeIndex::new(&topology);
    let mut operand = operand();
    assert_eq!(index.resolve(&operand), None);
    assert_eq!(index.resolve_loop(&operand), Some(100));
    operand.recipe_nodes.truncate(2);
    operand.recipe_program[2] = 2;
    assert_eq!(index.resolve_loop(&operand), None);
}

/// Add operand 4 for face 104 and a node for the closed edge between faces
/// 100 and 104. The node has one endpoint clause with the fan `fan`.
fn add_closed_edge_node(operand: &mut DesignFaceOperand, fan: [i32; 2]) {
    let mut reference = operand.recipe_references[3].clone();
    reference.selector_offset = 4;
    reference.token_offset = 4;
    reference.design_reference = 4;
    reference.design_reference_offset = 4;
    reference.candidate_faces = vec![crate::ids::brep_face_id(104)];
    operand.recipe_references.push(reference);
    let fields = vec![0, -1, 4, -1, 1, -1, 2, fan[0], -1, fan[1], -1, 0, 0, -1];
    let structure = crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::operands::recipe_structure::face_recipe_structure(ctx, &fields)
            .unwrap()
            .unwrap()
    });
    assert_eq!(structure.sides.len(), 1);
    let mut program = vec![-1, -1, 2];
    program.extend(fields);
    operand.recipe_program[2] = 4;
    operand.recipe_program.extend(&program);
    operand.recipe_nodes.push(DesignFaceRecipeNode {
        byte_offset: 0,
        end_byte_offset: 12,
        program,
        recipe_structure: Some(structure),
    });
}

#[test]
fn a_one_clause_node_binds_a_closed_edge_by_the_fan_at_its_one_vertex() {
    let index = FaceRecipeIndex::new(&topology_with_inner_loop());
    let mut operand = operand();
    add_closed_edge_node(&mut operand, [4, 0]);
    assert_eq!(operand.recipe_program[2], 4);
    assert_eq!(
        operand
            .recipe_nodes
            .iter()
            .map(|node| node.recipe_structure.as_ref().unwrap().prelude[1])
            .collect::<Vec<_>>(),
        [2, 2, 2, 1]
    );
    assert_eq!(index.resolve(&operand), Some(100));
    let mut operand = self::operand();
    add_closed_edge_node(&mut operand, [4, 1]);
    assert_eq!(index.resolve(&operand), None);
}

#[test]
fn a_one_clause_node_refuses_a_closed_edge_whose_vertex_meets_another_edge() {
    let mut operand = operand();
    add_closed_edge_node(&mut operand, [4, 0]);
    let mut topology = topology_with_inner_loop();
    topology.edges.push(21);
    topology.vertices.push(6);
    topology.edge_vertices.push(AsmHistoricalEdge {
        edge: 21,
        start_vertex: 5,
        end_vertex: 6,
    });
    let index = FaceRecipeIndex::new(&topology);
    assert!(!index.edge_faces.contains_key(&21));
    assert_eq!(index.closed_edge_fan(20, 5), None);
    assert_eq!(index.resolve(&operand), None);
}

/// Add face `face` with one loop of the two edges `edges`.
fn add_two_edge_face(topology: &mut AsmHistoricalTopology, face: i64, edges: [i64; 2]) {
    let loop_ = face + 100;
    let first = face * 10;
    topology.faces.push(face);
    topology.face_loops.push(AsmHistoricalRelation {
        owner_ref: face,
        member_refs: vec![loop_],
    });
    topology.loop_coedges.push(AsmHistoricalRelation {
        owner_ref: loop_,
        member_refs: vec![first, first + 1],
    });
    for (coedge, edge, other) in [(first, edges[0], first + 1), (first + 1, edges[1], first)] {
        topology.coedge_topology.push(AsmHistoricalCoedge {
            coedge,
            owner_loop: loop_,
            edge,
            next: other,
            previous: other,
            radial_next: 0,
        });
    }
}

fn add_edge(topology: &mut AsmHistoricalTopology, edge: i64, vertices: [i64; 2]) {
    topology.edges.push(edge);
    topology.edge_vertices.push(AsmHistoricalEdge {
        edge,
        start_vertex: vertices[0],
        end_vertex: vertices[1],
    });
}

fn singleton_lanes() -> Vec<BTreeSet<i64>> {
    (100..=107).map(|face| BTreeSet::from([face])).collect()
}

/// Whether the corner walk at `vertex` from edge 10 and face 100 matches
/// `ordinals` in their stored order, without the vertex shape gates.
fn corner_walk_without_gates(index: &FaceRecipeIndex, vertex: i64, ordinals: &[i32]) -> bool {
    let corners = &index.corners[&vertex];
    let mut incidence = std::collections::HashMap::<_, Vec<_>>::new();
    for (position, corner) in corners.iter().enumerate() {
        for &edge in &corner.edges {
            incidence.entry(edge).or_default().push(position);
        }
    }
    let lanes = singleton_lanes();
    let pattern: Vec<_> = ordinals
        .iter()
        .map(|&ordinal| &lanes[usize::try_from(ordinal).unwrap()])
        .collect();
    super::corner_walk_matches(10, 100, corners, &incidence, &pattern)
}

#[test]
fn a_branching_fan_refuses_a_second_four_use_edge() {
    let mut topology = topology_with_four_face_edge();
    add_edge(&mut topology, 17, [1, 4]);
    add_two_edge_face(&mut topology, 106, [16, 17]);
    add_two_edge_face(&mut topology, 107, [17, 16]);
    let index = FaceRecipeIndex::new(&topology);
    let ordinals = [0, 3, 4, 6, 7, 5, 1];
    assert!(corner_walk_without_gates(&index, 1, &ordinals));
    assert!(!index.branching_fan_matches(10, 1, 100, &ordinals, &singleton_lanes()));
}

#[test]
fn a_branching_fan_refuses_a_three_use_edge() {
    let mut topology = topology_with_four_face_edge();
    add_edge(&mut topology, 17, [1, 4]);
    add_two_edge_face(&mut topology, 106, [16, 17]);
    let index = FaceRecipeIndex::new(&topology);
    for ordinals in [[0, 3, 4, 6, 5, 1], [0, 3, 5, 6, 4, 1]] {
        assert!(!index.branching_fan_matches(10, 1, 100, &ordinals, &singleton_lanes()));
    }
}

#[test]
fn a_branching_fan_refuses_an_edge_without_a_corner_at_its_vertex() {
    let mut topology = topology_with_four_face_edge();
    let ordinals = [0, 1, 4, 5, 3];
    let index = FaceRecipeIndex::new(&topology);
    assert!(index.branching_fan_matches(10, 1, 100, &ordinals, &singleton_lanes()));
    add_edge(&mut topology, 18, [1, 9]);
    let index = FaceRecipeIndex::new(&topology);
    assert!(!index.branching_fan_matches(10, 1, 100, &ordinals, &singleton_lanes()));
}

#[test]
fn a_candidate_face_without_a_complete_boundary_vetoes_the_group() {
    let mut topology = topology();
    topology.faces.push(104);
    topology.face_loops.push(AsmHistoricalRelation {
        owner_ref: 104,
        member_refs: vec![204],
    });
    let mut operand = operand();
    operand.recipe_references[0]
        .candidate_faces
        .push(crate::ids::brep_face_id(104));
    let index = FaceRecipeIndex::new(&topology);
    assert!(!index.boundaries.contains_key(&104));
    assert_eq!(index.resolve(&operand), Some(100));
    let mut recipe = index.prepare(&operand).unwrap();
    recipe.candidates.insert(104);
    assert_eq!(index.resolve_prepared_boundary(recipe, false), None);
}

#[test]
fn a_one_clause_closed_edge_leaves_the_loop_recipe_unresolved() {
    let index = FaceRecipeIndex::new(&topology_with_inner_loop());
    let mut operand = operand();
    add_closed_edge_node(&mut operand, [4, 0]);
    let node = operand.recipe_nodes.pop().unwrap();
    operand.recipe_program = vec![0, -1, 1];
    operand.recipe_program.extend_from_slice(&node.program);
    operand.recipe_nodes = vec![node];
    let structure = operand.recipe_nodes[0].recipe_structure.as_ref().unwrap();
    assert_eq!(structure.prelude[1], 1);
    assert!(structure.has_valid_boundary_edge_ordinals(&operand.recipe_references));
    assert!(operand.resolved_face_slots.is_empty());
    assert_eq!(operand.group_record_index(), Some(20));
    let mut lanes: Vec<_> = (100..=104).map(|face| BTreeSet::from([face])).collect();
    assert!(index.node_matches(structure, 20, &lanes));
    assert_eq!(index.resolve_loop(&operand), None);

    let mut topology = topology_with_inner_loop();
    topology.faces.retain(|face| *face != 104);
    let other = topology.face_loops.pop().unwrap();
    topology.face_loops[0].member_refs.extend(other.member_refs);
    operand.recipe_references[4].candidate_faces = vec![crate::ids::brep_face_id(100)];
    lanes[4] = lanes[0].clone();
    let index = FaceRecipeIndex::new(&topology);
    assert_eq!(index.edge_faces[&20], [100, 100]);
    assert!(index.node_matches(structure, 20, &lanes));
    assert_eq!(index.resolve_loop(&operand), None);
}

#[test]
fn a_one_clause_node_does_not_bind_an_open_edge() {
    let index = FaceRecipeIndex::new(&topology());
    let mut operand = operand();
    let structure = operand.recipe_nodes[0].recipe_structure.as_mut().unwrap();
    structure.sides.truncate(1);
    assert_eq!(index.resolve(&operand), None);
}

#[test]
fn bounded_face_loop_proof_requires_distinct_edges_and_complete_endpoint_fans() {
    let index = FaceRecipeIndex::new(&topology());
    let mut operand = operand();
    operand.recipe_nodes[2] = operand.recipe_nodes[0].clone();
    assert_eq!(index.resolve_loop(&operand), None);
    let mut operand = self::operand();
    operand.recipe_nodes[0]
        .recipe_structure
        .as_mut()
        .unwrap()
        .sides[0]
        .scalars[0] = 2;
    assert_eq!(index.resolve_loop(&operand), None);
}

#[test]
fn bounded_face_loop_proof_requires_one_face_across_all_candidate_lanes() {
    let mut topology = topology();
    let mut other = topology.clone();
    for face in &mut other.faces {
        *face += 1000;
    }
    for edge in &mut other.edge_vertices {
        edge.edge += 1000;
        edge.start_vertex += 1000;
        edge.end_vertex += 1000;
    }
    for row in other.face_loops.iter_mut().chain(&mut other.loop_coedges) {
        row.owner_ref += 1000;
        for member in &mut row.member_refs {
            *member += 1000;
        }
    }
    for row in &mut other.coedge_topology {
        row.coedge += 1000;
        row.owner_loop += 1000;
        row.edge += 1000;
        row.next += 1000;
        row.previous += 1000;
    }
    topology.faces.extend(other.faces);
    topology.edge_vertices.extend(other.edge_vertices);
    topology.face_loops.extend(other.face_loops);
    topology.loop_coedges.extend(other.loop_coedges);
    topology.coedge_topology.extend(other.coedge_topology);
    let mut operand = operand();
    for (ordinal, reference) in operand.recipe_references.iter_mut().enumerate() {
        reference.candidate_faces.push(crate::ids::brep_face_id(
            1100 + i64::try_from(ordinal).unwrap(),
        ));
    }
    assert_eq!(FaceRecipeIndex::new(&topology).resolve_loop(&operand), None);
}

#[test]
fn bounded_face_nodes_keep_grouped_reference_ordinals() {
    let topology = topology();
    let mut operand = operand();
    let mut additional = operand.recipe_references[0].clone();
    additional.design_reference = 999;
    additional.candidate_faces.clear();
    operand.recipe_references.insert(1, additional);
    assert_eq!(FaceRecipeIndex::new(&topology).resolve(&operand), Some(100));
}

#[test]
fn bounded_face_nodes_join_a_packed_token_reference_to_the_preceding_operand() {
    let topology = topology();
    let mut operand = operand();
    let mut packed = operand.recipe_references[0].clone();
    packed.selector_offset = 10;
    packed.token_offset = 14;
    packed.design_reference = 999;
    operand.recipe_references.insert(1, packed);
    assert_eq!(FaceRecipeIndex::new(&topology).resolve(&operand), Some(100));
}

#[test]
fn a_negative_design_reference_adds_no_face_to_its_lane() {
    let topology = topology();
    let mut operand = operand();
    let every_face: Vec<_> = [100, 101, 102, 103]
        .into_iter()
        .map(crate::ids::brep_face_id)
        .collect();
    let mut references = Vec::new();
    for reference in &operand.recipe_references {
        let mut changed = reference.clone();
        changed.design_reference = -5;
        changed.candidate_faces.clone_from(&every_face);
        references.extend([reference.clone(), changed]);
    }
    operand.recipe_references = references;
    assert_eq!(FaceRecipeIndex::new(&topology).resolve(&operand), Some(100));
}

#[test]
fn a_leading_packed_token_reference_resolves_no_face() {
    let topology = topology();
    let mut operand = operand();
    operand.recipe_references[0].token_offset = operand.recipe_references[0].selector_offset + 4;
    assert_eq!(FaceRecipeIndex::new(&topology).resolve(&operand), None);
}

#[test]
fn bounded_face_nodes_require_every_distinct_boundary_edge_and_fan() {
    let topology = topology();
    let index = FaceRecipeIndex::new(&topology);
    let mut operand = operand();
    operand.recipe_nodes[2] = operand.recipe_nodes[0].clone();
    assert_eq!(index.resolve(&operand), None);
    let mut operand = self::operand();
    operand.recipe_nodes[0]
        .recipe_structure
        .as_mut()
        .unwrap()
        .sides[0]
        .scalars[0] = 2;
    assert_eq!(index.resolve(&operand), None);
    let mut operand = self::operand();
    operand.recipe_nodes[0].recipe_structure = None;
    assert_eq!(index.resolve(&operand), None);
    let mut operand = self::operand();
    operand.recipe_program[2] = 4;
    assert_eq!(index.resolve(&operand), None);
}

#[test]
fn bounded_face_nodes_require_unique_topology_rows_and_candidate_faces() {
    let mut topology = topology();
    topology
        .edge_vertices
        .push(topology.edge_vertices[0].clone());
    assert_eq!(FaceRecipeIndex::new(&topology).resolve(&operand()), None);
    let mut topology = self::topology();
    topology.face_loops.push(topology.face_loops[0].clone());
    assert_eq!(FaceRecipeIndex::new(&topology).resolve(&operand()), None);
    let topology = self::topology();
    let mut operand = operand();
    operand.recipe_references[0].candidate_faces.clear();
    assert_eq!(FaceRecipeIndex::new(&topology).resolve(&operand), None);
}

#[test]
fn endpoint_fans_retain_multiplicity_and_cyclic_adjacency() {
    let lanes: Vec<BTreeSet<_>> = (10..14).map(|face| BTreeSet::from([face])).collect();
    assert!(fan_matches(&[0, 1, 2, 3], &[12, 13, 10, 11], &lanes));
    assert!(fan_matches(&[0, 1, 2, 3], &[13, 12, 11, 10], &lanes));
    assert!(!fan_matches(&[0, 1, 2, 3], &[10, 12, 11, 13], &lanes));
    assert!(!fan_matches(&[0, 0, 2, 3], &[10, 11, 12, 13], &lanes));
    assert!(!fan_matches(&[0, 1, 2], &[10, 11, 12, 13], &lanes));
}

#[test]
fn delete_face_selection_retains_native_group_until_every_member_has_boundary_proof() {
    use cadmpeg_ir::features::selections::FaceSelection;
    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:test:scope#10",
        crate::records::feature::scope::DesignFeatureKind::DeleteFace,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.feature_ordinal_offset = draft.paired_byte_offset - 76;
            draft.history_state_id = Some(2);
            draft.previous_history_state_id = Some(1);
            draft.previous_history_state_id_offset = Some(draft.feature_ordinal_offset + 30);
        })
        .unwrap();
    let group: crate::records::topology::construction::DesignConstructionOperandGroup =
        cadmpeg_core::serde_erase::json::from_value(serde_json::json!({
            "id": "f3d:test:group#20", "scope_record_index": 10, "scope_reference_ordinal": 0,
            "record_index": 20, "byte_offset": 0, "class_tag": "338", "role": 0,
            "members": [21], "member_offsets": [0], "frame": {
                "member_count_offset": 0, "opaque_index": 1, "opaque_index_offset": 18,
                "opaque_scalar": 0.0, "opaque_scalar_offset": 22, "variant": false
            }, "role_offset": 0, "paired_class_tag": "261", "paired_byte_offset": 0
        }))
        .unwrap();
    let native = FaceSelection::Native(group.id.clone());
    let mut selection = native.clone();
    let mut operand = operand();
    operand.candidate_faces = vec![crate::ids::brep_face_id(101)];
    operand.preceding_candidate_faces = operand.candidate_faces.clone();
    super::super::selection::bind_face_selection(
        &mut selection,
        &scope,
        std::slice::from_ref(&group),
        std::slice::from_ref(&operand),
        &[101],
    );
    assert_eq!(selection, native);
    operand.resolved_face_slots = FaceRecipeIndex::new(&topology())
        .resolve(&operand)
        .into_iter()
        .collect();
    super::super::selection::bind_face_selection(
        &mut selection,
        &scope,
        std::slice::from_ref(&group),
        std::slice::from_ref(&operand),
        &[101],
    );
    let feature = crate::ids::neutral_feature_id(&scope);
    let expected =
        crate::ids::history_input_face_id(&crate::ids::feature_input_prefix(&feature, 1), 100);
    assert!(
        matches!(selection, FaceSelection::Historical { faces, .. } if faces.as_slice() == [expected])
    );
}

#[test]
fn bounded_face_nodes_select_by_incidence_across_both_pair_lanes() {
    let mut operand = operand();
    operand.recipe_references.swap(0, 1);
    let remap = |value| match value {
        0 => 1,
        1 => 0,
        other => other,
    };
    for node in &mut operand.recipe_nodes {
        let node = node.recipe_structure.as_mut().unwrap();
        node.root = remap(node.root);
        node.prelude[0] = remap(node.prelude[0]);
        for side in &mut node.sides {
            side.header_value = remap(side.header_value);
            for scalar in &mut side.scalars {
                *scalar = remap(*scalar);
            }
        }
    }
    assert_eq!(
        FaceRecipeIndex::new(&topology()).resolve(&operand),
        Some(100)
    );
}

#[test]
fn bounded_face_nodes_bind_two_edge_loops_with_two_shared_vertices() {
    let mut topology = topology();
    topology.faces = vec![100, 101, 102];
    topology.edge_vertices.truncate(3);
    for edge in &mut topology.edge_vertices {
        edge.start_vertex = 1;
        edge.end_vertex = 2;
    }
    topology.face_loops.truncate(3);
    topology.loop_coedges.truncate(3);
    topology.coedge_topology.clear();
    for (ordinal, edges) in [[10, 11], [11, 12], [12, 10]].into_iter().enumerate() {
        let loop_ = &mut topology.loop_coedges[ordinal];
        loop_.member_refs.truncate(2);
        for (index, edge) in edges.into_iter().enumerate() {
            topology.coedge_topology.push(AsmHistoricalCoedge {
                coedge: loop_.member_refs[index],
                owner_loop: loop_.owner_ref,
                edge,
                next: loop_.member_refs[1 - index],
                previous: loop_.member_refs[1 - index],
                radial_next: 0,
            });
        }
    }
    let mut operand = operand();
    operand.recipe_program[2] = 2;
    operand.recipe_nodes.truncate(2);
    for (index, node) in operand.recipe_nodes.iter_mut().enumerate() {
        let node = node.recipe_structure.as_mut().unwrap();
        node.root = 0;
        node.prelude[0] = i32::try_from(index + 1).unwrap();
        for side in &mut node.sides {
            side.header_value = 0;
            side.scalars = vec![1, 2];
        }
    }
    assert_eq!(FaceRecipeIndex::new(&topology).resolve(&operand), Some(100));
}

#[test]
fn a_one_clause_node_leaves_the_bounded_face_member_unresolved() {
    let topology = topology();
    let mut operand = operand();
    let structure = operand.recipe_nodes[0].recipe_structure.as_mut().unwrap();
    structure.prelude[1] = 1;
    structure.sides.truncate(1);
    assert_eq!(FaceRecipeIndex::new(&topology).resolve(&operand), None);
}

/// Face 100 owns both coedges of closed edge 20 in a one-edge loop.
#[test]
fn counted_shell_program_binds_a_one_clause_closed_edge_loop() {
    let mut topology = topology_with_inner_loop();
    topology.faces.retain(|face| *face != 104);
    let other = topology.face_loops.pop().unwrap();
    topology.face_loops[0].member_refs.extend(other.member_refs);
    let index = FaceRecipeIndex::new(&topology);
    let mut operand = operand();
    add_closed_edge_node(&mut operand, [4, 0]);
    operand.recipe_references[4].candidate_faces = vec![crate::ids::brep_face_id(100)];
    let node = operand.recipe_nodes.pop().unwrap();
    operand.recipe_program = vec![0, -1, 1];
    operand.recipe_program.extend_from_slice(&node.program);
    operand.recipe_nodes = vec![node];
    let recipe = index.prepare(&operand).unwrap();
    assert_eq!(recipe.nodes[0].prelude[1], 1);
    assert_eq!(recipe.candidates, BTreeSet::from([100]));
    assert!(index.node_matches(recipe.nodes[0], 20, &recipe.lanes));
    assert_eq!(
        index
            .resolve_loop_program(
                &operand,
                &cadmpeg_core::decode::WorkBudget::new(super::MAX_FACE_RECIPE_MATCH_WORK),
            )
            .unwrap(),
        Some(100)
    );
    assert!(super::bind_operation_operand(
        &mut operand,
        &crate::records::feature::scope::DesignFeatureKind::Shell,
        &topology,
        &mut super::SnapshotCache::default(),
    )
    .is_none());
    assert_eq!(operand.resolved_face_slots, [100]);
}

#[test]
fn counted_shell_program_binds_aliased_neighbor_faces() {
    let index = FaceRecipeIndex::new(&topology());
    let mut operand = operand();
    let neighbors = vec![crate::ids::brep_face_id(101), crate::ids::brep_face_id(102)];
    operand.recipe_references[1].candidate_faces = neighbors.clone();
    operand.recipe_references[2].candidate_faces = neighbors;
    assert_eq!(index.resolve_loop(&operand), None);
    assert_eq!(
        index
            .resolve_loop_program(
                &operand,
                &cadmpeg_core::decode::WorkBudget::new(super::MAX_FACE_RECIPE_MATCH_WORK),
            )
            .unwrap(),
        Some(100)
    );
}

#[test]
fn counted_shell_program_reports_work_exhaustion() {
    let index = FaceRecipeIndex::new(&topology());
    assert!(index
        .resolve_loop_program(&operand(), &cadmpeg_core::decode::WorkBudget::new(0))
        .is_err());
}

#[test]
fn complete_assignment_reassigns_an_earlier_node() {
    assert!(super::complete_assignment(
        &[vec![10, 11], vec![10]],
        &cadmpeg_core::decode::WorkBudget::new(100),
    )
    .unwrap());
}

#[test]
fn complete_assignment_rejects_a_shared_single_edge() {
    assert!(!super::complete_assignment(
        &[vec![10], vec![10]],
        &cadmpeg_core::decode::WorkBudget::new(100),
    )
    .unwrap());
}

#[test]
fn complete_assignment_reports_work_exhaustion() {
    assert!(
        super::complete_assignment(&[vec![10]], &cadmpeg_core::decode::WorkBudget::new(0),)
            .is_err()
    );
}
