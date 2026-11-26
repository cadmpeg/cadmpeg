// SPDX-License-Identifier: Apache-2.0
//! The slots whose revision the walk of a history does not fix.

use crate::history_records::{
    AsmBulletinBoard, AsmDeltaState, AsmEntityChange, AsmEntityChangeKind,
};

use super::Ambiguous;

fn state(state_id: i64, changes: Vec<AsmEntityChangeKind>) -> AsmDeltaState {
    AsmDeltaState {
        id: format!("history:state#{state_id}"),
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
        bulletin_boards: vec![AsmBulletinBoard {
            id: format!("history:board#{state_id}"),
            parent: "history".into(),
            byte_offset: 0,
            owner_ref: 0,
            number: 0,
            changes: changes
                .into_iter()
                .map(|kind| AsmEntityChange {
                    id: String::new(),
                    parent: String::new(),
                    byte_offset: 0,
                    kind,
                })
                .collect(),
        }],
        records: Vec::new(),
        transition: None,
    }
}

#[test]
fn a_slot_that_a_later_board_updates_twice_stays_ambiguous_until_an_earlier_board_changes_it() {
    let mut ambiguous = Ambiguous::default();
    ambiguous.undo(&state(
        9,
        vec![
            AsmEntityChangeKind::Update { old: 1, new: 5 },
            AsmEntityChangeKind::Update { old: 2, new: 5 },
            AsmEntityChangeKind::Update { old: 3, new: 6 },
        ],
    ));
    assert_eq!(ambiguous.first(), Some((5, 9)));
    ambiguous.undo(&state(
        8,
        vec![AsmEntityChangeKind::Update { old: 4, new: 6 }],
    ));
    assert_eq!(ambiguous.first(), Some((5, 9)));
    ambiguous.undo(&state(7, vec![AsmEntityChangeKind::Insert { new: 5 }]));
    assert_eq!(ambiguous.first(), None);
}

#[test]
fn a_board_that_updates_each_slot_once_leaves_no_slot_ambiguous() {
    let mut ambiguous = Ambiguous::default();
    ambiguous.undo(&state(
        9,
        vec![
            AsmEntityChangeKind::Update { old: 1, new: 5 },
            AsmEntityChangeKind::Update { old: 3, new: 6 },
            AsmEntityChangeKind::Delete { old: 7 },
        ],
    ));
    assert_eq!(ambiguous.first(), None);
}
