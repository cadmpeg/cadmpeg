// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use super::operand_brep_history_state_pair;
use crate::history_records::{AsmDeltaState, AsmHistoricalTopology, AsmHistory};
use crate::records::topology::body_recipe::{
    DesignBodyRecipeOperand, DesignBodyRecipeOperandDraft, DesignBodyRecipeReference,
    DesignOperandOwner,
};
use crate::test_support::history::State;
use cadmpeg_ir::ids::FaceId;

const STREAM: &str = "f3d:Design/BulkStream.dat";

fn state(history: &str, state_id: i64, previous_state_id: Option<i64>) -> State {
    (
        AsmDeltaState {
            id: format!("{history}:asm-delta-state#{state_id}"),
            parent: history.into(),
            byte_offset: 0,
            state_id,
            version_flag: 1,
            state_flag: 0,
            previous_ref: None,
            next_ref: previous_state_id,
            node_index: state_id,
            partner_ref: None,
            owner_ref: 0,
            bulletin_boards: Vec::new(),
            records: Vec::new(),
            transition: None,
        },
        Some(AsmHistoricalTopology::default()),
        Vec::new(),
    )
}

/// A history of the BREP blob `source` that stores the state pair `2 -> 9`.
fn history(source: &str) -> AsmHistory {
    let id = format!("f3d:asset/Breps.BlobParts/BREP.{source}.smbh:asm-history#1");
    crate::test_support::history::history(
        id.clone(),
        0,
        None,
        false,
        vec![state(&id, 9, Some(2)), state(&id, 2, None)],
    )
}

fn operand(faces: &[&str]) -> DesignBodyRecipeOperand {
    DesignBodyRecipeOperand::try_new(DesignBodyRecipeOperandDraft {
        id: format!("{STREAM}:design-body-recipe-operand#120"),
        scope_record_index: 100,
        owner: DesignOperandOwner::ScopeReference {
            scope_reference_ordinal: 0,
        },
        record_index: 120,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("300".to_owned()).unwrap(),
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
        references: vec![DesignBodyRecipeReference {
            design_reference: 1,
            design_reference_offset: 25,
            form: 4,
            form_offset: 33,
            candidate_faces: faces
                .iter()
                .map(|face| FaceId::mint(*face).unwrap())
                .collect(),
            preceding_candidate_faces: Vec::new(),
            preceding_body_slots: Vec::new(),
        }],
        nested_record_index: 123,
        nested_record_index_offset: 38,
        recipe_id: format!("{STREAM}:construction-recipe#1"),
        resolved_face_slot: None,
        resolved_body_state_id: None,
        resolved_body_slot: None,
        resolved_body_face_slots: Vec::new(),
        next_record_index: 124,
        next_byte_offset: 256,
    })
    .unwrap()
}

#[test]
fn a_shared_state_pair_binds_to_the_history_whose_brep_holds_the_operand_faces() {
    let histories = [history("first"), history("second")];
    let select = |faces: &[&str]| {
        let (history, state, previous) =
            operand_brep_history_state_pair(&operand(faces), &histories, 9, 2)?;
        Some((history.id.as_str(), state.state_id, previous.state_id))
    };
    assert_eq!(
        select(&[
            "f3d:brep/second.smbh/brep:entity#1",
            "f3d:brep/second.smbh/brep:entity#4",
        ]),
        Some((histories[1].id.as_str(), 9, 2))
    );
    assert_eq!(
        select(&[
            "f3d:brep/second.smbh/brep:entity#1",
            "f3d:brep/first.smbh/brep:entity#4",
        ]),
        None
    );
    assert_eq!(select(&["f3d:brep:entity#1"]), None);
    assert_eq!(select(&[]), None);
}
