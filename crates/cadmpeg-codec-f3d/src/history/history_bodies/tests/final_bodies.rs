// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::history_records::{AsmDeltaState, AsmEntityVersion, AsmHistoricalTopology};

const HISTORY: &str = "f3d:Breps.BlobParts/BREP.root.smbh:asm-history#1";
const MODEL_BODY: &str = "f3d:brep/root.smbh/brep:entity#1";

/// History `HISTORY` whose state 77 inserts body slot 1, followed by state 79
/// with `later` bodies.
fn history(later: Vec<i64>) -> AsmHistory {
    let state = |state_id: i64, bodies: Vec<i64>| {
        let versions = bodies
            .iter()
            .map(|body| AsmEntityVersion {
                entity_ref: *body,
                record_ref: state_id,
            })
            .collect();
        (
            AsmDeltaState {
                id: format!("f3d:history:asm-delta-state#{state_id}"),
                parent: "history".into(),
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
            Some(AsmHistoricalTopology {
                bodies,
                ..AsmHistoricalTopology::default()
            }),
            versions,
        )
    };
    crate::test_support::history::history(
        HISTORY.into(),
        0,
        None,
        false,
        vec![state(79, later), state(77, vec![1]), state(2, Vec::new())],
    )
}

fn final_body_of(history: &AsmHistory) -> Option<cadmpeg_ir::ids::BodyId> {
    let mut body = cadmpeg_ir::examples::unit_cube()
        .unwrap()
        .geometry
        .bodies
        .remove(0);
    body.id = cadmpeg_ir::ids::BodyId::mint(MODEL_BODY).unwrap();
    let bodies = [body];
    let base = feature("base");
    let want = Want {
        origin: &base,
        producer: Some(&base),
        consumer: None,
        state: 77,
        slot: 1,
        capture_current: false,
        consumed: false,
        identity: None,
    };
    final_body(
        history,
        &crate::history::stored_bodies::ActiveBodies::new(&bodies, None),
        &want,
    )
}

#[test]
fn a_nest_is_an_earlier_state_of_the_model_body_its_slot_keeps() {
    assert_eq!(
        final_body_of(&history(vec![1])).as_ref().map(cadmpeg_ir::ids::BodyId::as_str),
        Some(MODEL_BODY)
    );
}

#[test]
fn a_nest_whose_slot_a_later_transition_deletes_names_no_model_body() {
    assert_eq!(final_body_of(&history(Vec::new())), None);
}
