// SPDX-License-Identifier: Apache-2.0

use super::direct_body_fixture;
use crate::history::binding_histories::BindingHistories;
use crate::history::body_recipe_operands::bind_body_recipe_operand_history_candidates;
use crate::history_records::{
    AsmDeltaState, AsmHistoricalCarrierBinding, AsmHistoricalRelation, AsmHistoricalTopology,
    AsmHistory,
};

/// A state of the `BREP.input` history with the given topology.
fn recipe_state(
    state_id: i64,
    topology: AsmHistoricalTopology,
    next_ref: Option<i64>,
) -> crate::test_support::history::State {
    (
        AsmDeltaState {
            id: format!("f3d:Breps.BlobParts/BREP.input:asm-delta-state#{state_id}"),
            parent: "history".into(),
            byte_offset: 0,
            state_id,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref,
            node_index: state_id,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            transition: None,
        },
        Some(topology),
        Vec::new(),
    )
}

fn body_recipe_history_fixture() -> (
    crate::records::feature::scope::DesignParameterScope,
    AsmHistory,
    Vec<crate::records::topology::body_recipe::DesignBodyRecipeOperand>,
    cadmpeg_ir::ids::FaceId,
) {
    use cadmpeg_ir::ids::FaceId;

    let mut scope = crate::records::feature::scope::DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        crate::records::feature::scope::DesignFeatureKind::Extrude,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
        })
        .unwrap();
    let candidate = FaceId::mint("f3d:brep:entity#10").expect("identity grammar");
    let operands = vec![
        crate::records::topology::body_recipe::DesignBodyRecipeOperand::try_new(
            crate::records::topology::body_recipe::DesignBodyRecipeOperandDraft {
                id: "f3d:Design/BulkStream.dat:design-body-recipe-operand#21".into(),
                scope_record_index: 10,
                owner: crate::records::topology::body_recipe::DesignOperandOwner::Group {
                    group_record_index: 20,
                    group_member_ordinal: 0,
                },
                record_index: 21,
                byte_offset: 0,
                class_tag: crate::records::references::DesignClassTag::try_from("365".to_owned())
                    .unwrap(),
                asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned(),
                )
                .unwrap(),
                asset_id_offset: 56,
                context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                    "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned(),
                )
                .unwrap(),
                context_id_offset: 132,
                selector_tail: None,

                references: vec![
                    crate::records::topology::body_recipe::DesignBodyRecipeReference {
                        design_reference: 301,
                        design_reference_offset: 25,
                        form: 33,
                        form_offset: 33,
                        candidate_faces: vec![candidate.clone()],
                        preceding_candidate_faces: Vec::new(),
                        preceding_body_slots: Vec::new(),
                    },
                ],
                nested_record_index: 24,
                nested_record_index_offset: 38,
                recipe_id: "recipe".into(),
                resolved_face_slot: None,
                resolved_body_state_id: None,
                resolved_body_slot: None,
                resolved_body_face_slots: Vec::new(),
                next_record_index: 25,
                next_byte_offset: 256,
            },
        )
        .unwrap(),
    ];
    let relation = |owner_ref, member_refs| AsmHistoricalRelation {
        owner_ref,
        member_refs,
    };
    let topology = AsmHistoricalTopology {
        bodies: vec![1, 4],
        regions: vec![2, 5],
        shells: vec![3, 6],
        faces: vec![10, 11, 12, 20],
        surfaces: vec![100, 101, 102, 200],
        body_regions: vec![relation(1, vec![2]), relation(4, vec![5])],
        region_shells: vec![relation(2, vec![3]), relation(5, vec![6])],
        shell_faces: vec![relation(3, vec![10, 11, 12]), relation(6, vec![20])],
        shell_wire_edges: vec![relation(3, Vec::new()), relation(6, Vec::new())],
        shell_free_vertices: vec![relation(3, Vec::new()), relation(6, Vec::new())],
        face_loops: vec![
            relation(10, Vec::new()),
            relation(11, Vec::new()),
            relation(12, Vec::new()),
            relation(20, Vec::new()),
        ],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 101,
            },
            AsmHistoricalCarrierBinding {
                entity: 12,
                carrier: 102,
            },
            AsmHistoricalCarrierBinding {
                entity: 20,
                carrier: 200,
            },
        ],
        ..AsmHistoricalTopology::default()
    };
    let state = recipe_state;
    let previous = state(1, topology.clone(), None);
    let current = state(2, topology, Some(1));
    let history = crate::test_support::history::history(
        "history".into(),
        0,
        None,
        false,
        vec![current, previous],
    );

    (scope, history, operands, candidate)
}

#[test]
fn body_recipe_history_resolves_the_complete_input_body_boundary() {
    let (scope, history, mut operands, candidate) = body_recipe_history_fixture();
    bind_body_recipe_operand_history_candidates(
        &mut operands,
        &[],
        std::slice::from_ref(&scope),
        BindingHistories::Shared(std::slice::from_ref(&history)),
    )
    .unwrap();

    assert_eq!(
        operands[0].references()[0].preceding_candidate_faces,
        [candidate]
    );
    assert_eq!(operands[0].references()[0].preceding_body_slots, [1]);
    assert_eq!(operands[0].resolved_face_slot, Some(10));
    assert_eq!(operands[0].resolved_body_state_id, Some(1));
    assert_eq!(operands[0].resolved_body_slot, Some(1));
    assert_eq!(operands[0].resolved_body_face_slots, [10, 11, 12]);
}

#[test]
fn complete_body_boundary_rejects_incomplete_or_ambiguous_incidence() {
    let relation = |owner_ref, member_refs| AsmHistoricalRelation {
        owner_ref,
        member_refs,
    };
    let topology = AsmHistoricalTopology {
        bodies: vec![1],
        regions: vec![2],
        shells: vec![3],
        faces: vec![10, 11],
        body_regions: vec![relation(1, vec![2])],
        region_shells: vec![relation(2, vec![3])],
        shell_faces: vec![relation(3, vec![10, 11])],
        ..AsmHistoricalTopology::default()
    };
    assert_eq!(
        crate::history::body_index::CompleteBodyIndex::new(&topology).faces(1),
        Some(vec![10, 11])
    );

    let mut incomplete = topology.clone();
    incomplete.shell_faces[0].member_refs.clear();
    assert_eq!(
        crate::history::body_index::CompleteBodyIndex::new(&incomplete).faces(1),
        None
    );

    let mut ambiguous = topology;
    ambiguous.shell_faces.push(relation(4, vec![10]));
    assert_eq!(
        crate::history::body_index::CompleteBodyIndex::new(&ambiguous).faces(1),
        None
    );
}

/// The fixture operand with `candidate` as its one candidate face, bound
/// against a preceding state that holds `bodies` and none of the operand's
/// faces.
fn sole_body_case(candidate: &str, bodies: Vec<i64>) -> Option<i64> {
    let (scope, _, mut operands, _) = body_recipe_history_fixture();
    for reference in operands[0].reference_bindings_mut() {
        *reference.candidate_faces =
            vec![cadmpeg_ir::ids::FaceId::mint(candidate).expect("identity grammar")];
    }
    let topology = AsmHistoricalTopology {
        bodies,
        ..AsmHistoricalTopology::default()
    };
    let history = crate::test_support::history::history(
        "history".into(),
        0,
        None,
        false,
        vec![
            recipe_state(2, topology.clone(), Some(1)),
            recipe_state(1, topology, None),
        ],
    );
    bind_body_recipe_operand_history_candidates(
        &mut operands,
        &[],
        std::slice::from_ref(&scope),
        BindingHistories::Shared(std::slice::from_ref(&history)),
    )
    .unwrap();
    operands[0].resolved_body_slot
}

#[test]
fn a_body_operand_of_the_same_brep_selects_the_only_body_of_its_state() {
    assert_eq!(sole_body_case("f3d:brep:entity#99", vec![7]), Some(7));
    assert_eq!(
        sole_body_case("f3d:brep/input/brep:entity#99", vec![7]),
        Some(7)
    );
}

#[test]
fn a_body_operand_keeps_an_open_slot_in_a_state_of_two_bodies_or_of_another_brep() {
    assert_eq!(sole_body_case("f3d:brep:entity#99", vec![7, 8]), None);
    assert_eq!(
        sole_body_case("f3d:brep/other/brep:entity#99", vec![7]),
        None
    );
}

/// The fixture operand bound against a preceding state of bodies 7 and 8,
/// whose last persistent design links are `links`. Each face clause names
/// face 70 of body 7 and face 80 of body 8, or face 99 outside the state
/// when `in_state` is false. The operand's body recipe stores design id
/// `design_id` and `selector`.
fn design_linked_case(
    design_id: &str,
    selector: u32,
    links: &[(i64, i64)],
    in_state: bool,
) -> Option<i64> {
    design_linked_clauses_case(design_id, selector, links, &[in_state])
}

/// `design_linked_case` for an operand with one face clause for each entry
/// of `clauses_in_state`, in order.
fn design_linked_clauses_case(
    design_id: &str,
    selector: u32,
    links: &[(i64, i64)],
    clauses_in_state: &[bool],
) -> Option<i64> {
    let clauses = clauses_in_state
        .iter()
        .map(|&in_state| clause_faces(in_state))
        .collect::<Vec<_>>();
    design_linked_topology_case(design_id, selector, links, &clauses, |_| {})
}

/// Face 70 of body 7 and face 80 of body 8, or face 99 outside the state
/// when `in_state` is false.
fn clause_faces(in_state: bool) -> &'static [&'static str] {
    if in_state {
        &["f3d:brep:entity#70", "f3d:brep:entity#80"]
    } else {
        &["f3d:brep:entity#99"]
    }
}

/// The design-linked fixture for an operand with one face clause for each
/// entry of `clauses`, which lists that clause's candidate faces, with `edit`
/// applied to the preceding-state topology.
fn design_linked_topology_case(
    design_id: &str,
    selector: u32,
    links: &[(i64, i64)],
    clauses: &[&[&str]],
    edit: impl FnOnce(&mut AsmHistoricalTopology),
) -> Option<i64> {
    const CLAUSE_BYTES: u64 = 12;
    let relation = |owner_ref, member_refs| AsmHistoricalRelation {
        owner_ref,
        member_refs,
    };
    let (scope, _, mut operands, _) = body_recipe_history_fixture();
    let mut draft = operands.remove(0).into_draft();
    let first = draft.references[0].clone();
    draft.references = clauses
        .iter()
        .zip(0u64..)
        .map(|(&faces, ordinal)| {
            let mut reference = first.clone();
            reference.design_reference_offset += ordinal * CLAUSE_BYTES;
            reference.form_offset += ordinal * CLAUSE_BYTES;
            reference.candidate_faces = faces
                .iter()
                .map(|face| cadmpeg_ir::ids::FaceId::mint(*face).expect("identity grammar"))
                .collect();
            reference
        })
        .collect();
    let added = (u64::try_from(clauses.len()).unwrap() - 1) * CLAUSE_BYTES;
    draft.nested_record_index_offset += added;
    draft.asset_id_offset += added;
    draft.context_id_offset += added;
    operands.insert(
        0,
        crate::records::topology::body_recipe::DesignBodyRecipeOperand::try_new(draft).unwrap(),
    );
    let topology = AsmHistoricalTopology {
        bodies: vec![7, 8],
        regions: vec![71, 81],
        shells: vec![72, 82],
        faces: vec![70, 80],
        surfaces: vec![170, 180],
        body_regions: vec![relation(7, vec![71]), relation(8, vec![81])],
        region_shells: vec![relation(71, vec![72]), relation(81, vec![82])],
        shell_faces: vec![relation(72, vec![70]), relation(82, vec![80])],
        shell_wire_edges: vec![relation(72, Vec::new()), relation(82, Vec::new())],
        shell_free_vertices: vec![relation(72, Vec::new()), relation(82, Vec::new())],
        face_loops: vec![relation(70, Vec::new()), relation(80, Vec::new())],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 70,
                carrier: 170,
            },
            AsmHistoricalCarrierBinding {
                entity: 80,
                carrier: 180,
            },
        ],
        body_design_links: links
            .iter()
            .map(
                |&(body, design_reference)| crate::history_records::AsmHistoricalBodyDesignLink {
                    body,
                    design_id: "308".into(),
                    design_reference,
                },
            )
            .collect(),
        ..AsmHistoricalTopology::default()
    };
    let mut preceding = topology.clone();
    edit(&mut preceding);
    let history = crate::test_support::history::history(
        "history".into(),
        0,
        None,
        false,
        vec![
            recipe_state(2, topology, Some(1)),
            recipe_state(1, preceding, None),
        ],
    );
    let recipe = crate::records::recipes::ConstructionRecipe {
        id: operands[0].recipe_id.clone(),
        byte_offset: 0,
        kind: crate::records::recipes::ConstructionRecipeKind::Body,
        design: Some(crate::records::recipes::ConstructionRecipeDesign {
            id: crate::records::identity::RecordedValue {
                value: design_id.into(),
                offset: 0,
            },
            selector: Some(crate::records::recipes::ConstructionRecipeSelector {
                value: selector,
                byte_offset: 0,
            }),
        }),
        recipe_index: 0,
        record_index: None,
    };
    bind_body_recipe_operand_history_candidates(
        &mut operands,
        std::slice::from_ref(&recipe),
        std::slice::from_ref(&scope),
        BindingHistories::Shared(std::slice::from_ref(&history)),
    )
    .unwrap();
    operands[0].resolved_body_slot
}

#[test]
fn a_body_recipe_selects_the_input_body_whose_last_design_link_it_stores() {
    let links = [(7, 5), (8, 6)];
    assert_eq!(design_linked_case("308", 6, &links, true), Some(8));
    assert_eq!(design_linked_case("308", 5, &links, true), Some(7));
}

#[test]
fn body_recipes_resolve_design_link_307_8_before_body_deletion() {
    use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
    use crate::records::recipes::{
        ConstructionRecipe, ConstructionRecipeDesign, ConstructionRecipeKind,
        ConstructionRecipeSelector,
    };
    use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};

    let (fixture_scope, mut history, operands, _) = body_recipe_history_fixture();
    let (_, _, _, _, mut body, _, _, _) = direct_body_fixture();
    body.id = cadmpeg_ir::ids::BodyId::mint("f3d:brep:body#4").unwrap();
    let attribute = SourceAttribute {
        id: "f3d:brep:attribute#1".try_into().unwrap(),
        target: AttributeTarget::Body(body.id.clone()).into(),
        name: "ATTRIB_CUSTOM-attrib".into(),
        values: vec![
            Some(AttributeValue::String("generic_tag_attrib_def".into())),
            Some(AttributeValue::Integer(3)),
            Some(AttributeValue::Integer(3)),
            Some(AttributeValue::Integer(-1)),
            Some(AttributeValue::String("generic_tag_attrib_def ".into())),
            Some(AttributeValue::Integer(1)),
            Some(AttributeValue::Integer(3)),
            Some(AttributeValue::String("307".into())),
            Some(AttributeValue::Integer(8)),
            Some(AttributeValue::Integer(0)),
            Some(AttributeValue::Integer(0)),
        ],
        native_ref: None,
    };
    let brep = cadmpeg_asm::brep::AsmBrep {
        bodies: vec![body],
        attributes: vec![attribute],
        ..Default::default()
    };
    let tagged = crate::history::topology_rows::historical_topology(&brep)
        .unwrap()
        .unwrap();
    crate::test_support::history::edit(&mut history, 1, |topology, _| {
        topology.body_design_links = tagged.body_design_links;
    });
    crate::test_support::history::edit(&mut history, 0, |topology, _| {
        topology.bodies.retain(|body| *body != 4);
        topology.regions.retain(|region| *region != 5);
        topology.shells.retain(|shell| *shell != 6);
        topology.faces.retain(|face| *face != 20);
        topology.surfaces.retain(|surface| *surface != 200);
        topology.body_regions.retain(|row| row.owner_ref != 4);
        topology.region_shells.retain(|row| row.owner_ref != 5);
        topology.shell_faces.retain(|row| row.owner_ref != 6);
        topology.shell_wire_edges.retain(|row| row.owner_ref != 6);
        topology
            .shell_free_vertices
            .retain(|row| row.owner_ref != 6);
        topology.face_loops.retain(|row| row.owner_ref != 20);
        topology.face_surfaces.retain(|row| row.entity != 20);
    });
    let current = &history.states()[0];
    assert_eq!(history.topology(current).unwrap().bodies, [1]);
    assert_eq!(history.topology(current).unwrap().faces, [10, 11, 12]);
    assert_eq!(
        current.transition.as_ref().unwrap().topology.bodies.deleted,
        [4]
    );
    let recipe = ConstructionRecipe {
        id: operands[0].recipe_id.clone(),
        byte_offset: 0,
        kind: ConstructionRecipeKind::Body,
        design: Some(ConstructionRecipeDesign {
            id: crate::records::identity::RecordedValue {
                value: "307".into(),
                offset: 0,
            },
            selector: Some(ConstructionRecipeSelector {
                value: 8,
                byte_offset: 0,
            }),
        }),
        recipe_index: 0,
        record_index: None,
    };
    for kind in [
        DesignFeatureKind::Mirror,
        DesignFeatureKind::RemoveBody,
        DesignFeatureKind::Extrude,
    ] {
        let mut scope = DesignParameterScope::empty(&fixture_scope.id, kind, 10);
        scope
            .try_edit(|draft| draft.history_state_id = Some(2))
            .unwrap();
        let mut operands = operands.clone();
        for reference in operands[0].reference_bindings_mut() {
            *reference.candidate_faces = [10, 20]
                .map(|slot| {
                    cadmpeg_ir::ids::FaceId::mint(format!("f3d:brep:entity#{slot}")).unwrap()
                })
                .to_vec();
        }
        bind_body_recipe_operand_history_candidates(
            &mut operands,
            std::slice::from_ref(&recipe),
            std::slice::from_ref(&scope),
            BindingHistories::Shared(std::slice::from_ref(&history)),
        )
        .unwrap();
        assert_eq!(operands[0].references()[0].preceding_body_slots, [1, 4]);
        assert_eq!(operands[0].resolved_body_state_id, Some(1));
        assert_eq!(operands[0].resolved_body_slot, Some(4));
        assert_eq!(operands[0].resolved_body_face_slots, [20]);
    }
}

#[test]
fn a_body_recipe_design_link_shared_by_two_bodies_or_by_none_selects_no_body() {
    assert_eq!(design_linked_case("308", 6, &[(7, 6), (8, 6)], true), None);
    assert_eq!(design_linked_case("308", 7, &[(7, 5), (8, 6)], true), None);
    assert_eq!(design_linked_case("301", 6, &[(7, 5), (8, 6)], true), None);
}

#[test]
fn a_body_recipe_whose_face_clauses_partly_reach_the_state_ignores_its_design_link() {
    let links = [(7, 5), (8, 6)];
    assert_eq!(
        design_linked_clauses_case("308", 6, &links, &[true, true]),
        Some(8)
    );
    assert_eq!(
        design_linked_clauses_case("308", 6, &links, &[false, false]),
        Some(8)
    );
    assert_eq!(
        design_linked_clauses_case("308", 6, &links, &[true, false]),
        None
    );
    assert_eq!(
        design_linked_clauses_case("308", 6, &links, &[false, true]),
        None
    );
}

#[test]
fn a_body_recipe_ignores_its_design_link_when_the_state_has_an_unreadable_body_closure() {
    let links = [(7, 5), (8, 6)];
    for clauses in [
        [clause_faces(true), clause_faces(false)],
        [clause_faces(false), clause_faces(false)],
    ] {
        assert_eq!(
            design_linked_topology_case("308", 6, &links, &clauses, |topology| {
                topology.body_regions.retain(|row| row.owner_ref != 7);
            }),
            None
        );
    }
}

#[test]
fn a_body_recipe_whose_faces_were_renumbered_selects_its_design_linked_body() {
    let links = [(7, 5), (8, 6)];
    assert_eq!(design_linked_case("308", 6, &links, false), Some(8));
    assert_eq!(design_linked_case("308", 5, &links, false), Some(7));
    assert_eq!(design_linked_case("308", 6, &[(7, 6), (8, 6)], false), None);
}

#[test]
fn a_body_recipe_whose_face_clauses_have_no_candidate_faces_ignores_its_design_link() {
    let links = [(7, 5), (8, 6)];
    assert_eq!(
        design_linked_topology_case("308", 6, &links, &[&[], &[]], |_| {}),
        None
    );
}

#[test]
fn a_body_recipe_whose_faces_are_in_no_body_of_the_state_ignores_its_design_link() {
    let links = [(7, 5), (8, 6)];
    assert_eq!(
        design_linked_topology_case("308", 6, &links, &[&["f3d:brep:entity#90"]], |topology| {
            topology.faces.push(90);
        }),
        None
    );
}
