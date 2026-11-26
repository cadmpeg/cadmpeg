// SPDX-License-Identifier: Apache-2.0
//! The slots whose revision the walk of a history does not fix, and the
//! budget of the stored states of one decode.

use std::collections::BTreeSet;

use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_core::decode::DecodePolicy;
use cadmpeg_core::CodecError;
use cadmpeg_ir::units::Tolerances;

use crate::history_records::{
    AsmBulletinBoard, AsmDeltaState, AsmEntityChange, AsmEntityChangeKind,
};

use super::{Ambiguous, Blob, Budget, Read, Want, MAX_STATE_WORK};

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

fn blob(bytes: &[u8]) -> Blob<'_> {
    Blob {
        bytes,
        stream: "history",
        width: RefWidth::Eight,
        namespace: None,
    }
}

/// The read of the head of the history `bytes`, charged to `budget`.
fn read_head(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    budget: &Budget,
) -> Result<Result<Read, String>, CodecError> {
    super::read(
        ctx,
        &blob(bytes),
        &BTreeSet::new(),
        &BTreeSet::new(),
        &BTreeSet::new(),
        &Tolerances::default(),
        budget,
    )
}

#[test]
fn a_table_past_the_retained_share_of_the_stored_states_is_not_read_and_the_decode_goes_on() {
    let bytes = crate::history::tests::budgets::geometry_history(true);
    let head_cost = crate::test_support::with_decode_policy(&DecodePolicy::desktop(), |ctx| {
        let before = ctx.retained_bytes_available();
        let budget = Budget::new(ctx, MAX_STATE_WORK);
        assert!(read_head(ctx, &bytes, &budget).expect("no ceiling").is_ok());
        before - ctx.retained_bytes_available()
    });
    assert!(head_cost > 1);
    let mut policy = DecodePolicy::desktop();
    policy.limits.max_retained_bytes = 2 * head_cost - 1;
    crate::test_support::with_decode_policy(&policy, |ctx| {
        let budget = Budget::new(ctx, MAX_STATE_WORK);
        assert!(read_head(ctx, &bytes, &budget).expect("no ceiling").is_ok());
        let second = read_head(ctx, &bytes, &budget).expect("no ceiling");
        assert_eq!(
            second.err().as_deref(),
            Some("its head is not read (the retained-byte allowance of the stored states is reached)")
        );
        assert_eq!(ctx.resource_refusal(), None);
    });
}

#[test]
fn a_want_of_the_head_state_that_keeps_consumed_bodies_is_read() {
    let bytes = crate::history::tests::budgets::geometry_history(true);
    let blob = blob(&bytes);
    let head = *super::state_ids(&blob)
        .expect("state headers")
        .keys()
        .next_back()
        .expect("head state");
    let want = Want {
        state: head,
        consumed: BTreeSet::from([1]),
    };
    crate::test_support::with_decode_context(|ctx| {
        let budget = Budget::new(ctx, MAX_STATE_WORK);
        let read = super::read(
            ctx,
            &blob,
            &BTreeSet::new(),
            &BTreeSet::new(),
            &BTreeSet::from([want.clone()]),
            &Tolerances::default(),
            &budget,
        )
        .expect("no ceiling")
        .expect("history read");
        assert_eq!(read.head, head);
        assert!(matches!(read.states.as_slice(), [(found, Ok(_))] if *found == want));
    });
}
