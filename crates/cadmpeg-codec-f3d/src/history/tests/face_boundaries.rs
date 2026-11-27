// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use crate::history::face_resolution::{
    cyclic_point_subsequence, resolve_direct_face_recipe_clauses,
};
use crate::history::selection::bind_edge_identity_bounded_face_rules;
use crate::history_records::AsmHistoricalTopology;

#[test]
fn direct_face_recipe_clauses_resolve_ordered_changed_intersections() {
    use cadmpeg_ir::ids::FaceId;

    let reference =
        |selector_offset, candidates: &[i64]| crate::records::dimensions::DesignRecipeReference {
            selector: 1,
            selector_offset,
            token: "x".into(),
            token_integer: 0,
            token_offset: selector_offset + 1,
            design_reference: 1,
            design_reference_offset: selector_offset + 2,
            candidate_faces: candidates
                .iter()
                .map(|face| {
                    FaceId::mint(format!("f3d:brep:entity#{face}")).expect("identity grammar")
                })
                .collect(),
            candidate_edges: Vec::new(),
            alternate_selector_faces: Vec::new(),
            alternate_selector_edges: Vec::new(),
            operand_offset: None,
        };
    let references = [
        reference(10, &[1, 2]),
        reference(10, &[2, 3]),
        reference(20, &[4, 5]),
        reference(20, &[4, 6]),
        reference(30, &[2]),
    ];
    let topology = AsmHistoricalTopology {
        faces: vec![1, 2, 3, 4, 5, 6],
        ..AsmHistoricalTopology::default()
    };

    assert_eq!(
        resolve_direct_face_recipe_clauses(&references, &topology, &[2, 4].into_iter().collect()),
        [2, 4]
    );
    assert!(
        resolve_direct_face_recipe_clauses(&references, &topology, &[2].into_iter().collect())
            .is_empty()
    );
}

#[test]
fn bounded_face_copy_matches_cyclic_boundary_with_split_vertices() {
    use cadmpeg_ir::math::Point3;

    let point = |x, y| Point3 { x, y, z: 0.0 };
    let source = [
        point(0.0, 0.0),
        point(2.0, 0.0),
        point(2.0, 2.0),
        point(0.0, 2.0),
    ];
    let split_copy = [
        point(2.0, 2.0),
        point(1.0, 2.0),
        point(0.0, 2.0),
        point(0.0, 0.0),
        point(2.0, 0.0),
    ];
    assert!(cyclic_point_subsequence(&source, &split_copy));

    let reversed = split_copy.iter().copied().rev().collect::<Vec<_>>();
    assert!(cyclic_point_subsequence(&source, &reversed));

    let wrong_order = [
        point(0.0, 0.0),
        point(2.0, 2.0),
        point(2.0, 0.0),
        point(0.0, 2.0),
    ];
    assert!(!cyclic_point_subsequence(&source, &wrong_order));
    assert!(!cyclic_point_subsequence(&source, &split_copy[..3]));
}

fn bounded_face_rule_fixture() -> (
    Vec<crate::records::topology::edge_identity::DesignEdgeIdentityOperand>,
    crate::records::topology::face::DesignFaceOperand,
) {
    use crate::records::{
        recipes::ConstructionRecipeKind,
        topology::{
            edge_identity::DesignEdgeIdentityOperand, face::DesignFaceOperand,
            historical_context::DesignHistoricalFaceBoundaryContext,
            historical_context::DesignHistoricalFaceLoopContext,
            historical_context::DesignHistoricalFaceSupportContext,
        },
    };

    let identities = vec![DesignEdgeIdentityOperand::try_new(
        crate::records::topology::edge_identity::DesignEdgeIdentityOperandDraft {
            id: "f3d:Design/BulkStream.dat:edge-identity#10".into(),
            scope_record_index: 1,
            group_record_index: 2,
            group_member_ordinal: 0,
            record_index: 10,
            byte_offset: 100,
            class_tag: crate::records::references::DesignClassTag::try_from("297".to_owned())
                .unwrap(),
            layout: crate::records::topology::edge_identity::DesignEdgeIdentityLayout::Full,
            local_id: 13,
            asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
            )
            .unwrap(),
            asset_id_offset: 142,
            context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
            )
            .unwrap(),
            context_id_offset: 218,
            historical: None,
            treatment_radius_candidates: Vec::new(),
            transition_edge_candidates: vec![7, 8, 9],
            resolved_edge_slots: Vec::new(),
            resolved_edge_slot: None,
            resolution_identity_id: None,
        },
    )
    .unwrap()];
    let face = DesignFaceOperand::try_new(crate::records::topology::face::DesignFaceOperandDraft {
        id: "f3d:Design/BulkStream.dat:design-face-operand#10".into(),
        scope_record_index: 1,
        scope_reference_ordinal: 0,
        group: Some(crate::records::topology::body_recipe::DesignOperandGroup {
            group_record_index: 2,
            group_member_ordinal: 0,
        }),
        record_index: 10,
        byte_offset: 100,
        class_tag: crate::records::references::DesignClassTag::try_from("297".to_owned()).unwrap(),
        paired_byte_offset: 200,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned())
            .unwrap(),
        recipe_record_index: 13,
        recipe_record_byte_offset: 300,
        recipe_id: "recipe".into(),
        recipe_prefix_offset: 311,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_kind: ConstructionRecipeKind::BoundedFace,
        recipe_program_offset: 0,
        recipe_program: vec![0],

        recipe_nodes: Vec::new(),
        candidate_faces: Vec::new(),
        unreferenced_candidate_faces: Vec::new(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: vec![DesignHistoricalFaceSupportContext {
            active_face_slot: 30,
            surface_slot: 40,
            preceding_face_slots: vec![50],
            preceding_face_boundaries: vec![DesignHistoricalFaceBoundaryContext {
                face_slot: 50,
                loops: vec![DesignHistoricalFaceLoopContext {
                    loop_slot: 60,
                    boundary: crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Coedges(
                        vec![
                            crate::records::topology::historical_context::DesignHistoricalLoopCoedge {
                                coedge_slot: 70,
                                edge_slot: 8,
                            },
                            crate::records::topology::historical_context::DesignHistoricalLoopCoedge {
                                coedge_slot: 71,
                                edge_slot: 6,
                            },
                            crate::records::topology::historical_context::DesignHistoricalLoopCoedge {
                                coedge_slot: 72,
                                edge_slot: 7,
                            },
                        ],
                    ),
                }],
            }],
            changed_preceding_face_slots: vec![50],
            preceding_surface: None,
        }],
        resolved_face_slots: Vec::new(),
        resolved_active_face: None,
        next_record_index: 14,
        next_byte_offset: 400,
    })
    .unwrap();
    (identities, face)
}

#[test]
fn bounded_face_identity_selects_ordered_deleted_treatment_edges() {
    let (mut identities, face) = bounded_face_rule_fixture();
    bind_edge_identity_bounded_face_rules(&mut identities, &[face.clone()]);
    assert_eq!(identities[0].resolved_edge_slots, [8, 7]);
    assert_eq!(
        identities[0].resolution_identity_id.as_deref(),
        Some(face.id.as_str())
    );

    let mut inconsistent = face;
    inconsistent.historical_support_contexts[0]
        .changed_preceding_face_slots
        .clear();
    bind_edge_identity_bounded_face_rules(&mut identities, &[inconsistent]);
    assert!(identities[0].resolved_edge_slots.is_empty());
}

fn face_context(
    active_face_slot: i64,
    face_slot: i64,
    loops: &[&[i64]],
) -> crate::records::topology::historical_context::DesignHistoricalFaceSupportContext {
    use crate::records::topology::historical_context::{
        DesignHistoricalFaceBoundaryContext, DesignHistoricalFaceLoopContext,
        DesignHistoricalFaceSupportContext, DesignHistoricalLoopBoundary,
        DesignHistoricalLoopCoedge,
    };

    let mut coedge_slot = face_slot * 100;
    let loops = loops
        .iter()
        .zip(0..)
        .map(|(edges, ordinal)| DesignHistoricalFaceLoopContext {
            loop_slot: face_slot * 10 + ordinal,
            boundary: DesignHistoricalLoopBoundary::Coedges(
                edges
                    .iter()
                    .map(|&edge_slot| {
                        coedge_slot += 1;
                        DesignHistoricalLoopCoedge {
                            coedge_slot,
                            edge_slot,
                        }
                    })
                    .collect(),
            ),
        })
        .collect();
    DesignHistoricalFaceSupportContext {
        active_face_slot,
        surface_slot: face_slot + 1000,
        preceding_face_slots: vec![face_slot],
        preceding_face_boundaries: vec![DesignHistoricalFaceBoundaryContext { face_slot, loops }],
        changed_preceding_face_slots: vec![face_slot],
        preceding_surface: None,
    }
}

/// One bounded-face member at `ordinal` of group 2 whose first recipe
/// reference names the preceding face of `named` and whose unreferenced
/// support face is the preceding face of `support`.
fn named_face_member(
    ordinal: u32,
    transition: &[i64],
    named: crate::records::topology::historical_context::DesignHistoricalFaceSupportContext,
    support: crate::records::topology::historical_context::DesignHistoricalFaceSupportContext,
) -> (
    crate::records::topology::edge_identity::DesignEdgeIdentityOperand,
    crate::records::topology::face::DesignFaceOperand,
) {
    let (identities, face) = bounded_face_rule_fixture();
    let record_index = 10 + 10 * ordinal;
    let mut identity = identities.into_iter().next().unwrap().into_draft();
    identity.id = format!("f3d:Design/BulkStream.dat:edge-identity#{record_index}");
    identity.group_member_ordinal = ordinal;
    identity.record_index = record_index;
    identity.byte_offset = 100 * u64::from(record_index);
    identity.asset_id_offset = identity.byte_offset + 42;
    identity.context_id_offset = identity.byte_offset + 118;
    identity.local_id = u64::from(record_index) + 3;
    identity.transition_edge_candidates = transition.to_vec();
    let identity =
        crate::records::topology::edge_identity::DesignEdgeIdentityOperand::try_new(identity)
            .unwrap();

    let mut draft = face.into_draft();
    let byte_offset = 100 * u64::from(record_index);
    draft.id = format!("f3d:Design/BulkStream.dat:design-face-operand#{record_index}");
    draft.group = Some(crate::records::topology::body_recipe::DesignOperandGroup {
        group_record_index: 2,
        group_member_ordinal: ordinal,
    });
    draft.record_index = record_index;
    draft.byte_offset = byte_offset;
    draft.paired_byte_offset = byte_offset + 100;
    draft.recipe_record_index = record_index + 3;
    draft.recipe_record_byte_offset = byte_offset + 200;
    draft.recipe_prefix_offset = byte_offset + 211;
    draft.next_record_index = record_index + 4;
    draft.next_byte_offset = byte_offset + 300;
    draft.recipe_references = vec![crate::records::dimensions::DesignRecipeReference {
        selector: 1,
        selector_offset: byte_offset + 211,
        token: "x".into(),
        token_integer: 0,
        token_offset: byte_offset + 215,
        design_reference: i64::from(record_index) + 3,
        design_reference_offset: byte_offset + 216,
        candidate_faces: vec![cadmpeg_ir::ids::FaceId::mint(format!(
            "f3d:brep:entity#{}",
            named.active_face_slot
        ))
        .unwrap()],
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
        operand_offset: None,
    }];
    draft.historical_support_contexts = vec![support];
    let mut face = crate::records::topology::face::DesignFaceOperand::try_new(draft).unwrap();
    face.first_reference_support_contexts = vec![named];
    (identity, face)
}

/// A box whose top face 50 has edges 1-4, whose +X face 51 has edges 2, 5,
/// 6, 7, whose bottom face 52 has edges 6, 8, 9, 10, and whose -X face 53
/// has edges 4, 11, 12, 13.
const TOP: &[i64] = &[1, 2, 3, 4];
const PLUS_X: &[i64] = &[2, 5, 6, 7];
const BOTTOM: &[i64] = &[6, 8, 9, 10];
const MINUS_X: &[i64] = &[4, 11, 12, 13];

#[test]
fn bounded_face_identities_select_every_edge_of_two_adjacent_named_faces() {
    let transition = [1, 2, 3, 4, 5, 6, 7];
    let (top_identity, top_face) = named_face_member(
        0,
        &transition,
        face_context(30, 50, &[TOP]),
        face_context(32, 52, &[BOTTOM]),
    );
    let (side_identity, side_face) = named_face_member(
        1,
        &transition,
        face_context(31, 51, &[PLUS_X]),
        face_context(33, 53, &[MINUS_X]),
    );
    let mut identities = vec![top_identity, side_identity];
    bind_edge_identity_bounded_face_rules(&mut identities, &[top_face.clone(), side_face.clone()]);
    assert_eq!(identities[0].resolved_edge_slots, TOP);
    assert_eq!(identities[1].resolved_edge_slots, [2, 5, 6, 7]);
    assert_eq!(
        identities[0].resolution_identity_id.as_deref(),
        Some(top_face.id.as_str())
    );
    let mut union = identities
        .iter()
        .flat_map(|identity| identity.resolved_edge_slots.iter().copied())
        .collect::<Vec<_>>();
    union.sort_unstable();
    union.dedup();
    assert_eq!(union, transition);
}

#[test]
fn bounded_face_identity_keeps_an_edge_that_an_edge_group_also_selects() {
    let transition = [1, 2, 3, 4];
    let (identity, face) = named_face_member(
        0,
        &transition,
        face_context(30, 50, &[TOP]),
        face_context(32, 52, &[BOTTOM]),
    );
    let mut identities = vec![identity];
    bind_edge_identity_bounded_face_rules(&mut identities, &[face]);
    assert_eq!(identities[0].resolved_edge_slots, TOP);
}

#[test]
fn bounded_face_identity_selects_the_inner_loop_edges_of_the_named_face() {
    let inner = [20, 21, 22, 23];
    let transition = [1, 2, 3, 4, 20, 21, 22, 23];
    let (identity, face) = named_face_member(
        0,
        &transition,
        face_context(30, 50, &[TOP, &inner]),
        face_context(32, 52, &[BOTTOM]),
    );
    let mut identities = vec![identity];
    bind_edge_identity_bounded_face_rules(&mut identities, &[face]);
    assert_eq!(identities[0].resolved_edge_slots, transition);
}

#[test]
fn bounded_face_identity_requires_a_complete_named_face_boundary() {
    let transition = [1, 2, 3, 4, 6];
    let (identity, mut face) = named_face_member(
        0,
        &transition,
        face_context(30, 50, &[TOP, &[]]),
        face_context(32, 52, &[BOTTOM]),
    );
    let mut identities = vec![identity];
    bind_edge_identity_bounded_face_rules(&mut identities, &[face.clone()]);
    assert!(identities[0].resolved_edge_slots.is_empty());

    face.first_reference_support_contexts = vec![
        face_context(30, 50, &[TOP]),
        face_context(31, 51, &[PLUS_X]),
    ];
    bind_edge_identity_bounded_face_rules(&mut identities, &[face]);
    assert_eq!(identities[0].resolved_edge_slots, [6]);
}

#[test]
fn bounded_face_chamfer_identity_drops_the_tangent_loop_edges_of_the_named_face() {
    let transition = [1, 2, 3, 4];
    let (identity, mut face) = named_face_member(
        0,
        &transition,
        face_context(30, 50, &[TOP]),
        face_context(32, 52, &[BOTTOM]),
    );
    face.tangent_boundary_edge_slots = vec![1, 3];
    let mut identities = vec![identity];
    let losses = bind_edge_identity_bounded_face_rules(&mut identities, &[face]);
    assert_eq!(identities[0].resolved_edge_slots, [2, 4]);
    assert!(losses.is_empty());
}

#[test]
fn bounded_face_chamfer_identity_keeps_every_sharp_loop_edge_of_the_named_face() {
    let transition = [1, 2, 3, 4];
    let (identity, face) = named_face_member(
        0,
        &transition,
        face_context(30, 50, &[TOP]),
        face_context(32, 52, &[BOTTOM]),
    );
    let mut identities = vec![identity];
    let losses = bind_edge_identity_bounded_face_rules(&mut identities, &[face]);
    assert_eq!(identities[0].resolved_edge_slots, TOP);
    assert!(losses.is_empty());
}

#[test]
fn bounded_face_chamfer_identity_keeps_an_undetermined_loop_edge_with_a_loss() {
    let transition = [1, 2, 3, 4];
    let (identity, mut face) = named_face_member(
        0,
        &transition,
        face_context(30, 50, &[TOP]),
        face_context(32, 52, &[BOTTOM]),
    );
    face.tangent_boundary_edge_slots = vec![1];
    face.undetermined_continuity_edge_slots = vec![3, 9];
    let mut identities = vec![identity];
    let losses = bind_edge_identity_bounded_face_rules(&mut identities, &[face]);
    assert_eq!(identities[0].resolved_edge_slots, [2, 3, 4]);
    assert_eq!(
        losses
            .iter()
            .map(|loss| loss.code.clone())
            .collect::<Vec<_>>(),
        [crate::loss::F3dLossCode::HistoricalEdgeContinuityAssumedSharp.kind()]
    );
}
