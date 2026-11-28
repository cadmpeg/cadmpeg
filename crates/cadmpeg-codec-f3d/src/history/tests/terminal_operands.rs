// SPDX-License-Identifier: Apache-2.0
//! Terminal edges named by counted-name recipe operands.

use crate::history::edge_operand_history::{operand_terminal_faces, primary_side_shared_edge};
use crate::records::dimensions::DesignRecipeReference;
use crate::records::topology::edge_recipe::{DesignEdgeRecipeStructure, DesignTopologyRecipeSide};
use cadmpeg_ir::ids::FaceId;

fn face(name: &str) -> FaceId {
    FaceId::mint(format!("test:model:face#{name}").as_str()).expect("identity grammar")
}

fn reference(
    operand_offset: u64,
    candidate_faces: Vec<FaceId>,
    alternate_selector_faces: Vec<FaceId>,
) -> DesignRecipeReference {
    DesignRecipeReference {
        selector: 1,
        selector_offset: 0,
        token: "1".into(),
        token_integer: 0,
        token_offset: 0,
        design_reference: 1,
        design_reference_offset: 0,
        candidate_faces,
        candidate_edges: Vec::new(),
        alternate_selector_faces,
        alternate_selector_edges: Vec::new(),
        operand_offset: Some(operand_offset),
    }
}

fn side(header_value: i32, scalars: [i32; 2]) -> DesignTopologyRecipeSide {
    DesignTopologyRecipeSide {
        header_value,
        scalars: scalars.to_vec(),
        payload_prefix: vec![0],
        entries: Vec::new(),
    }
}

/// A primary side that names ordinals 2 and 1.
fn structure() -> DesignEdgeRecipeStructure {
    DesignEdgeRecipeStructure {
        root: 2,
        sides: vec![side(0, [2, 1]), side(0, [1, 3])],
    }
}

#[test]
fn one_operand_joins_the_faces_of_all_its_references() {
    let lane = [
        reference(10, vec![face("a")], vec![face("ignored")]),
        reference(10, Vec::new(), vec![face("b")]),
        reference(10, vec![face("a"), face("c")], Vec::new()),
    ];
    assert_eq!(
        operand_terminal_faces(&lane),
        vec![face("a"), face("b"), face("c")]
    );
}

#[test]
fn primary_operands_share_one_candidate_boundary_edge() {
    let sets = [vec![10, 11, 12], vec![12, 13, 14], vec![99]];
    let edges = |ordinal: usize| sets.get(ordinal).cloned();
    assert_eq!(
        primary_side_shared_edge(Some(&structure()), edges, &[11, 12, 13]),
        Some(12)
    );
    assert_eq!(
        primary_side_shared_edge(Some(&structure()), edges, &[11, 13]),
        None
    );
    let wide = [vec![10, 11, 12], vec![10, 12, 13]];
    assert_eq!(
        primary_side_shared_edge(
            Some(&structure()),
            |ordinal| wide.get(ordinal).cloned(),
            &[10, 11, 12, 13]
        ),
        None
    );
}

#[test]
fn primary_side_requires_two_ordinals_and_a_zero_payload() {
    let sets = [vec![10, 12], vec![12, 13]];
    let edges = |ordinal: usize| sets.get(ordinal).cloned();
    let mut one_ordinal = structure();
    one_ordinal.sides[0] = side(0, [1, 1]);
    assert_eq!(
        primary_side_shared_edge(Some(&one_ordinal), edges, &[12]),
        None
    );
    let mut payload = structure();
    payload.sides[1].payload_prefix = vec![1];
    assert_eq!(primary_side_shared_edge(Some(&payload), edges, &[12]), None);
}
