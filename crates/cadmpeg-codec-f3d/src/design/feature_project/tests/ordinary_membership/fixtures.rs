// SPDX-License-Identifier: Apache-2.0
//! Native feature inputs with complete record frames.

use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::recipes::ConstructionRecipeKind;
use crate::records::topology::body_recipe::DesignOperandGroup;
use crate::records::topology::construction::{DesignConstructionOperandGroup, DesignConstructionOperandRole};
use crate::records::topology::extrude_selection::DesignOperandRole;
use crate::records::topology::face::{DesignFaceOperand, DesignFaceOperandDraft};

pub(super) fn scope(kind: DesignFeatureKind, historical: bool) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty("f3d:Design/BulkStream.dat:scope#12", kind, 12);
    if historical {
        scope.try_edit(|draft| {
            draft.previous_history_state_id = Some(7);
            draft.layout_fixture_tail();
        }).unwrap();
    }
    scope
}

pub(super) fn group(index: u32, ordinal: u32, role: DesignOperandRole) -> DesignConstructionOperandGroup {
    let mut group = super::super::extrude_limits::profile_group(index, ordinal);
    group.operand_role = DesignConstructionOperandRole::Other(role);
    group
}

pub(super) fn face(index: u32, ordinal: u32, group: Option<u32>, slots: Vec<i64>) -> DesignFaceOperand {
    DesignFaceOperand::try_new(DesignFaceOperandDraft {
        id: format!("f3d:Design/BulkStream.dat:face-operand#{index}"),
        scope_record_index: 12,
        scope_reference_ordinal: ordinal,
        group: group.map(|group_record_index| DesignOperandGroup { group_record_index, group_member_ordinal: 0 }),
        record_index: index,
        byte_offset: 1200,
        class_tag: "297".to_owned().try_into().unwrap(),
        paired_byte_offset: 1250,
        paired_class_tag: "259".to_owned().try_into().unwrap(),
        recipe_record_index: index + 3,
        recipe_record_byte_offset: 1300,
        recipe_id: format!("f3d:Design/BulkStream.dat:recipe#{index}"),
        recipe_prefix_offset: 1311,
        recipe_prefix_bytes: Vec::new(),
        recipe_references: Vec::new(),
        recipe_kind: ConstructionRecipeKind::Face,
        recipe_program_offset: 1350,
        recipe_program: vec![0, -1],
        recipe_nodes: Vec::new(),
        candidate_faces: Vec::new(),
        unreferenced_candidate_faces: Vec::new(),
        alternate_selector_candidate_faces: Vec::new(),
        preceding_candidate_faces: Vec::new(),
        changed_candidate_faces: Vec::new(),
        historical_support_contexts: Vec::new(),
        resolved_face_slots: slots,
        resolved_active_face: None,
        next_record_index: index + 4,
        next_byte_offset: 1411,
    }).unwrap()
}

pub(super) fn historical_faces(scope: &DesignParameterScope, slots: &[i64]) -> Vec<cadmpeg_ir::ids::HistoricalFaceId> {
    let feature = crate::ids::neutral_feature_id(scope);
    let prefix = crate::ids::history_input_prefix(&feature.key(), 7);
    slots.iter().map(|slot| crate::ids::history_input_face_id(&prefix, *slot)).collect()
}

pub(super) fn historical_state(scope: &DesignParameterScope) -> cadmpeg_ir::ids::FeatureInputTopologyId {
    crate::ids::feature_input_topology_id(&crate::ids::neutral_feature_id(scope), 7)
}

pub(super) fn selection(index: u32, ordinal: u32, slot: i64) -> crate::records::topology::entity_selection::DesignEntitySelectionOperand {
    use crate::records::topology::entity_selection::{DesignEntitySelectionOperand, DesignEntitySelectionOperandDraft};
    DesignEntitySelectionOperand::try_new(DesignEntitySelectionOperandDraft {
        id: format!("f3d:Design/BulkStream.dat:entity-selection#{index}"),
        scope_record_index: 12,
        group_record_index: 100,
        group_member_ordinal: ordinal,
        record_index: index,
        byte_offset: 0,
        class_tag: "277".to_owned().try_into().unwrap(),
        asset_id: "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d".to_owned().try_into().unwrap(),
        asset_id_offset: 0,
        context_id: "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e".to_owned().try_into().unwrap(),
        context_id_offset: 0,
        identity_record_index: index + 3,
        identity_record_offset: 0,
        primary_identity: 225,
        primary_identity_offset: 21,
        secondary: None,
        historical_edge_candidates: Vec::new(),
        historical_face_candidates: Vec::new(),
        resolved_edge_slot: Some(slot),
        next_record_index: index + 4,
        next_byte_offset: 29,
    }).unwrap()
}
