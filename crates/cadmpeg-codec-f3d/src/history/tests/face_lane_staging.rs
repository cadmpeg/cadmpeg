// SPDX-License-Identifier: Apache-2.0
//! Candidate-lane priority and final face-slot storage.

use crate::history::bind_face_operand_history_candidates;
use crate::history_records::AsmHistory;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::recipes::{ConstructionRecipe, ConstructionRecipeKind};
use crate::records::topology::body_recipe::AsmHistoricalEntityKind;
use crate::records::topology::construction::DesignConstructionOperandGroup;
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::face::DesignFaceOperand;
use std::collections::HashMap;

fn fixture(
    kind: DesignFeatureKind,
    recipe_kind: ConstructionRecipeKind,
) -> (
    DesignParameterScope,
    DesignConstructionOperandGroup,
    DesignFaceOperand,
    AsmHistory,
) {
    use crate::history_records::{
        AsmDeltaState, AsmHistoricalTopology, AsmHistoricalTransition, AsmHistory,
    };
    use crate::records::{
        dimensions::DesignRecipeReference,
        feature::scope::DesignParameterScope,
        topology::{
            construction::DesignConstructionOperandGroup,
            construction::DesignConstructionOperandGroupFrame, face::DesignFaceOperand,
        },
    };
    use cadmpeg_ir::ids::FaceId;

    let face = |slot| FaceId::mint(format!("f3d:brep:entity#{slot}")).expect("identity grammar");
    let scope_id = "f3d:Design/BulkStream.dat:scope#42";
    let mut scope = DesignParameterScope::empty(scope_id, kind, 42);
    scope
        .try_edit(|draft| {
            draft.history_state_id = Some(2);
            draft.previous_history_state_id = Some(1);
            draft.layout_fixture_tail();
        })
        .unwrap();

    let group = DesignConstructionOperandGroup::try_from(
        crate::records::topology::construction::DesignConstructionOperandGroupDraft {
            id: "f3d:Design/BulkStream.dat:operand-group#100".into(),
            scope_record_index: 42,
            scope_reference_ordinal: 0,
            record_index: 100,
            byte_offset: 1_000,
            class_tag: crate::records::references::DesignClassTag::try_from("297".to_owned())
                .unwrap(),
            members: vec![crate::records::identity::Located {
                value: 200,
                offset: 1_010,
            }],
            lost_edge_references: Vec::new(),
            frame: DesignConstructionOperandGroupFrame::try_from(
                crate::records::topology::construction::DesignConstructionOperandGroupFrameDraft {
                    member_count_offset: 1_008,
                    auxiliary_records: Vec::new(),
                    auxiliary_paths: Vec::new(),
                    trailing_records: Vec::new(),
                    trailing_transforms: Vec::new(),
                    trailing_dual_transforms: Vec::new(),
                    trailing_flags: Vec::new(),
                    opaque_index: 1,
                    opaque_index_offset: 1_048,
                    opaque_scalar: 0.0,
                    opaque_scalar_offset: 1_052,
                    variant: false,
                },
            )
            .unwrap(),
            operand_role:
                crate::records::topology::construction::DesignConstructionOperandRole::Other(
                    DesignOperandRole::ROLE_0X10,
                ),
            role_offset: 1_030,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "259".to_owned(),
            )
            .unwrap(),
            paired_byte_offset: 1_100,
        },
    )
    .unwrap();
    let reference = |token: &str, design_reference, candidates: &[i64]| DesignRecipeReference {
        selector: 1,
        selector_offset: 1_411,
        token: token.into(),
        token_offset: 1_415,
        design_reference,
        design_reference_offset: 1_420,
        candidate_faces: candidates.iter().copied().map(face).collect(),
        candidate_edges: Vec::new(),
        alternate_selector_faces: Vec::new(),
        alternate_selector_edges: Vec::new(),
    };
    let operand =
        DesignFaceOperand::try_new(crate::records::topology::face::DesignFaceOperandDraft {
            id: "f3d:Design/BulkStream.dat:design-face-operand#200".into(),
            scope_record_index: 42,
            scope_reference_ordinal: 1,
            group: Some(crate::records::topology::body_recipe::DesignOperandGroup {
                group_record_index: 100,
                group_member_ordinal: 0,
            }),
            record_index: 200,
            byte_offset: 1_200,
            class_tag: crate::records::references::DesignClassTag::try_from("297".to_owned())
                .unwrap(),
            paired_byte_offset: 1_300,
            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "259".to_owned(),
            )
            .unwrap(),
            recipe_record_index: 203,
            recipe_record_byte_offset: 1_400,
            recipe_id: "f3d:Design/BulkStream.dat:construction-recipe#203".into(),
            recipe_prefix_offset: 1_411,
            recipe_prefix_bytes: Vec::new(),
            recipe_references: vec![reference("3", 203, &[7, 8]), reference("-1", 199, &[9, 10])],
            recipe_kind,
            recipe_program_offset: 1_430,
            recipe_program: vec![0, -1, 2],

            recipe_nodes: Vec::new(),
            candidate_faces: [7, 8, 9, 10].into_iter().map(face).collect(),
            unreferenced_candidate_faces: [9, 10].into_iter().map(face).collect(),
            alternate_selector_candidate_faces: Vec::new(),
            preceding_candidate_faces: Vec::new(),
            changed_candidate_faces: Vec::new(),
            historical_support_contexts: Vec::new(),
            resolved_face_slots: Vec::new(),
            resolved_active_face: None,
            next_record_index: 204,
            next_byte_offset: 1_500,
        })
        .unwrap();

    let mut transition = AsmHistoricalTransition {
        previous_state_id: Some(1),
        records: Default::default(),
        topology: Default::default(),
    };
    transition.topology.faces.updated.push(7);
    let state = |state_id, topology, transition| AsmDeltaState {
        id: format!("f3d:history:state#{state_id}"),
        parent: "f3d:history".into(),
        byte_offset: 0,
        state_id,
        version_flag: 1,
        state_flag: 0,
        previous_ref: None,
        next_ref: (state_id == 2).then_some(1),
        node_index: state_id,
        partner_ref: None,
        owner_ref: 0,
        bulletin_boards: Vec::new(),
        records: Vec::new(),
        entity_versions: Vec::new(),
        topology_cache: crate::history_records::AsmTopologyCache::Complete(topology),
        transition,
    };
    let history = AsmHistory {
        id: "f3d:history".into(),
        byte_offset: 0,
        preamble: None,
        record_table_binding_budget_exceeded: false,
        states: vec![
            state(2, AsmHistoricalTopology::default(), Some(transition)),
            state(
                1,
                AsmHistoricalTopology {
                    faces: vec![7, 8, 9, 10],
                    persistent_subentity_tags: [
                        (7, "3", 203),
                        (8, "3", 203),
                        (9, "-1", 199),
                        (10, "-1", 199),
                    ]
                    .into_iter()
                    .map(|(entity_ref, token, design_reference)| {
                        crate::history_records::AsmHistoricalPersistentSubentityTag {
                            entity_kind: AsmHistoricalEntityKind::Face,
                            entity_ref,
                            selector: 17,
                            token: token.into(),
                            design_references: vec![design_reference],
                            ordinal: 0,
                        }
                    })
                    .collect(),
                    ..AsmHistoricalTopology::default()
                },
                None,
            ),
        ],
    };

    (scope, group, operand, history)
}

fn direct_lane(kind: DesignFeatureKind, skipped: &str) {
    let (scope, group, mut operand, history) = fixture(kind, ConstructionRecipeKind::Face);
    let recipe = ConstructionRecipe {
        id: operand.recipe_id.clone(),
        byte_offset: 0,
        kind: ConstructionRecipeKind::Face,
        design: None,
        recipe_index: 0,
        record_index: Some(crate::records::identity::RecordedValue {
            value: 203,
            offset: 0,
        }),
    };
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_work_units = u64::MAX;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let _probe = cadmpeg_core::decode::refusal_probe::RefusalProbe::arm(
            cadmpeg_core::decode::ResourceDimension::WorkUnits,
            skipped,
            None,
        );
        bind_face_operand_history_candidates(
            ctx,
            std::slice::from_mut(&mut operand),
            &[scope],
            &[group],
            &[recipe],
            &[history],
            &HashMap::new(),
        )
    })
    .unwrap();
    assert_eq!(operand.resolved_face_slots, [7, 8]);
}

#[test]
fn direct_face_lane_skips_thread_candidates() {
    direct_lane(DesignFeatureKind::Thread, "copy F3D thread face candidates");
}

#[test]
fn direct_face_lane_skips_nested_split_group_lookup() {
    direct_lane(DesignFeatureKind::SplitFace, "find F3D history groups");
}

#[test]
fn direct_face_lane_skips_legacy_group_lookup() {
    direct_lane(DesignFeatureKind::Extrude, "find F3D history groups");
}

#[test]
fn thread_face_lane_allocates_final_slots_once() {
    let bind = |ctx: &cadmpeg_core::decode::DecodeContext<'_>| {
        let (scope, group, mut operand, history) = fixture(
            DesignFeatureKind::Thread,
            ConstructionRecipeKind::BoundedFace,
        );
        bind_face_operand_history_candidates(
            ctx,
            std::slice::from_mut(&mut operand),
            &[scope],
            &[group],
            &[],
            &[history],
            &HashMap::new(),
        )?;
        assert_eq!(operand.resolved_face_slots, [7]);
        Ok::<_, cadmpeg_core::CodecError>(())
    };
    let error = crate::test_support::resource_refusal_at(
        cadmpeg_core::decode::ResourceDimension::CollectionItems,
        "collect F3D resolved face slots",
        0,
        bind,
    );
    let cadmpeg_core::CodecError::ResourceLimit(limit) = error else {
        panic!("slot refusal");
    };
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_collection_items = limit.used.checked_add(limit.additional).unwrap();
    crate::test_support::with_decode_policy(&policy, bind).unwrap();
}
